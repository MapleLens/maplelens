import json

import pytest

from maplelens.market.snapshot import SnapshotError, load_snapshot

RAW = {
    "name": "Mithril Pole Arm", "category": "Weapons", "level": 30, "classes": ["Warrior"], "unitBasis": "item",
    "price": 12000, "priceBasis": "24h", "priceSales": 43, "p25": 9999, "p75": 14499, "low": 6500, "high": 99999,
    "priceAll": 14943.5, "trend": -0.018, "sold": 716, "lowData": False, "askMedian": 15000, "asks": 2388,
    "sellThrough": 0.231, "listingShops": 305, "mesosTraded": 12694326, "topSale": 99999,
    "last3": {"avg": 26332.6, "n": 3}, "lastSold": "2026-10-21T23:10:47Z", "firstSold": "2026-10-07T14:37:38Z",
    "history": [
        {"date": "2026-10-15", "median": 12999, "low": 10000, "high": 20000, "sold": 35},
        {"date": "2026-10-16", "median": None},
    ],
    "obtain": ["npc", "mob"],
    "npc": {"buy": 26000, "sell": 6500, "sellers": ["River (Perion)"], "shop": "River (Perion)"},
    "craft": {"discipline": "Woodcrafting", "level": 1, "qty": 500, "cost": 400, "perItem": 0.8,
              "parts": [{"name": "Processed Wood", "count": 1, "each": 299.5}]},
    "cash": {"nx": 5000, "onSale": True, "crateOdds": None},
}


def write(tmp_path, items, **extra):
    path = tmp_path / "list.json"
    path.write_text(json.dumps({"items": items, **extra}), encoding="utf-8")
    return path


def test_loads_every_field_we_use(tmp_path):
    snapshot = load_snapshot(write(tmp_path, [RAW], generated_at="2026-10-21T23:45:49+00:00"))
    item = snapshot.items[0]
    assert snapshot.generated_at == "2026-10-21T23:45:49+00:00"
    assert (item.name, item.category, item.level, item.classes) == ("Mithril Pole Arm", "Weapons", 30, ("Warrior",))
    assert (item.price, item.price_basis, item.price_sales, item.p25, item.p75) == (12000, "24h", 43, 9999, 14499)
    assert (item.low, item.high, item.ask_median, item.asks, item.listing_shops) == (6500, 99999, 15000, 2388, 305)
    assert item.last3_avg == 26332.6
    assert item.medians == (12999,)                       # days without a median are dropped
    assert item.npc.sell == 6500 and item.npc.sellers == 1
    assert item.craft.per_item == 0.8 and item.craft.parts[0].name == "Processed Wood"
    assert item.cash.nx == 5000 and item.cash.on_sale
    assert item.obtain == ("npc", "mob")


def test_unsold_item_has_safe_defaults(tmp_path):
    (item,) = load_snapshot(write(tmp_path, [{"name": "Unsold", "price": None}])).items
    assert item.price is None and item.sold == 0 and item.history == () and item.npc is None


def test_missing_file(tmp_path):
    with pytest.raises(SnapshotError, match="not found"):
        load_snapshot(tmp_path / "nope.json")


def test_wrong_document(tmp_path):
    path = tmp_path / "list.json"
    path.write_text("{}", encoding="utf-8")
    with pytest.raises(SnapshotError, match="not a MesoWatch"):
        load_snapshot(path)
