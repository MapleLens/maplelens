"""Find where a tooltip starts when the game has slid it up to stay on screen (pure).

A tooltip normally starts at the mouse + `config.TOOLTIP_OFFSET`, but one too tall for the room below the
mouse is pushed up until its bottom touches the bottom of the game. Its height is unknown until it is
found, so the only cue left is its title: the first row of title glyphs sits a fixed distance under the
tooltip's top edge, below a band of plain fill.
"""

import cv2
import numpy as np

from maplelens import config
from maplelens.vision.tooltip import measure_width, tooltip_present

_TITLE_COLS = slice(18, config.TOOLTIP_TITLE_WIDTH)   # same columns the title OCR reads
_GLYPH_PIXELS_MIN = 8      # a title glyph row has at least this many near-white pixels (descender rows: ~10)
_TEXT_THRESHOLD = 185      # as in vision/ocr.py: title glyphs are near-white, the fill never gets this bright
_TITLE_TOP_PAD = 18        # measured: first title glyph row is this far below the tooltip's top edge
_CLEAR_ROWS = 14           # rows of plain fill that must precede it (the top border itself is excluded)
_MAX_CANDIDATES = 8        # OCR is the expensive step; real titles rank ahead of stray text lines


def candidate_tops(strip: np.ndarray) -> list[int]:
    """Rows of `strip` (starting at the tooltip's left edge) where a tooltip could start, best first."""
    gray = cv2.cvtColor(strip[:, _TITLE_COLS], cv2.COLOR_BGR2GRAY)
    glyph = (gray > _TEXT_THRESHOLD).sum(axis=1) >= _GLYPH_PIXELS_MIN
    seen = np.concatenate(([0], np.cumsum(glyph)))   # glyph rows in any window in O(1)
    tops = []
    for row in range(_TITLE_TOP_PAD, len(glyph)):
        if glyph[row] and seen[row] - seen[row - _CLEAR_ROWS] == 0:
            top = row - _TITLE_TOP_PAD
            image = strip[top : top + config.TOOLTIP_CAPTURE_SIZE[1]]
            if tooltip_present(image) and measure_width(image, config.TOOLTIP_WIDTHS) is not None:
                tops.append(top)
    return tops[:_MAX_CANDIDATES]
