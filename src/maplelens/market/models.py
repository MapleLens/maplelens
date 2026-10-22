"""Market data model: the parts of a MesoWatch list.json item that overlays use."""

from dataclasses import dataclass, field


@dataclass(frozen=True, slots=True)
class Day:
    """One day of sales."""

    date: str   # YYYY-MM-DD
    median: float
    low: float
    high: float
    sold: int


@dataclass(frozen=True, slots=True)
class NpcShop:
    buy: float | None     # what an NPC charges for it
    sell: float | None    # what an NPC pays for it (the price floor)
    shop: str | None      # the nearest/main NPC that sells it
    sellers: int          # number of NPCs that sell it


@dataclass(frozen=True, slots=True)
class CraftPart:
    name: str
    count: int
    each: float | None    # market cost of one unit


@dataclass(frozen=True, slots=True)
class Craft:
    discipline: str
    level: int | None
    qty: int                      # items per craft
    cost: float | None            # total ingredient cost per craft
    per_item: float | None        # cost / qty
    parts: tuple[CraftPart, ...]


@dataclass(frozen=True, slots=True)
class CashInfo:
    nx: int | None
    on_sale: bool
    crate_odds: float | None      # % chance from a cash crate


@dataclass(frozen=True, slots=True, kw_only=True)
class Item:
    name: str
    item_id: str = ""             # e.g. "basil:01442004"; the digits are the game item ID
    category: str | None = None   # Equips, Weapons, Scrolls, Use, Stars, Etc, Cash Shop
    level: int | None = None
    classes: tuple[str, ...] = ()
    unit_basis: str = "item"      # "set" when the price is for a whole stack/set

    # Typical price: the median over `price_basis` ("24h" or "last10"), from `price_sales` sales.
    price: float | None = None    # None = never sold
    price_basis: str | None = None
    price_sales: int = 0
    p25: float | None = None
    p75: float | None = None
    low: float | None = None
    high: float | None = None
    price_all: float | None = None   # mean over everything seen
    last3_avg: float | None = None
    trend: float | None = None       # fractional change, 0.042 = +4.2%

    sold: int = 0
    low_data: bool = False           # too few sales for the price to be trusted
    scrolled: int = 0                # sales marked as scrolled / high-roll copies
    top_sale: float | None = None
    mesos_traded: float = 0
    last_sold: str | None = None     # ISO-8601 UTC
    first_sold: str | None = None

    ask_median: float | None = None  # median of current shop listings
    asks: int = 0                    # listings right now
    listing_shops: int = 0           # shops listing it right now
    sell_through: float | None = None   # share of listings that sold, 0..1

    history: tuple[Day, ...] = ()
    obtain: tuple[str, ...] = ()     # how it enters the game: npc / mob / craft / quest
    npc: NpcShop | None = None
    craft: Craft | None = None
    cash: CashInfo | None = field(default=None)

    @property
    def is_equip(self) -> bool:
        """Game item IDs starting 01 are equipment, which has the narrow stat-sheet tooltip. Cash Shop
        hats, pants and shoes count too, so the category alone cannot tell the tooltip type."""
        return self.item_id.rpartition(":")[2].startswith("01")

    @property
    def medians(self) -> tuple[float, ...]:
        return tuple(day.median for day in self.history)

    @classmethod
    def from_raw(cls, raw: dict) -> "Item":
        last3 = raw.get("last3") or {}
        return cls(
            name=raw["name"],
            item_id=raw.get("id") or "",
            category=raw.get("category"),
            level=raw.get("level"),
            classes=tuple(raw.get("classes") or ()),
            unit_basis=raw.get("unitBasis") or "item",
            price=raw.get("price"),
            price_basis=raw.get("priceBasis"),
            price_sales=int(raw.get("priceSales") or 0),
            p25=raw.get("p25"),
            p75=raw.get("p75"),
            low=raw.get("low"),
            high=raw.get("high"),
            price_all=raw.get("priceAll"),
            last3_avg=last3.get("avg"),
            trend=raw.get("trend"),
            sold=int(raw.get("sold") or 0),
            low_data=bool(raw.get("lowData")),
            scrolled=int(raw.get("scrolled") or 0),
            top_sale=raw.get("topSale"),
            mesos_traded=raw.get("mesosTraded") or 0,
            last_sold=raw.get("lastSold"),
            first_sold=raw.get("firstSold"),
            ask_median=raw.get("askMedian"),
            asks=int(raw.get("asks") or 0),
            listing_shops=int(raw.get("listingShops") or 0),
            sell_through=raw.get("sellThrough"),
            history=tuple(_day(d) for d in raw.get("history") or () if d.get("median") is not None),
            obtain=tuple(raw.get("obtain") or ()),
            npc=_npc(raw.get("npc")),
            craft=_craft(raw.get("craft")),
            cash=_cash(raw.get("cash")),
        )


def _day(raw: dict) -> Day:
    median = raw["median"]
    return Day(raw["date"], median, raw.get("low", median), raw.get("high", median), int(raw.get("sold") or 0))


def _npc(raw: dict | None) -> NpcShop | None:
    if not raw:
        return None
    return NpcShop(raw.get("buy"), raw.get("sell"), raw.get("shop"), len(raw.get("sellers") or ()))


def _craft(raw: dict | None) -> Craft | None:
    if not raw:
        return None
    parts = tuple(CraftPart(p["name"], int(p.get("count") or 1), p.get("each")) for p in raw.get("parts") or ())
    return Craft(raw.get("discipline") or "Crafting", raw.get("level"), int(raw.get("qty") or 1),
                 raw.get("cost"), raw.get("perItem"), parts)


def _cash(raw: dict | None) -> CashInfo | None:
    if not raw:
        return None
    return CashInfo(raw.get("nx"), bool(raw.get("onSale")), raw.get("crateOdds"))
