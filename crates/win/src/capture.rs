//! Screen capture: copies of what is on the screen (GDI BitBlt from the desktop), like a screenshot tool.
//! Our own overlay windows are left out: plain SRCCOPY skips layered windows, and they also ask Windows to
//! keep them out of captures (see `layered`).

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::RgbImage;
use maplelens_core::pipeline::Screen;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};

use crate::game;

/// Grabs screen rectangles; reuses its DIB while the size stays the same (the tooltip grab is the same size
/// every frame).
pub struct GdiScreen {
    memory_dc: HDC,
    bitmap: Option<(HBITMAP, *mut u8, i32, i32)>,
}

// SAFETY: the DCs and bitmap belong to this value alone and are only used from the thread that owns it.
unsafe impl Send for GdiScreen {}

impl GdiScreen {
    pub fn new() -> Self {
        // SAFETY: a memory DC compatible with the screen; released in Drop.
        let memory_dc = unsafe { CreateCompatibleDC(None) };
        Self { memory_dc, bitmap: None }
    }

    fn surface(&mut self, w: i32, h: i32) -> Option<*mut u8> {
        if let Some((_, bits, bw, bh)) = self.bitmap
            && (bw, bh) == (w, h)
        {
            return Some(bits);
        }
        self.release_bitmap();
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: valid header; the bitmap is deleted in release_bitmap.
        let bitmap = unsafe { CreateDIBSection(Some(self.memory_dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()? };
        // SAFETY: selecting our own bitmap into our own DC.
        unsafe { SelectObject(self.memory_dc, HGDIOBJ(bitmap.0)) };
        self.bitmap = Some((bitmap, bits.cast(), w, h));
        Some(bits.cast())
    }

    fn release_bitmap(&mut self) {
        if let Some((bitmap, ..)) = self.bitmap.take() {
            // SAFETY: the bitmap was created by us; deleting it while selected is allowed (it is freed when
            // deselected), and the next selection replaces it.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
        }
    }
}

impl Default for GdiScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GdiScreen {
    fn drop(&mut self) {
        self.release_bitmap();
        // SAFETY: our own DC.
        unsafe {
            let _ = DeleteDC(self.memory_dc);
        }
    }
}

impl Screen for GdiScreen {
    fn grab(&mut self, area: Rect) -> RgbImage {
        let (w, h) = (area.w.max(0), area.h.max(0));
        if w == 0 || h == 0 {
            return RgbImage::new(w as usize, h as usize);
        }
        let Some(bits) = self.surface(w, h) else { return RgbImage::new(w as usize, h as usize) };
        // SAFETY: the screen DC is released right after the copy; `bits` points at w*h*4 bytes owned by the DIB.
        unsafe {
            let screen = GetDC(None);
            let _ = BitBlt(self.memory_dc, 0, 0, w, h, Some(screen), area.x, area.y, SRCCOPY);
            ReleaseDC(None, screen);
            let pixels = std::slice::from_raw_parts(bits, (w * h * 4) as usize);
            RgbImage::from_4ch(w as usize, h as usize, pixels, true)
        }
    }

    fn cursor(&mut self) -> Option<Point> {
        game::cursor()
    }
}
