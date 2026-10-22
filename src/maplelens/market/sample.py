"""Built-in demo items so the overlay works before a real snapshot is saved. Illustrative numbers only."""

from maplelens.market.models import Day, Item, NpcShop


def _week(medians: tuple[float, ...], sold: tuple[int, ...]) -> tuple[Day, ...]:
    return tuple(
        Day(f"2026-10-{15 + i}", m, m * 0.9, m * 1.15, s) for i, (m, s) in enumerate(zip(medians, sold, strict=True))
    )


SAMPLE_ITEMS: tuple[Item, ...] = (
    Item(name="Emerald Ore", category="Etc", price=100, price_basis="24h", price_sales=61, p25=99, p75=119,
         low=40, high=900, price_all=121, last3_avg=104, trend=0.042, sold=1000, top_sale=900,
         mesos_traded=151_000, last_sold="2026-10-21T20:06:20Z", first_sold="2026-10-07T10:00:00Z",
         ask_median=118, asks=640, listing_shops=96, sell_through=0.31, npc=NpcShop(None, 2, None, 0),
         history=_week((92, 95, 99, 97, 103, 98, 100), (120, 90, 140, 110, 160, 130, 150)), obtain=("mob",)),
    Item(name="Tree Branch", category="Etc", price=20, price_basis="24h", price_sales=90, p25=19, p75=25,
         low=5, high=60, price_all=24, trend=-0.03, sold=1208, ask_median=30, asks=900, listing_shops=140,
         sell_through=0.12, last_sold="2026-10-21T12:46:30Z", first_sold="2026-10-07T10:00:00Z",
         history=_week((22, 21, 21, 20, 20, 19, 20), (80, 95, 70, 85, 110, 90, 100))),
)
