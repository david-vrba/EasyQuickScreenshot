// Screen capture via GDI BitBlt over the whole virtual desktop (all monitors).
// One capture = one DIB section; the overlay paints straight from it and the file writer
// crops out of it, so the screen is read once and the overlay can never appear in the output.

use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    HBITMAP, ROP_CODE, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Frozen snapshot of the entire virtual desktop, held in a top-down 32-bit DIB section.
///
/// The desktop is blitted straight into the DIB, so its pixels are readable in place: no
/// `GetDIBits` conversion into a separate buffer, and no second full-size copy for the
/// overlay, which selects this same bitmap to paint from. On a 6400x1440 desktop that is
/// 74 MB less to allocate on every hotkey press — what a machine short on memory feels most.
///
/// `origin_x/origin_y` map buffer (0,0) to virtual-screen coordinates (negative when a
/// monitor sits left of / above the primary).
pub struct Screenshot {
    bitmap: HBITMAP,
    bits: *const u8,
    pub width: i32,
    pub height: i32,
    pub origin_x: i32,
    pub origin_y: i32,
}

pub fn capture_virtual_screen() -> Result<Screenshot, String> {
    unsafe {
        let origin_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let origin_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if width <= 0 || height <= 0 {
            return Err("virtual screen has no size".into());
        }

        let screen_dc = GetDC(HWND::default());
        let info = top_down_info(width, height);
        let mut bits: *mut c_void = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(screen_dc, &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            ReleaseDC(HWND::default(), screen_dc);
            return Err("could not allocate the capture bitmap".into());
        };
        let mem_dc = CreateCompatibleDC(screen_dc);
        let old = SelectObject(mem_dc, bitmap);

        // CAPTUREBLT includes layered (per-pixel-alpha) windows in the capture.
        let blit = BitBlt(
            mem_dc,
            0,
            0,
            width,
            height,
            screen_dc,
            origin_x,
            origin_y,
            ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0),
        );
        // GDI batches drawing; the bits are only safe to read once the batch is flushed.
        let _ = GdiFlush();

        SelectObject(mem_dc, old);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(HWND::default(), screen_dc);

        if blit.is_err() {
            let _ = DeleteObject(bitmap);
            return Err("BitBlt failed (secure desktop or locked screen?)".into());
        }

        Ok(Screenshot {
            bitmap,
            bits: bits as *const u8,
            width,
            height,
            origin_x,
            origin_y,
        })
    }
}

/// A 32-bit DIB header with negative height, which makes the rows top-down like the screen.
pub fn top_down_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
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
    }
}

impl Screenshot {
    /// The bitmap itself, for a DC to paint from. It stays owned by the screenshot, and a
    /// bitmap can sit in only one DC at a time, so select it out again before this is used
    /// anywhere else.
    pub fn bitmap(&self) -> HBITMAP {
        self.bitmap
    }

    /// Top-down BGRA, `width * height * 4` bytes.
    pub fn pixels(&self) -> &[u8] {
        let len = self.width as usize * self.height as usize * 4;
        // The DIB lives until `drop`, and nothing draws into it after the capture flush.
        unsafe { std::slice::from_raw_parts(self.bits, len) }
    }

    /// Crop a rectangle given in buffer coordinates (not virtual-screen coordinates).
    /// Returns top-down BGRA rows of exactly w*h*4 bytes; clamps to the buffer.
    pub fn crop(&self, x: i32, y: i32, w: i32, h: i32) -> Option<(Vec<u8>, i32, i32)> {
        let x0 = x.clamp(0, self.width);
        let y0 = y.clamp(0, self.height);
        let x1 = (x + w).clamp(0, self.width);
        let y1 = (y + h).clamp(0, self.height);
        let (cw, ch) = (x1 - x0, y1 - y0);
        if cw <= 0 || ch <= 0 {
            return None;
        }
        let pixels = self.pixels();
        let stride = self.width as usize * 4;
        let mut out = Vec::with_capacity((cw * ch * 4) as usize);
        for row in y0..y1 {
            let start = row as usize * stride + x0 as usize * 4;
            out.extend_from_slice(&pixels[start..start + cw as usize * 4]);
        }
        Some((out, cw, ch))
    }
}

impl Drop for Screenshot {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.bitmap);
        }
    }
}
