//! Find item tooltips anywhere in a frame of the game (pure). Used where the mouse position is unknown (a
//! browser capturing the game window); with a mouse, the tooltip is simply at mouse + offset.
//!
//! Every tooltip has a band of plain fill between its title and its description (see `tooltip`). The fill is
//! translucent, so over a bright backdrop only part of it keeps the fill colour; a coarse scan for rows where
//! at least half of a 150 px stretch is fill gives candidates. The tooltip's bright left and top border lines
//! then pin down its corner, and the usual presence and width checks confirm it.

use crate::config;
use crate::geometry::{Point, Rect};
use crate::image::RgbImage;
use crate::vision::tooltip::{is_fill, measure_width, right_edge_unreadable, tooltip_present};

/// Scan every this many rows and columns: the band is 8 rows tall and 150+ px wide, so nothing is missed.
const SCAN_ROW_STEP: usize = 4;
const SCAN_COL_STEP: usize = 2;
/// A 150 px stretch (at the reference resolution) with at least half of it fill makes a band, as in
/// `tooltip_present`.
const BAND_MIN_FILL: f32 = 0.5;
/// The band covers tooltip columns 150..300; the tooltip's left edge is at most this far left of a band.
const BAND_LEFT: i32 = 300;
/// The band starts this far below the tooltip's top (rows 34..42, see `tooltip`).
const BAND_TOP: i32 = 34;
const BAND_BOTTOM: i32 = 42;
/// The border line is the brightest column/row, one pixel in from the tooltip's corner.
const BORDER_INSET: i32 = 1;
/// Compare a border pixel with the fill this far inside the tooltip.
const INSIDE_PX: i32 = 5;
/// A border pixel is at least this much brighter (darkest channel) than the fill inside it.
const LIFT_MIN: i16 = 30;
/// Border and fill both darker than this (darkest channel) cannot be told apart.
const UNREADABLE: i16 = 60;
/// A border line needs this share of its length readable to count at all.
const MIN_READABLE: f32 = 0.45;
/// Share of the border's length that must stand out.
const MIN_BORDER_SCORE: f32 = 0.5;
/// Band rows this close (two scan steps) with overlapping spans belong to one cluster (one tooltip, mostly).
const CLUSTER_GAP: i32 = 2 * SCAN_ROW_STEP as i32;
/// Two finds this close are the same tooltip (the first, from the strongest border, is kept).
const SAME_SPOT_PX: i32 = 8;
/// How far around a remembered spot `tooltip_at` looks (reference px), for a tooltip that moved a pixel or two.
const SPOT_MARGIN: i32 = 6;
/// Extra width (reference px) a band may have beyond a tooltip and its compare tooltip, the band windows' own width.
const MAX_SPAN_MARGIN: i32 = 160;
/// Share of a border line's length that must be lifted for the spot to be a tooltip.
const FRAME_LINE_SCORE: f32 = 0.8;
/// How much of the band right of the border must be fill.
const BORDER_FILL_PX: usize = 45;
/// How far a first guess at a border line can be off.
const SNAP_PX: i32 = 6;
/// Rows around the band that the left border is checked over.
const BORDER_ROWS: i32 = 24;
/// Unlifted rows that end a left border followed upwards (the resize can blur a pixel or two of it away).
const CLIMB_GAP: i32 = 4;
/// A left border running on at least this far above a found top: the top was a line inside the tooltip. (The job
/// row's box is ~150 rows below the top; a top border drawn two pixels thick is not a climb.)
const MIN_CLIMB: i32 = 20;

/// The lift of every pixel over its neighbour `INSIDE_PX` away, summed along rows and columns ahead of time: a
/// border's lift along any stretch is then two lookups, not a walk over the pixels. Every band row of a cluster asks
/// about its own overlapping stretches of the same few hundred columns and rows, which made this the cost of a frame
/// (two thirds of 50 ms natively, three times that in WebAssembly).
///
/// (Counts are u16: a region is a few hundred rows and a couple of thousand columns.)
///
/// A pair "lifts" when the first pixel is brighter than the second by LIFT_MIN (darkest channel). Over a dark
/// backdrop the translucent border is as dark as the fill beside it (both under `UNREADABLE`): those pairs are not
/// `readable` and say nothing either way.
struct Lifts {
    w: usize,
    h: usize,
    /// Running sums down each column, `(h + 1) * w`: pairs (x, y) - (x + INSIDE_PX, y).
    col_lifted: Vec<u16>,
    col_lifted_all: Vec<u16>,
    col_readable: Vec<u16>,
    /// Running sums along each row, `h * (w + 1)`: pairs (x, y) - (x, y + INSIDE_PX).
    row_lifted: Vec<u16>,
    row_lifted_all: Vec<u16>,
    row_readable: Vec<u16>,
}

/// What a stretch of pairs holds.
#[derive(Clone, Copy, Default)]
struct Counts {
    /// Lifted among the readable pairs.
    lifted: usize,
    /// Lifted among all of them, a dark border over a darker fill included.
    lifted_all: usize,
    readable: usize,
    total: usize,
}

impl Counts {
    /// Share of all the pairs that lift (unreadable ones count against). This places a border exactly.
    fn strict(self) -> f32 {
        if self.total == 0 { 0.0 } else { self.lifted_all as f32 / self.total as f32 }
    }

    /// Share of the readable pairs that lift; None when fewer than `MIN_READABLE` of the pairs can tell. This decides
    /// whether a placed border is a tooltip's.
    fn readable_share(self) -> Option<f32> {
        (self.total > 0 && self.readable as f32 >= MIN_READABLE * self.total as f32)
            .then(|| self.lifted as f32 / self.readable as f32)
    }
}

impl Lifts {
    fn new(frame: &RgbImage) -> Self {
        let (w, h) = (frame.width, frame.height);
        let darkest: Vec<i16> = frame.data.as_chunks::<3>().0.iter().map(|p| p[0].min(p[1]).min(p[2]) as i16).collect();
        let tell = |edge: i16, inside: i16| (edge.max(inside) >= UNREADABLE, edge - inside >= LIFT_MIN);
        let inside = INSIDE_PX as usize;
        let mut col_lifted = vec![0u16; (h + 1) * w];
        let mut col_lifted_all = vec![0u16; (h + 1) * w];
        let mut col_readable = vec![0u16; (h + 1) * w];
        let mut row_lifted = vec![0u16; h * (w + 1)];
        let mut row_lifted_all = vec![0u16; h * (w + 1)];
        let mut row_readable = vec![0u16; h * (w + 1)];
        for y in 0..h {
            for x in 0..w {
                let (mut lifted, mut all, mut readable) = (0, 0, 0);
                if x + inside < w {
                    let (r, l) = tell(darkest[y * w + x], darkest[y * w + x + inside]);
                    (readable, lifted, all) = (r as u16, (r && l) as u16, l as u16);
                }
                col_readable[(y + 1) * w + x] = col_readable[y * w + x] + readable;
                col_lifted[(y + 1) * w + x] = col_lifted[y * w + x] + lifted;
                col_lifted_all[(y + 1) * w + x] = col_lifted_all[y * w + x] + all;
                let (mut lifted, mut all, mut readable) = (0, 0, 0);
                if y + inside < h {
                    let (r, l) = tell(darkest[y * w + x], darkest[(y + inside) * w + x]);
                    (readable, lifted, all) = (r as u16, (r && l) as u16, l as u16);
                }
                row_readable[y * (w + 1) + x + 1] = row_readable[y * (w + 1) + x] + readable;
                row_lifted[y * (w + 1) + x + 1] = row_lifted[y * (w + 1) + x] + lifted;
                row_lifted_all[y * (w + 1) + x + 1] = row_lifted_all[y * (w + 1) + x] + all;
            }
        }
        Self { w, h, col_lifted, col_lifted_all, col_readable, row_lifted, row_lifted_all, row_readable }
    }

    /// The pairs (x, y) - (x + INSIDE_PX, y) for `rows`: a vertical border line at column `x`.
    fn side(&self, x: i32, rows: std::ops::Range<i32>) -> Counts {
        if x < 0 || x as usize + INSIDE_PX as usize >= self.w {
            return Counts::default();
        }
        let (y0, y1) = (rows.start.max(0) as usize, (rows.end.max(0) as usize).min(self.h));
        if y0 >= y1 {
            return Counts::default();
        }
        let at = |table: &[u16], y: usize| table[y * self.w + x as usize] as usize;
        Counts {
            lifted: at(&self.col_lifted, y1) - at(&self.col_lifted, y0),
            lifted_all: at(&self.col_lifted_all, y1) - at(&self.col_lifted_all, y0),
            readable: at(&self.col_readable, y1) - at(&self.col_readable, y0),
            total: y1 - y0,
        }
    }

    /// The pairs (x, y) - (x, y + INSIDE_PX) for `cols`: a horizontal border line at row `y`.
    fn top(&self, y: i32, cols: std::ops::Range<i32>) -> Counts {
        if y < 0 || y as usize + INSIDE_PX as usize >= self.h {
            return Counts::default();
        }
        let (x0, x1) = (cols.start.max(0) as usize, (cols.end.max(0) as usize).min(self.w));
        if x0 >= x1 {
            return Counts::default();
        }
        let at = |table: &[u16], x: usize| table[y as usize * (self.w + 1) + x] as usize;
        Counts {
            lifted: at(&self.row_lifted, x1) - at(&self.row_lifted, x0),
            lifted_all: at(&self.row_lifted_all, x1) - at(&self.row_lifted_all, x0),
            readable: at(&self.row_readable, x1) - at(&self.row_readable, x0),
            total: x1 - x0,
        }
    }
}

/// Rows with a band-like stretch of fill (`band_samples` samples, at least half of them fill), and the x range
/// the stretches cover.
fn bands(frame: &RgbImage, band_samples: usize) -> Vec<(i32, i32, i32)> {
    let mut found = Vec::new();
    for y in (0..frame.height).step_by(SCAN_ROW_STEP) {
        let fill: Vec<bool> = (0..frame.width)
            .step_by(SCAN_COL_STEP)
            .map(|x| {
                let [r, g, b] = frame.pixel(x, y);
                is_fill(r, g, b)
            })
            .collect();
        if fill.len() < band_samples {
            continue;
        }
        let need = (band_samples as f32 * BAND_MIN_FILL).ceil() as usize;
        let mut count = fill[..band_samples].iter().filter(|&&f| f).count();
        let mut span: Option<(usize, usize)> = None;
        for start in 0..=fill.len() - band_samples {
            if start > 0 {
                count = count + fill[start + band_samples - 1] as usize - fill[start - 1] as usize;
            }
            if count >= need {
                span = Some(span.map_or((start, start + band_samples), |(lo, _)| (lo, start + band_samples)));
            } else if let Some((lo, hi)) = span.take() {
                found.push((y as i32, (lo * SCAN_COL_STEP) as i32, (hi * SCAN_COL_STEP) as i32));
            }
        }
        if let Some((lo, hi)) = span {
            found.push((y as i32, (lo * SCAN_COL_STEP) as i32, (hi * SCAN_COL_STEP) as i32));
        }
    }
    found
}

/// Band rows grouped into clusters of nearby rows with overlapping spans, top to bottom.
fn clusters(bands: Vec<(i32, i32, i32)>) -> Vec<Vec<(i32, i32, i32)>> {
    let mut clusters: Vec<Vec<(i32, i32, i32)>> = Vec::new();
    for band in bands {
        let home = clusters.iter_mut().find(|c| {
            let last = c[c.len() - 1];
            band.0 - last.0 <= CLUSTER_GAP && band.1 < last.2 && band.2 > last.1
        });
        match home {
            Some(cluster) => cluster.push(band),
            None => clusters.push(vec![band]),
        }
    }
    clusters
}

/// The tooltip's left and top border lines run unbroken along its side and top: text and icons inside a
/// tooltip, or window edges near it, make only short lifted stretches. A line over a dark backdrop cannot be read
/// (see `readable_lift`) and is not held against it, as long as the other one is plainly there.
fn has_frame(lifts: &Lifts, spot: Point) -> bool {
    let (x, y) = (spot.x, spot.y);
    let side = |bx: i32| lifts.side(bx, y + 14..y + 118).readable_share();
    let top = |by: i32| lifts.top(by, x + 20..x + 300).readable_share();
    // Some(true): drawn; Some(false): read and absent; None: unreadable.
    let line = |score: &dyn Fn(i32) -> Option<f32>, at: i32| {
        let scores: Vec<Option<f32>> = (at..=at + 2).map(score).collect();
        if scores.iter().flatten().any(|&v| v >= FRAME_LINE_SCORE) {
            Some(true)
        } else if scores.iter().any(Option::is_some) {
            Some(false)
        } else {
            None
        }
    };
    matches!((line(&side, x), line(&top, y)), (Some(true), Some(true) | None) | (None, Some(true)))
}

/// The fill band starts right at the left border (a busy backdrop next to a tooltip has edges that look like
/// a border, but no fill right of them).
fn band_reaches_border(image: &RgbImage) -> bool {
    let band = image.crop(INSIDE_PX as usize, BAND_TOP as usize, BORDER_FILL_PX, BAND_BOTTOM as usize);
    let fill = band.data.as_chunks::<3>().0.iter().filter(|p| is_fill(p[0], p[1], p[2])).count();
    !band.is_empty() && fill as f32 / (band.width * band.height) as f32 >= BAND_MIN_FILL
}

/// Top-left corners of the tooltips in `frame` (whose UI is drawn at `scale`, see `geometry::ui_scale`), in
/// `frame` pixels, left to right: the hovered tooltip comes before its compare tooltip.
///
/// Bands are found on the frame as it is; only the small region around each band is brought to the reference
/// resolution, so a 1440p frame costs about what a 1200p one does.
pub fn find_tooltips(frame: &RgbImage, scale: f32) -> Vec<Point> {
    let px = |v: i32| (v as f32 * scale).round() as i32;
    let band_samples = ((150.0 * scale) as usize / SCAN_COL_STEP).max(1);
    let mut spots: Vec<Point> = Vec::new();
    for cluster in clusters(bands(frame, band_samples)) {
        // Everything the checks below look at, for every row of the cluster: from the leftmost possible border to
        // a wide tooltip's right edge, from above the highest top border to below the lowest capture.
        let lo = cluster.iter().map(|b| b.1).min().unwrap_or(0);
        let hi = cluster.iter().map(|b| b.2).max().unwrap_or(0);
        let (top, bottom) = (cluster[0].0, cluster[cluster.len() - 1].0);
        let area = Rect::new((lo - px(BAND_LEFT + 10)).max(0), (top - px(BAND_BOTTOM + SNAP_PX + 12)).max(0), 0, 0);
        let area = Rect { w: hi + px(config::TOOLTIP_WIDTHS[1] + 30) - area.x, h: bottom + px(100) - area.y, ..area };
        let mut region: Option<(RgbImage, Lifts)> = None; // brought to the reference resolution only if a row needs it
        for &(band_y, lo, hi) in &cluster {
            // A band wider than two tooltips side by side is a window or a bar of the UI, not a tooltip's.
            if hi - lo > px(config::TOOLTIP_COMPARE_OFFSET_X + config::TOOLTIP_WIDTHS[1] + MAX_SPAN_MARGIN) {
                continue;
            }
            // Fill rows further down an already found tooltip (between its description lines) are not new ones.
            let inside_found = spots.iter().any(|s| {
                band_y >= s.y
                    && band_y < s.y + px(config::TOOLTIP_MAX_HEIGHT)
                    && lo >= s.x
                    && hi <= s.x + px(config::TOOLTIP_WIDTHS[1])
            });
            if inside_found {
                continue;
            }
            let (region, lifts) = region.get_or_insert_with(|| {
                let native =
                    frame.crop(area.x as usize, area.y as usize, area.right() as usize, area.bottom() as usize);
                let image = if (scale - 1.0).abs() > 0.01 { native.resized(1.0 / scale) } else { native };
                let lifts = Lifts::new(&image);
                (image, lifts)
            });
            let to_region = |v: i32, origin: i32| ((v - origin) as f32 / scale).round() as i32;
            let known: Vec<Point> =
                spots.iter().map(|s| Point { x: to_region(s.x, area.x), y: to_region(s.y, area.y) }).collect();
            let band = (to_region(band_y, area.y), to_region(lo, area.x), to_region(hi, area.x));
            let spot = spot_at_band(region, lifts, band, &known, false)
                .or_else(|| spot_at_band(region, lifts, band, &known, true));
            if let Some(spot) = spot {
                spots.push(Point { x: area.x + px(spot.x), y: area.y + px(spot.y) });
            }
        }
    }
    spots.sort_by_key(|s| (s.x, s.y));
    spots
}

/// Is there still a tooltip with its top-left corner at `spot` (a point of `frame`, whose UI is drawn at `scale`)?
/// The same checks a found spot passes, on the few hundred pixels around it.
pub fn tooltip_at(frame: &RgbImage, scale: f32, spot: Point) -> bool {
    let px = |v: i32| (v as f32 * scale).round() as i32;
    let margin = px(SPOT_MARGIN);
    let (w, h) = config::TOOLTIP_CAPTURE_SIZE;
    let x0 = (spot.x - margin).max(0);
    let y0 = (spot.y - margin).max(0);
    let x1 = (spot.x + px(w + SPOT_MARGIN)).min(frame.width as i32);
    let y1 = (spot.y + px(h + SPOT_MARGIN)).min(frame.height as i32);
    if x1 - x0 <= px(w / 2) || y1 - y0 <= px(h / 2) {
        return false;
    }
    let native = frame.crop(x0 as usize, y0 as usize, x1 as usize, y1 as usize);
    let region = if (scale - 1.0).abs() > 0.01 { native.resized(1.0 / scale) } else { native };
    let local =
        Point { x: ((spot.x - x0) as f32 / scale).round() as i32, y: ((spot.y - y0) as f32 / scale).round() as i32 };
    if local.x + w > region.width as i32 || local.y + h > region.height as i32 {
        return false;
    }
    let lifts = Lifts::new(&region);
    let image = region.crop(local.x as usize, local.y as usize, (local.x + w) as usize, (local.y + h) as usize);
    has_frame(&lifts, local)
        && band_reaches_border(&image)
        && tooltip_present(&image)
        && (measure_width(&image, &config::TOOLTIP_WIDTHS).is_some()
            || right_edge_unreadable(&image, &config::TOOLTIP_WIDTHS))
}

/// The tooltip a fill band at row `band_y` (columns `lo..hi`) belongs to, in a frame at the reference
/// resolution; None if the band is not a tooltip's or its tooltip is one of `known`.
fn spot_at_band(
    frame: &RgbImage,
    lifts: &Lifts,
    (band_y, lo, hi): (i32, i32, i32),
    known: &[Point],
    relaxed: bool,
) -> Option<Point> {
    // Relaxed: over a dark backdrop only the readable pixels of a border line count (see `Lifts`).
    let score = |counts: Counts| if relaxed { counts.readable_share().unwrap_or(0.0) } else { counts.strict() };
    let (w, h) = config::TOOLTIP_CAPTURE_SIZE;
    let rows = (band_y - BORDER_ROWS)..(band_y + BORDER_ROWS);
    let scores: Vec<(f32, i32)> =
        ((lo - BAND_LEFT).max(0)..hi).map(|x| (score(lifts.side(x, rows.clone())), x)).collect();
    // Border columns, strongest first: the real border is lifted along its whole length, a stray edge only in
    // places. A bright backdrop left of the tooltip lifts too, so of each run of lifted columns the rightmost
    // (the one next to the fill) is the border.
    let mut borders: Vec<(f32, i32)> = Vec::new();
    for (i, &(score, x)) in scores.iter().enumerate() {
        let next_lifted = scores.get(i + 1).is_some_and(|n| n.0 >= MIN_BORDER_SCORE);
        if score >= MIN_BORDER_SCORE && !next_lifted {
            let run_best =
                scores[..=i].iter().rev().take_while(|c| c.0 >= MIN_BORDER_SCORE).map(|c| c.0).fold(0.0, f32::max);
            borders.push((run_best, x));
        }
    }
    borders.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, border_x) in borders {
        let cols = (border_x + 20)..(border_x + 300);
        let lifted_row = |y: i32| score(lifts.top(y, cols.clone())) >= MIN_BORDER_SCORE;
        // The top border row sits BAND_TOP..BAND_BOTTOM rows above the band row (inset by one); as with the left
        // border, the lowest lifted row is the one next to the fill.
        // Over a dark backdrop the border row is only partly lifted, so that test can stop a few rows early: a row
        // whose readable stretches are all lifted is the border, and the lowest such row beats the guess.
        let drawn_row = |y: i32| {
            lifts.top(y, (border_x + 20)..(border_x + 300)).readable_share().is_some_and(|s| s >= FRAME_LINE_SCORE)
        };
        let window = (band_y - BAND_BOTTOM + BORDER_INSET + 1)..=(band_y - BAND_TOP + BORDER_INSET);
        // The strict test places it exactly; the readable one only where the strict one finds no row at all.
        let Some(border_y) =
            window.clone().rev().find(|&y| lifted_row(y)).or_else(|| window.rev().find(|&y| drawn_row(y)))
        else {
            continue;
        };
        // Fill rows above the title also look like a band, which puts the window a few rows too high: snap to the
        // lowest lifted row (and rightmost lifted column) right around the guess.
        let border_y = (border_y..=border_y + SNAP_PX).rev().find(|&y| lifted_row(y)).unwrap_or(border_y);
        let border_y = (border_y..=border_y + SNAP_PX).rev().find(|&y| drawn_row(y)).unwrap_or(border_y);
        let rows = (border_y + 20)..(border_y + 100);
        let lifted_col = |x: i32| score(lifts.side(x, rows.clone())) >= MIN_BORDER_SCORE;
        let border_x = (border_x..=border_x + SNAP_PX).rev().find(|&x| lifted_col(x)).unwrap_or(border_x);
        let spot = Point { x: border_x - BORDER_INSET, y: border_y - BORDER_INSET };
        let is_known =
            |p: Point| known.iter().any(|s| (s.x - p.x).abs() <= SAME_SPOT_PX && (s.y - p.y).abs() <= SAME_SPOT_PX);
        if spot.x < 0 || spot.y < 0 || is_known(spot) {
            continue;
        }
        let image = frame.crop(spot.x as usize, spot.y as usize, (spot.x + w) as usize, (spot.y + h) as usize);
        if has_frame(lifts, spot)
            && band_reaches_border(&image)
            && tooltip_present(&image)
            && (measure_width(&image, &config::TOOLTIP_WIDTHS).is_some()
                || right_edge_unreadable(&image, &config::TOOLTIP_WIDTHS))
        {
            let spot = true_top(lifts, spot);
            return (!is_known(spot)).then_some(spot);
        }
    }
    None
}

/// A tooltip passes every check at a line inside it too: an equipment tooltip's job row is boxed, and below the box
/// is plain fill, so where the fill under the title lies over a bright window (the shop's, the inventory's) the band
/// is not fill and the box's line is taken for the top border. The left border then runs on above `spot`, unbroken,
/// up to the real top border: follow it there. A real top has no border line above it.
fn true_top(lifts: &Lifts, spot: Point) -> Point {
    let x = spot.x + BORDER_INSET;
    let start = spot.y + BORDER_INSET;
    // Over a dark backdrop the border cannot be told from the fill (see `Lifts`): such rows neither end the line
    // nor extend it. A few rows read and not lifted end it (the resize can blur a pixel or two away).
    let (mut top, mut gap, mut y) = (start, 0, start - 1);
    while y >= 0 && gap <= CLIMB_GAP {
        let counts = lifts.side(x, y..y + 1);
        if counts.lifted_all > 0 {
            (top, gap) = (y, 0);
        } else if counts.readable > 0 {
            gap += 1;
        }
        y -= 1;
    }
    // Scattered lifted pixels of a busy backdrop above a real top can chain up too; a border is lifted all along.
    let line = lifts.side(x, top..start).readable_share().is_some_and(|s| s >= FRAME_LINE_SCORE);
    if start - top < MIN_CLIMB || !line {
        return spot;
    }
    // The top border is where the line ends. Over a bright window the small UI's top line does not stand out from
    // the backdrop above it, so a lifted row right there only refines it; the left border, followed unbroken down to a
    // tooltip that passed every check, is the evidence.
    let cols = (x + 20)..(x + 300);
    let lifted_row = |y: i32| lifts.top(y, cols.clone()).strict() >= MIN_BORDER_SCORE;
    let border_y = (top - SNAP_PX / 2..=top + SNAP_PX / 2).rev().find(|&y| lifted_row(y)).unwrap_or(top - BORDER_INSET);
    Point { x: spot.x, y: border_y - BORDER_INSET }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The walk over the pixels that `Lifts` replaces, as the reference.
    fn naive(frame: &RgbImage, pairs: impl Iterator<Item = ((i32, i32), (i32, i32))>) -> Counts {
        let darkest = |x: i32, y: i32| -> Option<i16> {
            if x < 0 || y < 0 || x as usize >= frame.width || y as usize >= frame.height {
                return None;
            }
            let [r, g, b] = frame.pixel(x as usize, y as usize);
            Some(r.min(g).min(b) as i16)
        };
        let mut counts = Counts::default();
        for ((x, y), (ix, iy)) in pairs {
            if let (Some(edge), Some(inside)) = (darkest(x, y), darkest(ix, iy)) {
                counts.total += 1;
                counts.lifted_all += (edge - inside >= LIFT_MIN) as usize;
                if edge.max(inside) >= UNREADABLE {
                    counts.readable += 1;
                    counts.lifted += (edge - inside >= LIFT_MIN) as usize;
                }
            }
        }
        counts
    }

    #[test]
    fn prefix_sums_count_like_walking_the_pixels() {
        let (w, h) = (97usize, 61usize);
        let mut frame = RgbImage::new(w, h);
        let mut state = 7u32;
        for y in 0..h {
            for x in 0..w {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                let v = ((state >> 24) as u8 / 2).saturating_add(if x % 23 < 3 { 120 } else { 0 });
                frame.set_pixel(x, y, [v, v / 2, v.saturating_add(30)]);
            }
        }
        let lifts = Lifts::new(&frame);
        let inside = INSIDE_PX;
        for x in [-3, 0, 5, 40, 90, 91, 96, 120] {
            for rows in [(-5, 10), (0, 61), (10, 50), (55, 90), (30, 30)] {
                let side = lifts.side(x, rows.0..rows.1);
                let reference = naive(&frame, (rows.0..rows.1).map(|y| ((x, y), (x + inside, y))));
                let got = (side.total, side.readable, side.lifted, side.lifted_all);
                assert_eq!(
                    got,
                    (reference.total, reference.readable, reference.lifted, reference.lifted_all),
                    "side x {x} {rows:?}"
                );
            }
        }
        for y in [-3, 0, 5, 30, 55, 56, 60, 80] {
            for cols in [(-5, 20), (0, 97), (20, 80), (90, 130), (40, 40)] {
                let top = lifts.top(y, cols.0..cols.1);
                let reference = naive(&frame, (cols.0..cols.1).map(|x| ((x, y), (x, y + inside))));
                assert_eq!(
                    (top.total, top.readable, top.lifted),
                    (reference.total, reference.readable, reference.lifted),
                    "top y {y} {cols:?}"
                );
            }
        }
    }
}
