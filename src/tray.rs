// System tray icon + right-click menu. The tray is the app's only visible surface.

use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW,
    SetForegroundWindow, TrackPopupMenu, HICON, IDI_APPLICATION, IMAGE_FLAGS, MF_SEPARATOR,
    MF_STRING, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
};

pub const WM_TRAYICON: u32 = WM_APP + 1;
pub const CMD_OPEN_SHOTS: usize = 101;
pub const CMD_OPEN_CONFIG: usize = 102;
pub const CMD_RELOAD_CONFIG: usize = 103;
pub const CMD_QUIT: usize = 104;

/// The brand icon ships inside the binary; PNG data is valid icon-resource input
/// on Vista+ so no .ico parsing is needed. Falls back to the stock app icon.
fn app_icon() -> HICON {
    static ICON_PNG: &[u8] = include_bytes!("../assets/icon-64.png");
    unsafe {
        CreateIconFromResourceEx(ICON_PNG, true, 0x00030000, 32, 32, IMAGE_FLAGS(0))
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
    }
}

pub fn add_icon(hwnd: HWND, tip: &str) {
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: app_icon(),
            ..Default::default()
        };
        for (i, c) in tip.encode_utf16().take(data.szTip.len() - 1).enumerate() {
            data.szTip[i] = c;
        }
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
    }
}

pub fn remove_icon(hwnd: HWND) {
    unsafe {
        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

/// Show the tray context menu and return the chosen command id (0 = none).
pub fn show_menu(hwnd: HWND) -> usize {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return 0;
        };
        let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN_SHOTS, w!("Open shots folder"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN_CONFIG, w!("Open config"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_RELOAD_CONFIG, w!("Reload config"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit"));

        let mut pos = POINT::default();
        let _ = GetCursorPos(&mut pos);
        // Required quirk: the menu won't dismiss on outside-click unless our window is foreground.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pos.x,
            pos.y,
            0,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    }
}
