"""Status bar measurement on native captures of the bottom of a 1920x1200 client, one per UI Size mode."""

from pathlib import Path

import cv2
import pytest

from maplelens import config
from maplelens.vision.hud import find_bar

FIXTURES = Path(__file__).parent / "fixtures"


@pytest.mark.parametrize(("name", "width", "height"), [
    ("status_bar_normal_1920x1200.png", 1250, 106),
    ("status_bar_ideal_ratio_1920x1200.png", 800, 68),
])
def test_bar_size_in_each_ui_mode(name, width, height):
    strip = cv2.imread(str(FIXTURES / name))
    bar = find_bar(strip)
    assert bar is not None
    assert (bar.width, len(strip) - bar.top) == (width, height)
    assert bar.x == (strip.shape[1] - width) // 2


def test_ideal_ratio_scale_matches_the_bar_height_too():
    strip = cv2.imread(str(FIXTURES / "status_bar_ideal_ratio_1920x1200.png"))
    scale = find_bar(strip).width / config.STATUS_BAR_WIDTH
    assert scale == pytest.approx(68 / 106, abs=0.005)


def test_menu_over_the_outline_is_no_measurement():
    """A game menu cut into the top outline: better no answer than a wrong width (the last one is kept)."""
    assert find_bar(cv2.imread(str(FIXTURES / "status_bar_under_game_menu_1920x1200.png"))) is None


def test_no_bar_on_scenery():
    strip = cv2.imread(str(FIXTURES / "status_bar_normal_1920x1200.png"))[:20]   # above the bar
    assert find_bar(strip) is None
