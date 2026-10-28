//! The store book (pure): what the phone app's Store tab shows, kept from the shop watcher's events and its status,
//! owner and meso counter. The same record as the web page's (`docs/app/storebook.js`), in the same JSON shape, so
//! the phone reads either; the platform seals it (`phone/relay.js` `seal`) and uploads it to the relay.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{item_icon_url, picture};
use crate::market::catalog::Catalog;
use crate::render::status_pill::{text_for, tone_for, word_for};
use crate::shop::{ShopContext, ShopEvent, ShopStatus};

/// History kept, newest first: a day of a busy store, and a snapshot well under the relay's size limit.
const KEEP_SALES: usize = 150;
const KEEP_CHAT: usize = 80;
const KEEP_VISITS: usize = 80;
/// Past stores kept: a week of a player who opens a few a day.
const KEEP_SESSIONS: usize = 30;
/// History older than this is dropped (the phone shows today and yesterday apart, and a week's totals).
const MAX_AGE_MS: f64 = 7.0 * 24.0 * 3600.0 * 1000.0;
/// A store seen opening while the book still has one open continues it when that one was busy this recently:
/// MapleLens restarted (a page reload) over the same store, whose watcher reports it opening again.
const RESUME_WITHIN_MS: f64 = 30.0 * 60.0 * 1000.0;

/// The status line, as the Windows pill words it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub word: String,
    pub text: String,
    pub tone: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleEntry {
    /// Unix milliseconds, as JavaScript counts time.
    pub t: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qty: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buyer: Option<String>,
    pub mesos: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The balance moved while Sold Items was closed: something sold, the item unknown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unseen: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatEntry {
    pub t: f64,
    pub name: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VisitEntry {
    pub t: f64,
    pub name: String,
    #[serde(rename = "in")]
    pub came_in: bool,
}

/// A store from opening to closing. Its numbers are counted at closing from the history kept, so they are what its
/// page lists (until older entries are dropped).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub from: f64,
    pub to: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub sold: u64,
    pub earned: i64,
    /// Players who came in, each once.
    pub visitors: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Book {
    pub owner: Option<String>,
    pub open: bool,
    pub opened_at: Option<f64>,
    pub balance: Option<i64>,
    pub status: Option<Status>,
    pub in_store: Vec<String>,
    pub sales: Vec<SaleEntry>,
    pub chat: Vec<ChatEntry>,
    pub visits: Vec<VisitEntry>,
    /// Stores that closed, newest first.
    pub sessions: Vec<Session>,
}

impl Book {
    /// The newest moment the open store was known busy: its opening or its latest entry.
    fn last_activity(&self) -> Option<f64> {
        let latest =
            [self.sales.first().map(|e| e.t), self.chat.first().map(|e| e.t), self.visits.first().map(|e| e.t)];
        latest.into_iter().flatten().chain(self.opened_at).reduce(f64::max)
    }

    /// The open store, ended at `to`, into the past stores.
    fn end_session(&mut self, to: f64) {
        let Some(from) = self.opened_at.take() else { return };
        let within = |t: f64| (from..=to).contains(&t);
        let sales = self.sales.iter().filter(|e| within(e.t));
        let mut visitors: Vec<&str> =
            self.visits.iter().filter(|e| e.came_in && within(e.t)).map(|e| e.name.as_str()).collect();
        visitors.sort_unstable();
        visitors.dedup();
        let session = Session {
            from,
            to,
            owner: self.owner.clone(),
            sold: sales.clone().count() as u64,
            earned: sales.filter_map(|e| e.mesos).sum(),
            visitors: visitors.len() as u64,
        };
        self.sessions.insert(0, session);
    }
}

/// The book and whether the phones have seen its latest state.
#[derive(Clone, Debug, Default)]
pub struct StoreBook {
    pub book: Book,
    /// Something changed since the last upload.
    pub changed: bool,
}

impl StoreBook {
    /// The book as saved (`to_json`); who was inside when MapleLens closed is not known now.
    pub fn from_json(text: &str) -> Self {
        let mut book: Book = serde_json::from_str(text).unwrap_or_default();
        book.in_store.clear();
        Self { book, changed: false }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.book).unwrap_or_default()
    }

    /// One look: the watcher's status, owner, balance and events at `now_ms`. True when it changed the book.
    pub fn update(&mut self, status: ShopStatus, context: &ShopContext, events: &[ShopEvent], now_ms: f64) -> bool {
        let before = self.changed;
        self.changed = false;
        let status = text_for(status).map(|text| Status {
            word: word_for(status).into(),
            text: text.into(),
            tone: tone_for(status).unwrap_or("idle").into(),
        });
        if status != self.book.status {
            self.book.status = status;
            self.changed = true;
        }
        if context.owner.is_some() && context.owner != self.book.owner {
            self.book.owner.clone_from(&context.owner);
            self.changed = true;
        }
        if context.balance.is_some() && context.balance != self.book.balance {
            self.book.balance = context.balance;
            self.changed = true;
        }
        for event in events {
            self.add(event, now_ms);
        }
        let changed = self.changed;
        self.changed |= before;
        changed
    }

    fn add(&mut self, event: &ShopEvent, t: f64) {
        let book = &mut self.book;
        match event {
            // A sale of an item with the game's id has its picture at once, whatever is notified; others get one once
            // found by name (`set_picture`).
            ShopEvent::Sale { item, buyer, quantity, mesos, item_id } => book.sales.insert(
                0,
                SaleEntry {
                    t,
                    item: Some(item.clone()),
                    qty: Some((*quantity).max(1)),
                    buyer: Some(buyer.clone()),
                    mesos: *mesos,
                    icon: item_id.map(item_icon_url),
                    ..SaleEntry::default()
                },
            ),
            ShopEvent::SaleUnseen { mesos } => {
                book.sales.insert(0, SaleEntry { t, mesos: *mesos, unseen: true, ..SaleEntry::default() });
            }
            ShopEvent::PlayerEntered(name) => {
                if !book.in_store.contains(name) {
                    book.in_store.push(name.clone());
                }
                book.visits.insert(0, VisitEntry { t, name: name.clone(), came_in: true });
            }
            ShopEvent::PlayerLeft(name) => {
                book.in_store.retain(|n| n != name);
                book.visits.insert(0, VisitEntry { t, name: name.clone(), came_in: false });
            }
            ShopEvent::ChatMessage(text) => {
                // The game's chat lines read "Name : message".
                let entry = match text.split_once(" : ") {
                    Some((name, said)) if !name.is_empty() => {
                        ChatEntry { t, name: Some(name.into()), text: said.into() }
                    }
                    _ => ChatEntry { t, name: None, text: text.clone() },
                };
                book.chat.insert(0, entry);
            }
            ShopEvent::StoreOpened => {
                let last = book.last_activity();
                let resumed = book.open && book.opened_at.is_some() && last.is_some_and(|l| t - l <= RESUME_WITHIN_MS);
                if !resumed {
                    // One the book still has open was not seen closing (MapleLens was off): it ended when last busy.
                    if book.open {
                        book.end_session(last.unwrap_or(t));
                    }
                    (book.open, book.opened_at) = (true, Some(t));
                }
            }
            ShopEvent::StoreClosed => {
                book.end_session(t);
                book.open = false;
                book.in_store.clear();
            }
            ShopEvent::AmbiguousSale | ShopEvent::LostSync(_) => return,
        }
        self.trim(t);
        self.changed = true;
    }

    fn trim(&mut self, now_ms: f64) {
        let fresh = |t: f64| now_ms - t < MAX_AGE_MS;
        let book = &mut self.book;
        book.sales.retain(|e| fresh(e.t));
        book.sales.truncate(KEEP_SALES);
        book.chat.retain(|e| fresh(e.t));
        book.chat.truncate(KEEP_CHAT);
        book.visits.retain(|e| fresh(e.t));
        book.visits.truncate(KEEP_VISITS);
        book.sessions.retain(|s| fresh(s.to));
        book.sessions.truncate(KEEP_SESSIONS);
    }

    /// The picture of an item of this name, for its sales without one.
    pub fn set_picture(&mut self, item: &str, url: &str) {
        for sale in &mut self.book.sales {
            if sale.item.as_deref() == Some(item) && sale.icon.is_none() {
                sale.icon = Some(url.into());
                self.changed = true;
            }
        }
    }

    /// Pictures for the sales kept without one whose item the price list names exactly (recorded before pictures
    /// came by id). True when it changed the book.
    pub fn fill_pictures(&mut self, catalog: &Catalog) -> bool {
        let mut names: Vec<String> = Vec::new();
        for sale in &self.book.sales {
            if let Some(item) = &sale.item
                && sale.icon.is_none()
                && !names.contains(item)
            {
                names.push(item.clone());
            }
        }
        let before = self.changed;
        self.changed = false;
        for name in names {
            if let Some(url) = picture::in_catalog(catalog, &name) {
                self.set_picture(&name, &url);
            }
        }
        let changed = self.changed;
        self.changed |= before;
        changed
    }

    /// What the phone gets: the book, when it was made, and whether MapleLens is following the store.
    pub fn snapshot(&self, following: bool, now_ms: f64) -> Value {
        let mut value = serde_json::to_value(&self.book).unwrap_or_else(|_| json!({}));
        if let Value::Object(map) = &mut value {
            map.insert("v".into(), json!(1));
            map.insert("at".into(), json!(now_ms));
            map.insert("following".into(), json!(following));
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(owner: &str, balance: i64) -> ShopContext {
        ShopContext { owner: Some(owner.into()), balance: Some(balance) }
    }

    #[test]
    fn events_become_the_phones_record() {
        let mut book = StoreBook::default();
        let events = [
            ShopEvent::StoreOpened,
            ShopEvent::PlayerEntered("Wina".into()),
            ShopEvent::ChatMessage("Wina : hi! how much?".into()),
            ShopEvent::Sale {
                item: "Arrow".into(),
                buyer: "Wina".into(),
                quantity: 100,
                mesos: Some(500),
                item_id: None,
            },
            ShopEvent::SaleUnseen { mesos: None },
            ShopEvent::PlayerLeft("Wina".into()),
        ];
        book.update(ShopStatus::Watching, &context("ShopOwner", 1_000), &events, 5_000.0);
        assert!(book.changed);
        book.set_picture("Arrow", "https://maplestory.io/api/x/item/1/icon");
        let snap = book.snapshot(true, 6_000.0);
        assert_eq!(snap["v"], 1);
        assert_eq!(snap["following"], true);
        assert_eq!(snap["owner"], "ShopOwner");
        assert_eq!(snap["balance"], 1_000);
        assert_eq!(snap["open"], true);
        assert_eq!(snap["openedAt"], 5_000.0);
        assert_eq!(snap["status"], json!({"word": "watching", "text": "Store detected", "tone": "ok"}));
        assert_eq!(snap["inStore"], json!([]));
        assert_eq!(snap["chat"][0], json!({"t": 5_000.0, "name": "Wina", "text": "hi! how much?"}));
        assert_eq!(snap["visits"][0], json!({"t": 5_000.0, "name": "Wina", "in": false}));
        assert_eq!(snap["sales"][0], json!({"t": 5_000.0, "mesos": null, "unseen": true}));
        let sale = &snap["sales"][1];
        assert_eq!(
            (sale["item"].as_str(), sale["qty"].as_u64(), sale["buyer"].as_str()),
            (Some("Arrow"), Some(100), Some("Wina"))
        );
        assert_eq!(sale["icon"], "https://maplestory.io/api/x/item/1/icon");
    }

    fn sale(item: &str, item_id: Option<u64>) -> ShopEvent {
        ShopEvent::Sale { item: item.into(), buyer: "Wina".into(), quantity: 2, mesos: Some(2_900), item_id }
    }

    #[test]
    fn a_sale_with_the_games_id_has_its_picture_whatever_is_notified() {
        let mut book = StoreBook::default();
        book.update(ShopStatus::Watching, &ShopContext::default(), &[sale("Iron Ingot", Some(4_010_101))], 1.0);
        assert_eq!(book.book.sales[0].icon.as_deref(), Some("https://maplestory.io/api/MCW/1/item/4010101/icon"));
    }

    #[test]
    fn sales_kept_without_a_picture_get_the_price_lists() {
        use crate::market::models::Item;
        let catalog = Catalog::new(
            [("Iron Ingot", "basil:04010101"), ("Iron Ore", "basil:04010001")]
                .map(|(name, id)| Item { item_id: id.into(), ..Item::named(name) }),
            None,
        );
        // As a book saved before pictures came by id holds them.
        let mut book = StoreBook::from_json(
            &json!({"sales": [
                {"t": 3.0, "item": "Iron Ingot", "qty": 2, "buyer": "Wina", "mesos": 2900},
                {"t": 2.0, "item": "Iron Ingot", "qty": 1, "buyer": "Tikko", "mesos": 1450},
                {"t": 1.5, "item": "Iron Ingo", "qty": 1, "buyer": "Tikko", "mesos": 1450},
                {"t": 1.0, "item": "Pet Food", "qty": 1, "buyer": "Tikko", "mesos": 600, "icon": "https://maplestory.io/api/x"},
                {"t": 0.5, "mesos": 600, "unseen": true},
            ]})
            .to_string(),
        );
        assert!(book.fill_pictures(&catalog));
        assert!(book.changed, "to upload");
        let icons: Vec<Option<&str>> = book.book.sales.iter().map(|s| s.icon.as_deref()).collect();
        let ingot = Some("https://maplestory.io/api/MCW/1/item/4010101/icon");
        assert_eq!(icons, [ingot, ingot, None, Some("https://maplestory.io/api/x"), None], "an exact name only");
        book.changed = false;
        assert!(!book.fill_pictures(&catalog), "nothing left to fill");
    }

    #[test]
    fn a_quiet_look_changes_nothing_and_the_book_survives_a_restart() {
        let mut book = StoreBook::default();
        book.update(ShopStatus::NeedSoldWindow, &context("ShopOwner", 7), &[ShopEvent::StoreOpened], 1.0);
        assert!(!book.update(ShopStatus::NeedSoldWindow, &ShopContext::default(), &[], 2.0), "nothing new");
        assert!(book.changed, "still not uploaded");
        book.changed = false;
        assert!(book.update(ShopStatus::Watching, &ShopContext::default(), &[], 3.0), "the status moved");
        assert!(book.changed);
        let again = StoreBook::from_json(&book.to_json());
        assert_eq!(again.book.owner.as_deref(), Some("ShopOwner"));
        assert_eq!(again.book.status, book.book.status);
        assert_eq!(StoreBook::from_json("not json").book, Book::default());
    }

    fn entered(name: &str) -> ShopEvent {
        ShopEvent::PlayerEntered(name.into())
    }

    #[test]
    fn a_closed_store_becomes_a_past_store_with_its_own_numbers() {
        let mut book = StoreBook::default();
        let shop = context("ShopOwner", 0);
        let mut at = |events: &[ShopEvent], t: f64| {
            book.update(ShopStatus::Watching, &shop, events, t);
            book.book.sessions.len()
        };
        at(&[sale("Before", None)], 500.0);
        at(&[ShopEvent::StoreOpened], 1_000.0);
        at(&[entered("Wina")], 2_000.0);
        at(&[sale("Arrow", None)], 3_000.0);
        at(&[ShopEvent::PlayerLeft("Wina".into()), entered("Wina"), entered("Tikko")], 3_500.0);
        at(&[ShopEvent::SaleUnseen { mesos: Some(100) }], 4_000.0);
        at(&[ShopEvent::StoreClosed], 5_000.0);
        at(&[ShopEvent::StoreOpened], 9_000.0);
        assert_eq!(at(&[sale("Pet Food", None)], 9_500.0), 1, "the open store is not a past one yet");
        at(&[ShopEvent::StoreClosed], 10_000.0);
        let owner = Some("ShopOwner".to_string());
        assert_eq!(
            book.book.sessions,
            [
                Session { from: 9_000.0, to: 10_000.0, owner: owner.clone(), sold: 1, earned: 2_900, visitors: 0 },
                Session { from: 1_000.0, to: 5_000.0, owner, sold: 2, earned: 3_000, visitors: 2 },
            ]
        );
        assert!(!book.book.open);
        assert_eq!(book.book.opened_at, None);
        let snap = book.snapshot(true, 11_000.0);
        assert_eq!(
            snap["sessions"][1],
            json!({"from": 1_000.0, "to": 5_000.0, "owner": "ShopOwner", "sold": 2, "earned": 3_000, "visitors": 2})
        );
        assert_eq!(StoreBook::from_json(&book.to_json()).book.sessions, book.book.sessions, "kept over a restart");
    }

    #[test]
    fn a_store_seen_opening_again_soon_after_is_the_same_store() {
        const MIN: f64 = 60_000.0;
        let mut book = StoreBook::default();
        let mut at = |events: &[ShopEvent], t: f64| {
            book.update(ShopStatus::Watching, &ShopContext::default(), events, t);
            (book.book.opened_at, book.book.sessions.clone())
        };
        at(&[ShopEvent::StoreOpened], 0.0);
        at(&[sale("Arrow", None)], 20.0 * MIN);
        // MapleLens restarted: its watcher sees the open store as opening.
        assert_eq!(at(&[ShopEvent::StoreOpened], 45.0 * MIN), (Some(0.0), vec![]));
        // Gone for hours: the store it had open ended when last busy, and this is another.
        let (opened_at, sessions) = at(&[ShopEvent::StoreOpened], 300.0 * MIN);
        assert_eq!(opened_at, Some(300.0 * MIN));
        assert_eq!((sessions[0].from, sessions[0].to, sessions[0].sold), (0.0, 20.0 * MIN, 1));
    }

    #[test]
    fn old_and_excess_history_is_dropped() {
        let mut book = StoreBook::default();
        let chat: Vec<ShopEvent> = (0..100).map(|i| ShopEvent::ChatMessage(format!("line {i}"))).collect();
        book.update(ShopStatus::Watching, &ShopContext::default(), &chat, 0.0);
        assert_eq!(book.book.chat.len(), KEEP_CHAT);
        let stores: Vec<ShopEvent> = (0..40).flat_map(|_| [ShopEvent::StoreOpened, ShopEvent::StoreClosed]).collect();
        book.update(ShopStatus::Watching, &ShopContext::default(), &stores, 1.0);
        assert_eq!(book.book.sessions.len(), KEEP_SESSIONS);
        book.update(ShopStatus::Watching, &ShopContext::default(), &[ShopEvent::StoreClosed], MAX_AGE_MS + 1.0);
        assert!(book.book.chat.is_empty(), "a week old");
        assert!(book.book.sessions.is_empty(), "a week old");
    }
}
