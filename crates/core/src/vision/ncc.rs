//! Normalised cross-correlation template matching (OpenCV's TM_CCOEFF_NORMED), without OpenCV.
//!
//! Scoring every position of a full game frame at full resolution is ~4 G multiply-adds, so the search runs on
//! an image pyramid: every position at the coarsest level, then only the best few spots one level finer, down
//! to full resolution.

use crate::image::{GrayImage, RgbImage};

/// Coarse scores this high are worth re-checking one level finer: blurring 1 px strokes costs a true match a
/// good part of its score, so the bar is low and many candidates are kept. At the coarsest level a title bar is
/// only a few rows tall and every window's title bar looks alike, hence far more candidates there.
const COARSE_MIN: f32 = 0.35;
const COARSEST_CANDIDATES: usize = 256;
const CANDIDATES: usize = 16;
/// Search radius one level finer around a coarse hit (covers the rounding of halving odd sizes).
const REFINE_RADIUS: i32 = 3;
/// The coarsest level still keeps the template this big (smaller loses the letters' shapes).
const MIN_TEMPLATE: (usize, usize) = (24, 3);
/// Below this many positions an exhaustive full-resolution search is cheap enough.
const EXHAUSTIVE_POSITIONS: usize = 4096;

/// A template with its zero-mean pixel values precomputed.
pub struct Template {
    pub width: usize,
    pub height: usize,
    centred: Vec<f32>,
    norm: f32,
}

impl Template {
    pub fn new(image: &GrayImage) -> Self {
        let n = image.data.len().max(1) as f32;
        let mean = image.data.iter().map(|&v| v as f32).sum::<f32>() / n;
        let centred: Vec<f32> = image.data.iter().map(|&v| v as f32 - mean).collect();
        let norm = centred.iter().map(|v| v * v).sum::<f32>();
        Self { width: image.width, height: image.height, centred, norm }
    }

    /// Score in -1..1 of the template with its top-left at (x, y) in `image` (which must contain it).
    pub fn score_at(&self, image: &GrayImage, x: usize, y: usize) -> f32 {
        let (mut sum, mut sum_sq, mut dot) = (0.0f32, 0.0f32, 0.0f32);
        for ty in 0..self.height {
            let row = &image.data[(y + ty) * image.width + x..][..self.width];
            let trow = &self.centred[ty * self.width..][..self.width];
            for (&v, &t) in row.iter().zip(trow) {
                let v = v as f32;
                sum += v;
                sum_sq += v * v;
                dot += v * t;
            }
        }
        let n = (self.width * self.height) as f32;
        let variance = sum_sq - sum * sum / n;
        let denominator = (variance.max(0.0) * self.norm).sqrt();
        if denominator < 1e-3 { 0.0 } else { dot / denominator }
    }
}

/// Best (score, x, y) of `template` anywhere inside `image`, or None if it does not fit.
pub fn best_match(image: &GrayImage, template: &GrayImage) -> Option<(f32, usize, usize)> {
    ranked(image, template, MIN_TEMPLATE).map(|hits| hits.into_iter().next().unwrap_or((0.0, 0, 0)))
}

/// Up to `count` matches scoring at least `min`, best first, each at least a template size away from the
/// others (OpenCV's matchTemplate followed by suppressing every peak around itself). `coarsest` is the smallest
/// the template may shrink to in the pyramid: smaller is faster but needs a template with bold features.
pub fn matches(
    image: &GrayImage,
    template: &GrayImage,
    min: f32,
    count: usize,
    coarsest: (usize, usize),
) -> Vec<(f32, usize, usize)> {
    let mut kept: Vec<(f32, usize, usize)> = Vec::new();
    for hit in ranked(image, template, coarsest).unwrap_or_default() {
        if hit.0 < min || kept.len() == count {
            break;
        }
        if !kept.iter().any(|k| k.1.abs_diff(hit.1) < template.width && k.2.abs_diff(hit.2) < template.height) {
            kept.push(hit);
        }
    }
    kept
}

/// Every position of `template` in `image` scoring at least `min`, as `Template::score_at` would score it but far
/// faster (the scan of a whole frame is the cost of a search): the window sums come from integral images, and what
/// is left per position is one integer dot product per row, which the compiler vectorises. The template is
/// rounded to whole grey levels, which moves a score by about 0.01: this only nominates candidates, and every
/// later stage scores them exactly.
fn scan(image: &GrayImage, template: &GrayImage, min: f32) -> Vec<(f32, usize, usize)> {
    let (tw, th) = (template.width, template.height);
    let n = (tw * th) as i64;
    let mean = template.data.iter().map(|&v| v as i64).sum::<i64>() as f64 / n as f64;
    let centred: Vec<i16> = template.data.iter().map(|&v| (v as f64 - mean).round() as i16).collect();
    let (centred_sum, norm) =
        centred.iter().fold((0i64, 0i64), |(s, q), &t| (s + t as i64, q + (t as i64) * (t as i64)));
    // Integral images (one row and column of zeros first) of the grey levels and their squares.
    let stride = image.width + 1;
    let (mut sum, mut square) = (vec![0i64; stride * (image.height + 1)], vec![0i64; stride * (image.height + 1)]);
    for y in 0..image.height {
        let (mut row_sum, mut row_square) = (0i64, 0i64);
        for x in 0..image.width {
            let v = image.data[y * image.width + x] as i64;
            (row_sum, row_square) = (row_sum + v, row_square + v * v);
            sum[(y + 1) * stride + x + 1] = sum[y * stride + x + 1] + row_sum;
            square[(y + 1) * stride + x + 1] = square[y * stride + x + 1] + row_square;
        }
    }
    let window = |table: &[i64], x: usize, y: usize| {
        table[(y + th) * stride + x + tw] - table[y * stride + x + tw] - table[(y + th) * stride + x]
            + table[y * stride + x]
    };
    let mut hits = Vec::new();
    for y in 0..=image.height - th {
        for x in 0..=image.width - tw {
            let mut dot = 0i64;
            for ty in 0..th {
                let row = &image.data[(y + ty) * image.width + x..][..tw];
                let trow = &centred[ty * tw..][..tw];
                dot += row.iter().zip(trow).map(|(&v, &t)| v as i16 as i32 * t as i32).sum::<i32>() as i64;
            }
            let window_sum = window(&sum, x, y);
            let variance = window(&square, x, y) as f64 - (window_sum * window_sum) as f64 / n as f64;
            let denominator = (variance.max(0.0) * norm as f64).sqrt();
            if denominator < 1e-3 {
                continue;
            }
            // The template is not exactly zero-mean after rounding: take the image's mean out of the dot.
            let score = ((dot as f64 - window_sum as f64 * centred_sum as f64 / n as f64) / denominator) as f32;
            if score >= min {
                hits.push((score, x, y));
            }
        }
    }
    hits
}

/// The strongest positions, best first; None if the template does not fit.
fn ranked(image: &GrayImage, template: &GrayImage, coarsest: (usize, usize)) -> Option<Vec<(f32, usize, usize)>> {
    if template.width > image.width || template.height > image.height || template.data.is_empty() {
        return None;
    }
    let positions = (image.width - template.width + 1) * (image.height - template.height + 1);
    if positions <= EXHAUSTIVE_POSITIONS {
        let full = Template::new(template);
        let mut hits = Vec::with_capacity(positions);
        for y in 0..=image.height - template.height {
            for x in 0..=image.width - template.width {
                hits.push((full.score_at(image, x, y), x, y));
            }
        }
        return Some(strongest(hits, template, CANDIDATES));
    }
    // Pyramid levels, finest first, while the template stays recognisable.
    let mut levels = vec![(image.clone(), template.clone())];
    while let Some((img, tpl)) = levels.last()
        && tpl.width / 2 >= coarsest.0
        && tpl.height / 2 >= coarsest.1
    {
        let next = (halve(img), halve(tpl));
        levels.push(next);
    }
    let (top_image, top_template) = levels.last().expect("at least the full level");
    let mut candidates = strongest(scan(top_image, top_template, COARSE_MIN), top_template, COARSEST_CANDIDATES);
    for (img, tpl) in levels.iter().rev().skip(1) {
        let finer = Template::new(tpl);
        let (max_x, max_y) = ((img.width - tpl.width) as i32, (img.height - tpl.height) as i32);
        let refined = candidates
            .iter()
            .map(|&(_, x, y)| {
                let (cx, cy) = (x as i32 * 2, y as i32 * 2);
                let xs = (cx - REFINE_RADIUS).clamp(0, max_x) as usize..=(cx + REFINE_RADIUS).clamp(0, max_x) as usize;
                let ys = (cy - REFINE_RADIUS).clamp(0, max_y) as usize..=(cy + REFINE_RADIUS).clamp(0, max_y) as usize;
                exhaustive(img, &finer, xs, ys)
            })
            .collect();
        candidates = strongest(refined, tpl, CANDIDATES);
    }
    Some(candidates)
}

/// The best `count` hits, at most one per template-sized neighbourhood.
fn strongest(mut hits: Vec<(f32, usize, usize)>, template: &GrayImage, count: usize) -> Vec<(f32, usize, usize)> {
    hits.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut kept: Vec<(f32, usize, usize)> = Vec::new();
    for hit in hits {
        let close =
            kept.iter().any(|k| k.1.abs_diff(hit.1) * 2 < template.width && k.2.abs_diff(hit.2) * 2 < template.height);
        if !close {
            kept.push(hit);
            if kept.len() == count {
                break;
            }
        }
    }
    kept
}

fn exhaustive(
    image: &GrayImage,
    template: &Template,
    xs: impl Iterator<Item = usize> + Clone,
    ys: impl Iterator<Item = usize>,
) -> (f32, usize, usize) {
    let mut best = (f32::MIN, 0, 0);
    for y in ys {
        for x in xs.clone() {
            let score = template.score_at(image, x, y);
            if score > best.0 {
                best = (score, x, y);
            }
        }
    }
    best
}

/// Half-size copy (2x2 box average; an odd last row/column is dropped).
fn halve(image: &GrayImage) -> GrayImage {
    let (w, h) = (image.width / 2, image.height / 2);
    let mut out = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let sum = image.get(2 * x, 2 * y) as u16
                + image.get(2 * x + 1, 2 * y) as u16
                + image.get(2 * x, 2 * y + 1) as u16
                + image.get(2 * x + 1, 2 * y + 1) as u16;
            out.data[y * w + x] = ((sum + 2) / 4) as u8;
        }
    }
    out
}

/// TM_CCOEFF_NORMED of an RGB `template` with its top-left at (x, y) in `image` (which must contain it), over all
/// three channels like OpenCV on colour images: look-alikes that only match in grey score lower.
pub fn colour_score(image: &RgbImage, template: &RgbImage, x: usize, y: usize) -> f32 {
    let n = (template.width * template.height) as f32;
    let (mut t_mean, mut i_mean) = ([0.0f32; 3], [0.0f32; 3]);
    for ty in 0..template.height {
        let row = &image.data[((y + ty) * image.width + x) * 3..][..template.width * 3];
        let trow = &template.data[ty * template.width * 3..][..template.width * 3];
        for (i, (&v, &t)) in row.iter().zip(trow).enumerate() {
            i_mean[i % 3] += v as f32 / n;
            t_mean[i % 3] += t as f32 / n;
        }
    }
    let (mut dot, mut t_norm, mut i_norm) = (0.0f32, 0.0f32, 0.0f32);
    for ty in 0..template.height {
        let row = &image.data[((y + ty) * image.width + x) * 3..][..template.width * 3];
        let trow = &template.data[ty * template.width * 3..][..template.width * 3];
        for (i, (&v, &t)) in row.iter().zip(trow).enumerate() {
            let (v, t) = (v as f32 - i_mean[i % 3], t as f32 - t_mean[i % 3]);
            dot += v * t;
            t_norm += t * t;
            i_norm += v * v;
        }
    }
    let denominator = (t_norm * i_norm).sqrt();
    if denominator < 1e-3 { 0.0 } else { dot / denominator }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pseudo-random grey levels with some structure, so windows differ.
    fn image(width: usize, height: usize) -> GrayImage {
        let mut image = GrayImage::new(width, height);
        let mut state = 12345u32;
        for y in 0..height {
            for x in 0..width {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                image.data[y * width + x] = ((x * 3 + y * 5) % 120) as u8 + (state >> 28) as u8 * 8;
            }
        }
        image
    }

    #[test]
    fn the_fast_scan_scores_like_the_exact_one() {
        let source = image(160, 60);
        let template = source.crop(50, 20, 90, 28);
        let exact = Template::new(&template);
        let hits = scan(&source, &template, -1.0);
        assert_eq!(hits.len(), (160 - 40 + 1) * (60 - 8 + 1));
        for &(score, x, y) in hits.iter().step_by(7) {
            assert!((score - exact.score_at(&source, x, y)).abs() < 0.03, "({x}, {y})");
        }
        let best = hits.iter().max_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
        assert_eq!((best.1, best.2), (50, 20));
        assert!(best.0 > 0.98);
    }
}
