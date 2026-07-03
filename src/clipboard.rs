// Copies a capture to the Windows clipboard as CF_DIB so it pastes anywhere.
// Retries OpenClipboard briefly — another app may hold the clipboard lock.

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_DIB: u32 = 8;

pub fn copy_bgra(hwnd: HWND, bgra: &[u8], width: i32, height: i32) -> Result<(), String> {
    // CF_DIB = BITMAPINFOHEADER followed by bottom-up pixel rows.
    let header_size = 40usize; // sizeof(BITMAPINFOHEADER)
    let total = header_size + bgra.len();

    unsafe {
        let hglobal: HGLOBAL =
            GlobalAlloc(GMEM_MOVEABLE, total).map_err(|e| format!("GlobalAlloc: {}", e))?;
        let ptr = GlobalLock(hglobal) as *mut u8;
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }

        let header: [u8; 40] = build_bitmapinfoheader(width, height);
        std::ptr::copy_nonoverlapping(header.as_ptr(), ptr, header_size);

        // Flip top-down BGRA to the bottom-up order CF_DIB expects.
        let stride = width as usize * 4;
        let dst = ptr.add(header_size);
        for row in 0..height as usize {
            let src_row = &bgra[row * stride..(row + 1) * stride];
            let dst_row = dst.add((height as usize - 1 - row) * stride);
            std::ptr::copy_nonoverlapping(src_row.as_ptr(), dst_row, stride);
        }
        let _ = GlobalUnlock(hglobal);

        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(hwnd).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened {
            return Err("clipboard is locked by another application".into());
        }
        let result = EmptyClipboard()
            .and_then(|_| SetClipboardData(CF_DIB, HANDLE(hglobal.0)))
            .map(|_| ())
            .map_err(|e| format!("SetClipboardData: {}", e));
        let _ = CloseClipboard();
        // On success the clipboard owns hglobal — do not free it.
        result
    }
}

fn build_bitmapinfoheader(width: i32, height: i32) -> [u8; 40] {
    let mut h = [0u8; 40];
    h[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
    h[4..8].copy_from_slice(&width.to_le_bytes()); // biWidth
    h[8..12].copy_from_slice(&height.to_le_bytes()); // biHeight (positive = bottom-up)
    h[12..14].copy_from_slice(&1u16.to_le_bytes()); // biPlanes
    h[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount
    // biCompression = BI_RGB = 0, rest zero
    h
}
