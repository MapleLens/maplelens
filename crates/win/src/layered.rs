//! A per-pixel-alpha, click-through, always-on-top window that never takes focus.
//!
//! The game's tooltip is translucent while its text stays solid; that needs a layered window fed a
//! premultiplied BGRA bitmap (UpdateLayeredWindow).

use maplelens_core::geometry::Rect;
use maplelens_core::render::RgbaImage;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE,
    SetWindowDisplayAffinity, SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::w;

pub struct LayeredWindow {
    hwnd: HWND,
    rect: Option<Rect>,
    visible: bool,
}

impl LayeredWindow {
    pub fn new() -> windows::core::Result<Self> {
        // Clicks fall through (TRANSPARENT), it never takes focus (NOACTIVATE), and it has no taskbar button
        // (TOOLWINDOW). "Static" is a built-in window class, so no window procedure is needed.
        let style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE;
        // SAFETY: plain window creation with a system class; destroyed in Drop.
        let hwnd = unsafe {
            CreateWindowExW(style, w!("Static"), w!("MapleLens"), WS_POPUP, 0, 0, 1, 1, None, None, None, None)?
        };
        // Our own pixels must never be read back by the detection: a card drawn over the compare tooltip would
        // hide its title. Older Windows keeps the old behaviour (the call fails, nothing else breaks).
        // SAFETY: our own window.
        unsafe {
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        }
        Ok(Self { hwnd, rect: None, visible: false })
    }

    #[cfg(test)]
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[cfg(test)]
    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Show `image` with its top-left at screen (x, y).
    pub fn set_image(&mut self, image: &RgbaImage, x: i32, y: i32) {
        blit(self.hwnd, image, Some((x, y)));
        self.rect = Some(Rect::new(x, y, image.width as i32, image.height as i32));
        self.show();
    }

    /// Reposition without touching the bitmap (cheap: this runs every frame while the mouse moves).
    pub fn move_to(&mut self, x: i32, y: i32) {
        let Some(rect) = self.rect else { return };
        if (x, y) == (rect.x, rect.y) {
            return;
        }
        // SAFETY: our own window.
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
        self.rect = Some(Rect { x, y, ..rect });
        self.show();
    }

    pub fn hide(&mut self) {
        if self.visible {
            // SAFETY: our own window.
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }

    fn show(&mut self) {
        if !self.visible {
            // SAFETY: our own window; SHOWNOACTIVATE keeps focus on the game.
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.visible = true;
        }
    }
}

/// Give a layered window `image` as its per-pixel-alpha picture (and its size), its top-left moved to `at` if given.
pub fn blit(hwnd: HWND, image: &RgbaImage, at: Option<(i32, i32)>) {
    let (w, h) = (image.width as i32, image.height as i32);
    let pixels = image.premultiplied_bgra();
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let at = at.map(|(x, y)| POINT { x, y });
    // SAFETY: every GDI object created here is released before returning; the copy stays within the DIB.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        if let Ok(bitmap) = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
            let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                at.as_ref().map(std::ptr::from_ref),
                Some(&SIZE { cx: w, cy: h }),
                Some(memory),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(memory, previous);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
}

impl Drop for LayeredWindow {
    fn drop(&mut self) {
        // SAFETY: our own window, destroyed once.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
