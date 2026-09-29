// System sound for recordings: WASAPI loopback of the default output device, delivered as
// 48 kHz stereo 16-bit chunks stamped with the moment they were heard.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::mp4_writer::{AUDIO_CHANNELS, AUDIO_RATE, TICKS_PER_SECOND};

/// Sound heard `at` audio frames after the recording's zero point.
pub struct Chunk {
    pub at: u64,
    pub pcm: Vec<i16>,
}

/// The performance counter in Media Foundation's 100 ns units. WASAPI stamps each packet in
/// the same units, which is what lets audio and video share one clock.
pub fn now_ticks() -> i64 {
    unsafe {
        let (mut count, mut freq) = (0i64, 0i64);
        let _ = QueryPerformanceCounter(&mut count);
        let _ = QueryPerformanceFrequency(&mut freq);
        (count as i128 * TICKS_PER_SECOND as i128 / freq.max(1) as i128) as i64
    }
}

pub fn ticks_to_frames(ticks: i64) -> u64 {
    (ticks.max(0) as i128 * AUDIO_RATE as i128 / TICKS_PER_SECOND as i128) as u64
}

/// Capture until `stop` is set. `zero` is the recording's start on the `now_ticks` clock;
/// sound from before it is dropped.
pub fn capture(stop: Arc<AtomicBool>, zero: i64, out: Sender<Chunk>) -> Result<(), String> {
    unsafe { capture_inner(stop, zero, out) }.map_err(|e| format!("system sound: {e}"))
}

unsafe fn capture_inner(stop: Arc<AtomicBool>, zero: i64, out: Sender<Chunk>) -> windows::core::Result<()> {
    CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

    let block = (AUDIO_CHANNELS * 2) as u16;
    let format = WAVEFORMATEX {
        wFormatTag: 1, // WAVE_FORMAT_PCM
        nChannels: AUDIO_CHANNELS as u16,
        nSamplesPerSec: AUDIO_RATE,
        nAvgBytesPerSec: AUDIO_RATE * block as u32,
        nBlockAlign: block,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    // AUTOCONVERTPCM makes Windows resample and remix to this format, whatever the device
    // mixes at, so the rest of the recorder only ever sees 48 kHz stereo.
    client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
        2_000_000, // 200 ms of buffer
        0,
        &format,
        None,
    )?;
    let capture: IAudioCaptureClient = client.GetService()?;
    client.Start()?;

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(10));
        while capture.GetNextPacketSize()? > 0 {
            let mut data: *mut u8 = std::ptr::null_mut();
            let (mut frames, mut flags, mut heard) = (0u32, 0u32, 0u64);
            capture.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut heard))?;
            let samples = frames as usize * AUDIO_CHANNELS as usize;
            let pcm = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                vec![0i16; samples]
            } else {
                std::slice::from_raw_parts(data as *const i16, samples).to_vec()
            };
            capture.ReleaseBuffer(frames)?;
            let since = heard as i64 - zero;
            if since >= 0 && out.send(Chunk { at: ticks_to_frames(since), pcm }).is_err() {
                return client.Stop();
            }
        }
    }
    client.Stop()
}

/// Where the audio track has got to, in frames written.
///
/// Loopback delivers nothing while nothing is playing. Left alone, the audio track would
/// stop growing during every quiet stretch and the next sound would be placed too early —
/// out of sync by however long the quiet lasted. So the gaps are filled with silence,
/// driven by the clock, and every chunk is laid at the moment it was actually heard.
#[derive(Default)]
pub struct Timeline {
    pub written: u64,
}

/// How to lay one chunk onto the track: silence to write first, then the chunk minus
/// `skip` frames already covered.
#[derive(Debug, PartialEq)]
pub struct Placement {
    pub silence: u64,
    pub skip: usize,
}

/// Timestamps wobble by a few milliseconds between packets. Inside this window a chunk is
/// simply appended; outside it, it is placed where it belongs.
const JITTER_FRAMES: u64 = AUDIO_RATE as u64 / 100;

impl Timeline {
    pub fn place(&mut self, at: u64, frames: usize) -> Placement {
        let placement = if at > self.written + JITTER_FRAMES {
            Placement { silence: at - self.written, skip: 0 }
        } else if at + JITTER_FRAMES < self.written {
            // Heard during time already filled with silence to keep up: drop the overlap.
            let overlap = (self.written - at).min(frames as u64) as usize;
            Placement { silence: 0, skip: overlap }
        } else {
            Placement { silence: 0, skip: 0 }
        };
        self.written += placement.silence + (frames - placement.skip) as u64;
        placement
    }

    /// Silence to write so the track stays within `lag` frames of `now` while nothing
    /// plays. The lag leaves room for a real packet that is already on its way.
    pub fn catch_up(&mut self, now: u64, lag: u64) -> u64 {
        let gap = now.saturating_sub(lag).saturating_sub(self.written);
        self.written += gap;
        gap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_stretch_becomes_silence_so_the_next_sound_lands_on_time() {
        let mut t = Timeline::default();
        t.place(0, 480);
        let p = t.place(48_000, 480); // a full second of nothing, then a sound
        assert_eq!(p, Placement { silence: 47_520, skip: 0 });
        assert_eq!(t.written, 48_480);
    }

    #[test]
    fn a_few_milliseconds_of_wobble_is_just_appended() {
        let mut t = Timeline::default();
        t.place(0, 480);
        assert_eq!(t.place(480 + 200, 480), Placement { silence: 0, skip: 0 });
        assert_eq!(t.place(960 - 200 + 480, 480), Placement { silence: 0, skip: 0 });
    }

    #[test]
    fn sound_heard_during_filled_silence_drops_only_the_overlap() {
        let mut t = Timeline::default();
        assert_eq!(t.catch_up(48_000, 4_800), 43_200, "quiet: padded to 100 ms behind now");
        let p = t.place(40_000, 4_800); // arrives late, for time already filled
        assert_eq!(p, Placement { silence: 0, skip: 3_200 });
        assert_eq!(t.written, 43_200 + 1_600);
    }

    #[test]
    fn catching_up_never_moves_backwards() {
        let mut t = Timeline::default();
        t.place(0, 96_000);
        assert_eq!(t.catch_up(48_000, 4_800), 0);
    }

    #[test]
    fn clock_ticks_convert_to_audio_frames() {
        assert_eq!(ticks_to_frames(TICKS_PER_SECOND), 48_000);
        assert_eq!(ticks_to_frames(-5), 0);
    }
}
