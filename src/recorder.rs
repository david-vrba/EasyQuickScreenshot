// Screen recording: frames from the GPU, system sound from WASAPI, both into one MP4 on a
// worker thread so the tray app's message loop — and every screenshot — stays responsive.
// The first thing in the core that outlives a hotkey press, which is why it has a thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;

use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject,
    DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL, ICONINFO,
};

use crate::annotate::Rect;
use crate::mp4_writer::{Mp4Writer, AUDIO_CHANNELS, TICKS_PER_SECOND};
use crate::screen_frames::ScreenFrames;
use crate::system_audio::{self, Chunk, Timeline};

pub struct Settings {
    pub fps: u32,
    pub system_sound: bool,
}

pub struct Recording {
    stop: Arc<AtomicBool>,
    worker: JoinHandle<Result<(), String>>,
    pub path: PathBuf,
    /// What is actually being recorded, on the virtual desktop: the selection trimmed to one
    /// monitor and to even sizes.
    pub region: Rect,
}

/// Start recording `region` (virtual-desktop coordinates) into `path`. Returns once the
/// capture is running, or with the reason it could not start.
pub fn start(region: Rect, path: PathBuf, settings: Settings) -> Result<Recording, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = {
        let (stop, path) = (stop.clone(), path.clone());
        std::thread::spawn(move || record(region, &path, settings, stop, ready_tx))
    };
    match ready_rx.recv() {
        Ok(Ok(region)) => Ok(Recording { stop, worker, path, region }),
        Ok(Err(reason)) => {
            let _ = worker.join();
            Err(reason)
        }
        Err(_) => Err(worker.join().ok().and_then(Result::err).unwrap_or_else(|| "the recorder stopped".into())),
    }
}

impl Recording {
    /// Stop, finish the file, and hand back where it is.
    pub fn stop(self) -> Result<PathBuf, String> {
        self.stop.store(true, Ordering::Relaxed);
        match self.worker.join() {
            Ok(Ok(())) => Ok(self.path),
            Ok(Err(reason)) => Err(reason),
            Err(_) => Err("the recorder crashed".into()),
        }
    }
}

fn record(
    region: Rect,
    path: &Path,
    settings: Settings,
    stop: Arc<AtomicBool>,
    ready: mpsc::Sender<Result<Rect, String>>,
) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let setup = ScreenFrames::open(region).and_then(|frames| {
        let writer = Mp4Writer::create(path, frames.width(), frames.height(), settings.fps, settings.system_sound)?;
        let canvas = Canvas::new(frames.width() as i32, frames.height() as i32)?;
        Ok((frames, writer, canvas))
    });
    let (mut frames, writer, mut canvas) = match setup {
        Ok(parts) => parts,
        Err(reason) => {
            let _ = ready.send(Err(reason.clone()));
            return Err(reason);
        }
    };
    // Prime the copy: the first AcquireNextFrame after duplicating returns the current image.
    let _ = frames.wait_for_change(100);

    let zero = system_audio::now_ticks();
    let sound = settings.system_sound.then(|| {
        let (tx, rx) = mpsc::channel();
        let stop = stop.clone();
        let thread = std::thread::spawn(move || {
            // No output device, or a driver that refuses loopback: the video still records,
            // and the audio track is filled with silence by the timeline.
            let _ = system_audio::capture(stop, zero, tx);
        });
        (thread, rx)
    });
    let _ = ready.send(Ok(frames.screen));

    let period = TICKS_PER_SECOND / settings.fps as i64;
    let stride = frames.width() as usize * 4;
    let mut timeline = Timeline::default();
    let mut next = 0i64;
    let mut last_index = -1i64;
    let result = (|| -> Result<(), String> {
        loop {
            let elapsed = system_audio::now_ticks() - zero;
            let due = next * period;
            if elapsed < due {
                let wait_ms = ((due - elapsed) / 10_000).max(1) as u32;
                frames.wait_for_change(wait_ms)?;
                continue;
            }
            // Late frames are skipped rather than queued, so the video keeps real time.
            let index = elapsed / period;
            if index > last_index {
                frames.read_into(canvas.pixels_mut(), stride)?;
                canvas.draw_pointer(frames.screen);
                writer.write_frame(canvas.pixels(), stride, index)?;
                last_index = index;
            }
            next = index + 1;

            if let Some((_, rx)) = &sound {
                write_heard_sound(&writer, &mut timeline, rx)?;
                let now = system_audio::ticks_to_frames(elapsed);
                write_silence(&writer, &mut timeline, now, AUDIO_LAG)?;
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
        if let Some((thread, rx)) = sound {
            let _ = thread.join();
            write_heard_sound(&writer, &mut timeline, &rx)?;
            // The sound track ends where the video does.
            let end = system_audio::ticks_to_frames((last_index + 1) * period);
            write_silence(&writer, &mut timeline, end, 0)?;
        }
        Ok(())
    })();
    let finished = writer.finish();
    result.and(finished)
}

/// How far behind the clock the silence filler stays, so a packet already on its way is not
/// overtaken: 100 ms.
const AUDIO_LAG: u64 = 4_800;

fn write_heard_sound(writer: &Mp4Writer, timeline: &mut Timeline, rx: &Receiver<Chunk>) -> Result<(), String> {
    while let Ok(chunk) = rx.try_recv() {
        let frames = chunk.pcm.len() / AUDIO_CHANNELS as usize;
        let before = timeline.written;
        let placed = timeline.place(chunk.at, frames);
        if placed.silence > 0 {
            writer.write_audio(&silence(placed.silence), before)?;
        }
        let from = placed.skip * AUDIO_CHANNELS as usize;
        writer.write_audio(&chunk.pcm[from..], before + placed.silence)?;
    }
    Ok(())
}

fn write_silence(writer: &Mp4Writer, timeline: &mut Timeline, now: u64, lag: u64) -> Result<(), String> {
    let before = timeline.written;
    let gap = timeline.catch_up(now, lag);
    if gap > 0 {
        writer.write_audio(&silence(gap), before)?;
    }
    Ok(())
}

fn silence(frames: u64) -> Vec<i16> {
    vec![0i16; frames as usize * AUDIO_CHANNELS as usize]
}

/// A DIB the size of the region, so the pointer can be drawn onto each frame with GDI.
/// Desktop Duplication hands over the desktop without the cursor on it.
struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    len: usize,
}

impl Canvas {
    fn new(width: i32, height: i32) -> Result<Canvas, String> {
        unsafe {
            let info = crate::capture::top_down_info(width, height);
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .map_err(|e| format!("frame buffer: {e}"))?;
            let dc = CreateCompatibleDC(None);
            let old = SelectObject(dc, bitmap);
            Ok(Canvas { dc, bitmap, old, bits: bits as *mut u8, len: width as usize * height as usize * 4 })
        }
    }

    fn pixels_mut(&mut self) -> &mut [u8] {
        unsafe {
            // Anything GDI still has queued for this bitmap lands before the bytes change.
            let _ = GdiFlush();
            std::slice::from_raw_parts_mut(self.bits, self.len)
        }
    }

    fn pixels(&self) -> &[u8] {
        unsafe {
            let _ = GdiFlush();
            std::slice::from_raw_parts(self.bits, self.len)
        }
    }

    /// The live pointer, at its place inside `region` (virtual-desktop coordinates).
    fn draw_pointer(&mut self, region: Rect) {
        unsafe {
            let mut info = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
            if GetCursorInfo(&mut info).is_err() || info.flags != CURSOR_SHOWING {
                return;
            }
            let mut icon = ICONINFO::default();
            if GetIconInfo(info.hCursor, &mut icon).is_err() {
                return;
            }
            let POINT { x, y } = info.ptScreenPos;
            let _ = DrawIconEx(
                self.dc,
                x - icon.xHotspot as i32 - region.0,
                y - icon.yHotspot as i32 - region.1,
                info.hCursor,
                0,
                0,
                0,
                None,
                DI_NORMAL,
            );
            // GetIconInfo hands over copies of the cursor's bitmaps; they are ours to free.
            let _ = DeleteObject(icon.hbmMask);
            let _ = DeleteObject(icon.hbmColor);
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}
