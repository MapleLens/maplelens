//! The rank panel shown beside the Character Info window, styled like the game's own windows (pure).

use super::RgbaImage;
use super::canvas::Canvas;
use super::font::Fonts;
use super::gamewin;
use crate::market::format::{format_ago, group_thousands};
use crate::ranking::{Rank, RankResult, Status};

/// Native pixels: it sits next to a game window, so no CARD_SCALE.
pub const CARD_WIDTH: f32 = 204.0;
const MAX_HEIGHT: f32 = 300.0;
const HERO_H: f32 = 30.0;
const CREDIT: &str = "data: classicrank.com";
/// Room under the last row for the 'updated' line.
const NOTE_H: f32 = 14.0;

fn number(value: Option<i64>) -> String {
    value.map_or("—".into(), |v| format!("#{}", group_thousands(v)))
}

fn message(status: Status) -> (&'static str, &'static str) {
    match status {
        Status::Missing => ("Not ranked yet", "No record on classicrank.com"),
        Status::Failed => ("Rank unavailable", "will retry shortly"),
        Status::Loading | Status::Found => ("Looking up rank…", "one moment"),
    }
}

/// Panel for a player in whatever state their lookup is in (the game window already shows the name).
pub fn render_rank_card(fonts: &Fonts, result: &RankResult, now: f64, ui_scale: f32) -> RgbaImage {
    let mut body = Canvas::new(fonts, CARD_WIDTH, MAX_HEIGHT, ui_scale);
    let y = gamewin::PANEL_TOP + gamewin::PAD_V;
    let (y, note) = match &result.rank {
        None => {
            let (headline, detail) = message(result.status);
            (notice(&mut body, y, headline, detail), None)
        }
        Some(rank) => ranks(&mut body, y, rank, now),
    };
    if let Some(note) = &note {
        gamewin::centred(&mut body, CARD_WIDTH / 2.0, y + 5.0, note, 10.0, gamewin::INK_DIM);
    }
    let panel_bottom = y + if note.is_some() { NOTE_H } else { gamewin::PAD_V - gamewin::ROW_GAP };
    let height = panel_bottom + 1.0 + gamewin::STRIP_H;
    let mut card = Canvas::new(fonts, CARD_WIDTH, height, ui_scale);
    gamewin::window(&mut card, height, "RANKING");
    // Only the panel's content comes from the body; the frame above it is the card's own.
    card.composite_below(&body, gamewin::PANEL_TOP);
    gamewin::centred(
        &mut card,
        CARD_WIDTH / 2.0,
        panel_bottom + 1.0 + gamewin::STRIP_H / 2.0 - 2.0,
        CREDIT,
        10.0,
        gamewin::INK_DIM,
    );
    RgbaImage::from_canvas(&card)
}

fn ranks(cv: &mut Canvas<'_>, y: f32, rank: &Rank, now: f64) -> (f32, Option<String>) {
    let label = if rank.botting { "BOTTER RANK" } else { "WORLD RANK" };
    let y = gamewin::row(cv, y, label, &number(rank.overall_rank), HERO_H, 17.0);
    let note = format!("updated {}", format_ago(rank.last_checked.as_deref(), now));
    if rank.botting {
        // The API reuses overall_rank for the Botters list, so job/fame ranks would mislead.
        return (y, Some(format!("flagged as botting · {note}")));
    }
    let y = gamewin::row(cv, y, "JOB RANK", &number(rank.job_rank), gamewin::ROW_H, 14.0);
    let y = gamewin::row(cv, y, "FAME RANK", &number(rank.fame_rank), gamewin::ROW_H, 14.0);
    (y, Some(note))
}

fn notice(cv: &mut Canvas<'_>, y: f32, headline: &str, detail: &str) -> f32 {
    gamewin::value_box(cv, (gamewin::PAD, y, CARD_WIDTH - 2.0 * gamewin::PAD, 38.0));
    gamewin::centred(cv, CARD_WIDTH / 2.0, y + 12.0, headline, 13.0, gamewin::INK);
    gamewin::centred(cv, CARD_WIDTH / 2.0, y + 28.0, detail, 10.0, gamewin::INK_DIM);
    y + 38.0 + gamewin::ROW_GAP
}
