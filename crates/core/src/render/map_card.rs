//! The map card shown beside a world-map tooltip: who lives there, with their sprites.
//!
//! Every monster and NPC is listed. When one column would be taller than the game window, the lists flow into more
//! columns (the fewest that fit), so the card is as narrow as it can be without cutting anything off.

use std::sync::Arc;

use super::RgbaImage;
use super::canvas::{Anchor, Canvas, Ink};
use super::font::Fonts;
use super::glance::PAD;
use super::theme;
use crate::mapinfo::{MapInfo, Mob, Npc};
use crate::market::format::group_thousands;

/// Design width of one column of rows (1x; the card is drawn CARD_SCALE times bigger).
pub const COL_W: f32 = 266.0;
const COL_GAP: f32 = 18.0;
/// Square the sprite is fitted into.
const SLOT: f32 = 36.0;
const MOB_ROW_H: f32 = 42.0;
const NPC_ROW_H: f32 = 38.0;
/// Header block, a section's title row (with its gap), footer.
const HEADER_H: f32 = 76.0;
const SECTION_H: f32 = 36.0;
const FOOTER_H: f32 = 28.0;
const BAR: (f32, f32) = (58.0, 5.0);
const MAX_COLUMNS: usize = 6;
/// A card taller than this share of the window gets another column: tall slivers read badly.
const COMFORT: f32 = 0.75;
pub const LOADING_WIDTH: f32 = 290.0;
const LOADING_HEIGHT: f32 = 92.0;
/// One turn of the loading ring; the UI steps through them for the animation.
pub const SPINNER_FRAMES: usize = 12;

/// Sprite of a monster ("mob") or NPC ("npc"), when it is on disk.
pub type Icons<'a> = &'a dyn Fn(&str, u32) -> Option<Arc<RgbaImage>>;

fn rows(count: usize, cols: usize) -> usize {
    count.div_ceil(cols)
}

/// Design size of the card with `cols` columns.
fn size(info: &MapInfo, cols: usize) -> (f32, f32) {
    let mut height = HEADER_H + FOOTER_H;
    if !info.mobs.is_empty() {
        height += SECTION_H + rows(info.mobs.len(), cols) as f32 * MOB_ROW_H;
    }
    if !info.npcs.is_empty() {
        height += SECTION_H + rows(info.npcs.len(), cols) as f32 * NPC_ROW_H;
    }
    if info.mobs.is_empty() && info.npcs.is_empty() {
        height += 28.0;
    }
    (2.0 * PAD + cols as f32 * COL_W + (cols - 1) as f32 * COL_GAP, height)
}

/// Fewest columns that make a comfortably short card within `max_size` (design px). If even the widest allowed
/// layout is taller than that, it is still the widest one that fits across.
pub fn choose_columns(info: &MapInfo, max_size: (f32, f32)) -> usize {
    let most = info.mobs.len().max(info.npcs.len()).max(1);
    let mut best = 1;
    for cols in 1..=MAX_COLUMNS.min(most) {
        let (width, height) = size(info, cols);
        if width > max_size.0 {
            break;
        }
        best = cols;
        if height <= max_size.1 * COMFORT {
            break;
        }
    }
    best
}

/// The card for `info`; `max_size` (screen px) is the room the game window leaves for it.
pub fn render_map_card(
    fonts: &Fonts,
    info: &MapInfo,
    icons: Icons<'_>,
    ui_scale: f32,
    max_size: Option<(i32, i32)>,
) -> RgbaImage {
    let factor = theme::CARD_SCALE * ui_scale;
    let cols = max_size.map_or(1, |(w, h)| choose_columns(info, (w as f32 / factor, h as f32 / factor)));
    let (width, height) = size(info, cols);
    let mut cv = Canvas::new(fonts, width, height, factor);
    cv.frame(height);
    let mut y = header(&mut cv, 8.0, info);
    if !info.mobs.is_empty() {
        let note = format!("{} kinds", info.mobs.len());
        let spawns = info.spawns();
        y = section(&mut cv, y, ("MONSTERS", &note), &info.mobs, cols, MOB_ROW_H, |cv, x, row_y, mob| {
            mob_row(cv, x, row_y, mob, spawns, icons);
        });
    }
    if !info.npcs.is_empty() {
        let note = format!("{} here", info.npcs.len());
        y = section(&mut cv, y, ("NPCS", &note), &info.npcs, cols, NPC_ROW_H, |cv, x, row_y, npc| {
            npc_row(cv, x, row_y, npc, icons);
        });
    }
    if info.mobs.is_empty() && info.npcs.is_empty() {
        cv.text(PAD, y + 8.0, "No monsters or NPCs listed", Ink::new(12.0, theme::TEXT_DIM), Anchor::LA);
    }
    footer(&mut cv, height - FOOTER_H);
    RgbaImage::from_canvas(&cv)
}

/// A titled list that flows top to bottom through `cols` balanced columns; returns the y below it.
fn section<T>(
    cv: &mut Canvas<'_>,
    y: f32,
    (title, note): (&str, &str),
    items: &[T],
    cols: usize,
    row_h: f32,
    draw_row: impl Fn(&mut Canvas<'_>, f32, f32, &T),
) -> f32 {
    cv.divider(y, PAD);
    let faint = Ink::new(10.0, theme::TEXT_FAINT);
    cv.text(PAD, y + 12.0, title, faint.bold(), Anchor::LA);
    cv.text(cv.width - PAD, y + 12.0, note, faint, Anchor::RA);
    let top = y + SECTION_H - 8.0;
    let per_col = rows(items.len(), cols);
    for (i, item) in items.iter().enumerate() {
        let (col, row) = (i / per_col, i % per_col);
        draw_row(cv, PAD + col as f32 * (COL_W + COL_GAP), top + row as f32 * row_h, item);
    }
    top + per_col as f32 * row_h + 8.0
}

fn bullet(cv: &mut Canvas<'_>, x: f32, y: f32) {
    cv.circle(x + 3.5, y + 3.5, 3.5, theme::BULLET, None);
}

/// Title block; returns the y where the first section starts.
fn header(cv: &mut Canvas<'_>, y: f32, info: &MapInfo) -> f32 {
    bullet(cv, PAD, y + 8.0);
    cv.text(PAD + 13.0, y + 2.0, "Map", Ink::new(11.0, theme::TEXT_DIM).bold(), Anchor::LA);
    let chip = if info.is_town { Ink::new(10.0, theme::GOLD).bold() } else { Ink::new(10.0, theme::TEXT_DIM) };
    let label = if info.is_town { "Town" } else { info.street.as_str() };
    cv.chip(cv.width - PAD, y + 1.0, label, chip, true, 16.0);
    let name = fit_text(cv, &info.name, 17.0, true, cv.width - 2.0 * PAD);
    cv.text(PAD, y + 22.0, &name, Ink::new(17.0, theme::TEXT).bold(), Anchor::LA);
    let mut parts: Vec<String> = if info.is_town { vec![info.street.clone()] } else { Vec::new() };
    if let Some((low, high)) = info.levels() {
        parts.push(if low == high { format!("Lv {low}") } else { format!("Lv {low}–{high}") });
        parts.push(format!("{} spawn points", info.spawns()));
    }
    if !info.npcs.is_empty() {
        parts.push(format!("{} NPCs", info.npcs.len()));
    }
    cv.text(PAD, y + 46.0, &parts.join(" · "), Ink::new(11.0, theme::TEXT_DIM), Anchor::LA);
    HEADER_H
}

fn mob_row(cv: &mut Canvas<'_>, x: f32, y: f32, mob: &Mob, total: u32, icons: Icons<'_>) {
    sprite(cv, x, y, icons("mob", mob.id));
    let text_x = x + SLOT + 10.0;
    let right = x + COL_W;
    let name = fit_text(cv, &mob.name, 12.0, true, 120.0);
    let name_w = cv.text(text_x, y + 1.0, &name, Ink::new(12.0, theme::TEXT).bold(), Anchor::LA);
    cv.chip(text_x + name_w + 6.0, y, &format!("Lv {}", mob.level), Ink::new(10.0, theme::TEXT_DIM), false, 15.0);
    let stats = format!("EXP {} · HP {}", group_thousands(mob.exp as i64), group_thousands(mob.hp as i64));
    cv.text(text_x, y + 20.0, &stats, Ink::new(10.0, theme::TEXT_DIM), Anchor::LA);
    let share = if total > 0 { mob.count as f32 / total as f32 } else { 0.0 };
    cv.text(right, y, &format!("×{}", mob.count), Ink::new(14.0, theme::GOLD).bold(), Anchor::RA);
    let (bar_x, bar_y) = (right - BAR.0, y + 24.0);
    cv.rounded((bar_x, bar_y, BAR.0, BAR.1), 2.0, Some(theme::TRACK), None);
    let filled = (BAR.0 * share).round().max(3.0);
    cv.rounded((bar_x, bar_y, filled, BAR.1), 2.0, Some(theme::GOLD), None);
    let percent = format!("{:.0}%", share * 100.0);
    cv.text(bar_x - 5.0, bar_y + BAR.1 / 2.0, &percent, Ink::new(10.0, theme::TEXT_FAINT), Anchor::RM);
}

fn npc_row(cv: &mut Canvas<'_>, x: f32, y: f32, npc: &Npc, icons: Icons<'_>) {
    sprite(cv, x, y, icons("npc", npc.id));
    let (text_x, room) = (x + SLOT + 10.0, COL_W - SLOT - 10.0);
    let name = fit_text(cv, &npc.name, 12.0, true, room);
    cv.text(text_x, y + 2.0, &name, Ink::new(12.0, theme::TEXT).bold(), Anchor::LA);
    if !npc.role.is_empty() {
        let role = fit_text(cv, &npc.role, 10.0, false, room);
        cv.text(text_x, y + 20.0, &role, Ink::new(10.0, theme::TEXT_DIM), Anchor::LA);
    }
}

/// The sprite centred in a dark slot (an empty slot while the download is still running).
fn sprite(cv: &mut Canvas<'_>, x: f32, y: f32, image: Option<Arc<RgbaImage>>) {
    cv.rounded((x, y, SLOT, SLOT), 6.0, Some(theme::TRACK), Some(theme::CHIP_EDGE));
    let Some(image) = image.filter(|i| i.width > 0 && i.height > 0) else { return };
    let inner = SLOT - 6.0;
    // Never blow a small sprite up: it would blur.
    let fit = (inner / image.width as f32).min(inner / image.height as f32).min(1.0);
    let (w, h) = ((image.width as f32 * fit).round().max(1.0), (image.height as f32 * fit).round().max(1.0));
    let (left, top) = (x + ((SLOT - w) / 2.0).floor(), y + ((SLOT - h) / 2.0).floor());
    cv.image(left, top, w, h, (image.width, image.height, &image.data));
}

pub(super) fn fit_text(cv: &Canvas<'_>, text: &str, size: f32, bold: bool, width: f32) -> String {
    if cv.measure(text, size, bold) <= width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() && cv.measure(&format!("{}…", chars.iter().collect::<String>()), size, bold) > width {
        chars.pop();
    }
    format!("{}…", chars.iter().collect::<String>().trim_end())
}

fn footer(cv: &mut Canvas<'_>, y: f32) {
    cv.divider(y + 2.0, PAD);
    let faint = Ink::new(10.0, theme::TEXT_FAINT);
    cv.text(PAD, y + 9.0, "maplestory.io · classic-era data", faint, Anchor::LA);
    cv.text(cv.width - PAD, y + 9.0, "MapleLens", faint, Anchor::RA);
}

/// Placeholder while the map is looked up: a turning ring (`frame` picks the angle) under the name.
pub fn render_loading_card(fonts: &Fonts, name: &str, frame: usize, failed: bool, ui_scale: f32) -> RgbaImage {
    let mut cv = Canvas::new(fonts, LOADING_WIDTH, LOADING_HEIGHT, theme::CARD_SCALE * ui_scale);
    cv.frame(LOADING_HEIGHT);
    bullet(&mut cv, PAD, 12.0);
    cv.text(PAD + 13.0, 10.0, "Map", Ink::new(11.0, theme::TEXT_DIM).bold(), Anchor::LA);
    let name = fit_text(&cv, name, 15.0, true, LOADING_WIDTH - 2.0 * PAD);
    cv.text(PAD, 30.0, &name, Ink::new(15.0, theme::TEXT).bold(), Anchor::LA);
    if failed {
        cv.text(PAD, 60.0, "Can't reach maplestory.io, retrying…", Ink::new(11.0, theme::WARN), Anchor::LA);
    } else {
        spinner(&mut cv, PAD, 56.0, frame);
        let ink = Ink::new(11.0, theme::TEXT_DIM);
        cv.text(PAD + 26.0, 69.0, "Looking up monsters and NPCs…", ink, Anchor::LM);
    }
    RgbaImage::from_canvas(&cv)
}

/// A ring with a bright arc that advances one step (30 degrees) per frame, clockwise.
fn spinner(cv: &mut Canvas<'_>, x: f32, y: f32, frame: usize) {
    const SIZE: f32 = 20.0;
    let (cx, cy, r) = (x + SIZE / 2.0, y + SIZE / 2.0, SIZE / 2.0 - 3.5);
    let point = |degrees: f32| (cx + r * degrees.to_radians().cos(), cy + r * degrees.to_radians().sin());
    let ring: Vec<(f32, f32)> = (0..=48).map(|i| point(i as f32 * 7.5)).collect();
    cv.polyline(&ring, theme::TRACK, 3.0);
    let start = (frame % SPINNER_FRAMES) as f32 * 360.0 / SPINNER_FRAMES as f32;
    let arc: Vec<(f32, f32)> = (0..=20).map(|i| point(start + i as f32 * 5.0)).collect();
    cv.polyline(&arc, theme::GOLD, 3.0);
}
