//! Potions from the quickslots: which item each slot holds and how many, summed per item. The session follows how
//! those totals move (down: used, up: picked up).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::config;
use crate::geometry::Rect;
use crate::image::RgbImage;
use crate::render::RgbaImage;
use crate::vision::hud::Bar;
use crate::vision::item_icon::{Found, IconSet};
use crate::vision::quickslot::{self, CountFont};

/// Items that heal or buff: potions from 2000000, foods and drinks, and Pure Water, Unagi and the like up to
/// 2022999. (2023xxx are event blessings, 2028xxx meso pouches.)
const IDS: std::ops::RangeInclusive<u32> = 2_000_000..=2_022_999;
/// A total counts once this many looks in a row agree (a count is redrawn while it changes).
const CONFIRM: u32 = 2;
/// A slot's item is searched for again this often even while its count stays (the item could have been swapped
/// for another with the same count).
const RECHECK_S: f64 = 10.0;
/// A slot that held an item and is unreadable now (a window or the pointer over it) holds back that item's total
/// this long, rather than reading as all of it used.
const COVERED_S: f64 = 5.0;
/// How far around a slot its icon may sit, at 1x.
const ICON_REACH: f32 = 4.0;

/// maplestory.io's list of Use items in the consumable category (the Classic data the item pictures come from).
pub fn list_url() -> String {
    format!("{}/item?overallCategoryFilter=Use&categoryFilter=Consumable&count=1000", config::ITEM_API)
}

pub fn icon_url(id: u32) -> String {
    crate::notify::item_icon_url(u64::from(id))
}

/// The healing and buff items in `list_url`'s answer: (id, name).
pub fn candidates(json: &str) -> Vec<(u32, String)> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut out: Vec<(u32, String)> = items
        .iter()
        .filter_map(|item| {
            let id = u32::try_from(item.get("id")?.as_u64()?).ok().filter(|id| IDS.contains(id))?;
            let name = item.get("name")?.as_str()?.trim();
            (!name.is_empty()).then(|| (id, name.to_string()))
        })
        .collect();
    out.sort_by_key(|(id, _)| *id);
    out.dedup_by_key(|(id, _)| *id);
    out
}

#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    count: Option<u32>,
    item: Option<Found>,
    checked: f64,
    /// The last item seen here, and when.
    held: Option<(u32, f64)>,
}

/// Reads the quickslots into totals per item.
#[derive(Default)]
pub struct PotionWatcher {
    slots: [Slot; quickslot::SLOTS],
    font: Option<CountFont>,
    pending: HashMap<u32, (u32, u32)>,
    totals: HashMap<u32, u32>,
}

impl PotionWatcher {
    /// Look at the quickslots in `frame`, where the status bar is `bar` and the bar's image sits at `origin` in
    /// `frame`. Returns the confirmed total of each item in the quickslots.
    pub fn look(
        &mut self,
        frame: &RgbImage,
        bar: Bar,
        origin: (i32, i32),
        icons: &mut IconSet,
        now: f64,
    ) -> &HashMap<u32, u32> {
        let k = quickslot::scale_of(bar);
        if self.font.as_ref().is_none_or(|f| f.scale() != k) {
            self.font = Some(CountFont::new(k));
            self.slots = Default::default();
        }
        let font = self.font.as_ref().expect("just made");
        let reach = (ICON_REACH * k).ceil() as i32;
        let mut seen: HashMap<u32, u32> = HashMap::new();
        let mut covered: Vec<u32> = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let rect = quickslot::slot_rect(bar, origin, index);
            let count = font.read(frame, rect);
            let Some(count) = count else {
                slot.item = None;
                if let Some((id, at)) = slot.held
                    && now - at <= COVERED_S
                {
                    covered.push(id);
                }
                slot.count = None;
                continue;
            };
            let same = slot.count == Some(count) && now - slot.checked < RECHECK_S;
            let kept = slot.item.filter(|item| same && icons.still_there(frame, *item, k));
            slot.item = kept.or_else(|| {
                slot.checked = now;
                let area = Rect::new(rect.x - reach, rect.y - reach, rect.w + 2 * reach, rect.h + 2 * reach);
                icons.identify(frame, area, k)
            });
            slot.count = Some(count);
            if let Some(item) = slot.item {
                *seen.entry(item.id).or_default() += count;
                slot.held = Some((item.id, now));
            }
        }
        for id in &covered {
            seen.remove(id);
        }
        for (&id, &total) in &seen {
            let votes = match self.pending.get(&id) {
                Some(&(was, n)) if was == total => n + 1,
                _ => 1,
            };
            self.pending.insert(id, (total, votes));
            if votes >= CONFIRM {
                self.totals.insert(id, total);
            }
        }
        self.pending.retain(|id, _| seen.contains_key(id));
        // An item no longer in the quickslots (dragged out) is no longer followed: its last total says nothing now.
        self.totals.retain(|id, _| seen.contains_key(id) || covered.contains(id));
        &self.totals
    }
}

/// One item's part of the session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PotionLine {
    pub id: u32,
    pub name: String,
    pub used: u32,
    pub picked: u32,
    pub icon: Option<Arc<RgbaImage>>,
}

/// The session's potions: the ones that moved, with their icons.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Potions {
    pub lines: Vec<PotionLine>,
}

impl Potions {
    pub fn new(stats: &[super::session::PotionStat], icons: &IconSet) -> Self {
        let lines = stats
            .iter()
            .filter(|s| s.used > 0 || s.picked > 0)
            .map(|s| PotionLine {
                id: s.id,
                name: s.name.clone(),
                used: s.used,
                picked: s.picked,
                icon: icons.icon(s.id).map(|i| i.image.clone()),
            })
            .collect();
        Self { lines }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_healing_and_buff_items_are_candidates() {
        let json = r#"[{"id":2000000,"name":"Red Potion"},{"id":2022003,"name":"Unagi"},{"id":2030000,"name":"Return Scroll"},
            {"id":2000000,"name":"Red Potion"},{"id":2020015,"name":" "},{"name":"No id"}]"#;
        assert_eq!(candidates(json), [(2_000_000, "Red Potion".to_string()), (2_022_003, "Unagi".to_string())]);
        assert!(candidates("not json").is_empty());
        assert!(list_url().contains("categoryFilter=Consumable"));
        assert!(icon_url(2_000_000).ends_with("/item/2000000/icon"));
    }
}
