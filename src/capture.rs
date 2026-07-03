// Screen capture via GDI BitBlt over the whole virtual desktop (all monitors).
// One capture = one BGRA pixel buffer; the overlay and the file writer both work from it,
// so the screen is only read once and the overlay itself can never appear in the output.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    ROP_CODE, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Frozen snapshot of the entire virtual desktop.
/// `pixels` is top-down BGRA, `width * height * 4` bytes.
/// `origin_x/origin_y` map buffer (0,0) to virtual-screen coordinates
/// (negative when a monitor sits left of / above the primary).
pub struct Screenshot {
    pub pixels: Vec<u8>,
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
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
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

        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // negative = top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(HWND::default(), screen_dc);

        if blit.is_err() {
            return Err("BitBlt failed (secure desktop or locked screen?)".into());
        }
        if lines != height {
            return Err("GetDIBits returned incomplete image".into());
        }

        Ok(Screenshot {
            pixels,
            width,
            height,
            origin_x,
            origin_y,
        })
    }
}

impl Screenshot {
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
        let stride = self.width as usize * 4;
        let mut out = Vec::with_capacity((cw * ch * 4) as usize);
        for row in y0..y1 {
            let start = row as usize * stride + x0 as usize * 4;
            out.extend_from_slice(&self.pixels[start..start + cw as usize * 4]);
        }
        Some((out, cw, ch))
    }
}
