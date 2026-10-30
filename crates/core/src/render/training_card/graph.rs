//! The strip's background graph: EXP a minute over the last ten minutes. Like meso.watch's price trends (a thin line
//! over an area fading to nothing), but each point is the minute before it, so kills read as a pace and not a comb;
//! the time axis is fixed, so a young session fills only the right of it; the session's average is a dashed line to
//! read the pace against; level ups are marked where they happened, and the newest point glows. It sits behind the
//! numbers, faint enough that they read first.

use crate::render::canvas::Canvas;
use crate::render::theme::{self, Rgba};
use crate::training::session::{ExpGraph, GRAPH_S, GRAPH_STEP_S};

/// Fewer points than this is no trend yet.
const MIN_POINTS: usize = 3;
/// Headroom over the highest point, so the line never runs along the top.
const HEADROOM: f64 = 1.2;
const LINE: Rgba = theme::with_alpha(theme::GOLD, 130);
const AREA_TOP: Rgba = theme::with_alpha(theme::GOLD, 58);
const AREA_BOTTOM: Rgba = theme::with_alpha(theme::GOLD, 0);
const AVERAGE: Rgba = theme::with_alpha(theme::TEXT_FAINT, 120);
const LEVEL_UP: Rgba = theme::with_alpha(theme::BULLET, 110);
const DASH: f32 = 3.0;

/// The rate at the graph's top edge.
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

pub(super) fn draw(cv: &mut Canvas<'_>, area: (f32, f32, f32, f32), graph: &ExpGraph, average: Option<f64>) {
    let top = top(graph, average);
    if graph.rates.len() < MIN_POINTS || top <= 0.0 {
        return;
    }
    let (x, y, w, h) = area;
    let line = points(graph, area, top);
    let Some((&first, &newest)) = line.first().zip(line.last()) else { return };
    let mut shape = line.clone();
    shape.extend([(newest.0, y + h), (first.0, y + h)]);
    cv.gradient_polygon(&shape, (y, y + h), &[(0.0, AREA_TOP), (1.0, AREA_BOTTOM)]);
    if let Some(average) = average.filter(|a| *a > 0.0) {
        let ay = y + h - h * (average / top) as f32;
        let mut dx = first.0;
        while dx < x + w {
            cv.rect(dx, ay, DASH.min(x + w - dx), 0.75, AVERAGE);
            dx += 2.0 * DASH;
        }
    }
    for &age in &graph.level_ups {
        let lx = x + w * (1.0 - (age / GRAPH_S) as f32);
        if lx >= first.0 {
            cv.rect(lx - 0.5, y, 1.0, h, LEVEL_UP);
            cv.polygon(&[(lx - 3.0, y), (lx + 3.0, y), (lx, y + 4.0)], theme::BULLET);
        }
    }
    cv.polyline(&line, LINE, 1.25);
    cv.circle(newest.0 - 1.0, newest.1, 3.5, theme::with_alpha(theme::GOLD, 60), None);
    cv.circle(newest.0 - 1.0, newest.1, 1.75, theme::GOLD, None);
}
