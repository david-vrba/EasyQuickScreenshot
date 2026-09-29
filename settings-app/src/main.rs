// EasyQuickScreenshot Settings — a small Tauri companion window for editing config.toml,
// browsing saved screenshots, and a stats dashboard. Runs as a separate process from the
// resident capture engine, so nothing here affects capture speed.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config_io;
mod gallery;
mod tray_signal;
mod video_editor;

use config_io::ConfigDto;
use gallery::{GalleryStats, ShotDto};
use tauri::Manager;
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
    // Defense-in-depth: this is only ever called with the project's GitHub URL, but guard
    // the scheme so it can never become an arbitrary-launch primitive (file://, a custom
    // protocol handler, etc.) if the webview is ever abused.
    let allowed = url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:");
    if !allowed {
        return Err("refused to open a non-web URL".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// The recording to edit, when the tray app opened this window as the video editor.
#[tauri::command]
fn video_session(session: tauri::State<Option<video_editor::Session>>) -> Option<video_editor::SessionDto> {
    session.as_ref().map(|s| video_editor::SessionDto {
        path: s.video.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
async fn save_video(
    session: tauri::State<'_, Option<video_editor::Session>>,
    to: String,
    start: f64,
    end: f64,
    crop: Option<[i32; 4]>,
    volume: f64,
) -> Result<String, String> {
    let Some(session) = session.as_ref() else {
        return Err("no recording is open".into());
    };
    let (video, eqs) = (session.video.clone(), session.eqs.clone());
    let config = config_io::resolve_config_path();
    // Re-encoding a long recording takes seconds; keep it off the UI thread.
    tauri::async_runtime::spawn_blocking(move || {
        let session = video_editor::Session { video, eqs };
        video_editor::save(&session, &config, &to, start, end, crop, volume)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn close_editor(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    let session = video_editor::from_args();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let session = app.state::<Option<video_editor::Session>>();
            if let Some(session) = session.as_ref() {
                // The webview may read exactly this one file and nothing else on disk.
                let _ = app.asset_protocol_scope().allow_file(&session.video);
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_title("Edit recording — EasyQuickScreenshot");
                    let _ = window.set_size(tauri::LogicalSize::new(1180.0, 820.0));
                    let _ = window.center();
                    // Q and E only work once the window has the keyboard.
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        .manage(session)
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            gallery_stats,
            gallery_list,
            pick_shots_folder,
            open_path,
            reveal_path,
            open_url,
            video_session,
            save_video,
            close_editor,
        ])
        .run(tauri::generate_context!())
        .expect("error while running EasyQuickScreenshot Settings");
}
