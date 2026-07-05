// EasyQuickScreenshot — instant region screenshots for Windows.
// Resident tray app: two global hotkeys, crosshair overlay, PNG to disk + clipboard.

#![windows_subsystem = "windows"]

mod capture;
mod clipboard;
mod config;
mod overlay;
mod save;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, MessageBoxW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW,
    TranslateMessage, GWLP_USERDATA, MB_ICONERROR, MB_ICONWARNING, MB_OK, MSG, SW_SHOWNORMAL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_RBUTTONUP,
    WNDCLASSW,
};

use crate::config::Config;

const HOTKEY_QUICK: i32 = 1;
const HOTKEY_SAVE: i32 = 2;
const HOTKEY_FOLDER: i32 = 3;

/// Blocks re-entrant captures if a hotkey fires while the overlay is already open.
static IN_CAPTURE: AtomicBool = AtomicBool::new(false);

struct App {
    config: Config,
    config_override: Option<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config_override = arg_value(&args, "--config");

    // Headless test hook: eqs --shoot X Y W H out.png (virtual-screen coordinates).
    // Exercises capture -> crop -> encode without the interactive overlay.
    if let Some(i) = args.iter().position(|a| a == "--shoot") {
        std::process::exit(headless_shoot(&args[i + 1..]));
    }

    // Headless test hook: eqs --render-test SX SY W H (lines|cursor) out.png
    // Composes one real overlay frame (guides + selection border) with no window/message
    // pump, so the drawing code can be verified pixel-for-pixel from a screenshot diff.
    if let Some(i) = args.iter().position(|a| a == "--render-test") {
        std::process::exit(headless_render_test(&args[i + 1..]));
    }

    unsafe {
        let mutex = CreateMutexW(None, true, w!("EasyQuickScreenshot_SingleInstance"));
        if mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            message_box("EasyQuickScreenshot is already running (check the tray).", MB_ICONWARNING);
            return;
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let config = match config::load(config_override.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            message_box(&format!("Config error:\n{}", e), MB_ICONERROR);
            return;
        }
    };
    let _ = std::fs::create_dir_all(&config.saved_dir);

    let app = Box::into_raw(Box::new(App {
        config,
        config_override,
    }));

    unsafe {
        let instance = GetModuleHandleW(None).expect("GetModuleHandleW");
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("EQS_MAIN"),
            ..Default::default()
        };
        RegisterClassW(&class);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EQS_MAIN"),
            w!("EasyQuickScreenshot"),
            WINDOW_STYLE(0), // hidden message window — the tray is the only UI
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        )
        .expect("CreateWindowExW");
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);

        let cfg = &(*app).config;
        tray::add_icon(
            hwnd,
            &format!(
                "EasyQuickScreenshot — {} quick / {} save",
                cfg.quick_hotkey_label, cfg.save_hotkey_label
            ),
        );
        register_hotkeys(hwnd, cfg);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        drop(Box::from_raw(app));
    }
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

unsafe fn register_hotkeys(hwnd: HWND, cfg: &Config) {
    let mut failed = Vec::new();
    if RegisterHotKey(hwnd, HOTKEY_QUICK, cfg.quick_hotkey.modifiers, cfg.quick_hotkey.vk).is_err()
    {
        failed.push(cfg.quick_hotkey_label.clone());
    }
    if RegisterHotKey(hwnd, HOTKEY_SAVE, cfg.save_hotkey.modifiers, cfg.save_hotkey.vk).is_err() {
        failed.push(cfg.save_hotkey_label.clone());
    }
    if RegisterHotKey(hwnd, HOTKEY_FOLDER, cfg.folder_hotkey.modifiers, cfg.folder_hotkey.vk)
        .is_err()
    {
        failed.push(cfg.folder_hotkey_label.clone());
    }
    if !failed.is_empty() {
        message_box(
            &format!(
                "Could not register hotkey(s): {}\n\nAnother app already uses them. \
                 Change the binding in config.toml (tray > Open config), then tray > Reload config.",
                failed.join(", ")
            ),
            MB_ICONWARNING,
        );
    }
}

unsafe fn unregister_hotkeys(hwnd: HWND) {
    let _ = UnregisterHotKey(hwnd, HOTKEY_QUICK);
    let _ = UnregisterHotKey(hwnd, HOTKEY_SAVE);
    let _ = UnregisterHotKey(hwnd, HOTKEY_FOLDER);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if app_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let app = &mut *app_ptr;

    match msg {
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            if id == HOTKEY_FOLDER {
                // Not a capture — just reveal the current save folder. Reads the live
                // config, so it always opens wherever shots_dir points right now.
                open_in_explorer(&app.config.saved_dir);
            } else if (id == HOTKEY_QUICK || id == HOTKEY_SAVE)
                && IN_CAPTURE
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                run_capture(app, hwnd, id);
                IN_CAPTURE.store(false, Ordering::SeqCst);
            }
            LRESULT(0)
        }
        tray::WM_TRAYICON => {
            let event = (lparam.0 as u32) & 0xffff;
            if event == WM_RBUTTONUP {
                match tray::show_menu(hwnd) {
                    tray::CMD_SETTINGS => launch_settings(app),
                    tray::CMD_OPEN_SHOTS => open_in_explorer(&app.config.shots_dir),
                    tray::CMD_OPEN_CONFIG => {
                        if !app.config.config_path.exists() {
                            let _ = std::fs::write(&app.config.config_path, config::DEFAULT_CONFIG);
                        }
                        open_in_explorer(&app.config.config_path);
                    }
                    tray::CMD_RELOAD_CONFIG => reload_config(app, hwnd),
                    tray::CMD_QUIT => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            } else if event == WM_LBUTTONDBLCLK {
                launch_settings(app);
            }
            LRESULT(0)
        }
        tray::WM_EQS_RELOAD => {
            // The settings app saved config.toml — apply it live.
            reload_config(app, hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            unregister_hotkeys(hwnd);
            tray::remove_icon(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn run_capture(app: &App, hwnd: HWND, hotkey_id: i32) {
    let shot = match capture::capture_virtual_screen() {
        Ok(s) => s,
        Err(e) => {
            message_box(&format!("Capture failed: {}", e), MB_ICONERROR);
            return;
        }
    };
    let Some((x, y, w, h)) = overlay::select_region(&shot, app.config.crosshair_style) else {
        return; // cancelled
    };
    let Some((bgra, cw, ch)) = shot.crop(x, y, w, h) else {
        return;
    };

    let path = if hotkey_id == HOTKEY_QUICK {
        app.config.temp_path.clone()
    } else {
        save::timestamped_path(&app.config.saved_dir)
    };
    if let Err(e) = save::write_png_atomic(&path, &bgra, cw, ch) {
        message_box(&format!("Could not save screenshot:\n{}", e), MB_ICONERROR);
        return;
    }
    if app.config.copy_to_clipboard {
        // Clipboard is best-effort: the file already landed, so stay silent on failure.
        let _ = clipboard::copy_bgra(hwnd, &bgra, cw, ch);
    }
}

fn reload_config(app: &mut App, hwnd: HWND) {
    match config::load(app.config_override.as_deref()) {
        Ok(new_config) => unsafe {
            unregister_hotkeys(hwnd);
            app.config = new_config;
            let _ = std::fs::create_dir_all(&app.config.saved_dir);
            register_hotkeys(hwnd, &app.config);
        },
        Err(e) => message_box(&format!("Config error (kept old config):\n{}", e), MB_ICONERROR),
    }
}

/// Launch the settings/gallery companion (a separate process — never touches the capture
/// engine). Looks for eqs-settings.exe next to this exe; if it's already open, its
/// single-instance behavior brings it forward. Passes the active config path so both
/// operate on the exact same file.
fn launch_settings(app: &App) {
    let exe = config::exe_dir().join("eqs-settings.exe");
    if !exe.exists() {
        message_box(
            "Settings app not found.\n\nExpected eqs-settings.exe next to eqs.exe.\n\
             Build it with:  cd settings-app && cargo build --release",
            MB_ICONWARNING,
        );
        return;
    }
    let _ = std::process::Command::new(exe)
        .arg("--config")
        .arg(&app.config.config_path)
        .spawn();
}

fn open_in_explorer(path: &std::path::Path) {
    if let Some(dir) = path.parent().filter(|_| path.is_file()) {
        let _ = std::fs::create_dir_all(dir);
    } else if path.extension().is_none() {
        let _ = std::fs::create_dir_all(path);
    }
    let wide = to_wide(&path.to_string_lossy());
    unsafe {
        ShellExecuteW(
            HWND::default(),
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

fn message_box(text: &str, style: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) {
    let wide = to_wide(text);
    unsafe {
        MessageBoxW(
            HWND::default(),
            PCWSTR(wide.as_ptr()),
            w!("EasyQuickScreenshot"),
            MB_OK | style,
        );
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// eqs --shoot X Y W H out.png — capture a region (virtual-screen coords) with no UI.
/// Exit codes: 0 ok, 2 bad args, 3 capture failed, 4 empty crop, 5 write failed.
fn headless_shoot(rest: &[String]) -> i32 {
    if rest.len() < 5 {
        return 2;
    }
    let parse = |s: &String| s.parse::<i32>().ok();
    let (Some(x), Some(y), Some(w), Some(h)) =
        (parse(&rest[0]), parse(&rest[1]), parse(&rest[2]), parse(&rest[3]))
    else {
        return 2;
    };
    let Ok(shot) = capture::capture_virtual_screen() else {
        return 3;
    };
    let Some((bgra, cw, ch)) = shot.crop(x - shot.origin_x, y - shot.origin_y, w, h) else {
        return 4;
    };
    match save::write_png_atomic(std::path::Path::new(&rest[4]), &bgra, cw, ch) {
        Ok(()) => 0,
        Err(_) => 5,
    }
}

/// eqs --render-test SX SY W H (lines|cursor) out.png — draws one overlay frame (as if
/// dragging from (SX,SY) to (SX+W,SY+H), in output-image/buffer coordinates — NOT
/// virtual-screen coordinates, since the output PNG IS the buffer) over a real capture,
/// with no window at all.
/// Exit codes: 0 ok, 2 bad args, 3 capture failed, 4 compose failed, 5 write failed.
fn headless_render_test(rest: &[String]) -> i32 {
    if rest.len() < 6 {
        return 2;
    }
    let parse = |s: &String| s.parse::<i32>().ok();
    let (Some(x), Some(y), Some(w), Some(h)) =
        (parse(&rest[0]), parse(&rest[1]), parse(&rest[2]), parse(&rest[3]))
    else {
        return 2;
    };
    let style = match rest[4].as_str() {
        "lines" => config::CrosshairStyle::Lines,
        "cursor" => config::CrosshairStyle::Cursor,
        _ => return 2,
    };
    let Ok(shot) = capture::capture_virtual_screen() else {
        return 3;
    };
    let start = (x, y);
    let cur = (x + w, y + h);
    let Ok(bgra) = overlay::render_test_frame(&shot, style, start, cur) else {
        return 4;
    };
    match save::write_png_atomic(std::path::Path::new(&rest[5]), &bgra, shot.width, shot.height) {
        Ok(()) => 0,
        Err(_) => 5,
    }
}
