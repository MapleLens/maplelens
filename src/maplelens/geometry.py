"""Tiny geometry primitive shared by capture and overlay."""

from typing import NamedTuple


class Box(NamedTuple):
    """Axis-aligned rectangle in pixels."""

    x: int
    y: int
    w: int
    h: int

    @property
    def right(self) -> int:
        return self.x + self.w

    @property
    def bottom(self) -> int:
        return self.y + self.h
