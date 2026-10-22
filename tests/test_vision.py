"""Tooltip presence and title OCR on real, native-resolution captures of the game.

Each fixture's top-left pixel is the tooltip's top-left corner, as the pipeline captures it.
"""

from pathlib import Path

import cv2
import numpy as np
import pytest

from maplelens.vision.tooltip import tooltip_present

FIXTURES = Path(__file__).parent / "fixtures"


def load(name: str, x: int = 0, y: int = 0) -> np.ndarray:
    return cv2.imread(str(FIXTURES / name))[y:, x:]


# Tooltips over very different backdrops (sky, equipment window, character, shop sign), plus one with
# our own price card directly under it.
CASES = {
    "emerald over a busy backdrop": (load("tooltip_native_over_window.png"), "Emerald Ore"),
    "emerald over sky": (load("tooltip_native_over_sky.png"), "Emerald Ore"),
    "octopus leg": (load("tooltip_native_octopus_leg.png"), "Octopus Leg"),
    "silver ore": (load("tooltip_native_silver_ore.png"), "Silver Ore"),
    "squishy with our card below": (load("tooltip_squishy_liquid.png", 31, 33), "Squishy Liquid"),
}


@pytest.mark.parametrize("case", CASES)
def test_tooltip_is_detected(case):
    assert tooltip_present(CASES[case][0][:46, :450])


def test_plain_frames_have_no_tooltip():
    assert not tooltip_present(np.full((46, 450, 3), 200, np.uint8))
    assert not tooltip_present(np.zeros((46, 450, 3), np.uint8))


# Over a busy backdrop (shop sign text and windows behind the translucent box) the blue fill is no
# longer reliable, so the pipeline trusts a recognised title instead. Only OCR is asserted here.
BUSY = {
    "mushroom spore, busy backdrop": (load("tooltip_mushroom_spore_busy_backdrop.png"), "Mushroom Spore"),
}


@pytest.mark.parametrize("case", [*CASES, *BUSY])
def test_ocr_reads_title(case):
    from maplelens.vision.ocr import TitleReader

    image, title = {**CASES, **BUSY}[case]
    assert TitleReader().read(image[:46, :450], 450) == title


def test_equipment_compare_reads_both_titles():
    """Two equipment tooltips side by side: the first title must not leak into the second or vice versa."""
    from maplelens import config
    from maplelens.vision.ocr import TitleReader

    strip = load("compare_zamadar_garnier_strip.png")
    reader = TitleReader()
    assert reader.read(strip, config.TOOLTIP_TITLE_WIDTH) == "Beginner's Triangular Zamadar"
    second = strip[:, config.TOOLTIP_COMPARE_OFFSET_X :]
    assert reader.read(second, second.shape[1]) == "Beginner's Garnier"


# -- tooltip width ------------------------------------------------------------------------------------------------
WIDTHS = (370, 450)
WIDE = ["tooltip_native_over_sky.png", "tooltip_native_over_window.png", "tooltip_native_octopus_leg.png",
        "tooltip_native_silver_ore.png"]
NARROW = ["tooltip_equip_kitty_camping_hat.png", "tooltip_equip_shop_pants.png"]


@pytest.mark.parametrize("name", WIDE)
def test_wide_tooltips_are_measured_as_450(name):
    from maplelens.vision.tooltip import measure_width

    assert measure_width(load(name)[:120], WIDTHS) == 450


@pytest.mark.parametrize("name", NARROW)
def test_equipment_tooltips_are_measured_as_370(name):
    """Real shop captures. The item itself (a Cash Shop hat, not in the price list) is unknown to the catalog,
    which is exactly why the width has to come from the screen."""
    from maplelens.vision.tooltip import measure_width

    assert measure_width(load(name), WIDTHS) == 370


def test_width_is_unknown_on_a_plain_frame():
    from maplelens.vision.tooltip import measure_width

    assert measure_width(np.full((120, 743, 3), 120, np.uint8), WIDTHS) is None
