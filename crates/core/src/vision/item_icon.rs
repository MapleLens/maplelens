//! Which item an icon on screen is, out of a set of known icons (pure).
//!
//! Icons are compared in colour over their opaque pixels, slid across the area where the item is drawn: shape alone
//! does not do (the Red, Orange and White Potions are one bottle in three colours), and colour does. A count drawn
//! over the icon's corner, or the slot behind its see-through edges, costs every candidate alike.

use std::sync::Arc;

use crate::geometry::Rect;
use crate::image::{RgbImage, decode_png_rgba};
use crate::render::RgbaImage;

/// A channel difference counts at most this much, so a few pixels under a count or the pointer cannot outweigh the
/// rest of the icon.
const CAP: u32 = 64;
/// Mean capped difference per channel above which an icon is not there (measured: a match 12-22, the nearest other
/// item 32 and up, at Ideal Ratio and at 1.5625x).
const MAX_ERROR: f32 = 27.0;
/// The best item must beat the best differently named one by this much (two ids can share a name and an icon).
const MARGIN: f32 = 6.0;
/// Icon pixels at least this opaque are compared.
const SOLID: u8 = 200;
/// Within this of a whole number the game draws the UI with whole pixels.
const WHOLE_SCALE_SLACK: f32 = 0.02;

/// An item's icon, as maplestory.io serves it.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub id: u32,
    pub name: String,
    pub image: Arc<RgbaImage>,
}

/// An icon at the scale the UI is drawn: its solid pixels as (x, y, rgb).
struct Scaled {
    width: usize,
    height: usize,
    pixels: Vec<(usize, usize, [u8; 3])>,
}

fn scale(image: &RgbaImage, k: f32) -> Scaled {
    let whole = k.round();
    let (w, h) = if (k - whole).abs() <= WHOLE_SCALE_SLACK && whole >= 1.0 {
        (image.width * whole as usize, image.height * whole as usize)
    } else {
        (((image.width as f32) * k).round().max(1.0) as usize, ((image.height as f32) * k).round().max(1.0) as usize)
    };
    // Whole scales keep whole pixels; in between the game smooths icons (unlike its digits), so they are blended
    // too, colour weighted by coverage.
    let sample = |x: usize, y: usize| -> [f32; 4] {
        let at = |sx: i64, sy: i64| {
            let p = image
                .pixel(sx.clamp(0, image.width as i64 - 1) as usize, sy.clamp(0, image.height as i64 - 1) as usize);
            let a = p[3] as f32 / 255.0;
            [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, a]
        };
        let fx = (x as f32 + 0.5) * image.width as f32 / w as f32 - 0.5;
        let fy = (y as f32 + 0.5) * image.height as f32 / h as f32 - 0.5;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let mut out = [0.0; 4];
        for (dx, dy, weight) in
            [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)]
        {
            let p = at(x0 + dx, y0 + dy);
            for c in 0..4 {
                out[c] += p[c] * weight;
            }
        }
        out
    };
    let whole_scale = w == image.width * whole.max(1.0) as usize && (k - whole).abs() <= WHOLE_SCALE_SLACK;
    let mut pixels = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let [r, g, b, a] = if whole_scale {
                let p = image.pixel(x * image.width / w, y * image.height / h);
                [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32 / 255.0]
            } else {
                let [r, g, b, a] = sample(x, y);
                if a > 0.0 { [r / a, g / a, b / a, a] } else { [0.0; 4] }
            };
            if a * 255.0 >= SOLID as f32 {
                pixels.push((x, y, [r.round() as u8, g.round() as u8, b.round() as u8]));
            }
        }
    }
    Scaled { width: w, height: h, pixels }
}

/// Known icons, matched against the screen at one UI scale at a time.
#[derive(Default)]
pub struct IconSet {
    icons: Vec<Icon>,
    scaled: Option<(u32, Vec<Scaled>)>,
}

/// An item found on screen: which, where its icon's top-left is (frame pixels), and how well it matched.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub error: f32,
}

impl IconSet {
    pub fn len(&self) -> usize {
        self.icons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }

    /// Add an item's icon (its PNG); false when the PNG cannot be read.
    pub fn add_png(&mut self, id: u32, name: &str, png: &[u8]) -> bool {
        let Ok((width, height, data)) = decode_png_rgba(png) else { return false };
        if width == 0 || height == 0 {
            return false;
        }
        self.icons.retain(|i| i.id != id);
        self.icons.push(Icon { id, name: name.to_string(), image: Arc::new(RgbaImage { width, height, data }) });
        self.scaled = None;
        true
    }

    pub fn icon(&self, id: u32) -> Option<&Icon> {
        self.icons.iter().find(|i| i.id == id)
    }

    fn scaled(&mut self, k: f32) -> &[Scaled] {
        let key = k.to_bits();
        if self.scaled.as_ref().is_none_or(|(at, _)| *at != key) {
            self.scaled = Some((key, self.icons.iter().map(|i| scale(&i.image, k)).collect()));
        }
        &self.scaled.as_ref().expect("just made").1
    }

    /// The item drawn inside `area` (frame pixels) at UI scale `k`, if one of the known icons is clearly there.
    pub fn identify(&mut self, frame: &RgbImage, area: Rect, k: f32) -> Option<Found> {
        let names: Vec<String> = self.icons.iter().map(|i| i.name.clone()).collect();
        let ids: Vec<u32> = self.icons.iter().map(|i| i.id).collect();
        let scaled = self.scaled(k);
        let mut found: Vec<(Found, &str)> = Vec::new();
        for ((icon, id), name) in scaled.iter().zip(&ids).zip(&names) {
            if let Some((error, x, y)) = best_place(frame, icon, area) {
                found.push((Found { id: *id, x, y, error }, name.as_str()));
            }
        }
        found.sort_by(|a, b| a.0.error.total_cmp(&b.0.error));
        let (best, name) = *found.first()?;
        let rival = found.iter().find(|(_, n)| *n != name).map_or(f32::INFINITY, |(f, _)| f.error);
        (best.error <= MAX_ERROR && rival - best.error >= MARGIN).then_some(best)
    }

    /// Is item `id` still drawn where it was found? (A cheap check, made instead of a new search.)
    pub fn still_there(&mut self, frame: &RgbImage, found: Found, k: f32) -> bool {
        let Some(index) = self.icons.iter().position(|i| i.id == found.id) else { return false };
        let icon = &self.scaled(k)[index];
        error_at(frame, icon, found.x, found.y, f32::INFINITY) <= MAX_ERROR
    }
}

/// The lowest error of `icon` anywhere inside `area`, and where.
fn best_place(frame: &RgbImage, icon: &Scaled, area: Rect) -> Option<(f32, i32, i32)> {
    let mut best: Option<(f32, i32, i32)> = None;
    for y in area.y..=area.bottom() - icon.height as i32 {
        for x in area.x..=area.right() - icon.width as i32 {
            let bound = best.map_or(f32::INFINITY, |b| b.0);
            let error = error_at(frame, icon, x, y, bound);
            if error < bound {
                best = Some((error, x, y));
            }
        }
    }
    best
}

/// Mean capped channel difference of `icon` at (x, y); gives up (infinity) once it cannot beat `bound`.
fn error_at(frame: &RgbImage, icon: &Scaled, x: i32, y: i32, bound: f32) -> f32 {
    if icon.pixels.is_empty() || x < 0 || y < 0 {
        return f32::INFINITY;
    }
    let (x, y) = (x as usize, y as usize);
    if x + icon.width > frame.width || y + icon.height > frame.height {
        return f32::INFINITY;
    }
    let total = (icon.pixels.len() * 3) as f32;
    let limit = if bound.is_finite() { (bound * total) as u32 } else { u32::MAX };
    let mut sum = 0u32;
    for (i, &(px, py, rgb)) in icon.pixels.iter().enumerate() {
        let seen = frame.pixel(x + px, y + py);
        for c in 0..3 {
            sum += (seen[c] as u32).abs_diff(rgb[c] as u32).min(CAP);
        }
        // Checked now and then: the sum only grows.
        if i % 32 == 31 && sum > limit {
            return f32::INFINITY;
        }
    }
    sum as f32 / total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::encode_png;

    fn square(rgb: [u8; 3]) -> Vec<u8> {
        let mut image = RgbImage::filled(10, 10, rgb);
        for i in 0..10 {
            image.set_pixel(i, i, [0, 0, 0]); // a diagonal, so the shape has a place
        }
        encode_png(&image)
    }

    #[test]
    fn the_icon_in_colour_is_told_from_the_same_shape_in_another() {
        let mut set = IconSet::default();
        assert!(set.add_png(1, "Red", &square([200, 30, 30])));
        assert!(set.add_png(2, "Blue", &square([30, 30, 200])));
        assert!(!set.add_png(3, "Broken", b"not a png"));
        let mut frame = RgbImage::filled(40, 40, [120, 160, 90]);
        for y in 0..10 {
            for x in 0..10 {
                frame.set_pixel(12 + x, 15 + y, if x == y { [0, 0, 0] } else { [30, 30, 200] });
            }
        }
        let found = set.identify(&frame, Rect::new(8, 10, 20, 20), 1.0).expect("found");
        assert_eq!((found.id, found.x, found.y), (2, 12, 15));
        assert!(set.still_there(&frame, found, 1.0));
        assert_eq!(set.identify(&RgbImage::filled(40, 40, [120, 160, 90]), Rect::new(8, 10, 20, 20), 1.0), None);
    }
}
