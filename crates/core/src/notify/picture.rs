//! The item's picture in a sale message, from maplestory.io's MapleStory Classic World data (`config::ITEM_API`), which
//! numbers items as the game does: a sale whose item snapped to the price list shows the picture of its own id, with
//! no request (1,866 of the list's 1,867 items are there; 200 of 200 sampled icons loaded). A sale without an id (its
//! name matched no item, or two) is searched by name, once per name; the platform asks (`search_url`) and hands the
//! answer back (`Pictures::learn`). The GMS data used before numbered most items differently (Garnet is 4020100 in
//! Classic, 4021000 there) and named a quarter of them otherwise (Classic's Iron Ingot is its Steel Plate), so a
//! quarter of the sales showed no picture.

use std::collections::HashMap;

use serde_json::Value;

use super::Notification;
use crate::market::catalog::Catalog;

/// The search matches parts of names: "Sapphire" also finds 45 hair colours and earrings before the stone.
const SEARCH_COUNT: usize = 1000;

/// What each id-less item name's picture is, once asked (None: maplestory.io has no item of that name).
#[derive(Clone, Debug, Default)]
pub struct Pictures {
    known: HashMap<String, Option<u64>>,
}

/// The maplestory.io search for items named like `name`.
pub fn search_url(name: &str) -> String {
    format!("{}/item?count={SEARCH_COUNT}&searchFor={}", crate::config::ITEM_API, encoded(name.trim()))
}

/// The picture's id in a search answer: the item of exactly that name (the first of gendered copies). Items of
/// several kinds (the id's first digit: equip, use, etc) are different things that share the name ("Poisonous
/// Mushroom" was a monster drop and a potion), and without the sale's id which one was sold is not known: none.
pub fn pick(answer: &str, name: &str) -> Option<u64> {
    let answer: Value = serde_json::from_str(answer).ok()?;
    let name = folded(name);
    let named: Vec<u64> = answer
        .as_array()?
        .iter()
        .filter(|item| item["name"].as_str().is_some_and(|n| folded(n) == name))
        .filter_map(|item| item["id"].as_u64())
        .collect();
    let kind = |id: u64| id / 1_000_000;
    let first = *named.first()?;
    named.iter().all(|&id| kind(id) == kind(first)).then_some(first)
}

/// The picture of the price list's item of exactly this name: for a sale kept from before pictures came by id. A
/// fuzzy match is not enough here, it could show another item's picture.
pub fn in_catalog(catalog: &Catalog, name: &str) -> Option<String> {
    let item = catalog.lookup(name).filter(|item| folded(&item.name) == folded(name))?;
    item.game_id().map(super::item_icon_url)
}

impl Pictures {
    /// The sold items' names to search for, each once: those without the game's id whose picture is not known yet.
    pub fn wanted(&self, batch: &[Notification]) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for notification in batch {
            if let Notification::Sale(sale) = notification
                && sale.item_id.is_none()
                && !self.known.contains_key(&folded(&sale.item))
                && !names.iter().any(|n| folded(n) == folded(&sale.item))
            {
                names.push(sale.item.clone());
            }
        }
        names
    }

    /// maplestory.io's answer to `search_url(name)`. A failed request is not an answer: the name stays wanted.
    pub fn learn(&mut self, name: &str, answer: &str) {
        self.known.insert(folded(name), pick(answer, name));
    }

    /// Gives every sale in `batch` its picture: its own id, else the one found by its name.
    pub fn fill(&self, batch: &mut [Notification]) {
        for notification in batch {
            if let Notification::Sale(sale) = notification {
                sale.picture = sale.item_id.or_else(|| self.known.get(&folded(&sale.item)).copied().flatten());
            }
        }
    }

    /// The picture's address for an item of this name, once found (for showing it beside the sale, not only in it).
    pub fn url(&self, name: &str) -> Option<String> {
        self.known.get(&folded(name)).copied().flatten().map(super::item_icon_url)
    }
}

fn folded(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// `text` as a URL query value.
fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::Sale;

    /// maplestory.io's answer for "Garnet" (shortened to the fields read).
    const GARNET: &str = r#"[{"name":"Garnet Ore","id":4010100},{"name":"Garnet","id":4020100}]"#;

    fn sale(item: &str, item_id: Option<u64>) -> Notification {
        Notification::Sale(Sale { item: item.into(), item_id, ..Sale::default() })
    }

    #[test]
    fn pictures_are_of_the_games_own_data() {
        let search = search_url(" Mrs. Ming Ming's Stew ");
        assert!(search.ends_with("/api/MCW/1/item?count=1000&searchFor=Mrs.%20Ming%20Ming%27s%20Stew"), "{search}");
        assert_eq!(crate::notify::item_icon_url(4_010_101), "https://maplestory.io/api/MCW/1/item/4010101/icon");
    }

    #[test]
    fn the_picture_is_the_item_of_exactly_that_name() {
        assert_eq!(pick(GARNET, "Garnet"), Some(4_020_100));
        assert_eq!(pick(GARNET, "garnet "), Some(4_020_100));
        assert_eq!(pick(GARNET, "Garnet Earrings"), None);
        assert_eq!(pick("not json", "Garnet"), None);
    }

    #[test]
    fn items_of_one_name_but_several_kinds_name_none() {
        let answer = r#"[{"name":"Poisonous Mushroom","id":2011000},{"name":"Poisonous Mushroom","id":4000176}]"#;
        assert_eq!(pick(answer, "Poisonous Mushroom"), None);
        let gendered = r#"[{"name":"Red Cloth Pants","id":1062001},{"name":"Red Cloth Pants","id":1061002}]"#;
        assert_eq!(pick(gendered, "Red Cloth Pants"), Some(1_062_001));
    }

    #[test]
    fn a_sale_with_the_games_id_needs_no_search() {
        let mut pictures = Pictures::default();
        let mut batch = vec![
            sale("Iron Ingot", Some(4_010_101)),
            sale("Garnet", None),
            sale("Garnet", None),
            sale("Unknown Thing", None),
            Notification::Store(Default::default()),
        ];
        assert_eq!(pictures.wanted(&batch), ["Garnet", "Unknown Thing"]);
        pictures.learn("Garnet", GARNET);
        pictures.learn("Unknown Thing", "[]");
        assert!(pictures.wanted(&batch).is_empty());
        pictures.fill(&mut batch);
        let found: Vec<Option<u64>> =
            batch.iter().filter_map(|n| if let Notification::Sale(s) = n { Some(s.picture) } else { None }).collect();
        assert_eq!(found, [Some(4_010_101), Some(4_020_100), Some(4_020_100), None]);
    }
}
