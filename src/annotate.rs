// Quick-annotate layer: shape model, GDI+ rendering, and the floating toolbar.
// Lives inside the capture overlay — no extra window, no second process. GDI+ is used
// (not plain GDI) purely for anti-aliasing and round caps; it is started lazily so the
// plain capture path never pays for it.

use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    GetStockObject, SetBkMode, SetTextColor, TextOutW, DEFAULT_GUI_FONT, HDC, SelectObject,
    TRANSPARENT,
};
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipCreatePen1, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics,
    GdipDeletePen, GdipDrawCurveI, GdipDrawEllipseI, GdipDrawLineI, GdipDrawLinesI,
    GdipDrawRectangleI, GdipFillPolygonI, GdipFillRectangleI, GdipSetPenEndCap, GdipSetPenLineJoin,
    GdipSetPenStartCap, GdipSetSmoothingMode, GdiplusStartup, FillModeAlternate, GdiplusStartupInput,
    GpBrush, GpGraphics, GpPen, LineCapRound, LineJoinRound, Point, SmoothingModeAntiAlias, UnitPixel,
};

/// Stroke colours, ARGB. Index 0 (red) is the default and never moves.
pub const PALETTE: [u32; 8] = [
    0xFFFF3B30, // red
    0xFFFF9500, // orange
    0xFFFFCC00, // yellow
    0xFF34C759, // green
    0xFF007AFF, // blue
    0xFFAF52DE, // purple
    0xFF000000, // black
    0xFFFFFFFF, // white
];
pub const WIDTHS: [f32; 3] = [2.0, 4.0, 6.0];

#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Rect,
    Arrow,
    Line,
    Circle,
    Pen,
}

const TOOLS: [Tool; 5] = [Tool::Rect, Tool::Arrow, Tool::Line, Tool::Circle, Tool::Pen];

#[derive(Clone)]
pub struct Shape {
    pub tool: Tool,
    pub color: u32,
    pub width: f32,
    /// Two points for every tool except Pen, which stores the whole freehand trail.
    pub pts: Vec<(i32, i32)>,
}

impl Shape {
    /// Re-point the shape being dragged. `constrain` is Shift: square, circle, or 45°.
    pub fn drag_to(&mut self, x: i32, y: i32, constrain: bool) {
        if self.tool == Tool::Pen {
            // Skip sub-pixel jitter so a long stroke stays a short point list.
            if self.pts.last().is_none_or(|p| (p.0 - x).abs() + (p.1 - y).abs() >= 2) {
                self.pts.push((x, y));
            }
            return;
        }
        let a = self.pts[0];
        let (mut dx, mut dy) = (x - a.0, y - a.1);
        if constrain {
            match self.tool {
                Tool::Line | Tool::Arrow => {
                    // Snap the vector to the nearest 45°, keeping its length.
                    let len = ((dx * dx + dy * dy) as f64).sqrt();
                    let step = std::f64::consts::FRAC_PI_4;
                    let angle = ((dy as f64).atan2(dx as f64) / step).round() * step;
                    dx = (angle.cos() * len).round() as i32;
                    dy = (angle.sin() * len).round() as i32;
                }
                _ => {
                    let side = dx.abs().max(dy.abs());
                    dx = side * dx.signum();
                    dy = side * dy.signum();
                }
            }
        }
        self.pts[1] = (a.0 + dx, a.1 + dy);
    }

    /// A click that never became a drag leaves nothing worth keeping.
    pub fn is_degenerate(&self) -> bool {
        match self.tool {
            Tool::Pen => self.pts.len() < 2,
            _ => {
                let (a, b) = (self.pts[0], self.pts[1]);
                (a.0 - b.0).abs() < 3 && (a.1 - b.1).abs() < 3
            }
        }
    }
}

// ---------------------------------------------------------------- GDI+ plumbing

/// GDI+ must be started once per process. Deliberately never shut down: the token would
/// outlive nothing useful, and Windows reclaims it at exit.
unsafe fn ensure_gdiplus() {
    use std::sync::Once;
    static START: Once = Once::new();
    START.call_once(|| {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        let mut token = 0usize;
        let _ = GdiplusStartup(&mut token, &input, std::ptr::null_mut());
    });
}

struct Canvas(*mut GpGraphics);

impl Canvas {
    unsafe fn new(hdc: HDC) -> Option<Canvas> {
        ensure_gdiplus();
        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipCreateFromHDC(hdc, &mut g) != windows::Win32::Graphics::GdiPlus::Ok || g.is_null() {
            return None;
        }
        GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
        Some(Canvas(g))
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe { GdipDeleteGraphics(self.0) };
    }
}

unsafe fn with_pen<T>(color: u32, width: f32, f: impl FnOnce(*mut GpPen) -> T) {
    let mut pen: *mut GpPen = std::ptr::null_mut();
    if GdipCreatePen1(color, width, UnitPixel, &mut pen) != windows::Win32::Graphics::GdiPlus::Ok {
        return;
    }
    // Round caps and joins are what stop strokes reading as MS-Paint output.
    GdipSetPenStartCap(pen, LineCapRound);
    GdipSetPenEndCap(pen, LineCapRound);
    GdipSetPenLineJoin(pen, LineJoinRound);
    let _ = f(pen);
    GdipDeletePen(pen);
}

unsafe fn with_brush<T>(color: u32, f: impl FnOnce(*mut GpBrush) -> T) {
    let mut brush = std::ptr::null_mut();
    if GdipCreateSolidFill(color, &mut brush) != windows::Win32::Graphics::GdiPlus::Ok {
        return;
    }
    let _ = f(brush as *mut GpBrush);
    GdipDeleteBrush(brush as *mut GpBrush);
}

fn pt(x: i32, y: i32) -> Point {
    Point { X: x, Y: y }
}

fn bounds(a: (i32, i32), b: (i32, i32)) -> (i32, i32, i32, i32) {
    (
        a.0.min(b.0),
        a.1.min(b.1),
        (a.0 - b.0).abs(),
        (a.1 - b.1).abs(),
    )
}

// ---------------------------------------------------------------- shape drawing

/// Paint every committed shape plus the one being dragged. Shapes are stroke-only by
/// product rule; the arrow head is the sole filled element (a hollow head reads as a "V").
pub unsafe fn draw_shapes(hdc: HDC, shapes: &[Shape], active: Option<&Shape>) {
    if shapes.is_empty() && active.is_none() {
        return;
    }
    let Some(canvas) = Canvas::new(hdc) else { return };
    for s in shapes.iter().chain(active) {
        draw_one(canvas.0, s);
    }
}

unsafe fn draw_one(g: *mut GpGraphics, s: &Shape) {
    match s.tool {
        Tool::Pen => {
            let pts: Vec<Point> = s.pts.iter().map(|&(x, y)| pt(x, y)).collect();
            if pts.len() < 2 {
                return;
            }
            with_pen(s.color, s.width, |pen| {
                // A cardinal spline through the samples: smooths the hand shake that
                // makes raw polylines look jagged.
                if pts.len() > 2 {
                    GdipDrawCurveI(g, pen, pts.as_ptr(), pts.len() as i32);
                } else {
                    GdipDrawLinesI(g, pen, pts.as_ptr(), pts.len() as i32);
                }
            });
        }
        Tool::Line => {
            let (a, b) = (s.pts[0], s.pts[1]);
            with_pen(s.color, s.width, |pen| {
                GdipDrawLineI(g, pen, a.0, a.1, b.0, b.1);
            });
        }
        Tool::Rect => {
            let (x, y, w, h) = bounds(s.pts[0], s.pts[1]);
            with_pen(s.color, s.width, |pen| {
                GdipDrawRectangleI(g, pen, x, y, w, h);
            });
        }
        Tool::Circle => {
            let (x, y, w, h) = bounds(s.pts[0], s.pts[1]);
            with_pen(s.color, s.width, |pen| {
                GdipDrawEllipseI(g, pen, x, y, w, h);
            });
        }
        Tool::Arrow => draw_arrow(g, s),
    }
}

/// Tapered shaft + a concave four-point head. The notch is what separates it from the
/// flat triangle every default arrow tool draws.
unsafe fn draw_arrow(g: *mut GpGraphics, s: &Shape) {
    let (tail, head) = (s.pts[0], s.pts[1]);
    let (dx, dy) = ((head.0 - tail.0) as f64, (head.1 - tail.1) as f64);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let (px, py) = (-uy, ux);

    let head_len = (s.width as f64 * 6.0).max(16.0).min(len);
    let half = head_len * 0.46;
    let base = (head.0 as f64 - ux * head_len, head.1 as f64 - uy * head_len);
    let notch = (head.0 as f64 - ux * head_len * 0.62, head.1 as f64 - uy * head_len * 0.62);

    // Stop the shaft inside the head so the two never show a seam.
    with_pen(s.color, s.width, |pen| {
        GdipDrawLineI(g, pen, tail.0, tail.1, notch.0.round() as i32, notch.1.round() as i32);
    });

    let poly = [
        pt(head.0, head.1),
        pt((base.0 + px * half).round() as i32, (base.1 + py * half).round() as i32),
        pt(notch.0.round() as i32, notch.1.round() as i32),
        pt((base.0 - px * half).round() as i32, (base.1 - py * half).round() as i32),
    ];
    with_brush(s.color, |brush| {
        GdipFillPolygonI(g, brush, poly.as_ptr(), 4, FillModeAlternate);
    });
}

// ---------------------------------------------------------------- toolbar

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Pick(Tool),
    Color(usize),
    Width(usize),
    Commit { keeper: bool },
}

pub struct Cell {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub action: Action,
}

impl Cell {
    fn hit(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

const CELL: i32 = 28;
const SWATCH: i32 = 22;
const WCELL: i32 = 20;
const BTN: i32 = 46;
const PAD: i32 = 8;
const GAP: i32 = 12;
pub const BAR_H: i32 = CELL + PAD * 2;
const HINT_H: i32 = 18;

fn bar_width() -> i32 {
    PAD * 2
        + CELL * TOOLS.len() as i32
        + GAP
        + SWATCH * PALETTE.len() as i32
        + GAP
        + WCELL * WIDTHS.len() as i32
        + GAP
        + BTN * 2
}

/// Place the bar under the selection, flipping above it (or inside it) when the screen
/// edge is in the way, and clamp so it is always fully on-screen.
pub fn layout(sel: (i32, i32, i32, i32), screen: (i32, i32)) -> Vec<Cell> {
    let bw = bar_width();
    let x = (sel.0).clamp(0, (screen.0 - bw).max(0));
    let below = sel.1 + sel.3 + 10;
    let y = if below + BAR_H + HINT_H <= screen.1 {
        below
    } else if sel.1 - BAR_H - 10 >= 0 {
        sel.1 - BAR_H - 10
    } else {
        (screen.1 - BAR_H - HINT_H).max(0)
    };

    let mut cells = Vec::new();
    let mut cx = x + PAD;
    let cy = y + PAD;
    for &t in TOOLS.iter() {
        cells.push(Cell { x: cx, y: cy, w: CELL, h: CELL, action: Action::Pick(t) });
        cx += CELL;
    }
    cx += GAP;
    for i in 0..PALETTE.len() {
        cells.push(Cell { x: cx, y: cy + 3, w: SWATCH, h: CELL - 6, action: Action::Color(i) });
        cx += SWATCH;
    }
    cx += GAP;
    for i in 0..WIDTHS.len() {
        cells.push(Cell { x: cx, y: cy, w: WCELL, h: CELL, action: Action::Width(i) });
        cx += WCELL;
    }
    cx += GAP;
    cells.push(Cell { x: cx, y: cy, w: BTN, h: CELL, action: Action::Commit { keeper: false } });
    cx += BTN;
    cells.push(Cell { x: cx, y: cy, w: BTN, h: CELL, action: Action::Commit { keeper: true } });
    cells
}

pub fn hit_test(cells: &[Cell], x: i32, y: i32) -> Option<Action> {
    cells.iter().find(|c| c.hit(x, y)).map(|c| c.action)
}

/// True while the pointer is over the bar, so a drag started there never draws a shape.
pub fn over_bar(cells: &[Cell], x: i32, y: i32) -> bool {
    let Some(first) = cells.first() else { return false };
    let last = cells.last().unwrap();
    let (x0, y0) = (first.x - PAD, first.y - PAD);
    let (x1, y1) = (last.x + last.w + PAD, first.y + CELL + PAD);
    x >= x0 && x < x1 && y >= y0 && y < y1
}

pub unsafe fn draw_toolbar(hdc: HDC, cells: &[Cell], tool: Tool, color: usize, width: usize) {
    let Some(first) = cells.first() else { return };
    let last = cells.last().unwrap();
    let (bx, by) = (first.x - PAD, first.y - PAD);
    let bw = last.x + last.w + PAD - bx;

    if let Some(canvas) = Canvas::new(hdc) {
        let g = canvas.0;
        with_brush(0xE6141414, |b| GdipFillRectangleI(g, b, bx, by, bw, BAR_H));
        with_pen(0x40FFFFFF, 1.0, |p| GdipDrawRectangleI(g, p, bx, by, bw - 1, BAR_H - 1));

        for cell in cells {
            let selected = matches!(
                (cell.action, tool),
                (Action::Pick(t), cur) if t == cur
            ) || cell.action == Action::Color(color)
                || cell.action == Action::Width(width);
            if selected {
                with_brush(0x40FFFFFF, |b| {
                    GdipFillRectangleI(g, b, cell.x, cell.y - 2, cell.w, cell.h + 4)
                });
            }
            match cell.action {
                Action::Pick(t) => draw_tool_glyph(g, t, cell),
                Action::Color(i) => {
                    let (sx, sy, sw, sh) = (cell.x + 4, cell.y + 4, cell.w - 8, cell.h - 8);
                    with_brush(PALETTE[i], |b| GdipFillRectangleI(g, b, sx, sy, sw, sh));
                    // Without an outline the black swatch disappears into the bar.
                    with_pen(0x59FFFFFF, 1.0, |p| GdipDrawRectangleI(g, p, sx, sy, sw - 1, sh - 1));
                }
                Action::Width(i) => {
                    let d = 3 + i as i32 * 3;
                    with_brush(0xFFFFFFFF, |b| {
                        GdipFillRectangleI(
                            g,
                            b,
                            cell.x + (cell.w - d) / 2,
                            cell.y + (cell.h - d) / 2,
                            d,
                            d,
                        )
                    });
                }
                Action::Commit { .. } => {}
            }
        }
    }

    // Text after GDI+ is torn down, so the two drawing stacks never share the DC.
    let old_font = SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT));
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, COLORREF(0x00FFFFFF));
    for cell in cells {
        if let Action::Commit { keeper } = cell.action {
            let label: Vec<u16> = if keeper { "KEEP" } else { "SAVE" }.encode_utf16().collect();
            let _ = TextOutW(hdc, cell.x + 7, cell.y + 7, &label);
        }
    }
    let hint: Vec<u16> =
        "R rect  A arrow  L line  C circle  P pen   1-8 colour   wheel width   Ctrl+Z undo   Enter save   Shift+Enter keep   Esc cancel"
            .encode_utf16()
            .collect();
    SetTextColor(hdc, COLORREF(0x00000000));
    let _ = TextOutW(hdc, bx + 2, by + BAR_H + 4, &hint);
    SetTextColor(hdc, COLORREF(0x00FFFFFF));
    let _ = TextOutW(hdc, bx + 1, by + BAR_H + 3, &hint);
    SelectObject(hdc, old_font);
}

/// Each tool's icon is the shape it draws — no icon assets, and it stays honest.
unsafe fn draw_tool_glyph(g: *mut GpGraphics, tool: Tool, c: &Cell) {
    let (x, y, w, h) = (c.x + 7, c.y + 7, c.w - 14, c.h - 14);
    let white = 0xFFFFFFFF;
    match tool {
        Tool::Rect => with_pen(white, 1.6, |p| GdipDrawRectangleI(g, p, x, y, w, h)),
        Tool::Circle => with_pen(white, 1.6, |p| GdipDrawEllipseI(g, p, x, y, w, h)),
        Tool::Line => with_pen(white, 1.6, |p| GdipDrawLineI(g, p, x, y + h, x + w, y)),
        Tool::Arrow => draw_one(
            g,
            &Shape {
                tool: Tool::Arrow,
                color: white,
                width: 1.6,
                pts: vec![(x, y + h), (x + w, y)],
            },
        ),
        Tool::Pen => {
            let pts = [
                pt(x, y + h),
                pt(x + w / 3, y + h / 3),
                pt(x + w * 2 / 3, y + h),
                pt(x + w, y),
            ];
            with_pen(white, 1.6, |p| GdipDrawCurveI(g, p, pts.as_ptr(), 4));
        }
    }
}

/// Keyboard shortcuts for the bar. Returns None for keys the overlay handles itself.
pub fn key_action(vk: u16) -> Option<Action> {
    match vk as u8 as char {
        'R' => Some(Action::Pick(Tool::Rect)),
        'A' => Some(Action::Pick(Tool::Arrow)),
        'L' => Some(Action::Pick(Tool::Line)),
        'C' => Some(Action::Pick(Tool::Circle)),
        'P' => Some(Action::Pick(Tool::Pen)),
        c @ '1'..='8' => Some(Action::Color(c as usize - '1' as usize)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(tool: Tool, a: (i32, i32), b: (i32, i32)) -> Shape {
        Shape { tool, color: PALETTE[0], width: 4.0, pts: vec![a, b] }
    }

    #[test]
    fn shift_locks_a_square() {
        let mut s = shape(Tool::Rect, (10, 10), (10, 10));
        s.drag_to(60, 30, true);
        let (_, _, w, h) = bounds(s.pts[0], s.pts[1]);
        assert_eq!(w, h, "constrained rectangle must be square");
    }

    #[test]
    fn shift_snaps_a_line_to_45_degrees() {
        let mut s = shape(Tool::Line, (0, 0), (0, 0));
        s.drag_to(100, 10, true); // shallow angle -> snaps to horizontal
        assert_eq!(s.pts[1].1, 0);
        s.drag_to(100, 90, true); // near-diagonal -> snaps to exactly 45
        let (dx, dy) = (s.pts[1].0, s.pts[1].1);
        assert_eq!(dx, dy);
    }

    #[test]
    fn pen_skips_jitter_but_keeps_real_movement() {
        let mut s = Shape { tool: Tool::Pen, color: 0, width: 2.0, pts: vec![(0, 0)] };
        s.drag_to(1, 0, false); // 1px -> noise
        assert_eq!(s.pts.len(), 1);
        s.drag_to(0, 5, false);
        assert_eq!(s.pts.len(), 2);
    }

    #[test]
    fn a_click_without_a_drag_is_discarded() {
        assert!(shape(Tool::Rect, (5, 5), (6, 6)).is_degenerate());
        assert!(!shape(Tool::Rect, (5, 5), (60, 40)).is_degenerate());
    }

    #[test]
    fn toolbar_stays_on_screen_when_the_selection_hugs_an_edge() {
        let screen = (1920, 1080);
        for sel in [(0, 0, 100, 100), (1900, 1040, 20, 40), (960, 540, 400, 300)] {
            let cells = layout(sel, screen);
            let first = cells.first().unwrap();
            let last = cells.last().unwrap();
            assert!(first.x - PAD >= 0, "bar off the left for {:?}", sel);
            assert!(last.x + last.w + PAD <= screen.0, "bar off the right for {:?}", sel);
            assert!(first.y - PAD >= 0, "bar off the top for {:?}", sel);
            assert!(first.y + CELL + PAD <= screen.1, "bar off the bottom for {:?}", sel);
        }
    }

    #[test]
    fn every_cell_is_reachable_by_click() {
        let cells = layout((100, 100, 800, 400), (1920, 1080));
        for c in &cells {
            let hit = hit_test(&cells, c.x + c.w / 2, c.y + c.h / 2);
            assert!(hit.is_some(), "cell not hittable at its own centre");
        }
        assert!(hit_test(&cells, 0, 0).is_none());
    }
}
