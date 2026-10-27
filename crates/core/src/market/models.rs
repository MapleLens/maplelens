//! Market data model: the parts of a MesoWatch list.json item that overlays use.

use serde_json::Value;

/// One day of sales.
#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    /// YYYY-MM-DD
    pub date: String,
    pub median: f64,
    pub low: f64,
    pub high: f64,
    pub sold: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NpcShop {
    /// What an NPC charges for it.
    pub buy: Option<f64>,
    /// What an NPC pays for it (the price floor).
    pub sell: Option<f64>,
    /// The nearest/main NPC that sells it.
    pub shop: Option<String>,
    /// Number of NPCs that sell it.
    pub sellers: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CraftPart {
    pub name: String,
    pub count: i64,
    /// Market cost of one unit.
    pub each: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Craft {
    pub discipline: String,
    pub level: Option<i64>,
    /// Items per craft.
    pub qty: i64,
    /// Total ingredient cost per craft.
    pub cost: Option<f64>,
    /// cost / qty
    pub per_item: Option<f64>,
    pub parts: Vec<CraftPart>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CashInfo {
    pub nx: Option<i64>,
    pub on_sale: bool,
    /// % chance from a cash crate.
    pub crate_odds: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub name: String,
    /// e.g. "basil:01442004"; the digits are the game item ID.
    pub item_id: String,
    /// Equips, Weapons, Scrolls, Use, Stars, Etc, Cash Shop.
    pub category: Option<String>,
    pub level: Option<i64>,
    pub classes: Vec<String>,
    /// "set" when the price is for a whole stack/set.
    pub unit_basis: String,

    /// Typical price: the median over `price_basis` ("24h" or "last10"), from `price_sales` sales. None = never sold.
    pub price: Option<f64>,
    pub price_basis: Option<String>,
    pub price_sales: i64,
    pub p25: Option<f64>,
    pub p75: Option<f64>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    /// Mean over everything seen.
    pub price_all: Option<f64>,
    pub last3_avg: Option<f64>,
    /// Fractional change, 0.042 = +4.2%.
    pub trend: Option<f64>,

    pub sold: i64,
    /// Too few sales for the price to be trusted.
    pub low_data: bool,
    /// Sales marked as scrolled / high-roll copies.
    pub scrolled: i64,
    pub top_sale: Option<f64>,
    pub mesos_traded: f64,
    /// ISO-8601 UTC.
    pub last_sold: Option<String>,
    pub first_sold: Option<String>,

    /// Median of current shop listings.
    pub ask_median: Option<f64>,
    /// Listings right now.
    pub asks: i64,
    /// Shops listing it right now.
    pub listing_shops: i64,
    /// Share of listings that sold, 0..1.
    pub sell_through: Option<f64>,

    pub history: Vec<Day>,
    /// How it enters the game: npc / mob / craft / quest.
    pub obtain: Vec<String>,
    pub npc: Option<NpcShop>,
    pub craft: Option<Craft>,
    pub cash: Option<CashInfo>,
}

impl Item {
    pub fn named(name: &str) -> Self {
        Self { name: name.to_string(), unit_basis: "item".into(), ..Self::default() }
    }

    /// Game item IDs starting 01 are equipment, which has the narrow stat-sheet tooltip. Cash Shop hats, pants
    /// and shoes count too, so the category alone cannot tell the tooltip type.
    pub fn is_equip(&self) -> bool {
        self.item_id.rsplit(':').next().unwrap_or("").starts_with("01")
    }

    /// The game's item ID: the digits after the source prefix ("basil:04010101" -> 4010101).
    pub fn game_id(&self) -> Option<u64> {
        let digits: String = self.item_id.chars().skip_while(|c| *c != ':').filter(char::is_ascii_digit).collect();
        digits.parse().ok()
    }

    pub fn medians(&self) -> Vec<f64> {
        self.history.iter().map(|d| d.median).collect()
    }

    /// One item of list.json. Missing or null fields take safe defaults; only a missing name is an error.
    pub fn from_raw(raw: &Value) -> Option<Self> {
        let last3 = raw.get("last3");
        Some(Self {
            name: raw.get("name")?.as_str()?.to_string(),
            item_id: string(raw, "id").unwrap_or_default(),
            category: string(raw, "category"),
            level: int(raw, "level"),
            classes: strings(raw, "classes"),
            unit_basis: string(raw, "unitBasis").filter(|s| !s.is_empty()).unwrap_or_else(|| "item".into()),
            price: num(raw, "price"),
            price_basis: string(raw, "priceBasis"),
            price_sales: int(raw, "priceSales").unwrap_or(0),
            p25: num(raw, "p25"),
            p75: num(raw, "p75"),
            low: num(raw, "low"),
            high: num(raw, "high"),
            price_all: num(raw, "priceAll"),
            last3_avg: last3.and_then(|l| num(l, "avg")),
            trend: num(raw, "trend"),
            sold: int(raw, "sold").unwrap_or(0),
            low_data: raw.get("lowData").and_then(Value::as_bool).unwrap_or(false),
            scrolled: int(raw, "scrolled").unwrap_or(0),
            top_sale: num(raw, "topSale"),
            mesos_traded: num(raw, "mesosTraded").unwrap_or(0.0),
            last_sold: string(raw, "lastSold"),
            first_sold: string(raw, "firstSold"),
            ask_median: num(raw, "askMedian"),
            asks: int(raw, "asks").unwrap_or(0),
            listing_shops: int(raw, "listingShops").unwrap_or(0),
            sell_through: num(raw, "sellThrough"),
            history: raw
                .get("history")
                .and_then(Value::as_array)
                .map_or_else(Vec::new, |days| days.iter().filter_map(day).collect()),
            obtain: strings(raw, "obtain"),
            npc: raw.get("npc").filter(|v| truthy(v)).map(npc),
            craft: raw.get("craft").filter(|v| truthy(v)).map(craft),
            cash: raw.get("cash").filter(|v| truthy(v)).map(|c| CashInfo {
                nx: int(c, "nx"),
                on_sale: c.get("onSale").and_then(Value::as_bool).unwrap_or(false),
                crate_odds: num(c, "crateOdds"),
            }),
        })
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Object(o) => !o.is_empty(),
        _ => true,
    }
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn int(v: &Value, key: &str) -> Option<i64> {
    num(v, key).map(|n| n as i64)
}

fn string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
}

/// Days without a median carry no price and are dropped.
fn day(raw: &Value) -> Option<Day> {
    let median = num(raw, "median")?;
    Some(Day {
        date: string(raw, "date").unwrap_or_default(),
        median,
        low: num(raw, "low").unwrap_or(median),
        high: num(raw, "high").unwrap_or(median),
        sold: int(raw, "sold").unwrap_or(0),
    })
}

fn npc(raw: &Value) -> NpcShop {
    NpcShop {
        buy: num(raw, "buy"),
        sell: num(raw, "sell"),
        shop: string(raw, "shop"),
        sellers: raw.get("sellers").and_then(Value::as_array).map_or(0, Vec::len),
    }
}

fn craft(raw: &Value) -> Craft {
    let parts = raw.get("parts").and_then(Value::as_array).map_or_else(Vec::new, |parts| {
        parts
            .iter()
            .filter_map(|p| {
                Some(CraftPart {
                    name: string(p, "name")?,
                    count: int(p, "count").filter(|&c| c != 0).unwrap_or(1),
                    each: num(p, "each"),
                })
            })
            .collect()
    });
    Craft {
        discipline: string(raw, "discipline").filter(|s| !s.is_empty()).unwrap_or_else(|| "Crafting".into()),
        level: int(raw, "level"),
        qty: int(raw, "qty").filter(|&q| q != 0).unwrap_or(1),
        cost: num(raw, "cost"),
        per_item: num(raw, "perItem"),
        parts,
    }
}
