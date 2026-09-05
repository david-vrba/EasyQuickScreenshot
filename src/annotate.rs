// Quick-annotate layer: shape model, GDI+ rendering, and the floating toolbar.
// Lives inside the capture overlay — no extra window, no second process. GDI+ is used
// (not plain GDI) purely for anti-aliasing and round caps; it is started lazily so the
// plain capture path never pays for it. Text goes through GDI in a second pass, which
// is why text always paints above the shapes.

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateRectRgn, DeleteObject, GetStockObject, GetTextExtentPoint32W, PatBlt,
    SelectClipRgn, SelectObject, SetBkMode, SetTextColor, TextOutW, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DEFAULT_PITCH, DSTINVERT, FW_BOLD,
    HDC, HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::Graphics::GdiPlus::{
    CombineModeReplace, FillModeAlternate, GdipCreateFromHDC, GdipCreatePen1, GdipCreateSolidFill,
    GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePen, GdipDrawCurveI, GdipDrawEllipseI,
    GdipDrawLineI, GdipDrawLinesI, GdipDrawRectangleI, GdipFillPolygonI, GdipFillRectangleI,
    GdipSetClipRectI, GdipSetPenEndCap, GdipSetPenLineJoin, GdipSetPenStartCap,
    GdipSetSmoothingMode, GdiplusStartup, GdiplusStartupInput, GpBrush, GpGraphics, GpPen,
    LineCapRound, LineJoinRound, Point, SmoothingModeAntiAlias, UnitPixel,
};

pub type Rect = (i32, i32, i32, i32);

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

/// How close (px) the pointer must be to a border to grab it for moving.
pub const GRAB_TOLERANCE: i32 = 7;

#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Rect,
    Arrow,
    Line,
    Circle,
    Pen,
    Text,
}

const TOOLS: [Tool; 6] = [
    Tool::Rect,
    Tool::Arrow,
    Tool::Line,
    Tool::Circle,
    Tool::Pen,
    Tool::Text,
];

#[derive(Clone)]
pub struct Shape {
    pub tool: Tool,
    pub color: u32,
    pub width: f32,
    /// Two points for every tool except Pen (the whole trail) and Text (one anchor).
    pub pts: Vec<(i32, i32)>,
    /// UTF-16 so it can go straight to TextOutW. Empty for everything but Text.
    pub text: Vec<u16>,
}

impl Shape {
    pub fn new(tool: Tool, color: u32, width: f32, at: (i32, i32)) -> Shape {
        let pts = if tool == Tool::Text { vec![at] } else { vec![at, at] };
        Shape { tool, color, width, pts, text: Vec::new() }
    }

    /// Re-point the shape being dragged. `constrain` is Shift: square, circle, or 45°.
    pub fn drag_to(&mut self, x: i32, y: i32, constrain: bool) {
        match self.tool {
            Tool::Text => return,
            Tool::Pen => {
                // Skip sub-pixel jitter so a long stroke stays a short point list.
                if self.pts.last().is_none_or(|p| (p.0 - x).abs() + (p.1 - y).abs() >= 2) {
                    self.pts.push((x, y));
                }
                return;
            }
            _ => {}
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

    /// A click that never became a drag (or text nobody typed) leaves nothing worth keeping.
    pub fn is_degenerate(&self) -> bool {
        match self.tool {
            Tool::Text => self.text.is_empty(),
            Tool::Pen => self.pts.len() < 2,
            _ => {
                let (a, b) = (self.pts[0], self.pts[1]);
                (a.0 - b.0).abs() < 3 && (a.1 - b.1).abs() < 3
            }
        }
    }

    pub fn move_by(&mut self, dx: i32, dy: i32) {
        for p in &mut self.pts {
            p.0 += dx;
            p.1 += dy;
        }
    }

    /// Axis-aligned bounds as (x0, y0, x1, y1), exclusive on the far side.
    pub unsafe fn bounds(&self) -> (i32, i32, i32, i32) {
        if self.tool == Tool::Text {
            let (w, h) = text_extent(&self.text, self.width);
            let a = self.pts[0];
            return (a.0, a.1, a.0 + w, a.1 + h);
        }
        let x0 = self.pts.iter().map(|p| p.0).min().unwrap_or(0);
        let y0 = self.pts.iter().map(|p| p.1).min().unwrap_or(0);
        let x1 = self.pts.iter().map(|p| p.0).max().unwrap_or(0);
        let y1 = self.pts.iter().map(|p| p.1).max().unwrap_or(0);
        (x0, y0, x1 + 1, y1 + 1)
    }

    /// Is the pointer on this shape's outline (or on the text box)? This is the grab test
    /// for moving, so it deliberately ignores the hollow interior of rectangles and circles.
    pub unsafe fn hits_border(&self, x: i32, y: i32) -> bool {
        let tol = (GRAB_TOLERANCE as f64).max(self.width as f64);
        match self.tool {
            Tool::Text => {
                let (x0, y0, x1, y1) = self.bounds();
                x >= x0 - 2 && x < x1 + 2 && y >= y0 - 2 && y < y1 + 2
            }
            Tool::Line | Tool::Arrow => segment_distance(self.pts[0], self.pts[1], (x, y)) <= tol,
            Tool::Pen => self
                .pts
                .windows(2)
                .any(|w| segment_distance(w[0], w[1], (x, y)) <= tol),
            Tool::Rect => {
                let (rx, ry, rw, rh) = bounds_of(self.pts[0], self.pts[1]);
                let corners = [(rx, ry), (rx + rw, ry), (rx + rw, ry + rh), (rx, ry + rh)];
                (0..4).any(|i| segment_distance(corners[i], corners[(i + 1) % 4], (x, y)) <= tol)
            }
            Tool::Circle => {
                let (rx, ry, rw, rh) = bounds_of(self.pts[0], self.pts[1]);
                let (a, b) = ((rw as f64 / 2.0).max(1.0), (rh as f64 / 2.0).max(1.0));
                let (cx, cy) = (rx as f64 + a, ry as f64 + b);
                // Normalised radial distance: 1.0 is exactly on the ellipse. Scaled back by the
                // smaller radius so the tolerance is roughly in pixels.
                let r = (((x as f64 - cx) / a).powi(2) + ((y as f64 - cy) / b).powi(2)).sqrt();
                (r - 1.0).abs() * a.min(b) <= tol
            }
        }
    }
}

fn segment_distance(a: (i32, i32), b: (i32, i32), p: (i32, i32)) -> f64 {
    let (ax, ay, bx, by, px, py) =
        (a.0 as f64, a.1 as f64, b.0 as f64, b.1 as f64, p.0 as f64, p.1 as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    let (qx, qy) = (ax + t * dx, ay + t * dy);
    ((px - qx).powi(2) + (py - qy).powi(2)).sqrt()
}

/// Is the pointer on the outline of `rect` (the capture region's border)?
pub fn hits_rect_border(rect: Rect, x: i32, y: i32) -> bool {
    let (rx, ry, rw, rh) = rect;
    let corners = [(rx, ry), (rx + rw, ry), (rx + rw, ry + rh), (rx, ry + rh)];
    let tol = GRAB_TOLERANCE as f64;
    (0..4).any(|i| segment_distance(corners[i], corners[(i + 1) % 4], (x, y)) <= tol)
}

fn bounds_of(a: (i32, i32), b: (i32, i32)) -> Rect {
    (
        a.0.min(b.0),
        a.1.min(b.1),
        (a.0 - b.0).abs(),
        (a.1 - b.1).abs(),
    )
}

// ---------------------------------------------------------------- text editing

/// The text being typed plus its one selection state. There is no cursor position: text
/// grows at the end, and Ctrl+A selects everything, which the next edit then replaces.
pub struct TextInput {
    pub text: Vec<u16>,
    pub all_selected: bool,
}

impl TextInput {
    pub fn new() -> TextInput {
        TextInput { text: Vec::new(), all_selected: false }
    }

    fn replace_selection(&mut self) {
        if self.all_selected {
            self.text.clear();
            self.all_selected = false;
        }
    }

    pub fn type_char(&mut self, ch: u16) {
        self.replace_selection();
        self.text.push(ch);
    }

    pub fn backspace(&mut self) {
        if self.all_selected {
            self.replace_selection();
        } else {
            self.text.pop();
        }
    }

    pub fn select_all(&mut self) {
        self.all_selected = !self.text.is_empty();
    }

    /// Clipboard text may span lines; the tool renders one line, so breaks become spaces.
    pub fn paste(&mut self, clip: &[u16]) {
        self.replace_selection();
        let mut last_space = self.text.last().is_none_or(|&u| u == ' ' as u16);
        for &u in clip {
            let u = if u == '\r' as u16 || u == '\n' as u16 || u == '\t' as u16 {
                ' ' as u16
            } else {
                u
            };
            if u == ' ' as u16 && last_space {
                continue;
            }
            last_space = u == ' ' as u16;
            self.text.push(u);
        }
    }

    /// Everything typed so far — Ctrl+C has no partial selection to be narrower than.
    pub fn copy(&self) -> Vec<u16> {
        self.text.clone()
    }

    pub fn cut(&mut self) -> Vec<u16> {
        let out = self.text.clone();
        self.text.clear();
        self.all_selected = false;
        out
    }
}

// ---------------------------------------------------------------- text (GDI)

/// Text size follows the stroke width so the wheel controls both.
fn font_px(width: f32) -> i32 {
    12 + (width * 4.0) as i32
}

unsafe fn make_font(width: f32) -> HFONT {
    CreateFontW(
        -font_px(width),
        0,
        0,
        0,
        FW_BOLD.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        DEFAULT_PITCH.0 as u32,
        w!("Segoe UI"),
    )
}

/// Measure without a window: a throwaway screen DC is enough for GDI text metrics.
unsafe fn text_extent(text: &[u16], width: f32) -> (i32, i32) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    let dc = GetDC(HWND::default());
    let font = make_font(width);
    let old = SelectObject(dc, font);
    let probe: Vec<u16> = if text.is_empty() { vec![' ' as u16] } else { text.to_vec() };
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, &probe, &mut size);
    SelectObject(dc, old);
    let _ = DeleteObject(font);
    ReleaseDC(HWND::default(), dc);
    (size.cx.max(4), size.cy.max(font_px(width)))
}

fn colorref(argb: u32) -> COLORREF {
    let (r, g, b) = ((argb >> 16) & 0xff, (argb >> 8) & 0xff, argb & 0xff);
    COLORREF((b << 16) | (g << 8) | r)
}

/// How the text being typed should be shown: with a caret, and inverted when select-all is on.
pub struct Typing<'a> {
    pub shape: &'a Shape,
    pub all_selected: bool,
}

unsafe fn draw_text(hdc: HDC, s: &Shape, typing: Option<&Typing>) {
    let font = make_font(s.width);
    let old = SelectObject(hdc, font);
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, colorref(s.color));
    let (x, y) = s.pts[0];
    if !s.text.is_empty() {
        let _ = TextOutW(hdc, x, y, &s.text);
    }
    if let Some(t) = typing {
        let (w, h) = {
            let mut size = SIZE::default();
            if s.text.is_empty() {
                (0, font_px(s.width))
            } else {
                let _ = GetTextExtentPoint32W(hdc, &s.text, &mut size);
                (size.cx, size.cy)
            }
        };
        if t.all_selected && w > 0 {
            // Inverting the text box is the classic "everything is selected" look and
            // stays visible on any background without needing alpha.
            let _ = PatBlt(hdc, x - 1, y, w + 2, h, DSTINVERT);
        }
        let bar: Vec<u16> = "|".encode_utf16().collect();
        let _ = TextOutW(hdc, x + w, y + (h - font_px(s.width)) / 2, &bar);
    }
    SelectObject(hdc, old);
    let _ = DeleteObject(font);
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

// ---------------------------------------------------------------- shape drawing

/// Paint every committed shape, the one being dragged, and the text being typed — all
/// clipped to the capture region so the preview never shows more than the file keeps.
/// Shapes are stroke-only by product rule; the arrow head is the sole filled element.
pub unsafe fn draw_shapes(
    hdc: HDC,
    clip: Rect,
    shapes: &[Shape],
    active: Option<&Shape>,
    typing: Option<Typing>,
) {
    if shapes.is_empty() && active.is_none() && typing.is_none() {
        return;
    }
    if let Some(canvas) = Canvas::new(hdc) {
        GdipSetClipRectI(canvas.0, clip.0, clip.1, clip.2, clip.3, CombineModeReplace);
        for s in shapes.iter().chain(active).filter(|s| s.tool != Tool::Text) {
            draw_one(canvas.0, s);
        }
    }
    // Text after GDI+ is torn down, so the two drawing stacks never share the DC.
    let rgn = CreateRectRgn(clip.0, clip.1, clip.0 + clip.2, clip.1 + clip.3);
    SelectClipRgn(hdc, rgn);
    for s in shapes.iter().filter(|s| s.tool == Tool::Text) {
        draw_text(hdc, s, None);
    }
    if let Some(t) = typing.as_ref() {
        draw_text(hdc, t.shape, Some(t));
    }
    SelectClipRgn(hdc, None);
    let _ = DeleteObject(rgn);
}

unsafe fn draw_one(g: *mut GpGraphics, s: &Shape) {
    match s.tool {
        Tool::Text => {}
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
            with_pen(s.color, s.width, |pen| GdipDrawLineI(g, pen, a.0, a.1, b.0, b.1));
        }
        Tool::Rect => {
            let (x, y, w, h) = bounds_of(s.pts[0], s.pts[1]);
            with_pen(s.color, s.width, |pen| GdipDrawRectangleI(g, pen, x, y, w, h));
        }
        Tool::Circle => {
            let (x, y, w, h) = bounds_of(s.pts[0], s.pts[1]);
            with_pen(s.color, s.width, |pen| GdipDrawEllipseI(g, pen, x, y, w, h));
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
        GdipDrawLineI(g, pen, tail.0, tail.1, notch.0.round() as i32, notch.1.round() as i32)
    });

    let poly = [
        pt(head.0, head.1),
        pt((base.0 + px * half).round() as i32, (base.1 + py * half).round() as i32),
        pt(notch.0.round() as i32, notch.1.round() as i32),
        pt((base.0 - px * half).round() as i32, (base.1 - py * half).round() as i32),
    ];
    with_brush(s.color, |brush| GdipFillPolygonI(g, brush, poly.as_ptr(), 4, FillModeAlternate));
}

// ---------------------------------------------------------------- toolbar

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Pick(Tool),
    Color(usize),
    Width(usize),
    /// `keeper` = a timestamped file in saved/ (the Save mode); otherwise temp.png (Quick).
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
const BTN: i32 = 50;
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
pub fn layout(sel: Rect, screen: (i32, i32)) -> Vec<Cell> {
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
            let label: Vec<u16> = if keeper { "SAVE" } else { "QUICK" }.encode_utf16().collect();
            let _ = TextOutW(hdc, cell.x + 7, cell.y + 7, &label);
        }
    }
    let hint: Vec<u16> =
        "R rect  A arrow  L line  C circle  P pen  T text (Ctrl+A/C/V/X while typing)   1-8 colour   wheel size   drag a border to move   Ctrl+Z undo   Enter quick   Shift+Enter save   Esc"
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
                text: Vec::new(),
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
        Tool::Text => with_pen(white, 2.0, |p| {
            GdipDrawLineI(g, p, x, y + 1, x + w, y + 1);
            GdipDrawLineI(g, p, x + w / 2, y + 1, x + w / 2, y + h)
        }),
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
        'T' => Some(Action::Pick(Tool::Text)),
        c @ '1'..='8' => Some(Action::Color(c as usize - '1' as usize)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(tool: Tool, a: (i32, i32), b: (i32, i32)) -> Shape {
        let mut s = Shape::new(tool, PALETTE[0], 4.0, a);
        s.pts[1] = b;
        s
    }

    #[test]
    fn shift_locks_a_square() {
        let mut s = shape(Tool::Rect, (10, 10), (10, 10));
        s.drag_to(60, 30, true);
        let (_, _, w, h) = bounds_of(s.pts[0], s.pts[1]);
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
        let mut s = Shape::new(Tool::Pen, 0, 2.0, (0, 0));
        s.pts.truncate(1);
        s.drag_to(1, 0, false); // 1px -> noise
        assert_eq!(s.pts.len(), 1);
        s.drag_to(0, 5, false);
        assert_eq!(s.pts.len(), 2);
    }

    #[test]
    fn a_click_without_a_drag_is_discarded() {
        assert!(shape(Tool::Rect, (5, 5), (6, 6)).is_degenerate());
        assert!(!shape(Tool::Rect, (5, 5), (60, 40)).is_degenerate());
        let mut t = Shape::new(Tool::Text, 0, 4.0, (5, 5));
        assert!(t.is_degenerate(), "empty text is nothing");
        t.text.push('x' as u16);
        assert!(!t.is_degenerate());
    }

    #[test]
    fn rectangle_grabs_on_its_outline_not_its_middle() {
        let s = shape(Tool::Rect, (100, 100), (300, 200));
        unsafe {
            assert!(s.hits_border(100, 150), "left edge");
            assert!(s.hits_border(200, 203), "just outside the bottom edge");
            assert!(!s.hits_border(200, 150), "hollow interior must not grab");
            assert!(!s.hits_border(400, 150), "far away");
        }
    }

    #[test]
    fn line_and_circle_grab_within_tolerance() {
        let l = shape(Tool::Line, (0, 0), (100, 0));
        unsafe {
            assert!(l.hits_border(50, 5));
            assert!(!l.hits_border(50, 20));
        }
        let c = shape(Tool::Circle, (0, 0), (200, 100)); // centre (100,50), radii 100/50
        unsafe {
            assert!(c.hits_border(200, 50), "rightmost point of the ellipse");
            assert!(c.hits_border(100, 3), "top of the ellipse");
            assert!(!c.hits_border(100, 50), "centre is hollow");
        }
    }

    #[test]
    fn region_border_grabs_only_near_the_edge() {
        let r = (100, 100, 400, 300);
        assert!(hits_rect_border(r, 100, 250));
        assert!(hits_rect_border(r, 503, 250));
        assert!(!hits_rect_border(r, 300, 250));
    }

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn select_all_then_typing_replaces_everything() {
        let mut t = TextInput::new();
        for c in u("hello") {
            t.type_char(c);
        }
        t.select_all();
        assert!(t.all_selected);
        t.type_char('x' as u16);
        assert_eq!(t.text, u("x"));
        assert!(!t.all_selected, "the replacement clears the selection");
    }

    #[test]
    fn backspace_deletes_one_or_all() {
        let mut t = TextInput::new();
        for c in u("abc") {
            t.type_char(c);
        }
        t.backspace();
        assert_eq!(t.text, u("ab"));
        t.select_all();
        t.backspace();
        assert!(t.text.is_empty());
        t.backspace(); // nothing left — must not panic
        assert!(t.text.is_empty());
    }

    #[test]
    fn select_all_on_empty_text_selects_nothing() {
        let mut t = TextInput::new();
        t.select_all();
        assert!(!t.all_selected);
    }

    #[test]
    fn paste_flattens_line_breaks_and_replaces_a_selection() {
        let mut t = TextInput::new();
        t.paste(&u("one\r\ntwo\tthree"));
        assert_eq!(t.text, u("one two three"));
        t.select_all();
        t.paste(&u("new"));
        assert_eq!(t.text, u("new"));
        t.paste(&u(" more"));
        assert_eq!(t.text, u("new more"), "paste appends when nothing is selected");
    }

    #[test]
    fn paste_does_not_double_spaces_at_the_join() {
        let mut t = TextInput::new();
        t.paste(&u("end "));
        t.paste(&u(" start"));
        assert_eq!(t.text, u("end start"));
    }

    #[test]
    fn cut_returns_the_text_and_empties_the_input() {
        let mut t = TextInput::new();
        t.paste(&u("gone"));
        assert_eq!(t.copy(), u("gone"));
        assert_eq!(t.cut(), u("gone"));
        assert!(t.text.is_empty());
        assert!(!t.all_selected);
    }

    #[test]
    fn move_shifts_every_point() {
        let mut s = shape(Tool::Line, (10, 10), (20, 30));
        s.move_by(5, -5);
        assert_eq!(s.pts, vec![(15, 5), (25, 25)]);
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
