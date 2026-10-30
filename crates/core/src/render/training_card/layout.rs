//! The strip's body between its header and its gauge, laid out for the room it has, over the graph drawn behind it
//! (`graph`). Narrow (4:3 screens): the rate on top, the session at the foot, the graph showing between. Medium (16:10
//! and wider): the rate and the session's total side by side, the graph across the middle. The text has a soft shadow
//! so it reads over the graph.

use super::graph;
use super::strip::{FOOT_Y, HEADER_Y, STRIP_HEIGHT};
use super::{clock, eta, fit};
use crate::market::format::format_mesos;
use crate::render::canvas::{Anchor, Canvas, Ink};
use crate::render::glance::PAD;
use crate::render::theme;
use crate::training::session::TrainingView;

/// From this width (design units) the medium layout.
const MEDIUM: f32 = 200.0;
/// The body starts under the header and ends over the gauge.
const TOP: f32 = HEADER_Y + 11.0;
const BOTTOM: f32 = STRIP_HEIGHT - 19.0;
const SMALL: f32 = 9.0;
/// Under every letter, so the numbers read over the graph.
const SHADOW: theme::Rgba = [6, 8, 28, 190];
const SHADOW_OFFSET: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Narrow,
    Medium,
}

impl Shape {
    pub(super) fn of(width: f32) -> Self {
        if width >= MEDIUM { Self::Medium } else { Self::Narrow }
    }
}

/// The lines the layouts arrange.
struct Lines {
    rate: String,
    average: String,
    total: String,
    time: String,
    pace: String,
    level: String,
}

impl Lines {
    fn of(view: &TrainingView) -> Self {
        let rate = view.last_minute.or(view.recent_per_min).map_or_else(|| "—".to_string(), |r| format_mesos(Some(r)));
        let average = view.average_per_min.map_or_else(|| "—".into(), |a| format_mesos(Some(a)));
        let best = format_mesos(Some(view.session.best_minute as f64));
        let session = &view.session;
        let mut pace = Vec::new();
        if let Some(per_hour) = view.percent_per_hour {
            pace.push(format!("{per_hour:.1}%/h"));
        }
        if session.levels > 0 {
            pace.push(format!("{} level{}", session.levels, if session.levels == 1 { "" } else { "s" }));
        }
        let level = match (view.reading, view.to_level_s) {
            (_, Some(s)) => format!("Level up {}", eta(s)),
            (Some(_), None) => "Level up —".into(),
            (None, None) => String::new(),
        };
        Self {
            rate,
            average: format!("avg {average} · best {best}"),
            total: format!("+{}", format_mesos(Some(session.gained as f64))),
            time: format!("in {}", clock(session.active_s)),
            pace: pace.join(" · "),
            level,
        }
    }
}

pub(super) fn body(cv: &mut Canvas<'_>, view: &TrainingView, shape: Shape) {
    // Behind everything: the whole body, from under the header down to the gauge.
    let area = (4.0, TOP - 3.0, cv.width - 8.0, FOOT_Y - 3.0 - TOP);
    graph::draw(cv, area, &view.graph, view.average_per_min);
    let lines = Lines::of(view);
    match shape {
        Shape::Narrow => narrow(cv, &lines),
        Shape::Medium => medium(cv, &lines),
    }
}

/// Text over the graph: its shadow first.
fn put(cv: &mut Canvas<'_>, x: f32, y: f32, text: &str, ink: Ink, anchor: Anchor) -> f32 {
    let shadow = Ink { colour: SHADOW, ..ink };
    cv.text(x + SHADOW_OFFSET, y + SHADOW_OFFSET, text, shadow, anchor);
    cv.text(x, y, text, ink, anchor)
}

/// The rate now, big, with "/min" after it.
fn rate(cv: &mut Canvas<'_>, x: f32, y: f32, size: f32, text: &str) {
    let width = put(cv, x, y, text, Ink::new(size, theme::GOLD).bold(), Anchor::LA);
    put(cv, x + width + 3.0, y + size * 0.42, "/min", Ink::new(SMALL, theme::TEXT_DIM), Anchor::LA);
}

/// "+108k in 1:02:05" ending at `right`.
fn total(cv: &mut Canvas<'_>, right: f32, y: f32, lines: &Lines, size: f32) {
    let total_ink = Ink::new(size, theme::TEXT).bold();
    let time_ink = Ink::new(SMALL, theme::TEXT_DIM);
    let (total_w, time_w) = (cv.measure(&lines.total, size, true), cv.measure(&lines.time, SMALL, false));
    let left = right - total_w - 4.0 - time_w;
    put(cv, left, y, &lines.total, total_ink, Anchor::LA);
    put(cv, left + total_w + 4.0, y + (size - SMALL) * 0.75, &lines.time, time_ink, Anchor::LA);
}

fn small(cv: &mut Canvas<'_>, x: f32, y: f32, text: &str, room: f32, colour: theme::Rgba) -> f32 {
    let text = fit(cv, text, SMALL, room);
    put(cv, x, y, &text, Ink::new(SMALL, colour), Anchor::LA)
}

fn narrow(cv: &mut Canvas<'_>, lines: &Lines) {
    let room = cv.width - 2.0 * PAD;
    rate(cv, PAD, TOP, 17.0, &lines.rate);
    small(cv, PAD, TOP + 21.0, &lines.average, room, theme::TEXT_DIM);
    let session = format!("{} {}", lines.total, lines.time);
    small(cv, PAD, BOTTOM - 25.0, &session, room, theme::TEXT);
    small(cv, PAD, BOTTOM - 12.0, &lines.level, room, theme::TEXT_DIM);
}

fn medium(cv: &mut Canvas<'_>, lines: &Lines) {
    let room = cv.width - 2.0 * PAD;
    rate(cv, PAD, TOP + 1.0, 20.0, &lines.rate);
    total(cv, cv.width - PAD, TOP + 3.0, lines, 12.0);
    // The pace goes beside the average when the strip has room for both, else at the foot.
    let both = format!("{} · {}", lines.average, lines.pace);
    let pace_above = !lines.pace.is_empty() && cv.measure(&both, SMALL, false) <= room;
    let above = if pace_above { both } else { lines.average.clone() };
    small(cv, PAD, TOP + 27.0, &above, room, theme::TEXT_DIM);
    let y = BOTTOM - 10.0;
    let level_w = small(cv, PAD, y, &lines.level, room, theme::TEXT);
    if !pace_above && !lines.pace.is_empty() {
        let pace = fit(cv, &lines.pace, SMALL, room - level_w - 10.0);
        put(cv, cv.width - PAD, y, &pace, Ink::new(SMALL, theme::TEXT_FAINT), Anchor::RA);
    }
}
