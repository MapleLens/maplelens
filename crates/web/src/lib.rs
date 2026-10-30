//! MapleLens in the browser: the same detection, reading and cards as the Windows app, driven frame by frame
//! from JavaScript (docs/app). The page captures the game window with screen sharing and shows the cards in a
//! picture-in-picture window; this engine never touches the network itself (the page fetches ranks and runs
//! the OCR fallback, and hands the answers back).

mod ocr;
mod shopwatch;

use std::sync::{Arc, Mutex};

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::RgbImage;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::format::format_mesos;
use maplelens_core::market::sample::sample_items;
use maplelens_core::market::snapshot::parse_snapshot;
use maplelens_core::notify::{NotifySettings, discord, phone, refusal, telegram};
use maplelens_core::pill::{Facts, PillPicker};
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::{Hover, HoverDetector};
use maplelens_core::pipeline::info::{InfoDetector, InfoWindow};
use maplelens_core::ranking::{DEFAULT_RETRY_S, Fetched, RankCache, RankResult, Status, parse_response, rank_url};
use maplelens_core::render::RgbaImage;
use maplelens_core::render::card::{CardSpec, render_card};
use maplelens_core::render::font::Fonts;
use maplelens_core::render::rank_card::render_rank_card;
use maplelens_core::render::status_pill::{text_for, tone_for};
use maplelens_core::render::training_card::{Show, is_training, render_training_card};
use maplelens_core::shop::ShopStatus;
use maplelens_core::training::potions;
use maplelens_core::training::session::Tracker;
use maplelens_core::training::watcher::{Training, TrainingWatcher};
use maplelens_core::vision::glyphs::Atlas;
use maplelens_core::vision::reader::{TextReader, name_atlas, title_atlas};
use ocr::{OcrQueue, QueuedImageOcr, QueuedOcr};
use serde_json::{Value, json};
use shopwatch::{ShopRunner, event_json, status_json};
use wasm_bindgen::prelude::*;

/// Frames arrive about 10 times a second, not 120: a tooltip missing from a frame or two is still there.
const HIDE_GRACE_S: f64 = 0.5;
/// The Character Info window search is the expensive part (80-200 ms of a frame in WebAssembly, so every few frames
/// the picture stuttered): while no window is known, one horizontal band of the frame is searched this often, and
/// the bands together cover the whole frame in `INFO_BANDS` steps (a window that opens is found within ~1.2 s).
const INFO_BAND_S: f64 = 0.3;
const INFO_BANDS: usize = 4;
/// Neighbouring bands overlap by this much (reference px, scaled) so a title bar cut by a seam is whole in one.
const INFO_BAND_OVERLAP: i32 = 24;
/// Keep the pointer on one item this long and the glance card grows into the full one.
const EXPAND_DELAY_S: f64 = 1.2;

/// A clock for timing the engine's parts (milliseconds): `performance.now()` in the browser, the process's own natively
/// (the tests).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn clock_ms() -> f64;
}

#[cfg(not(target_arch = "wasm32"))]
fn clock_ms() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// A rendered card and what it shows, so it is only redrawn when that changes.
struct Drawn<K> {
    key: K,
    image: RgbaImage,
    version: u32,
}

/// The rank panel's subject as `Engine::rank_about` describes it.
fn rank_json(read: &str, result: &RankResult) -> Value {
    let status = match result.status {
        Status::Loading => "loading",
        Status::Found => "found",
        Status::Missing => "missing",
        Status::Failed => "failed",
    };
    let rank = result.rank.as_ref().map(|r| {
        json!({
            "name": r.name, "level": r.level, "job": r.job, "fame": r.fame,
            "overall_rank": r.overall_rank, "job_rank": r.job_rank, "fame_rank": r.fame_rank, "botting": r.botting,
            "last_checked": r.last_checked, "profile_url": r.profile_url, "photo": r.photo,
        })
    });
    json!({"read": read, "status": status, "rank": rank})
}

/// Band `index` of `INFO_BANDS` of the game's area, each overlapping its neighbours.
fn info_band(bounds: Rect, index: usize, scale: f32) -> Rect {
    let overlap = (INFO_BAND_OVERLAP as f32 * scale).ceil() as i32;
    let step = bounds.h / INFO_BANDS as i32;
    let top = (bounds.y + step * index as i32 - overlap).max(bounds.y);
    let bottom = if index + 1 == INFO_BANDS { bounds.bottom() } else { bounds.y + step * (index as i32 + 1) + overlap };
    Rect::new(bounds.x, top, bounds.w, (bottom - top).max(0))
}

#[wasm_bindgen]
pub struct Engine {
    fonts: Fonts,
    catalog: Arc<Catalog>,
    hover: HoverDetector,
    info: InfoDetector,
    title_ocr: Arc<Mutex<OcrQueue>>,
    name_ocr: Arc<Mutex<OcrQueue>>,
    ranks: RankCache,
    last_hover: Option<(Hover, f64)>,
    last_info: Option<(InfoWindow, f64)>,
    next_info_search: f64,
    info_band: usize,
    shop: ShopRunner,
    training: TrainingWatcher,
    map_ocr: Arc<Mutex<OcrQueue>>,
    training_show: Show,
    /// The training hints of the Windows app's status pill; the page shows the store's status first.
    pill: PillPicker,
    training_card: Option<Drawn<(Training, Show, u64)>>,
    since: f64,
    card: Option<Drawn<(String, bool)>>,
    /// What the price card is about, as JSON for the page's list of recent items.
    card_about: String,
    rank_card: Option<Drawn<(String, String)>>,
    /// Who the rank panel is about and their ranks, as JSON: the page draws its own panel from it.
    rank_about: String,
    versions: u32,
    /// Device pixels per CSS pixel where the cards are shown.
    pub card_scale: f32,
}

fn js_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

#[wasm_bindgen]
impl Engine {
    /// An engine with the built-in demo prices and the card font (regular and bold TrueType bytes).
    #[wasm_bindgen(constructor)]
    pub fn new(regular: Vec<u8>, bold: Vec<u8>) -> Result<Engine, JsError> {
        let fonts = Fonts::from_bytes(regular, bold).map_err(js_error)?;
        let catalog = Arc::new(Catalog::new(sample_items(), None));
        let (title_ocr, name_ocr) = (OcrQueue::shared(false), OcrQueue::shared(true));
        let title_reader = TextReader::new(title_atlas(), Box::new(QueuedOcr(title_ocr.clone())));
        let name_reader = TextReader::new(name_atlas(), Box::new(QueuedOcr(name_ocr.clone())));
        let map_ocr = OcrQueue::shared(false);
        let training = TrainingWatcher::new(Box::new(QueuedImageOcr(map_ocr.clone())), Tracker::new());
        Ok(Engine {
            fonts,
            hover: HoverDetector::new(catalog.clone(), title_reader),
            info: InfoDetector::new(name_reader),
            catalog,
            title_ocr,
            name_ocr,
            ranks: RankCache::default(),
            last_hover: None,
            last_info: None,
            next_info_search: 0.0,
            info_band: 0,
            shop: ShopRunner::new(),
            training,
            map_ocr,
            training_show: Show { exp: true, quest: false },
            pill: PillPicker::default(),
            training_card: None,
            since: 0.0,
            card: None,
            card_about: "{}".into(),
            rank_card: None,
            rank_about: "{}".into(),
            versions: 0,
            card_scale: 1.0,
        })
    }

    /// Replace the demo prices with a MesoWatch-format snapshot. Returns the number of items.
    pub fn load_catalog(&mut self, json: &str) -> Result<u32, JsError> {
        let snapshot = parse_snapshot(json).map_err(js_error)?;
        self.catalog = Arc::new(Catalog::new(snapshot.items, snapshot.generated_at));
        self.hover.set_catalog(self.catalog.clone());
        self.shop.set_catalog(self.catalog.clone());
        self.card = None;
        Ok(self.catalog.len() as u32)
    }

    /// Letters learnt in an earlier visit (the page keeps them in localStorage), for "title" or "name".
    pub fn learn_glyphs(&mut self, font: &str, json: &str) -> Result<u32, JsError> {
        let learnt = Atlas::from_json(json).map_err(js_error)?;
        let reader = self.reader(font).ok_or_else(|| js_error("unknown font"))?;
        let mut atlas = reader.atlas().clone();
        let added = atlas.merge(&learnt) as u32;
        reader.replace_atlas(atlas);
        Ok(added)
    }

    /// Whether the page can recognise images the atlas cannot read (the OCR fallback): without it no jobs are made.
    pub fn set_ocr(&mut self, enabled: bool) {
        for queue in [&self.title_ocr, &self.name_ocr, &self.map_ocr] {
            if let Ok(mut queue) = queue.lock() {
                queue.set_enabled(enabled);
            }
        }
        self.shop.set_ocr(enabled);
        self.hover.reader_mut().forget_reads();
        self.info.reader_mut().forget_reads();
    }

    /// Follow the player's Store Permit (sales, visitors, the store opening and closing) in the shared window.
    pub fn set_shop(&mut self, enabled: bool) {
        self.shop.set_enabled(enabled);
    }

    /// Where sold items and the store opening or closing are reported: a Discord webhook URL, a Telegram bot's
    /// `sendMessage` URL with its `chat_id` (anything else is ignored, and a Telegram URL without its chat sends
    /// nothing yet) and a phone pairing key, and whether to report sales and store changes.
    pub fn set_notify(&mut self, webhook: &str, telegram_webhook: &str, phone_key: &str, sales: bool, store: bool) {
        let discord_webhook = discord::valid_webhook(webhook).unwrap_or_default().to_string();
        let (bot, telegram_chat) = telegram::parse_webhook(telegram_webhook).unwrap_or_default();
        let telegram_bot = bot.to_string();
        let phone_key = phone::valid_key(phone_key).unwrap_or_default().to_string();
        self.shop.set_notify(NotifySettings { discord_webhook, telegram_bot, telegram_chat, phone_key, sales, store });
    }

    /// The QR code of a phone pairing address (the page builds it, so a copy of the site pairs with itself):
    /// `width * width` bytes, 1 = dark, row by row, without the quiet zone; empty for anything but an https address
    /// carrying a pairing key.
    pub fn phone_qr(url: &str) -> Vec<u8> {
        let key = url.split_once("?pair=").map(|(_, key)| key).unwrap_or_default();
        if !url.starts_with("https://") || phone::valid_key(key) != Some(key) {
            return Vec::new();
        }
        phone::qr_modules(url).map(|(_, modules)| modules.into_iter().map(u8::from).collect()).unwrap_or_default()
    }

    /// The test message for a phone pairing key as `{"service", "url", "body"}` JSON, or undefined when it is not one.
    pub fn phone_test_request(&self, key: &str, unix_now: f64) -> Option<String> {
        phone::valid_key(key)?;
        let settings = NotifySettings { phone_key: key.to_string(), sales: true, store: true, ..Default::default() };
        self.shop.test_request(&settings, unix_now)
    }

    /// The webhook URL as it may be shown (its token hidden), or "" when it is not a Discord webhook.
    pub fn webhook_masked(webhook: &str) -> String {
        discord::masked(webhook)
    }

    /// HTTP requests to send now, `[{"service", "url", "body", "fallback"?}]` JSON (POST, JSON): sales that came in
    /// since the last call; `fallback` (`{"url", "body"}`) goes instead if the service refuses the request.
    pub fn take_requests(&mut self) -> String {
        self.shop.take_requests()
    }

    /// Sold items whose picture to look up first, `[{"name", "url"}]` JSON (GET maplestory.io); each answer's text
    /// goes to `picture_found`. The sales wait a few seconds for them.
    pub fn take_picture_lookups(&mut self) -> String {
        self.shop.take_picture_lookups()
    }

    pub fn picture_found(&mut self, name: &str, answer: &str) {
        self.shop.picture_found(name, answer);
    }

    /// The picture of an item of this name once a lookup found it, for the page's store log and the phone app.
    pub fn picture_url(&self, name: &str) -> Option<String> {
        self.shop.picture_url(name)
    }

    /// The picture of the price list's item of exactly this name, for sales the page kept without one.
    pub fn catalog_picture(&self, name: &str) -> Option<String> {
        maplelens_core::notify::picture::in_catalog(&self.catalog, name)
    }

    /// The test message for `webhook` as `{"service", "url", "body"}` JSON, or undefined when it is not a Discord
    /// webhook.
    pub fn notify_test_request(&self, webhook: &str, unix_now: f64) -> Option<String> {
        let settings =
            NotifySettings { discord_webhook: webhook.to_string(), sales: true, store: true, ..Default::default() };
        self.shop.test_request(&settings, unix_now)
    }

    /// The test message for a Telegram webhook URL as `{"service", "url", "body"}` JSON, or undefined when it is not
    /// one or has no chat yet.
    pub fn telegram_test_request(&self, url: &str, unix_now: f64) -> Option<String> {
        let (bot, telegram_chat) = telegram::parse_webhook(url)?;
        let settings = NotifySettings {
            telegram_bot: bot.to_string(),
            telegram_chat,
            sales: true,
            store: true,
            ..Default::default()
        };
        self.shop.test_request(&settings, unix_now)
    }

    /// The request asking the bot in `url` (a webhook URL or a token) who wrote to it lately, as `{"url", "body"}` JSON
    /// (POST); its answer goes to `telegram_linked`.
    pub fn telegram_link_request(url: &str) -> Option<String> {
        let (bot, _) = telegram::parse_webhook(url)?;
        telegram::link_request(bot).map(|r| serde_json::json!({"url": r.url, "body": r.body}).to_string())
    }

    /// The whole webhook URL of the bot in `url` with the chat that last wrote to it, from the answer to
    /// `telegram_link_request`; "" when none did lately.
    pub fn telegram_linked(url: &str, answer: &str) -> String {
        match (telegram::parse_webhook(url), telegram::chat_from_updates(answer)) {
            (Some((bot, _)), Some(chat)) => telegram::webhook_url(bot, chat),
            _ => String::new(),
        }
    }

    /// What was pasted as a Telegram webhook URL in its one form ("" when it holds no bot), for the page to keep.
    pub fn telegram_webhook(text: &str) -> String {
        telegram::parse_webhook(text).map(|(bot, chat)| telegram::webhook_url(bot, chat)).unwrap_or_default()
    }

    /// A service's refusal (HTTP `status`) in words for the player.
    pub fn notify_refusal(service: &str, status: u16) -> String {
        refusal(service, status)
    }

    /// The atlas of `font` as JSON when it learnt something since the last call, else undefined.
    pub fn learnt_glyphs(&mut self, font: &str) -> Option<String> {
        let reader = self.reader(font)?;
        reader.take_learned().then(|| reader.atlas().to_json())
    }

    fn reader(&mut self, font: &str) -> Option<&mut TextReader> {
        match font {
            "title" => Some(self.hover.reader_mut()),
            "name" => Some(self.info.reader_mut()),
            _ => None,
        }
    }

    /// One frame of the game window (RGBA, as a canvas gives it). `top_inset` rows at the top are the window's
    /// title bar, not the game; `scale` is the game's UI scale (0 = from the game's height). `now` is seconds on
    /// any steady clock, `unix_now` seconds since the epoch. Returns the state as JSON.
    #[allow(clippy::too_many_arguments)] // flat numbers are what JavaScript passes most cheaply across the boundary
    pub fn process(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        top_inset: u32,
        scale: f32,
        now: f64,
        unix_now: f64,
    ) -> String {
        let t0 = clock_ms();
        let frame = RgbImage::from_4ch(width as usize, height as usize, rgba, false);
        let inset = top_inset.min(height) as i32;
        let bounds = Rect::new(0, inset, width as i32, height as i32 - inset);
        let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
        let t1 = clock_ms();
        // Without a scale given, it is measured off the status bar ("UI Size: Ideal Ratio" keeps the UI small at
        // any window size), falling back to the window height.
        let hover = if scale > 0.0 {
            self.hover.step_at_scale(&mut screen, bounds, scale, now)
        } else {
            self.hover.step(&mut screen, bounds, now)
        };
        if let Some(hover) = hover {
            self.last_hover = Some((hover, now));
        }
        let t2 = clock_ms();
        let scale = if scale > 0.0 { scale } else { self.hover.ui_scale().of(bounds) };
        let known_window = self.last_info.as_ref().is_some_and(|(_, seen)| now - seen <= HIDE_GRACE_S);
        // A known window is re-checked in place (a tiny grab); looking for one is spread over the frame's bands.
        let search_area = if known_window {
            Some(bounds)
        } else if now >= self.next_info_search {
            self.next_info_search = now + INFO_BAND_S;
            let band = self.info_band;
            self.info_band = (self.info_band + 1) % INFO_BANDS;
            Some(info_band(bounds, band, scale))
        } else {
            None
        };
        if let Some(area) = search_area
            && let Some(info) = self.info.step_at_scale(&mut screen, area, scale)
        {
            self.last_info = Some((info, now));
        }
        let t3 = clock_ms();
        let hover = self.last_hover.as_ref().filter(|(_, seen)| now - seen <= HIDE_GRACE_S).map(|(h, _)| h.clone());
        let info = self.last_info.as_ref().filter(|(_, seen)| now - seen <= HIDE_GRACE_S).map(|(i, _)| i.clone());
        // The Character Info window's name (checked against the rank list) settles how the name plate is spelled.
        self.shop.suggest_owner(info.as_ref().and_then(|i| i.name.as_deref()));
        self.update_card(hover.as_ref(), now, unix_now);
        self.update_rank_card(info.as_ref(), now, unix_now);
        let t4 = clock_ms();
        // The shop, when the page asked for it, and only on the frames it is due. It gets the whole frame: it finds its
        // window by template wherever it is, and the page's guess at the title bar (which cycles while nothing else is
        // detected) would move the window under it and make it forget what it had seen.
        let shop_events: Vec<Value> = self.shop.step(|| frame.clone(), now, unix_now).iter().map(event_json).collect();
        let t5 = clock_ms();
        // EXP off the status bar and the map off the minimap, when due (it paces itself), against the wall clock:
        // sessions are saved with their dates.
        self.training.step(&mut screen, bounds, unix_now);
        let training = self.training.training(unix_now);
        self.update_training_card(&training, unix_now);
        // No drops panel here, and the page puts the store's own status first.
        let facts = Facts {
            store: ShopStatus::Hidden,
            exp_card: self.training_show.exp,
            map_drops: false,
            training: Some(&training),
            no_drops: None,
        };
        let hint =
            self.pill.pick(&facts, unix_now).and_then(|p| Some(json!({"text": text_for(p)?, "tone": tone_for(p)})));
        let t6 = clock_ms();
        let pending_ocr = self.title_ocr.lock().map_or(0, |q| q.waiting())
            + self.name_ocr.lock().map_or(0, |q| q.waiting())
            + self.map_ocr.lock().map_or(0, |q| q.waiting())
            + self.shop.waiting_jobs();
        json!({
            "scale": scale,
            "hover": hover.as_ref().map(|h| json!({
                "title": h.title,
                "known": h.item.as_ref().is_some_and(|i| i.price.is_some()),
                "at": h.found_at.map(|p| [p.x, p.y]),
                "size": [h.tooltip_size.0, h.tooltip_size.1],
                "compare": h.compare,
            })),
            "info": info.as_ref().map(|i| json!({"name": i.name, "rect": [i.rect.x, i.rect.y, i.rect.w, i.rect.h]})),
            "card": self.card.as_ref().filter(|_| hover.is_some()).map(|c| c.version),
            "rank": self.rank_card.as_ref().filter(|_| info.as_ref().is_some_and(|i| i.name.is_some())).map(|c| c.version),
            "ocr": pending_ocr,
            "tooltip_in_view": self.hover.tooltip_in_view(),
            "training": training_json(&training),
            "hint": hint,
            "potion_icons": self.training.potion_icons(),
            "training_card": self.training_card.as_ref().map(|c| c.version),
            // Where the frame's time went (ms): converting the pixels, the tooltip, the Character Info window, the cards,
            // the shop, the training reads.
            "parts": [t1 - t0, t2 - t1, t3 - t2, t4 - t3, t5 - t4, t6 - t5],
            "shop": {"status": status_json(self.shop.status()), "owner": self.shop.owner(), "balance": self.shop.balance(), "events": shop_events, "looks": self.shop.looks, "undone": self.shop.undone, "owner_reads": self.shop.owner_reads()},
        })
        .to_string()
    }

    /// Whether the training card shows the EXP rates. The party quest helper is hidden: never on the page's card.
    pub fn set_training_show(&mut self, exp: bool) {
        self.training_show = Show { exp, quest: false };
    }

    /// Pause or resume the training session (neither time nor EXP counts while paused).
    pub fn training_pause(&mut self, paused: bool) {
        self.training.tracker_mut().set_paused(paused);
    }

    /// End the training session (into the history) and start a new one.
    pub fn training_new_session(&mut self, unix_now: f64) {
        self.training.tracker_mut().new_session(unix_now);
    }

    /// Where the page gets the list of healing and buff items (maplestory.io's Classic data).
    pub fn potion_list_url() -> String {
        potions::list_url()
    }

    /// The healing and buff items in that list, as `[[id, name, icon url]]` JSON.
    pub fn potion_candidates(json: &str) -> String {
        let items: Vec<Value> =
            potions::candidates(json).into_iter().map(|(id, name)| json!([id, name, potions::icon_url(id)])).collect();
        Value::Array(items).to_string()
    }

    /// An item's icon (its PNG), so the quickslots can tell which item a slot holds. False when unreadable.
    pub fn add_potion_icon(&mut self, id: u32, name: &str, png: &[u8]) -> bool {
        self.training.add_potion_icon(id, name, png)
    }

    /// Sessions saved by an earlier visit (the page keeps `training_save()` in localStorage).
    pub fn load_training(&mut self, json: &str) {
        *self.training.tracker_mut() = Tracker::from_json(json);
    }

    /// The session and its history, for the page to keep.
    pub fn training_save(&self) -> String {
        self.training.tracker().to_json()
    }

    /// Finished sessions, oldest first, as JSON.
    pub fn training_history(&self) -> String {
        serde_json::to_string(self.training.tracker().history()).unwrap_or_else(|_| "[]".into())
    }

    pub fn training_width(&self) -> u32 {
        self.training_card.as_ref().map_or(0, |c| c.image.width as u32)
    }

    pub fn training_height(&self) -> u32 {
        self.training_card.as_ref().map_or(0, |c| c.image.height as u32)
    }

    pub fn training_pixels(&self) -> Vec<u8> {
        self.training_card.as_ref().map_or_else(Vec::new, |c| c.image.data.clone())
    }

    fn next_version(&mut self) -> u32 {
        self.versions += 1;
        self.versions
    }

    fn update_card(&mut self, hover: Option<&Hover>, now: f64, unix_now: f64) {
        let Some(hover) = hover else { return };
        if self.card.as_ref().is_none_or(|c| c.key.0 != hover.title) {
            self.since = now; // a new item: start with the glance card
        }
        let expanded = now - self.since >= EXPAND_DELAY_S;
        let key = (hover.title.clone(), expanded);
        if self.card.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let item = self.catalog.lookup(&hover.title).or_else(|| hover.item.clone());
        let spec = CardSpec {
            item: item.as_deref(),
            title: &hover.title,
            generated_at: self.catalog.generated_at.as_deref(),
            now: unix_now,
            expanded,
            ui_scale: self.card_scale,
            // The wide card is for the app, where it sits under two tooltips; the page's card panel is narrow, so a
            // compared item gets the same tall card as any other.
            wide: false,
            source: if self.catalog.generated_at.is_some() { "meso.watch" } else { "demo data" },
        };
        let image = render_card(&self.fonts, &spec);
        let version = self.next_version();
        self.card = Some(Drawn { key, image, version });
        self.card_about = json!({
            "name": item.as_ref().map_or(hover.title.as_str(), |i| i.name.as_str()),
            "price": item.as_ref().filter(|i| i.price.is_some())
                .map(|i| format!("{}{}", if i.low_data { "~" } else { "" }, format_mesos(i.price))),
            "trend": item.as_ref().and_then(|i| i.trend),
        })
        .to_string();
    }

    /// Redrawn when the numbers change, and every second (the card's clocks).
    fn update_training_card(&mut self, training: &Training, unix_now: f64) {
        let key = (training.clone(), self.training_show, unix_now as u64);
        if self.training_card.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        match render_training_card(&self.fonts, training, self.training_show, unix_now, self.card_scale) {
            Some(image) => {
                let version = self.next_version();
                self.training_card = Some(Drawn { key, image, version });
            }
            None => self.training_card = None,
        }
    }

    fn update_rank_card(&mut self, info: Option<&InfoWindow>, now: f64, unix_now: f64) {
        let Some(name) = info.and_then(|i| i.name.clone()) else { return };
        let result = self.ranks.get(&name, now);
        if let (Some(rank), Some(info)) = (&result.rank, info)
            && rank.name.eq_ignore_ascii_case(&name)
        {
            // classicrank.com knows this exact name, so the read was right: learn its letters.
            self.info.reader_mut().confirm(&info.read, &rank.name);
        }
        let key = (name, format!("{:?}{:?}", result.status, result.rank.as_ref().map(|r| &r.last_checked)));
        if self.rank_card.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        self.rank_about = rank_json(&key.0, &result).to_string();
        let image = render_rank_card(&self.fonts, &result, unix_now, self.card_scale);
        let version = self.next_version();
        self.rank_card = Some(Drawn { key, image, version });
    }

    pub fn card_width(&self) -> u32 {
        self.card.as_ref().map_or(0, |c| c.image.width as u32)
    }

    pub fn card_height(&self) -> u32 {
        self.card.as_ref().map_or(0, |c| c.image.height as u32)
    }

    /// The price card as straight RGBA (ready for `ImageData`).
    pub fn card_pixels(&self) -> Vec<u8> {
        self.card.as_ref().map_or_else(Vec::new, |c| c.image.data.clone())
    }

    /// The item the price card is about, `{"name", "price", "trend"}` JSON: the catalog's name (the read title when
    /// it names nothing), the price as the card writes it (null without sales), the week's trend as a fraction.
    pub fn card_about(&self) -> String {
        self.card_about.clone()
    }

    pub fn rank_width(&self) -> u32 {
        self.rank_card.as_ref().map_or(0, |c| c.image.width as u32)
    }

    pub fn rank_height(&self) -> u32 {
        self.rank_card.as_ref().map_or(0, |c| c.image.height as u32)
    }

    pub fn rank_pixels(&self) -> Vec<u8> {
        self.rank_card.as_ref().map_or_else(Vec::new, |c| c.image.data.clone())
    }

    /// Who the rank panel is about, `{"read", "status", "rank"}` JSON: the name as read off the Character Info window,
    /// "loading" / "found" / "missing" / "failed", and classicrank.com's answer (name, level, job, fame, the three
    /// ranks, botting, last_checked, profile_url, photo) or null. The page draws its own panel from it, with the
    /// character's photo and name: the floating window has no Character Info window beside it.
    pub fn rank_about(&self) -> String {
        self.rank_about.clone()
    }

    /// The next rank lookup the page should make, as `{"name", "url"}` JSON, or undefined. The page paces them
    /// (one every two seconds keeps under the API's 30 requests a minute).
    pub fn next_rank_request(&mut self) -> Option<String> {
        self.ranks.next_due().map(|name| json!({"url": rank_url(&name), "name": name}).to_string())
    }

    /// How the page's lookup of `name` went: HTTP `status` (0 = network error), the body, and Retry-After.
    pub fn rank_response(&mut self, name: &str, status: u16, body: &str, retry_after: f64, now: f64) {
        let fetched = match status {
            200 => parse_response(body),
            404 => Fetched::Missing,
            _ => Fetched::Failed(if retry_after > 0.0 { retry_after } else { DEFAULT_RETRY_S }),
        };
        self.ranks.store(name, fetched, now);
    }

    /// Text the glyph atlas could not read, waiting for the page's OCR: `[{"id", "font", "width", "height"}]`.
    pub fn ocr_jobs(&mut self) -> String {
        let mut jobs = Vec::new();
        let queues =
            [("title", &self.title_ocr), ("name", &self.name_ocr), ("map", &self.map_ocr), ("shop", self.shop.queue())];
        for (font, queue) in queues {
            if let Ok(queue) = queue.lock() {
                jobs.extend(
                    queue.jobs().map(
                        |(id, image)| json!({"id": id, "font": font, "width": image.width, "height": image.height}),
                    ),
                );
            }
        }
        Value::Array(jobs).to_string()
    }

    /// Greyscale pixels of OCR job `id` (dark text on white, upscaled), as RGBA for a canvas.
    pub fn ocr_job_pixels(&self, id: u32) -> Vec<u8> {
        [&self.title_ocr, &self.name_ocr, &self.map_ocr, self.shop.queue()]
            .iter()
            .find_map(|q| {
                q.lock()
                    .ok()
                    .and_then(|q| q.job_image(id).map(|img| img.data.iter().flat_map(|&v| [v, v, v, 255]).collect()))
            })
            .unwrap_or_default()
    }

    /// The OCR answer for job `id`.
    pub fn ocr_answer(&mut self, id: u32, text: &str) {
        for queue in [&self.title_ocr, &self.name_ocr, &self.map_ocr, self.shop.queue()] {
            if let Ok(mut queue) = queue.lock() {
                queue.answer(id, text);
            }
        }
    }

    /// Items with prices (the demo has two).
    pub fn item_count(&self) -> u32 {
        self.catalog.len() as u32
    }

    /// The MapleLens version.
    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

/// The training numbers for the page (it draws its own panels from them).
fn training_json(training: &Training) -> Value {
    let exp = &training.exp;
    json!({
        "active": is_training(exp),
        "live": exp.live,
        "paused": exp.paused,
        "exp": exp.reading.map(|r| r.exp),
        "percent": exp.reading.map(|r| r.percent()),
        "last_minute": exp.last_minute,
        "recent_per_min": exp.recent_per_min,
        "average_per_min": exp.average_per_min,
        "percent_per_hour": exp.percent_per_hour,
        "to_level_s": exp.to_level_s,
        "session": exp.session,
        "map": exp.map,
        "map_stat": exp.map_stat,
        "potions": {
            "lines": training.potions.lines.iter().map(|l| json!({
                "id": l.id,
                "name": l.name,
                "used": l.used,
                "picked": l.picked,
                "icon": potions::icon_url(l.id),
            })).collect::<Vec<_>>(),
        },
    })
}
