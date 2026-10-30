//! The strip's graph, behind its numbers: EXP a minute over the last ten minutes, a thin gold line over a glow fading
//! down (like meso.watch's price trends). Each point is the minute before it, so kills read as a pace and not a comb;
//! the time axis is fixed, so a young session fills only the right of it; the session's average is a dashed line to
//! read the pace against; level ups are marked where they happened, and the newest point glows.

use crate::render::canvas::Canvas;
use crate::render::theme::{self, Rgba};
use crate::training::session::{ExpGraph, GRAPH_S, GRAPH_STEP_S};

/// Fewer points than this is no trend yet.
const MIN_POINTS: usize = 3;
/// Headroom over the highest point, so the line never runs along the top.
const HEADROOM: f64 = 1.2;
const LINE: Rgba = theme::with_alpha(theme::GOLD, 150);
const GLOW_TOP: Rgba = theme::with_alpha(theme::GOLD, 64);
const GLOW_BOTTOM: Rgba = theme::with_alpha(theme::GOLD, 0);
const AVERAGE: Rgba = theme::with_alpha(theme::TEXT_FAINT, 110);
const LEVEL_UP: Rgba = theme::with_alpha(theme::BULLET, 110);
const DASH: f32 = 3.0;

/// The x of a moment `age` seconds ago in `(x, w)`: now at the right edge, `GRAPH_S` ago at the left.
fn x_of(age: f64, x: f32, w: f32) -> f32 {
    x + w * (1.0 - (age / GRAPH_S) as f32)
}

pub(super) fn draw(cv: &mut Canvas<'_>, (x, y, w, h): (f32, f32, f32, f32), graph: &ExpGraph, average: Option<f64>) {
    let top = graph.rates.iter().copied().chain(average).fold(0.0, f64::max) * HEADROOM;
    if graph.rates.len() < MIN_POINTS || top <= 0.0 {
        return;
    }
    let level = |rate: f64| y + h - h * (rate / top) as f32;
    let last = graph.rates.len() - 1;
    let line: Vec<(f32, f32)> = (graph.rates.iter().enumerate())
        .map(|(i, &rate)| (x_of((last - i) as f64 * GRAPH_STEP_S, x, w), level(rate)))
        .collect();
    let (first, newest) = (line[0], line[last]);
    let mut glow = line.clone();
    glow.extend([(newest.0, y + h), (first.0, y + h)]);
    cv.gradient_polygon(&glow, (y, y + h), &[(0.0, GLOW_TOP), (1.0, GLOW_BOTTOM)]);
    if let Some(ay) = average.filter(|a| *a > 0.0).map(level) {
        let mut dx = first.0;
        while dx < x + w {
            cv.rect(dx, ay, DASH.min(x + w - dx), 0.75, AVERAGE);
            dx += 2.0 * DASH;
        }
    }
    for lx in graph.level_ups.iter().map(|&age| x_of(age, x, w)).filter(|&lx| lx >= first.0) {
        cv.rect(lx - 0.5, y, 1.0, h, LEVEL_UP);
        cv.polygon(&[(lx - 3.0, y), (lx + 3.0, y), (lx, y + 4.0)], theme::BULLET);
    }
    cv.polyline(&line, LINE, 1.25);
    cv.circle(newest.0 - 1.0, newest.1, 3.5, theme::with_alpha(theme::GOLD, 60), None);
    cv.circle(newest.0 - 1.0, newest.1, 1.75, theme::GOLD, None);
}
