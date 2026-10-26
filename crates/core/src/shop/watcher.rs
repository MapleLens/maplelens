//! ShopWatcher: frame in, shop events out. The only stateful piece; everything it calls is pure.

use super::align::{new_at_back, new_at_front};
use super::buttons::{StoreState, read_state};
use super::layout::{self, Located, View};
use super::lines::{self, Line};
use super::names::{NameBook, same_name_key};
pub use super::observation::Observation;
use super::observation::balance_same;
use super::parse::{NO_NAME, parse_chat, parse_sale};
use super::scrollbar::{Thumb, thumb_position};
use super::soldlist::{self, Row};
use super::text::{self, bitmap_block};
use super::{ListKind, ShopContext, ShopEvent, ShopStatus, chatlog};
use crate::image::RgbImage;
use crate::market::catalog::Catalog;
use crate::vision::Mask;
use crate::vision::gaps::fill_gaps;
use crate::vision::hud::find_bar;
use std::sync::{Arc, Mutex};

/// Closing a store can look like the window vanishing or like it going back to its set-up buttons, and either can
/// also be a dialog dragged over it; only a change this long counts as closed.
pub const STORE_GONE_AFTER: f64 = 5.0;
/// A shop window not on screen at all for this long is closed. The game switching resolution, ratio or full screen
/// hides it for 15-30 s (seen live at 1440p) and brings back the same open store: with 5 s it was announced closed
/// and opened again. A store closed by its button is gone too, so its closing is announced this much later.
pub const STORE_VANISHED_AFTER: f64 = 60.0;
/// Consecutive frames that must agree before the store state is believed.
pub const STATE_FRAMES: u32 = 2;
/// OCR is touchy about margins: a chat read that fails to parse is retried with the next one.
const OCR_PADS: [usize; 3] = [4, 1, 8];
/// Tries at reading the counter's first value, and the owner's name, before giving up on this store.
const BASELINE_TRIES: u32 = 4;
const OWNER_TRIES: u32 = 8;
const MIN_OWNER_LETTERS: usize = 3;
/// Brightness levels the plate's mask is made at, and the readings that must agree on a name.
const OWNER_INK_LEVELS: [u8; 3] = [100, 120, 145];
const HUD_INK_LEVELS: [u8; 3] = [90, 110, 125];
const OWNER_VOTES: usize = 3;
/// The status bar's name, relative to its outline's top-left: (dx, dy, width, height). The job stands above it.
const HUD_NAME: (i32, i32, i32, i32) = (128, 78, 110, 20);
/// Its letters are crisp light grey on dark: these ink levels all keep them whole.
/// Looks at the chat log's first lines, and how many looks, for the owner's name in a larger font.
const OWNER_SPELLING_LINES: usize = 12;
const OWNER_SPELLING_TRIES: u32 = 3;
/// The counter is dark digits on white.
const DIGIT_INK: u8 = 150;

/// Glyph mask (true = ink) -> text; None while the text is still being read (the browser's OCR answers later).
pub type ReadText = Box<dyn FnMut(&Mask) -> Option<String> + Send>;

/// A read whose text is not in yet.
struct Pending;

/// The meso counter's number. It holds only digits, so letters OCR is known to confuse with them are read as those
/// digits ("5,ggo" for 5,990 at the smallest UI size); anything else leaves the number unknown, never wrong.
pub fn counter_value(text: &str) -> Option<i64> {
    let mut digits = String::new();
    for c in text.trim().chars() {
        match c {
            '0'..='9' => digits.push(c),
            'o' | 'O' | 'D' | 'Q' => digits.push('0'),
            'l' | 'I' | 'i' | '|' => digits.push('1'),
            'z' | 'Z' => digits.push('2'),
            's' | 'S' => digits.push('5'),
            'b' => digits.push('6'),
            'B' => digits.push('8'),
            'g' | 'q' => digits.push('9'),
            ',' | '.' | ' ' => {}
            _ => return None,
        }
    }
    digits.parse().ok()
}

/// `mask` with `pad` blank pixels around it.
fn padded(mask: &Mask, pad: usize) -> Mask {
    let mut out = Mask::new(mask.width + 2 * pad, mask.height + 2 * pad);
    for y in 0..mask.height {
        for x in 0..mask.width {
            out.set(x + pad, y + pad, mask.get(x, y));
        }
    }
    out
}

/// Cloning a watcher copies its state and shares the reader: the browser reads text asynchronously, so a frame whose
/// text is not read yet is undone (the state restored from a clone) and tried again when the answers are in.
#[derive(Clone)]
pub struct ShopWatcher {
    read: Arc<Mutex<ReadText>>,
    /// Where the shop was on the last frame that showed it.
    located: Option<Located>,
    /// Where the shop was when its lists were last read: a window back after frames without it (the game switching
    /// resolution or ratio) at another scale or place holds lists read another way.
    lists_at: Option<Located>,
    last: Option<Observation>,
    /// Confirmed: STATE_FRAMES consecutive frames agreed; None = no shop window.
    state: Option<StoreState>,
    candidate: Option<StoreState>,
    candidate_frames: u32,
    /// What the status pill shows; settles over STATE_FRAMES frames like the state.
    status: ShopStatus,
    status_candidate: ShopStatus,
    status_frames: u32,
    // The shop's contents: a closed shop's lists mean nothing to the next one.
    sold: Option<Vec<Row>>,
    chat: Option<Vec<Line>>,
    balance: Option<RgbImage>,
    balance_value: Option<i64>,
    /// Tries made to read the counter's first value (it is read only to have something to compare with).
    baseline_tries: u32,
    /// The player who opened the store, read off the name plate, and the tries made.
    owner: Option<String>,
    owner_tries: u32,
    owner_reads: Vec<String>,
    hud_reads: Vec<String>,
    /// Tries made at settling the plate's spelling from the chat log (its first line is the owner entering).
    owner_spelling_tries: u32,
    owner_spelled: bool,
    /// A name known from elsewhere (the Character Info window): used for the spelling when the plate's reads agree
    /// with it up to case and look-alike letters.
    owner_hint: Option<String>,
    /// Balance changes seen while the Sold Items dialog was closed: sales not yet identified.
    unseen: usize,
    /// When our store stopped being open.
    away_since: Option<f64>,
    announced_closed: bool,
    was_open: bool,
    /// Every player's name is reported as its most-read spelling.
    names: NameBook,
    /// Item names read off the Sold Items list are matched against it: OCR drops and swaps letters.
    catalog: Option<Arc<Catalog>>,
    /// The last update met an event (a sale, a visitor) whose text was not read yet.
    missing_text: bool,
    /// Lines already read by the shop's font.
    texts: text::Cache,
}

impl ShopWatcher {
    pub fn new(read: ReadText) -> Self {
        Self {
            read: Arc::new(Mutex::new(read)),
            located: None,
            lists_at: None,
            last: None,
            state: None,
            candidate: None,
            candidate_frames: 0,
            status: ShopStatus::Hidden,
            status_candidate: ShopStatus::Hidden,
            status_frames: 0,
            sold: None,
            chat: None,
            balance: None,
            balance_value: None,
            baseline_tries: 0,
            owner: None,
            owner_tries: 0,
            owner_reads: Vec::new(),
            hud_reads: Vec::new(),
            owner_spelling_tries: 0,
            owner_spelled: false,
            owner_hint: None,
            unseen: 0,
            away_since: None,
            announced_closed: false,
            was_open: false,
            names: NameBook::default(),
            catalog: None,
            missing_text: false,
            texts: text::Cache::default(),
        }
    }

    /// The items sold are named as the catalog names them when the read is close enough to one.
    pub fn set_catalog(&mut self, catalog: Arc<Catalog>) {
        self.catalog = Some(catalog);
    }

    /// What the name plate and the status bar have read as so far (for diagnosing a store whose owner is not found).
    pub fn owner_reads(&self) -> Vec<String> {
        self.hud_reads
            .iter()
            .map(|r| format!("hud:{r}"))
            .chain(self.owner_reads.iter().map(|r| format!("plate:{r}")))
            .collect()
    }

    /// A name for the owner known from elsewhere, to settle the plate's spelling (case and I/l are unclear there).
    pub fn suggest_owner(&mut self, name: Option<&str>) {
        self.owner_hint = name.map(str::to_string);
    }

    /// Whose store it is and what the counter last read.
    pub fn context(&self) -> ShopContext {
        // The plate's name is only given out once its spelling has been settled from the chat log (or that did not work
        // out in a few looks): it would be announced with its case wrong ("ShOpOwner") otherwise.
        let settled =
            self.owner.clone().filter(|_| self.owner_spelled || self.owner_spelling_tries >= OWNER_SPELLING_TRIES);
        ShopContext { owner: settled, balance: self.balance_value }
    }

    pub fn status(&self) -> ShopStatus {
        self.status
    }

    /// The last update met a sale or a visitor whose text is still being read: its events are incomplete, and the
    /// caller that reads text asynchronously should undo it and try the frame again once the text is in. Text read
    /// for the watcher's own sake (the owner's name, the counter's first value) never asks for that: it is simply
    /// read again on the next look.
    pub fn missing_text(&self) -> bool {
        self.missing_text
    }

    /// True while the shop window is on screen (the caller may poll faster then).
    pub fn sees_shop(&self) -> bool {
        self.located.is_some()
    }

    /// The UI scale the shop window is drawn at, measured off it (None while it is not on screen).
    pub fn scale(&self) -> Option<f32> {
        self.located.map(|l| l.scale)
    }

    fn reset(&mut self) {
        self.sold = None;
        self.chat = None;
        self.balance = None;
        self.balance_value = None;
        self.baseline_tries = 0;
        self.owner = None;
        self.owner_tries = 0;
        self.owner_reads.clear();
        self.hud_reads.clear();
        self.owner_spelling_tries = 0;
        self.owner_spelled = false;
        self.unseen = 0;
        self.away_since = None;
        self.announced_closed = false;
        self.was_open = false;
        self.names.clear();
    }

    /// Call with every captured client frame (any rate, any size: the shop measures the scale its UI is drawn at
    /// itself); returns what newly happened, oldest first.
    pub fn update(&mut self, frame: &RgbImage, now: f64) -> Vec<ShopEvent> {
        self.missing_text = false;
        let events = self.read_frame(frame, now);
        events.into_iter().map(|event| self.settle_names(event)).collect()
    }

    fn settle_names(&mut self, event: ShopEvent) -> ShopEvent {
        match event {
            ShopEvent::Sale { item, buyer, quantity, mesos, .. } => {
                let found = self.catalog.as_ref().and_then(|c| c.lookup(&item));
                let item_id = found.as_ref().and_then(|f| {
                    let digits: String =
                        f.item_id.chars().skip_while(|c| *c != ':').filter(char::is_ascii_digit).collect();
                    digits.parse().ok()
                });
                let item = found.map_or(item, |f| f.name.clone());
                ShopEvent::Sale { item, buyer: self.names.buyer(&buyer), quantity, mesos, item_id }
            }
            ShopEvent::PlayerEntered(name) => ShopEvent::PlayerEntered(self.names.entered(&name)),
            ShopEvent::PlayerLeft(name) => ShopEvent::PlayerLeft(self.names.left(&name)),
            other => other,
        }
    }

    fn read_frame(&mut self, frame: &RgbImage, now: f64) -> Vec<ShopEvent> {
        if frame.is_empty() {
            // Minimized or mid-resize: nothing to read, and nothing has closed.
            return Vec::new();
        }
        let before = self.located;
        let located = layout::locate(frame, before.as_ref(), &soldlist::has_list);
        if located.is_none() && before.is_some_and(|b| layout::window_remains(frame, &b)) {
            // Something covers the buttons (a tooltip, the pointer) but the window is still there: no news.
            return Vec::new();
        }
        self.located = located;
        // Without it, a window back at a new scale after a frame without it compared its lists with the old scale's
        // and reported both lists lost (seen live switching 1440p default ratio to Ideal Ratio).
        self.forget_moved(before.or(self.lists_at));
        if located.is_some() {
            self.lists_at = located;
        }
        let Some(located) = located else {
            let events = self.follow_state(None, now);
            self.follow_status(None);
            self.last = None;
            return events;
        };
        let view = View { frame, scale: located.scale };
        let enter = located.anchors.enter.expect("a located shop has its ENTER anchor");
        let reading = read_state(&view, enter);
        if reading == StoreState::Unknown {
            // Something covers the buttons: no news about the state.
            return Vec::new();
        }
        let mut events = self.follow_state(Some(reading), now);
        if self.state == Some(StoreState::Open) {
            self.read_owner(&view, enter);
        }
        let observation = self.observe(&view, enter);
        self.follow_status(Some(&observation));
        self.respell_owner_from_chat(&view, observation.chat.as_deref().unwrap_or(&[]));
        let previous = self.last.replace(observation.clone());
        let Some(previous) = previous else { return events };
        if self.state != Some(StoreState::Open) || Some(reading) != self.state {
            return events;
        }
        if observation.sold_agrees(&previous) {
            events.extend(self.sold_events(&view, &observation));
        }
        if observation.chat_agrees(&previous) {
            events.extend(self.chat_events(&view, &observation));
        }
        events
    }

    fn judge(&self, observation: Option<&Observation>) -> ShopStatus {
        let Some(observation) = observation else { return ShopStatus::Hidden };
        match self.state {
            None | Some(StoreState::Visiting) | Some(StoreState::Unknown) => ShopStatus::Hidden,
            Some(StoreState::NotOpen) => ShopStatus::NotOpen,
            Some(StoreState::Open) if self.located.is_some_and(|l| l.anchors.sold_x.is_none()) => {
                ShopStatus::NeedSoldWindow
            }
            Some(StoreState::Open) if observation.sold.is_none() || observation.chat.is_none() => ShopStatus::Covered,
            Some(StoreState::Open) if !(observation.sold_live && observation.chat_live) => ShopStatus::Scrolled,
            Some(StoreState::Open) => ShopStatus::Watching,
        }
    }

    fn follow_status(&mut self, observation: Option<&Observation>) {
        let judged = self.judge(observation);
        self.status_frames = if judged == self.status_candidate { self.status_frames + 1 } else { 1 };
        self.status_candidate = judged;
        if self.status_frames >= STATE_FRAMES {
            self.status = judged;
        }
    }

    /// Opening and closing of *our* store. Someone else's store, or a store being set up, is never 'open'.
    fn follow_state(&mut self, reading: Option<StoreState>, now: f64) -> Vec<ShopEvent> {
        self.candidate_frames = if reading == self.candidate { self.candidate_frames + 1 } else { 1 };
        self.candidate = reading;
        if self.candidate_frames >= STATE_FRAMES {
            self.state = reading;
        }
        let mut events = Vec::new();
        if self.state == Some(StoreState::Open) {
            self.away_since = None;
            if !self.was_open {
                if self.announced_closed {
                    self.reset(); // a new shop: start from scratch
                }
                self.was_open = true;
                events.push(ShopEvent::StoreOpened);
            }
        } else if self.was_open && !self.announced_closed {
            let since = *self.away_since.get_or_insert(now);
            let after = if self.state.is_none() { STORE_VANISHED_AFTER } else { STORE_GONE_AFTER };
            if now - since >= after {
                self.announced_closed = true;
                self.was_open = false;
                events.push(ShopEvent::StoreClosed);
            }
        }
        events
    }

    /// A dragged window shows the same content at new coordinates, but frames mid-drag read as garbage. A window
    /// drawn at another scale (the game resized) is a new layout for everything.
    fn forget_moved(&mut self, before: Option<Located>) {
        let (Some(before), Some(now)) = (before, self.located) else { return };
        let rescaled = before.scale != now.scale;
        if rescaled || (before.marker.x, before.marker.y) != (now.marker.x, now.marker.y) {
            (self.chat, self.balance, self.last) = (None, None, None);
        }
        if rescaled || (before.sold_box.is_some() && now.sold_box.is_some() && before.sold_box != now.sold_box) {
            (self.sold, self.last) = (None, None);
        }
    }

    pub(crate) fn observe(&self, view: &View<'_>, enter: layout::Point) -> Observation {
        let sold_x = self.located.and_then(|l| l.anchors.sold_x);
        let (sold, sold_live) = match sold_x {
            Some(sold_x) => {
                let (column, track) = layout::sold_scroll(sold_x);
                let thumb = thumb_position(view, column, track);
                (soldlist::read_rows(view, sold_x), matches!(thumb, Thumb::None | Thumb::Start))
            }
            None => (None, false),
        };
        let (column, track) = layout::chat_scroll(enter);
        let area = layout::balance(enter);
        Observation {
            sold,
            sold_live,
            balance: Some(view.crop(area)),
            balance_area: Some(area),
            chat: chatlog::read_lines(view, enter),
            chat_live: matches!(thumb_position(view, column, track), Thumb::None | Thumb::End),
        }
    }

    pub(crate) fn sold_events(&mut self, view: &View<'_>, obs: &Observation) -> Vec<ShopEvent> {
        let moved = self.balance.is_some() && !balance_same(&self.balance, &obs.balance);
        self.balance = obs.balance.clone();
        // The counter is read once when first seen, so that the first change has something to be measured against
        // (and the balance is known for a notification), and again whenever it moves.
        if self.balance_value.is_none()
            && !moved
            && self.baseline_tries < BASELINE_TRIES
            && let Some(counter) = &obs.balance
            && let Ok(value) = self.read_counter(view, counter, obs.balance_area)
        {
            self.baseline_tries += 1;
            self.balance_value = value;
        }
        let gain =
            if moved { obs.balance.as_ref().and_then(|b| self.balance_gain(view, b, obs.balance_area)) } else { None };
        let Some(sold) = &obs.sold else {
            // The dialog is closed: the meso counter is the only sign of a sale.
            if !moved {
                return Vec::new();
            }
            self.unseen += 1;
            return vec![ShopEvent::SaleUnseen { mesos: gain }];
        };
        if !obs.sold_live {
            return Vec::new();
        }
        let rows: Vec<Row> = sold.iter().filter(|r| !r.is_total).cloned().collect();
        let known = self.sold.replace(rows.clone());
        let unseen = std::mem::take(&mut self.unseen);
        let Some(known) = known else {
            // First sight of the list: its newest rows are the sales that happened while it was closed (the dialog
            // may have opened in the very frame the counter changed).
            let fresh = (unseen + usize::from(moved)).min(rows.len());
            let sales: Vec<ShopEvent> = rows[..fresh].iter().rev().map(|row| self.read_sale(view, row)).collect();
            return with_gain(sales, gain);
        };
        let Some(added) = new_at_front(&known, &rows, soldlist::same) else {
            return vec![ShopEvent::LostSync(ListKind::Sold)];
        };
        let mut events = Vec::new();
        if added == 0 && moved && !known.is_empty() && known.iter().all(|r| soldlist::same(r, &known[0])) {
            events.push(ShopEvent::AmbiguousSale);
        }
        // Oldest first, so listeners see sales in the order they happened.
        let sales: Vec<ShopEvent> = rows[..added].iter().rev().map(|row| self.read_sale(view, row)).collect();
        events.extend(with_gain(sales, gain));
        events
    }

    /// Change of the meso counter, read by OCR; None when either reading failed.
    fn balance_gain(&mut self, view: &View<'_>, balance: &RgbImage, area: Option<layout::Area>) -> Option<i64> {
        let before = self.balance_value;
        let Ok(value) = self.read_counter(view, balance, area) else {
            self.missing_text = true;
            return None;
        };
        self.balance_value = value;
        Some(value? - before?)
    }

    /// The meso counter's number, read by OCR from the frame's own pixels (None when it is not a number).
    fn read_counter(
        &mut self,
        view: &View<'_>,
        balance: &RgbImage,
        area: Option<layout::Area>,
    ) -> Result<Option<i64>, Pending> {
        let exact = area.and_then(|area| self.texts.read_area(view, area, false));
        if let Some(read) = exact.as_ref().filter(|t| t.unknown == 0) {
            return Ok(counter_value(&read.text));
        }
        let ink = match area {
            Some(area) => lines::glyphs_at(view, area, false),
            None => {
                let gray = balance.to_gray();
                Mask {
                    width: gray.width,
                    height: gray.height,
                    bits: gray.data.iter().map(|&v| v < DIGIT_INK).collect(),
                }
            }
        };
        let ocr = self.ocr(&padded(&ink, OCR_PADS[0])).ok_or(Pending)?;
        Ok(counter_value(&exact.map_or(ocr.clone(), |e| fill_gaps(&e.text, &ocr))))
    }

    /// Reads the owner's name until it comes out (OCR is touchy, and the browser's answers take a moment): from the
    /// status bar, where the character's name stands under their job in a clean light font, and failing that from the
    /// shop window's name plate (whose letters are anti-aliased into its blue, so their case is unreliable).
    fn read_owner(&mut self, view: &View<'_>, enter: layout::Point) {
        if self.owner.is_some() || self.owner_tries >= OWNER_TRIES {
            return;
        }
        // At Ideal Ratio the plate is the bitmap font, read exactly: no vote needed.
        if bitmap_block(view.scale).is_some()
            && let Some(read) = self.texts.read_area(view, layout::owner_name(enter), true)
            && read.unknown == 0
            && read.text.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_OWNER_LETTERS
        {
            self.owner = Some(read.text);
            self.owner_spelled = true;
            return;
        }
        // A try counts once its reads are in: the browser's first answers take seconds (its OCR loads first).
        if let Some(area) = hud_name_area(view) {
            let Ok(reads) = self.read_name_area(view, area, HUD_INK_LEVELS) else { return };
            self.hud_reads.extend(reads);
            if let Some(name) = vote(&self.hud_reads, None) {
                self.owner = Some(name);
                self.owner_spelled = true; // the status bar spells it as the game does
                return;
            }
        }
        let Ok(reads) = self.read_name_area(view, layout::owner_name(enter), OWNER_INK_LEVELS) else { return };
        self.owner_tries += 1;
        self.owner_reads.extend(reads);
        self.owner = vote(&self.owner_reads, self.owner_hint.as_ref());
    }

    /// The readings of light text in `area` at each ink level and margin.
    fn read_name_area(&mut self, view: &View<'_>, area: layout::Area, levels: [u8; 3]) -> Result<Vec<String>, Pending> {
        let mut reads = Vec::new();
        let mut pending = false;
        for level in levels {
            let ink = lines::light_glyphs_at(view, area, level);
            if ink.width == 0 {
                continue;
            }
            for pad in OCR_PADS {
                // Every read is asked for, pending or not, so the browser reads them all at once.
                let Some(text) = self.ocr(&padded(&ink, pad)) else {
                    pending = true;
                    continue;
                };
                let text = text.trim().to_string();
                if text.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_OWNER_LETTERS {
                    reads.push(text);
                }
            }
        }
        if pending { Err(Pending) } else { Ok(reads) }
    }

    fn ocr(&self, mask: &Mask) -> Option<String> {
        // A reader that panicked once is still usable: its cache holds plain text.
        let mut read = self.read.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (*read)(mask)
    }

    /// A line's text: the shop's own font first (exact for every letter it knows, no OCR), OCR's letters only where
    /// the font has none ('?'), checked against the letters it is sure of. None while OCR's answer is pending.
    fn line_text(&self, view: &View<'_>, line: &Line, pad: usize) -> Option<String> {
        let exact = line.source.and_then(|(area, _)| self.texts.read_area(view, area, false));
        if let Some(read) = exact.as_ref().filter(|t| t.unknown == 0) {
            return Some(read.text.clone());
        }
        let ocr = self.ocr(&padded(&self.glyphs(view, line), pad))?;
        // The bars are settled again once OCR's letters stand beside them ("?lNO" + "Y" is "YINO").
        Some(text::settle(&exact.map_or(ocr.clone(), |e| fill_gaps(&e.text, &ocr))))
    }

    /// A line's glyphs, from the frame's own pixels (the game's small glyphs read better than an enlarged copy).
    fn glyphs(&self, view: &View<'_>, line: &Line) -> Mask {
        match line.source {
            Some((area, grey_only)) => lines::glyphs_at(view, area, grey_only),
            None => line.glyphs.clone(),
        }
    }

    fn read_sale(&mut self, view: &View<'_>, row: &Row) -> ShopEvent {
        let reads: Vec<Option<String>> = row.lines.iter().map(|line| self.line_text(view, line, OCR_PADS[0])).collect();
        self.missing_text |= reads.iter().any(Option::is_none);
        let texts: Vec<String> = reads.into_iter().map(Option::unwrap_or_default).collect();
        let text = |i: usize| texts.get(i).map_or("", String::as_str);
        let (item, buyer, quantity) = (text(0), text(1), text(2));
        parse_sale(item, buyer, quantity)
    }

    /// The plate's letters are anti-aliased into its blue, so OCR gets their case wrong ("ShOpOwner" for "ShopOwner"). The
    /// chat log's lines are larger and cleaner, and the owner is in them ("ShopOwner has entered."): the spelling of the
    /// name there that is the plate's name (up to case and look-alikes) is the one to use.
    fn respell_owner_from_chat(&mut self, view: &View<'_>, chat: &[Line]) {
        let Some(owner) = self.owner.clone() else { return };
        if self.owner_spelled || self.owner_spelling_tries >= OWNER_SPELLING_TRIES {
            return;
        }
        let key = same_name_key(&owner);
        let mut pending = false;
        for line in chat.iter().take(OWNER_SPELLING_LINES) {
            let Some(text) = self.line_text(view, line, OCR_PADS[0]) else {
                pending = true;
                continue;
            };
            if let Some(ShopEvent::PlayerEntered(name) | ShopEvent::PlayerLeft(name)) = parse_chat(&text)
                && name != NO_NAME
                && same_name_key(&name) == key
            {
                self.owner = Some(name);
                self.owner_spelled = true;
                return;
            }
        }
        // A look counts once all its lines were read.
        if !pending {
            self.owner_spelling_tries += 1;
        }
    }

    fn read_chat(&mut self, view: &View<'_>, line: &Line) -> Option<ShopEvent> {
        let mut event = None;
        for pad in OCR_PADS {
            let Some(text) = self.line_text(view, line, pad) else {
                self.missing_text = true;
                return None;
            };
            event = parse_chat(&text);
            let named = match &event {
                Some(ShopEvent::PlayerEntered(name) | ShopEvent::PlayerLeft(name)) => name != NO_NAME,
                _ => false,
            };
            if named {
                break;
            }
        }
        event
    }

    fn chat_events(&mut self, view: &View<'_>, obs: &Observation) -> Vec<ShopEvent> {
        let Some(chat) = obs.chat.as_ref().filter(|_| obs.chat_live) else { return Vec::new() };
        let Some(known) = self.chat.replace(chat.clone()) else { return Vec::new() };
        let Some(added) = new_at_back(&known, chat, lines::same) else {
            return vec![ShopEvent::LostSync(ListKind::Chat)];
        };
        chat[chat.len() - added..].iter().filter_map(|line| self.read_chat(view, line)).collect()
    }
}

/// Where the character's name is in the status bar: under their job, right of the level box, offsets measured on a
/// real 1920x1200 capture from the bar's outline (reference pixels).
fn hud_name_area(view: &View<'_>) -> Option<layout::Area> {
    let (width, height) = view.size();
    let strip = crate::config::STATUS_BAR_STRIP_HEIGHT;
    let top = height.checked_sub(strip)?;
    let bar = find_bar(&view.crop((0, top, width, strip)))?;
    let (x, y) = (bar.x as i32, top + bar.top as i32);
    Some((x + HUD_NAME.0, y + HUD_NAME.1, HUD_NAME.2, HUD_NAME.3))
}

/// The name most readings agree on (readings that differ only in case and look-alike letters are one name), when at
/// least `OWNER_VOTES` do and no other name has as many; spelled as `hint` spells it if that is the same name, else
/// as it was read most often (the first of those when level).
fn vote(reads: &[String], hint: Option<&String>) -> Option<String> {
    let mut names: Vec<(String, Vec<&String>)> = Vec::new();
    for read in reads {
        let key = same_name_key(read);
        match names.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1.push(read),
            None => names.push((key, vec![read])),
        }
    }
    let best = names.iter().map(|(_, reads)| reads.len()).max().unwrap_or(0);
    let leaders: Vec<&(String, Vec<&String>)> = names.iter().filter(|(_, reads)| reads.len() == best).collect();
    // A tie between two names is no answer yet: the next tries add readings.
    let [(key, reads)] = leaders.as_slice() else { return None };
    if best < OWNER_VOTES {
        return None;
    }
    if let Some(hint) = hint.filter(|h| same_name_key(h) == *key) {
        return Some(hint.clone());
    }
    let mut spellings: Vec<(&String, usize)> = Vec::new();
    for read in reads {
        match spellings.iter_mut().find(|(s, _)| s == read) {
            Some(entry) => entry.1 += 1,
            None => spellings.push((read, 1)),
        }
    }
    let most = spellings.iter().map(|&(_, n)| n).max().unwrap_or(0);
    spellings.iter().find(|&&(_, n)| n == most).map(|&(s, _)| s.clone())
}

/// When one sale is all there is, the counter's gain is its price; with several the gain is theirs together and goes to
/// none of them.
fn with_gain(mut sales: Vec<ShopEvent>, gain: Option<i64>) -> Vec<ShopEvent> {
    if let ([ShopEvent::Sale { mesos, .. }], Some(gain)) = (sales.as_mut_slice(), gain) {
        *mesos = Some(gain);
    }
    sales
}

#[cfg(test)]
mod name_tests {
    use super::*;
    use crate::market::sample::sample_items;

    #[test]
    fn sales_and_visitors_are_reported_with_settled_names() {
        let mut watcher = ShopWatcher::new(Box::new(|_| Some(String::new())));
        watcher.set_catalog(Arc::new(Catalog::new(sample_items(), None)));
        let sale = |item: &str, buyer: &str| ShopEvent::Sale {
            item: item.into(),
            buyer: buyer.into(),
            quantity: 2,
            mesos: None,
            item_id: None,
        };
        assert_eq!(
            watcher.settle_names(ShopEvent::PlayerEntered("AssassinSSR".into())),
            ShopEvent::PlayerEntered("AssassinSSR".into())
        );
        // A noisy item read becomes the catalog's name; a noisy buyer, the visitor's.
        assert_eq!(watcher.settle_names(sale("Emerald 0re", "As sass jnSSR")), sale("Emerald Ore", "AssassinSSR"));
        // Nothing like any item: kept as read.
        assert_eq!(watcher.settle_names(sale("Zzzzzzz Qqqq", "AssassinSSR")), sale("Zzzzzzz Qqqq", "AssassinSSR"));
    }
}
