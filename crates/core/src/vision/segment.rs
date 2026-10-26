//! Cutting a text mask into lines and glyph-sized column runs (pure).
//!
//! Game text is drawn with clear gaps between letters, so a glyph is a run of columns with ink. Lines are
//! found by their dense band (x-height and caps), whose last row is the baseline: that anchors every glyph
//! at the same height whatever row the tooltip happened to start on.

use super::Mask;

/// A row is part of a line's dense band when it holds at least this share of the busiest row's ink.
const DENSE_SHARE: f32 = 0.25;
const DENSE_MIN_PIXELS: usize = 3;
/// Bands thinner than this are the edge of the next line (cap tops peeking in), not a line.
const MIN_BAND_ROWS: usize = 4;
/// Runs with less ink are specks of a bright backdrop showing through, not glyphs.
const MIN_RUN_INK: usize = 5;
/// Text on one line never has a gap this wide; anything further away is other UI, not the title.
const CLUSTER_GAP: usize = 24;

/// No letter has a horizontal stroke this long: such ink is a window border or a UI rule, not text.
const LONG_LINE: usize = 40;

/// `mask` without long horizontal lines (the tooltip's own top border sits right above the title).
pub fn without_rules(mask: &Mask) -> Mask {
    let mut out = mask.clone();
    for y in 0..mask.height {
        let mut x = 0;
        while x < mask.width {
            if !mask.get(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < mask.width && mask.get(x, y) {
                x += 1;
            }
            if x - start >= LONG_LINE {
                (start..x).for_each(|c| out.set(c, y, false));
            }
        }
    }
    out
}

/// Baseline rows of every text line in `mask`, top to bottom.
pub fn baselines(mask: &Mask) -> Vec<usize> {
    let counts: Vec<usize> = (0..mask.height).map(|y| (0..mask.width).filter(|&x| mask.get(x, y)).count()).collect();
    // A text row has inked rows next to it; a lone busy row is a border broken into dashes (too short for
    // `without_rules`), and counting it would set the bar so high that the band, and the baseline, end too early.
    let neighbour =
        |y: usize| counts.get(y.wrapping_sub(1)).copied().unwrap_or(0).max(counts.get(y + 1).copied().unwrap_or(0));
    let busiest = (0..counts.len()).map(|y| counts[y].min(neighbour(y))).max().unwrap_or(0);
    let dense_min = DENSE_MIN_PIXELS.max((busiest as f32 * DENSE_SHARE).ceil() as usize);
    let dense: Vec<bool> = counts.iter().map(|&c| c >= dense_min).collect();
    let mut lines = Vec::new();
    let mut y = 0;
    while y < dense.len() {
        if !dense[y] {
            y += 1;
            continue;
        }
        let start = y;
        let mut end = y; // last dense row of the band
        // A single thin row inside a letter (the gap in an 'e') must not split the band.
        while y < dense.len() && (dense[y] || (y + 1 < dense.len() && dense[y + 1])) {
            if dense[y] {
                end = y;
            }
            y += 1;
        }
        // A band running into the bottom edge is the top of a line the capture cut off.
        if end + 1 - start >= MIN_BAND_ROWS && end + 1 < dense.len() {
            lines.push(end);
        }
    }
    lines
}

/// Rows `baseline + 1 - above .. baseline + 1 + below` of `mask`, padded with blank rows where it ends.
pub fn cell(mask: &Mask, baseline: usize, above: usize, below: usize) -> Mask {
    let mut out = Mask::new(mask.width, above + below);
    for row in 0..above + below {
        let y = baseline as isize + 1 - above as isize + row as isize;
        if y < 0 || y as usize >= mask.height {
            continue;
        }
        for x in 0..mask.width {
            out.set(x, row, mask.get(x, y as usize));
        }
    }
    out
}

/// Columns `x0..x1` holding one glyph (or touching glyphs) of a line cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub x0: usize,
    pub x1: usize,
    pub ink: usize,
}

/// Glyph runs of the line's text, left to right: specks dropped, and only the cluster with the most ink kept
/// (the neighbouring compare tooltip or a bright window edge lands far from the title). Empty when the line
/// is mostly bright backdrop rather than letters (blobs far wider than any glyph).
pub fn runs(cell: &Mask) -> Vec<Run> {
    let all = column_runs(cell);
    let blob_ink: usize = all.iter().filter(|r| r.x1 - r.x0 > MAX_RUN_WIDTH).map(|r| r.ink).sum();
    let mut clusters: Vec<Vec<Run>> = Vec::new();
    for run in all.into_iter().filter(|r| r.x1 - r.x0 <= MAX_RUN_WIDTH) {
        match clusters.last_mut() {
            Some(cluster) if run.x0 - cluster.last().map_or(0, |r| r.x1) <= CLUSTER_GAP => cluster.push(run),
            _ => clusters.push(vec![run]),
        }
    }
    let text = clusters.into_iter().max_by_key(|c| c.iter().map(|r| r.ink).sum::<usize>()).unwrap_or_default();
    if text.iter().map(|r| r.ink).sum::<usize>() < blob_ink { Vec::new() } else { text }
}

/// Three touching letters are about this wide; wider ink is a backdrop blob or a UI edge.
const MAX_RUN_WIDTH: usize = 40;

fn column_runs(cell: &Mask) -> Vec<Run> {
    let ink_in = |x: usize| (0..cell.height).filter(|&y| cell.get(x, y)).count();
    let mut all = Vec::new();
    let mut x = 0;
    while x < cell.width {
        if ink_in(x) == 0 {
            x += 1;
            continue;
        }
        let x0 = x;
        let mut ink = 0;
        while x < cell.width && ink_in(x) > 0 {
            ink += ink_in(x);
            x += 1;
        }
        if ink >= MIN_RUN_INK {
            all.push(Run { x0, x1: x, ink });
        }
    }
    all
}
