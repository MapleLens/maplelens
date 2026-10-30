//! The training card: EXP rates of the session being played, and the party quest's stage with what to do there
//! (Cloto's answers on the first stage, every way to stand on the ones that need it). Drawn like the game's
//! tooltips, so it sits over the game as if it were part of it. The Windows app draws the EXP as a strip beside the
//! status bar, with a graph of the last minutes behind its numbers and the map drops' button (`strip`, `layout`,
//! `graph`); the web's floating window, a box, keeps the tall card.

mod graph;
mod layout;
mod strip;

pub use strip::{
    FALLBACK_WIDTH, MAX_STRIP_WIDTH, MIN_STRIP_WIDTH, STRIP_HEIGHT, drops_button, on_drops_button, strip_width,
};

use super::RgbaImage;
use super::canvas::{Anchor, Canvas, Ink};
use super::font::Fonts;
use super::glance::PAD;
use super::theme;
use crate::market::format::format_mesos;
use crate::training::kpq::{self, Run};
use crate::training::potions::Potions;
use crate::training::session::{MIN_RATE_S, TrainingView};
use crate::training::watcher::Training;

/// Design width (1x; drawn CARD_SCALE times bigger, like the other cards).
pub const CARD_WIDTH: f32 = 214.0;
const MAX_HEIGHT: f32 = 900.0;
const GAUGE_H: f32 = 5.0;
const COMBO_H: f32 = 17.0;
const COMBO_GAP: f32 = 4.0;
const ANSWER_ROW: f32 = 15.0;
/// A potion's icon on the card, and the chip around it.
const POTION_ICON: f32 = 16.0;
const POTION_CHIP_H: f32 = 20.0;
/// Potions shown at most, the ones that moved most first.
const POTION_LINES: usize = 6;

/// What the player wants on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Show {
    pub exp: bool,
    pub quest: bool,
    /// The wide strip (the Windows app), else the tall card.
    pub strip: Option<Strip>,
}

/// The strip: its map drops button (None while the map has no drops to show), its width in pixels (`strip_width`;
/// None: `FALLBACK_WIDTH`, above the HUD).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Strip {
    pub drops: Option<Button>,
    pub width: Option<u32>,
}

/// How a button on a card is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Idle,
    /// Under the pointer.
    Lit,
    /// What it opens is open.
    On,
}

/// 75 -> "1:15", 3_725 -> "1:02:05".
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3_600 {
        format!("{}:{:02}:{:02}", s / 3_600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// "~14 min", "~2 h 5 min", "under a minute".
pub fn eta(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match minutes {
        0 => "under a minute".into(),
        1..=59 => format!("~{minutes} min"),
        _ if minutes.is_multiple_of(60) => format!("~{} h", minutes / 60),
        _ => format!("~{} h {} min", minutes / 60, minutes % 60),
    }
}

/// Is there anything worth a card? EXP once the session has gained some while the bar is in view; the quest while on
/// its maps.
pub fn has_content(training: &Training, show: Show) -> bool {
    (show.exp && is_training(&training.exp)) || (show.quest && training.quest.is_some())
}

/// Is the player training now: a session with EXP gained while the bar is in view, or one held by a pause? From the
/// first kill, so a player training sees the EXP card at once (the rates fill in after `MIN_RATE_S`); what the card's
/// EXP section waits for, and the web page's training panel and floating window too.
pub fn is_training(view: &TrainingView) -> bool {
    view.live && (view.paused || view.session.gained > 0)
}

/// The card, or None when there is nothing to show.
pub fn render_training_card(
    fonts: &Fonts,
    training: &Training,
    show: Show,
    now: f64,
    ui_scale: f32,
) -> Option<RgbaImage> {
    if !has_content(training, show) {
        return None;
    }
    let factor = theme::CARD_SCALE * ui_scale;
    if let Some(strip) = show.strip {
        return Some(strip::render(fonts, training, show, strip, now, factor));
    }
    let mut body = Canvas::new(fonts, CARD_WIDTH, MAX_HEIGHT, factor);
    let mut y = 8.0;
    if show.exp && is_training(&training.exp) {
        y = exp_section(&mut body, y, &training.exp);
        y = potions_section(&mut body, y, &training.potions);
    }
    if show.quest
        && let Some(run) = training.quest
    {
        if y > 8.0 {
            body.divider(y, PAD);
            y += 8.0;
        }
        y = quest_section(&mut body, y, run, now);
    }
    let height = y + 4.0;
    let mut card = Canvas::new(fonts, CARD_WIDTH, height, factor);
    card.frame(height);
    card.composite(&body, 0.0, 0.0);
    Some(RgbaImage::from_canvas(&card))
}

fn title(cv: &mut Canvas<'_>, y: f32, text: &str, chip: Option<(&str, theme::Rgba)>) {
    cv.circle(PAD + 3.5, y + 7.5, 3.5, theme::BULLET, None);
    cv.text(PAD + 13.0, y + 2.0, text, Ink::new(11.0, theme::TEXT_DIM).bold(), Anchor::LA);
    if let Some((label, colour)) = chip {
        cv.chip(cv.width - PAD, y, label, Ink::new(10.0, colour).bold(), true, 16.0);
    }
}

fn exp_section(cv: &mut Canvas<'_>, y: f32, view: &TrainingView) -> f32 {
    let chip = if view.paused { ("PAUSED", theme::WARN) } else { ("LIVE", theme::UP) };
    title(cv, y, "EXP", Some(chip));
    let mut y = y + 24.0;
    let rate = view.last_minute.or(view.recent_per_min);
    let big = rate.map_or_else(|| "—".to_string(), |r| format_mesos(Some(r)));
    let width = cv.text(PAD, y, &big, Ink::new(20.0, theme::GOLD).bold(), Anchor::LA);
    let label = if view.last_minute.is_some() { "a minute, last minute" } else { "a minute" };
    cv.text(PAD + width + 6.0, y + 7.0, label, Ink::new(10.0, theme::TEXT_DIM), Anchor::LA);
    y += 26.0;
    let average = view.average_per_min.map_or_else(|| "—".into(), |a| format_mesos(Some(a)));
    let best = format_mesos(Some(view.session.best_minute as f64));
    cv.text(PAD, y, &format!("Average {average} · best minute {best}"), Ink::new(11.0, theme::TEXT), Anchor::LA);
    y += 18.0;
    if let Some(reading) = view.reading {
        let share = reading.percent();
        let to_level = view.to_level_s.map_or_else(|| "Level up: —".into(), |s| format!("Level up in {}", eta(s)));
        cv.text(PAD, y, &to_level, Ink::new(11.0, theme::TEXT), Anchor::LA);
        cv.text(cv.width - PAD, y, &format!("{share:.2}%"), Ink::new(11.0, theme::TEXT_DIM), Anchor::RA);
        y += 16.0;
        let track = cv.width - 2.0 * PAD;
        cv.rounded((PAD, y, track, GAUGE_H), 2.0, Some(theme::TRACK), None);
        let filled = (track * (share as f32 / 100.0)).clamp(0.0, track);
        if filled >= 1.0 {
            cv.rounded((PAD, y, filled.max(3.0), GAUGE_H), 2.0, Some(theme::UP), None);
        }
        y += GAUGE_H + 9.0;
    }
    let session = &view.session;
    let mut parts = vec![format!("+{} in {}", format_mesos(Some(session.gained as f64)), clock(session.active_s))];
    if session.levels > 0 {
        parts.push(format!("{} level{}", session.levels, if session.levels == 1 { "" } else { "s" }));
    }
    if let Some(per_hour) = view.percent_per_hour {
        parts.push(format!("{per_hour:.1}%/h"));
    }
    cv.text(PAD, y, &parts.join(" · "), Ink::new(10.0, theme::TEXT_DIM), Anchor::LA);
    y += 15.0;
    if let (Some(map), Some(stat)) = (&view.map, &view.map_stat) {
        let rate = (stat.active_s >= MIN_RATE_S)
            .then(|| format!(" · {}/min", format_mesos(Some(stat.gained as f64 * 60.0 / stat.active_s))));
        let text = format!("{map}: +{}{}", format_mesos(Some(stat.gained as f64)), rate.unwrap_or_default());
        let text = fit(cv, &text, 10.0, cv.width - 2.0 * PAD);
        cv.text(PAD, y, &text, Ink::new(10.0, theme::TEXT_FAINT), Anchor::LA);
        y += 15.0;
    }
    y + 2.0
}

/// The quickslots' potions this session: each one's icon with how many were used (red) and picked up (green).
fn potions_section(cv: &mut Canvas<'_>, y: f32, potions: &Potions) -> f32 {
    if potions.lines.is_empty() {
        return y;
    }
    let mut lines: Vec<_> = potions.lines.iter().collect();
    lines.sort_by_key(|l| std::cmp::Reverse(l.used + l.picked));
    let (mut x, mut y) = (PAD, y);
    let size = 11.0;
    for line in lines.into_iter().take(POTION_LINES) {
        let parts: Vec<(String, theme::Rgba)> = [(line.used, '−', theme::DOWN), (line.picked, '+', theme::UP)]
            .into_iter()
            .filter(|(n, ..)| *n > 0)
            .map(|(n, sign, colour)| (format!("{sign}{n}"), colour))
            .collect();
        let text_w: f32 = parts.iter().map(|(t, _)| cv.measure(t, size, true) + 4.0).sum();
        let width = 6.0 + POTION_ICON + 4.0 + text_w + 4.0;
        if x + width > cv.width - PAD {
            x = PAD;
            y += POTION_CHIP_H + 4.0;
        }
        cv.rounded((x, y, width, POTION_CHIP_H), 5.0, Some(theme::CHIP_FILL), Some(theme::CHIP_EDGE));
        if let Some(icon) = &line.icon {
            let fit = (POTION_ICON / icon.width as f32).min(POTION_ICON / icon.height as f32);
            let (w, h) = (icon.width as f32 * fit, icon.height as f32 * fit);
            let (ix, iy) = (x + 6.0 + (POTION_ICON - w) / 2.0, y + (POTION_CHIP_H - h) / 2.0);
            cv.image(ix, iy, w, h, (icon.width, icon.height, &icon.data));
        }
        let mut tx = x + 6.0 + POTION_ICON + 4.0;
        for (text, colour) in &parts {
            tx += cv.text(tx, y + POTION_CHIP_H / 2.0, text, Ink::new(size, *colour).bold(), Anchor::LM) + 4.0;
        }
        x += width + 4.0;
    }
    y + POTION_CHIP_H + 8.0
}

fn quest_section(cv: &mut Canvas<'_>, y: f32, run: Run, now: f64) -> f32 {
    let stage = kpq::stage(run.stage);
    let elapsed = now - run.started;
    let left = kpq::TIME_LIMIT_S - elapsed;
    let timer = if left > 0.0 { format!("{} left", clock(left)) } else { clock(elapsed) };
    title(cv, y, kpq::NAME, Some((&timer, theme::TEXT_DIM)));
    let mut y = y + 24.0;
    let heading = if run.stage < kpq::STAGES.len() {
        format!("Stage {} of {} · {}", run.stage + 1, kpq::STAGES.len(), stage.name)
    } else {
        stage.name.to_string()
    };
    cv.text(PAD, y, &heading, Ink::new(14.0, theme::TEXT).bold(), Anchor::LA);
    y += 20.0;
    for line in cv.wrap(stage.goal, 10.0, cv.width - 2.0 * PAD, false) {
        cv.text(PAD, y, &line, Ink::new(10.0, theme::TEXT_DIM), Anchor::LA);
        y += 13.0;
    }
    y += 4.0;
    if run.stage == 0 {
        y = answers(cv, y);
    }
    if let Some((pick, of)) = stage.combination {
        y = combinations(cv, y, pick, of);
    }
    y
}

fn answers(cv: &mut Canvas<'_>, mut y: f32) -> f32 {
    cv.text(PAD, y, "CLOTO ASKS → COUPONS", Ink::new(9.0, theme::TEXT_FAINT).bold(), Anchor::LA);
    y += 14.0;
    for question in kpq::QUESTIONS {
        let asks = fit(cv, question.asks, 10.0, cv.width - 2.0 * PAD - 26.0);
        cv.text(PAD + 24.0, y, &asks, Ink::new(10.0, theme::TEXT), Anchor::LA);
        cv.text(PAD + 16.0, y, &question.coupons.to_string(), Ink::new(11.0, theme::GOLD).bold(), Anchor::RA);
        y += ANSWER_ROW;
    }
    y + 4.0
}

/// Every way to stand, as chips flowing across the card; the spots are numbered from the left.
fn combinations(cv: &mut Canvas<'_>, mut y: f32, pick: u8, of: u8) -> f32 {
    let all = kpq::combinations(pick, of);
    cv.text(
        PAD,
        y,
        &format!("{} WAYS · SPOTS 1–{of} FROM THE LEFT", all.len()),
        Ink::new(9.0, theme::TEXT_FAINT).bold(),
        Anchor::LA,
    );
    y += 15.0;
    let mut x = PAD;
    let ink = Ink::new(10.0, theme::TEXT).bold();
    for combo in all {
        let label: String = combo.iter().map(u8::to_string).collect::<Vec<_>>().join(" ");
        let width = cv.measure(&label, ink.size, true) + 14.0;
        if x + width > cv.width - PAD {
            x = PAD;
            y += COMBO_H + COMBO_GAP;
        }
        x += cv.chip(x, y, &label, ink, false, COMBO_H) + COMBO_GAP;
    }
    y + COMBO_H + 8.0
}

fn fit(cv: &Canvas<'_>, text: &str, size: f32, width: f32) -> String {
    if cv.measure(text, size, false) <= width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() && cv.measure(&format!("{}…", chars.iter().collect::<String>()), size, false) > width {
        chars.pop();
    }
    format!("{}…", chars.iter().collect::<String>().trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_naturally() {
        assert_eq!(clock(75.0), "1:15");
        assert_eq!(clock(3_725.0), "1:02:05");
        assert_eq!(eta(20.0), "under a minute");
        assert_eq!(eta(14.0 * 60.0), "~14 min");
        assert_eq!(eta(125.0 * 60.0), "~2 h 5 min");
        assert_eq!(eta(120.0 * 60.0), "~2 h");
    }
}
