//! A monster's page in the panel: Back, its sprite and what it is (level, HP, EXP, spawns here), and every drop it is
//! known for, rarest first, credited to the page they came from.

use super::{Canvas, Hit, Icons, LIT, PanelState, SLOT, SLOT_GAP, credit, more, slot, sprite};
use crate::drops::MapDrops;
use crate::market::format::{format_mesos, group_thousands};
use crate::render::canvas::{Anchor, Ink};
use crate::render::font::Fonts;
use crate::render::glance::PAD;
use crate::render::map_card::fit_text;
use crate::render::theme;

const BACK: (f32, f32, f32, f32) = (PAD - 2.0, 7.0, 54.0, 18.0);
const BIG: f32 = 40.0;
const INFO_Y: f32 = 32.0;
const INFO_X: f32 = PAD + BIG + 8.0;
const LABEL_Y: f32 = INFO_Y + BIG + 9.0;
const GRID_Y: f32 = LABEL_Y + 14.0;
/// Under the grid, before the credit.
const GRID_END: f32 = 8.0;

fn columns() -> usize {
    ((super::CARD_WIDTH - 2.0 * PAD + SLOT_GAP) / (SLOT + SLOT_GAP)).floor() as usize
}

/// How many drops get a slot (the last counting the rest when they do not all fit), and the page's height.
fn layout(count: usize, credit: bool, height: Option<u32>) -> (usize, f32) {
    let credit_h = if credit { super::CREDIT_H } else { 0.0 };
    let rows_for = |n: usize| n.div_ceil(columns()).max(1);
    let tall = |rows: usize| GRID_Y + rows as f32 * (SLOT + SLOT_GAP) - SLOT_GAP + GRID_END + credit_h;
    let Some(height) = height else { return (count, tall(rows_for(count))) };
    let room = ((height as f32 - GRID_Y - GRID_END - credit_h + SLOT_GAP) / (SLOT + SLOT_GAP)).floor().max(1.0);
    let fits = room as usize * columns();
    let shown = if count > fits { fits - 1 } else { count };
    (shown, (height as f32).max(tall(rows_for(shown + usize::from(shown < count)))))
}

fn slot_at(index: usize) -> (f32, f32) {
    let (row, column) = (index / columns(), index % columns());
    (PAD + column as f32 * (SLOT + SLOT_GAP), GRID_Y + row as f32 * (SLOT + SLOT_GAP))
}

pub(super) fn draw<'f>(
    fonts: &'f Fonts,
    drops: &MapDrops,
    index: usize,
    icons: Icons<'_>,
    state: PanelState,
    factor: f32,
) -> Canvas<'f> {
    let mob = &drops.mobs[index];
    let (shown, height) = layout(mob.drops.len(), !mob.credit.is_empty(), state.height);
    let mut cv = Canvas::new(fonts, super::CARD_WIDTH, height, factor);
    cv.frame(height);
    back(&mut cv, state.hover == Some(Hit::Back));
    cv.chip(cv.width - PAD, 8.0, &format!("Lv {}", mob.level), Ink::new(10.0, theme::TEXT_DIM), true, 16.0);
    sprite(&mut cv, (PAD, INFO_Y, BIG), icons("mob", mob.id), false);
    let right = cv.width - PAD;
    let price = mob.best_price().map(|p| format_mesos(Some(p)));
    let price_w = price.as_ref().map_or(0.0, |p| cv.measure(p, 12.0, true) + 8.0);
    let name = fit_text(&cv, &mob.name, 13.0, true, right - price_w - INFO_X);
    cv.text(INFO_X, INFO_Y, &name, Ink::new(13.0, theme::TEXT).bold(), Anchor::LA);
    if let Some(price) = price {
        cv.text(right, INFO_Y + 1.0, &price, Ink::new(12.0, theme::GOLD).bold(), Anchor::RA);
    }
    let stats = format!("HP {} · EXP {}", group_thousands(mob.hp as i64), group_thousands(mob.exp as i64));
    cv.text(INFO_X, INFO_Y + 19.0, &stats, Ink::new(10.0, theme::TEXT_DIM), Anchor::LA);
    let spawns = format!("{} spawn point{} on this map", mob.count, if mob.count == 1 { "" } else { "s" });
    cv.text(INFO_X, INFO_Y + 32.0, &spawns, Ink::new(10.0, theme::TEXT_FAINT), Anchor::LA);
    let label = format!("ALL DROPS · {}", mob.drops.len());
    cv.text(PAD, LABEL_Y, &label, Ink::new(9.0, theme::TEXT_FAINT).bold(), Anchor::LA);
    for (i, drop) in mob.drops[..shown].iter().enumerate() {
        let (x, y) = slot_at(i);
        let lit = state.hover == Some(Hit::Slot((index, i)));
        slot(&mut cv, (x, y, SLOT), drop, icons("item", drop.item), lit);
    }
    if shown < mob.drops.len() {
        let (x, y) = slot_at(shown);
        more(&mut cv, x, y, mob.drops.len() - shown);
    }
    credit(&mut cv, height, &mob.credit);
    cv
}

/// The Back button: a chip with a left arrow, lit under the pointer.
fn back(cv: &mut Canvas<'_>, lit: bool) {
    let (x, y, w, h) = BACK;
    let (fill, edge) = if lit { (LIT, theme::TEXT) } else { (theme::CHIP_FILL, theme::CHIP_EDGE) };
    cv.rounded((x, y, w, h), 5.0, Some(fill), Some(edge));
    let (cx, cy) = (x + 10.0, y + h / 2.0);
    cv.polygon(&[(cx + 3.0, cy - 4.0), (cx + 3.0, cy + 4.0), (cx - 2.0, cy)], theme::TEXT);
    cv.text(x + 18.0, cy, "Back", Ink::new(10.0, theme::TEXT).bold(), Anchor::LM);
}

pub(super) fn hit(drops: &MapDrops, index: usize, state: PanelState, x: f32, y: f32) -> Option<Hit> {
    let (bx, by, bw, bh) = BACK;
    if x >= bx && x < bx + bw && y >= by && y < by + bh {
        return Some(Hit::Back);
    }
    let mob = &drops.mobs[index];
    let (shown, _) = layout(mob.drops.len(), !mob.credit.is_empty(), state.height);
    if x < PAD || y < GRID_Y {
        return None;
    }
    let (column, row) = (((x - PAD) / (SLOT + SLOT_GAP)) as usize, ((y - GRID_Y) / (SLOT + SLOT_GAP)) as usize);
    let (sx, sy) = slot_at(row * columns() + column);
    let inside = column < columns() && x < sx + SLOT && y < sy + SLOT;
    let drop = row * columns() + column;
    (inside && drop < shown).then_some(Hit::Slot((index, drop)))
}

pub(super) fn slot_top(drop: usize) -> f32 {
    slot_at(drop).1
}

#[cfg(test)]
pub(super) fn slot_centre(drop: usize) -> (f32, f32) {
    let (x, y) = slot_at(drop);
    (x + SLOT / 2.0, y + SLOT / 2.0)
}

#[cfg(test)]
pub(super) const BACK_CENTRE: (f32, f32) = (BACK.0 + BACK.2 / 2.0, BACK.1 + BACK.3 / 2.0);
