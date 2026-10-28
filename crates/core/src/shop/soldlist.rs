//! Rows of the Sold Items dialog: three text lines each (item, buyer, quantity), newest on top.
//!
//! Rows are compared by the ink of their text, trimmed to its bounding box: the slots are a fractional number of
//! pixels apart, so the same row looks different around its edges depending on which slot it is in.

use super::layout::{Point, View, sold_list, total_label};
use super::lines;
use super::lines::{Line, MIN_INK, is_grey, make_line};
use crate::image::RgbImage;
use crate::vision::Mask;
use crate::vision::ncc::best_match;

/// RGB of a row's text cell.
const CELL_FILL: [i32; 3] = [205, 217, 227];
const FILL_TOL: i32 = 12;
/// Share of a pixel row that is cell fill for it to belong to a text line.
const LINE_MIN_FILL: f32 = 0.15;
const LINE_MIN_H: usize = 15;
const LINES_PER_ROW: usize = 3;
/// A text line with its descenders.
const LINE_H: i32 = 31;
/// Capitals reach above the cell fill that the line detection samples.
const LINE_TOP: i32 = 3;
/// Inside the list box: the text cell, without the icon cell.
const FILL_X: (usize, usize) = (67, 302);
/// The first glyph starts left of where the cell fill is sampled.
const INK_X: usize = 55;
/// Measured: the totals row 0.84-1.0 at UI scales 0.64-1.28 (read resampled), sale rows at most 0.34.
const TOTAL_MATCH: f32 = 0.7;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub lines: Vec<Line>,
    /// The closing "Total Sales / Total Received" row, not a sale.
    pub is_total: bool,
}

pub fn same(a: &Row, b: &Row) -> bool {
    a.lines.len() == b.lines.len() && a.lines.iter().zip(&b.lines).all(|(x, y)| lines::same(x, y))
}

/// Top of each text line in the list box (sampled across the text cell).
fn line_starts(list: &RgbImage) -> Vec<usize> {
    let (x0, x1) = (FILL_X.0.min(list.width), FILL_X.1.min(list.width));
    if x1 <= x0 {
        return Vec::new();
    }
    let filled = (0..list.height).map(|y| {
        let fill = (x0..x1)
            .filter(|&x| {
                let p = list.pixel(x, y);
                (0..3).map(|c| (p[c] as i32 - CELL_FILL[c]).abs()).sum::<i32>() < FILL_TOL
            })
            .count();
        fill as f32 / (x1 - x0) as f32 > LINE_MIN_FILL
    });
    let mut starts = Vec::new();
    let mut run = None;
    for (y, on) in filled.chain([false]).enumerate() {
        match (on, run) {
            (true, None) => run = Some(y),
            (false, Some(first)) => {
                if y - first >= LINE_MIN_H {
                    starts.push(first);
                }
                run = None;
            }
            _ => {}
        }
    }
    starts
}

/// A pixel of the white line between two text lines.
const SEPARATOR_MIN: u8 = 240;
const SEPARATOR_SHARE: f32 = 0.5;

/// Top of each text line, the lines told apart by the white rows between them rather than by fill: "Default Ratio
/// (Filter Applied)" sharpens a line of text into halos that leave a row of it with too little plain fill, and the
/// line fell apart into pieces.
fn line_starts_by_separators(list: &RgbImage) -> Vec<usize> {
    let (x0, x1) = (FILL_X.0.min(list.width), FILL_X.1.min(list.width));
    if x1 <= x0 {
        return Vec::new();
    }
    let share = |y: usize, test: &dyn Fn([u8; 3]) -> bool| {
        (x0..x1).filter(|&x| test(list.pixel(x, y))).count() as f32 / (x1 - x0) as f32
    };
    let separator = |y: usize| share(y, &|p| p[0].min(p[1]).min(p[2]) > SEPARATOR_MIN) >= SEPARATOR_SHARE;
    let filled = |y: usize| {
        share(y, &|p| (0..3).map(|c| (p[c] as i32 - CELL_FILL[c]).abs()).sum::<i32>() < FILL_TOL) > LINE_MIN_FILL
    };
    let mut starts = Vec::new();
    let mut span: Option<(usize, usize)> = None; // first and last filled row since the last separator
    for y in 0..=list.height {
        if y == list.height || separator(y) {
            if let Some((first, last)) = span.take()
                && last + 1 - first >= LINE_MIN_H
            {
                starts.push(first);
            }
            continue;
        }
        if filled(y) {
            span = Some(span.map_or((y, y), |(first, _)| (first, y)));
        }
    }
    starts
}

/// Rows are this far apart in the list and lines this far apart in a row (reference px, measured: 9, 37, 66 | 100,
/// 128, 156 | 190, 219, 247 | 281, 309, 337).
const ROW_PITCH: f32 = 90.67;
const LINE_PITCH: f32 = 28.33;
/// A line found this close to where the grid puts one is that line.
const GRID_SLACK: f32 = 3.0;

/// The line tops of whole rows, from two finders that each missed a line or two (a row's faint first line, the totals
/// row's): the rows are on a fixed grid, so the grid's phase is fitted to every top either found, and a row counts once
/// two of its three lines were seen (one alone could be a stray edge).
fn on_grid(a: &[usize], b: &[usize], height: usize) -> Vec<usize> {
    let mut found: Vec<f32> = a.iter().chain(b).map(|&s| s as f32).collect();
    found.sort_by(f32::total_cmp);
    found.dedup_by(|x, y| (*x - *y).abs() <= GRID_SLACK);
    let near = |at: f32| found.iter().copied().find(|s| (s - at).abs() <= GRID_SLACK);
    let rows = |phase: f32| (0..).map(move |r| phase + r as f32 * ROW_PITCH).take_while(|&top| top < height as f32);
    let lines = |top: f32| (0..LINES_PER_ROW).map(move |l| top + l as f32 * LINE_PITCH);
    let phases =
        found.iter().flat_map(|&s| (0..LINES_PER_ROW).map(move |l| (s - l as f32 * LINE_PITCH).rem_euclid(ROW_PITCH)));
    let score = |phase: f32| rows(phase).flat_map(lines).filter(|&at| near(at).is_some()).count();
    let Some(phase) = phases.max_by_key(|&p| score(p)) else { return Vec::new() };
    let mut starts = Vec::new();
    for top in rows(phase) {
        let seen: Vec<Option<f32>> = lines(top).map(near).collect();
        if seen.iter().flatten().count() >= 2 && top + 2.0 * LINE_PITCH + LINE_MIN_H as f32 <= height as f32 {
            starts.extend(lines(top).zip(&seen).map(|(at, s)| s.unwrap_or(at).round() as usize));
        }
    }
    starts
}

/// True when a Sold Items list (even an empty one: its slots still show their cell lines) hangs below `anchor`.
pub fn has_list(view: &View<'_>, anchor: Point) -> bool {
    let (x, y, w, h) = sold_list(anchor);
    if x < 0 || y < 0 {
        return false;
    }
    let list = view.crop((x, y, w, h));
    list.height == h as usize && line_starts(&list).len() >= LINES_PER_ROW
}

/// Non-empty rows top to bottom; None when the dialog is mid-redraw or something covers part of it.
pub fn read_rows(view: &View<'_>, anchor: Point) -> Option<Vec<Row>> {
    let (x, y, w, h) = sold_list(anchor);
    let list = view.crop((x, y, w, h));
    let mut starts = line_starts(&list);
    if !starts.len().is_multiple_of(LINES_PER_ROW) {
        let by_separators = line_starts_by_separators(&list);
        starts = if by_separators.len().is_multiple_of(LINES_PER_ROW) {
            by_separators
        } else {
            on_grid(&starts, &by_separators, list.height)
        };
    }
    if !starts.len().is_multiple_of(LINES_PER_ROW) {
        return None;
    }
    let text = list.crop(INK_X, 0, FILL_X.1, list.height);
    let gray = text.to_gray();
    let neutral = Mask {
        width: text.width,
        height: text.height,
        bits: text.data.as_chunks::<3>().0.iter().map(|p| is_grey([p[0], p[1], p[2]])).collect(),
    };
    let label_area = list.crop(INK_X, 0, list.width, list.height).to_gray();
    let mut rows = Vec::new();
    for group in starts.chunks(LINES_PER_ROW) {
        let window = |s: usize| {
            let top = (s as i32 - LINE_TOP).max(0) as usize;
            let bottom = (s as i32 - LINE_TOP + LINE_H).max(0) as usize;
            (top, bottom)
        };
        let lines: Vec<Line> = group
            .iter()
            .map(|&s| {
                let (top, bottom) = window(s);
                let mut line = make_line(
                    &gray.crop(0, top, gray.width, bottom),
                    Some(&neutral.crop(0, top, neutral.width, bottom)),
                );
                let rows = bottom.min(gray.height).saturating_sub(top);
                line.source = Some(((x + INK_X as i32, y + top as i32, gray.width as i32, rows as i32), true));
                line
            })
            .collect();
        if lines.iter().map(Line::ink).sum::<f32>() < MIN_INK {
            continue;
        }
        let (top, _) = window(group[0]);
        let label = label_area.crop(0, top, label_area.width, top + LINE_H as usize + 2);
        let is_total = best_match(&label, total_label()).is_some_and(|(score, _, _)| score >= TOTAL_MATCH);
        rows.push(Row { lines, is_total });
    }
    Some(rows)
}
