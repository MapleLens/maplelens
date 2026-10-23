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


# Every pixel constant in config.py was measured at this client height (1920x1200). The game draws its UI
# proportionally to the window height (measured at 2560x1440: equipment tooltip 442 px = 370 * 1.2, compare
# tooltip offset 442 = 373 * 1.2), so other resolutions scale those numbers by height / this.
REFERENCE_HEIGHT = 1200


def ui_scale(client: Box) -> float:
    """How much bigger (or smaller) than at the measured resolution the game draws its UI in `client`."""
    return client.h / REFERENCE_HEIGHT
