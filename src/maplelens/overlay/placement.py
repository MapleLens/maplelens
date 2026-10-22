"""Where the card goes relative to the game's tooltip (pure)."""

from maplelens.geometry import Box


def place_card(tooltip: Box, card: tuple[int, int], gap: int, bounds: Box, *, compare: bool) -> tuple[int, int]:
    """Top-left of a card of size `card` (w, h) for the tooltip at `tooltip`, inside `bounds`.

    Preference: right of the tooltip, else left of it, else under it. With an equipment compare (two
    tooltips side by side) the sides are taken, so it always goes under.
    """
    width, height = card
    if not compare:
        right = tooltip.right + gap
        if right + width <= bounds.right:
            return right, _clamp_y(tooltip.y, height, bounds)
        left = tooltip.x - gap - width
        if left >= bounds.x:
            return left, _clamp_y(tooltip.y, height, bounds)
    below = tooltip.bottom + gap
    if below + height > bounds.bottom:   # no room under it either: sit above
        below = tooltip.y - gap - height
    x = max(bounds.x, min(tooltip.x, bounds.right - width))
    return x, _clamp_y(below, height, bounds)


def _clamp_y(y: int, height: int, bounds: Box) -> int:
    return max(bounds.y, min(y, bounds.bottom - height))


def place_beside_window(window: Box, card: tuple[int, int], gap: int, bounds: Box) -> tuple[int, int] | None:
    """Top-left of a card to the left of a game window (else to its right), or None if neither side fits.

    Unlike the tooltip card this never slides over the window itself: covering the game's own UI is worse
    than showing nothing.
    """
    width, height = card
    for x in (window.x - gap - width, window.right + gap):
        if bounds.x <= x and x + width <= bounds.right:
            return x, _clamp_y(window.y, height, bounds)
    return None
