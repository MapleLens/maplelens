"""Finding a tooltip the game slid up to fit on screen.

Both fixtures are strips whose left edge is the tooltip's left edge; the tooltip starts below some UI
that belongs to something else, and its bottom is the bottom of the screen.
"""

from pathlib import Path

import cv2
import pytest

from maplelens.vision.locate import candidate_tops

FIXTURES = Path(__file__).parent / "fixtures"
CASES = [("strip_pushed_up_cash_ring.png", 26), ("strip_pushed_up_cash_ring_b.png", 7)]


@pytest.mark.parametrize(("name", "top"), CASES)
def test_finds_the_pushed_up_tooltip(name, top):
    assert candidate_tops(cv2.imread(str(FIXTURES / name))) == [top]


def test_reads_the_long_title_at_the_found_top():
    from maplelens.vision.ocr import TitleReader

    strip = cv2.imread(str(FIXTURES / CASES[0][0]))
    (top,) = candidate_tops(strip)
    assert TitleReader().read(strip[top : top + 120], 370, lines=2) == "Ink-and-Wash Painting Name Tag Ring"


def test_plain_backdrop_has_no_tooltip():
    assert candidate_tops(cv2.imread(str(FIXTURES / "tooltip_native_over_sky.png"))[300:]) == []


def test_pipeline_finds_the_pushed_up_item(monkeypatch):
    """The whole path: mouse low on screen, the long-named ring's tooltip slid up -> Hover with its real height."""
    from maplelens import pipeline
    from maplelens.geometry import Box
    from maplelens.market.catalog import Catalog
    from maplelens.market.models import Item
    from maplelens.vision.ocr import TitleReader

    screen = cv2.imread(str(FIXTURES / "strip_pushed_up_cash_ring.png"))   # tooltip left edge = screen x 0
    ring = Item(name="Ink-and-Wash Painting Name Tag Ring", category="Cash Shop", price=1_899_999, sold=57)
    monkeypatch.setattr(pipeline.game_window, "cursor_pos", lambda: (-1, 400))   # tooltip is at mouse + (1, 32)

    class Grabber:
        def grab(self, area):
            return screen[area.y : area.y + area.h, area.x : area.x + area.w]

    bounds = Box(0, 0, screen.shape[1], screen.shape[0])
    hoverer = pipeline.HoverPipeline(Catalog([ring]))
    assert hoverer._publish_pushed_up(Grabber(), TitleReader(), bounds, 1.0)
    hover = hoverer.events.get_nowait()
    assert hover.item is ring
    assert hover.pushed_up_height == screen.shape[0] - 26   # from the found top to the bottom of the screen
