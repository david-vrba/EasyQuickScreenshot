// PNG encoding + atomic file writes.
// The quick-shot temp file is written to <name>.tmp then renamed, so a reader
// (script, AI agent, watcher) can never see a half-written image.

use std::io::BufWriter;
use std::path::{Path, PathBuf};

use windows::Win32::System::SystemInformation::GetLocalTime;

pub fn write_png_atomic(path: &Path, bgra: &[u8], width: i32, height: i32) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {}", dir.display(), e))?;
    }
    let tmp = path.with_extension("png.tmp");
    encode_png(&tmp, bgra, width, height)?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename to {}: {}", path.display(), e))
}

fn encode_png(path: &Path, bgra: &[u8], width: i32, height: i32) -> Result<(), String> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }

    let file = std::fs::File::create(path).map_err(|e| format!("create {}: {}", path.display(), e))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())
}

/// Timestamped path in the saved/ folder, e.g. 2026-07-03_14-05-22.png.
/// Appends a counter if several captures land in the same second.
pub fn timestamped_path(saved_dir: &Path) -> PathBuf {
    let t = unsafe { GetLocalTime() };
    let base = format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    let mut path = saved_dir.join(format!("{}.png", base));
    let mut counter = 2;
    while path.exists() {
        path = saved_dir.join(format!("{}_{}.png", base, counter));
        counter += 1;
    }
    path
}
