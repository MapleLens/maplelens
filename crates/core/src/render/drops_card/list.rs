//! The panel's list: every monster of the map as a tile along the top (pressed, its page opens), a line
//! naming the one under the pointer, then the map's ten dearest drops in big slots, and the best one at the foot.

use super::{CREDIT_H, Canvas, Hit, Icons, LIT, PanelState, TOP_RIGHT, close, credit, slot, sprite};
use crate::drops::{MapDrops, Rarity, Shown};
use crate::market::format::format_mesos;
use crate::render::canvas::{Anchor, Ink};
use crate::render::font::Fonts;
use crate::render::glance::PAD;
use crate::render::map_card::fit_text;
use crate::render::theme;

const HEADER_H: f32 = 44.0;
const TILE: f32 = 34.0;
const TILE_GAP: f32 = 5.0;
/// Two rows of tiles at most: ten kinds of monster is more than any field has.
const MAX_TILE_ROWS: usize = 2;
/// The line naming the monster under the pointer, and the rare drops' label.
const INFO_H: f32 = 18.0;
const LABEL_H: f32 = 14.0;
const FOOTER_H: f32 = 26.0;
/// The drops worth a look: the map's ten dearest, five to a row in slots bigger than the page's.
const TOP: usize = 10;
const COLUMNS: usize = 5;
const BIG_SLOT: f32 = 30.0;
const BIG_GAP: f32 = 6.0;

fn tiles_per_row() -> usize {
    ((super::CARD_WIDTH - 2.0 * PAD + TILE_GAP) / (TILE + TILE_GAP)).floor() as usize
}

/// Where things go: the tiles shown, the grid's top, the drops given a slot (a short panel's last rows left out),
/// and the panel's height.
struct Layout {
    tiles: usize,
    grid_y: f32,
    shown: usize,
    height: f32,
}

fn layout(drops: &MapDrops, rare: usize, height: Option<u32>) -> Layout {
    let tiles = drops.mobs.len().min(tiles_per_row() * MAX_TILE_ROWS);
    let tile_rows = tiles.div_ceil(tiles_per_row()).max(1);
    let grid_y = HEADER_H + tile_rows as f32 * (TILE + TILE_GAP) - TILE_GAP + 4.0 + INFO_H + LABEL_H;
    let tail = tail(drops);
    let tall = |rows: usize| grid_y + rows as f32 * (BIG_SLOT + BIG_GAP) - BIG_GAP + 8.0 + tail;
    let Some(height) = height else {
        return Layout { tiles, grid_y, shown: rare, height: tall(rare.div_ceil(COLUMNS).max(1)) };
    };
    let room = ((height as f32 - grid_y - 8.0 - tail + BIG_GAP) / (BIG_SLOT + BIG_GAP)).floor().max(1.0) as usize;
    let shown = rare.min(room * COLUMNS);
    Layout { tiles, grid_y, shown, height: (height as f32).max(tall(shown.div_ceil(COLUMNS).max(1))) }
}

/// The best drop's line and the credit's.
fn tail(drops: &MapDrops) -> f32 {
    let best = if drops.best().is_some() { FOOTER_H } else { 0.0 };
    best + if drops.credit.is_empty() { 0.0 } else { CREDIT_H }
}

fn tile_at(index: usize) -> (f32, f32) {
    let (row, column) = (index / tiles_per_row(), index % tiles_per_row());
    (PAD + column as f32 * (TILE + TILE_GAP), HEADER_H + row as f32 * (TILE + TILE_GAP))
}

fn slot_at(grid_y: f32, index: usize) -> (f32, f32) {
    let (row, column) = (index / COLUMNS, index % COLUMNS);
    let width = COLUMNS as f32 * (BIG_SLOT + BIG_GAP) - BIG_GAP;
    let left = (super::CARD_WIDTH - width) / 2.0;
    (left + column as f32 * (BIG_SLOT + BIG_GAP), grid_y + row as f32 * (BIG_SLOT + BIG_GAP))
}

pub(super) fn draw<'f>(
    fonts: &'f Fonts,
    drops: &MapDrops,
    icons: Icons<'_>,
    state: PanelState,
    factor: f32,
) -> Canvas<'f> {
    let rare = drops.top(TOP);
    let at = layout(drops, rare.len(), state.height);
    let mut cv = Canvas::new(fonts, super::CARD_WIDTH, at.height, factor);
    cv.frame(at.height);
    header(&mut cv, drops);
    close(&mut cv, state.hover == Some(Hit::Close));
    for (i, mob) in drops.mobs[..at.tiles].iter().enumerate() {
        let (x, y) = tile_at(i);
        let pointed = state.hover == Some(Hit::Mob(i));
        if pointed {
            cv.rounded((x - 2.0, y - 2.0, TILE + 4.0, TILE + 4.0), 6.0, Some(LIT), None);
        }
        sprite(&mut cv, (x, y, TILE), icons("mob", mob.id), pointed);
        level_badge(&mut cv, x, y, mob.level);
    }
    let info_y = at.grid_y - LABEL_H - INFO_H;
    info_line(&mut cv, info_y, drops, state);
    let label = if rare.is_empty() { "TOP DROPS".to_string() } else { format!("TOP {} DROPS BY PRICE", at.shown) };
    cv.text(PAD, at.grid_y - LABEL_H, &label, Ink::new(9.0, theme::TEXT_FAINT).bold(), Anchor::LA);
    if rare.is_empty() {
        let none = Ink::new(10.0, theme::TEXT_FAINT);
        cv.text(PAD, at.grid_y + 4.0, "Only common drops here", none, Anchor::LA);
    }
    for (k, &(m, d)) in rare[..at.shown].iter().enumerate() {
        let (x, y) = slot_at(at.grid_y, k);
        let drop = &drops.mobs[m].drops[d];
        slot(&mut cv, (x, y, BIG_SLOT), drop, icons("item", drop.item), state.hover == Some(Hit::Slot((m, d))));
    }
    if let Some(best) = drops.best() {
        footer(&mut cv, at.height - tail(drops), best);
    }
    credit(&mut cv, at.height, &drops.credit);
    cv
}

fn header(cv: &mut Canvas<'_>, drops: &MapDrops) {
    cv.circle(PAD + 3.5, 15.5, 3.5, theme::BULLET, None);
    cv.text(PAD + 13.0, 10.0, "DROPS", Ink::new(11.0, theme::TEXT_DIM).bold(), Anchor::LA);
    if let Some((low, high)) = drops.levels {
        let levels = if low == high { format!("Lv {low}") } else { format!("Lv {low}–{high}") };
        cv.chip(TOP_RIGHT, 8.0, &levels, Ink::new(10.0, theme::TEXT_DIM), true, 16.0);
    }
    let name = fit_text(cv, &drops.map, 12.0, true, cv.width - 2.0 * PAD);
    cv.text(PAD, 27.0, &name, Ink::new(12.0, theme::TEXT).bold(), Anchor::LA);
}

/// The monster's level on its tile, in the corner, as the game marks levels.
fn level_badge(cv: &mut Canvas<'_>, x: f32, y: f32, level: u32) {
    let text = level.to_string();
    let w = cv.measure(&text, 8.0, true) + 6.0;
    let (bx, by) = (x + TILE - w - 1.0, y + TILE - 11.0);
    cv.rounded((bx, by, w, 10.0), 3.0, Some([16, 18, 48, 230]), None);
    cv.text(bx + 3.0, by + 5.0, &text, Ink::new(8.0, theme::GOLD).bold(), Anchor::LM);
}

/// The monster under the pointer (name, level, spawns), else what pressing a tile does.
fn info_line(cv: &mut Canvas<'_>, y: f32, drops: &MapDrops, state: PanelState) {
    let room = cv.width - 2.0 * PAD;
    match state.hover.and_then(|h| match h {
        Hit::Mob(i) => drops.mobs.get(i),
        _ => None,
    }) {
        Some(mob) => {
            let name = fit_text(cv, &mob.name, 11.0, true, room * 0.6);
            let width = cv.text(PAD, y + 2.0, &name, Ink::new(11.0, theme::TEXT).bold(), Anchor::LA);
            let rest = format!("Lv {} · ×{} · all drops", mob.level, mob.count);
            let rest = fit_text(cv, &rest, 10.0, false, room - width - 6.0);
            cv.text(PAD + width + 6.0, y + 3.0, &rest, Ink::new(10.0, theme::TEXT_DIM), Anchor::LA);
        }
        None => {
            let hint = Ink::new(10.0, theme::TEXT_FAINT);
            cv.text(PAD, y + 3.0, "Click a monster for all its drops", hint, Anchor::LA);
        }
    }
}

fn footer(cv: &mut Canvas<'_>, y: f32, best: &Shown) {
    cv.divider(y, PAD);
    let y = y + 8.0;
    let label = Ink::new(10.0, theme::TEXT_FAINT);
    let label_w = cv.text(PAD, y, "Best drop", label, Anchor::LA) + 6.0;
    let right = cv.width - PAD;
    let price = format_mesos(best.price);
    let price_w = cv.text(right, y - 1.0, &price, Ink::new(11.0, theme::GOLD).bold(), Anchor::RA);
    let ink = if best.rarity == Rarity::Common { theme::TEXT } else { super::colour(best.rarity) };
    let name = fit_text(cv, &super::short_name(&best.name), 10.0, true, right - price_w - 6.0 - PAD - label_w);
    cv.text(PAD + label_w, y, &name, Ink::new(10.0, ink).bold(), Anchor::LA);
}

/// A monster's tile, or a rare drop's slot.
pub(super) fn hit(drops: &MapDrops, state: PanelState, x: f32, y: f32) -> Option<Hit> {
    let rare = drops.top(TOP);
    let at = layout(drops, rare.len(), state.height);
    let inside = |(sx, sy): (f32, f32), size: f32| x >= sx && x < sx + size && y >= sy && y < sy + size;
    if let Some(i) = (0..at.tiles).find(|&i| inside(tile_at(i), TILE)) {
        return Some(Hit::Mob(i));
    }
    (0..at.shown).find(|&k| inside(slot_at(at.grid_y, k), BIG_SLOT)).map(|k| Hit::Slot(rare[k]))
}

pub(super) fn slot_top(drops: &MapDrops, slot: (usize, usize)) -> f32 {
    let rare = drops.top(TOP);
    let at = layout(drops, rare.len(), None);
    slot_at(at.grid_y, rare.iter().position(|&s| s == slot).unwrap_or(0)).1
}

#[cfg(test)]
pub(super) fn tile_centre(index: usize) -> (f32, f32) {
    let (x, y) = tile_at(index);
    (x + TILE / 2.0, y + TILE / 2.0)
}

#[cfg(test)]
pub(super) fn slot_centre(drops: &MapDrops, index: usize) -> (f32, f32) {
    let at = layout(drops, drops.top(TOP).len(), None);
    let (x, y) = slot_at(at.grid_y, index);
    (x + BIG_SLOT / 2.0, y + BIG_SLOT / 2.0)
}
