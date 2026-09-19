// Focus mode: the list of on-screen windows, snapshotted at capture time.
// The overlay covers the screen, so nothing can be hit-tested live once it is up —
// the rectangles have to be collected before the window appears, then searched by point.

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible,
    GWL_EXSTYLE, WS_EX_TOOLWINDOW,
};

use crate::annotate::Rect;

/// Smallest window worth offering. Anything thinner is a sliver of chrome, not a target.
const MIN_PANE_PX: i32 = 40;

/// Every pickable window, topmost first, in buffer coordinates.
/// `origin` is where the capture buffer sits on the virtual desktop.
pub fn panes(origin: (i32, i32), width: i32, height: i32) -> Vec<Rect> {
    let mut found: Vec<Rect> = Vec::new();
    // EnumWindows walks top-level windows in z-order, front to back, which is exactly the
    // order a hit test wants: the first rectangle containing the point is the visible one.
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut found as *mut Vec<Rect> as isize));
    }
    found
        .into_iter()
        .filter_map(|r| clip(r, origin, width, height))
        .collect()
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if let Some(rect) = pickable(hwnd) {
        let list = &mut *(lparam.0 as *mut Vec<Rect>);
        list.push(rect);
    }
    TRUE
}

/// The window's visible rectangle in virtual-screen coordinates, or None if it is not
/// something a person would point at.
unsafe fn pickable(hwnd: HWND) -> Option<Rect> {
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || is_cloaked(hwnd) {
        return None;
    }
    if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return None;
    }
    if is_desktop(hwnd) {
        return None;
    }
    let r = frame_bounds(hwnd)?;
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    (w >= MIN_PANE_PX && h >= MIN_PANE_PX).then_some((r.left, r.top, w, h))
}

/// A UWP app that has been closed leaves its window alive but cloaked. `IsWindowVisible`
/// still reports true for those, so without this check the screen is covered in invisible
/// panes that steal every hit test.
unsafe fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut _,
        std::mem::size_of::<u32>() as u32,
    )
    .is_ok()
        && cloaked != 0
}

/// The desktop itself is a window. Highlighting it would put a pane over the whole screen
/// the moment the pointer left an app, which is not what "pick a window" means.
unsafe fn is_desktop(hwnd: HWND) -> bool {
    let mut buf = [0u16; 32];
    let n = GetClassNameW(hwnd, &mut buf) as usize;
    let class = String::from_utf16_lossy(&buf[..n]);
    class == "Progman" || class == "WorkerW"
}

/// `GetWindowRect` reports the resize border too, which on Windows 10/11 is several
/// transparent pixels outside what you can see. The DWM frame bounds are what the window
/// actually occupies, so a pane drawn on them lines up with its edges.
unsafe fn frame_bounds(hwnd: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    let ok = DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut r as *mut RECT as *mut _,
        std::mem::size_of::<RECT>() as u32,
    )
    .is_ok();
    if !ok && GetWindowRect(hwnd, &mut r).is_err() {
        return None;
    }
    Some(r)
}

/// Virtual-screen rectangle to buffer coordinates, trimmed to the captured area.
fn clip(rect: Rect, origin: (i32, i32), width: i32, height: i32) -> Option<Rect> {
    let x0 = (rect.0 - origin.0).max(0);
    let y0 = (rect.1 - origin.1).max(0);
    let x1 = (rect.0 - origin.0 + rect.2).min(width);
    let y1 = (rect.1 - origin.1 + rect.3).min(height);
    (x1 - x0 >= MIN_PANE_PX && y1 - y0 >= MIN_PANE_PX).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// The topmost pane under the pointer.
pub fn at(panes: &[Rect], x: i32, y: i32) -> Option<Rect> {
    panes
        .iter()
        .copied()
        .find(|r| x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3)
}

/// One step of the pane sliding from where it is drawn to the window now under the pointer.
/// Exponential easing: fast at first, settling in, which is how the Windows snap preview
/// moves. Returns the new drawn rectangle and whether it still has distance to cover.
pub fn ease(shown: (f32, f32, f32, f32), target: Rect, step: f32) -> ((f32, f32, f32, f32), bool) {
    let to = (target.0 as f32, target.1 as f32, target.2 as f32, target.3 as f32);
    let next = (
        shown.0 + (to.0 - shown.0) * step,
        shown.1 + (to.1 - shown.1) * step,
        shown.2 + (to.2 - shown.2) * step,
        shown.3 + (to.3 - shown.3) * step,
    );
    let far = (next.0 - to.0).abs().max((next.1 - to.1).abs())
        > 0.5
        || (next.2 - to.2).abs().max((next.3 - to.3).abs()) > 0.5;
    if far {
        (next, true)
    } else {
        (to, false) // snap, or it creeps toward the target forever
    }
}

pub fn to_rect(shown: (f32, f32, f32, f32)) -> Rect {
    (
        shown.0.round() as i32,
        shown.1.round() as i32,
        shown.2.round() as i32,
        shown.3.round() as i32,
    )
}

/// The area to repaint when the pane moves from one rectangle to another: both of them,
/// plus room for the border. Repainting only this is what keeps the animation smooth on a
/// multi-monitor desktop, where redrawing every pixel each frame costs tens of megabytes.
pub fn dirty(from: Rect, to: Rect, margin: i32) -> Rect {
    let x0 = from.0.min(to.0) - margin;
    let y0 = from.1.min(to.1) - margin;
    let x1 = (from.0 + from.2).max(to.0 + to.2) + margin;
    let y1 = (from.1 + from.3).max(to.1 + to.3) + margin;
    (x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_topmost_window_wins_an_overlap() {
        // EnumWindows hands them over front to back, so the first match is the one you see.
        let panes = [(100, 100, 400, 400), (0, 0, 800, 800)];
        assert_eq!(at(&panes, 200, 200), Some((100, 100, 400, 400)));
        assert_eq!(at(&panes, 700, 700), Some((0, 0, 800, 800)));
        assert_eq!(at(&panes, 900, 900), None);
    }

    #[test]
    fn a_window_hanging_off_the_screen_is_trimmed_not_dropped() {
        let clipped = clip((-200, -100, 800, 600), (0, 0), 1920, 1080);
        assert_eq!(clipped, Some((0, 0, 600, 500)), "only the visible part is offered");
        assert_eq!(clip((-790, 0, 800, 600), (0, 0), 1920, 1080), None, "a sliver is not a target");
    }

    #[test]
    fn a_monitor_left_of_the_primary_maps_into_the_buffer() {
        // David's desktop starts at x = -1920, so a window on the left monitor has a
        // negative screen x that has to become a positive buffer x.
        assert_eq!(
            clip((-1920, 182, 1920, 1080), (-1920, 0), 6400, 1440),
            Some((0, 182, 1920, 1080))
        );
    }

    #[test]
    fn the_pane_reaches_its_target_and_stops() {
        let mut shown = (0.0, 0.0, 100.0, 100.0);
        let target = (500, 300, 800, 600);
        let mut frames = 0;
        loop {
            let (next, moving) = ease(shown, target, 0.28);
            shown = next;
            frames += 1;
            if !moving {
                break;
            }
            assert!(frames < 120, "the ease never settles");
        }
        assert_eq!(to_rect(shown), target, "it lands exactly on the window");
        assert!(frames < 40, "and gets there inside half a second at 60fps, not eventually");
    }

    #[test]
    fn the_repaint_area_covers_where_the_pane_was_and_where_it_is_going() {
        let d = dirty((0, 0, 100, 100), (300, 200, 50, 50), 4);
        assert_eq!(d, (-4, -4, 358, 258));
    }
}
