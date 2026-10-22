"""Find the game's Character Info window by its title bar, and read the player's name off its name plate (pure)."""

import re
from functools import cache
from pathlib import Path

import cv2
import numpy as np

from maplelens import config
from maplelens.geometry import Box

_TITLE_PNG = Path(__file__).resolve().parent.parent / "assets" / "character_info_title.png"
# The title text is dark on white, so a good match scores ~1.0; other UI never gets near this.
_MATCH_MIN = 0.9
_NAME_TEXT_MIN = 200       # the name is white on a mid-grey plate (<= 140)
_NAME = re.compile(r"[A-Za-z0-9]{2,13}")   # MapleStory names; anything else is an OCR slip


@cache
def _title() -> np.ndarray:
    template = cv2.imread(str(_TITLE_PNG), cv2.IMREAD_GRAYSCALE)
    if template is None:
        raise FileNotFoundError(_TITLE_PNG)
    return template


def title_box(window: Box) -> Box:
    """Screen rectangle of the title text of a window whose top-left is `window`."""
    h, w = _title().shape
    return Box(window.x + config.INFO_TITLE_OFFSET[0], window.y + config.INFO_TITLE_OFFSET[1], w, h)


def find_window(gray: np.ndarray) -> tuple[int, int] | None:
    """Top-left of the Character Info window inside `gray` (a capture of the game), or None."""
    scores = cv2.matchTemplate(gray, _title(), cv2.TM_CCOEFF_NORMED)
    _, best, _, (x, y) = cv2.minMaxLoc(scores)
    if best < _MATCH_MIN:
        return None
    return x - config.INFO_TITLE_OFFSET[0], y - config.INFO_TITLE_OFFSET[1]


def is_window(title_gray: np.ndarray) -> bool:
    """Does a capture of exactly the title area still show the title (i.e. the window has not moved)?"""
    template = _title()
    if title_gray.shape != template.shape:
        return False
    return float(cv2.matchTemplate(title_gray, template, cv2.TM_CCOEFF_NORMED)[0, 0]) >= _MATCH_MIN


def name_box(window: Box) -> Box:
    x, y, w, h = config.INFO_NAME_BOX
    return Box(window.x + x, window.y + y, w, h)


def name_mask(name_bgr: np.ndarray) -> np.ndarray:
    """Glyph mask (True = text) of a capture of the name plate."""
    text = (cv2.cvtColor(name_bgr, cv2.COLOR_BGR2GRAY) > _NAME_TEXT_MIN).astype(np.uint8)
    # The name is drawn with 1 px strokes, which the OCR engine drops entirely; thickening them fixes that.
    return cv2.dilate(text, np.ones((2, 2), np.uint8)).astype(bool)


def clean_name(text: str) -> str | None:
    """The OCR text as a character name, or None if it cannot be one."""
    squashed = text.replace(" ", "")
    return squashed if _NAME.fullmatch(squashed) else None
