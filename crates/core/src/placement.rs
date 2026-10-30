//! Where a card goes relative to the game's tooltip or window (pure).

use crate::geometry::Rect;
use crate::settings::Corner;

/// Top-left of a card of size `card` (w, h) for the tooltip at `tooltip`, inside `bounds`.
///
/// Preference: right of the tooltip, else left of it, else under it. With an equipment compare (two
/// tooltips side by side) the sides are taken, so it always goes under.
pub fn place_card(tooltip: Rect, card: (i32, i32), gap: i32, bounds: Rect, compare: bool) -> (i32, i32) {
    let (width, height) = card;
    if !compare {
        let right = tooltip.right() + gap;
        if right + width <= bounds.right() {
            return (right, clamp_y(tooltip.y, height, bounds));
        }
        let left = tooltip.x - gap - width;
        if left >= bounds.x {
            return (left, clamp_y(tooltip.y, height, bounds));
        }
    }
    let mut below = tooltip.bottom() + gap;
    if below + height > bounds.bottom() {
        below = tooltip.y - gap - height; // no room under it either: sit above
    }
    let x = bounds.x.max(tooltip.x.min(bounds.right() - width));
    (x, clamp_y(below, height, bounds))
}

fn clamp_y(y: i32, height: i32, bounds: Rect) -> i32 {
    bounds.y.max(y.min(bounds.bottom() - height))
}

/// Top-left of a card to the left of a game window (else to its right), or None if neither side fits.
///
/// Unlike the tooltip card this never slides over the window itself: covering the game's own UI is worse
/// than showing nothing.
pub fn place_beside_window(window: Rect, card: (i32, i32), gap: i32, bounds: Rect) -> Option<(i32, i32)> {
    let (width, height) = card;
    [window.x - gap - width, window.right() + gap]
        .into_iter()
        .find(|&x| bounds.x <= x && x + width <= bounds.right())
        .map(|x| (x, clamp_y(window.y, height, bounds)))
}

/// Does a card at (x, y) of size `card` lie fully inside `bounds`? (Overlays only ever show over the game.)
pub fn fits_inside(x: i32, y: i32, card: (i32, i32), bounds: Rect) -> bool {
    x >= bounds.x && y >= bounds.y && x + card.0 <= bounds.right() && y + card.1 <= bounds.bottom()
}

/// Top-left of a card in `corner` of `client`, `margin` in from its edges, or None when it does not fit.
///
/// It keeps clear of the game's own HUD in that corner: top left it goes right of the minimap (`minimap_right`,
/// from the client's left edge, when the minimap is on screen), and at the bottom a card that would cover the
/// status bar (`status_bar`, centred, narrower than the client) sits above it; one that fits beside the bar sits level
/// with it, its foot on the bar's (the EXP strip, as tall as the bar, then lines up with it).
pub fn place_in_corner(
    client: Rect,
    card: (i32, i32),
    corner: Corner,
    margin: i32,
    status_bar: Option<Rect>,
    minimap_right: Option<i32>,
) -> Option<(i32, i32)> {
    let (width, height) = card;
    let mut x = if corner.is_left() { client.x + margin } else { client.right() - margin - width };
    if corner == Corner::TopLeft
        && let Some(right) = minimap_right
    {
        x = x.max(client.x + right + margin);
    }
    let mut y = if corner.is_top() { client.y + margin } else { client.bottom() - margin - height };
    if !corner.is_top()
        && let Some(bar) = status_bar
    {
        y = if x < bar.right() && x + width > bar.x { y.min(bar.y - margin - height) } else { bar.bottom() - height };
    }
    fits_inside(x, y, card, client).then_some((x, y))
}

/// Top-left of the EXP strip in a bottom `corner`, flush with the client's side and foot, so it fills the room beside
/// the status bar (`bar`); None in a top corner or when it would reach the bar.
pub fn place_beside_bar(client: Rect, card: (i32, i32), corner: Corner, bar: Rect) -> Option<(i32, i32)> {
    let (width, height) = card;
    if corner.is_top() {
        return None;
    }
    let x = if corner.is_left() { client.x } else { client.right() - width };
    let clear = if corner.is_left() { x + width <= bar.x } else { x >= bar.right() };
    let at = (x, client.bottom() - height);
    (clear && fits_inside(at.0, at.1, card, client)).then_some(at)
}

/// Top-left of a card opened from a panel in `corner` (a drop's price card): on the panel's side towards the middle of
/// the client, else the other side, its top level with `top` (the hovered row) as far as `client` allows.
pub fn place_from_panel(
    panel: Rect,
    top: i32,
    card: (i32, i32),
    corner: Corner,
    gap: i32,
    client: Rect,
) -> Option<(i32, i32)> {
    let (width, height) = card;
    let (inward, outward) = (panel.right() + gap, panel.x - gap - width);
    let sides = if corner.is_left() { [inward, outward] } else { [outward, inward] };
    let y = clamp_y(top, height, client);
    sides.into_iter().map(|x| (x, y)).find(|&(x, y)| fits_inside(x, y, card, client))
}

/// Top-left of a panel that extends a card in `corner` (the map drops beside the EXP card): on the card's side
/// towards the middle of the client (else the other side), `gap` from it, level with its top in a top corner and its
/// bottom in a bottom one, so it grows away from the corner as the card does. None when neither side fits: the card
/// is never moved to make room.
pub fn place_beside_card(card: Rect, panel: (i32, i32), corner: Corner, gap: i32, client: Rect) -> Option<(i32, i32)> {
    let (width, height) = panel;
    let (right, left) = (card.right() + gap, card.x - gap - width);
    let sides = if corner.is_left() { [right, left] } else { [left, right] };
    let y = if corner.is_top() { card.y } else { card.bottom() - height };
    sides.into_iter().map(|x| (x, y)).find(|&(x, y)| fits_inside(x, y, panel, client))
}

/// Top-left of a panel opened from a wide card in `corner` (the map drops from the EXP strip): above the card in a
/// bottom corner and below it in a top one, `gap` from it, lined up with the card's outer side; else beside it
/// (`place_beside_card`). None when nothing fits: the card is never moved to make room.
pub fn place_off_card(card: Rect, panel: (i32, i32), corner: Corner, gap: i32, client: Rect) -> Option<(i32, i32)> {
    let (width, height) = panel;
    let x = if corner.is_left() { card.x } else { card.right() - width };
    let y = if corner.is_top() { card.bottom() + gap } else { card.y - gap - height };
    let x = x.clamp(client.x, (client.right() - width).max(client.x));
    if fits_inside(x, y, panel, client) {
        return Some((x, y));
    }
    place_beside_card(card, panel, corner, gap, client)
}

/// The corner a card at `card` is nearest (its centre's quarter of `client`): which way a panel beside it grows.
pub fn nearest_corner(card: Rect, client: Rect) -> Corner {
    let (cx, cy) = (card.x + card.w / 2, card.y + card.h / 2);
    Corner::of(cy < client.y + client.h / 2, cx < client.x + client.w / 2)
}

/// A card's top-left at (x, y) as fractions of `client`: where it was dragged, kept when the window is resized.
pub fn fraction_of(client: Rect, (x, y): (i32, i32)) -> (f32, f32) {
    ((x - client.x) as f32 / client.w.max(1) as f32, (y - client.y) as f32 / client.h.max(1) as f32)
}

/// Top-left of a card kept at `fraction` of `client`, moved back inside it where it would stick out; None when the
/// card is bigger than the client.
pub fn place_at(client: Rect, card: (i32, i32), (fx, fy): (f32, f32)) -> Option<(i32, i32)> {
    let (width, height) = card;
    if width > client.w || height > client.h {
        return None;
    }
    let x = (client.x + (fx * client.w as f32).round() as i32).clamp(client.x, client.right() - width);
    let y = (client.y + (fy * client.h as f32).round() as i32).clamp(client.y, client.bottom() - height);
    Some((x, y))
}
