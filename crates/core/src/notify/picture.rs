//! The item's picture in a sale message. The price list's ids are MapleStory Classic's, and maplestory.io (the public
//! picture service, GMS data) numbers most items differently: Garnet is 4020100 in Classic and 4021000 there, and of
//! 30 random items of the list only 2 ids named the same item (an unknown id shows no picture, a reused one the wrong
//! one). Names mostly agree (23 of the 30), so the picture is found by searching the item's name, once per name; the
//! platform asks (`search_url`) and hands the answer back (`Pictures::learn`).

use std::collections::HashMap;

use serde_json::Value;

use super::Notification;

/// The search matches parts of names: "Sapphire" also finds 45 hair colours and earrings before the stone.
const SEARCH_COUNT: usize = 1000;

/// What each item name's picture is, once asked (None: maplestory.io has no item of that name).
#[derive(Clone, Debug, Default)]
pub struct Pictures {
    known: HashMap<String, Option<u64>>,
}

/// The maplestory.io search for items named like `name`.
pub fn search_url(name: &str) -> String {
    format!("{}/item?count={SEARCH_COUNT}&searchFor={}", crate::config::MAP_API, encoded(name.trim()))
}

/// The picture's id in a search answer: the item of exactly that name, the one with the game's own id if several
/// are (gendered copies), else one of the same kind (the id's first digit: equip, use, etc). An item of another kind
/// is another thing that shares the name ("Poisonous Mushroom" is a monster drop and a potion), so none.
pub fn pick(answer: &str, name: &str, game_id: Option<u64>) -> Option<u64> {
    let answer: Value = serde_json::from_str(answer).ok()?;
    let name = folded(name);
    let named: Vec<u64> = answer
        .as_array()?
        .iter()
        .filter(|item| item["name"].as_str().is_some_and(|n| folded(n) == name))
        .filter_map(|item| item["id"].as_u64())
        .collect();
    let Some(game_id) = game_id else { return named.first().copied() };
    let kind = |id: u64| id / 1_000_000;
    named.iter().copied().find(|&id| id == game_id).or_else(|| named.into_iter().find(|&id| kind(id) == kind(game_id)))
}

impl Pictures {
    /// The sold items' names whose picture is not known yet, each once.
    pub fn wanted(&self, batch: &[Notification]) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for notification in batch {
            if let Notification::Sale(sale) = notification
                && !self.known.contains_key(&folded(&sale.item))
                && !names.iter().any(|n| folded(n) == folded(&sale.item))
            {
                names.push(sale.item.clone());
            }
        }
        names
    }

    /// maplestory.io's answer to `search_url(name)`. A failed request is not an answer: the name stays wanted.
    pub fn learn(&mut self, name: &str, answer: &str, game_id: Option<u64>) {
        self.known.insert(folded(name), pick(answer, name, game_id));
    }

    /// Gives every sale in `batch` its picture, where known.
    pub fn fill(&self, batch: &mut [Notification]) {
        for notification in batch {
            if let Notification::Sale(sale) = notification {
                sale.picture = self.known.get(&folded(&sale.item)).copied().flatten();
            }
        }
    }

    /// The game's id of the sale of `name` in `batch`, for `learn`.
    pub fn game_id(batch: &[Notification], name: &str) -> Option<u64> {
        batch.iter().find_map(|n| match n {
            Notification::Sale(sale) if folded(&sale.item) == folded(name) => sale.item_id,
            _ => None,
        })
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
    const GARNET: &str = r#"[{"name":"Garnet Ore","id":4020000},{"name":"Garnet","id":4021000}]"#;

    fn sale(item: &str, item_id: Option<u64>) -> Notification {
        Notification::Sale(Sale { item: item.into(), item_id, ..Sale::default() })
    }

    #[test]
    fn the_search_is_for_the_name() {
        assert!(
            search_url(" Mrs. Ming Ming's Stew ").ends_with("/item?count=1000&searchFor=Mrs.%20Ming%20Ming%27s%20Stew")
        );
    }

    #[test]
    fn the_picture_is_the_item_of_that_name_not_of_that_id() {
        // Classic's Garnet is 4020100, which maplestory.io does not have.
        assert_eq!(pick(GARNET, "Garnet", Some(4_020_100)), Some(4_021_000));
        assert_eq!(pick(GARNET, "garnet ", None), Some(4_021_000));
        assert_eq!(pick(GARNET, "Garnet Earrings", Some(4_020_100)), None);
        assert_eq!(pick("not json", "Garnet", None), None);
    }

    #[test]
    fn of_several_items_of_one_name_the_same_kind_wins() {
        let answer = r#"[{"name":"Poisonous Mushroom","id":2011000},{"name":"Poisonous Mushroom","id":4000176}]"#;
        assert_eq!(pick(answer, "Poisonous Mushroom", Some(4_000_064)), Some(4_000_176));
        assert_eq!(pick(answer, "Poisonous Mushroom", Some(2_011_000)), Some(2_011_000));
        assert_eq!(pick(answer, "Poisonous Mushroom", Some(1_000_000)), None, "no equip of that name");
    }

    #[test]
    fn each_name_is_asked_once_and_filled_in() {
        let mut pictures = Pictures::default();
        let mut batch = vec![
            sale("Garnet", Some(4_020_100)),
            sale("Garnet", Some(4_020_100)),
            sale("Unknown Thing", None),
            Notification::Store(Default::default()),
        ];
        assert_eq!(pictures.wanted(&batch), ["Garnet", "Unknown Thing"]);
        assert_eq!(Pictures::game_id(&batch, "Garnet"), Some(4_020_100));
        pictures.learn("Garnet", GARNET, Some(4_020_100));
        pictures.learn("Unknown Thing", "[]", None);
        assert!(pictures.wanted(&batch).is_empty());
        pictures.fill(&mut batch);
        let found: Vec<Option<u64>> =
            batch.iter().filter_map(|n| if let Notification::Sale(s) = n { Some(s.picture) } else { None }).collect();
        assert_eq!(found, [Some(4_021_000), Some(4_021_000), None]);
    }
}
