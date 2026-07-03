// Region-selection overlay: a borderless topmost window spanning all monitors,
// painted with the frozen screenshot at full brightness (no dimming — speed and
// clarity over ceremony). Position is marked by full-screen crosshair lines or a
// plain crosshair cursor (config `crosshair_style`). Lines and the selection border
// are drawn with R2_NOT (pixel inversion) so they read on any background.
// Returns the selection in screenshot-buffer coordinates. Esc / right-click cancels.

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC,
    DeleteObject, EndPaint, GetDC, GetStockObject, InvalidateRect, LineTo, MoveToEx, Rectangle,
    ReleaseDC, SelectObject, SetBkMode, SetROP2, SetTextColor, TextOutW, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DEFAULT_GUI_FONT, DIB_RGB_COLORS, HBITMAP, HDC, NULL_BRUSH,
    PAINTSTRUCT, R2_COPYPEN, R2_NOT, SRCCOPY, TRANSPARENT, WHITE_PEN,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, LoadCursorW,
    RegisterClassW, SetCursor, SetForegroundWindow, ShowWindow, TranslateMessage, CREATESTRUCTW,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HCURSOR, IDC_CROSS, MSG, SW_SHOW, WM_ERASEBKGND,
    WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT,
    WM_RBUTTONDOWN, WM_SETCURSOR, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
#[allow(unused_imports)]
use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW};

use crate::capture::Screenshot;
use crate::config::CrosshairStyle;

const CLASS_NAME: PCWSTR = w!("EQS_OVERLAY");
const MIN_SELECTION_PX: i32 = 3;

struct Overlay {
    back_dc: HDC,
    bright_dc: HDC,
    width: i32,
    height: i32,
    style: CrosshairStyle,
    dragging: bool,
    start: (i32, i32),
    cur: (i32, i32),
    result: Option<(i32, i32, i32, i32)>,
    done: bool,
}

/// Show the selection UI over the frozen screenshot.
/// Returns (x, y, w, h) in buffer coordinates, or None if cancelled.
pub fn select_region(shot: &Screenshot, style: CrosshairStyle) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        register_class_once(instance.into());

        let screen_dc = GetDC(HWND::default());
        let bright_dc = CreateCompatibleDC(screen_dc);
        let back_dc = CreateCompatibleDC(screen_dc);
        let bright_bmp = dib_from_pixels(screen_dc, shot.width, shot.height, &shot.pixels);
        let back_bmp = CreateCompatibleBitmap(screen_dc, shot.width, shot.height);
        ReleaseDC(HWND::default(), screen_dc);

        let Some(bright_bmp) = bright_bmp else {
            let _ = DeleteObject(back_bmp);
            let _ = DeleteDC(bright_dc);
            let _ = DeleteDC(back_dc);
            return None;
        };
        let old_bright = SelectObject(bright_dc, bright_bmp);
        let old_back = SelectObject(back_dc, back_bmp);

        let state = Box::into_raw(Box::new(Overlay {
            back_dc,
            bright_dc,
            width: shot.width,
            height: shot.height,
            style,
            dragging: false,
            start: (0, 0),
            cur: (-1, -1),
            result: None,
            done: false,
        }));

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            shot.origin_x,
            shot.origin_y,
            shot.width,
            shot.height,
            None,
            None,
            instance,
            Some(state as *const c_void),
        );

        let result = match hwnd {
            Ok(hwnd) => {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);

                let mut msg = MSG::default();
                loop {
                    if (*state).done {
                        break;
                    }
                    if GetMessageW(&mut msg, HWND::default(), 0, 0).0 <= 0 {
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = DestroyWindow(hwnd);
                (*state).result
            }
            Err(_) => None,
        };

        SelectObject(bright_dc, old_bright);
        SelectObject(back_dc, old_back);
        let _ = DeleteObject(bright_bmp);
        let _ = DeleteObject(back_bmp);
        let _ = DeleteDC(bright_dc);
        let _ = DeleteDC(back_dc);
        drop(Box::from_raw(state));

        result
    }
}

unsafe fn register_class_once(instance: windows::Win32::Foundation::HINSTANCE) {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&class);
    });
}

unsafe fn dib_from_pixels(dc: HDC, width: i32, height: i32, pixels: &[u8]) -> Option<HBITMAP> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
    Some(bmp)
}

fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 & 0xffff) as u16 as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as i32;
    (x, y)
}

fn normalized(a: (i32, i32), b: (i32, i32)) -> (i32, i32, i32, i32) {
    let x = a.0.min(b.0);
    let y = a.1.min(b.1);
    (x, y, (a.0 - b.0).abs(), (a.1 - b.1).abs())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Overlay;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_SETCURSOR if state.style == CrosshairStyle::Lines => {
            // The full-screen lines ARE the cursor in this mode.
            SetCursor(HCURSOR::default());
            LRESULT(1)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint(state, hdc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            state.cur = lparam_xy(lparam);
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            state.dragging = true;
            state.start = lparam_xy(lparam);
            state.cur = state.start;
            SetCapture(hwnd);
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP if state.dragging => {
            let _ = ReleaseCapture();
            state.cur = lparam_xy(lparam);
            let (x, y, w, h) = normalized(state.start, state.cur);
            if w >= MIN_SELECTION_PX && h >= MIN_SELECTION_PX {
                state.result = Some((x, y, w, h));
            }
            state.done = true;
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            if state.dragging {
                let _ = ReleaseCapture();
            }
            state.done = true;
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
            if state.dragging {
                let _ = ReleaseCapture();
            }
            state.done = true;
            LRESULT(0)
        }
        WM_KILLFOCUS if !state.dragging => {
            state.done = true;
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint(state: &Overlay, hdc: HDC) {
    let (w, h) = (state.width, state.height);
    let back = state.back_dc;

    // Frozen screen at full brightness — no dimming.
    let _ = BitBlt(back, 0, 0, w, h, state.bright_dc, 0, 0, SRCCOPY);

    // Guides and border invert the pixels beneath them (R2_NOT): visible everywhere,
    // and they exist only in the preview — the output crops from the untouched buffer.
    let old_pen = SelectObject(back, GetStockObject(WHITE_PEN));
    let old_brush = SelectObject(back, GetStockObject(NULL_BRUSH));
    SetROP2(back, R2_NOT);

    if state.style == CrosshairStyle::Lines && state.cur.0 >= 0 {
        let _ = MoveToEx(back, state.cur.0, 0, None);
        let _ = LineTo(back, state.cur.0, h);
        let _ = MoveToEx(back, 0, state.cur.1, None);
        let _ = LineTo(back, w, state.cur.1);
    }

    if state.dragging {
        let (sx, sy, sw, sh) = normalized(state.start, state.cur);
        let _ = Rectangle(back, sx - 1, sy - 1, sx + sw + 1, sy + sh + 1);
    }

    SetROP2(back, R2_COPYPEN);
    SelectObject(back, old_brush);
    SelectObject(back, old_pen);

    if state.dragging {
        let (sx, sy, sw, sh) = normalized(state.start, state.cur);
        draw_size_label(state, back, sx, sy, sw, sh);
    }

    let _ = BitBlt(hdc, 0, 0, w, h, back, 0, 0, SRCCOPY);
}

unsafe fn draw_size_label(state: &Overlay, dc: HDC, sx: i32, sy: i32, sw: i32, sh: i32) {
    let text: Vec<u16> = format!("{} x {}", sw, sh).encode_utf16().collect();
    let old_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
    SetBkMode(dc, TRANSPARENT);
    // Place below-right of the selection, clamped to the screen.
    let tx = (sx + 4).min(state.width - 80);
    let ty = (sy + sh + 6).min(state.height - 20);
    // Shadow + white text so it reads on any background.
    SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x00000000));
    let _ = TextOutW(dc, tx + 1, ty + 1, &text);
    SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x00FFFFFF));
    let _ = TextOutW(dc, tx, ty, &text);
    SelectObject(dc, old_font);
}
