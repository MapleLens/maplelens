"""Is an item tooltip showing at the spot where the game draws it?

The game always draws the tooltip's top-left at a fixed offset from the mouse (`config.TOOLTIP_OFFSET`),
so nothing is searched for. The tooltip fill is slate blue (about RGB 75,78,127); between the title and
the description of every tooltip there is a band of pure fill, which is what we look for.
"""

import numpy as np

_BLUE_OVER_RED = (35, 75)
_GREEN_OVER_RED_MAX = 12
_BLUE_RANGE = (90, 160)
_BAND_ROWS = slice(34, 42)
_BAND_COLS = slice(150, 300)
_BAND_MIN_FILL = 0.5   # low on purpose: fill blended over bright windows drifts out of the colour range


def tooltip_present(image: np.ndarray) -> bool:
    """`image` starts at the expected tooltip top-left corner."""
    band = image[_BAND_ROWS, _BAND_COLS]
    if band.size == 0:
        return False
    b, g, r = (band[..., i].astype(np.int16) for i in range(3))
    fill = (
        (b - r > _BLUE_OVER_RED[0]) & (b - r < _BLUE_OVER_RED[1])
        & (abs(g - r) < _GREEN_OVER_RED_MAX)
        & (b > _BLUE_RANGE[0]) & (b < _BLUE_RANGE[1])
    )
    return bool(fill.mean() >= _BAND_MIN_FILL)


# -- width ----------------------------------------------------------------------------------------------------
_LIFT_ROWS = slice(14, 118)    # below the title, above where the shortest tooltip ends
_LIFT_INSIDE_PX = 5            # compare each column with the one this far to its left (inside the tooltip)
_LIFT_MIN = 30                 # a border pixel is at least this much brighter than the fill next to it
_EDGE_WINDOW_PX = (-8, 6)      # where to look for the border around a candidate width
_MIN_EDGE_SCORE = 0.5          # share of rows with a bright column: the true border runs the full height
_MIN_MARGIN = 0.2              # the winning width must beat the other by this much, else we are not sure


def _edge_score(image: np.ndarray, width: int) -> float:
    """How strongly a vertical border line stands at about `width` px from the tooltip's left edge (0..1)."""
    gray = image[_LIFT_ROWS].min(axis=2).astype(np.int16)
    best = 0.0
    for x in range(width + _EDGE_WINDOW_PX[0], width + _EDGE_WINDOW_PX[1]):
        if x >= gray.shape[1] or x < _LIFT_INSIDE_PX:
            continue
        best = max(best, float(((gray[:, x] - gray[:, x - _LIFT_INSIDE_PX]) >= _LIFT_MIN).mean()))
    return best


def measure_width(image: np.ndarray, widths: tuple[int, ...]) -> int | None:
    """Which of the game's known tooltip `widths` this tooltip has, or None when it cannot be told.

    Tooltips are only ever a few fixed widths (narrow for equipment, wide for everything else). Finding the
    border anywhere on a busy backdrop is unreliable, but asking "is the border at 370 or at 450?" is not:
    the right one scores 0.55-1.0 on real captures, the wrong one at most 0.34.
    """
    scores = sorted(((_edge_score(image, w), w) for w in widths), reverse=True)
    (best, width), runner_up = scores[0], scores[1][0] if len(scores) > 1 else 0.0
    return width if best >= _MIN_EDGE_SCORE and best - runner_up >= _MIN_MARGIN else None
