// After saving config, nudge the running tray app to hot-reload it by posting a private
// window message to its hidden main window (class "EQS_MAIN"). No-op if the tray isn't running.

use windows::core::w;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_APP};

// Must match the core app's reload message id.
const WM_EQS_RELOAD: u32 = WM_APP + 2;

pub fn request_reload() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(w!("EQS_MAIN"), None) {
            if !hwnd.is_invalid() {
                let _ = PostMessageW(hwnd, WM_EQS_RELOAD, WPARAM(0), LPARAM(0));
            }
        }
    }
}
