// MP4 writer on Media Foundation's sink writer: H.264 video plus optional AAC audio, through
// the hardware encoders when the machine has them. Fed top-down BGRA frames and 16-bit PCM.

use std::path::Path;
use std::sync::Once;

use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaType, IMFSinkWriter, MFAudioFormat_AAC, MFAudioFormat_PCM,
    MFCreateAttributes, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromURL, MFMediaType_Audio, MFMediaType_Video, MFStartup,
    MFVideoFormat_H264, MFVideoFormat_RGB32, MFVideoInterlace_Progressive,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AVG_BITRATE,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MFSTARTUP_FULL, MF_VERSION,
};

/// Audio is always 48 kHz stereo 16-bit: the rate every Windows output device mixes at by
/// default, and one the AAC encoder accepts.
pub const AUDIO_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u32 = 2;

/// Media Foundation time is counted in 100 ns units.
pub const TICKS_PER_SECOND: i64 = 10_000_000;

pub struct Mp4Writer {
    writer: IMFSinkWriter,
    video: u32,
    audio: Option<u32>,
    width: u32,
    height: u32,
    frame_ticks: i64,
}

/// Media Foundation must be started once per process before any of it is used.
pub fn start_media_foundation() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    });
}

/// Two 32-bit values in one attribute, the way Media Foundation stores sizes and ratios.
fn packed(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

/// Enough bits for screen content to stay sharp without the file ballooning: about 0.12
/// bits per pixel per frame, which puts 1080p30 near 7.5 Mbit/s.
pub fn video_bitrate(width: u32, height: u32, fps: u32) -> u32 {
    let bits = width as u64 * height as u64 * fps as u64 * 12 / 100;
    bits.clamp(2_000_000, 40_000_000) as u32
}

impl Mp4Writer {
    /// `width` and `height` must be even: H.264 stores colour at half resolution.
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        with_audio: bool,
    ) -> Result<Mp4Writer, String> {
        start_media_foundation();
        unsafe { Self::create_inner(path, width, height, fps, with_audio) }
            .map_err(|e| format!("video writer: {e}"))
    }

    unsafe fn create_inner(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        with_audio: bool,
    ) -> windows::core::Result<Mp4Writer> {
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 1)?;
        let attributes = attributes.expect("MFCreateAttributes returned nothing");
        // NVENC, Quick Sync or AMF when present; Media Foundation falls back to software.
        attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;

        let url = HSTRING::from(path.as_os_str());
        let writer = MFCreateSinkWriterFromURL(&url, None, &attributes)?;

        let video_out = video_type(&MFVideoFormat_H264, width, height, fps)?;
        video_out.SetUINT32(&MF_MT_AVG_BITRATE, video_bitrate(width, height, fps))?;
        let video = writer.AddStream(&video_out)?;
        let video_in = video_type(&MFVideoFormat_RGB32, width, height, fps)?;
        // Positive stride means top-down rows, which is how the frames arrive. Left unset,
        // Media Foundation assumes bottom-up RGB and the whole video comes out upside down.
        video_in.SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4)?;
        writer.SetInputMediaType(video, &video_in, None)?;

        let audio = if with_audio {
            let audio_out = audio_type(&MFAudioFormat_AAC)?;
            // 24 000 bytes/s = 192 kbit/s, one of the four rates the AAC encoder accepts.
            audio_out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000)?;
            let stream = writer.AddStream(&audio_out)?;
            let audio_in = audio_type(&MFAudioFormat_PCM)?;
            audio_in.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, AUDIO_CHANNELS * 2)?;
            audio_in.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AUDIO_RATE * AUDIO_CHANNELS * 2)?;
            writer.SetInputMediaType(stream, &audio_in, None)?;
            Some(stream)
        } else {
            None
        };

        writer.BeginWriting()?;
        Ok(Mp4Writer {
            writer,
            video,
            audio,
            width,
            height,
            frame_ticks: TICKS_PER_SECOND / fps as i64,
        })
    }

    /// Frame number `index` of the video, from top-down BGRA rows `stride` bytes apart.
    pub fn write_frame(&self, bgra: &[u8], stride: usize, index: i64) -> Result<(), String> {
        let row = self.width as usize * 4;
        let len = row * self.height as usize;
        unsafe {
            let buffer = MFCreateMemoryBuffer(len as u32).map_err(|e| e.to_string())?;
            let mut dst: *mut u8 = std::ptr::null_mut();
            buffer.Lock(&mut dst, None, None).map_err(|e| e.to_string())?;
            for y in 0..self.height as usize {
                std::ptr::copy_nonoverlapping(bgra.as_ptr().add(y * stride), dst.add(y * row), row);
            }
            let _ = buffer.Unlock();
            buffer.SetCurrentLength(len as u32).map_err(|e| e.to_string())?;
            let sample = MFCreateSample().map_err(|e| e.to_string())?;
            sample.AddBuffer(&buffer).map_err(|e| e.to_string())?;
            sample.SetSampleTime(index * self.frame_ticks).map_err(|e| e.to_string())?;
            sample.SetSampleDuration(self.frame_ticks).map_err(|e| e.to_string())?;
            self.writer.WriteSample(self.video, &sample).map_err(|e| e.to_string())
        }
    }

    /// Interleaved stereo PCM starting `at` audio frames into the recording.
    pub fn write_audio(&self, pcm: &[i16], at: u64) -> Result<(), String> {
        let Some(stream) = self.audio else { return Ok(()) };
        if pcm.is_empty() {
            return Ok(());
        }
        let frames = (pcm.len() / AUDIO_CHANNELS as usize) as i64;
        let bytes = pcm.len() * 2;
        unsafe {
            let buffer = MFCreateMemoryBuffer(bytes as u32).map_err(|e| e.to_string())?;
            let mut dst: *mut u8 = std::ptr::null_mut();
            buffer.Lock(&mut dst, None, None).map_err(|e| e.to_string())?;
            std::ptr::copy_nonoverlapping(pcm.as_ptr() as *const u8, dst, bytes);
            let _ = buffer.Unlock();
            buffer.SetCurrentLength(bytes as u32).map_err(|e| e.to_string())?;
            let sample = MFCreateSample().map_err(|e| e.to_string())?;
            sample.AddBuffer(&buffer).map_err(|e| e.to_string())?;
            let rate = AUDIO_RATE as i64;
            sample.SetSampleTime(at as i64 * TICKS_PER_SECOND / rate).map_err(|e| e.to_string())?;
            sample.SetSampleDuration(frames * TICKS_PER_SECOND / rate).map_err(|e| e.to_string())?;
            self.writer.WriteSample(stream, &sample).map_err(|e| e.to_string())
        }
    }

    /// Flush the encoders and close the file. Without this the MP4 has no index and plays
    /// nowhere.
    pub fn finish(self) -> Result<(), String> {
        unsafe { self.writer.Finalize() }.map_err(|e| format!("finishing the video: {e}"))
    }
}

unsafe fn video_type(
    subtype: &windows::core::GUID,
    width: u32,
    height: u32,
    fps: u32,
) -> windows::core::Result<IMFMediaType> {
    let t = MFCreateMediaType()?;
    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
    t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
    t.SetUINT64(&MF_MT_FRAME_SIZE, packed(width, height))?;
    t.SetUINT64(&MF_MT_FRAME_RATE, packed(fps, 1))?;
    t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, packed(1, 1))?;
    Ok(t)
}

unsafe fn audio_type(subtype: &windows::core::GUID) -> windows::core::Result<IMFMediaType> {
    let t = MFCreateMediaType()?;
    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
    t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
    t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
    t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE)?;
    t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS)?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bitrate_follows_the_picture_and_stays_in_bounds() {
        assert_eq!(video_bitrate(1920, 1080, 30), 7_464_960);
        assert_eq!(video_bitrate(64, 64, 30), 2_000_000, "a tiny region still gets a floor");
        assert_eq!(video_bitrate(7680, 4320, 60), 40_000_000, "8K60 is capped");
    }

    #[test]
    fn sizes_pack_high_word_first() {
        assert_eq!(packed(1920, 1080), (1920u64 << 32) | 1080);
    }
}
