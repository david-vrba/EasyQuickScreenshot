// Lists saved screenshots and builds small base64 thumbnails on demand, so the gallery is
// self-contained (no asset-protocol scope tied to a dynamic shots folder). Full-size viewing
// is delegated to the OS default viewer via the opener plugin.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Serialize;

const THUMB_MAX: u32 = 320;

#[derive(Serialize)]
pub struct ShotDto {
    pub name: String,
    pub path: String,
    pub size_kb: u64,
    pub modified: i64, // unix seconds; 0 if unknown
    pub width: u32,
    pub height: u32,
    pub thumb: String, // "data:image/png;base64,..." or empty on failure
}

#[derive(Serialize)]
pub struct GalleryStats {
    pub count: usize,
    pub total_mb: f64,
    pub saved_dir: String,
    pub exists: bool,
}

fn saved_files(saved_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(saved_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false))
        .collect();
    // Newest first
    files.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    files.reverse();
    files
}

pub fn stats(saved_dir: &str) -> GalleryStats {
    let dir = Path::new(saved_dir);
    let files = saved_files(dir);
    let total: u64 = files
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
        .sum();
    GalleryStats {
        count: files.len(),
        total_mb: (total as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0,
        saved_dir: saved_dir.to_string(),
        exists: dir.is_dir(),
    }
}

pub fn list(saved_dir: &str) -> Vec<ShotDto> {
    saved_files(Path::new(saved_dir))
        .into_iter()
        .map(|p| describe(&p))
        .collect()
}

fn describe(path: &Path) -> ShotDto {
    let meta = std::fs::metadata(path).ok();
    let modified = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let size_kb = meta.as_ref().map(|m| m.len() / 1024).unwrap_or(0);

    let (thumb, width, height) = thumbnail(path).unwrap_or_default();

    ShotDto {
        name: path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string(),
        path: path.to_string_lossy().into_owned(),
        size_kb,
        modified,
        width,
        height,
        thumb,
    }
}

fn thumbnail(path: &Path) -> Option<(String, u32, u32)> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    let thumb = img.thumbnail(THUMB_MAX, THUMB_MAX);

    let mut png = std::io::Cursor::new(Vec::new());
    thumb
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
    Some((format!("data:image/png;base64,{b64}"), w, h))
}
