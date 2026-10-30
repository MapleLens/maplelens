//! The strip's graph: EXP a minute over the last ten minutes, in a dark well of its own. Like meso.watch's price
//! trends (a thin line over an area fading to nothing), but each point is the minute before it, so kills read as a
//! pace and not a comb; the time axis is fixed, so a young session fills only the right of it; the session's average
//! is a dashed line to read the pace against; level ups are marked where they happened, and the newest point glows.
//! Given room, it says its scale (the best point) and its time axis.

use crate::market::format::format_mesos;
use crate::render::canvas::{Anchor, Canvas, Ink};
use crate::render::theme::{self, Rgba};
use crate::training::session::{ExpGraph, GRAPH_S, GRAPH_STEP_S};

/// Fewer points than this is no trend yet.
const MIN_POINTS: usize = 3;
/// Headroom over the highest point, so the line never runs along the top.
const HEADROOM: f64 = 1.25;
const WELL: Rgba = [14, 16, 44, 150];
const WELL_EDGE: Rgba = [150, 152, 190, 60];
const LINE: Rgba = theme::with_alpha(theme::GOLD, 235);
const AREA_TOP: Rgba = theme::with_alpha(theme::GOLD, 96);
const AREA_BOTTOM: Rgba = theme::with_alpha(theme::GOLD, 0);
const GRID: Rgba = [200, 204, 235, 34];
const AVERAGE: Rgba = theme::with_alpha(theme::TEXT_FAINT, 170);
const LEVEL_UP: Rgba = theme::with_alpha(theme::BULLET, 150);
const LABEL: f32 = 7.5;
const DASH: f32 = 3.0;
/// Inside the well, and the time axis's row under the plot.
const INSET: f32 = 3.0;
const AXIS_H: f32 = 10.0;

/// What the graph says besides its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Labels {
    None,
    /// The best point's value.
    Peak,
    /// That, the average's, and the time axis.
    Axes,
}

/// The rate at the plot's top edge.
fn top(graph: &ExpGraph, average: Option<f64>) -> f64 {
    graph.rates.iter().copied().chain(average).fold(0.0, f64::max) * HEADROOM
}

/// Where the points go in `(x, y, w, h)` under `top`: the newest at the right edge, one `GRAPH_S` across.
fn points(graph: &ExpGraph, (x, y, w, h): (f32, f32, f32, f32), top: f64) -> Vec<(f32, f32)> {
    let last = graph.rates.len().saturating_sub(1);
    graph
        .rates
        .iter()
        .enumerate()
        .map(|(i, &rate)| {
            let age = (last - i) as f64 * GRAPH_STEP_S;
            (x + w * (1.0 - (age / GRAPH_S) as f32), y + h - h * (rate / top) as f32)
        })
        .collect()
}

pub(super) fn draw(
    cv: &mut Canvas<'_>,
    well: (f32, f32, f32, f32),
    graph: &ExpGraph,
    average: Option<f64>,
    labels: Labels,
) {
    cv.rounded(well, 4.0, Some(WELL), Some(WELL_EDGE));
    let (wx, wy, ww, wh) = well;
    let axis = if labels == Labels::Axes { AXIS_H } else { 0.0 };
    let plot = (wx + INSET, wy + INSET, ww - 2.0 * INSET, wh - 2.0 * INSET - axis);
    let (x, y, w, h) = plot;
    let faint = Ink::new(LABEL, theme::TEXT_FAINT);
    if labels == Labels::Axes {
        let row = y + h + AXIS_H / 2.0 + 1.0;
        cv.text(x + 1.0, row, "10 min ago", faint, Anchor::LM);
        cv.text(x + w - 1.0, row, "now", faint, Anchor::RM);
    }
    let top = top(graph, average);
    if graph.rates.len() < MIN_POINTS || top <= 0.0 {
        let note = "Your pace shows here after 30 s";
        let note_w = cv.measure(note, LABEL, false);
        if note_w < w - 8.0 {
            cv.text(x + (w - note_w) / 2.0, y + h / 2.0, note, faint, Anchor::LM);
        }
        return;
    }
    let level = |rate: f64| y + h - h * (rate / top) as f32;
    let peak = graph.rates.iter().copied().fold(0.0, f64::max);
    if labels != Labels::None {
        cv.rect(x, level(peak), w, 0.75, GRID);
        cv.text(x + 2.0, level(peak) - 1.0, &format_mesos(Some(peak)), faint, Anchor::LS);
    }
    let line = points(graph, plot, top);
    let Some((&first, &newest)) = line.first().zip(line.last()) else { return };
    let mut shape = line.clone();
    shape.extend([(newest.0, y + h), (first.0, y + h)]);
    cv.gradient_polygon(&shape, (y, y + h), &[(0.0, AREA_TOP), (1.0, AREA_BOTTOM)]);
    if let Some(average) = average.filter(|a| *a > 0.0) {
        let ay = level(average);
        let mut dx = first.0;
        while dx < x + w {
            cv.rect(dx, ay, DASH.min(x + w - dx), 0.75, AVERAGE);
            dx += 2.0 * DASH;
        }
        if labels == Labels::Axes {
            cv.text(first.0 + 2.0, ay + 1.0, "avg", faint, Anchor::LA);
        }
    }
    for &age in &graph.level_ups {
        let lx = x + w * (1.0 - (age / GRAPH_S) as f32);
        if lx >= first.0 {
            cv.rect(lx - 0.5, y, 1.0, h, LEVEL_UP);
            cv.polygon(&[(lx - 3.0, y), (lx + 3.0, y), (lx, y + 4.0)], theme::BULLET);
        }
    }
    cv.polyline(&line, LINE, 1.3);
    cv.circle(newest.0 - 1.0, newest.1, 3.5, theme::with_alpha(theme::GOLD, 70), None);
    cv.circle(newest.0 - 1.0, newest.1, 1.75, theme::GOLD, None);
}
