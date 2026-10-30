//! The EXP as a strip beside the game's status bar, from the screen's side to the bar and from the client's foot up to
//! the top of the quickslots' Pup button, so it reads as part of the HUD. Its height is fixed with the UI; its width is
//! the room beside the bar (as on a 16:9 screen at most, and `MAX_STRIP_WIDTH`), and the layout follows it (`layout`):
//! one column when narrow, the graph across the middle, or a column of numbers beside a tall graph when wide. The
//! header carries the potions used and the map drops' button, the foot the level's gauge. The party quest's section,
//! when shown, hangs below it.

use super::layout::{self, Shape};
use super::{Button, Show, Strip, fit, is_training, quest_section};
use crate::render::RgbaImage;
use crate::render::canvas::{Anchor, Canvas, Ink, rounded_rect};
use crate::render::font::Fonts;
use crate::render::glance::PAD;
use crate::render::theme;
use crate::training::potions::Potions;
use crate::training::session::TrainingView;
use crate::training::watcher::Training;

/// A design unit at the UI's 1:1 size (Ideal Ratio, UI scale 0.64), in pixels.
const ONE_X: f32 = theme::CARD_SCALE * 0.64;
/// The Pup quickslot's top above the client's foot at 1:1, measured on the game's own renders (mscw-render, every
/// resolution and UI size: 139 px at 1x, 195 at 1080p's 1.41x) and a 1080p capture.
const PUP_TOP_1X: f32 = 139.0;
/// The strip's height in design units: level with the Pup button.
pub const STRIP_HEIGHT: f32 = PUP_TOP_1X / ONE_X;
/// Narrower than this there is no room beside the bar (5:4, 800x600): the strip goes above the HUD at `FALLBACK_WIDTH`.
/// 4:3 leaves 117.
pub const MIN_STRIP_WIDTH: f32 = 110.0;
/// Ideal Ratio at 1080p leaves 583; wider (Ideal Ratio at 1440p leaves 917) the numbers would sit far from the graph.
pub const MAX_STRIP_WIDTH: f32 = 600.0;
pub const FALLBACK_WIDTH: f32 = 260.0;
/// Clear of the status bar's outline, at 1x.
const BAR_GAP_1X: f32 = 2.0;
/// The widest screen whose room counts: past 16:9 (21:9, 32:9) the strip keeps 16:9's width.
const MAX_ASPECT: f32 = 16.0 / 9.0;
/// 10% less see-through than the other cards' tooltip fill (215): over a busy field the numbers must read at a glance.
const FILL: theme::Rgba = [34, 37, 80, 241];
const BUTTON: (f32, f32) = (24.0, 16.0);
/// Potions in the header at most (the ones that moved most).
const POTIONS: usize = 3;
const POTION_ICON: f32 = 11.0;
/// Under the pointer: the button lit this much.
const LIT: theme::Rgba = [255, 255, 255, 34];
/// The header's middle, and the foot's (the gauge and the level's share).
pub(super) const HEADER_Y: f32 = 13.0;
const FOOT_Y: f32 = STRIP_HEIGHT - 9.0;
const GAUGE_H: f32 = 4.0;

/// The strip's width in pixels for a client of `client` (w, h) with the status bar `bar_w` pixels wide, drawn at
/// `factor` pixels a design unit: the room from the client's side to the bar, as on a 16:9 screen at most and
/// `MAX_STRIP_WIDTH` at most; None when that is under `MIN_STRIP_WIDTH`.
pub fn strip_width(client: (i32, i32), bar_w: i32, factor: f32) -> Option<u32> {
    let room = (client.0 as f32).min(client.1 as f32 * MAX_ASPECT);
    let px = (room - bar_w as f32) / 2.0 - BAR_GAP_1X / ONE_X * factor;
    let design = (px / factor).min(MAX_STRIP_WIDTH);
    (design >= MIN_STRIP_WIDTH).then(|| (design * factor).floor() as u32)
}

/// The map drops' button of a strip `width` design units wide, top right: (x, y, w, h).
pub fn drops_button(width: f32) -> (f32, f32, f32, f32) {
    (width - 8.0 - BUTTON.0, HEADER_Y - BUTTON.1 / 2.0, BUTTON.0, BUTTON.1)
}

/// Is (x, y), in design units from the corner of a strip `width` design units wide, on the map drops' button?
pub fn on_drops_button(width: f32, x: f32, y: f32) -> bool {
    let (bx, by, bw, bh) = drops_button(width);
    (bx..bx + bw).contains(&x) && (by..by + bh).contains(&y)
}

pub(super) fn render(fonts: &Fonts, training: &Training, show: Show, strip: Strip, now: f64, factor: f32) -> RgbaImage {
    let width = strip.width.map_or(FALLBACK_WIDTH, |px| px as f32 / factor);
    let exp = show.exp && is_training(&training.exp);
    let quest = training.quest.filter(|_| show.quest);
    let top = if exp { STRIP_HEIGHT } else { 0.0 };
    let height = match quest {
        Some(run) => {
            let mut probe = Canvas::new(fonts, width, 900.0, factor);
            quest_section(&mut probe, top + 8.0, run, now) + 4.0
        }
        None => top,
    };
    let mut cv = Canvas::new(fonts, width, height, factor);
    frame(&mut cv, height);
    if exp {
        let view = &training.exp;
        let shape = Shape::of(width);
        header(&mut cv, view, &training.potions, strip.drops, shape);
        layout::body(&mut cv, view, shape);
        foot(&mut cv, view);
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

/// What it is (and a pause), the map when there is room, the potions and the drops' button on the right.
fn header(cv: &mut Canvas<'_>, view: &TrainingView, potions: &Potions, drops: Option<Button>, shape: Shape) {
    cv.circle(PAD + 3.5, HEADER_Y, 3.5, theme::BULLET, None);
    let label = Ink::new(10.0, theme::TEXT_DIM).bold();
    let mut x = PAD + 12.0 + cv.text(PAD + 12.0, HEADER_Y, "EXP", label, Anchor::LM) + 6.0;
    if view.paused {
        // Only a pause is said: a strip in view is live.
        x += cv.chip(x, HEADER_Y - 6.5, "PAUSED", Ink::new(8.0, theme::WARN).bold(), false, 13.0) + 6.0;
    }
    let mut right = cv.width - PAD + 4.0;
    if let Some(button) = drops {
        drops_button_at(cv, button);
        right = drops_button(cv.width).0 - 6.0;
    }
    right = potions_row(cv, potions, x, right);
    if shape != Shape::Narrow
        && let Some(map) = &view.map
    {
        let room = right - x - 8.0;
        let name = fit(cv, map, 9.0, room);
        if room > 30.0 {
            cv.text(x, HEADER_Y, &name, Ink::new(9.0, theme::TEXT_FAINT), Anchor::LM);
        }
    }
}

/// The level's share along the foot, like the game's EXP bar, with the share at its end.
fn foot(cv: &mut Canvas<'_>, view: &TrainingView) {
    let Some(reading) = view.reading else { return };
    let share = reading.percent();
    let text = format!("{share:.2}%");
    let ink = Ink::new(9.0, theme::TEXT).bold();
    let end = cv.width - PAD;
    let track = end - cv.measure(&text, ink.size, true) - 6.0 - PAD;
    cv.text(end, FOOT_Y, &text, ink, Anchor::RM);
    let y = FOOT_Y - GAUGE_H / 2.0;
    cv.rounded((PAD, y, track, GAUGE_H), 2.0, Some(theme::TRACK), None);
    let filled = (track * share as f32 / 100.0).clamp(0.0, track);
    if filled >= 1.0 {
        cv.rounded((PAD, y, filled.max(3.0), GAUGE_H), 2.0, Some(theme::GOLD), None);
    }
}

/// The potions that moved most, right-aligned before `right`: icon and how many were used (else picked up). Those
/// that do not fit after `left` are left out. Returns where the row starts.
fn potions_row(cv: &mut Canvas<'_>, potions: &Potions, left: f32, right: f32) -> f32 {
    let mut lines: Vec<_> = potions.lines.iter().filter(|l| l.used + l.picked > 0).collect();
    lines.sort_by_key(|l| std::cmp::Reverse(l.used + l.picked));
    let mut x = right;
    for line in lines.into_iter().take(POTIONS) {
        let (text, colour) = if line.used > 0 {
            (format!("−{}", line.used), theme::DOWN)
        } else {
            (format!("+{}", line.picked), theme::UP)
        };
        let width = POTION_ICON + 2.0 + cv.measure(&text, 8.5, true);
        if x - width < left + 40.0 {
            break;
        }
        x -= width;
        if let Some(icon) = &line.icon {
            let fit = (POTION_ICON / icon.width as f32).min(POTION_ICON / icon.height as f32);
            let (w, h) = (icon.width as f32 * fit, icon.height as f32 * fit);
            let (ix, iy) = (x + (POTION_ICON - w) / 2.0, HEADER_Y - h / 2.0);
            cv.image(ix, iy, w, h, (icon.width, icon.height, &icon.data));
        }
        cv.text(x + POTION_ICON + 2.0, HEADER_Y, &text, Ink::new(8.5, colour).bold(), Anchor::LM);
        x -= 7.0;
    }
    x
}

/// The map drops' button: a little loot bag, lit under the pointer, gold while the panel is open.
fn drops_button_at(cv: &mut Canvas<'_>, button: Button) {
    let (x, y, w, h) = drops_button(cv.width);
    let (fill, edge, ink, knot) = match button {
        Button::Idle => (theme::CHIP_FILL, theme::CHIP_EDGE, theme::TEXT_DIM, [40, 42, 82, 255]),
        Button::Lit => (LIT, theme::TEXT, theme::TEXT, [40, 42, 82, 255]),
        Button::On => (theme::with_alpha(theme::GOLD, 60), theme::GOLD, theme::GOLD, [80, 70, 60, 255]),
    };
    cv.rounded((x, y, w, h), 4.0, Some(fill), Some(edge));
    let (cx, cy) = (x + w / 2.0, y + h / 2.0 + 1.0);
    // The bag: a round body, a neck tied off, two ears over the knot.
    cv.circle(cx, cy + 1.5, 4.5, ink, None);
    cv.rect(cx - 2.0, cy - 4.0, 4.0, 2.5, ink);
    cv.polygon(&[(cx - 1.5, cy - 4.0), (cx - 4.0, cy - 6.5), (cx - 0.5, cy - 5.0)], ink);
    cv.polygon(&[(cx + 1.5, cy - 4.0), (cx + 4.0, cy - 6.5), (cx + 0.5, cy - 5.0)], ink);
    cv.rect(cx - 2.5, cy - 2.0, 5.0, 1.0, knot);
}
