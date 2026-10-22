import pytest

from maplelens.market.catalog import Catalog
from maplelens.market.models import Item


def make(name: str, sold: int = 10) -> Item:
    return Item(name=name, category="Etc", price=100, sold=sold)


@pytest.fixture
def catalog() -> Catalog:
    return Catalog([make("Emerald Ore"), make("Tree Branch"), make("Red Snail Shell")])


def test_exact_match_ignores_case_and_punctuation(catalog):
    assert catalog.lookup("emerald  ORE!").name == "Emerald Ore"


def test_tolerates_ocr_slip(catalog):
    assert catalog.lookup("Emerald 0re").name == "Emerald Ore"


def test_unknown_item_returns_none(catalog):
    assert catalog.lookup("Dragon Scale") is None


def test_empty_text_returns_none(catalog):
    assert catalog.lookup("  ") is None


def test_name_collision_keeps_item_with_most_sales():
    catalog = Catalog([make("Iron Ore", sold=5), make("Iron Ore", sold=900), make("Iron Ore", sold=50)])
    assert catalog.lookup("Iron Ore").sold == 900
    assert len(catalog) == 1


def test_equipment_is_recognised_by_item_id_not_category():
    from maplelens.config import TOOLTIP_SIZES
    from maplelens.pipeline import tooltip_size

    cash_pants = Item(name="Lined Hip-Hop Pants", item_id="basil:01067001", category="Cash Shop")
    coupon = Item(name="Selective 5-slot Coupon", item_id="cash:05680001", category="Cash Shop")
    cash_hat = Item(name="Bunny Earmuffs", item_id="cash:01007136", category="Cash Shop")
    assert cash_pants.is_equip and cash_hat.is_equip and not coupon.is_equip
    assert tooltip_size(cash_pants) == tooltip_size(cash_hat) == TOOLTIP_SIZES["Equip"]
    assert tooltip_size(coupon) == TOOLTIP_SIZES["Cash Shop"]


def test_trailing_ocr_junk_is_ignored(catalog):
    from maplelens.market.catalog import clean_title

    assert catalog.lookup("Emerald Ore ii").name == "Emerald Ore"
    assert clean_title("Kitty Camping Hat ii") == "Kitty Camping Hat"
    assert clean_title("Hat Accuracy Scroll: Greater") == "Hat Accuracy Scroll: Greater"


def test_tooltip_size_uses_the_measured_width():
    from maplelens.config import TOOLTIP_SIZES
    from maplelens.pipeline import tooltip_size

    # unknown item, narrow tooltip measured: equipment size, and never the wide default that misplaced the card
    assert tooltip_size(None, 370) == (370, TOOLTIP_SIZES["Equip"][1])
    assert tooltip_size(None, 450)[0] == 450
    assert tooltip_size(None, None) == tuple(TOOLTIP_SIZES["Etc"])   # nothing known: the default size
