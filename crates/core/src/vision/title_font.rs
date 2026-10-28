//! Item titles read exactly by the game's own font (pure).
//!
//! A tooltip's title is the game's bold bitmap font: drawn 1:1 below 1080p and at "Ideal Ratio" (twice as large at its
//! 1440p double), and at the default sizes drawn at 768 px high and scaled up with the whole frame, which
//! `pixel_font::unscaled_line_mask` undoes. Either way a letter is a lookup in a table learnt from rendered scenes
//! (`examples/build_title_font`), never a look-alike: what the table does not know reads '?', for the glyph atlas and
//! OCR to try.

use std::sync::OnceLock;

use super::pixel_font::{PixelFont, line_mask, unscaled};
use crate::config;
use crate::geometry::scaled;
use crate::image::RgbImage;
use crate::shop::text::{bitmap_block, settle_bars};

/// The bitmap font's UI scale ("Ideal Ratio"): the default sizes draw at 768 px high, 0.641 of the reference 1200.
const BITMAP_SCALE: f32 = 0.641;
/// Where the title's ink lies in a tooltip, at UI scale 1 from its top-left corner: past the bullet, and from the
/// capitals' tops to the descenders (measured on 690 rendered tooltips: ink from x 28, rows 21-38, with room for
/// rounding at small scales).
const TITLE_LEFT: i32 = 22;
const TITLE_ROWS: (i32, i32) = (10, 41);
/// The band stops this far short of the tooltip's right side: its border is ink to the mask.
const RIGHT_MARGIN: i32 = 10;
/// Font pixels between words: counting only full-strength ink, the bold letters stand up to 4 apart ("C urtains").
pub const SPACE_GAP: usize = 5;

pub fn title_font() -> &'static PixelFont {
    static FONT: OnceLock<PixelFont> = OnceLock::new();
    FONT.get_or_init(|| PixelFont::from_json(include_str!("../../assets/glyphs/title_small.json")).unwrap_or_default())
}

/// The title band of a tooltip as captured: `native` is the tooltip's top-left corner onward in the frame's own
/// pixels, at UI `scale`; `width` how far the title may run (UI scale 1); `shift` rows further down for a crafted
/// item. None when the band is outside the capture.
pub fn title_band(native: &RgbImage, scale: f32, width: usize, shift: i32) -> Option<RgbImage> {
    let x0 = scaled(TITLE_LEFT, scale).max(0) as usize;
    let x1 = (scaled(width as i32 - RIGHT_MARGIN, scale).max(0) as usize).min(native.width);
    let (y0, y1) = (scaled(TITLE_ROWS.0 + shift, scale).max(0) as usize, scaled(TITLE_ROWS.1 + shift, scale) as usize);
    (x1 > x0 && y1 <= native.height).then(|| native.crop(x0, y0, x1, y1))
}

/// A title pixel the letter covers fully is the title's colour at full strength (white, or an enhanced item's orange:
/// red at 255 either way); its anti-aliased edges are blended with whatever the translucent box shows behind them,
/// so only the full pixels are the same in every scene.
const FULL_INK: u8 = 235;
const SHRUNK_FULL_INK: u8 = 200;
/// Pixels a shrunk title's letter may differ from the table's by.
const SHRUNK_SLACK: u32 = 3;

/// A title's pixels (a band as `title_band` cuts it, or any box around the line) as the font's mask.
pub fn title_mask(pixels: &RgbImage, scale: f32) -> Option<super::Mask> {
    match bitmap_block(scale) {
        Some(block) => {
            let mut full = pixels.clone();
            for p in full.data.as_chunks_mut::<3>().0 {
                let v = if p[0].max(p[1]).max(p[2]) >= FULL_INK { 255 } else { 0 };
                *p = [v, v, v];
            }
            line_mask(&full, block, true)
        }
        None => {
            // Shrunk back, a fully covered pixel stays bright but no longer full: the scaling blurred it a little.
            let base = unscaled(pixels, scale / BITMAP_SCALE, |p| p[0].max(p[1]).max(p[2]))?;
            let mut full = base;
            for p in full.data.as_chunks_mut::<3>().0 {
                let v = if p[0] >= SHRUNK_FULL_INK { 255 } else { 0 };
                *p = [v, v, v];
            }
            line_mask(&full, 1, true)
        }
    }
}

/// The title of a tooltip as the font reads it, and how many of its letters it does not know.
pub fn read_title(native: &RgbImage, scale: f32, width: usize, crafted: bool) -> Option<(String, usize)> {
    let shift = if crafted { config::TOOLTIP_CRAFTED_SHIFT } else { 0 };
    let mask = title_mask(&title_band(native, scale, width, shift)?, scale)?;
    let (text, unknown) = match bitmap_block(scale) {
        Some(_) => title_font().read_spaced(&mask, SPACE_GAP),
        // Recovered from a scaled frame, an edge pixel can fall either way.
        None => title_font().read_near(&mask, SPACE_GAP, SHRUNK_SLACK),
    };
    // Capital I and lowercase l are one bar here too.
    let text = settle_bars(text.trim());
    (!text.is_empty()).then_some((text, unknown))
}

/// Font pixels from the band's end within which a title's ink runs into the tooltip's edge: the game cut it there.
const CUT_PX: usize = 2;

/// Does the title's ink run to the end of its band, cut by the tooltip's right edge (a long scroll's name)? Its last
/// letter may then be part of one ('m' cut to "rr").
pub fn title_cut(native: &RgbImage, scale: f32, width: usize, crafted: bool) -> bool {
    let shift = if crafted { config::TOOLTIP_CRAFTED_SHIFT } else { 0 };
    let Some(mask) = title_band(native, scale, width, shift).and_then(|band| title_mask(&band, scale)) else {
        return false;
    };
    (0..mask.width)
        .rev()
        .find(|&x| (0..mask.height).any(|y| mask.get(x, y)))
        .is_some_and(|x| x + CUT_PX + 1 >= mask.width)
}
