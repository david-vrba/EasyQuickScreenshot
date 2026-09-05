// Region-selection overlay: a borderless topmost window spanning all monitors,
// painted with the frozen screenshot at full brightness (no dimming — speed and
// clarity over ceremony). Position is marked by full-screen crosshair lines or a
// plain crosshair cursor (config `crosshair_style`). Lines and the selection border
// are drawn with R2_NOT (pixel inversion) so they read on any background.
// Returns the selection in screenshot-buffer coordinates. Esc / right-click cancels.
//
// With `annotate` set, releasing the drag does not finish the capture: the same window
// switches to a second phase where the frozen selection can be drawn on (see annotate.rs)
// before it is committed. Nothing extra is created for it — no window, no process.

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC,
    DeleteObject, EndPaint, GetDC, GetDIBits, GetStockObject, InvalidateRect, LineTo, MoveToEx,
    Rectangle, ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetROP2, SetTextColor, TextOutW,
    BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DEFAULT_GUI_FONT, DIB_RGB_COLORS, HBITMAP, HDC, NULL_BRUSH,
    PAINTSTRUCT, R2_COPYPEN, R2_NOT, SRCCOPY, TRANSPARENT, WHITE_PEN,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_ESCAPE, VK_RETURN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    LoadCursorW, PostQuitMessage, RegisterClassW, SetCursor, SetForegroundWindow, ShowWindow,
    TranslateMessage, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HCURSOR, IDC_ARROW,
    IDC_CROSS, IDC_SIZEALL, MSG, SW_SHOW, WM_CHAR, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT, WM_QUIT,
    WM_RBUTTONDOWN, WM_SETCURSOR, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
#[allow(unused_imports)]
use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW};

use crate::annotate::{self, Action, Cell, Rect, Shape, Tool, PALETTE, WIDTHS};
use crate::capture::Screenshot;
use crate::config::CrosshairStyle;

const CLASS_NAME: PCWSTR = w!("EQS_OVERLAY");
const MIN_SELECTION_PX: i32 = 3;

#[derive(PartialEq, Clone, Copy)]
enum Phase {
    Select,
    Annotate,
}

/// What the overlay hands back. `pixels` is set only for an annotated capture, where the
/// drawing has already been flattened into the cropped region.
pub struct Selection {
    pub rect: (i32, i32, i32, i32),
    pub pixels: Option<Vec<u8>>,
    pub keeper: bool,
}

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

    // Annotate phase
    annotate: bool,
    phase: Phase,
    sel: (i32, i32, i32, i32),
    cells: Vec<Cell>,
    shapes: Vec<Shape>,
    active: Option<Shape>,
    tool: Tool,
    color: usize,
    stroke: usize,
    keeper: bool,
    /// Text being typed; committed to `shapes` on Enter or when the mouse does anything else.
    typing: Option<Shape>,
    /// A shape or the whole region being dragged by its border, with the last pointer position.
    moving: Option<(Moving, (i32, i32))>,
    /// Set for the final compose: shapes only, no border, guides, or toolbar.
    exporting: bool,
}

#[derive(Clone, Copy)]
enum Moving {
    Shape(usize),
    Region,
}

/// Show the selection UI over the frozen screenshot.
/// Returns the selection, or None if cancelled.
pub fn select_region(
    shot: &Screenshot,
    style: CrosshairStyle,
    annotate: bool,
) -> Option<Selection> {
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

        // Seed with the live cursor position so the guides show before the first mouse move.
        let mut cursor = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut cursor);

        let state = Box::into_raw(Box::new(Overlay {
            back_dc,
            bright_dc,
            width: shot.width,
            height: shot.height,
            style,
            dragging: false,
            start: (0, 0),
            cur: (cursor.x - shot.origin_x, cursor.y - shot.origin_y),
            result: None,
            done: false,
            annotate,
            phase: Phase::Select,
            sel: (0, 0, 0, 0),
            cells: Vec::new(),
            shapes: Vec::new(),
            active: None,
            tool: Tool::Rect,
            color: 0,
            stroke: 1,
            keeper: false,
            typing: None,
            moving: None,
            exporting: false,
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
                        // Don't swallow an app-wide quit inside this nested loop —
                        // re-post it so the main message loop also exits.
                        if msg.message == WM_QUIT {
                            PostQuitMessage(msg.wParam.0 as i32);
                        }
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                // Flatten the drawing before the window (and its DCs) go away.
                let selection = (*state).result.map(|rect| Selection {
                    rect,
                    pixels: if (*state).phase == Phase::Annotate {
                        export_annotated(&mut *state)
                    } else {
                        None
                    },
                    keeper: (*state).keeper,
                });
                let _ = DestroyWindow(hwnd);
                selection
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

/// Re-compose with shapes only, then lift just the selection rectangle out of the back
/// buffer. Returns top-down BGRA, exactly `w * h * 4` bytes.
unsafe fn export_annotated(state: &mut Overlay) -> Option<Vec<u8>> {
    let (sx, sy, sw, sh) = state.sel;
    if sw <= 0 || sh <= 0 {
        return None;
    }
    state.exporting = true;
    compose(state);
    state.exporting = false;

    let screen_dc = GetDC(HWND::default());
    let dc = CreateCompatibleDC(screen_dc);
    ReleaseDC(HWND::default(), screen_dc);

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: sw,
            biHeight: -sh, // negative = top-down rows, matching the capture buffer
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let Ok(bmp) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
        let _ = DeleteDC(dc);
        return None;
    };
    let old = SelectObject(dc, bmp);
    let ok = BitBlt(dc, 0, 0, sw, sh, state.back_dc, sx, sy, SRCCOPY).is_ok();

    let pixels = if ok && !bits.is_null() {
        let len = sw as usize * sh as usize * 4;
        Some(std::slice::from_raw_parts(bits as *const u8, len).to_vec())
    } else {
        None
    };

    SelectObject(dc, old);
    let _ = DeleteObject(bmp);
    let _ = DeleteDC(dc);
    pixels
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

unsafe fn key_down(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
    GetKeyState(vk.0 as i32) < 0
}

/// Keep drawing inside the captured region — a stroke that spilled outside would be
/// invisible in the output and look like a bug.
fn clamp_to(sel: (i32, i32, i32, i32), x: i32, y: i32) -> (i32, i32) {
    (
        x.clamp(sel.0, sel.0 + sel.2 - 1),
        y.clamp(sel.1, sel.1 + sel.3 - 1),
    )
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

    if state.phase == Phase::Annotate {
        return annotate_proc(hwnd, state, msg, wparam, lparam);
    }

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
            if w < MIN_SELECTION_PX || h < MIN_SELECTION_PX {
                state.done = true;
                return LRESULT(0);
            }
            state.result = Some((x, y, w, h));
            if state.annotate {
                // Second phase in the same window: freeze the region and raise the bar.
                state.dragging = false;
                state.sel = (x, y, w, h);
                state.cells = annotate::layout(state.sel, (state.width, state.height));
                state.phase = Phase::Annotate;
                let _ = InvalidateRect(hwnd, None, false);
            } else {
                state.done = true;
            }
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

unsafe fn annotate_proc(
    hwnd: HWND,
    state: &mut Overlay,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint(state, hdc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let (x, y) = cursor_in_client(hwnd);
            let cursor = if annotate::over_bar(&state.cells, x, y) {
                IDC_ARROW
            } else if state.moving.is_some() || grab_target(state, x, y).is_some() {
                IDC_SIZEALL
            } else {
                IDC_CROSS
            };
            SetCursor(LoadCursorW(None, cursor).unwrap_or_default());
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_xy(lparam);
            if let Some(action) = annotate::hit_test(&state.cells, x, y) {
                commit_typing(state);
                apply(state, action);
                if state.done {
                    return LRESULT(0);
                }
            } else if !annotate::over_bar(&state.cells, x, y) {
                commit_typing(state);
                if let Some(target) = grab_target(state, x, y) {
                    state.moving = Some((target, (x, y)));
                    SetCapture(hwnd);
                } else {
                    let p = clamp_to(state.sel, x, y);
                    let shape = Shape::new(state.tool, PALETTE[state.color], WIDTHS[state.stroke], p);
                    if state.tool == Tool::Text {
                        state.typing = Some(shape);
                    } else {
                        state.active = Some(shape);
                        SetCapture(hwnd);
                    }
                }
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = lparam_xy(lparam);
            if let Some((target, last)) = state.moving {
                move_target(state, target, x - last.0, y - last.1);
                state.moving = Some((target, (x, y)));
                let _ = InvalidateRect(hwnd, None, false);
            } else if let Some(shape) = state.active.as_mut() {
                let (cx, cy) = clamp_to(state.sel, x, y);
                shape.drag_to(cx, cy, key_down(VK_SHIFT));
                // A Shift-constrained endpoint can land outside the region; pull it back so
                // the preview never shows more than the export will keep.
                if let Some(end) = shape.pts.last_mut() {
                    *end = clamp_to(state.sel, end.0, end.1);
                }
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            state.moving = None;
            if let Some(shape) = state.active.take() {
                if !shape.is_degenerate() {
                    state.shapes.push(shape);
                }
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // Layout-independent size control — [ and ] sit on different physical keys
            // across layouts, the wheel does not.
            let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
            let next = state.stroke as i32 + if delta > 0 { 1 } else { -1 };
            apply(state, Action::Width(next.clamp(0, WIDTHS.len() as i32 - 1) as usize));
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            // Mouse-only undo; matches the right-click-cancels reflex from phase one.
            if state.typing.take().is_none() {
                state.active = None;
                state.shapes.pop();
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(t) = state.typing.as_mut() {
                let ch = wparam.0 as u16;
                if ch == 0x08 {
                    t.text.pop();
                } else if ch >= 0x20 {
                    t.text.push(ch);
                }
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            if state.typing.is_some() {
                // While typing, letters are text — only Enter and Esc mean anything else.
                if vk == VK_ESCAPE.0 {
                    state.typing = None;
                } else if vk == VK_RETURN.0 {
                    commit_typing(state);
                }
                let _ = InvalidateRect(hwnd, None, false);
                return LRESULT(0);
            }
            let ctrl = key_down(VK_CONTROL);
            if vk == VK_ESCAPE.0 {
                if state.active.take().is_none() {
                    state.result = None; // second Esc aborts the whole capture
                    state.done = true;
                }
            } else if vk == VK_RETURN.0 {
                apply(state, Action::Commit { keeper: key_down(VK_SHIFT) });
            } else if ctrl && vk as u8 as char == 'Z' {
                state.active = None;
                state.shapes.pop();
            } else if !ctrl {
                if let Some(action) = annotate::key_action(vk) {
                    apply(state, action);
                }
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn cursor_in_client(hwnd: HWND) -> (i32, i32) {
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    let _ = ScreenToClient(hwnd, &mut p);
    (p.x, p.y)
}

/// What a press at (x, y) would drag: the topmost shape whose outline is under the pointer,
/// else the region border, else nothing.
unsafe fn grab_target(state: &Overlay, x: i32, y: i32) -> Option<Moving> {
    if let Some(i) = state.shapes.iter().rposition(|s| s.hits_border(x, y)) {
        return Some(Moving::Shape(i));
    }
    if annotate::hits_rect_border(state.sel, x, y) {
        return Some(Moving::Region);
    }
    None
}

unsafe fn move_target(state: &mut Overlay, target: Moving, dx: i32, dy: i32) {
    match target {
        Moving::Region => {
            let (sx, sy, sw, sh) = state.sel;
            let nx = (sx + dx).clamp(0, (state.width - sw).max(0));
            let ny = (sy + dy).clamp(0, (state.height - sh).max(0));
            state.sel = (nx, ny, sw, sh);
            state.result = Some(state.sel);
            state.cells = annotate::layout(state.sel, (state.width, state.height));
        }
        Moving::Shape(i) => {
            let Some(shape) = state.shapes.get_mut(i) else { return };
            let (x0, y0, x1, y1) = shape.bounds();
            let (sx, sy, sw, sh) = state.sel;
            // Keep the whole shape inside the region; a shape wider than the region just
            // stays put on that axis.
            let dx = if x1 - x0 <= sw { dx.clamp(sx - x0, sx + sw - x1) } else { 0 };
            let dy = if y1 - y0 <= sh { dy.clamp(sy - y0, sy + sh - y1) } else { 0 };
            shape.move_by(dx, dy);
        }
    }
}

fn commit_typing(state: &mut Overlay) {
    if let Some(t) = state.typing.take() {
        if !t.is_degenerate() {
            state.shapes.push(t);
        }
    }
}

fn apply(state: &mut Overlay, action: Action) {
    match action {
        Action::Pick(t) => state.tool = t,
        Action::Color(i) => state.color = i,
        Action::Width(i) => state.stroke = i,
        Action::Commit { keeper } => {
            state.keeper = keeper;
            state.done = true;
        }
    }
    if let Some(t) = state.typing.as_mut() {
        t.color = PALETTE[state.color];
        t.width = WIDTHS[state.stroke];
    }
}

unsafe fn paint(state: &Overlay, hdc: HDC) {
    compose(state);
    let _ = BitBlt(hdc, 0, 0, state.width, state.height, state.back_dc, 0, 0, SRCCOPY);
}

/// Draws one frame into `state.back_dc`. Pure GDI composition, no window/message-pump
/// dependency — also used by `render_test_frame` to verify drawing headlessly.
unsafe fn compose(state: &Overlay) {
    let (w, h) = (state.width, state.height);
    let back = state.back_dc;

    // Frozen screen at full brightness — no dimming.
    let _ = BitBlt(back, 0, 0, w, h, state.bright_dc, 0, 0, SRCCOPY);

    if state.phase == Phase::Annotate {
        annotate::draw_shapes(
            back,
            state.sel,
            &state.shapes,
            state.active.as_ref(),
            state.typing.as_ref(),
        );
        if state.exporting {
            return; // the output must carry the drawing and nothing else
        }
        let old_pen = SelectObject(back, GetStockObject(WHITE_PEN));
        let old_brush = SelectObject(back, GetStockObject(NULL_BRUSH));
        SetROP2(back, R2_NOT);
        let (sx, sy, sw, sh) = state.sel;
        let _ = Rectangle(back, sx - 1, sy - 1, sx + sw + 1, sy + sh + 1);
        SetROP2(back, R2_COPYPEN);
        SelectObject(back, old_brush);
        SelectObject(back, old_pen);
        annotate::draw_toolbar(back, &state.cells, state.tool, state.color, state.stroke);
        return;
    }

    // Guides and border invert the pixels beneath them (R2_NOT): visible everywhere,
    // and they exist only in the preview — the output crops from the untouched buffer.
    let old_pen = SelectObject(back, GetStockObject(WHITE_PEN));
    let old_brush = SelectObject(back, GetStockObject(NULL_BRUSH));
    SetROP2(back, R2_NOT);

    if state.style == CrosshairStyle::Lines && state.cur.0 >= 0 {
        if state.dragging {
            // `cur` sits exactly on one corner of the selection, so a full-length guide
            // line shares a column/row with the border GDI actually draws there (Rectangle's
            // bottom/right edge is exclusive, landing one pixel in from what you'd expect).
            // R2_NOT inverts whatever's already drawn, so painting that pixel twice cancels
            // back to the original color — an invisible edge. Gap the guides around the
            // rectangle's full span so they never touch a border pixel on any side.
            let (sx, sy, sw, sh) = normalized(state.start, state.cur);
            let (bx0, by0, bx1, by1) = (sx - 1, sy - 1, sx + sw + 1, sy + sh + 1);
            if by0 > 0 {
                let _ = MoveToEx(back, state.cur.0, 0, None);
                let _ = LineTo(back, state.cur.0, by0);
            }
            if by1 < h {
                let _ = MoveToEx(back, state.cur.0, by1, None);
                let _ = LineTo(back, state.cur.0, h);
            }
            if bx0 > 0 {
                let _ = MoveToEx(back, 0, state.cur.1, None);
                let _ = LineTo(back, bx0, state.cur.1);
            }
            if bx1 < w {
                let _ = MoveToEx(back, bx1, state.cur.1, None);
                let _ = LineTo(back, w, state.cur.1);
            }
        } else {
            let _ = MoveToEx(back, state.cur.0, 0, None);
            let _ = LineTo(back, state.cur.0, h);
            let _ = MoveToEx(back, 0, state.cur.1, None);
            let _ = LineTo(back, w, state.cur.1);
        }
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
}

/// Headless verification hook: composes one frame against a real capture without ever
/// creating a window, so the exact drawing code can be inspected pixel-for-pixel from
/// a CLI flag. Returns top-down BGRA pixels.
pub fn render_test_frame(
    shot: &Screenshot,
    style: CrosshairStyle,
    start: (i32, i32),
    cur: (i32, i32),
    annotate_demo: bool,
) -> Result<Vec<u8>, String> {
    unsafe {
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
            return Err("failed to build source DIB".into());
        };
        let old_bright = SelectObject(bright_dc, bright_bmp);
        let old_back = SelectObject(back_dc, back_bmp);

        let sel = normalized(start, cur);
        let mut state = Overlay {
            back_dc,
            bright_dc,
            width: shot.width,
            height: shot.height,
            style,
            dragging: true,
            start,
            cur,
            result: None,
            done: false,
            annotate: annotate_demo,
            phase: Phase::Select,
            sel,
            cells: Vec::new(),
            shapes: Vec::new(),
            active: None,
            tool: Tool::Rect,
            color: 0,
            stroke: 1,
            keeper: false,
            typing: None,
            moving: None,
            exporting: false,
        };

        if annotate_demo {
            // One of every tool, so a single frame proves the whole render path.
            state.phase = Phase::Annotate;
            state.dragging = false;
            state.cells = annotate::layout(sel, (shot.width, shot.height));
            state.shapes = demo_shapes(sel);
        }
        compose(&state);

        let mut pixels = vec![0u8; shot.width as usize * shot.height as usize * 4];
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: shot.width,
                biHeight: -shot.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        GetDIBits(
            back_dc,
            back_bmp,
            0,
            shot.height as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(bright_dc, old_bright);
        SelectObject(back_dc, old_back);
        let _ = DeleteObject(bright_bmp);
        let _ = DeleteObject(back_bmp);
        let _ = DeleteDC(bright_dc);
        let _ = DeleteDC(back_dc);

        Ok(pixels)
    }
}

/// Headless check for the commit path: builds the same demo annotation, then runs the real
/// export. Proves the crop is the right size, carries the drawing, and excludes the toolbar
/// and selection border — the three ways flattening can silently go wrong.
pub fn export_test(
    shot: &Screenshot,
    start: (i32, i32),
    cur: (i32, i32),
) -> Result<(Vec<u8>, i32, i32), String> {
    unsafe {
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
            return Err("failed to build source DIB".into());
        };
        let old_bright = SelectObject(bright_dc, bright_bmp);
        let old_back = SelectObject(back_dc, back_bmp);

        let sel = normalized(start, cur);
        let mut state = Overlay {
            back_dc,
            bright_dc,
            width: shot.width,
            height: shot.height,
            style: CrosshairStyle::Lines,
            dragging: false,
            start,
            cur,
            result: Some(sel),
            done: true,
            annotate: true,
            phase: Phase::Annotate,
            sel,
            cells: annotate::layout(sel, (shot.width, shot.height)),
            shapes: demo_shapes(sel),
            active: None,
            tool: Tool::Rect,
            color: 0,
            stroke: 1,
            keeper: false,
            typing: None,
            moving: None,
            exporting: false,
        };
        let pixels = export_annotated(&mut state);

        SelectObject(bright_dc, old_bright);
        SelectObject(back_dc, old_back);
        let _ = DeleteObject(bright_bmp);
        let _ = DeleteObject(back_bmp);
        let _ = DeleteDC(bright_dc);
        let _ = DeleteDC(back_dc);

        pixels
            .map(|p| (p, sel.2, sel.3))
            .ok_or_else(|| "export produced no pixels".to_string())
    }
}

fn demo_shapes((sx, sy, sw, sh): Rect) -> Vec<Shape> {
    let cell = sw / 6;
    let (top, bot) = (sy + sh / 4, sy + sh * 3 / 4);
    let at = |i: i32| sx + cell * i + cell / 6;
    let to = |i: i32| sx + cell * (i + 1) - cell / 6;
    let two = |tool, color, i: i32, a: i32, b: i32| {
        let mut s = Shape::new(tool, color, 4.0, (at(i), a));
        s.pts[1] = (to(i), b);
        s
    };
    let mut pen = Shape::new(Tool::Pen, PALETTE[2], 4.0, (at(4), top));
    pen.pts = (0..12)
        .map(|i| (at(4) + i * (to(4) - at(4)) / 11, if i % 2 == 0 { top } else { bot }))
        .collect();
    let mut text = Shape::new(Tool::Text, PALETTE[0], 4.0, (at(5), top));
    text.text = "Ducky!".encode_utf16().collect();
    vec![
        two(Tool::Rect, PALETTE[0], 0, top, bot),
        two(Tool::Arrow, PALETTE[0], 1, bot, top),
        two(Tool::Line, PALETTE[4], 2, bot, top),
        two(Tool::Circle, PALETTE[3], 3, top, bot),
        pen,
        text,
    ]
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
