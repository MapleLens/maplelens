//! NiaDB (meowdb.com, its MapleStory Classic section), used with the site's permission: the drops players confirmed
//! in Classic, per monster. Its monster list is read off its monster page (the data the page is built from), each
//! monster's drops from its API. Neither has the game's monster IDs, so a map's monsters are matched by name and
//! level. The platform makes the requests (paced and cached); parsing and matching are here.
//!
//! The site has no drop chances of its own. Each monster's page shows OSMLib's rates (osmlib.com, the old GMS data)
//! "for reference", warning that Classic's may differ; they are read off that page and shown as such, never taken
//! for Classic's (rarity stays by price).

use std::collections::HashSet;

use serde_json::{Value, json};

use super::Drop;
use crate::mapinfo::Mob;

const SITE: &str = "https://meowdb.com/msclassic";
/// How the site asks to be credited: by this name, with the page the data came from (written out where it cannot be
/// a link, as on the overlay).
pub const CREDIT: &str = "NiaDB";
/// Where the monster pages' reference rates come from, as the page credits them.
pub const RATE_CREDIT: &str = "OSMLib";
/// The monster page's section of reference rates.
const RATES_SECTION: &str = "<section id=\"osmlib-drop-rates\"";
/// A rate's row names its item by the game's ID in its icon's path (the OSMLib link's ID is OSMLib's own).
const ICON_PATH: &str = "/api/assets/icons/";
/// Where Next.js streams a page's data, as JSON string literals.
const PAGE_DATA: &str = "self.__next_f.push([1,";

/// A monster as the site lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monster {
    /// The site's own ID (its pages and API), not the game's.
    pub id: u32,
    pub name: String,
    pub level: u32,
    pub hp: u64,
    pub exp: u64,
}

pub fn monsters_url() -> String {
    format!("{SITE}/monsters")
}

/// A monster's page: its reference drop rates.
pub fn monster_url(id: u32) -> String {
    format!("{SITE}/monsters/{id}")
}

pub fn drops_url(id: u32) -> String {
    format!("{SITE}/api/drops?monsterId={id}")
}

/// The credit for one monster's drops: "NiaDB · meowdb.com/msclassic/monsters/9".
pub fn credit(id: u32) -> String {
    format!("{CREDIT} · meowdb.com/msclassic/monsters/{id}")
}

/// The credit under the panel, for every monster on it.
pub fn panel_credit() -> String {
    format!("{CREDIT} · meowdb.com/msclassic/monsters")
}

/// The monsters in the monster page's HTML: the objects in its streamed data that carry a monster's id, name, level,
/// HP and EXP, each once.
pub fn monsters_from_page(html: &str) -> Vec<Monster> {
    let mut data = String::new();
    let mut rest = html;
    while let Some(at) = rest.find(PAGE_DATA) {
        rest = &rest[at + PAGE_DATA.len()..];
        let mut chunks = serde_json::Deserializer::from_str(rest).into_iter::<String>();
        if let Some(Ok(chunk)) = chunks.next() {
            data.push_str(&chunk);
            rest = &rest[chunks.byte_offset()..];
        }
    }
    let (mut monsters, mut seen) = (Vec::new(), HashSet::new());
    let mut rest = data.as_str();
    while let Some(at) = rest.find("{\"id\":") {
        rest = &rest[at..];
        let mut values = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
        match values.next().and_then(Result::ok).as_ref().and_then(monster) {
            Some(found) => {
                rest = &rest[values.byte_offset()..];
                if seen.insert(found.id) {
                    monsters.push(found);
                }
            }
            // Not a monster, or one holding the list: look inside it.
            None => rest = &rest[1..],
        }
    }
    monsters
}

fn monster(value: &Value) -> Option<Monster> {
    let number = |key: &str| value.get(key).and_then(Value::as_u64);
    Some(Monster {
        id: u32::try_from(number("id")?).ok()?,
        name: value.get("name")?.as_str()?.trim().to_string(),
        level: u32::try_from(number("level")?).ok()?,
        hp: number("hp")?,
        exp: number("exp")?,
    })
}

/// The list as kept on disk (only what matching needs, not the page).
pub fn monsters_to_json(monsters: &[Monster]) -> String {
    let list: Vec<Value> = monsters
        .iter()
        .map(|m| json!({"id": m.id, "name": m.name, "level": m.level, "hp": m.hp, "exp": m.exp}))
        .collect();
    Value::Array(list).to_string()
}

pub fn monsters_from_json(text: &str) -> Vec<Monster> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    list.iter().filter_map(monster).collect()
}

fn fold(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// The site's monster for a map's monster: the same name and level, the closest in EXP then HP among several (two
/// Stirges share both), the lowest ID among equals. A name the site has once counts at another level (Classic moved
/// a few); a name it has several times needs the level.
pub fn find<'a>(monsters: &'a [Monster], mob: &Mob) -> Option<&'a Monster> {
    let name = fold(&mob.name);
    let named: Vec<&Monster> = monsters.iter().filter(|m| fold(&m.name) == name).collect();
    let closest = named
        .iter()
        .filter(|m| m.level == mob.level)
        .min_by_key(|m| (m.exp.abs_diff(mob.exp), m.hp.abs_diff(mob.hp), m.id))
        .copied();
    closest.or_else(|| (named.len() == 1).then(|| named[0]))
}

/// `api/drops?monsterId=`: the drops players confirmed (more confirmations than doubts: `score` above 0), by the game's
/// item ID (`itemIconKey`, the ID the game, maplestory.io and the price list share).
pub fn parse_drops(text: &str) -> Result<Vec<Drop>, String> {
    let raw: Value = serde_json::from_str(text).map_err(|e| format!("bad JSON: {e}"))?;
    let list = raw.get("drops").and_then(Value::as_array).ok_or("no drops list")?;
    Ok(list
        .iter()
        .filter_map(|d| {
            let key = d.get("itemIconKey")?;
            let item = key.as_u64().or_else(|| key.as_str()?.trim().parse().ok())?;
            let name = d.get("itemName")?.as_str()?.trim();
            let confirmed = d.get("score").and_then(Value::as_f64).is_some_and(|s| s > 0.0);
            (confirmed && !name.is_empty()).then(|| {
                Some(Drop { item: u32::try_from(item).ok()?, name: name.to_string(), chance: None, rate: None })
            })?
        })
        .collect())
}

/// The reference rates on a monster's page, by the game's item ID, per kill (0..1). Each row's "About 1 in X" is
/// taken before its percent: the percent is rounded to one digit ("0.03%" for 1 in 3,333.33). A page without the
/// section has none.
pub fn rates_from_page(html: &str) -> Vec<(u32, f64)> {
    let Some(start) = html.find(RATES_SECTION) else { return Vec::new() };
    let section = &html[start..];
    let section = &section[..section.find("</section>").unwrap_or(section.len())];
    section.split("<tr").filter_map(rate_row).collect()
}

fn rate_row(row: &str) -> Option<(u32, f64)> {
    let icon = &row[row.find(ICON_PATH)? + ICON_PATH.len()..];
    let item: u32 = icon[..icon.find(|c: char| !c.is_ascii_digit())?].parse().ok()?;
    let one_in = row.find("1 in ").and_then(|at| {
        let text = &row[at + 5..];
        let number: String = text[..text.find('<')?].chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
        number.parse::<f64>().ok().filter(|x| *x >= 1.0).map(|x| 1.0 / x)
    });
    let percent = || {
        let end = row.find("%<")?;
        let start = row[..end].rfind('>')? + 1;
        row[start..end].trim().parse::<f64>().ok().filter(|p| *p > 0.0 && *p <= 100.0).map(|p| p / 100.0)
    };
    Some((item, one_in.or_else(percent)?))
}

/// The rates as kept on disk: `{"4000009": 0.4}`.
pub fn rates_to_json(rates: &[(u32, f64)]) -> String {
    let map: serde_json::Map<String, Value> =
        rates.iter().map(|&(item, rate)| (item.to_string(), json!(rate))).collect();
    Value::Object(map).to_string()
}

pub fn rates_from_json(text: &str) -> Vec<(u32, f64)> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    map.iter().filter_map(|(item, rate)| Some((item.parse().ok()?, rate.as_f64()?))).collect()
}

/// `drops` with each one's reference rate from `rates`, where the page had one.
pub fn with_rates(mut drops: Vec<Drop>, rates: &[(u32, f64)]) -> Vec<Drop> {
    for drop in &mut drops {
        drop.rate = rates.iter().find(|(item, _)| *item == drop.item).map(|&(_, rate)| rate);
    }
    drops
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page's shape, hand-written: the list streams as a JSON string (split anywhere), inside other objects.
    fn page() -> String {
        let data = r#"0:{"props":{"id":0,"monsters":[{"id":9,"name":"Orange Mushroom","level":8,"hp":80,"mp":10,"exp":15,"regions":["VictoriaIsland"]},{"id":800020,"name":"Stirge","level":11,"hp":120,"exp":20},{"id":800021,"name":"Stirge","level":11,"hp":150,"exp":24},{"id":7,"name":"Slime","level":6,"hp":50,"exp":10},{"id":9,"name":"Orange Mushroom","level":8,"hp":80,"exp":15}]},"href":"/msclassic/monsters/9"}"#;
        let (a, b) = data.split_at(120);
        let push = |s: &str| format!("<script>self.__next_f.push([1,{}])</script>", serde_json::to_string(s).unwrap());
        format!("<html><body>{}<p>x</p>{}</body></html>", push(a), push(b))
    }

    fn mob(name: &str, level: u32, hp: u64, exp: u64) -> Mob {
        Mob { id: 1, name: name.into(), level, exp, hp, count: 1 }
    }

    #[test]
    fn the_monster_list_is_read_off_the_page_once_each() {
        let monsters = monsters_from_page(&page());
        let ids: Vec<u32> = monsters.iter().map(|m| m.id).collect();
        assert_eq!(ids, [9, 800020, 800021, 7]);
        assert_eq!(monsters[0], Monster { id: 9, name: "Orange Mushroom".into(), level: 8, hp: 80, exp: 15 });
        assert_eq!(monsters_from_json(&monsters_to_json(&monsters)), monsters);
        assert!(monsters_from_page("<html>no data</html>").is_empty());
    }

    #[test]
    fn a_map_monster_is_matched_by_name_and_level() {
        let monsters = monsters_from_page(&page());
        let id = |m: &Mob| find(&monsters, m).map(|m| m.id);
        assert_eq!(id(&mob("Orange Mushroom", 8, 80, 15)), Some(9));
        assert_eq!(id(&mob("orange mushroom", 9, 80, 15)), Some(9), "a name listed once, at another level");
        assert_eq!(id(&mob("Stirge", 11, 150, 24)), Some(800021), "the closer in EXP");
        assert_eq!(id(&mob("Stirge", 11, 1, 1)), Some(800020));
        assert_eq!(id(&mob("Stirge", 12, 150, 24)), None, "two of that name, neither at its level");
        assert_eq!(id(&mob("Blue Snail", 2, 15, 4)), None);
    }

    #[test]
    fn only_confirmed_drops_count_by_the_game_s_item_id() {
        let text = r#"{"drops":[
            {"id":1,"itemId":356,"itemName":"Orange Mushroom Cap","itemIconKey":"4000009","score":33},
            {"id":2,"itemId":270,"itemName":"Red Potion","itemIconKey":2000000,"score":0.5},
            {"id":3,"itemId":1,"itemName":"Dried Squid","itemIconKey":"2020007","score":-2},
            {"id":4,"itemId":2,"itemName":"No Key","score":5},
            {"id":5,"itemId":3,"itemName":"Unvoted","itemIconKey":"4000000"}],"isLoggedIn":false}"#;
        let drops = parse_drops(text).expect("readable");
        let items: Vec<(u32, &str)> = drops.iter().map(|d| (d.item, d.name.as_str())).collect();
        assert_eq!(items, [(4_000_009, "Orange Mushroom Cap"), (2_000_000, "Red Potion")]);
        assert!(drops.iter().all(|d| d.chance.is_none()));
        assert!(parse_drops("{}").is_err());
        assert!(parse_drops("<html>").is_err());
    }

    /// The page's rates section, hand-written in its shape: the icon names the game's item, the link OSMLib's.
    fn rates_page() -> String {
        let row = |niadb: u32, icon: u32, percent: &str, one_in: &str| {
            format!(
                r#"<tr class="border-t"><td><a href="/msclassic/item-db/{niadb}"><img src="/msclassic/api/assets/icons/{icon}" alt=""/><span>Item</span></a></td><td><a href="https://osmlib.com/?locale=en&amp;search=1" class="font-semibold">{percent}</a><span class="block">{one_in}</span></td></tr>"#
            )
        };
        format!(
            r#"<html><section id="drops"><tr><td><img src="/msclassic/api/assets/icons/1"/>9%</td></tr></section><section id="osmlib-drop-rates" class="p-4"><h2>Reference</h2><table><thead><tr><th>Item</th><th>Rate</th></tr></thead><tbody>{}{}{}{}</tbody></table><p>Source: <a href="https://osmlib.com/">OSMLib</a></p></section><section><img src="/msclassic/api/assets/icons/2"/>5%</section></html>"#,
            row(356, 4_000_009, "40%", "About 1 in 2.5"),
            row(577, 1_312_001, "0.03%", "About 1 in 3,333.33"),
            row(578, 2_000_000, "1%", ""),
            row(579, 2_000_001, "n/a", "")
        )
    }

    #[test]
    fn reference_rates_are_read_off_the_monster_page_by_the_game_s_item_id() {
        let rates = rates_from_page(&rates_page());
        assert_eq!(rates.len(), 3, "only the rates section's rows with a rate: {rates:?}");
        assert_eq!(rates[0], (4_000_009, 0.4));
        assert_eq!(rates[1].0, 1_312_001);
        assert!((rates[1].1 - 0.0003).abs() < 1e-9, "1 in 3,333.33 before the rounded percent");
        assert_eq!(rates[2], (2_000_000, 0.01), "the percent when there is no 1 in X");
        assert!(rates_from_page("<html>no section</html>").is_empty());
        let mut kept = rates_from_json(&rates_to_json(&rates));
        kept.sort_by_key(|r| r.0);
        let mut sorted = rates.clone();
        sorted.sort_by_key(|r| r.0);
        assert_eq!(kept, sorted);
        let drops = parse_drops(r#"{"drops":[{"itemName":"Cap","itemIconKey":"4000009","score":3},{"itemName":"Ore","itemIconKey":"4010000","score":3}]}"#)
            .expect("readable");
        let rated: Vec<Option<f64>> = with_rates(drops, &rates).iter().map(|d| d.rate).collect();
        assert_eq!(rated, [Some(0.4), None]);
    }

    #[test]
    fn credits_name_the_site_and_the_page() {
        assert_eq!(credit(9), "NiaDB · meowdb.com/msclassic/monsters/9");
        assert_eq!(drops_url(9), "https://meowdb.com/msclassic/api/drops?monsterId=9");
        assert_eq!(monster_url(9), "https://meowdb.com/msclassic/monsters/9");
    }
}
