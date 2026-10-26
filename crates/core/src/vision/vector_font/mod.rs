//! The game's anti-aliased UI font (shop text at the default UI sizes), read exactly by drawing it (pure).
//!
//! At the default UI sizes the game draws its text as a vector font at a size proportional to the UI scale, each
//! letter at a fractional position: the same letter is never the same pixels twice, and letters touch ("ft", "ro"),
//! so neither cutting letters apart nor matching binarised bitmaps works (the glyph atlas POC read 13% of lines).
//! Instead every letter is a darkness template on a grid `R` times finer than a reference pixel (UI scale 1), learnt
//! from the game's own pixels (`learn`). Drawn at scale s with its pen at a fractional position, a frame pixel's
//! darkness is the template's mean over the pixel's footprint, so one template serves every UI size: trained on two
//! of 0.9 / 1.0 / 1.2, each held-out scale read every line whose letters were known.
//!
//! Reading (`read`) finds the cheapest path of letters across the line's pen positions; a span no letter explains
//! reads as `?` (filled by OCR), never as a look-alike.

mod learn;
mod read;

#[cfg(test)]
mod tests;

pub use learn::{Sample, fit_report, train};
pub use read::{Reading, align, read};

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::{Value, json};

use crate::image::{RgbImage, decode_png, encode_png, luma};

/// Template cells per reference pixel: 4 resolves the quarter-pixel positions the game draws at, at UI scale 1.
pub const R: usize = 4;
/// Reference pixels a template spans above and below the baseline (capitals and ascenders reach ~14 above,
/// descenders ~4 below).
pub(crate) const ASC: usize = 17;
const DESC: usize = 6;
/// Room (reference px) left of the pen and right of the advance, for overhangs and anti-aliasing.
const LEFT: usize = 3;
const RIGHT: usize = 4;
pub const ROWS: usize = (ASC + DESC) * R;
const PEN: usize = LEFT * R;
pub(crate) const BASE: usize = ASC * R;

/// Template darkness below this is saved as none.
const FAINT: f32 = 0.03;

/// One letter: its darkness template (ROWS x cols, pen at column PEN, baseline at row BASE) and its advance.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub cols: usize,
    pub template: Vec<f32>,
    /// Reference pixels from this letter's pen to the next one's.
    pub advance: f32,
}

impl Glyph {
    /// A blank glyph (a space, or a letter before it is learnt) of the given advance.
    pub fn blank(advance: f32) -> Self {
        let cols = ((LEFT as f32 + advance + RIGHT as f32) * R as f32).ceil() as usize;
        Self { cols, template: vec![0.0; ROWS * cols], advance }
    }

    #[inline]
    fn at(&self, row: usize, col: usize) -> f32 {
        self.template[row * self.cols + col]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorFont {
    pub glyphs: BTreeMap<char, Glyph>,
}

/// For each output pixel along one axis: the first cell it covers and the share of the pixel each covered cell
/// takes. Output pixel k covers cells [(k - origin) R / s, (k + 1 - origin) R / s).
fn footprint(n_out: usize, origin: f32, scale: f32, n_cells: usize) -> Vec<(usize, Vec<f32>)> {
    let span = R as f32 / scale;
    (0..n_out)
        .map(|k| {
            let a = (k as f32 - origin) * span;
            let b = a + span;
            let lo = a.max(0.0).floor() as usize;
            let hi = (b.min(n_cells as f32).ceil().max(0.0) as usize).max(lo);
            let weights = (lo..hi).map(|c| ((b.min(c as f32 + 1.0) - a.max(c as f32)).max(0.0)) / span).collect();
            (lo, weights)
        })
        .collect()
}

/// A glyph drawn at `scale` with its pen at frame x `pen` and its baseline at frame y `base`: the darkness of the
/// frame pixels it covers, from (x0, y0).
#[derive(Clone, Debug)]
pub struct Patch {
    pub x0: i32,
    pub y0: i32,
    pub width: usize,
    pub height: usize,
    pub dark: Vec<f32>,
}

impl Patch {
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.dark[y * self.width + x]
    }
}

/// Where a glyph's template lands: frame origin (x0, y0) and the per-axis footprints.
struct Placement {
    x0: i32,
    y0: i32,
    xs: Vec<(usize, Vec<f32>)>,
    ys: Vec<(usize, Vec<f32>)>,
}

fn place(glyph: &Glyph, pen: f32, base: f32, scale: f32) -> Placement {
    let left = pen - LEFT as f32 * scale;
    let x0 = left.floor();
    let nx = (left + glyph.cols as f32 / R as f32 * scale).ceil() - x0 + 1.0;
    let top = base - ASC as f32 * scale;
    let y0 = top.floor();
    let ny = (top + ROWS as f32 / R as f32 * scale).ceil() - y0 + 1.0;
    Placement {
        x0: x0 as i32,
        y0: y0 as i32,
        xs: footprint(nx as usize, left - x0, scale, glyph.cols),
        ys: footprint(ny as usize, top - y0, scale, ROWS),
    }
}

pub fn draw(glyph: &Glyph, pen: f32, base: f32, scale: f32) -> Patch {
    let p = place(glyph, pen, base, scale);
    // Columns first (template rows x frame columns), then rows.
    let mut by_col = vec![0.0f32; ROWS * p.xs.len()];
    for row in 0..ROWS {
        for (k, (first, weights)) in p.xs.iter().enumerate() {
            by_col[row * p.xs.len() + k] = weights.iter().enumerate().map(|(i, w)| w * glyph.at(row, first + i)).sum();
        }
    }
    let (width, height) = (p.xs.len(), p.ys.len());
    let mut dark = vec![0.0f32; width * height];
    for (y, (first, weights)) in p.ys.iter().enumerate() {
        for (i, w) in weights.iter().enumerate() {
            let row = &by_col[(first + i) * width..][..width];
            for (d, v) in dark[y * width..][..width].iter_mut().zip(row) {
                *d += w * v;
            }
        }
    }
    Patch { x0: p.x0, y0: p.y0, width, height, dark }
}

/// A line of text ready to read: its darkness (0 = paper, 1 = ink) in frame pixels, the scale it is drawn at, and
/// its baseline (frame rows, fractional).
#[derive(Clone, Debug)]
pub struct Line {
    pub width: usize,
    pub height: usize,
    pub dark: Vec<f32>,
    pub scale: f32,
    pub base: f32,
}

/// Box edges: inked (above EDGE_INK) along more than EDGE_ROW tenths of a row or EDGE_COL tenths of a column.
const EDGE_INK: f32 = 0.1;
const EDGE_ROW: usize = 7;
const EDGE_COL: usize = 9;
/// What an edge line itself reaches (sold rows' separators ~0.15, the counter's frame ~0.4); text is darker.
const EDGE_KEEP: f32 = 0.5;
/// A line whose darkest pixel is this close to its paper holds no text.
const MIN_CONTRAST: f32 = 60.0;

impl Line {
    /// `pixels` is the line's box in frame pixels; `light_text` for light letters on a dark plate.
    pub fn new(pixels: &RgbImage, scale: f32, light_text: bool) -> Option<Self> {
        let values: Vec<f32> = pixels
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                let v = luma(p[0], p[1], p[2]) as f32;
                if light_text { 255.0 - v } else { v }
            })
            .collect();
        if values.is_empty() {
            return None;
        }
        // Paper is what most of the box is; ink the darkest the text gets.
        let mut sorted = values.clone();
        sorted.sort_by(f32::total_cmp);
        let paper = sorted[sorted.len() / 2];
        let ink = sorted[0];
        if paper - ink < MIN_CONTRAST {
            return None;
        }
        let dark: Vec<f32> = pixels
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .zip(&values)
            .map(|(_, &v)| ((paper - v) / (paper - ink)).clamp(0.0, 1.0))
            .collect();
        let mut line = Self { width: pixels.width, height: pixels.height, dark, scale, base: 0.0 };
        line.clear_box_edges();
        line.base = line.baseline()?;
        Some(line)
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.dark[y * self.width + x]
    }

    /// The edges of the box the text sits in (the meso counter's frame): a row inked across most of the width or a
    /// column down most of the height is never text.
    fn clear_box_edges(&mut self) {
        let (w, h) = (self.width, self.height);
        let edge_rows: Vec<usize> =
            (0..h).filter(|&y| (0..w).filter(|&x| self.get(x, y) > EDGE_INK).count() * 10 > w * EDGE_ROW).collect();
        let edge_cols: Vec<usize> =
            (0..w).filter(|&x| (0..h).filter(|&y| self.get(x, y) > EDGE_INK).count() * 10 > h * EDGE_COL).collect();
        // Only a row's own faint pixels go: a descender crossing a sold row's separator is still text.
        for y in edge_rows {
            for d in &mut self.dark[y * w..][..w] {
                if *d < EDGE_KEEP {
                    *d = 0.0;
                }
            }
        }
        // No letter runs the line's whole height: a full column is the box's side.
        for x in edge_cols {
            for y in 0..h {
                self.dark[y * w + x] = 0.0;
            }
        }
    }

    /// The baseline: the row most inked columns end on (descenders end below it, but they are few), at the bottom
    /// edge of that row. A dense band would be fooled by wide descenders ("Smoggy": 1.3 px too low); `read` refines
    /// this by fitting the row profile.
    fn baseline(&self) -> Option<f32> {
        let mut bottoms = vec![0usize; self.height];
        for x in 0..self.width {
            if let Some(y) = (0..self.height).rev().find(|&y| self.get(x, y) > 0.5) {
                bottoms[y] += 1;
            }
        }
        let (row, &count) = bottoms.iter().enumerate().max_by_key(|&(y, &n)| (n, y))?;
        (count > 0).then_some(row as f32 + 1.0)
    }

    /// The first and last columns with ink.
    pub fn ink_span(&self) -> Option<(usize, usize)> {
        let inked = |x: usize| (0..self.height).any(|y| self.get(x, y) > 0.15);
        let first = (0..self.width).find(|&x| inked(x))?;
        let last = (0..self.width).rev().find(|&x| inked(x))?;
        Some((first, last))
    }
}

impl VectorFont {
    /// The glyph atlas as a grey PNG (templates side by side, ROWS high) and its metrics as JSON.
    pub fn to_files(&self) -> (Vec<u8>, String) {
        let width: usize = self.glyphs.values().map(|g| g.cols).sum::<usize>().max(1);
        let mut atlas = RgbImage::new(width, ROWS);
        let mut metrics = Vec::new();
        let mut x = 0;
        for (&c, g) in &self.glyphs {
            for row in 0..ROWS {
                for col in 0..g.cols {
                    // Faint noise in empty cells (learning leaves it) only bloats the PNG.
                    let t = g.at(row, col);
                    let v = if t < FAINT { 255 } else { (255.0 * (1.0 - t)).round().clamp(0.0, 255.0) as u8 };
                    atlas.set_pixel(x + col, row, [v; 3]);
                }
            }
            metrics.push(json!({"c": c.to_string(), "x": x, "cols": g.cols, "advance": g.advance}));
            x += g.cols;
        }
        (
            encode_png(&atlas),
            serde_json::to_string(&json!({"r": R, "rows": ROWS, "glyphs": metrics})).unwrap_or_default(),
        )
    }

    /// None when the files are not an atlas this module wrote (or one with another grid).
    pub fn from_files(png: &[u8], metrics: &str) -> Option<Self> {
        let atlas = decode_png(png).ok()?;
        let value: Value = serde_json::from_str(metrics).ok()?;
        if value.get("r")?.as_u64()? != R as u64 || value.get("rows")?.as_u64()? != ROWS as u64 {
            return None;
        }
        let mut glyphs = BTreeMap::new();
        for g in value.get("glyphs")?.as_array()? {
            let c = g.get("c")?.as_str()?.chars().next()?;
            let (x, cols) = (g.get("x")?.as_u64()? as usize, g.get("cols")?.as_u64()? as usize);
            if x + cols > atlas.width || atlas.height < ROWS {
                return None;
            }
            let mut template = Vec::with_capacity(ROWS * cols);
            for row in 0..ROWS {
                for col in 0..cols {
                    template.push(1.0 - atlas.pixel(x + col, row)[0] as f32 / 255.0);
                }
            }
            glyphs.insert(c, Glyph { cols, template, advance: g.get("advance")?.as_f64()? as f32 });
        }
        Some(Self { glyphs })
    }
}

/// The shop's font as learnt from the game's own pixels (`examples/build_shop_font`, from `tests/fixtures/shop_text.tsv`).
pub fn shop_font() -> &'static VectorFont {
    static FONT: OnceLock<VectorFont> = OnceLock::new();
    FONT.get_or_init(|| {
        VectorFont::from_files(
            include_bytes!("../../../assets/glyphs/shop_vector.png"),
            include_str!("../../../assets/glyphs/shop_vector.json"),
        )
        .unwrap_or_default()
    })
}
