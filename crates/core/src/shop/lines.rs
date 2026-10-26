//! One line of shop text, compared by how much ink each pixel column holds.
//!
//! The game draws its font at fractional vertical offsets, so the same text anti-aliases differently depending on
//! the row slot or scroll position and a pixel-for-pixel comparison never matches. Summing the ink down each column
//! barely moves under a vertical shift, yet one changed digit still stands out locally.

use super::layout::{Area, View};
use crate::image::{GrayImage, luma};
use crate::vision::Mask;

/// Grey level where ink starts to count; the fills and the page are all lighter.
const DARK_FROM: i32 = 200;
/// Columns compared together: about one glyph and its neighbours.
const WINDOW: usize = 14;
/// Keeps empty stretches from dividing by zero.
const EPS: f32 = 300.0;
/// Measured: the same text scores < 0.1, text differing by one digit > 0.3.
const SAME_BELOW: f32 = 0.2;
/// Total column ink below which a line is empty.
pub const MIN_INK: f32 = 2000.0;
/// Glyph pixel for OCR; anti-aliasing above this fuses neighbouring glyphs.
const OCR_INK: u8 = 150;
/// How much bluer than red a pixel may be and still be glyph grey.
const NEUTRAL: i32 = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Ink per pixel column.
    pub profile: Vec<f32>,
    /// Glyph pixels trimmed to the text, for OCR.
    pub glyphs: Mask,
    /// Where the line was read (reference pixels) and whether only grey pixels count as text: at another UI scale
    /// the text is read again from the sharper unscaled frame (`glyphs_at`).
    pub source: Option<(Area, bool)>,
}

impl Line {
    pub fn ink(&self) -> f32 {
        self.profile.iter().sum()
    }
}

/// `gray` is the line's window; `valid` optionally masks out pixels that are not text.
pub fn make_line(gray: &GrayImage, valid: Option<&Mask>) -> Line {
    let mut profile = vec![0.0f32; gray.width];
    let mut glyphs = Mask::new(gray.width, gray.height);
    for y in 0..gray.height {
        for (x, column) in profile.iter_mut().enumerate() {
            if valid.is_some_and(|m| !m.get(x, y)) {
                continue;
            }
            let v = gray.get(x, y);
            *column += (DARK_FROM - v as i32).max(0) as f32;
            glyphs.set(x, y, v < OCR_INK);
        }
    }
    Line { profile, glyphs: trim(&glyphs), source: None }
}

/// Glyphs are grey; the dark blue-ish separator lines between sold rows are not.
pub fn is_grey([r, _, b]: [u8; 3]) -> bool {
    (b as i32 - r as i32) < NEUTRAL
}

/// The glyph mask of `area` (reference pixels) read from the frame's own pixels. OCR reads the game's own small
/// glyphs far better than an enlarged, blurred copy of them.
pub fn glyphs_at(view: &View<'_>, area: Area, grey_only: bool) -> Mask {
    let (x0, y0, x1, y1) = view.frame_area(area);
    let crop = view.frame.crop(x0, y0, x1, y1);
    let bits = crop.data.as_chunks::<3>().0.iter().map(|p| {
        let rgb = [p[0], p[1], p[2]];
        luma(rgb[0], rgb[1], rgb[2]) < OCR_INK && (!grey_only || is_grey(rgb))
    });
    trim(&Mask { width: crop.width, height: crop.height, bits: bits.collect() })
}

/// Like `glyphs_at`, for light letters on a dark plate (a name under a character): the mask is where the pixels are
/// bright, so it reads as ink like any other text.
pub fn light_glyphs_at(view: &View<'_>, area: Area, ink: u8) -> Mask {
    let (x0, y0, x1, y1) = view.frame_area(area);
    let crop = view.frame.crop(x0, y0, x1, y1);
    let bits = crop.data.as_chunks::<3>().0.iter().map(|p| luma(p[0], p[1], p[2]) >= ink);
    trim(&Mask { width: crop.width, height: crop.height, bits: bits.collect() })
}

fn trim(mask: &Mask) -> Mask {
    let rows: Vec<usize> = (0..mask.height).filter(|&y| (0..mask.width).any(|x| mask.get(x, y))).collect();
    let cols: Vec<usize> = (0..mask.width).filter(|&x| (0..mask.height).any(|y| mask.get(x, y))).collect();
    match (rows.first(), rows.last(), cols.first(), cols.last()) {
        (Some(&y0), Some(&y1), Some(&x0), Some(&x1)) => mask.crop(x0, y0, x1 + 1, y1 + 1),
        _ => Mask::new(0, 0),
    }
}

/// Sums of `values` over a WINDOW-wide window around each index (numpy's `convolve(..., "same")` with ones).
fn windowed(values: &[f32]) -> Vec<f32> {
    let before = WINDOW / 2;
    (0..values.len())
        .map(|i| values[i.saturating_sub(before)..(i + WINDOW - before).min(values.len())].iter().sum())
        .collect()
}

pub fn same(a: &Line, b: &Line) -> bool {
    if a.profile.len() != b.profile.len() {
        return false;
    }
    let diff: Vec<f32> = a.profile.iter().zip(&b.profile).map(|(x, y)| (x - y).abs()).collect();
    let mass: Vec<f32> = a.profile.iter().zip(&b.profile).map(|(x, y)| x + y).collect();
    windowed(&diff).iter().zip(windowed(&mass)).all(|(d, m)| d / (m + EPS) < SAME_BELOW)
}
