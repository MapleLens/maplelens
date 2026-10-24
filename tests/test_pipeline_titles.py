from pathlib import Path

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
    "to send",   # fragment of the same tooltip, repeats for several frames
    "",
])
def test_ui_hints_and_noise_are_not_titles(text):
    assert not _looks_like_title(text)


def test_crafted_item_title_sits_below_the_crafter_line():
    import dataclasses

    import cv2

    from maplelens.market.catalog import Catalog
    from maplelens.market.sample import SAMPLE_ITEMS
    from maplelens.pipeline import HoverPipeline
    from maplelens.vision.ocr import TitleReader

    # "bTang's" prints on its own line above "Great Red Helmet", so the usual title row misses the name.
    image = cv2.imread(str(Path(__file__).parent / "fixtures" / "tooltip_equip_crafted_great_red_helmet.png"))
    helmet = dataclasses.replace(SAMPLE_ITEMS[0], name="Great Red Helmet", category="Equip")
    pipeline = HoverPipeline(Catalog([helmet]))
    reader = TitleReader()
    item, crafted = pipeline._lookup(reader, image[:120], reader.read(image[:120], 370))
    assert item is not None and item.name == "Great Red Helmet"
    assert crafted
