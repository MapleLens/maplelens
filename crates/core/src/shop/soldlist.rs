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
        starts = line_starts_by_separators(&list);
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
