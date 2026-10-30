//! The EXP as a wide strip as tall as the game's status bar, so it sits beside the bar, level with it: the rate now
//! and the session's on the left, the session's total, pace and the next level on the right, the potions used and the
//! map drops' button along the top, the level's gauge along the foot, and the last ten minutes' graph behind it all.
//! The party quest's section, when shown, hangs below it.

use super::graph;
use super::{Button, Show, Strip, clock, eta, is_training, quest_section};
use crate::market::format::format_mesos;
use crate::render::RgbaImage;
use crate::render::canvas::{Anchor, Canvas, Ink, rounded_rect};
use crate::render::font::Fonts;
use crate::render::glance::PAD;
use crate::render::theme;
use crate::training::potions::Potions;
use crate::training::session::TrainingView;
use crate::training::watcher::Training;

/// Design width: drawn 1.5x the UI scale it fits beside the status bar at 16:9 (the room each side of it is 397 px at
/// 1080p, 283 at 768p); at 16:10 it goes above the bar.
pub const STRIP_WIDTH: f32 = 260.0;
/// The status bar's height at UI scale 1 (106 px) in design units: the strip is as tall as the bar.
pub const STRIP_HEIGHT: f32 = 106.0 / theme::CARD_SCALE;
/// The map drops' button, top right: (x, y, w, h) in design units.
pub const DROPS_BUTTON: (f32, f32, f32, f32) = (STRIP_WIDTH - 8.0 - 24.0, 5.0, 24.0, 16.0);
/// 10% less see-through than the other cards' tooltip fill (215): over a busy field the numbers must read at a glance.
const FILL: theme::Rgba = [34, 37, 80, 241];
/// The right column's left edge.
const RIGHT: f32 = 138.0;
const GAUGE_Y: f32 = 60.0;
const GAUGE_H: f32 = 4.0;
/// The graph's box: below the header, above the gauge, inside the frame.
const GRAPH: (f32, f32, f32, f32) = (3.0, 24.0, STRIP_WIDTH - 6.0, GAUGE_Y - 26.0);
/// Potions in the header at most (the ones that moved most).
const POTIONS: usize = 3;
const POTION_ICON: f32 = 11.0;
/// Under the pointer: the button lit this much.
const LIT: theme::Rgba = [255, 255, 255, 34];

/// Is (x, y), in design units from the card's corner, on the map drops' button?
pub fn on_drops_button(x: f32, y: f32) -> bool {
    let (bx, by, bw, bh) = DROPS_BUTTON;
    (bx..bx + bw).contains(&x) && (by..by + bh).contains(&y)
}

pub(super) fn render(fonts: &Fonts, training: &Training, show: Show, strip: Strip, now: f64, factor: f32) -> RgbaImage {
    let exp = show.exp && is_training(&training.exp);
    let quest = training.quest.filter(|_| show.quest);
    let top = if exp { STRIP_HEIGHT } else { 0.0 };
    let height = match quest {
        Some(run) => {
            let mut probe = Canvas::new(fonts, STRIP_WIDTH, 900.0, factor);
            quest_section(&mut probe, top + 8.0, run, now) + 4.0
        }
        None => top,
    };
    let mut cv = Canvas::new(fonts, STRIP_WIDTH, height, factor);
    frame(&mut cv, height);
    if exp {
        exp_strip(&mut cv, &training.exp, &training.potions, strip);
    }
    if let Some(run) = quest {
        if exp {
            cv.divider(top, PAD);
        }
        quest_section(&mut cv, top + 8.0, run, now);
    }
    RgbaImage::from_canvas(&cv)
}

/// The tooltip frame over the strip's denser fill.
fn frame(cv: &mut Canvas<'_>, height: f32) {
    let w = cv.width;
    cv.rect(0.0, 0.0, w, height, FILL);
    if let Some(path) = rounded_rect(0.5, 0.5, w - 1.0, height - 1.0, 0.0) {
        cv.stroke_path(&path, theme::EDGE, 1.0);
    }
    if let Some(path) = rounded_rect(2.5, 2.5, w - 5.0, height - 5.0, 0.0) {
        cv.stroke_path(&path, theme::EDGE_INNER, 1.0);
    }
}

fn exp_strip(cv: &mut Canvas<'_>, view: &TrainingView, potions: &Potions, strip: Strip) {
    graph::draw(cv, GRAPH, &view.graph, view.average_per_min);
    // The header: what it is and whether it counts, the potions, the drops' button.
    cv.circle(PAD + 3.5, 12.5, 3.5, theme::BULLET, None);
    let label = Ink::new(10.0, theme::TEXT_DIM).bold();
    let x = PAD + 12.0 + cv.text(PAD + 12.0, 12.5, "EXP", label, Anchor::LM) + 6.0;
    // Only a pause is said: a strip in view is live.
    let chip_end =
        if view.paused { x + cv.chip(x, 6.0, "PAUSED", Ink::new(8.0, theme::WARN).bold(), false, 13.0) } else { x };
    let mut right = STRIP_WIDTH - PAD + 4.0;
    if let Some(button) = strip.drops {
        drops_button(cv, button);
        right = DROPS_BUTTON.0 - 6.0;
    }
    potions_row(cv, potions, chip_end + 6.0, right);

    // Left: the rate now, the session's average and best minute.
    let rate = view.last_minute.or(view.recent_per_min);
    let big = rate.map_or_else(|| "—".to_string(), |r| format_mesos(Some(r)));
    let width = cv.text(PAD, 22.0, &big, Ink::new(18.0, theme::GOLD).bold(), Anchor::LA);
    cv.text(PAD + width + 3.0, 29.0, "/min", Ink::new(9.0, theme::TEXT_DIM), Anchor::LA);
    let average = view.average_per_min.map_or_else(|| "—".into(), |a| format_mesos(Some(a)));
    let best = format_mesos(Some(view.session.best_minute as f64));
    let small = Ink::new(9.0, theme::TEXT_DIM);
    cv.text(PAD, 45.0, &format!("avg {average} · best {best}"), small, Anchor::LA);

    // Right: the session's total and time, its pace, the next level.
    let session = &view.session;
    let total = format!("+{}", format_mesos(Some(session.gained as f64)));
    let width = cv.text(RIGHT, 22.0, &total, Ink::new(11.0, theme::TEXT).bold(), Anchor::LA);
    cv.text(RIGHT + width + 4.0, 24.0, &format!("in {}", clock(session.active_s)), small, Anchor::LA);
    let mut pace = Vec::new();
    if let Some(per_hour) = view.percent_per_hour {
        pace.push(format!("{per_hour:.1}%/h"));
    }
    if session.levels > 0 {
        pace.push(format!("{} level{}", session.levels, if session.levels == 1 { "" } else { "s" }));
    }
    if !pace.is_empty() {
        cv.text(RIGHT, 34.5, &pace.join(" · "), Ink::new(9.0, theme::TEXT_FAINT), Anchor::LA);
    }
    if let Some(reading) = view.reading {
        let to_level = view.to_level_s.map_or_else(|| "Level up —".into(), |s| format!("Level up {}", eta(s)));
        cv.text(RIGHT, 45.0, &to_level, small, Anchor::LA);
        let share = reading.percent();
        cv.text(STRIP_WIDTH - PAD, 45.0, &format!("{share:.2}%"), Ink::new(9.0, theme::TEXT).bold(), Anchor::RA);
        gauge(cv, share as f32);
    }
}

/// The level's share along the foot, like the game's EXP bar.
fn gauge(cv: &mut Canvas<'_>, share: f32) {
    let track = STRIP_WIDTH - 2.0 * PAD;
    cv.rounded((PAD, GAUGE_Y, track, GAUGE_H), 2.0, Some(theme::TRACK), None);
    let filled = (track * share / 100.0).clamp(0.0, track);
    if filled >= 1.0 {
        cv.rounded((PAD, GAUGE_Y, filled.max(3.0), GAUGE_H), 2.0, Some(theme::GOLD), None);
    }
}

/// The potions that moved most, right-aligned before `right`: icon and how many were used (else picked up). Those
/// that do not fit after `left` are left out.
fn potions_row(cv: &mut Canvas<'_>, potions: &Potions, left: f32, right: f32) {
    let mut lines: Vec<_> = potions.lines.iter().filter(|l| l.used + l.picked > 0).collect();
    lines.sort_by_key(|l| std::cmp::Reverse(l.used + l.picked));
    let ink = |colour| Ink::new(8.5, colour).bold();
    let mut x = right;
    for line in lines.into_iter().take(POTIONS) {
        let (text, colour) = if line.used > 0 {
            (format!("−{}", line.used), theme::DOWN)
        } else {
            (format!("+{}", line.picked), theme::UP)
        };
        let width = POTION_ICON + 2.0 + cv.measure(&text, 8.5, true);
        if x - width < left {
            break;
        }
        x -= width;
        if let Some(icon) = &line.icon {
            let fit = (POTION_ICON / icon.width as f32).min(POTION_ICON / icon.height as f32);
            let (w, h) = (icon.width as f32 * fit, icon.height as f32 * fit);
            let (ix, iy) = (x + (POTION_ICON - w) / 2.0, 12.5 - h / 2.0);
            cv.image(ix, iy, w, h, (icon.width, icon.height, &icon.data));
        }
        cv.text(x + POTION_ICON + 2.0, 12.5, &text, ink(colour), Anchor::LM);
        x -= 7.0;
    }
}

/// The map drops' button: a little loot bag, lit under the pointer, gold while the panel is open.
fn drops_button(cv: &mut Canvas<'_>, button: Button) {
    let (x, y, w, h) = DROPS_BUTTON;
    let (fill, edge, ink) = match button {
        Button::Idle => (theme::CHIP_FILL, theme::CHIP_EDGE, theme::TEXT_DIM),
        Button::Lit => (LIT, theme::TEXT, theme::TEXT),
        Button::On => (theme::with_alpha(theme::GOLD, 60), theme::GOLD, theme::GOLD),
    };
    cv.rounded((x, y, w, h), 4.0, Some(fill), Some(edge));
    let (cx, cy) = (x + w / 2.0, y + h / 2.0 + 1.0);
    // The bag: a round body, a neck tied off, two ears over the knot.
    cv.circle(cx, cy + 1.5, 4.5, ink, None);
    cv.rect(cx - 2.0, cy - 4.0, 4.0, 2.5, ink);
    cv.polygon(&[(cx - 1.5, cy - 4.0), (cx - 4.0, cy - 6.5), (cx - 0.5, cy - 5.0)], ink);
    cv.polygon(&[(cx + 1.5, cy - 4.0), (cx + 4.0, cy - 6.5), (cx + 0.5, cy - 5.0)], ink);
    cv.rect(cx - 2.5, cy - 2.0, 5.0, 1.0, fill_behind(button));
}

/// The knot's line, in the button's own colour so it reads as a cut in the bag.
fn fill_behind(button: Button) -> theme::Rgba {
    match button {
        Button::On => [80, 70, 60, 255],
        _ => [40, 42, 82, 255],
    }
}
