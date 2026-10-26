//! The Store Permit watcher in the browser: the same `ShopWatcher` as the Windows app, fed the shared window's frames.
//!
//! Text is read by the page's OCR, which is asynchronous, while the watcher asks for it synchronously. So a frame
//! whose text is not read yet is undone (the watcher is restored from a copy made before it) and tried again once
//! the answers are in: the sold list still shows the new rows then. Sales go to the notification connectors; the page
//! sends the requests this produces (the engine never touches the network).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use maplelens_core::image::RgbImage;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::notify::announce::Announcer;
use maplelens_core::notify::{Notification, NotifySettings, Request, connectors};
use maplelens_core::render::status_pill::{text_for, tone_for};
use maplelens_core::shop::watcher::{ReadText, ShopWatcher};
use maplelens_core::shop::{ShopEvent, ShopStatus};
use maplelens_core::vision::Mask;
use maplelens_core::vision::reader::{Ocr, OcrAnswer};
use serde_json::{Value, json};

use crate::ocr::{OcrQueue, QueuedOcr};

/// A shop changes a few times a minute at most: one look a second is plenty while one is on screen...
const POLL_S: f64 = 1.0;
/// ...and while none is, a full search of the frame costs too much to do often.
const IDLE_POLL_S: f64 = 4.0;
/// After a frame was undone for want of text, try again this soon (the answers take a moment).
const RETRY_S: f64 = 0.3;
/// Jobs waiting at once: a burst of sales needs three lines read for each, and dropping one starves the watcher.
const MAX_WAITING_JOBS: usize = 48;
/// Sales that come together are one message.
const BATCH_S: f64 = 1.5;

pub struct ShopRunner {
    watcher: ShopWatcher,
    ocr: Arc<Mutex<OcrQueue>>,
    /// Set by the watcher's reader when it was asked for text that is not read yet.
    missed: Arc<AtomicBool>,
    enabled: bool,
    ocr_on: bool,
    next_poll: f64,
    notify: NotifySettings,
    pending: Vec<Notification>,
    pending_since: f64,
    outbox: Vec<Request>,
    announcer: Announcer,
    /// How many looks were made, and how many of them were undone for want of text (for diagnosing a page).
    pub looks: u32,
    pub undone: u32,
}

impl ShopRunner {
    pub fn new() -> Self {
        let ocr = OcrQueue::shared_with(false, MAX_WAITING_JOBS);
        let missed = Arc::new(AtomicBool::new(false));
        let (reader_ocr, reader_missed) = (QueuedOcr(ocr.clone()), missed.clone());
        let mut engine = reader_ocr;
        let read: ReadText = Box::new(move |mask: &Mask| match engine.recognize(mask) {
            OcrAnswer::Text(text) => Some(text.trim().to_string()),
            OcrAnswer::Pending => {
                reader_missed.store(true, Ordering::Relaxed);
                None
            }
            // No reader: the text stays unknown (the store is still followed).
            _ => Some(String::new()),
        });
        Self {
            watcher: ShopWatcher::new(read),
            ocr,
            missed,
            enabled: false,
            ocr_on: false,
            next_poll: 0.0,
            notify: NotifySettings::default(),
            pending: Vec::new(),
            pending_since: 0.0,
            outbox: Vec::new(),
            announcer: Announcer::default(),
            looks: 0,
            undone: 0,
        }
    }

    /// Sold items are named as the catalog names them, and their pictures found by its ids.
    pub fn set_catalog(&mut self, catalog: Arc<Catalog>) {
        self.watcher.set_catalog(catalog);
    }

    pub fn queue(&self) -> &Arc<Mutex<OcrQueue>> {
        &self.ocr
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Whether the page can read text: without it the store is followed (the status) but nothing it says is.
    pub fn set_ocr(&mut self, on: bool) {
        self.ocr_on = on;
        if let Ok(mut queue) = self.ocr.lock() {
            queue.set_enabled(on);
        }
    }

    pub fn set_notify(&mut self, settings: NotifySettings) {
        self.notify = settings;
        self.pending.clear();
    }

    pub fn status(&self) -> ShopStatus {
        self.watcher.status()
    }

    /// The Character Info window's name, to settle the spelling of what the plate reads.
    pub fn suggest_owner(&mut self, name: Option<&str>) {
        self.watcher.suggest_owner(name);
    }

    /// Whose store it is, when known.
    pub fn owner(&self) -> Option<String> {
        self.watcher.context().owner
    }

    /// The store's meso counter as last read.
    pub fn balance(&self) -> Option<i64> {
        self.watcher.context().balance
    }

    pub fn owner_reads(&self) -> Vec<String> {
        self.watcher.owner_reads()
    }

    pub fn waiting_jobs(&self) -> usize {
        self.ocr.lock().map_or(0, |q| q.waiting())
    }

    /// One look at the frame (the game's client area, any size: the shop measures its own UI scale), if one is due;
    /// what newly happened.
    pub fn step(&mut self, frame: impl FnOnce() -> RgbImage, now: f64, unix_now: f64) -> Vec<ShopEvent> {
        let mut events = Vec::new();
        if self.enabled && now >= self.next_poll {
            let backup = self.ocr_on.then(|| self.watcher.clone());
            self.missed.store(false, Ordering::Relaxed);
            self.looks += 1;
            events = self.watcher.update(&frame(), now);
            let missed = self.missed.load(Ordering::Relaxed);
            self.next_poll = now
                + if missed {
                    RETRY_S
                } else if self.watcher.sees_shop() {
                    POLL_S
                } else {
                    IDLE_POLL_S
                };
            match backup {
                // Undo the frame when a sale or a visitor's text is not in yet; the events come when it has been read.
                // Text the watcher reads for itself (the owner's name) is asked again on the next look instead, so
                // the store is followed while the page's OCR is still loading.
                Some(before) if self.watcher.missing_text() => {
                    self.undone += 1;
                    self.watcher = before;
                    events.clear();
                }
                // No text reader: nothing said in the shop can be read, so only what the pixels alone tell is kept.
                None => events
                    .retain(|e| matches!(e, ShopEvent::StoreOpened | ShopEvent::StoreClosed | ShopEvent::LostSync(_))),
                _ => {}
            }
        }
        // Every call, events or none: a store found open waits a moment for its owner's name before it is announced.
        let sales = self.announcer.feed(&events, &self.watcher.context(), now);
        if !sales.is_empty() {
            if self.pending.is_empty() {
                self.pending_since = now;
            }
            self.pending.extend(sales);
        }
        self.flush(now, unix_now);
        events
    }

    /// Batches that have waited long enough become requests.
    fn flush(&mut self, now: f64, unix_now: f64) {
        if self.pending.is_empty() || now - self.pending_since < BATCH_S {
            return;
        }
        let mut batch = std::mem::take(&mut self.pending);
        batch.retain(|n| self.notify.allows(n));
        if batch.is_empty() {
            return;
        }
        for connector in connectors(&self.notify) {
            self.outbox.extend(connector.requests(&batch, unix_now));
        }
    }

    /// The requests the page should send, as `[{"url", "body"}]` JSON.
    pub fn take_requests(&mut self) -> String {
        let requests: Vec<Value> =
            std::mem::take(&mut self.outbox).into_iter().map(|r| json!({"url": r.url, "body": r.body})).collect();
        Value::Array(requests).to_string()
    }

    /// The test message of the first connector, as `{"url", "body"}` JSON, for settings not yet in use.
    pub fn test_request(&self, settings: &NotifySettings, unix_now: f64) -> Option<String> {
        let owner = self.owner();
        let owner = owner.as_deref();
        let connector = connectors(settings).into_iter().next()?;
        let request = connector.test_request(owner, unix_now);
        Some(json!({"url": request.url, "body": request.body}).to_string())
    }
}

/// An event as the page gets it.
pub fn event_json(event: &ShopEvent) -> Value {
    match event {
        ShopEvent::Sale { item, buyer, quantity, mesos, .. } => {
            json!({"kind": "sale", "item": item, "buyer": buyer, "quantity": quantity, "mesos": mesos})
        }
        ShopEvent::SaleUnseen { mesos } => json!({"kind": "sale-unseen", "mesos": mesos}),
        ShopEvent::AmbiguousSale => json!({"kind": "sale-ambiguous"}),
        ShopEvent::PlayerEntered(name) => json!({"kind": "entered", "name": name}),
        ShopEvent::PlayerLeft(name) => json!({"kind": "left", "name": name}),
        ShopEvent::ChatMessage(text) => json!({"kind": "chat", "text": text}),
        ShopEvent::StoreOpened => json!({"kind": "opened"}),
        ShopEvent::StoreClosed => json!({"kind": "closed"}),
        ShopEvent::LostSync(_) => json!({"kind": "lost-sync"}),
    }
}

/// The status for the page: a word to act on, and the pill's own text and tone so the browser says what the Windows
/// app says.
pub fn status_json(status: ShopStatus) -> Value {
    json!({
        "status": status_word(status),
        "text": text_for(status),
        "tone": tone_for(status),
    })
}

fn status_word(status: ShopStatus) -> &'static str {
    match status {
        ShopStatus::Hidden => "hidden",
        ShopStatus::NotOpen => "not-open",
        ShopStatus::Watching => "watching",
        ShopStatus::NeedSoldWindow => "need-sold-window",
        ShopStatus::Scrolled => "scrolled",
        ShopStatus::Covered => "covered",
    }
}
