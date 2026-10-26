//! The app icon: from the exe's own resources (embedded at build time), else from the bundled .ico bytes.

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, GetSystemMetrics, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXSMICON,
};
use windows::core::PCWSTR;

const ICO: &[u8] = include_bytes!("../assets/maplelens.ico");
/// winresource embeds the icon under this resource id.
const ICON_RESOURCE_ID: u16 = 1;

/// HICON at `size` px; the default size is the system's small icon (tray / title bar; DPI aware).
pub fn load(size: Option<i32>) -> HICON {
    // SAFETY: plain metric query and resource loads; a failed load returns a null icon, which Windows accepts.
    unsafe {
        let size = size.unwrap_or_else(|| GetSystemMetrics(SM_CXSMICON));
        if let Ok(module) = GetModuleHandleW(None)
            && let Ok(handle) = LoadImageW(
                Some(module.into()),
                PCWSTR(ICON_RESOURCE_ID as usize as *const u16),
                IMAGE_ICON,
                size,
                size,
                LR_DEFAULTCOLOR,
            )
        {
            return HICON(handle.0);
        }
        best_entry(size)
            .and_then(|(offset, len)| {
                CreateIconFromResourceEx(&ICO[offset..offset + len], true, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
                    .ok()
            })
            .unwrap_or_default()
    }
}

/// (offset, length) of the .ico image closest to `size` px.
fn best_entry(size: i32) -> Option<(usize, usize)> {
    let count = u16::from_le_bytes(ICO.get(4..6)?.try_into().ok()?) as usize;
    (0..count)
        .filter_map(|i| {
            let entry = ICO.get(6 + i * 16..6 + (i + 1) * 16)?;
            let width = if entry[0] == 0 { 256 } else { entry[0] as i32 };
            let len = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
            (offset + len <= ICO.len()).then_some(((width - size).abs(), offset, len))
        })
        .min_by_key(|e| e.0)
        .map(|(_, offset, len)| (offset, len))
}
