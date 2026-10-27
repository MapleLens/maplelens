//! Training from frames: the EXP bar and the quickslots' potions about once a second, the minimap's map name every
//! few seconds, into the session tracker and the party quest tracker. The platform calls `step` from its capture
//! loop, and hands over the potions' icons (`add_potion_icon`) as it loads them.

use super::kpq::{QuestTracker, Run};
use super::map_name::MapName;
use super::potions::{PotionWatcher, Potions};
use super::session::{Tracker, TrainingView};
use crate::config;
use crate::geometry::{Rect, scaled, ui_scale};
use crate::pipeline::Screen;
use crate::vision::gauge::read_exp;
use crate::vision::hud::find_bar;
use crate::vision::item_icon::IconSet;
use crate::vision::minimap::find_title;
use crate::vision::quickslot;
use crate::vision::reader::{ImageOcr, OcrAnswer};

/// EXP arrives a monster at a time; once a second is plenty and costs a few milliseconds.
const EXP_POLL_S: f64 = 1.0;
/// The map changes rarely, and each look may ask OCR for two lines.
const MAP_POLL_S: f64 = 2.0;
/// The minimap's corner of the client, at UI scale 1: the window grows with the map's name.
const MINIMAP_AREA: (i32, i32) = (900, 100);
/// A map name not read again for this long (the minimap closed or moved) is no longer trusted.
const MAP_KEEP_S: f64 = 120.0;
/// The bar counts as in view while it was read this recently.
const LIVE_S: f64 = 10.0;
/// Room around the quickslots for their icons to be searched in, at 1x.
const QUICKSLOT_REACH: f32 = 6.0;

/// Everything the cards and the page show about training.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Training {
    pub exp: TrainingView,
    pub quest: Option<Run>,
    /// The minimap window's right edge from the client's left, while it is on screen: a card in the top-left corner
    /// goes beside it.
    pub minimap_right: Option<i32>,
    /// The session's potions from the quickslots.
    pub potions: Potions,
}

/// "Hidden  Street " -> "Hidden Street"; OCR's stray marks at either end are dropped.
fn clean(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let words = words.into_iter().skip_while(|w| !w.chars().any(char::is_alphanumeric));
    let mut words: Vec<&str> = words.collect();
    while words.last().is_some_and(|w| !w.chars().any(|c| c.is_alphanumeric() || c == '>')) {
        words.pop();
    }
    words.join(" ")
}

pub struct TrainingWatcher {
    tracker: Tracker,
    quest: QuestTracker,
    ocr: Box<dyn ImageOcr + Send>,
    place: MapName,
    map: Option<(String, f64)>,
    minimap_right: Option<i32>,
    scale: Option<f32>,
    next_exp: f64,
    next_map: f64,
    icons: IconSet,
    potions: PotionWatcher,
}

impl TrainingWatcher {
    pub fn new(ocr: Box<dyn ImageOcr + Send>, tracker: Tracker) -> Self {
        Self {
            tracker,
            quest: QuestTracker::default(),
            ocr,
            place: MapName::default(),
            map: None,
            minimap_right: None,
            scale: None,
            next_exp: f64::NEG_INFINITY,
            next_map: f64::NEG_INFINITY,
            icons: IconSet::default(),
            potions: PotionWatcher::default(),
        }
    }

    /// A healing or buff item's icon (PNG from maplestory.io), so the quickslots can tell it. False when unreadable.
    pub fn add_potion_icon(&mut self, id: u32, name: &str, png: &[u8]) -> bool {
        self.icons.add_png(id, name, png)
    }

    pub fn potion_icons(&self) -> usize {
        self.icons.len()
    }

    pub fn tracker(&self) -> &Tracker {
        &self.tracker
    }

    pub fn tracker_mut(&mut self) -> &mut Tracker {
        &mut self.tracker
    }

    /// Look at the game when due. `bounds` is the client area on `screen`; `now` is seconds since the epoch.
    /// Returns whether anything was read.
    pub fn step(&mut self, screen: &mut dyn Screen, bounds: Rect, now: f64) -> bool {
        let mut looked = false;
        if now >= self.next_exp {
            self.next_exp = now + EXP_POLL_S;
            looked = true;
            let height = bounds.h.min(scaled(config::STATUS_BAR_STRIP_HEIGHT, ui_scale(bounds)));
            let strip_at = (bounds.x, bounds.bottom() - height);
            let strip = screen.grab(Rect::new(strip_at.0, strip_at.1, bounds.w, height));
            let bar = find_bar(&strip);
            let reading = bar.and_then(|bar| {
                self.scale = Some(bar.width as f32 / config::STATUS_BAR_WIDTH as f32);
                read_exp(&strip, bar)
            });
            self.tracker.observe(now, reading);
            if let Some(bar) = bar.filter(|_| !self.icons.is_empty()) {
                self.look_at_quickslots(screen, bar, strip_at, now);
            }
        }
        if now >= self.next_map {
            self.next_map = now + MAP_POLL_S;
            looked = true;
            self.look_at_minimap(screen, bounds, now);
        }
        looked
    }

    /// The quickslots sit over the bar's right end, above the strip the bar was found in: grabbed on their own.
    fn look_at_quickslots(
        &mut self,
        screen: &mut dyn Screen,
        bar: crate::vision::hud::Bar,
        strip_at: (i32, i32),
        now: f64,
    ) {
        let reach = (QUICKSLOT_REACH * quickslot::scale_of(bar)).ceil() as i32;
        let slots = quickslot::area(bar, strip_at);
        let area = Rect::new(slots.x - reach, slots.y - reach, slots.w + 2 * reach, slots.h + 2 * reach);
        let image = screen.grab(area);
        let origin = (strip_at.0 - area.x, strip_at.1 - area.y);
        let totals = self.potions.look(&image, bar, origin, &mut self.icons, now).clone();
        let icons = &self.icons;
        self.tracker.observe_potions(now, &totals, |id| icons.icon(id).map(|i| i.name.clone()).unwrap_or_default());
    }

    fn look_at_minimap(&mut self, screen: &mut dyn Screen, bounds: Rect, now: f64) {
        let scale = self.scale.unwrap_or_else(|| ui_scale(bounds));
        let (w, h) = MINIMAP_AREA;
        let area = Rect::new(bounds.x, bounds.y, scaled(w, scale).min(bounds.w), scaled(h, scale).min(bounds.h));
        let title = find_title(&screen.grab(area), scale);
        self.minimap_right = title.as_ref().map(|t| t.right as i32);
        // The pointer over the title changes what OCR reads; two looks with it held still would be a new map.
        let pointed = screen
            .cursor()
            .zip(self.minimap_right)
            .is_some_and(|(p, right)| p.x >= area.x && p.x < area.x + right && p.y >= area.y && p.y < area.bottom());
        let read = title.filter(|_| !pointed).and_then(|title| {
            match (self.ocr.recognize_image(&title.street), self.ocr.recognize_image(&title.map)) {
                (OcrAnswer::Text(street), OcrAnswer::Text(map)) => Some((clean(&street), clean(&map))),
                _ => None,
            }
        });
        if let Some((_, map)) = read.filter(|(_, map)| !map.is_empty()) {
            if let Some(settled) = self.place.read(&map) {
                self.map = Some((settled.to_string(), now));
            }
        } else if pointed && let Some((_, seen)) = &mut self.map {
            *seen = now;
        }
        if self.map.as_ref().is_some_and(|(_, seen)| now - seen > MAP_KEEP_S) {
            self.map = None;
            self.place.clear();
        }
        let map = self.map.as_ref().map(|(m, _)| m.clone());
        self.quest.update(map.as_deref(), now);
        self.tracker.set_map(map, now);
    }

    pub fn training(&mut self, now: f64) -> Training {
        let exp = self.tracker.view(now, LIVE_S);
        let potions = Potions::new(&exp.session.potions, &self.icons);
        Training { exp, quest: self.quest.current(now), minimap_right: self.minimap_right, potions }
    }
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn ocr_debris_is_dropped() {
        assert_eq!(clean(" Hidden  Street "), "Hidden Street");
        assert_eq!(clean("' Free Market <3> ."), "Free Market <3>");
    }
}
