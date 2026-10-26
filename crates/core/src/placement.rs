//! Where a card goes relative to the game's tooltip or window (pure).

use crate::geometry::Rect;

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
