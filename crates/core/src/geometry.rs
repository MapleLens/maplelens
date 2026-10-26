//! Tiny geometry primitives shared by capture, vision and overlay.

/// Axis-aligned rectangle in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

/// A point in screen pixels (the mouse, a window corner).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Every pixel constant in `config` was measured at this client height (1920x1200). The game draws its UI
/// proportionally to the window height (measured at 2560x1440: equipment tooltip 442 px = 370 * 1.2, compare
/// tooltip offset 442 = 373 * 1.2), so other resolutions scale those numbers by height / this.
pub const REFERENCE_HEIGHT: i32 = 1200;

/// How much bigger (or smaller) than at the measured resolution the game draws its UI in `client`.
pub fn ui_scale(client: Rect) -> f32 {
    client.h as f32 / REFERENCE_HEIGHT as f32
}

/// The UI scale the game draws at, measured off its status bar (`vision::hud`) by one detector and read by
/// the others.
///
/// The window height alone is wrong in "UI Size: Ideal Ratio", where the UI stays the same size at any window
/// size, so `ui_scale` is only the fallback until the bar has been measured for this client size (or while it is
/// hidden).
#[derive(Debug, Default)]
pub struct UiScale {
    measured: std::sync::Mutex<Option<((i32, i32), f32)>>,
}

impl UiScale {
    pub fn of(&self, client: Rect) -> f32 {
        match *self.measured.lock().unwrap_or_else(|e| e.into_inner()) {
            Some((size, scale)) if size == (client.w, client.h) => scale,
            _ => ui_scale(client),
        }
    }

    pub fn set(&self, client: Rect, scale: f32) {
        *self.measured.lock().unwrap_or_else(|e| e.into_inner()) = Some(((client.w, client.h), scale));
    }
}

/// A measured pixel constant at UI scale `scale`, rounded to the nearest pixel.
pub fn scaled(value: i32, scale: f32) -> i32 {
    (value as f32 * scale).round() as i32
}
