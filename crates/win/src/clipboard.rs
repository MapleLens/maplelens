//! The clipboard's text, for pasting a Discord webhook URL into the settings window (which draws its own boxes, so
//! has no text entry of its own).

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

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
