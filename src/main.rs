// EasyQuickScreenshot — instant region screenshots for Windows.
// Resident tray app: two global hotkeys, crosshair overlay, PNG to disk + clipboard.

#![windows_subsystem = "windows"]

mod annotate;
mod capture;
mod clipboard;
mod config;
mod overlay;
mod rec_bar;
mod mp4_writer;
mod recorder;
mod save;
mod screen_frames;
mod system_audio;
mod tray;
mod video_export;
mod window_pick;

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
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageTime, GetMessageW,
    GetWindowLongPtrW, MessageBoxW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW,
    TranslateMessage, GWLP_USERDATA, MB_ICONERROR, MB_ICONWARNING, MB_OK, MSG, SW_SHOWNORMAL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_RBUTTONUP,
    WNDCLASSW,
};

use crate::config::Config;

const HOTKEY_QUICK: i32 = 1;
const HOTKEY_RECORD: i32 = 2;
const HOTKEY_FOLDER: i32 = 3;
const HOTKEY_WINDOW: i32 = 4;
const HOTKEY_VIDEOS: i32 = 5;

/// Blocks re-entrant captures if a hotkey fires while the overlay is already open.
static IN_CAPTURE: AtomicBool = AtomicBool::new(false);

struct App {
    config: Config,
    config_override: Option<String>,
    /// The recording in progress and the controls shown for it.
    recording: Option<(recorder::Recording, rec_bar::RecBar)>,
}

/// A panic cannot unwind out of a window procedure, so Rust aborts and the tray icon
/// just disappears leaving nothing written down. Record the reason next to the exe first:
/// the panic message carries its own file and line, which is what makes a crash reportable.
fn install_panic_log() {
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        use windows::Win32::System::SystemInformation::GetLocalTime;
        let Ok(exe) = std::env::current_exe() else { return };
        let t = unsafe { GetLocalTime() };
        let entry = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  eqs {}\n{}\n\n",
            t.wYear,
            t.wMonth,
            t.wDay,
            t.wHour,
            t.wMinute,
            t.wSecond,
            env!("CARGO_PKG_VERSION"),
            info,
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(exe.with_file_name("eqs-panic.log"))
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }));
}

fn main() {
    install_panic_log();
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
    // The video editor's save: eqs --export-video IN quick|keep START END CROP VOLUME.
    // CROP is "x,y,w,h" in video pixels or "-". Prints where the file landed.
    if let Some(i) = args.iter().position(|a| a == "--export-video") {
        std::process::exit(export_video(&args[i + 1..], config_override.as_deref()));
    }

    // Headless test hook: eqs --record-test X Y W H SECONDS out.mp4 (virtual-screen
    // coordinates). Records with system sound and no on-screen controls, for ffprobe.
    if let Some(i) = args.iter().position(|a| a == "--record-test") {
        std::process::exit(headless_record(&args[i + 1..]));
    }

    if args.iter().any(|a| a == "--time-startup") {
        print_to_console(&overlay::startup_timing(5));
        return;
    }

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
    annotate::warm_up();

    let app = Box::into_raw(Box::new(App {
        config,
        config_override,
        recording: None,
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
                "EasyQuickScreenshot — {} capture · {} window · {} record",
                cfg.quick_hotkey_label, cfg.window_hotkey_label, cfg.record_hotkey_label
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

/// One line per capture in `eqs-timing.log`, the newest 50 kept. Written after the overlay
/// has closed, so measuring never slows the thing it measures.
fn log_timing(
    queued_ms: u32,
    pressed: std::time::Instant,
    grabbed: std::time::Instant,
    frame: Option<(std::time::Instant, std::time::Instant)>,
    shot: &capture::Screenshot,
) {
    let ms = |a: std::time::Instant, b: std::time::Instant| b.saturating_duration_since(a).as_millis();
    let (window, first) = match frame {
        Some((shown, painted)) => (ms(grabbed, shown), ms(shown, painted)),
        None => (0, 0),
    };
    let total = frame.map(|(_, painted)| ms(pressed, painted)).unwrap_or(0) + queued_ms as u128;
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  queued {:>4}ms  grab {:>4}ms  window {:>4}ms  first-frame {:>4}ms  total {:>4}ms  ({}x{})",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond,
        queued_ms, ms(pressed, grabbed), window, first, total, shot.width, shot.height,
    );
    let path = config::exe_dir().join("eqs-timing.log");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<&str> = old.lines().collect();
    lines.push(&line);
    let keep = &lines[lines.len().saturating_sub(50)..];
    let _ = std::fs::write(&path, keep.join("\n") + "\n");
}

fn export_video(rest: &[String], config_override: Option<&str>) -> i32 {
    let Some(edits) = parse_edits(rest) else {
        return 2;
    };
    let Ok(config) = config::load(config_override) else {
        return 2;
    };
    let input = std::path::PathBuf::from(&rest[0]);
    let destination = match rest[1].as_str() {
        "quick" => config.videos_dir.join("temp.mp4"),
        "keep" => save::timestamped(&config.videos_dir.join("saved"), "mp4"),
        _ => return 2,
    };
    if let Some(dir) = destination.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Written beside the destination, then renamed over it: whoever is watching temp.mp4
    // never sees half a file. The sink writer picks the container from the extension, so
    // the part file still has to end in .mp4.
    let part = destination.with_extension("part.mp4");
    if let Err(e) = video_export::export(&input, &part, edits) {
        print_to_console(&format!("could not save the video: {e}\n"));
        return 4;
    }
    if let Err(e) = std::fs::rename(&part, &destination) {
        print_to_console(&format!("could not move the video into place: {e}\n"));
        return 5;
    }
    if config.copy_to_clipboard {
        let _ = clipboard::copy_file(&destination);
    }
    print_to_console(&format!("{}\n", destination.display()));
    0
}

/// START END CROP VOLUME, after the input path and the destination word.
fn parse_edits(rest: &[String]) -> Option<video_export::Edits> {
    let num = |i: usize| rest.get(i).and_then(|s| s.parse::<f64>().ok());
    let crop = match rest.get(4).map(String::as_str) {
        Some("-") => None,
        Some(spec) => {
            let v: Vec<i32> = spec.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            if v.len() != 4 {
                return None;
            }
            Some((v[0], v[1], v[2], v[3]))
        }
        None => return None,
    };
    Some(video_export::Edits {
        start: num(2)?,
        end: num(3)?,
        crop,
        volume: num(5)?.clamp(0.0, 2.0) as f32,
    })
}

fn headless_record(rest: &[String]) -> i32 {
    let int = |s: &String| s.parse::<i32>().ok();
    let (Some(x), Some(y), Some(w), Some(h)) = (int(&rest[0]), int(&rest[1]), int(&rest[2]), int(&rest[3]))
    else {
        return 2;
    };
    let (Some(seconds), Some(out)) = (rest.get(4).and_then(|s| s.parse::<f64>().ok()), rest.get(5)) else {
        return 2;
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let settings = recorder::Settings { fps: 30, system_sound: true };
    let recording = match recorder::start((x, y, w, h), out.into(), settings) {
        Ok(r) => r,
        Err(e) => {
            print_to_console(&format!("could not start: {e}\n"));
            return 3;
        }
    };
    let region = recording.region;
    std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
    match recording.stop() {
        Ok(path) => {
            print_to_console(&format!("recorded {:?} to {}\n", region, path.display()));
            0
        }
        Err(e) => {
            print_to_console(&format!("could not finish: {e}\n"));
            5
        }
    }
}

/// The exe is built for the Windows subsystem, so it has no console of its own. Borrow
/// the parent's when there is one, which is the case when it is run from a terminal.
fn print_to_console(text: &str) {
    use std::io::Write;
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    let _ = std::io::stdout().write_all(text.as_bytes());
    let _ = std::io::stdout().flush();
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
    if RegisterHotKey(hwnd, HOTKEY_RECORD, cfg.record_hotkey.modifiers, cfg.record_hotkey.vk)
        .is_err()
    {
        failed.push(cfg.record_hotkey_label.clone());
    }
    if RegisterHotKey(
        hwnd,
        HOTKEY_VIDEOS,
        cfg.videos_folder_hotkey.modifiers,
        cfg.videos_folder_hotkey.vk,
    )
    .is_err()
    {
        failed.push(cfg.videos_folder_hotkey_label.clone());
    }
    if RegisterHotKey(hwnd, HOTKEY_FOLDER, cfg.folder_hotkey.modifiers, cfg.folder_hotkey.vk)
        .is_err()
    {
        failed.push(cfg.folder_hotkey_label.clone());
    }
    // Focus mode is opt-out: with it off the binding is never claimed, so nothing else
    // on the machine loses the key.
    if cfg.window_pick
        && RegisterHotKey(
            hwnd,
            HOTKEY_WINDOW,
            cfg.window_hotkey.modifiers,
            cfg.window_hotkey.vk,
        )
        .is_err()
    {
        failed.push(cfg.window_hotkey_label.clone());
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
    let _ = UnregisterHotKey(hwnd, HOTKEY_RECORD);
    let _ = UnregisterHotKey(hwnd, HOTKEY_VIDEOS);
    let _ = UnregisterHotKey(hwnd, HOTKEY_FOLDER);
    let _ = UnregisterHotKey(hwnd, HOTKEY_WINDOW);
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
            // How long the press sat in the queue before this thread picked it up. Large
            // numbers mean the app itself was slow to wake, not the capture.
            let queued_ms = GetTickCount().wrapping_sub(GetMessageTime() as u32);
            if id == HOTKEY_FOLDER {
                // Not a capture — just reveal the current save folder. Reads the live
                // config, so it always opens wherever shots_dir points right now.
                open_in_explorer(&app.config.saved_dir);
            } else if id == HOTKEY_VIDEOS {
                open_in_explorer(&app.config.videos_dir);
            } else if id == HOTKEY_RECORD {
                toggle_recording(app, hwnd);
            } else if (id == HOTKEY_QUICK || id == HOTKEY_WINDOW)
                && IN_CAPTURE
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                run_capture(app, hwnd, id, queued_ms);
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
                    tray::CMD_OPEN_VIDEOS => open_in_explorer(&app.config.videos_dir),
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
        rec_bar::WM_EQS_STOP_RECORDING => {
            stop_recording(app);
            LRESULT(0)
        }
        tray::WM_EQS_RELOAD => {
            // The settings app saved config.toml — apply it live.
            reload_config(app, hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            // Quitting mid-recording still finishes the file; an unfinished MP4 has no
            // index and plays nowhere.
            if let Some((recording, bar)) = app.recording.take() {
                bar.close();
                let _ = recording.stop();
            }
            unregister_hotkeys(hwnd);
            tray::remove_icon(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn run_capture(app: &App, hwnd: HWND, hotkey_id: i32, queued_ms: u32) {
    let pressed = std::time::Instant::now();
    let shot = match capture::capture_virtual_screen() {
        Ok(s) => s,
        Err(e) => {
            message_box(&format!("Capture failed: {}", e), MB_ICONERROR);
            return;
        }
    };
    // Every hotkey now ends in the editor; they differ only in how the rectangle is
    // chosen. The destination is picked there — Q writes the temp file, E keeps a copy.
    let start = if hotkey_id == HOTKEY_WINDOW {
        overlay::Start::PickWindow
    } else {
        overlay::Start::Drag
    };
    let grabbed = std::time::Instant::now();
    let selection = overlay::select_region(&shot, app.config.crosshair_style, true, start);
    if app.config.timing_log {
        log_timing(queued_ms, pressed, grabbed, overlay::take_first_frame(), &shot);
    }
    let Some(sel) = selection else {
        return; // cancelled
    };
    let (x, y, w, h) = sel.rect;
    // An annotated capture arrives already flattened; a plain one crops the untouched buffer.
    let Some((bgra, cw, ch)) = (match sel.pixels {
        Some(pixels) => Some((pixels, w, h)),
        None => shot.crop(x, y, w, h),
    }) else {
        return;
    };

    // Annotating picks its destination at commit time: Enter overwrites the temp file,
    // Shift+Enter (or the KEEP button) files a timestamped copy.
    let keep = sel.keeper;
    let path = if keep {
        save::timestamped_path(&app.config.saved_dir)
    } else {
        app.config.temp_path.clone()
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

/// `Ctrl+Alt+E`: start a recording, or stop the one running.
fn toggle_recording(app: &mut App, hwnd: HWND) {
    if app.recording.is_some() {
        stop_recording(app);
        return;
    }
    if IN_CAPTURE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    start_recording(app, hwnd);
    IN_CAPTURE.store(false, Ordering::SeqCst);
}

/// The same crosshair drag as a screenshot picks the region; then recording starts.
fn start_recording(app: &mut App, hwnd: HWND) {
    let shot = match capture::capture_virtual_screen() {
        Ok(s) => s,
        Err(e) => {
            message_box(&format!("Capture failed: {}", e), MB_ICONERROR);
            return;
        }
    };
    let selection = overlay::select_region(&shot, app.config.crosshair_style, false, overlay::Start::Drag);
    let Some(sel) = selection else {
        return; // cancelled
    };
    let (x, y, w, h) = sel.rect;
    let region = (x + shot.origin_x, y + shot.origin_y, w, h);
    drop(shot); // 37 MB that the recording has no use for
    let settings = recorder::Settings {
        fps: app.config.record_fps,
        system_sound: app.config.record_system_sound,
    };
    let path = app.config.videos_dir.join("recording.mp4");
    match recorder::start(region, path, settings) {
        Ok(recording) => {
            let bar = rec_bar::show(recording.region, hwnd);
            app.recording = Some((recording, bar));
        }
        Err(e) => message_box(&format!("Could not start recording:\n{}", e), MB_ICONERROR),
    }
}

fn stop_recording(app: &mut App) {
    let Some((recording, bar)) = app.recording.take() else { return };
    bar.close();
    match recording.stop() {
        Ok(raw) => open_video_editor(app, &raw),
        Err(e) => message_box(&format!("The recording could not be finished:\n{}", e), MB_ICONERROR),
    }
}

/// Hand the finished recording to the editor in the settings app, where Q and E save it.
/// Without the settings app there is no editor, so the recording simply becomes the quick
/// video, like a screenshot saved with Q.
fn open_video_editor(app: &App, raw: &std::path::Path) {
    let editor = config::exe_dir().join("eqs-settings.exe");
    if editor.exists() {
        // Windows only lets a new window take focus if whoever launches it just had input.
        // This process did — the hotkey or the stop button — so it passes that right on.
        // Without it the editor opens behind the app you were recording and Q does nothing.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
                windows::Win32::UI::WindowsAndMessaging::ASFW_ANY,
            );
        }
        let this = std::env::current_exe().unwrap_or_default();
        let _ = std::process::Command::new(editor)
            .arg("--edit-video")
            .arg(raw)
            .arg("--eqs")
            .arg(this)
            .arg("--config")
            .arg(&app.config.config_path)
            .spawn();
    } else {
        let _ = std::fs::rename(raw, app.config.videos_dir.join("temp.mp4"));
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


/// eqs --render-test SX SY W H (lines|cursor) out.png [annotate|export] — draws one overlay frame (as if
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
    // `export` runs the real commit path instead of a preview frame, so the flattened
    // output can be checked without driving the UI.
    if rest.get(6).map(|s| s == "export").unwrap_or(false) {
        let Ok(shot) = capture::capture_virtual_screen() else {
            return 3;
        };
        let Ok((bgra, cw, ch)) = overlay::export_test(&shot, (x, y), (x + w, y + h)) else {
            return 4;
        };
        return match save::write_png_atomic(std::path::Path::new(&rest[5]), &bgra, cw, ch) {
            Ok(()) => 0,
            Err(_) => 5,
        };
    }
    let demo = match rest.get(6).map(String::as_str) {
        Some("annotate") => overlay::Demo::Annotating,
        Some("pick") => overlay::Demo::Picking,
        _ => overlay::Demo::Selecting,
    };
    let Ok(bgra) = overlay::render_test_frame(&shot, style, start, cur, demo) else {
        return 4;
    };
    match save::write_png_atomic(std::path::Path::new(&rest[5]), &bgra, shot.width, shot.height) {
        Ok(()) => 0,
        Err(_) => 5,
    }
}

#[cfg(test)]
mod tests {
    /// `set_hook` is process-wide, so this replaces the test harness's own hook. Harmless
    /// while every other test passes, and this is the only way to prove a crash leaves a
    /// trace without shipping a flag that deliberately crashes the app.
    #[test]
    fn a_panic_is_written_down_instead_of_vanishing() {
        super::install_panic_log();
        let log = std::env::current_exe().unwrap().with_file_name("eqs-panic.log");
        let _ = std::fs::remove_file(&log);
        let _ = std::panic::catch_unwind(|| panic!("panic-log self check"));
        let body = std::fs::read_to_string(&log).expect("the hook must have written a file");
        assert!(body.contains("panic-log self check"), "the reason: {}", body);
        assert!(body.contains("src\\main.rs"), "and where it happened: {}", body);
        let _ = std::fs::remove_file(&log);
    }
}
