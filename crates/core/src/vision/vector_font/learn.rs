//! Learning the font from labelled lines of the game's own pixels: a super-resolution fit of every letter's
//! template to all its appearances, at every UI size and sub-pixel position at once.

use std::collections::BTreeMap;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};

use super::{BASE, Glyph, Line, PEN, R, ROWS, VectorFont, align, place};
use crate::image::RgbImage;

/// The game's text size at UI scale 1 (px per em), fitted on lines at 0.9, 1.0 and 1.2: a starting point only.
pub const SIZE_AT_ONE: f32 = 19.05;
/// Step of each template update (iterative back-projection): large, since every cell's step is normalised by how
/// much of the lines it explains.
const STEP: f32 = 0.8;
/// A learnt advance moves this far toward the median spacing measured in a round.
const ADVANCE_RATE: f32 = 0.5;
/// A template cell this dark is the glyph's body; cells further than BODY_REACH cells from any are cleared after each
/// step. Every '.' ends a line after a 'd', and the 'd' left a faint ghost stem in it that fit "left the" better than
/// a space did.
const BODY: f32 = 0.4;
const BODY_REACH: usize = R;

/// One labelled line: its pixels (the box the shop watcher reads), the UI scale, and its text.
#[derive(Clone, Debug)]
pub struct Sample {
    pub pixels: RgbImage,
    pub scale: f32,
    pub light_text: bool,
    pub text: String,
}

impl VectorFont {
    /// A starting point drawn from a font file (outline coverage at the game's size), for every char of `chars`.
    pub fn from_ttf(bytes: &[u8], chars: impl IntoIterator<Item = char>) -> Option<Self> {
        let font = FontRef::try_from_slice(bytes).ok()?;
        let fine =
            font.as_scaled(PxScale::from(SIZE_AT_ONE * R as f32 * font.height_unscaled() / font.units_per_em()?));
        let mut glyphs = BTreeMap::new();
        for c in chars {
            let id = font.glyph_id(c);
            let advance = fine.h_advance(id) / R as f32;
            let mut glyph = Glyph::blank(advance);
            if let Some(outline) =
                font.outline_glyph(id.with_scale_and_position(fine.scale(), point(PEN as f32, BASE as f32)))
            {
                let bounds = outline.px_bounds();
                outline.draw(|x, y, coverage| {
                    let (col, row) = (bounds.min.x as i32 + x as i32, bounds.min.y as i32 + y as i32);
                    if col >= 0 && row >= 0 && (col as usize) < glyph.cols && (row as usize) < ROWS {
                        glyph.template[row as usize * glyph.cols + col as usize] = coverage.clamp(0.0, 1.0);
                    }
                });
            }
            glyphs.insert(c, glyph);
        }
        Some(Self { glyphs })
    }
}

/// Each letter's fit on `samples`: (letter, instances, mean squared error per reference column of its cell).
pub fn fit_report(font: &VectorFont, samples: &[Sample]) -> Vec<(char, usize, f32)> {
    let mut sums: BTreeMap<char, (usize, f32)> = BTreeMap::new();
    for sample in samples {
        let Some(line) = Line::new(&sample.pixels, sample.scale, sample.light_text) else { continue };
        let Some((reading, base)) = align(&line, font, &sample.text) else { continue };
        for &(c, q) in &reading.pens {
            let glyph = &font.glyphs[&c];
            let patch = super::draw(glyph, q, base, line.scale);
            let (c0, c1) =
                (q.floor().max(0.0) as usize, ((q + glyph.advance * line.scale).ceil() as usize).min(line.width));
            let mut err = 0.0;
            for x in c0..c1 {
                for y in 0..line.height {
                    let (px, py) = (x as i32 - patch.x0, y as i32 - patch.y0);
                    let m = if px >= 0 && py >= 0 && (px as usize) < patch.width && (py as usize) < patch.height {
                        patch.get(px as usize, py as usize)
                    } else {
                        0.0
                    };
                    err += (line.get(x, y) - m).powi(2);
                }
            }
            let entry = sums.entry(c).or_default();
            entry.0 += 1;
            entry.1 += err / (line.scale * line.scale) / (glyph.advance.max(1.0));
        }
    }
    sums.into_iter().map(|(c, (n, e))| (c, n, e / n as f32)).collect()
}

/// `rounds` rounds of fitting `font` to `samples`; returns the font and each round's total alignment cost.
pub fn train(mut font: VectorFont, samples: &[Sample], rounds: usize) -> (VectorFont, Vec<f32>) {
    let mut costs = Vec::new();
    for _ in 0..rounds {
        let mut grads: BTreeMap<char, (Vec<f32>, Vec<f32>)> = BTreeMap::new();
        let mut spans: BTreeMap<char, Vec<f32>> = BTreeMap::new();
        let mut total = 0.0;
        for sample in samples {
            let Some(line) = Line::new(&sample.pixels, sample.scale, sample.light_text) else { continue };
            let Some((reading, base)) = align(&line, &font, &sample.text) else { continue };
            total += reading.cost;
            accumulate(&font, &line, &reading.pens, base, &mut grads);
            for pair in reading.pens.windows(2) {
                spans.entry(pair[0].0).or_default().push((pair[1].1 - pair[0].1) / line.scale);
            }
        }
        costs.push(total);
        for (c, glyph) in font.glyphs.iter_mut() {
            if let Some((grad, weight)) = grads.get(c)
                && *c != ' '
            {
                for ((t, g), w) in glyph.template.iter_mut().zip(grad).zip(weight) {
                    *t = (*t + STEP * g / w.max(1e-3)).clamp(0.0, 1.0);
                }
                clear_ghosts(glyph);
            }
            if let Some(spans) = spans.get_mut(c) {
                spans.sort_by(f32::total_cmp);
                glyph.advance += ADVANCE_RATE * (spans[spans.len() / 2] - glyph.advance);
            }
        }
    }
    (font, costs)
}

/// Clears template cells further than BODY_REACH from the glyph's body.
fn clear_ghosts(glyph: &mut Glyph) {
    let cols = glyph.cols;
    let body: Vec<bool> = glyph.template.iter().map(|&t| t >= BODY).collect();
    if !body.iter().any(|&b| b) {
        return;
    }
    // Distance to the body along rows, then columns (a square neighbourhood).
    let mut near_row = vec![false; body.len()];
    for row in 0..ROWS {
        for col in 0..cols {
            let (a, b) = (col.saturating_sub(BODY_REACH), (col + BODY_REACH + 1).min(cols));
            near_row[row * cols + col] = (a..b).any(|c| body[row * cols + c]);
        }
    }
    for row in 0..ROWS {
        for col in 0..cols {
            let (a, b) = (row.saturating_sub(BODY_REACH), (row + BODY_REACH + 1).min(ROWS));
            if !(a..b).any(|r| near_row[r * cols + col]) {
                glyph.template[row * cols + col] = 0.0;
            }
        }
    }
}

/// Adds each placed letter's back-projected residual (and how much each template cell was seen) to `grads`.
fn accumulate(
    font: &VectorFont,
    line: &Line,
    pens: &[(char, f32)],
    base: f32,
    grads: &mut BTreeMap<char, (Vec<f32>, Vec<f32>)>,
) {
    let s = line.scale;
    let mut model = vec![0.0f32; line.width * line.height];
    let placements: Vec<_> = pens.iter().map(|&(c, q)| (c, place(&font.glyphs[&c], q, base, s))).collect();
    for (c, p) in &placements {
        let glyph = &font.glyphs[c];
        for (y, (fy, wys)) in p.ys.iter().enumerate() {
            let fy_frame = p.y0 + y as i32;
            if fy_frame < 0 || fy_frame as usize >= line.height {
                continue;
            }
            for (x, (fx, wxs)) in p.xs.iter().enumerate() {
                let fx_frame = p.x0 + x as i32;
                if fx_frame < 0 || fx_frame as usize >= line.width {
                    continue;
                }
                let mut v = 0.0;
                for (i, wy) in wys.iter().enumerate() {
                    for (j, wx) in wxs.iter().enumerate() {
                        v += wy * wx * glyph.at(fy + i, fx + j);
                    }
                }
                model[fy_frame as usize * line.width + fx_frame as usize] += v;
            }
        }
    }
    for (c, p) in &placements {
        let glyph = &font.glyphs[c];
        let (grad, weight) =
            grads.entry(*c).or_insert_with(|| (vec![0.0; glyph.template.len()], vec![0.0; glyph.template.len()]));
        for (y, (fy, wys)) in p.ys.iter().enumerate() {
            let fy_frame = p.y0 + y as i32;
            if fy_frame < 0 || fy_frame as usize >= line.height {
                continue;
            }
            for (x, (fx, wxs)) in p.xs.iter().enumerate() {
                let fx_frame = p.x0 + x as i32;
                if fx_frame < 0 || fx_frame as usize >= line.width {
                    continue;
                }
                let at = fy_frame as usize * line.width + fx_frame as usize;
                let residual = line.dark[at] - model[at].min(1.0);
                for (i, wy) in wys.iter().enumerate() {
                    for (j, wx) in wxs.iter().enumerate() {
                        let cell = (fy + i) * glyph.cols + fx + j;
                        grad[cell] += wy * wx * residual;
                        weight[cell] += wy * wx;
                    }
                }
            }
        }
    }
}
