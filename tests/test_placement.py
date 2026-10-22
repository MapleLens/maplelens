from maplelens.geometry import Box
from maplelens.overlay.placement import place_card

GAME = Box(100, 50, 1920, 1200)
CARD = (300, 400)
GAP = 6


def test_goes_right_of_the_tooltip():
    tooltip = Box(500, 300, 450, 178)
    assert place_card(tooltip, CARD, GAP, GAME, compare=False) == (956, 300)


def test_goes_left_when_right_is_off_screen():
    tooltip = Box(1500, 300, 450, 178)   # right edge 1950, card would end at 2256 > 2020
    assert place_card(tooltip, CARD, GAP, GAME, compare=False) == (1500 - GAP - 300, 300)


def test_goes_below_when_neither_side_fits():
    tooltip = Box(150, 300, 1800, 178)
    x, y = place_card(tooltip, CARD, GAP, GAME, compare=False)
    assert (x, y) == (150, 300 + 178 + GAP)


def test_equipment_compare_always_goes_below():
    tooltip = Box(500, 300, 370, 324)
    assert place_card(tooltip, CARD, GAP, GAME, compare=True) == (500, 300 + 324 + GAP)


def test_below_flips_above_when_no_room_at_the_bottom():
    tooltip = Box(500, 800, 370, 324)   # bottom 1124, card would end at 1530 > 1250
    assert place_card(tooltip, CARD, GAP, GAME, compare=True) == (500, 800 - GAP - 400)


def test_card_is_clamped_inside_the_window_vertically():
    tooltip = Box(500, 1100, 450, 178)
    _, y = place_card(tooltip, CARD, GAP, GAME, compare=False)
    assert y + 400 <= GAME.bottom
