//! The strip's body between its header and its gauge, laid out for the room it has. Narrow (4:3 screens): one column,
//! the graph between the rate and the session. Medium (16:10 and 16:9 at Default Ratio): the rate and the session's
//! total side by side over a graph across the strip. Wide (Ideal Ratio at 1080p, 1600x900): a column of numbers beside
//! a tall graph with its scale and time axis.

use super::graph::{self, Labels};
use super::strip::{HEADER_Y, STRIP_HEIGHT};
use super::{clock, eta, fit};
use crate::market::format::format_mesos;
use crate::render::canvas::{Anchor, Canvas, Ink};
use crate::render::glance::PAD;
use crate::render::theme;
use crate::training::session::TrainingView;

/// From these widths (design units) the medium and the wide layouts.
const MEDIUM: f32 = 200.0;
const WIDE: f32 = 360.0;
/// The body starts under the header and ends over the gauge.
const TOP: f32 = HEADER_Y + 11.0;
const BOTTOM: f32 = STRIP_HEIGHT - 19.0;
const SMALL: f32 = 9.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Narrow,
    Medium,
    Wide,
}

impl Shape {
    pub(super) fn of(width: f32) -> Self {
        if width >= WIDE {
            Self::Wide
        } else if width >= MEDIUM {
            Self::Medium
        } else {
            Self::Narrow
        }
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
    let lines = Lines::of(view);
    match shape {
        Shape::Narrow => narrow(cv, view, &lines),
        Shape::Medium => medium(cv, view, &lines),
        Shape::Wide => wide(cv, view, &lines),
    }
}

/// The rate now, big, with "/min" after it.
fn rate(cv: &mut Canvas<'_>, x: f32, y: f32, size: f32, text: &str) {
    let width = cv.text(x, y, text, Ink::new(size, theme::GOLD).bold(), Anchor::LA);
    cv.text(x + width + 3.0, y + size * 0.42, "/min", Ink::new(SMALL, theme::TEXT_DIM), Anchor::LA);
}

/// "+108k in 1:02:05" from `x` (or ending at `x` when `right`).
fn total(cv: &mut Canvas<'_>, x: f32, y: f32, lines: &Lines, size: f32, right: bool) {
    let total_ink = Ink::new(size, theme::TEXT).bold();
    let time_ink = Ink::new(SMALL, theme::TEXT_DIM);
    let (total_w, time_w) = (cv.measure(&lines.total, size, true), cv.measure(&lines.time, SMALL, false));
    let left = if right { x - total_w - 4.0 - time_w } else { x };
    cv.text(left, y, &lines.total, total_ink, Anchor::LA);
    cv.text(left + total_w + 4.0, y + (size - SMALL) * 0.75, &lines.time, time_ink, Anchor::LA);
}

fn small(cv: &mut Canvas<'_>, x: f32, y: f32, text: &str, room: f32, colour: theme::Rgba) {
    let text = fit(cv, text, SMALL, room);
    cv.text(x, y, &text, Ink::new(SMALL, colour), Anchor::LA);
}

fn narrow(cv: &mut Canvas<'_>, view: &TrainingView, lines: &Lines) {
    let room = cv.width - 2.0 * PAD;
    rate(cv, PAD, TOP, 17.0, &lines.rate);
    small(cv, PAD, TOP + 21.0, &lines.average, room, theme::TEXT_DIM);
    let well = (PAD - 3.0, TOP + 34.0, room + 6.0, 38.0);
    graph::draw(cv, well, &view.graph, view.average_per_min, Labels::None);
    let session = format!("{} {}", lines.total, lines.time);
    small(cv, PAD, BOTTOM - 25.0, &session, room, theme::TEXT);
    small(cv, PAD, BOTTOM - 12.0, &lines.level, room, theme::TEXT_DIM);
}

fn medium(cv: &mut Canvas<'_>, view: &TrainingView, lines: &Lines) {
    let room = cv.width - 2.0 * PAD;
    rate(cv, PAD, TOP + 1.0, 20.0, &lines.rate);
    total(cv, cv.width - PAD, TOP + 3.0, lines, 12.0, true);
    // The pace goes beside the average when the strip has room for both, else under the graph.
    let both = format!("{} · {}", lines.average, lines.pace);
    let pace_above = !lines.pace.is_empty() && cv.measure(&both, SMALL, false) <= room;
    let above = if pace_above { both } else { lines.average.clone() };
    small(cv, PAD, TOP + 27.0, &above, room, theme::TEXT_DIM);
    let well = (PAD - 3.0, TOP + 41.0, room + 6.0, BOTTOM - TOP - 54.0);
    graph::draw(cv, well, &view.graph, view.average_per_min, Labels::Peak);
    let y = BOTTOM - 10.0;
    let level_w = cv.text(PAD, y, &lines.level, Ink::new(SMALL, theme::TEXT_DIM), Anchor::LA);
    if !pace_above && !lines.pace.is_empty() {
        let pace = fit(cv, &lines.pace, SMALL, room - level_w - 10.0);
        cv.text(cv.width - PAD, y, &pace, Ink::new(SMALL, theme::TEXT_FAINT), Anchor::RA);
    }
}

fn wide(cv: &mut Canvas<'_>, view: &TrainingView, lines: &Lines) {
    let column = (cv.width * 0.36).clamp(160.0, 210.0);
    let room = column - 10.0;
    rate(cv, PAD, TOP + 1.0, 22.0, &lines.rate);
    small(cv, PAD, TOP + 30.0, &lines.average, room, theme::TEXT_DIM);
    total(cv, PAD, TOP + 46.0, lines, 12.0, false);
    small(cv, PAD, TOP + 64.0, &lines.pace, room, theme::TEXT_FAINT);
    small(cv, PAD, TOP + 79.0, &lines.level, room, theme::TEXT);
    let x = PAD + column;
    let well = (x, TOP + 1.0, cv.width - PAD - x + 3.0, BOTTOM - TOP - 1.0);
    graph::draw(cv, well, &view.graph, view.average_per_min, Labels::Axes);
}
