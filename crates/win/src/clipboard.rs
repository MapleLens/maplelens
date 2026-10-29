//! The clipboard's text, for pasting a webhook URL into the settings window (which draws its own boxes, so has no
//! text entry of its own), and for handing back a Telegram webhook URL the app completed.

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};

/// Clipboard format of UTF-16 text.
const CF_UNICODETEXT: u32 = 13;
/// A webhook URL is about 120 characters; anything far bigger is not one and is not read.
const MAX_CHARS: usize = 4096;

/// The clipboard's text, or None when it holds none (or another program is using the clipboard).
pub fn text() -> Option<String> {
    // SAFETY: the clipboard is opened for this call only and closed on every path; the global memory block is locked
    // while read, and only as many units as the block holds (its size, asked, capped at MAX_CHARS) are read. The text
    // is not trusted to end in a NUL: any program can put it there, and this one may be running as administrator.
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
            let units = MAX_CHARS.min(GlobalSize(global) / 2);
            let text = up_to_nul(std::slice::from_raw_parts(pointer, units));
            let _ = GlobalUnlock(global);
            Some(text)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// The text in a clipboard block: up to its first NUL, or all of it when it has none.
fn up_to_nul(block: &[u16]) -> String {
    let length = block.iter().position(|&unit| unit == 0).unwrap_or(block.len());
    String::from_utf16_lossy(&block[..length])
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

#[cfg(test)]
mod tests {
    use super::up_to_nul;

    #[test]
    fn a_block_without_its_nul_is_read_to_its_end_and_no_further() {
        let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(up_to_nul(&[wide("https://x"), vec![0], wide("after")].concat()), "https://x");
        assert_eq!(up_to_nul(&wide("no terminator")), "no terminator");
        assert_eq!(up_to_nul(&[]), "");
    }
}
