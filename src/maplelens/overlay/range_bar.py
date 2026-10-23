"""The price range bar: where the typical price sits among every sale seen, and where shops list it now.

    ▾ in shops 118                    <- median shop listing: blue (shops, not sales); amber when well above
                                         what it actually sells for
    ━━━━━━━|███━━━━━━━━━━━━━━━━━━     <- groove = lowest..highest sale, gold = middle half, white = typical
    40   usually 99 – 119       900   <- the band's label sits under the band, not mid-card

Everything is on the same log scale (`insights.range_position`), so the marks can be compared by eye.
"""

from maplelens.market import insights
from maplelens.market.format import format_mesos
from maplelens.market.models import Item
from maplelens.overlay import theme
from maplelens.overlay.canvas import Canvas
from maplelens.overlay.theme import RGBA

PAD = 12
_BAR_H = 8
_ASK_ROW = 14       # room above the bar for the listing marker
_LABEL_GAP = 16     # keeps the band label clear of the low/high numbers at the ends
_SIZE = 10


def range_bar(cv: Canvas, y: int, item: Item) -> int:
    """Draw the bar block at `y` and return the y below it (unchanged when the item has no range)."""
    def at(value: float | None) -> float | None:
        return insights.range_position(value, item.low, item.high)

    mid = at(item.price)
    if mid is None:
        return y
    listing = at(item.ask_median)
    width = cv.width - 2 * PAD
    top = y + 2 + (_ASK_ROW if listing is not None else 0)
    band = (at(item.p25), at(item.p75))
    _track(cv, top, width, band)
    tick = PAD + mid * width
    cv.draw.rectangle((tick - 1, top - 3, tick + 1, top + _BAR_H + 2), fill=theme.TEXT)
    if listing is not None:
        _listing_marker(cv, PAD + listing * width, top, item.ask_median, _listing_colour(item))
    _scale_labels(cv, top + _BAR_H + 6, width, item, band)
    return top + 34


def _track(cv: Canvas, top: int, width: int, band: tuple[float | None, float | None]) -> None:
    lo, hi = band

    def draw(d, s):
        d.rounded_rectangle((0, 0, width * s - 1, _BAR_H * s - 1), radius=4 * s, fill=theme.TRACK)
        if lo is not None and hi is not None:
            # a minimum width so a tight band still reads as a band, not a second tick
            d.rounded_rectangle((lo * width * s, 0, max(hi * width * s, lo * width * s + 5 * s), _BAR_H * s - 1),
                                radius=4 * s, fill=theme.GOLD_SOFT[:3] + (200,))

    cv.overlay((PAD, top, width, _BAR_H), draw, scale=3)


def _listing_colour(item: Item) -> RGBA:
    premium = insights.ask_premium(item)
    return theme.WARN if premium is not None and premium >= insights.ASK_PREMIUM_NOTABLE else theme.BULLET


def _listing_marker(cv: Canvas, x: float, top: int, ask: float, colour: RGBA) -> None:
    """Down-arrow pointing into the bar at `x`, labelled on whichever side has room."""
    cv.arrow(round(x - 3.5), top - 11, False, colour, 7)
    label = f"in shops {format_mesos(ask)}"
    if x + 8 + cv.measure(label, _SIZE) <= cv.width - PAD:
        cv.text(x + 8, top - 15, label, _SIZE, colour)
    else:
        cv.text(x - 8, top - 15, label, _SIZE, colour, anchor="ra")


def _scale_labels(cv: Canvas, y: int, width: int, item: Item, band: tuple[float | None, float | None]) -> None:
    """Lowest and highest sale at the ends; "usually p25 – p75" centred under the gold band where it fits."""
    low, high = format_mesos(item.low), format_mesos(item.high)
    cv.text(PAD, y, low, _SIZE, theme.TEXT_FAINT)
    cv.text(cv.width - PAD, y, high, _SIZE, theme.TEXT_FAINT, anchor="ra")
    lo, hi = band
    if not (item.p25 and item.p75) or lo is None or hi is None:
        return
    text = f"usually {format_mesos(item.p25)} – {format_mesos(item.p75)}"
    half = cv.measure(text, _SIZE) / 2
    left = PAD + cv.measure(low, _SIZE) + _LABEL_GAP + half
    right = cv.width - PAD - cv.measure(high, _SIZE) - _LABEL_GAP - half
    centre = PAD + (lo + hi) / 2 * width
    cv.text(min(max(centre, left), right), y, text, _SIZE, theme.TEXT, anchor="ma")
