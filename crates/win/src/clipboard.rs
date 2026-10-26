//! The clipboard's text, for pasting a webhook URL into the settings window (which draws its own boxes, so has no
//! text entry of its own), and for handing back a Telegram webhook URL the app completed.

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

/// Clipboard format of UTF-16 text.
const CF_UNICODETEXT: u32 = 13;
/// A webhook URL is about 120 characters; anything far bigger is not one and is not read.
const MAX_CHARS: usize = 4096;

/// The clipboard's text, or None when it holds none (or another program is using the clipboard).
pub fn text() -> Option<String> {
    // SAFETY: the clipboard is opened for this call only and closed on every path; the global memory block is locked
    // while read and holds a NUL-terminated UTF-16 string, as CF_UNICODETEXT promises, and the scan stops at the NUL or
    // at MAX_CHARS, whichever comes first.
    unsafe {
        IsClipboardFormatAvailable(CF_UNICODETEXT).ok()?;
        OpenClipboard(None).ok()?;
        let text = (|| {
            let handle: HANDLE = GetClipboardData(CF_UNICODETEXT).ok()?;
            let global = HGLOBAL(handle.0);
            let pointer = GlobalLock(global) as *const u16;
            if pointer.is_null() {
                return None;
            }
            let mut length = 0;
            while length < MAX_CHARS && *pointer.add(length) != 0 {
                length += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length));
            let _ = GlobalUnlock(global);
            Some(text)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// Puts `text` on the clipboard; false when another program holds it.
pub fn set_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: the clipboard is opened for this call only and closed on every path. The block is allocated movable, as
    // SetClipboardData requires, filled while locked with exactly `wide` (NUL included), and handed to the clipboard,
    // which owns it from then on; it is freed here only when the clipboard did not take it.
    unsafe {
        let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) else { return false };
        let pointer = GlobalLock(global) as *mut u16;
        if pointer.is_null() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
        let _ = GlobalUnlock(global);
        if OpenClipboard(None).is_err() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        let set = EmptyClipboard().is_ok() && SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global.0))).is_ok();
        if !set {
            let _ = GlobalFree(Some(global));
        }
        let _ = CloseClipboard();
        set
    }
}
