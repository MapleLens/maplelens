//! Glyph-sized bitmaps and how alike two of them are (pure).

use super::Mask;

/// Widths of one letter vary by a pixel or two at most; more is a different letter.
const MAX_WIDTH_SLACK: isize = 3;

/// A glyph-sized bitmap: bit `x` of `rows[y]` is ink at column `x`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    pub width: usize,
    pub rows: Vec<u64>,
}

impl Bitmap {
    pub(super) fn from_cell(cell: &Mask, x0: usize, x1: usize) -> Self {
        let width = (x1 - x0).min(64);
        let rows = (0..cell.height)
            .map(|y| (0..width).fold(0u64, |acc, x| acc | ((cell.get(x0 + x, y) as u64) << x)))
            .collect();
        Self { width, rows }
    }

    /// Columns `x0..x0 + width` (zero past the end).
    pub(super) fn columns(&self, x0: usize, width: usize) -> Self {
        let mask = if width >= 64 { u64::MAX } else { (1u64 << width) - 1 };
        Self { width, rows: self.rows.iter().map(|r| (r >> x0.min(63)) & mask).collect() }
    }

    pub(super) fn ink(&self) -> u32 {
        self.rows.iter().map(|r| r.count_ones()).sum()
    }
}

/// (differing pixels, pixels inked by either) of `sample` against `glyph`, at the best of a few small shifts:
/// the baseline estimate and the column split can each be a pixel off.
pub(super) fn compare(sample: &Bitmap, glyph: &Bitmap) -> (u32, u32) {
    let spare = sample.width as isize - glyph.width as isize;
    if spare.abs() > MAX_WIDTH_SLACK {
        return (1, 1); // a different letter: widths of one letter vary by a pixel or two at most
    }
    let mut best = (u32::MAX, 1);
    let shifts: &[isize] = if spare == 0 { &[0] } else { &[0, -1, 1] };
    for &dy in &[0isize, -1, 1] {
        for &dx in shifts {
            let offset = if spare > 0 { (spare / 2 + dx).clamp(0, spare) } else { (spare / 2 + dx).clamp(spare, 0) };
            let (mut diff, mut union) = (0u32, 0u32);
            for (y, &s) in sample.rows.iter().enumerate() {
                let gy = y as isize + dy;
                let g = if gy < 0 || gy as usize >= glyph.rows.len() { 0 } else { glyph.rows[gy as usize] };
                let g = if offset >= 0 { g << offset } else { g >> -offset };
                diff += (s ^ g).count_ones();
                union += (s | g).count_ones();
            }
            if (diff as u64) * (best.1 as u64) < (best.0 as u64) * (union.max(1) as u64) {
                best = (diff, union.max(1));
            }
        }
    }
    best
}

pub(super) fn cost(sample: &Bitmap, glyph: &Bitmap) -> f32 {
    let (diff, union) = compare(sample, glyph);
    diff as f32 / union as f32
}
