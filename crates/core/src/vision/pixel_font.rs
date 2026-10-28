//! Exact reading of a pixel font: the small UI's Character Info names (pure).
//!
//! At "Ideal Ratio" the game draws names in a bitmap font with 1 px strokes: every letter is the same pixels every
//! time, on the same rows of the name plate, with a blank column before the next. So a letter is a lookup, not a
//! guess: cut the plate at blank columns and find each piece's exact bitmap in a table. A letter not in the table is
//! unknown, never a look-alike, which is why learning letters from names the rank lookup confirmed is safe here.
//! OCR (built for document text) misreads this font: "HHai" as "HHaj", "MattyAcid" as "Mattßcid".

use std::collections::HashMap;

use serde_json::{Value, json};

use super::Mask;
use crate::image::{RgbImage, luma};

/// Ink of one column of a piece, top row in the lowest bit.
type Column = u32;

/// Letters that touch their neighbour ("rz" in Xanderz) are found as up to this many known letters side by side.
const MAX_JOINED: usize = 4;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PixelFont {
    /// Each piece's text: one letter, or several for letters the font draws touching ("rz").
    glyphs: HashMap<Vec<Column>, String>,
}

/// The plate's ink, cut at blank columns.
fn pieces(mask: &Mask) -> Vec<Vec<Column>> {
    let column = |x: usize| (0..mask.height.min(32)).fold(0, |bits, y| bits | (Column::from(mask.get(x, y)) << y));
    let mut out = Vec::new();
    let mut piece = Vec::new();
    for x in 0..mask.width {
        match column(x) {
            0 if piece.is_empty() => {}
            0 => out.push(std::mem::take(&mut piece)),
            bits => piece.push(bits),
        }
    }
    if !piece.is_empty() {
        out.push(piece);
    }
    out
}

impl PixelFont {
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// The text of a plate's mask, `?` for each piece the table cannot spell, and how many those are.
    pub fn read(&self, mask: &Mask) -> (String, usize) {
        let mut text = String::new();
        let mut unknown = 0;
        for piece in pieces(mask) {
            match self.spell(&piece, MAX_JOINED) {
                Some(letters) => text.push_str(&letters),
                None => {
                    text.push('?');
                    unknown += 1;
                }
            }
        }
        (text, unknown)
    }

    /// A piece as one known letter, or as up to `joined` known letters drawn touching.
    fn spell(&self, piece: &[Column], joined: usize) -> Option<String> {
        if let Some(text) = self.glyphs.get(piece) {
            return Some(text.clone());
        }
        if joined < 2 {
            return None;
        }
        (1..piece.len()).find_map(|split| {
            let first = self.glyphs.get(&piece[..split])?;
            self.spell(&piece[split..], joined - 1).map(|rest| format!("{first}{rest}"))
        })
    }

    /// Learn the pieces of a plate whose text is known to be `truth`, only where each piece's text is certain: one
    /// piece per letter, or every piece but one already known, the one left spelling what remains (letters drawn
    /// touching). Returns how many pieces were new. A bitmap already standing for other text keeps it: in this font
    /// capital I and lowercase l are the same bar (`ranking::look_alikes`).
    pub fn learn(&mut self, mask: &Mask, truth: &str) -> usize {
        let pieces = pieces(mask);
        let letters: Vec<char> = truth.chars().collect();
        let texts: Vec<String> = if pieces.len() == letters.len() {
            letters.iter().map(char::to_string).collect()
        } else {
            match self.fill_gap(&pieces, &letters) {
                Some(texts) => texts,
                None => return 0,
            }
        };
        let mut added = 0;
        for (piece, text) in pieces.into_iter().zip(texts) {
            if let std::collections::hash_map::Entry::Vacant(slot) = self.glyphs.entry(piece) {
                slot.insert(text);
                added += 1;
            }
        }
        added
    }

    /// Each piece's text when all but one piece spell themselves and match `letters` around the gap.
    fn fill_gap(&self, pieces: &[Vec<Column>], letters: &[char]) -> Option<Vec<String>> {
        let spelt: Vec<Option<String>> = pieces.iter().map(|p| self.spell(p, MAX_JOINED)).collect();
        let gap = spelt.iter().position(Option::is_none)?;
        if spelt.iter().filter(|s| s.is_none()).count() != 1 {
            return None;
        }
        let before: String = spelt[..gap].iter().flatten().map(String::as_str).collect();
        let after: String = spelt[gap + 1..].iter().flatten().map(String::as_str).collect();
        let (n_before, n_after) = (before.chars().count(), after.chars().count());
        if n_before + n_after >= letters.len()
            || letters[..n_before].iter().collect::<String>() != before
            || letters[letters.len() - n_after..].iter().collect::<String>() != after
        {
            return None;
        }
        let middle: String = letters[n_before..letters.len() - n_after].iter().collect();
        Some(spelt.into_iter().map(|s| s.unwrap_or_else(|| middle.clone())).collect())
    }

    /// The shapes this table has that `base` lacks (what learning added to it).
    pub fn added_since(&self, base: &PixelFont) -> PixelFont {
        let glyphs =
            self.glyphs.iter().filter(|(k, _)| !base.glyphs.contains_key(*k)).map(|(k, v)| (k.clone(), v.clone()));
        PixelFont { glyphs: glyphs.collect() }
    }

    /// Another font's letters added (a bundled table and the letters learnt on this computer).
    pub fn merge(&mut self, other: &PixelFont) {
        for (piece, text) in &other.glyphs {
            self.glyphs.entry(piece.clone()).or_insert_with(|| text.clone());
        }
    }

    pub fn to_json(&self) -> String {
        let mut glyphs: Vec<(&Vec<Column>, &String)> = self.glyphs.iter().collect();
        glyphs.sort_by(|a, b| (a.1, a.0).cmp(&(b.1, b.0)));
        let list: Vec<Value> =
            glyphs.into_iter().map(|(columns, text)| json!({"text": text, "columns": columns})).collect();
        serde_json::to_string(&json!({ "glyphs": list })).unwrap_or_default()
    }

    /// None when the text is not a font this module wrote.
    pub fn from_json(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        let mut glyphs = HashMap::new();
        for glyph in value.get("glyphs")?.as_array()? {
            let text = glyph.get("text")?.as_str()?.to_string();
            if text.is_empty() {
                return None;
            }
            let columns: Option<Vec<Column>> = glyph
                .get("columns")?
                .as_array()?
                .iter()
                .map(|c| c.as_u64().and_then(|c| Column::try_from(c).ok()))
                .collect();
            glyphs.insert(columns?, text);
        }
        Some(Self { glyphs })
    }
}

/// The table's baseline row: capitals stand on it from row 2, descenders reach row 12.
const BASELINE_ROW: usize = 10;
/// Font pixels between words; letters stand 1-2 apart (measured on shop chat lines).
const SPACE_GAP: usize = 3;
/// A row inked across more than this share of the line is a separator (sold rows), never text.
const SEPARATOR_SHARE: f32 = 0.7;

/// A line of the bitmap font as captured (`pixels`, the UI drawn at `block` frame px per font pixel: 1 at Ideal
/// Ratio, 2 at its 1440p double), as a mask in font pixels whose baseline is the table's row: chat lines, sold rows
/// and counters have no fixed plate rows to anchor them. None when the box holds no text.
pub fn line_mask(pixels: &RgbImage, block: usize, light_text: bool) -> Option<Mask> {
    let values: Vec<i32> = pixels
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            let v = i32::from(luma(p[0], p[1], p[2]));
            if light_text { 255 - v } else { v }
        })
        .collect();
    let mut sorted = values.clone();
    sorted.sort_unstable();
    let (paper, ink) = (*sorted.get(sorted.len() / 2)?, sorted[0]);
    if paper - ink < 60 {
        return None;
    }
    let cut = (paper + ink) / 2;
    // The owner's own messages are slate blue: ink is ink whatever its tint.
    let on = |x: usize, y: usize| values[y * pixels.width + x] < cut;
    let block = block.max(1);
    // The grid of the doubled font: the phase where the fewest blocks are mixed.
    let mixed = |(ox, oy): (usize, usize)| {
        let (bw, bh) = ((pixels.width - ox) / block, (pixels.height - oy) / block);
        (0..bh)
            .flat_map(|by| (0..bw).map(move |bx| (bx, by)))
            .filter(|&(bx, by)| {
                let first = on(ox + bx * block, oy + by * block);
                (0..block).any(|dy| (0..block).any(|dx| on(ox + bx * block + dx, oy + by * block + dy) != first))
            })
            .count()
    };
    let (ox, oy) = (0..block).flat_map(|oy| (0..block).map(move |ox| (ox, oy))).min_by_key(|&o| mixed(o))?;
    let (w, h) = ((pixels.width - ox) / block, (pixels.height - oy) / block);
    let separator: Vec<bool> = (0..h)
        .map(|y| (0..w).filter(|&x| on(ox + x * block, oy + y * block)).count() as f32 > SEPARATOR_SHARE * w as f32)
        .collect();
    // No letter runs the band's whole height: a full column is the box's side (the meso counter's frame).
    let side: Vec<bool> =
        (0..w).map(|x| (0..h).filter(|&y| on(ox + x * block, oy + y * block)).count() * 10 >= h * 9).collect();
    let fine = |x: usize, y: usize| !separator[y] && !side[x] && on(ox + x * block, oy + y * block);
    // The baseline: the bottom of the dense band (descenders are sparse).
    let rows: Vec<usize> = (0..h).map(|y| (0..w).filter(|&x| fine(x, y)).count()).collect();
    let most = *rows.iter().max()?;
    let base = rows.iter().rposition(|&r| r * 10 >= most * 3)?;
    let mut mask = Mask::new(w, BASELINE_ROW + 4);
    for y in 0..mask.height {
        let Some(src) = (base + y).checked_sub(BASELINE_ROW) else { continue };
        if src >= h {
            continue;
        }
        for x in 0..w {
            mask.set(x, y, fine(x, src));
        }
    }
    Some(mask)
}

/// Phases tried per axis when undoing a scale: quarter steps of a font pixel.
const PHASES: usize = 4;

/// One axis of an area-weighted shrink by `factor` starting `phase` font pixels in: for each output cell, the source
/// indices it covers and how much of each.
fn shrink_weights(len: usize, factor: f32, phase: f32) -> Vec<Vec<(usize, f32)>> {
    let start = phase * factor;
    let cells = ((len as f32 - start) / factor).floor().max(0.0) as usize;
    (0..cells)
        .map(|u| {
            let (a, b) = (start + u as f32 * factor, start + (u + 1) as f32 * factor);
            (a.floor() as usize..(b.ceil() as usize).min(len))
                .filter_map(|x| {
                    let w = (b.min(x as f32 + 1.0) - a.max(x as f32)).max(0.0);
                    (w > 0.0).then_some((x, w / factor))
                })
                .collect()
        })
        .collect()
}

/// A line drawn in the bitmap font and then scaled up smoothly with the whole frame by `factor` (the default UI sizes
/// draw the game at 768 px high and scale it to the window: 1.40625 at 1080p), shrunk back onto the font's own grid
/// and read as `line_mask` reads a line drawn 1:1. The grid's phase is unknown (the frame is scaled as a whole), so
/// every quarter step is tried and the one leaving the fewest half-tone pixels wins.
pub fn unscaled_line_mask(pixels: &RgbImage, factor: f32, light_text: bool) -> Option<Mask> {
    if factor < 1.0 || pixels.is_empty() {
        return None;
    }
    let gray: Vec<f32> = pixels.data.as_chunks::<3>().0.iter().map(|p| f32::from(luma(p[0], p[1], p[2]))).collect();
    let shrink = |px: f32, py: f32| {
        let (xs, ys) = (shrink_weights(pixels.width, factor, px), shrink_weights(pixels.height, factor, py));
        let mut out = RgbImage::new(xs.len(), ys.len());
        for (v, row) in ys.iter().enumerate() {
            for (u, col) in xs.iter().enumerate() {
                let mut sum = 0.0;
                for &(y, wy) in row {
                    for &(x, wx) in col {
                        sum += gray[y * pixels.width + x] * wx * wy;
                    }
                }
                let g = f32::round(sum).clamp(0.0, 255.0) as u8;
                out.set_pixel(u, v, [g, g, g]);
            }
        }
        out
    };
    // Half-tone: neither paper nor ink, by more than a quarter of the contrast.
    let blur = |image: &RgbImage| {
        let values: Vec<i32> = image.data.iter().step_by(3).map(|&v| i32::from(v)).collect();
        let (lo, hi) = (values.iter().min().copied().unwrap_or(0), values.iter().max().copied().unwrap_or(0));
        let quarter = (hi - lo) / 4;
        values.iter().filter(|&&v| v > lo + quarter && v < hi - quarter).count()
    };
    let mut best: Option<(usize, RgbImage)> = None;
    for py in 0..PHASES {
        for px in 0..PHASES {
            let image = shrink(px as f32 / PHASES as f32, py as f32 / PHASES as f32);
            let score = blur(&image);
            if !image.is_empty() && best.as_ref().is_none_or(|(b, _)| score < *b) {
                best = Some((score, image));
            }
        }
    }
    let (_, best) = best?;
    line_mask(&best, 1, light_text)
}

impl PixelFont {
    /// Like `read`, for a whole line: a gap of `SPACE_GAP` blank columns or more is a space.
    pub fn read_line(&self, mask: &Mask) -> (String, usize) {
        let mut text = String::new();
        let mut unknown = 0;
        for (gap, piece) in spaced_pieces(mask) {
            let spelt = self.spell(&piece, MAX_JOINED);
            // A narrow '1' leaves a word-sized gap; numbers never hold spaces.
            let in_number = text.ends_with(|c: char| c.is_ascii_digit() || c == ',')
                && spelt.as_ref().is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit() || c == ','));
            if gap >= SPACE_GAP && !text.is_empty() && !in_number {
                text.push(' ');
            }
            match spelt {
                Some(letters) => text.push_str(&letters),
                None => {
                    text.push('?');
                    unknown += 1;
                }
            }
        }
        (text, unknown)
    }
}

/// The pieces of a mask with the blank columns before each.
fn spaced_pieces(mask: &Mask) -> Vec<(usize, Vec<Column>)> {
    let column = |x: usize| (0..mask.height.min(32)).fold(0, |bits, y| bits | (Column::from(mask.get(x, y)) << y));
    let (mut out, mut piece, mut gap) = (Vec::new(), Vec::new(), 0);
    for x in 0..mask.width {
        match column(x) {
            0 if piece.is_empty() => gap += 1,
            0 => {
                out.push((gap, std::mem::take(&mut piece)));
                gap = 1;
            }
            bits => piece.push(bits),
        }
    }
    if !piece.is_empty() {
        out.push((gap, piece));
    }
    out
}

/// The same font's shapes learnt from the shop's labelled lines (`examples/build_shop_font`), added to the names'.
pub fn shop_small_font() -> PixelFont {
    let mut font = small_name_font();
    font.merge(&PixelFont::from_json(include_str!("../../assets/glyphs/shop_small.json")).unwrap_or_default());
    font
}

/// The name font as drawn at "Ideal Ratio", learnt from labelled live captures (`reading_bench::learn_pixel_names`).
pub fn small_name_font() -> PixelFont {
    PixelFont::from_json(include_str!("../../assets/glyphs/name_small.json")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mask from rows of text ('#' = ink).
    fn mask(rows: &[&str]) -> Mask {
        let mut m = Mask::new(rows[0].len(), rows.len());
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                m.bits[y * m.width + x] = c == '#';
            }
        }
        m
    }

    const AB: [&str; 3] = ["#.##.", "#.#.#", "#.##."];
    const BA: [&str; 3] = ["##.#.", "#.##.", "##.#."];

    #[test]
    fn letters_are_learnt_from_a_known_plate_and_read_exactly() {
        let mut font = PixelFont::default();
        assert_eq!(font.learn(&mask(&AB), "lb"), 2);
        assert_eq!(font.read(&mask(&BA)), ("bl".to_string(), 0));
        assert_eq!(font.read(&mask(&["#..#", "#.##", "#..#"])), ("l?".to_string(), 1), "an unseen shape stays unknown");
    }

    #[test]
    fn touching_letters_are_spelt_from_known_ones() {
        let mut font = PixelFont::default();
        font.learn(&mask(&AB), "lb");
        // "bl" drawn with no blank column between the letters.
        assert_eq!(font.read(&mask(&["###", "#.#", "###"])).0, "?");
        assert_eq!(font.read(&mask(&["##.#", "#.##", "##.#"])).0, "bl");
    }

    #[test]
    fn a_plate_that_does_not_cut_into_one_piece_per_letter_teaches_nothing() {
        let mut font = PixelFont::default();
        assert_eq!(font.learn(&mask(&AB), "lbx"), 0);
        assert!(font.is_empty());
    }

    #[test]
    fn a_piece_of_touching_letters_is_learnt_between_known_ones() {
        let mut font = PixelFont::default();
        font.learn(&mask(&AB), "lb");
        // l, then "xy" drawn touching (a shape no known letters spell), then b.
        let plate = mask(&["#.###.##.", "#.#.#.#.#", "#.###.##."]);
        assert_eq!(font.learn(&plate, "lxyb"), 1);
        assert_eq!(font.read(&plate), ("lxyb".to_string(), 0));
        assert_eq!(font.learn(&plate, "lb"), 0, "the gap must spell something");
    }

    #[test]
    fn a_names_d_reads_as_d_not_as_c_and_l() {
        // The status bar and the store's name plate draw 'd' with its bowl touching the stem: 'c' and 'l' side by
        // side, which is what the table spelt it as before it knew this shape. "od", rows 0-10 (10 is the baseline).
        let od = mask(&[
            "...........",
            "...........",
            "..........#",
            "..........#",
            ".###...##.#",
            "#...#.#..##",
            "#...#.#...#",
            "#...#.#...#",
            "#...#.#...#",
            "#...#.#..##",
            ".###...##.#",
        ]);
        assert_eq!(shop_small_font().read(&od), ("od".to_string(), 0));
    }

    #[test]
    fn a_shape_keeps_its_first_letter_and_the_table_survives_json() {
        let mut font = PixelFont::default();
        font.learn(&mask(&AB), "lb");
        assert_eq!(font.learn(&mask(&AB), "Ib"), 0, "I and l are one bar");
        assert_eq!(font.read(&mask(&AB)).0, "lb");
        assert_eq!(PixelFont::from_json(&font.to_json()), Some(font));
        assert_eq!(PixelFont::from_json("{}"), None);
    }
}
