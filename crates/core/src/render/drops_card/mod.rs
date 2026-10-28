//! The map drops panel: beside the EXP card (as tall as it), each monster of the map with its sprite and its worthy
//! drops as item slots framed in their rarity's colour (`list`); a monster pressed opens its page there, with every
//! drop and a Back button (`page`). Drawn like the game's tooltips, as the other cards are. `hit` says what is under a
//! point, for the pointer's hover and presses.

mod list;
mod page;

use std::sync::Arc;

use super::RgbaImage;
use super::canvas::{Anchor, Canvas, Ink, rounded_rect};
use super::font::Fonts;
use super::glance::PAD;
use super::map_card::{Icons, fit_text};
use super::theme::{self, Rgba};
use crate::drops::{MapDrops, Rarity, Shown};

/// Design width (1x; drawn CARD_SCALE times bigger, like the other cards).
pub const CARD_WIDTH: f32 = 224.0;
const SLOT: f32 = 20.0;
const SLOT_GAP: f32 = 3.0;
/// The icon inside a slot, this much smaller than it.
const ICON_INSET: f32 = 4.0;
/// The source's credit, faint, at the very foot.
const CREDIT_H: f32 = 15.0;
/// Very rare purple: the game has no such colour, so one between the rare blue and the legendary orange.
const VERY_RARE: Rgba = [192, 140, 255, 255];
/// A slot is tinted with its rarity's colour too: a frame alone is a hairline at Ideal Ratio.
const SLOT_TINT: u8 = 55;
const SLOT_EDGE_W: f32 = 1.5;
/// Under the pointer: a row or button lit this much.
const LIT: Rgba = [255, 255, 255, 22];

/// A drop: (monster, drop), indices into `MapDrops`.
pub type Slot = (usize, usize);

/// What a point of the panel is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hit {
    /// A drop's slot: its price card opens.
    Slot(Slot),
    /// A monster's row in the list: pressed, its page opens.
    Mob(usize),
    /// The page's Back button.
    Back,
}

/// How the panel is drawn besides its content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PanelState {
    /// The monster whose page is open (an index into `MapDrops::mobs`); None for the list.
    pub page: Option<usize>,
    /// What the pointer is over, lit up.
    pub hover: Option<Hit>,
    /// The height to take in design units (the EXP card's), else as tall as the content. Content that does not fit
    /// is left out (the list's last monsters, the page's last drops counted in a "+N" slot).
    pub height: Option<u32>,
}

/// The colour of a rarity: the slots' frames and the best drop's name.
pub fn colour(rarity: Rarity) -> Rgba {
    match rarity {
        Rarity::Common => theme::TEXT_FAINT,
        Rarity::Uncommon => theme::UP,
        Rarity::Rare => theme::BULLET,
        Rarity::VeryRare => VERY_RARE,
        Rarity::Legendary => theme::ORANGE,
    }
}

/// The panel for `drops`; `icons` gives monster ("mob") and item ("item") pictures once they are on disk.
pub fn render_drops_card(
    fonts: &Fonts,
    drops: &MapDrops,
    icons: Icons<'_>,
    state: PanelState,
    ui_scale: f32,
) -> RgbaImage {
    let factor = theme::CARD_SCALE * ui_scale;
    let canvas = match open_page(drops, state) {
        Some(mob) => page::draw(fonts, drops, mob, icons, state, factor),
        None => list::draw(fonts, drops, icons, state, factor),
    };
    RgbaImage::from_canvas(&canvas)
}

/// What is at (x, y) in the panel's design units (pixels from its corner / CARD_SCALE / UI scale).
pub fn hit(drops: &MapDrops, state: PanelState, x: f32, y: f32) -> Option<Hit> {
    match open_page(drops, state) {
        Some(mob) => page::hit(drops, mob, state, x, y),
        None => list::hit(drops, state, x, y),
    }
}

/// The top of `slot`, in design units: its price card opens level with it.
pub fn slot_top(drops: &MapDrops, state: PanelState, slot: Slot) -> f32 {
    match open_page(drops, state) {
        Some(_) => page::slot_top(slot.1),
        None => list::slot_top(drops, slot),
    }
}

fn open_page(drops: &MapDrops, state: PanelState) -> Option<usize> {
    state.page.filter(|&i| i < drops.mobs.len())
}

/// A monster's sprite centred in a dark square (empty while the download is still running), its edge lit when the
/// pointer is on its row.
fn sprite(cv: &mut Canvas<'_>, (x, y, size): (f32, f32, f32), image: Option<Arc<RgbaImage>>, lit: bool) {
    let edge = if lit { theme::TEXT } else { theme::CHIP_EDGE };
    cv.rounded((x, y, size, size), 5.0, Some(theme::TRACK), Some(edge));
    let Some(image) = image.filter(|i| i.width > 0 && i.height > 0) else { return };
    let inner = size - 4.0;
    let fit = (inner / image.width as f32).min(inner / image.height as f32).min(1.0);
    let (w, h) = ((image.width as f32 * fit).round().max(1.0), (image.height as f32 * fit).round().max(1.0));
    let (left, top) = (x + ((size - w) / 2.0).floor(), y + ((size - h) / 2.0).floor());
    cv.image(left, top, w, h, (image.width, image.height, &image.data));
}

/// An inventory-like slot framed in the drop's rarity colour, a second ring for a legendary one, a white one under
/// the pointer; a diamond of that colour stands in for the icon until it is on disk.
fn slot(cv: &mut Canvas<'_>, (x, y, size): (f32, f32, f32), drop: &Shown, image: Option<Arc<RgbaImage>>, lit: bool) {
    let edge = colour(drop.rarity);
    if drop.rarity == Rarity::Legendary {
        cv.rounded((x - 1.5, y - 1.5, size + 3.0, size + 3.0), 4.5, None, Some(theme::with_alpha(edge, 150)));
    }
    cv.rounded((x, y, size, size), 3.0, Some(theme::TRACK), None);
    cv.rounded((x, y, size, size), 3.0, Some(theme::with_alpha(edge, SLOT_TINT)), None);
    let inset = SLOT_EDGE_W / 2.0;
    if let Some(path) = rounded_rect(x + inset, y + inset, size - SLOT_EDGE_W, size - SLOT_EDGE_W, 3.0 - inset) {
        cv.stroke_path(&path, edge, SLOT_EDGE_W);
    }
    let (cx, cy) = (x + size / 2.0, y + size / 2.0);
    match image.filter(|i| i.width > 0 && i.height > 0) {
        Some(image) => {
            let icon = size - ICON_INSET;
            let fit = (icon / image.width as f32).min(icon / image.height as f32);
            let (w, h) = (image.width as f32 * fit, image.height as f32 * fit);
            cv.image(cx - w / 2.0, cy - h / 2.0, w, h, (image.width, image.height, &image.data));
        }
        None => cv.polygon(&[(cx, cy - 4.0), (cx + 4.0, cy), (cx, cy + 4.0), (cx - 4.0, cy)], edge),
    }
    if lit {
        cv.rounded((x - 2.5, y - 2.5, size + 5.0, size + 5.0), 5.0, None, Some(theme::TEXT));
    }
}

/// A slot counting the drops left out.
fn more(cv: &mut Canvas<'_>, x: f32, y: f32, count: usize) {
    cv.rounded((x, y, SLOT, SLOT), 3.0, Some(theme::CHIP_FILL), Some(theme::CHIP_EDGE));
    let (text, ink) = (format!("+{count}"), Ink::new(9.0, theme::TEXT_DIM).bold());
    let left = x + (SLOT - cv.measure(&text, ink.size, true)) / 2.0;
    cv.text(left, y + SLOT / 2.0, &text, ink, Anchor::LM);
}

/// The source's credit along the foot.
fn credit(cv: &mut Canvas<'_>, height: f32, text: &str) {
    if text.is_empty() {
        return;
    }
    // The page's address must show whole: the label goes first when both do not fit.
    let (labelled, room) = (format!("Drops: {text}"), cv.width - 2.0 * PAD);
    let line = if cv.measure(&labelled, 9.0, false) <= room { labelled } else { fit_text(cv, text, 9.0, false, room) };
    cv.text(PAD, height - CREDIT_H + 1.0, &line, Ink::new(9.0, theme::TEXT_FAINT), Anchor::LA);
}

/// "Scroll for Gloves for ATT 60%" -> "Gloves ATT 60%", as players write it: the full name never fits the foot.
pub fn short_name(name: &str) -> String {
    match name.strip_prefix("Scroll for ") {
        Some(rest) => rest.replacen(" for ", " ", 1),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests;
