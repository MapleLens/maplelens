//! Reading a line: the cheapest path of letters across its pen positions (dynamic programming, no segmentation).

use super::{ASC, Line, R, ROWS, VectorFont, draw};

/// Pen positions are tried on this grid (frame px): the game places letters at quarter pixels and finer.
const STEP: f32 = 0.25;
const PHASES: usize = 4;
/// How far a letter may sit from where the previous one's advance puts it (frame px): the game rounds positions,
/// advances are learnt to a fraction of a pixel, and its pairs are kerned. At 1.2 the letter after an 'a' sits more
/// than a pixel off ("zabaraz", "lazy", "Crazyyyy" read '?' there with +-1): 237 corpus lines exact, 232 with +-1.
/// Training aligns with the same window: with a narrow one an advance far from the game's forced its letters into
/// wrong places, and the templates learnt from them blurred (a doubled 'I').
const JITTER: [f32; 13] = [-1.5, -1.25, -1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5];
/// Cost per reference pixel a letter sits off its advance: pairs the game kerns ("ft", "yr") may sit a pixel off,
/// but a letter that only fits off its advance is less likely; 'I' and 'l' differ mostly by their advance.
const JITTER_COST: f32 = 0.4;
/// Cost of each letter (in squared darkness per reference pixel of area): one 'm' beats 'r' + 'n'.
const LETTER_COST: f32 = 0.6;
/// Punctuation is a few dots of ink: it needs a little more evidence than a letter, or a speck of anti-aliasing in
/// a word gap reads as ':' or '.' (at 1.5 a line's closing full stop was not worth reading at all).
const PUNCTUATION_COST: f32 = 0.9;
/// Cost per reference column of a span no letter explains, read as '?'. Measured on held-out names: true letters
/// cost at most 0.27 a column (99th percentile 0.27, worst 0.51), letters standing in for unseen ones 0.3-1.9.
const UNKNOWN_COST: f32 = 0.4;
/// Widths (reference px) an unknown span may take: from a narrow letter to a wide one.
const UNKNOWN_WIDTH: (f32, f32) = (3.0, 15.0);
/// An unknown span needs some real ink (a stray anti-aliased edge is not a letter).
const UNKNOWN_INK: f32 = 0.3;
/// Where the first pen may be, before the first ink (reference px): a letter's left side bearing.
const LEAD: f32 = 3.0;
/// Baseline offsets tried around the estimate (frame px) by fitting the line's row profile.
const BASE_SEARCH: f32 = 1.0;
/// A blank run at least this wide (frame px) is crossed in one step, as one space: right-aligned digits and short
/// chat lines leave long blank stretches that a space per few pixels would make too costly to keep.
const BLANK_RUN: usize = 4;
/// Columns this faint are blank.
const BLANK: f32 = 0.05;
/// Letters tried off their advance at each pen position, the best fits where the advance puts them.
const SCREEN: usize = 20;
/// A path more than this behind the best one reaching the same columns is dropped (cost units): several letters'
/// worth, so the right reading, briefly behind a look-alike, survives.
const PRUNE_MARGIN: f32 = 6.0;

/// What a line reads as: its text ('?' for each span no letter explains), how many such spans, and the path's cost.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub text: String,
    pub unknown: usize,
    pub cost: f32,
    /// Each letter's pen (frame px), for learning.
    pub pens: Vec<(char, f32)>,
}

/// A letter drawn at one sub-pixel phase on this line: its darkness over its advance cell, column by column (all of
/// the line's rows), and the sum of its squares.
struct Cell {
    width: usize,
    columns: Vec<f32>,
    /// Each column's total darkness, for a cheap bound on the error.
    sums: Vec<f32>,
    energy: f32,
    /// Its ink right of the cell ('f' and 't' reach into the next letter's cell), column by column.
    spill: Vec<f32>,
    /// The pen's advance to the next letter, in frame px.
    advance: f32,
}

/// The line, column by column, with running sums of squared darkness.
struct Columns {
    height: usize,
    dark: Vec<f32>,
    prefix: Vec<f32>,
    max: Vec<f32>,
    sums: Vec<f32>,
}

impl Columns {
    fn new(line: &Line) -> Self {
        let mut dark = vec![0.0f32; line.width * line.height];
        for y in 0..line.height {
            for x in 0..line.width {
                dark[x * line.height + y] = line.get(x, y);
            }
        }
        let mut prefix = vec![0.0f32; line.width + 1];
        let mut max = vec![0.0f32; line.width];
        let mut sums = vec![0.0f32; line.width];
        for x in 0..line.width {
            let col = &dark[x * line.height..][..line.height];
            prefix[x + 1] = prefix[x] + col.iter().map(|v| v * v).sum::<f32>();
            max[x] = col.iter().copied().fold(0.0, f32::max);
            sums[x] = col.iter().sum();
        }
        Self { height: line.height, dark, prefix, max, sums }
    }

    fn width(&self) -> usize {
        self.max.len()
    }

    /// Squared darkness of columns [a, b).
    fn energy(&self, a: usize, b: usize) -> f32 {
        let (a, b) = (a.min(self.width()), b.min(self.width()));
        if b > a { self.prefix[b] - self.prefix[a] } else { 0.0 }
    }

    /// Squared error of `cell` with its first column at x, the previous letter's spill (starting at column
    /// `spill.0`) taken off what is seen; None when it cannot beat `budget`.
    fn cost(&self, cell: &Cell, x: usize, budget: f32, spill: Option<(usize, &[f32])>) -> Option<f32> {
        let end = (x + cell.width).min(self.width());
        if end <= x {
            return Some(0.0);
        }
        let h = self.height;
        // The spill's columns inside this cell, and what is seen there once it is taken off.
        let spilled = spill.map(|(from, columns)| (from, columns.len() / h, columns));
        let spill_at = |col: usize| {
            spilled.and_then(|(from, n, columns)| {
                (col >= from && col < from + n).then(|| &columns[(col - from) * h..][..h])
            })
        };
        let mut observed = self.energy(x, end);
        if let Some((from, n, _)) = spilled {
            for col in from.max(x)..(from + n).min(end) {
                let d = &self.dark[col * h..][..h];
                let s = spill_at(col).unwrap_or(d);
                observed += d.iter().zip(s).map(|(a, b)| (a - b).max(0.0).powi(2) - a * a).sum::<f32>();
            }
        }
        // |a - b| >= | |a| - |b| |: a cheap bound before the dot product.
        let bound = (observed.sqrt() - cell.energy.sqrt()).powi(2);
        if bound > budget {
            return None;
        }
        // Column by column, (sum of differences)^2 <= height * sum of squared differences (Cauchy-Schwarz): a bound
        // a column long instead of a column's every pixel. The spill only lowers what is seen, so it is ignored here.
        if spill.is_none() {
            let columns_bound: f32 =
                (0..end - x).map(|k| (self.sums[x + k] - cell.sums[k]).powi(2)).sum::<f32>() / h as f32;
            if columns_bound > budget {
                return None;
            }
        }
        let mut dot = 0.0f32;
        for k in 0..end - x {
            let m = &cell.columns[k * h..][..h];
            let d = &self.dark[(x + k) * h..][..h];
            dot += match spill_at(x + k) {
                Some(s) => m.iter().zip(d).zip(s).map(|((m, d), s)| m * (d - s).max(0.0)).sum::<f32>(),
                None => m.iter().zip(d).map(|(a, b)| a * b).sum::<f32>(),
            };
        }
        // The cell's own energy, less any columns past the line's end.
        let model =
            if end - x == cell.width { cell.energy } else { cell.columns[..(end - x) * h].iter().map(|v| v * v).sum() };
        Some(observed - 2.0 * dot + model)
    }
}

/// Every letter of the font drawn on this line at every phase.
fn cells(line: &Line, font: &VectorFont, base: f32) -> Vec<(char, [Cell; PHASES])> {
    let h = line.height;
    font.glyphs
        .iter()
        .map(|(&c, glyph)| {
            let cell = |phase: usize| {
                let q = phase as f32 * STEP;
                let advance = glyph.advance * line.scale;
                let width = (q + advance).ceil().max(1.0) as usize;
                let patch = draw(glyph, q, base, line.scale);
                let reach = (patch.x0 + patch.width as i32).max(width as i32) as usize;
                let mut all = vec![0.0f32; reach * h];
                for k in 0..reach {
                    let px = k as i32 - patch.x0;
                    if px < 0 || px as usize >= patch.width {
                        continue;
                    }
                    for y in 0..h {
                        let py = y as i32 - patch.y0;
                        if py >= 0 && (py as usize) < patch.height {
                            all[k * h + y] = patch.get(px as usize, py as usize);
                        }
                    }
                }
                let spill = all.split_off(width * h);
                let columns = all;
                let energy = columns.iter().map(|v| v * v).sum();
                let sums = columns.chunks_exact(h).map(|c| c.iter().sum()).collect();
                Cell { width, columns, sums, energy, spill, advance }
            };
            (c, [cell(0), cell(1), cell(2), cell(3)])
        })
        .collect()
}

/// The baseline (frame rows) whose rows best match the font's average letter, near the line's own estimate.
fn fit_baseline(line: &Line, font: &VectorFont) -> f32 {
    let mut profile = vec![0.0f32; ROWS];
    for glyph in font.glyphs.values() {
        for (row, p) in profile.iter_mut().enumerate() {
            *p += glyph.template[row * glyph.cols..][..glyph.cols].iter().sum::<f32>();
        }
    }
    let rows: Vec<f32> = (0..line.height).map(|y| (0..line.width).map(|x| line.get(x, y)).sum()).collect();
    let norm = |v: &[f32]| {
        let total: f32 = v.iter().sum();
        v.iter().map(|x| x / total.max(1e-6)).collect::<Vec<f32>>()
    };
    let rows = norm(&rows);
    let s = line.scale;
    let mut best = (f32::INFINITY, line.base);
    let mut offset = -BASE_SEARCH;
    while offset <= BASE_SEARCH {
        let base = line.base + offset;
        // The profile's rows drawn like a template: each frame row averages the cells it covers.
        let mut model = vec![0.0f32; line.height];
        for (y, m) in model.iter_mut().enumerate() {
            let (a, b) = (
                ((y as f32 - base) / s + ASC as f32) * R as f32,
                ((y as f32 + 1.0 - base) / s + ASC as f32) * R as f32,
            );
            let (lo, hi) = (a.max(0.0).floor() as usize, (b.min(ROWS as f32).ceil().max(0.0) as usize));
            *m = (lo..hi.max(lo)).map(|c| profile[c] * ((b.min(c as f32 + 1.0) - a.max(c as f32)).max(0.0))).sum();
        }
        let model = norm(&model);
        let err: f32 = model.iter().zip(&rows).map(|(m, r)| (m - r).powi(2)).sum();
        if err < best.0 {
            best = (err, base);
        }
        offset += STEP;
    }
    best.1
}

/// The cheapest path at one baseline. `sequence` fixes the letters in order (alignment of a known text).
fn path(line: &Line, font: &VectorFont, base: f32, sequence: Option<&[char]>) -> Option<Reading> {
    let (first, last) = line.ink_span()?;
    let s = line.scale;
    let area = s * s; // costs are per reference pixel of area, so every UI size weighs letters alike
    let columns = Columns::new(line);
    let drawn = cells(line, font, base);
    let index = |c: char| drawn.iter().position(|(d, _)| *d == c);
    let n = (line.width as f32 / STEP) as usize + 2;
    let slots = sequence.map_or(1, |q| q.len() + 1);
    let seq_index: Option<Vec<usize>> = match sequence {
        Some(seq) => Some(seq.iter().map(|&c| index(c)).collect::<Option<Vec<usize>>>()?),
        None => None,
    };
    let mut best = vec![f32::INFINITY; n * slots];
    let mut back: Vec<Option<(usize, char, f32)>> = vec![None; n * slots];
    let start = (first as f32 - LEAD * s).max(0.0);
    let mut p = (start / STEP).floor() * STEP;
    while p <= first as f32 {
        let i = (p / STEP).round() as usize;
        best[i * slots] = columns.energy(0, p.floor() as usize) / area;
        p += STEP;
    }
    // The best cost of any path so far plus leaving the columns after it blank: a bound on what a good path costs.
    let mut frontier = f32::INFINITY;
    let mut frontier_x = 0usize;
    for i in 0..n {
        let pen = i as f32 * STEP;
        if pen > last as f32 + 2.0 {
            break;
        }
        let x = pen.floor() as usize;
        if frontier.is_finite() && x > frontier_x {
            frontier += columns.energy(frontier_x, x) / area;
            frontier_x = x;
        }
        for m in 0..slots {
            let here = best[i * slots + m];
            if !here.is_finite() {
                continue;
            }
            if sequence.is_none() {
                if here > frontier + PRUNE_MARGIN {
                    continue;
                }
                if here < frontier {
                    (frontier, frontier_x) = (here, x);
                }
            }
            let next_slot = if sequence.is_some() { m + 1 } else { 0 };
            let spill = back[i * slots + m].and_then(|(_, c, q)| {
                let (_, phases) = drawn.iter().find(|(d, _)| *d == c)?;
                let qx = q.floor();
                let cell = &phases[((q - qx) / STEP).round() as usize % PHASES];
                (!cell.spill.is_empty()).then(|| (qx as usize + cell.width, cell.spill.as_slice()))
            });
            let options = match &seq_index {
                None => 0..drawn.len(),
                Some(seq) if m < seq.len() => seq[m]..seq[m] + 1,
                Some(_) => 0..0,
            };
            // Reading freely, every letter is tried where the advance puts it, and only the few that fit best there
            // are tried a little off it too (pairs the game kerns): most of a line's work otherwise.
            let screened: Vec<usize> = if sequence.is_some() {
                options.collect()
            } else {
                let mut fits: Vec<(f32, usize)> = Vec::with_capacity(SCREEN + 1);
                for g in options {
                    let cell = &drawn[g].1[((pen - pen.floor()) / STEP).round() as usize % PHASES];
                    let worst = if fits.len() == SCREEN { fits[SCREEN - 1].0 } else { f32::INFINITY };
                    if let Some(err) = columns.cost(cell, x, worst, spill) {
                        let at = fits.partition_point(|f| f.0 <= err);
                        fits.insert(at, (err, g));
                        fits.truncate(SCREEN);
                    }
                }
                fits.into_iter().map(|(_, g)| g).collect()
            };
            for g in screened {
                let (c, phases) = &drawn[g];
                for &j in &JITTER {
                    let q = pen + j;
                    if q < 0.0 {
                        continue;
                    }
                    let qx = q.floor();
                    let cell = &phases[((q - qx) / STEP).round() as usize % PHASES];
                    let k = ((q + cell.advance) / STEP).round() as usize;
                    if k >= n {
                        continue;
                    }
                    let target = k * slots + next_slot;
                    let budget = (best[target] - here - LETTER_COST.min(PUNCTUATION_COST)) * area;
                    let Some(err) = columns.cost(cell, qx as usize, budget, spill) else { continue };
                    let letter = if c.is_ascii_punctuation() { PUNCTUATION_COST } else { LETTER_COST };
                    let off = if sequence.is_some() { 0.0 } else { JITTER_COST * j.abs() / s };
                    let total = here + err / area + letter + off;
                    if total < best[target] {
                        best[target] = total;
                        back[target] = Some((i * slots + m, *c, q));
                    }
                }
            }
            if sequence.is_none() && x < columns.width() && columns.max[x] < BLANK {
                let run = columns.max[x..].iter().take_while(|&&v| v < BLANK).count();
                if run >= BLANK_RUN && x + run < columns.width() {
                    let ink = (x + run) as f32;
                    let mut q = ((ink - LEAD * s).max(pen + STEP) / STEP).ceil() * STEP;
                    while q <= ink {
                        let k = (q / STEP).round() as usize;
                        let total = here + LETTER_COST;
                        if k < n && total < best[k * slots] {
                            best[k * slots] = total;
                            back[k * slots] = Some((i * slots, ' ', pen));
                        }
                        q += STEP;
                    }
                }
            }
            if sequence.is_none() {
                let mut w = UNKNOWN_WIDTH.0 * s;
                while w <= UNKNOWN_WIDTH.1 * s {
                    let (c0, c1) = (x, ((pen + w).ceil() as usize).min(columns.width()));
                    if c1 > c0 && columns.max[c0..c1].iter().copied().fold(0.0, f32::max) >= UNKNOWN_INK {
                        let total = here + UNKNOWN_COST * w / s + LETTER_COST;
                        let k = ((pen + w) / STEP).round() as usize;
                        if k < n && total < best[k * slots] {
                            best[k * slots] = total;
                            back[k * slots] = Some((i * slots, '?', pen));
                        }
                    }
                    w += 2.0 * STEP;
                }
            }
        }
    }
    let end_slot = slots - 1;
    let (cost, mut at) = (0..n)
        .filter(|&i| i as f32 * STEP >= last as f32 - 1.0 && best[i * slots + end_slot].is_finite())
        .map(|i| {
            let pen = i as f32 * STEP;
            (best[i * slots + end_slot] + columns.energy(pen.floor() as usize, line.width) / area, i * slots + end_slot)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let mut pens = Vec::new();
    while let Some((prev, c, q)) = back[at] {
        pens.push((c, q));
        at = prev;
    }
    pens.reverse();
    let text = if sequence.is_some() { pens.iter().map(|&(c, _)| c).collect() } else { absorb(&pens) };
    let unknown = text.matches('?').count();
    Some(Reading { text: text.trim().to_string(), unknown, cost, pens })
}

/// The text of a path, a letter touching an unknown span taken into it: a span no letter explains often leaves a
/// sliver that a look-alike fits ("?v" for an unseen "w"), and a wrong letter is worse than a gap OCR can fill.
fn absorb(pens: &[(char, f32)]) -> String {
    let chars: Vec<char> = pens.iter().map(|&(c, _)| c).collect();
    let gap = |i: usize| chars.get(i) == Some(&'?');
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let touching = c.is_alphanumeric() && ((i > 0 && gap(i - 1)) || gap(i + 1));
        let c = if touching { '?' } else { c };
        if !(c == '?' && out.ends_with('?')) {
            out.push(c);
        }
    }
    out
}

/// How long each stage of a read takes (ms): drawing the letters, fitting the baseline, the path.
#[cfg(test)]
pub(super) fn stage_times(line: &Line, font: &VectorFont) -> (f64, f64, f64) {
    let t = std::time::Instant::now();
    let _ = cells(line, font, line.base);
    let drawn = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let base = fit_baseline(line, font);
    let fitted = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let _ = path(line, font, base, None);
    (drawn, fitted, t.elapsed().as_secs_f64() * 1e3)
}

/// The line's text at its own baseline estimate, without fitting it (for debugging the fit).
#[cfg(test)]
pub(super) fn read_at(line: &Line, font: &VectorFont) -> Option<Reading> {
    path(line, font, line.base, None)
}

/// The line's text as the font reads it.
pub fn read(line: &Line, font: &VectorFont) -> Option<Reading> {
    path(line, font, fit_baseline(line, font), None)
}

/// Where each letter of a known `text` sits on the line, and the baseline: for learning. The baseline is searched
/// more widely than when reading: learning can afford it, and a misplaced line teaches wrong templates.
pub fn align(line: &Line, font: &VectorFont, text: &str) -> Option<(Reading, f32)> {
    let sequence: Vec<char> = text.chars().collect();
    if sequence.iter().any(|c| !font.glyphs.contains_key(c)) {
        return None;
    }
    let fitted = fit_baseline(line, font);
    [-0.25f32, 0.0, 0.25]
        .iter()
        .filter_map(|d| path(line, font, fitted + d, Some(&sequence)).map(|r| (r, fitted + d)))
        .min_by(|a, b| a.0.cost.total_cmp(&b.0.cost))
}
