//! Detection as platform-neutral state machines. The platform calls `step` in its own loop with a `Screen`
//! (how it captures pixels) and the game's client area, and shows whatever comes back:
//! - `hover::HoverDetector`: mouse position -> tooltip -> item title -> market item.
//! - `info::InfoDetector`: the Character Info window -> whose it is.

pub mod hover;
pub mod info;
pub mod map;

use crate::geometry::{Point, Rect};
use crate::image::RgbImage;

/// Where pixels and the mouse come from.
pub trait Screen {
    /// RGB pixels of a screen rectangle (parts outside the screen are black).
    fn grab(&mut self, area: Rect) -> RgbImage;
    /// The mouse in screen pixels, or None where it cannot be known (a browser capturing another window).
    fn cursor(&mut self) -> Option<Point>;
}

/// A screen made of one captured frame (the browser's view of the game window, or a test fixture).
pub struct FrameScreen<'a> {
    pub frame: &'a RgbImage,
    /// Screen position of the frame's top-left pixel.
    pub origin: Point,
    pub cursor: Option<Point>,
}

impl Screen for FrameScreen<'_> {
    fn grab(&mut self, area: Rect) -> RgbImage {
        let inside = area.x >= self.origin.x
            && area.y >= self.origin.y
            && (area.right() - self.origin.x) as usize <= self.frame.width
            && (area.bottom() - self.origin.y) as usize <= self.frame.height;
        if inside {
            let (x, y) = ((area.x - self.origin.x) as usize, (area.y - self.origin.y) as usize);
            return self.frame.crop(x, y, x + area.w.max(0) as usize, y + area.h.max(0) as usize);
        }
        let mut out = RgbImage::new(area.w.max(0) as usize, area.h.max(0) as usize);
        for y in 0..out.height {
            let fy = area.y + y as i32 - self.origin.y;
            if fy < 0 || fy as usize >= self.frame.height {
                continue;
            }
            for x in 0..out.width {
                let fx = area.x + x as i32 - self.origin.x;
                if fx >= 0 && (fx as usize) < self.frame.width {
                    out.set_pixel(x, y, self.frame.pixel(fx as usize, fy as usize));
                }
            }
        }
        out
    }

    fn cursor(&mut self) -> Option<Point> {
        self.cursor
    }
}

/// Grab `area`, then bring it to the resolution the vision constants were measured at.
pub fn grab_normalised(screen: &mut dyn Screen, area: Rect, scale: f32) -> RgbImage {
    let image = screen.grab(area);
    if (scale - 1.0).abs() > 0.01 { image.resized(1.0 / scale) } else { image }
}
