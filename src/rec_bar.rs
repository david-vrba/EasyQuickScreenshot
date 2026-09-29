// What is on screen while recording: a small bar with the elapsed time and a stop button,
// and a thin red frame around the recorded region. Both are excluded from capture, so
// neither ever appears in the video, and neither takes focus from the app being recorded.

use std::ffi::c_void;
use std::time::Instant;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, Ellipse, EndPaint, HGDIOBJ,
    InvalidateRect,
    FillRect, GetMonitorInfoW, MonitorFromPoint, Rectangle, RoundRect, SelectObject, SetBkMode,
    SetTextColor, TextOutW, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_QUALITY, FF_DONTCARE,
    FW_SEMIBOLD, GetStockObject, MONITORINFO, MONITOR_DEFAULTTONEAREST, NULL_BRUSH, OUT_TT_PRECIS,
    PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW,
    LoadCursorW, PostMessageW, RegisterClassW, SetCursor, SetLayeredWindowAttributes, SetTimer,
    SetWindowDisplayAffinity, SetWindowLongPtrW, ShowWindow, CREATESTRUCTW, GWLP_USERDATA,
    IDC_HAND, LWA_COLORKEY, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WM_APP, WM_ERASEBKGND,
    WM_LBUTTONUP, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

use crate::annotate::Rect;

/// Posted to the tray window when the stop button is clicked.
pub const WM_EQS_STOP_RECORDING: u32 = WM_APP + 3;

const BAR_CLASS: PCWSTR = w!("EQS_REC_BAR");
const FRAME_CLASS: PCWSTR = w!("EQS_REC_FRAME");
/// Every pixel of the frame window painted this colour is see-through and click-through.
const SEE_THROUGH: COLORREF = COLORREF(0x00FF00FF);
const RED: COLORREF = COLORREF(0x003030FF);

pub struct RecBar {
    bar: HWND,
    frame: HWND,
}

struct BarState {
    started: Instant,
    notify: HWND,
    scale: f32,
}

/// Put the controls up around `region` (virtual-desktop coordinates). The stop button posts
/// `WM_EQS_STOP_RECORDING` to `notify`.
pub fn show(region: Rect, notify: HWND) -> RecBar {
    unsafe {
        register_classes();
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let (monitor, scale) = monitor_of(region);
        let (bw, bh) = ((170.0 * scale) as i32, (38.0 * scale) as i32);
        let (bx, by) = bar_position(region, monitor, (bw, bh), (8.0 * scale) as i32);

        let state = Box::into_raw(Box::new(BarState { started: Instant::now(), notify, scale }));
        let bar = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            BAR_CLASS,
            w!("Recording"),
            WS_POPUP,
            bx,
            by,
            bw,
            bh,
            None,
            None,
            instance,
            Some(state as *const c_void),
        )
        .unwrap_or_default();

        let pad = (3.0 * scale).ceil() as i32;
        let frame = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            FRAME_CLASS,
            w!(""),
            WS_POPUP,
            region.0 - pad,
            region.1 - pad,
            region.2 + pad * 2,
            region.3 + pad * 2,
            None,
            None,
            instance,
            None,
        )
        .unwrap_or_default();
        let _ = SetLayeredWindowAttributes(frame, SEE_THROUGH, 0, LWA_COLORKEY);

        for hwnd in [bar, frame] {
            // Needs Windows 10 2004. On anything older the call fails and the controls would
            // show in the video — still better than no stop button.
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        let _ = SetTimer(bar, 1, 250, None);
        RecBar { bar, frame }
    }
}

impl RecBar {
    pub fn close(self) {
        unsafe {
            let _ = DestroyWindow(self.bar);
            let _ = DestroyWindow(self.frame);
        }
    }
}

/// Below the region if it fits on the monitor, above it if that fits, otherwise just inside
/// its bottom edge — where it still cannot be recorded. Centred on the region either way.
fn bar_position(region: Rect, monitor: RECT, (w, h): (i32, i32), gap: i32) -> (i32, i32) {
    let x = (region.0 + (region.2 - w) / 2).clamp(monitor.left, (monitor.right - w).max(monitor.left));
    let below = region.1 + region.3 + gap;
    let above = region.1 - gap - h;
    let y = if below + h <= monitor.bottom {
        below
    } else if above >= monitor.top {
        above
    } else {
        region.1 + region.3 - h - gap
    };
    (x, y)
}

unsafe fn monitor_of(region: Rect) -> (RECT, f32) {
    let centre = POINT { x: region.0 + region.2 / 2, y: region.1 + region.3 / 2 };
    let monitor = MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(monitor, &mut info);
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    (info.rcMonitor, dx as f32 / 96.0)
}

unsafe fn register_classes() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        for (name, proc_) in [
            (BAR_CLASS, bar_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
            (FRAME_CLASS, frame_proc),
        ] {
            let class = WNDCLASSW {
                lpfnWndProc: Some(proc_),
                hInstance: instance.into(),
                lpszClassName: name,
                ..Default::default()
            };
            RegisterClassW(&class);
        }
    });
}

/// "0:07", "12:30", "1:02:03".
pub fn elapsed_label(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

unsafe extern "system" fn bar_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BarState;
    if state.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_bar(hwnd, &*state);
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(None, IDC_HAND).unwrap_or_default());
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            let _ = PostMessageW((*state).notify, WM_EQS_STOP_RECORDING, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(state));
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_bar(hwnd: HWND, state: &BarState) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let s = |v: f32| (v * state.scale) as i32;
    let (w, h) = (s(170.0), s(38.0));

    let bg = CreateSolidBrush(COLORREF(0x001A1A1A));
    let edge = CreatePen(PS_SOLID, 1, COLORREF(0x00444444));
    let (old_brush, old_pen) = (SelectObject(dc, bg), SelectObject(dc, edge));
    let _ = RoundRect(dc, 0, 0, w, h, s(12.0), s(12.0));

    // Pulsing dot: solid for three quarters of every second, so the bar reads as live.
    let lit = state.started.elapsed().as_millis() % 1000 < 750;
    let dot = CreateSolidBrush(if lit { RED } else { COLORREF(0x00303060) });
    let no_pen = CreatePen(PS_SOLID, 0, if lit { RED } else { COLORREF(0x00303060) });
    SelectObject(dc, dot);
    SelectObject(dc, no_pen);
    let _ = Ellipse(dc, s(14.0), s(13.0), s(26.0), s(25.0));

    let font = CreateFontW(
        -s(15.0), 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0,
        DEFAULT_CHARSET.0 as u32, OUT_TT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32, FF_DONTCARE.0 as u32, w!("Segoe UI"),
    );
    let old_font = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, COLORREF(0x00F0F0F0));
    let label: Vec<u16> = elapsed_label(state.started.elapsed().as_secs()).encode_utf16().collect();
    let _ = TextOutW(dc, s(36.0), s(9.0), &label);

    // The stop button: a white square, the one symbol every player uses for it.
    let stop = CreateSolidBrush(COLORREF(0x00F0F0F0));
    let white_pen = CreatePen(PS_SOLID, 0, COLORREF(0x00F0F0F0));
    SelectObject(dc, stop);
    SelectObject(dc, white_pen);
    let _ = Rectangle(dc, w - s(34.0), s(12.0), w - s(20.0), s(26.0));

    SelectObject(dc, old_font);
    SelectObject(dc, old_brush);
    SelectObject(dc, old_pen);
    let made: [HGDIOBJ; 7] = [bg.into(), edge.into(), dot.into(), no_pen.into(), stop.into(), white_pen.into(), font.into()];
    for obj in made {
        let _ = DeleteObject(obj);
    }
    let _ = EndPaint(hwnd, &ps);
}

unsafe extern "system" fn frame_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut r = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut r);
            let clear = CreateSolidBrush(SEE_THROUGH);
            FillRect(dc, &r, clear);
            let pen = CreatePen(PS_SOLID, 2, RED);
            let (old_pen, old_brush) = (SelectObject(dc, pen), SelectObject(dc, GetStockObject(NULL_BRUSH)));
            let _ = Rectangle(dc, r.left + 1, r.top + 1, r.right, r.bottom);
            SelectObject(dc, old_pen);
            SelectObject(dc, old_brush);
            let _ = DeleteObject(pen);
            let _ = DeleteObject(clear);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor() -> RECT {
        RECT { left: 0, top: 0, right: 2560, bottom: 1440 }
    }

    #[test]
    fn the_bar_sits_below_the_region_when_there_is_room() {
        assert_eq!(bar_position((400, 300, 800, 600), monitor(), (170, 38), 8), (715, 908));
    }

    #[test]
    fn the_bar_goes_above_a_region_that_reaches_the_bottom() {
        assert_eq!(bar_position((400, 800, 800, 640), monitor(), (170, 38), 8), (715, 754));
    }

    #[test]
    fn a_full_screen_region_keeps_the_bar_inside_its_bottom_edge() {
        let (_, y) = bar_position((0, 0, 2560, 1440), monitor(), (170, 38), 8);
        assert_eq!(y, 1440 - 38 - 8);
    }

    #[test]
    fn the_clock_reads_like_a_clock() {
        assert_eq!(elapsed_label(7), "0:07");
        assert_eq!(elapsed_label(750), "12:30");
        assert_eq!(elapsed_label(3723), "1:02:03");
    }
}
