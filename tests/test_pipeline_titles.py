import pytest

from maplelens.pipeline import _looks_like_title


@pytest.mark.parametrize("text", ["Emerald Ore", "Kitty Camping Hat", "Hunter's Bow"])
def test_item_names_are_titles(text):
    assert _looks_like_title(text)


@pytest.mark.parametrize("text", [
    "Double-click to send whispers.",   # friend list hover tooltip
    "C) oubl*-click: tü send",
    "r •.lfll IVIlP-f: lif:k tri rid b'dhi P.",
    "to",
    "",
])
def test_ui_hints_and_noise_are_not_titles(text):
    assert not _looks_like_title(text)
