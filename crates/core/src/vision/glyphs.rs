//! Reading game text by matching glyph bitmaps against an atlas of the game's own letters (pure).
//!
//! The game draws titles with one font at one size, so a letter looks the same every time up to a little
//! threshold noise from the backdrop showing through. Exact-ish bitmap matching is therefore deterministic
//! (no flip-flopping between frames like an OCR engine) and needs no OCR install. Letters the atlas has never
//! seen read as `?`; they are learnt from reads the market catalog confirms (`Atlas::learn`).

use serde::{Deserialize, Serialize};

use super::Mask;
use super::bitmap::{Bitmap, compare, cost};
use super::segment::{self, Run};

/// Highest share of differing pixels (of the pixels either bitmap inks) that still counts as the same glyph.
const MATCH_MAX: f32 = 0.26;
/// The best letter must beat every other letter by this much, else the glyph reads as unknown.
const AMBIGUOUS_MARGIN: f32 = 0.04;
/// A new sample this close to a known variant adds nothing to the atlas.
const LEARN_DUPLICATE: f32 = 0.08;
/// A sample of a letter the atlas has never seen that is this close to another letter is that letter, mislabelled.
const MIMIC_MAX: f32 = 0.06;
/// How much closer to another letter than to its own a sample must be to count as mislabelled.
const CLEARLY_OTHER: f32 = 0.1;
/// Backdrops change a glyph's edge pixels; a few more variants keep it recognisable over all of them.
const MAX_VARIANTS: usize = 10;
/// Touching letters ('tt', 'ff') are explained as up to this many glyphs.
const MAX_SPLIT: usize = 4;
/// Narrower runs are one letter (the widest single letters, 'm' and 'M', are about 17 px).
const SPLIT_MIN_WIDTH: usize = 12;
/// Mismatch charged for each extra letter a run is split into.
const SPLIT_PENALTY: u32 = 10;
/// Widths an unknown letter between known ones may take when touching letters are cut apart for learning.
const UNKNOWN_WIDTH: std::ops::RangeInclusive<usize> = 3..=18;
/// Letters that this font draws identically: which one it is follows from where it stands in the word.
const LOOKALIKES: [char; 2] = ['I', 'l'];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub ch: char,
    pub bitmap: Bitmap,
}

/// The letters of one game font, plus where its baseline sits in a glyph cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atlas {
    /// Cell rows from the top of the tallest letter down to the baseline (inclusive).
    pub above: usize,
    /// Cell rows below the baseline (descenders).
    pub below: usize,
    /// A gap between glyph runs at least this wide is a space.
    pub space_gap: usize,
    pub glyphs: Vec<Glyph>,
}

#[derive(Serialize, Deserialize)]
struct AtlasFile {
    above: usize,
    below: usize,
    space_gap: usize,
    glyphs: Vec<GlyphFile>,
}

#[derive(Serialize, Deserialize)]
struct GlyphFile {
    ch: char,
    /// One string per cell row, `#` for ink and `.` for none: diffs of the atlas stay readable.
    rows: Vec<String>,
}

/// What a line of text read as.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineRead {
    pub text: String,
    /// Glyph runs no atlas letter explained (each is a `?` in `text`).
    pub unknown: usize,
}

impl Atlas {
    pub fn empty(above: usize, below: usize, space_gap: usize) -> Self {
        Self { above, below, space_gap, glyphs: Vec::new() }
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let file: AtlasFile = serde_json::from_str(json)?;
        let glyphs = file
            .glyphs
            .into_iter()
            .map(|g| {
                let width = g.rows.iter().map(|r| r.len()).max().unwrap_or(0).min(64);
                let rows = g
                    .rows
                    .iter()
                    .map(|r| r.bytes().take(64).enumerate().fold(0u64, |acc, (x, c)| acc | (((c == b'#') as u64) << x)))
                    .collect();
                Glyph { ch: g.ch, bitmap: Bitmap { width, rows } }
            })
            .collect();
        Ok(Self { above: file.above, below: file.below, space_gap: file.space_gap, glyphs })
    }

    pub fn to_json(&self) -> String {
        let mut glyphs: Vec<&Glyph> = self.glyphs.iter().collect();
        glyphs.sort_by_key(|g| g.ch);
        let file = AtlasFile {
            above: self.above,
            below: self.below,
            space_gap: self.space_gap,
            glyphs: glyphs
                .into_iter()
                .map(|g| GlyphFile {
                    ch: g.ch,
                    rows: g
                        .bitmap
                        .rows
                        .iter()
                        .map(|r| (0..g.bitmap.width).map(|x| if r >> x & 1 == 1 { '#' } else { '.' }).collect())
                        .collect(),
                })
                .collect(),
        };
        serde_json::to_string_pretty(&file).unwrap_or_default()
    }

    /// Add `other`'s variants that this atlas does not already have.
    pub fn merge(&mut self, other: &Atlas) -> usize {
        let mut added = 0;
        for glyph in &other.glyphs {
            added += self.add_variant(glyph.ch, &glyph.bitmap) as usize;
        }
        added
    }

    fn add_variant(&mut self, ch: char, bitmap: &Bitmap) -> bool {
        let known: Vec<&Glyph> = self.glyphs.iter().filter(|g| g.ch == ch).collect();
        if known.len() >= MAX_VARIANTS || known.iter().any(|g| cost(bitmap, &g.bitmap) <= LEARN_DUPLICATE) {
            return false;
        }
        // A sample that is plainly another letter was cut or labelled wrongly; learning it would make that letter read
        // as this one from now on ('Pan LKd' after an 'l' was taught as 'K'). Similar letters ('i' and 'l') stay
        // learnable: the sample must be clearly closer to the other letter than to its own, or, for a letter not
        // seen yet, all but identical to another one.
        let own = known.iter().map(|g| cost(bitmap, &g.bitmap)).reduce(f32::min);
        let other = self
            .glyphs
            .iter()
            .filter(|g| g.ch != ch && !(LOOKALIKES.contains(&g.ch) && LOOKALIKES.contains(&ch)))
            .map(|g| cost(bitmap, &g.bitmap))
            .reduce(f32::min)
            .unwrap_or(f32::MAX);
        let wrong = match own {
            Some(own) => other + CLEARLY_OTHER < own && other <= MATCH_MAX,
            None => other <= MIMIC_MAX,
        };
        if wrong {
            return false;
        }
        self.glyphs.push(Glyph { ch, bitmap: bitmap.clone() });
        true
    }

    fn line_runs(&self, mask: &Mask) -> Vec<(Mask, Vec<Run>)> {
        let mask = segment::without_rules(mask);
        segment::baselines(&mask)
            .into_iter()
            .map(|baseline| {
                let cell = segment::cell(&mask, baseline, self.above, self.below);
                let runs = segment::runs(&cell);
                (cell, runs)
            })
            .filter(|(_, runs)| !runs.is_empty())
            .collect()
    }

    /// Every text line in `mask`, read and joined with spaces.
    pub fn read(&self, mask: &Mask) -> LineRead {
        let mut out = LineRead::default();
        for (cell, runs) in self.line_runs(mask) {
            let line = self.read_runs(&cell, &runs);
            if !out.text.is_empty() {
                out.text.push(' ');
            }
            out.text.push_str(&line.text);
            out.unknown += line.unknown;
        }
        out
    }

    fn read_runs(&self, cell: &Mask, runs: &[Run]) -> LineRead {
        let mut out = LineRead::default();
        let mut word_start = true;
        for (i, run) in runs.iter().enumerate() {
            if i > 0 && run.x0 - runs[i - 1].x1 >= self.space_gap {
                out.text.push(' ');
                word_start = true;
            }
            let sample = Bitmap::from_cell(cell, run.x0, run.x1);
            match self.classify(&sample, word_start) {
                Some(text) => out.text.push_str(&text),
                None => {
                    out.text.push('?');
                    out.unknown += 1;
                }
            }
            word_start = false;
        }
        out
    }

    /// The letter(s) a run shows: one glyph, else a few touching ones, else None.
    fn classify(&self, sample: &Bitmap, word_start: bool) -> Option<String> {
        let mut scored: Vec<(f32, char)> = self.glyphs.iter().map(|g| (cost(sample, &g.bitmap), g.ch)).collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        if let Some(&(best, ch)) = scored.first()
            && best <= MATCH_MAX
        {
            let lookalike = LOOKALIKES.contains(&ch)
                && scored.iter().any(|&(c, other)| other != ch && LOOKALIKES.contains(&other) && c <= best + 0.02);
            if lookalike {
                return Some(if word_start { 'I' } else { 'l' }.to_string());
            }
            // A noisy 'B' can come within reach of an 'E': when another letter scores almost as well, '?' is the
            // honest answer (OCR or the catalog's wildcard settle it), never a guess.
            let rival = scored
                .iter()
                .find(|&&(_, other)| other != ch && !(LOOKALIKES.contains(&other) && LOOKALIKES.contains(&ch)));
            if rival.is_some_and(|&(c, _)| c - best < AMBIGUOUS_MARGIN) {
                return None;
            }
            return Some(ch.to_string());
        }
        self.split(sample)
    }

    /// Explain a wide run as 2..=MAX_SPLIT touching letters, cheapest total mismatch first. Punctuation never
    /// touches a letter, and every extra letter costs a penalty, so a single unknown letter ('H') is not passed
    /// off as a chain of strokes ('l-l').
    fn split(&self, sample: &Bitmap) -> Option<String> {
        let width = sample.width;
        if width < SPLIT_MIN_WIDTH {
            return None;
        }
        // best[p] = (differing pixels incl. penalties, inked pixels, letters) explaining columns 0..p
        let mut best: Vec<Option<(u32, u32, String)>> = vec![None; width + 2];
        best[0] = Some((0, 0, String::new()));
        for p in 0..=width {
            let Some((diff, union, text)) = best[p].clone() else { continue };
            if text.chars().count() >= MAX_SPLIT {
                continue;
            }
            for glyph in self.glyphs.iter().filter(|g| g.ch.is_alphanumeric()) {
                let piece = sample.columns(p, glyph.bitmap.width);
                if piece.ink() == 0 {
                    continue;
                }
                let (d, u) = compare(&piece, &glyph.bitmap);
                if d as f32 / u as f32 > MATCH_MAX {
                    continue;
                }
                let penalty = if text.is_empty() { 0 } else { SPLIT_PENALTY };
                // The next letter may start right after this one or share its last column.
                for next in [p + glyph.bitmap.width, p + glyph.bitmap.width - 1] {
                    let next = next.min(width + 1);
                    let candidate = (diff + d + penalty, union + u, format!("{text}{}", glyph.ch));
                    let better = best[next]
                        .as_ref()
                        .is_none_or(|b| (candidate.0 as u64) * (b.1 as u64) < (b.0 as u64) * (candidate.1 as u64));
                    if better && next > p {
                        best[next] = Some(candidate);
                    }
                }
            }
        }
        [width, width + 1]
            .into_iter()
            .filter_map(|end| best[end].clone())
            .filter(|(d, u, text)| text.chars().count() >= 2 && (*d as f32 / *u as f32) <= MATCH_MAX)
            .min_by(|a, b| (a.0 as u64 * b.1 as u64).cmp(&(b.0 as u64 * a.1 as u64)))
            .map(|(_, _, text)| text)
    }

    /// Learn the letters of `text` from a mask that shows that text. The mask's words (split at wide gaps and line
    /// ends) are lined up with the words of `text`; extra words at either end (bright scenery beside the title) are
    /// left out when exactly one placement fits. Within a word, glyph runs line up one-to-one with its letters, or a
    /// run holding touching letters ("AP") is cut where its known letters match the atlas, which teaches the one
    /// letter it did not know. Returns how many new variants were added.
    pub fn learn(&mut self, mask: &Mask, text: &str) -> usize {
        let lines = self.line_runs(mask);
        let mut words: Vec<Vec<(&Mask, Run)>> = Vec::new();
        for (cell, runs) in &lines {
            for (i, run) in runs.iter().enumerate() {
                if i == 0 || run.x0 - runs[i - 1].x1 >= self.space_gap {
                    words.push(Vec::new());
                }
                if let Some(word) = words.last_mut() {
                    word.push((cell, *run));
                }
            }
        }
        let texts: Vec<Vec<char>> = text.split_whitespace().map(|w| w.chars().collect()).collect();
        let Some(first) = align_words(&words, &texts) else { return 0 };
        let mut samples = Vec::new();
        for (runs, letters) in words[first..].iter().zip(&texts) {
            let bitmaps: Vec<Bitmap> = runs.iter().map(|(cell, run)| Bitmap::from_cell(cell, run.x0, run.x1)).collect();
            samples.extend(self.word_samples(&bitmaps, letters));
        }
        samples.iter().map(|(ch, bitmap)| self.add_variant(*ch, bitmap) as usize).sum()
    }

    /// The (letter, bitmap) pairs a word's runs teach: one per run, or with touching letters split apart. Nothing
    /// when no way of sharing the letters among the runs fits.
    fn word_samples(&self, runs: &[Bitmap], letters: &[char]) -> Vec<(char, Bitmap)> {
        if runs.len() == letters.len() {
            return letters.iter().copied().zip(runs.iter().cloned()).collect();
        }
        self.share(runs, letters).map(|(_, samples)| samples).unwrap_or_default()
    }

    /// The cheapest way to share `letters` among `runs` in order (each run one to MAX_SPLIT letters), as (mismatch,
    /// samples).
    fn share(&self, runs: &[Bitmap], letters: &[char]) -> Option<(f32, Vec<(char, Bitmap)>)> {
        let (run, rest) = runs.split_first()?;
        let mut best: Option<(f32, Vec<(char, Bitmap)>)> = None;
        for take in 1..=MAX_SPLIT.min(letters.len()) {
            if letters.len() - take < rest.len() || (rest.is_empty() && take != letters.len()) {
                continue;
            }
            let Some((cost, mut samples)) = self.split_as(run, &letters[..take]) else { continue };
            let tail = if rest.is_empty() { Some((0.0, Vec::new())) } else { self.share(rest, &letters[take..]) };
            let Some((tail_cost, tail_samples)) = tail else { continue };
            samples.extend(tail_samples);
            let total = cost + tail_cost;
            if best.as_ref().is_none_or(|(b, _)| total < *b) {
                best = Some((total, samples));
            }
        }
        best
    }

    /// `sample` explained as exactly `letters` side by side: known letters must match an atlas variant, at most one
    /// unknown letter takes the columns between them. Returns (mismatch of the known letters, samples).
    fn split_as(&self, sample: &Bitmap, letters: &[char]) -> Option<(f32, Vec<(char, Bitmap)>)> {
        if let [ch] = letters {
            // A run that is one letter must look like that letter, if the atlas knows it.
            let best = self.glyphs.iter().filter(|g| g.ch == *ch).map(|g| cost(sample, &g.bitmap)).reduce(f32::min);
            return match best {
                Some(c) if c > MATCH_MAX => None,
                known => Some((known.unwrap_or(0.0), vec![(*ch, sample.clone())])),
            };
        }
        let unknown = letters.iter().filter(|&&ch| !self.glyphs.iter().any(|g| g.ch == ch)).count();
        if unknown > 1 || unknown == letters.len() {
            return None;
        }
        let (diff, union, pieces) = self.split_from(sample, letters, 0)?;
        Some((diff as f32 / union.max(1) as f32, pieces))
    }

    /// `split_as` from column `p`: the best (differing, inked) over the known letters, and the pieces.
    fn split_from(&self, sample: &Bitmap, letters: &[char], p: usize) -> Option<Split> {
        let Some((&ch, rest)) = letters.split_first() else {
            return (p + 1 >= sample.width && p <= sample.width + 1).then_some((0, 0, Vec::new()));
        };
        // (width, next start, differing, inked) for each way this letter can sit at p.
        let mut options: Vec<(usize, usize, u32, u32)> = Vec::new();
        let variants: Vec<&Glyph> = self.glyphs.iter().filter(|g| g.ch == ch).collect();
        if variants.is_empty() {
            for width in UNKNOWN_WIDTH {
                if p + width <= sample.width + 1 && sample.columns(p, width).ink() > 0 {
                    options.push((width, p + width, 0, 0));
                }
            }
        }
        for glyph in variants {
            let width = glyph.bitmap.width;
            let piece = sample.columns(p, width);
            if piece.ink() == 0 {
                continue;
            }
            let (d, u) = compare(&piece, &glyph.bitmap);
            if d as f32 / u as f32 <= MATCH_MAX {
                options.push((width, p + width, d, u));
                options.push((width, p + width - 1, d, u));
            }
        }
        let mut best: Option<Split> = None;
        for (width, next, d, u) in options {
            let Some((td, tu, tail)) = self.split_from(sample, rest, next) else { continue };
            let (d, u) = (d + td, u + tu);
            if best.as_ref().is_none_or(|b| (d as u64) * (b.1.max(1) as u64) < (b.0 as u64) * (u.max(1) as u64)) {
                let mut all = vec![(ch, sample.columns(p, width))];
                all.extend(tail);
                best = Some((d, u, all));
            }
        }
        best
    }
}

/// A run cut into letters: (differing pixels, inked pixels) over its known letters, and each letter's bitmap.
type Split = (u32, u32, Vec<(char, Bitmap)>);

/// Where the title's words start among the mask's words: the only placement in which every word has a plausible
/// number of glyph runs for its letters (touching letters make fewer, never more). None when no placement or more
/// than one fits.
fn align_words<T>(words: &[Vec<T>], texts: &[Vec<char>]) -> Option<usize> {
    if texts.is_empty() || words.len() < texts.len() {
        return None;
    }
    let fits = |k: usize| {
        words[k..k + texts.len()]
            .iter()
            .zip(texts)
            .all(|(runs, letters)| runs.len() <= letters.len() && runs.len() * MAX_SPLIT >= letters.len())
    };
    let mut placements = (0..=words.len() - texts.len()).filter(|&k| fits(k));
    match (placements.next(), placements.next()) {
        (Some(k), None) => Some(k),
        _ => None,
    }
}
