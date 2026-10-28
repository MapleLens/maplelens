//! Reading the game's screen: tooltip presence and width, pushed-up tooltips, the Character Info window, and
//! the text of titles and name plates (glyph matching, with an optional OCR fallback).

pub mod bitmap;
pub mod find;
pub mod gaps;
pub mod gauge;
pub mod glyphs;
pub mod hud;
pub mod info_window;
pub mod item_icon;
pub mod locate;
pub mod map_tooltip;
pub mod minimap;
pub mod ncc;
pub mod pixel_font;
pub mod quickslot;
pub mod reader;
pub mod samples;
pub mod segment;
pub mod title_font;
pub mod tooltip;
pub mod vector_font;

use crate::image::GrayImage;

/// Title glyphs are near-white; the translucent tooltip fill never gets this bright.
pub const TEXT_THRESHOLD: u8 = 185;

/// A binary image: true where text ink is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub bits: Vec<bool>,
}

impl Mask {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, bits: vec![false; width * height] }
    }

    /// Pixels brighter than `threshold`.
    pub fn threshold(gray: &GrayImage, threshold: u8) -> Self {
        Self { width: gray.width, height: gray.height, bits: gray.data.iter().map(|&v| v > threshold).collect() }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.bits[y * self.width + x]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, on: bool) {
        self.bits[y * self.width + x] = on;
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn count(&self) -> usize {
        self.bits.iter().filter(|&&b| b).count()
    }

    pub fn crop(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Self {
        let (x1, y1) = (x1.min(self.width), y1.min(self.height));
        let (x0, y0) = (x0.min(x1), y0.min(y1));
        let mut out = Self::new(x1 - x0, y1 - y0);
        for y in y0..y1 {
            for x in x0..x1 {
                out.set(x - x0, y - y0, self.get(x, y));
            }
        }
        out
    }

    /// Thicker strokes (each pixel also lights its right and lower neighbour). OCR engines drop 1 px strokes.
    pub fn dilated(&self) -> Self {
        let mut out = self.clone();
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) {
                    if x + 1 < self.width {
                        out.set(x + 1, y, true);
                    }
                    if y + 1 < self.height {
                        out.set(x, y + 1, true);
                        if x + 1 < self.width {
                            out.set(x + 1, y + 1, true);
                        }
                    }
                }
            }
        }
        out
    }

    /// Compact identity of the mask: a moving tooltip keeps the same title over changing backgrounds, so the
    /// text mask alone is the cache key for reads.
    pub fn key(&self) -> Vec<u8> {
        let mut key = Vec::with_capacity(8 + self.bits.len() / 8 + 1);
        key.extend_from_slice(&(self.width as u32).to_le_bytes());
        key.extend_from_slice(&(self.height as u32).to_le_bytes());
        for chunk in self.bits.chunks(8) {
            key.push(chunk.iter().enumerate().fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i)));
        }
        key
    }

    /// Dark text on white, one byte per pixel (what OCR engines read best).
    pub fn to_ink_on_paper(&self) -> GrayImage {
        GrayImage {
            width: self.width,
            height: self.height,
            data: self.bits.iter().map(|&b| if b { 0 } else { 255 }).collect(),
        }
    }
}
