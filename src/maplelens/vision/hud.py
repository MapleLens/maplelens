"""Measure the status bar at the bottom of the game, the ruler for how big the game draws its UI (pure).

The bar (chat line, level, HP/MP/EXP, menu buttons) is centred at the bottom of the client and outlined by a
1 px near-black line: one unbroken row on top, one column down each side. It is drawn at the same UI size as
tooltips and windows, in every "UI Size" mode, so its width tells the UI scale without knowing the mode.
"""

from typing import NamedTuple

import numpy as np

_DARK_MAX = 40            # outline pixels: every channel at most this (measured 1-9)
_MIN_WIDTH_SHARE = 0.2    # the bar spans at least this much of the client (Ideal Ratio at 2560 px: ~31%)
_CENTRE_SLACK_PX = 6      # the bar is centred; a dark line elsewhere is scenery
_CORNER_PX = 6            # rounded corners: the side outline stands this close to the ends of the top row
_SIDE_MIN_DARK = 0.6      # share of the rows under the top outline where a side outline is dark (measured 0.91)


class Bar(NamedTuple):
    """The status bar's outline in strip coordinates."""

    x: int
    top: int
    width: int


def _longest_run(row: np.ndarray) -> tuple[int, int]:
    """Start and end (exclusive) of the longest run of True in `row`."""
    padded = np.concatenate(([False], row, [False])).astype(np.int8)
    edges = np.flatnonzero(np.diff(padded))
    starts, ends = edges[::2], edges[1::2]
    if not len(starts):
        return 0, 0
    best = int(np.argmax(ends - starts))
    return int(starts[best]), int(ends[best])


def _side(dark: np.ndarray, top: int, columns: range) -> int | None:
    """The column in `columns` that is dark the most below `top`, if it is dark enough to be an outline."""
    below = dark[top + 1 :]
    if not len(below):
        return None
    columns = [x for x in columns if 0 <= x < dark.shape[1]]
    scores = [below[:, x].mean() for x in columns]
    if not scores or max(scores) < _SIDE_MIN_DARK:
        return None
    return columns[int(np.argmax(scores))]


def find_bar(strip: np.ndarray) -> Bar | None:
    """The status bar in `strip`, the full-width bottom rows of the client (BGR), or None when it is not there."""
    dark = strip.max(axis=2) <= _DARK_MAX
    centre = strip.shape[1] / 2
    for top in range(len(dark) - 2):   # topmost first: the bar's outline is the first long centred line
        start, end = _longest_run(dark[top])
        if end - start < _MIN_WIDTH_SHARE * strip.shape[1] or abs((start + end) / 2 - centre) > _CENTRE_SLACK_PX:
            continue
        left = _side(dark, top, range(start - _CORNER_PX, start + _CORNER_PX))
        right = _side(dark, top, range(end - _CORNER_PX, end + _CORNER_PX))
        if left is not None and right is not None:
            return Bar(left, top, right - left + 1)
    return None
