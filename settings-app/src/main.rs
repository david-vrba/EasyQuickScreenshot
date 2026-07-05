// EasyQuickScreenshot Settings — a small Tauri companion window for editing config.toml,
// browsing saved screenshots, and a stats dashboard. Runs as a separate process from the
// resident capture engine, so nothing here affects capture speed.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config_io;
mod gallery;
mod tray_signal;

use config_io::ConfigDto;
use gallery::{GalleryStats, ShotDto};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
fn load_config() -> ConfigDto {
    config_io::load()
}

#[tauri::command]
fn save_config(cfg: ConfigDto) -> Result<(), String> {
    config_io::save(&cfg)?;
    // Live-apply in the running tray app (no-op if it isn't running).
    tray_signal::request_reload();
    Ok(())
}

#[tauri::command]
fn gallery_stats() -> GalleryStats {
    gallery::stats(&config_io::load().saved_dir_abs)
}

#[tauri::command]
fn gallery_list() -> Vec<ShotDto> {
    gallery::list(&config_io::load().saved_dir_abs)
}

#[tauri::command]
async fn pick_shots_folder(app: tauri::AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .ok()
        .flatten()
        .and_then(|fp| fp.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn open_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            gallery_stats,
            gallery_list,
            pick_shots_folder,
            open_path,
            reveal_path,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running EasyQuickScreenshot Settings");
}
