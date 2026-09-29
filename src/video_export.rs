// Trim, crop and volume for a recording: Media Foundation's source reader decodes it, the
// edits are applied to raw frames and samples, and the MP4 writer encodes the result.
// Runs as `eqs --export-video …`, so the editor in the settings app holds no media code.

use std::path::Path;

use windows::core::{Interface, GUID, HSTRING, PROPVARIANT};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MFAudioFormat_PCM,
    MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio,
    MFMediaType_Video, MFVideoFormat_RGB32, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_ANY_STREAM,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_MEDIASOURCE,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::annotate::Rect;
use crate::mp4_writer::{self, Mp4Writer, AUDIO_CHANNELS, AUDIO_RATE, TICKS_PER_SECOND};

/// What the editor asks for. Times in seconds, the crop in the video's own pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edits {
    pub start: f64,
    pub end: f64,
    pub crop: Option<Rect>,
    /// 0 is muted, 1 is as recorded, 2 is twice as loud.
    pub volume: f32,
}

impl Edits {
    /// Nothing to do: the recording can be copied instead of encoded again.
    pub fn changes_nothing(&self, duration: f64, size: (u32, u32)) -> bool {
        let whole = (0, 0, size.0 as i32, size.1 as i32);
        self.start <= 0.0
            && self.end >= duration - 0.05
            && self.crop.is_none_or(|c| c == whole)
            && (self.volume - 1.0).abs() < 0.005
    }
}

/// A crop H.264 can encode: inside the frame, with even offsets and even sizes (colour is
/// stored at half resolution, so an odd edge would split a colour sample in two).
pub fn even_crop(crop: Rect, (w, h): (u32, u32)) -> Rect {
    let x = crop.0.clamp(0, w as i32 - 2) & !1;
    let y = crop.1.clamp(0, h as i32 - 2) & !1;
    let cw = (crop.2.min(w as i32 - x).max(2)) & !1;
    let ch = (crop.3.min(h as i32 - y).max(2)) & !1;
    (x, y, cw, ch)
}

/// Scale 16-bit samples by `gain`, clipping at full scale instead of wrapping around when
/// it is turned up — wrapping turns a loud moment into a crack.
pub fn apply_gain(pcm: &mut [i16], gain: f32) {
    if (gain - 1.0).abs() < 0.005 {
        return;
    }
    for s in pcm.iter_mut() {
        *s = (*s as f32 * gain).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    }
}

/// Which frames of a chunk of audio fall inside [start, end): (frames to skip at the front,
/// frames to keep after that). Positions are in audio frames from the start of the file.
pub fn keep_frames(chunk_at: u64, frames: u64, start: u64, end: u64) -> (u64, u64) {
    let skip = start.saturating_sub(chunk_at).min(frames);
    let last = end.saturating_sub(chunk_at).min(frames);
    (skip, last.saturating_sub(skip))
}

/// The recording's length and picture size, for the fast path and the crop bounds.
pub fn probe(input: &Path) -> Result<(f64, (u32, u32)), String> {
    unsafe {
        let reader = open_reader(input)?;
        let video = reader.GetCurrentMediaType(first_video()).map_err(|e| e.to_string())?;
        Ok((duration(&reader)?, frame_size(&video)?))
    }
}

pub fn export(input: &Path, output: &Path, edits: Edits) -> Result<(), String> {
    let (length, size) = probe(input)?;
    if edits.changes_nothing(length, size) {
        return std::fs::copy(input, output).map(|_| ()).map_err(|e| format!("copying the recording: {e}"));
    }
    unsafe { encode(input, output, edits, length, size) }
}

fn first_video() -> u32 {
    windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32
}

fn first_audio() -> u32 {
    windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32
}

unsafe fn open_reader(input: &Path) -> Result<IMFSourceReader, String> {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    mp4_writer::start_media_foundation();
    let mut attributes: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attributes, 1).map_err(|e| e.to_string())?;
    let attributes = attributes.ok_or("no attributes")?;
    // Lets the reader convert H.264 into plain RGB frames, which is what cropping needs.
    attributes
        .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
        .map_err(|e| e.to_string())?;
    MFCreateSourceReaderFromURL(&HSTRING::from(input.as_os_str()), &attributes)
        .map_err(|e| format!("opening {}: {e}", input.display()))
}

unsafe fn duration(reader: &IMFSourceReader) -> Result<f64, String> {
    let value = reader
        .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
        .map_err(|e| e.to_string())?;
    let ticks = u64::try_from(&value).map_err(|e| e.to_string())?;
    Ok(ticks as f64 / TICKS_PER_SECOND as f64)
}

unsafe fn frame_size(video: &IMFMediaType) -> Result<(u32, u32), String> {
    let packed = video.GetUINT64(&MF_MT_FRAME_SIZE).map_err(|e| e.to_string())?;
    Ok(((packed >> 32) as u32, packed as u32))
}

/// Which actual stream index each first stream is, since samples arrive tagged by index.
unsafe fn stream_index(reader: &IMFSourceReader, major: &GUID) -> Option<u32> {
    (0..8).find(|&i| {
        reader
            .GetCurrentMediaType(i)
            .and_then(|t| t.GetGUID(&MF_MT_MAJOR_TYPE))
            .is_ok_and(|g| g == *major)
    })
}

unsafe fn encode(input: &Path, output: &Path, edits: Edits, length: f64, size: (u32, u32)) -> Result<(), String> {
    let reader = open_reader(input)?;
    let fail = |e: windows::core::Error| e.to_string();

    reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).map_err(fail)?;
    reader.SetStreamSelection(first_video(), true).map_err(fail)?;
    let rgb = MFCreateMediaType().map_err(fail)?;
    rgb.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(fail)?;
    rgb.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).map_err(fail)?;
    reader.SetCurrentMediaType(first_video(), None, &rgb).map_err(fail)?;
    let video_type = reader.GetCurrentMediaType(first_video()).map_err(fail)?;
    let fps = frame_rate(&video_type).unwrap_or(30);

    // Muted means no sound track at all rather than a track of silence.
    let with_audio = edits.volume > 0.0 && select_pcm_audio(&reader).is_ok();

    let crop = even_crop(edits.crop.unwrap_or((0, 0, size.0 as i32, size.1 as i32)), size);
    let writer = Mp4Writer::create(output, crop.2 as u32, crop.3 as u32, fps, with_audio)?;
    let video_index = stream_index(&reader, &MFMediaType_Video);
    let audio_index = if with_audio { stream_index(&reader, &MFMediaType_Audio) } else { None };

    let start = (edits.start.max(0.0) * TICKS_PER_SECOND as f64) as i64;
    let end = (edits.end.min(length) * TICKS_PER_SECOND as f64) as i64;
    if start > 0 {
        reader
            .SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from(start))
            .map_err(fail)?;
    }
    let stride = default_stride(&video_type, size.0);
    let frame_ticks = TICKS_PER_SECOND / fps as i64;
    let (start_frame, end_frame) = (to_audio_frames(start), to_audio_frames(end));
    let (mut video_done, mut audio_done) = (video_index.is_none(), audio_index.is_none());
    let mut last_frame = -1i64;
    let mut cropped = vec![0u8; crop.2 as usize * crop.3 as usize * 4];

    while !(video_done && audio_done) {
        let (mut index, mut flags, mut at) = (0u32, 0u32, 0i64);
        let mut sample: Option<IMFSample> = None;
        reader
            .ReadSample(
                MF_SOURCE_READER_ANY_STREAM.0 as u32,
                0,
                Some(&mut index),
                Some(&mut flags),
                Some(&mut at),
                Some(&mut sample),
            )
            .map_err(fail)?;
        let ended = flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0;
        if Some(index) == video_index {
            if ended || at >= end {
                video_done = true;
            } else if let Some(sample) = sample {
                // The seek lands on the keyframe before `start`; frames before it are dropped.
                let frame = ((at - start) as f64 / frame_ticks as f64).round() as i64;
                if at + frame_ticks / 2 >= start && frame > last_frame {
                    crop_frame(&sample, stride, size, crop, &mut cropped)?;
                    writer.write_frame(&cropped, crop.2 as usize * 4, frame)?;
                    last_frame = frame;
                }
            }
        } else if Some(index) == audio_index {
            if ended || at >= end {
                audio_done = true;
            } else if let Some(sample) = sample {
                let mut pcm = read_pcm(&sample)?;
                let frames = (pcm.len() / AUDIO_CHANNELS as usize) as u64;
                let chunk_at = to_audio_frames(at);
                let (skip, keep) = keep_frames(chunk_at, frames, start_frame, end_frame);
                if keep > 0 {
                    let ch = AUDIO_CHANNELS as usize;
                    let part = &mut pcm[skip as usize * ch..(skip + keep) as usize * ch];
                    apply_gain(part, edits.volume);
                    writer.write_audio(part, chunk_at + skip - start_frame)?;
                }
            }
        } else if ended {
            break;
        }
    }
    writer.finish()
}

fn to_audio_frames(ticks: i64) -> u64 {
    (ticks.max(0) as i128 * AUDIO_RATE as i128 / TICKS_PER_SECOND as i128) as u64
}

unsafe fn frame_rate(video: &IMFMediaType) -> Option<u32> {
    let packed = video.GetUINT64(&MF_MT_FRAME_RATE).ok()?;
    let (num, den) = ((packed >> 32) as u32, packed as u32);
    (den > 0).then(|| ((num as f64 / den as f64).round() as u32).clamp(1, 120))
}

/// Ask the reader for 48 kHz stereo 16-bit, resampling if the file is anything else.
unsafe fn select_pcm_audio(reader: &IMFSourceReader) -> windows::core::Result<()> {
    reader.SetStreamSelection(first_audio(), true)?;
    let pcm = MFCreateMediaType()?;
    pcm.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
    pcm.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
    pcm.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
    pcm.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE)?;
    pcm.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS)?;
    reader.SetCurrentMediaType(first_audio(), None, &pcm)
}

/// Bytes from one row to the next in the decoded frames. Negative means bottom-up rows,
/// which is Media Foundation's default for RGB when the type does not say otherwise.
unsafe fn default_stride(video: &IMFMediaType, width: u32) -> i32 {
    video
        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
        .map(|s| s as i32)
        .unwrap_or(-(width as i32 * 4))
}

/// Copy the crop out of a decoded frame as top-down rows.
unsafe fn crop_frame(sample: &IMFSample, stride: i32, size: (u32, u32), crop: Rect, out: &mut [u8]) -> Result<(), String> {
    let buffer = sample.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
    let row = crop.2 as usize * 4;
    // A 2D buffer knows its own pitch, which beats trusting the media type.
    if let Ok(image) = buffer.cast::<IMF2DBuffer>() {
        let (mut top, mut pitch) = (std::ptr::null_mut(), 0i32);
        image.Lock2D(&mut top, &mut pitch).map_err(|e| e.to_string())?;
        copy_rows(top, pitch, crop, row, out);
        let _ = image.Unlock2D();
        return Ok(());
    }
    let mut base: *mut u8 = std::ptr::null_mut();
    buffer.Lock(&mut base, None, None).map_err(|e| e.to_string())?;
    let top = if stride < 0 { base.add((size.1 as usize - 1) * (-stride) as usize) } else { base };
    copy_rows(top, stride, crop, row, out);
    let _ = buffer.Unlock();
    Ok(())
}

unsafe fn copy_rows(top: *const u8, pitch: i32, crop: Rect, row: usize, out: &mut [u8]) {
    for y in 0..crop.3 as isize {
        let src = top.offset((crop.1 as isize + y) * pitch as isize + crop.0 as isize * 4);
        std::ptr::copy_nonoverlapping(src, out.as_mut_ptr().add(y as usize * row), row);
    }
}

unsafe fn read_pcm(sample: &IMFSample) -> Result<Vec<i16>, String> {
    let buffer = sample.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
    let (mut data, mut len) = (std::ptr::null_mut(), 0u32);
    buffer.Lock(&mut data, None, Some(&mut len)).map_err(|e| e.to_string())?;
    let pcm = std::slice::from_raw_parts(data as *const i16, len as usize / 2).to_vec();
    let _ = buffer.Unlock();
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crop_is_moved_inside_the_frame_and_made_even() {
        assert_eq!(even_crop((101, 51, 641, 361), (1280, 720)), (100, 50, 640, 360));
        assert_eq!(even_crop((1200, 700, 400, 400), (1280, 720)), (1200, 700, 80, 20));
        assert_eq!(even_crop((-40, -10, 99999, 99999), (1280, 720)), (0, 0, 1280, 720));
    }

    #[test]
    fn turning_it_up_clips_instead_of_wrapping() {
        let mut pcm = [1000i16, -1000, 30000, -30000];
        apply_gain(&mut pcm, 2.0);
        assert_eq!(pcm, [2000, -2000, i16::MAX, i16::MIN]);
        let mut quiet = [1000i16, -999];
        apply_gain(&mut quiet, 0.5);
        assert_eq!(quiet, [500, -500]);
    }

    #[test]
    fn trimming_keeps_only_the_frames_inside_the_window() {
        assert_eq!(keep_frames(0, 1000, 400, 800), (400, 400), "cut at both ends");
        assert_eq!(keep_frames(500, 1000, 400, 10_000), (0, 1000), "wholly inside");
        assert_eq!(keep_frames(900, 100, 0, 800), (0, 0), "wholly after the end");
    }

    #[test]
    fn untouched_edits_mean_copy_the_file() {
        let none = Edits { start: 0.0, end: 12.0, crop: None, volume: 1.0 };
        assert!(none.changes_nothing(12.0, (1280, 720)));
        let whole = Edits { crop: Some((0, 0, 1280, 720)), ..none };
        assert!(whole.changes_nothing(12.0, (1280, 720)), "a crop of everything is no crop");
        assert!(!Edits { start: 0.5, ..none }.changes_nothing(12.0, (1280, 720)));
        assert!(!Edits { volume: 0.0, ..none }.changes_nothing(12.0, (1280, 720)));
    }
}
