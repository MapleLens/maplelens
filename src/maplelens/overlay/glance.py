"""The glance card: what matters in the second you hover an item. Each part takes (canvas, y, item) and returns
the y below itself.

Reading order, top to bottom: which item/category -> the price (big), with where it is heading on the right ->
where it sits in its range (`range_bar.py`) -> quick signals (how fast it sells, how many sales, how recent).
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
_SPARK_W, _SPARK_H = 84, 24
_FLAT_TREND = 0.005   # under half a percent reads as noise, so it shows as "steady"


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
    """The price, large, on the left; the trend pill over a sparkline of the week on the right, so a long price
    never runs into them."""
    text = ("~" if item.low_data else "") + format_mesos(item.price)   # "~": few sales, treat as rough
    cv.text(PAD, y, text, 30, theme.GOLD, bold=True)
    cv.image.alpha_composite(_COIN, (PAD + cv.measure(text, 30, True) + 6, y + 10))
    basis = "median of the last 24 h" if item.price_basis == "24h" else "median of the last 10 sales"
    if item.unit_basis == "set":
        basis += " · per set"
    cv.text(PAD, y + 40, basis, 11, theme.TEXT_DIM)
    right = cv.width - PAD
    if item.trend is not None and abs(item.trend) >= _FLAT_TREND:
        colour, label = (theme.UP if item.trend > 0 else theme.DOWN), f"{abs(item.trend) * 100:.1f}%"
        cv.pill(right - cv.pill_width(label, arrow=True), y + 3, label, colour, arrow=item.trend > 0)
    elif item.trend is not None:
        cv.chip(right, y + 5, "steady", right_align=True)
    _sparkline(cv, right - _SPARK_W, y + 27, item)
    return y + 60


def _sparkline(cv: Canvas, x: int, y: int, item: Item) -> None:
    values = item.medians
    if len(values) < 2:
        return
    low, span = min(values), (max(values) - min(values)) or 1

    def draw(d, s):
        pts = [((_SPARK_W - 8) * i / (len(values) - 1) * s + 4 * s,
                (_SPARK_H - 6 - (_SPARK_H - 12) * (v - low) / span) * s + 2 * s) for i, v in enumerate(values)]
        d.polygon([*pts, (pts[-1][0], _SPARK_H * s), (pts[0][0], _SPARK_H * s)], fill=theme.GOLD_SOFT[:3] + (40,))
        d.line(pts, fill=theme.GOLD, width=2 * s, joint="curve")
        ex, ey = pts[-1]
        d.ellipse((ex - 3.5 * s, ey - 3.5 * s, ex + 3.5 * s, ey + 3.5 * s), fill=theme.GOLD, outline=theme.TEXT,
                  width=s)

    cv.overlay((x, y, _SPARK_W, _SPARK_H), draw, scale=3)


def signals(cv: Canvas, y: int, item: Item, now) -> int:
    """Quick facts as chips: how it sells, how many sales ever, how recent the last one is."""
    pace = insights.market_pace(item)
    parts: list[tuple[str, theme.RGBA]] = []
    if pace:
        parts.append((f"{pace.capitalize()} seller", theme.UP if pace == "fast" else theme.WARN))
    if item.sold:
        parts.append((f"{item.sold:,} sold", theme.TEXT_DIM))
    if item.last_sold:
        parts.append((f"last sale {format_ago(item.last_sold, now)}", theme.TEXT_DIM))
    x = PAD
    for text, colour in parts:
        x += cv.chip(x, y, text, colour, size=11, height=20) + 5
    return y + 24 if parts else y
