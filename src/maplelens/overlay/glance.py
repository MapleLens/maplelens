"""The glance card: what matters in the second you hover an item. Each part takes (canvas, y, item) and returns
the y below itself.

Reading order, top to bottom: which item/category -> the price (big) and where it is heading -> how sure the
price is and where it sits in its range -> quick signals (how fast it sells, how many sales, how recent).
"""

from pathlib import Path

from PIL import Image

from maplelens.market import insights
from maplelens.market.format import format_ago, format_mesos
from maplelens.market.models import Item
from maplelens.overlay import theme
from maplelens.overlay.canvas import Canvas

PAD = 12
_COIN = Image.open(Path(__file__).resolve().parent.parent / "assets" / "meso.png")
_SPARK_W, _SPARK_H = 84, 30
_ASK_ROW = 14   # room above the range bar for the "asking N" marker


def header(cv: Canvas, y: int, item: Item | None) -> int:
    cv.overlay((PAD, y + 6, 7, 7), lambda d, s: d.ellipse((0, 0, 7 * s - 1, 7 * s - 1), fill=theme.BULLET), scale=4)
    cv.text(PAD + 13, y + 2, "MesoWatch", 11, theme.TEXT_DIM, bold=True)
    if item is None:
        return y + 26
    right = cv.width - PAD
    if item.low_data:
        right -= cv.chip(right, y, "LOW DATA", theme.WARN, bold=True, right_align=True) + 4
    if item.category:
        cv.chip(right, y, item.category, right_align=True)
    return y + 26


def hero(cv: Canvas, y: int, item: Item) -> int:
    """The price, large, with its trend pill and a sparkline of the week."""
    text = ("~" if item.low_data else "") + format_mesos(item.price)   # "~": few sales, treat as rough
    cv.text(PAD, y, text, 30, theme.GOLD, bold=True)
    x = PAD + cv.measure(text, 30, True) + 6
    cv.image.alpha_composite(_COIN, (x, y + 10))
    x += _COIN.width + 10
    if item.unit_basis == "set":
        cv.text(x, y + 22, "per set", 10, theme.TEXT_DIM, anchor="lm")
        x += cv.measure("per set", 10) + 8
    if item.trend is not None and abs(item.trend) >= 0.005:
        colour = theme.UP if item.trend > 0 else theme.DOWN
        cv.pill(x, y + 12, f"{abs(item.trend) * 100:.1f}%", colour, arrow=item.trend > 0)
    _sparkline(cv, cv.width - PAD - _SPARK_W, y + 4, item)
    basis = "median of the last 24 h" if item.price_basis == "24h" else "median of the last 10 sales"
    cv.text(PAD, y + 40, basis, 11, theme.TEXT_DIM)
    return y + 62


def _sparkline(cv: Canvas, x: int, y: int, item: Item) -> None:
    values = item.medians
    if len(values) < 2:
        return
    low, span = min(values), (max(values) - min(values)) or 1

    def draw(d, s):
        pts = [((_SPARK_W - 8) * i / (len(values) - 1) * s + 4 * s,
                (_SPARK_H - 8 - (_SPARK_H - 16) * (v - low) / span) * s + 4 * s) for i, v in enumerate(values)]
        d.polygon([*pts, (pts[-1][0], _SPARK_H * s), (pts[0][0], _SPARK_H * s)], fill=theme.GOLD_SOFT[:3] + (40,))
        d.line(pts, fill=theme.GOLD, width=2 * s, joint="curve")
        ex, ey = pts[-1]
        d.ellipse((ex - 3.5 * s, ey - 3.5 * s, ex + 3.5 * s, ey + 3.5 * s), fill=theme.GOLD, outline=theme.TEXT,
                  width=s)

    cv.overlay((x, y, _SPARK_W, _SPARK_H), draw, scale=3)


def _ask_marker(cv: Canvas, x: float, top: int, ask: float) -> None:
    """Down-arrow over the bar at `x` with "asking N" beside it, on the side that has room."""
    cv.arrow(round(x - 3.5), top - 11, False, theme.TEXT_DIM, 7)
    label = f"asking {format_mesos(ask)}"
    if x + 8 + cv.measure(label, 10) <= cv.width - PAD:
        cv.text(x + 8, top - 15, label, 10, theme.TEXT_DIM)
    else:
        cv.text(x - 8, top - 15, label, 10, theme.TEXT_DIM, anchor="ra")


def range_bar(cv: Canvas, y: int, item: Item) -> int:
    """Where the typical price sits among everything seen: dark groove = lowest..highest sale, gold = the
    middle half of sales, white tick = the typical price. A labelled arrow above marks the median shop listing."""
    position = insights.range_position
    mid = position(item.price, item.low, item.high)
    if mid is None:
        return y
    listing = position(item.ask_median, item.low, item.high)
    width, top = cv.width - 2 * PAD, y + (_ASK_ROW if listing is not None else 0) + 2
    lo, hi = position(item.p25, item.low, item.high), position(item.p75, item.low, item.high)

    def bar(d, s):
        d.rounded_rectangle((0, 0, width * s - 1, 8 * s - 1), radius=4 * s, fill=theme.TRACK)
        if lo is not None and hi is not None:
            d.rounded_rectangle((lo * width * s, 0, max(hi * width * s, lo * width * s + 5 * s), 8 * s - 1),
                                radius=4 * s, fill=theme.GOLD_SOFT[:3] + (200,))

    cv.overlay((PAD, top, width, 8), bar, scale=3)
    cv.draw.rectangle((PAD + mid * width - 1, top - 3, PAD + mid * width + 1, top + 10), fill=theme.TEXT)
    if listing is not None:
        _ask_marker(cv, PAD + listing * width, top, item.ask_median)
    cv.text(PAD, top + 14, format_mesos(item.low), 10, theme.TEXT_FAINT)
    cv.text(cv.width - PAD, top + 14, format_mesos(item.high), 10, theme.TEXT_FAINT, anchor="ra")
    if item.p25 and item.p75:
        cv.text(cv.width / 2, top + 14, f"usually {format_mesos(item.p25)} – {format_mesos(item.p75)}", 10,
                theme.TEXT, anchor="ma")
    return top + 34


def signals(cv: Canvas, y: int, item: Item, now) -> int:
    """Quick facts as pills: how it sells, how many sales, how recent the last one is."""
    pace = insights.market_pace(item)
    parts: list[tuple[str, theme.RGBA]] = []
    if pace:
        parts.append((f"{pace.capitalize()} seller", theme.UP if pace == "fast" else theme.WARN))
    if item.sold:
        parts.append((f"{item.sold:,} sold", theme.TEXT_DIM))
    if item.last_sold:
        parts.append((f"last {format_ago(item.last_sold, now)}", theme.TEXT_DIM))
    x = PAD
    for text, colour in parts:
        x += cv.chip(x, y, text, colour, size=11, height=20) + 5
    return y + 26 if parts else y
