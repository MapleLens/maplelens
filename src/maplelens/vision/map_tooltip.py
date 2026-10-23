"""World-map tooltips ("Victoria Road : Henesys"): text mask and tooltip box from OCR lines (pure)."""

from typing import NamedTuple

import cv2
import numpy as np

from maplelens.geometry import Box
from maplelens.mapinfo.models import Location
from maplelens.mapinfo.resolve import parse_location
from maplelens.vision.ocr import TextLine

_TOPHAT_KERNEL = 11     # wider than a glyph stroke, narrower than the white clouds of the map behind the tooltip
_TOPHAT_MIN = 50        # text is much brighter than its surroundings...
_GRAY_MIN = 150         # ...and bright in itself
_TITLE_ROWS = 4         # the title is among the top rows of text; the scene behind may add stray ones above
_SAME_ROW = 0.6         # lines whose centres are closer than this many line heights are one row of text
_MAX_LINE_GAP = 1.2     # a description line sits within this many line heights below the previous one
_PAD_X = 10             # tooltip border to text, measured on the two samples
_PAD_SINGLE = (8, 8)    # one row: just the title
_PAD_TOP, _PAD_BOTTOM = 16, 10   # with a description the title sits lower in the box


class MapTooltip(NamedTuple):
    location: Location
    box: Box   # the tooltip, in the pixels of the image the lines were read from


def text_mask(bgr: np.ndarray) -> np.ndarray:
    """Pixels of near-white text, whatever the (translucent) tooltip shows behind it."""
    gray = cv2.cvtColor(bgr, cv2.COLOR_BGR2GRAY)
    top_hat = cv2.morphologyEx(gray, cv2.MORPH_TOPHAT,
                               cv2.getStructuringElement(cv2.MORPH_RECT, (_TOPHAT_KERNEL, _TOPHAT_KERNEL)))
    return (top_hat > _TOPHAT_MIN) & (gray > _GRAY_MIN)


def _rows(lines: list[TextLine]) -> list[list[TextLine]]:
    """Lines grouped into rows, top to bottom (OCR may split one printed row into several lines)."""
    rows: list[list[TextLine]] = []
    for line in sorted(lines, key=lambda ln: ln.y):
        if rows and abs((line.y + line.h / 2) - _centre(rows[-1])) < _SAME_ROW * line.h:
            rows[-1].append(line)
        else:
            rows.append([line])
    return rows


def _centre(row: list[TextLine]) -> float:
    return sum(ln.y + ln.h / 2 for ln in row) / len(row)


def find_tooltip(lines: list[TextLine]) -> MapTooltip | None:
    """The tooltip whose title is the top-most row of text that reads as a location, or None.

    A row with the "Street : Name" colon beats one without: the colon is what separates a title from the
    stray text OCR finds in the scene behind the tooltip.
    """
    rows = _rows([ln for ln in lines if ln.text])[:_TITLE_ROWS]
    parsed = [(i, parse_location(" ".join(ln.text for ln in sorted(row, key=lambda ln: ln.x))))
              for i, row in enumerate(rows)]
    titles = [(i, loc) for i, loc in parsed if loc is not None]
    for i, location in sorted(titles, key=lambda t: not t[1].street):   # stable: top-most first within each kind
        return _tooltip(rows, i, location)
    return None


def _tooltip(rows: list[list[TextLine]], i: int, location: Location) -> MapTooltip:
    row = rows[i]
    block = list(row)
    bottom = max(ln.y + ln.h for ln in row)
    for below in rows[i + 1:]:   # the description, while the rows stay close together
        if min(ln.y for ln in below) - bottom > _MAX_LINE_GAP * max(ln.h for ln in below):
            break
        block += below
        bottom = max(ln.y + ln.h for ln in below)
    left, top = min(ln.x for ln in block), min(ln.y for ln in block)
    right = max(ln.x + ln.w for ln in block)
    above, under = _PAD_SINGLE if len(block) == len(row) else (_PAD_TOP, _PAD_BOTTOM)
    return MapTooltip(location, Box(left - _PAD_X, top - above, right - left + 2 * _PAD_X,
                                    bottom - top + above + under))


TINT_MIN_FRACTION = 0.01   # a tooltip fills a few % of the scan window; over a bright map its fill barely tints
TEXT_ROWS_MIN = 8          # a tooltip's text spans at least this many pixel rows


def tint_fraction(bgr: np.ndarray) -> float:
    """Share of pixels in the tooltip's translucent blue: far bluer than red or green, never bright."""
    b, g, r = (bgr[..., i].astype(np.int16) for i in range(3))
    return float(((b >= 80) & (b <= 200) & (b - g >= 12) & (b - r >= 12)).mean())


def text_rows(mask: np.ndarray) -> int:
    """Pixel rows that hold text-like pixels."""
    return int((mask.sum(axis=1) >= 3).sum())


def may_hold_tooltip(bgr: np.ndarray, mask: np.ndarray) -> bool:
    """Cheap pre-check so OCR only runs where a blue box with text could be (the game scene is usually not)."""
    return tint_fraction(bgr) >= TINT_MIN_FRACTION and text_rows(mask) >= TEXT_ROWS_MIN
