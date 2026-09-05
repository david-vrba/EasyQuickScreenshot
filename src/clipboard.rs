// Copies a capture to the Windows clipboard as CF_DIB so it pastes anywhere, and moves
// plain text in and out for the annotate text tool.
// Retries OpenClipboard briefly — another app may hold the clipboard lock.

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;

/// OpenClipboard fails while another app holds the lock; a few short retries cover it.
unsafe fn open_with_retry(hwnd: HWND) -> bool {
    for _ in 0..10 {
        if OpenClipboard(hwnd).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// Put UTF-16 text on the clipboard (a trailing NUL is added here).
pub fn copy_text(hwnd: HWND, text: &[u16]) -> Result<(), String> {
    let bytes = (text.len() + 1) * 2;
    unsafe {
        let hglobal: HGLOBAL =
            GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| format!("GlobalAlloc: {}", e))?;
        let ptr = GlobalLock(hglobal) as *mut u16;
        if ptr.is_null() {
            let _ = GlobalFree(hglobal);
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr, text.len());
        *ptr.add(text.len()) = 0;
        let _ = GlobalUnlock(hglobal);

        if !open_with_retry(hwnd) {
            let _ = GlobalFree(hglobal);
            return Err("clipboard is locked by another application".into());
        }
        let result = EmptyClipboard()
            .and_then(|_| SetClipboardData(CF_UNICODETEXT, HANDLE(hglobal.0)))
            .map(|_| ())
            .map_err(|e| format!("SetClipboardData: {}", e));
        let _ = CloseClipboard();
        if result.is_err() {
            let _ = GlobalFree(hglobal);
        }
        result
    }
}

/// Read UTF-16 text from the clipboard, without the trailing NUL. None if there is no text.
pub fn read_text(hwnd: HWND) -> Option<Vec<u16>> {
    unsafe {
        if !open_with_retry(hwnd) {
            return None;
        }
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|handle| {
            let hglobal = HGLOBAL(handle.0);
            let ptr = GlobalLock(hglobal) as *const u16;
            if ptr.is_null() {
                return None;
            }
            // Stop at the NUL, but never read past what the block actually holds.
            let max = GlobalSize(hglobal) / 2;
            let units = std::slice::from_raw_parts(ptr, max);
            let len = units.iter().position(|&u| u == 0).unwrap_or(max);
            let out = units[..len].to_vec();
            let _ = GlobalUnlock(hglobal);
            Some(out)
        });
        let _ = CloseClipboard();
        text
    }
}

pub fn copy_bgra(hwnd: HWND, bgra: &[u8], width: i32, height: i32) -> Result<(), String> {
    // CF_DIB = BITMAPINFOHEADER followed by bottom-up pixel rows.
    let header_size = 40usize; // sizeof(BITMAPINFOHEADER)
    let total = header_size + bgra.len();

    unsafe {
        let hglobal: HGLOBAL =
            GlobalAlloc(GMEM_MOVEABLE, total).map_err(|e| format!("GlobalAlloc: {}", e))?;
        let ptr = GlobalLock(hglobal) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(hglobal);
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

        if !open_with_retry(hwnd) {
            let _ = GlobalFree(hglobal);
            return Err("clipboard is locked by another application".into());
        }
        let result = EmptyClipboard()
            .and_then(|_| SetClipboardData(CF_DIB, HANDLE(hglobal.0)))
            .map(|_| ())
            .map_err(|e| format!("SetClipboardData: {}", e));
        let _ = CloseClipboard();
        // On success the clipboard owns hglobal — free it only on failure.
        if result.is_err() {
            let _ = GlobalFree(hglobal);
        }
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
