//! The settings window, drawn like one of the game's own windows (pure): what it looks like for a state, and which
//! control sits under a point. The Windows app shows the picture and feeds the clicks and keys back.

mod panel;

use crate::notify::pairing::{Pairing, Panel, Route};
use crate::notify::{discord, telegram};
use crate::render::RgbaImage;
use crate::render::canvas::{Anchor, Canvas, Ink};
use crate::render::font::Fonts;
use crate::render::gamewin::{self, INK, INK_DIM, PAD, VALUE_EDGE, VALUE_FILL};
use crate::render::maple_ui::{self, ACCENT, Area, Tone, Touch};
use crate::render::theme::with_alpha;
use crate::settings::{Corner, EXPAND_DELAY_RANGE_S, Settings, delay_text};

/// Size in design units (device pixels at 96 DPI).
pub const SIZE: (f32, f32) = (380.0, 700.0);
const LABEL_W: f32 = 80.0;
const VALUE_X: f32 = PAD + LABEL_W + 4.0;
const VALUE_W: f32 = SIZE.0 - PAD - VALUE_X;
const ROW_H: f32 = 26.0;
const NOTE_SIZE: f32 = 10.0;
const CLEAR_W: f32 = 60.0;
const SLIDER_STEPS: f64 = 10.0;
/// One tick per half second.
const TICKS: usize = 6;
const STATUS_H: f32 = 48.0;
const PHONE_CONNECT_W: f32 = 96.0;
const PANEL_BUTTON_W: f32 = 132.0;
/// The pairing panel's buttons, at its foot.
const PANEL_BUTTONS_Y: f32 = SIZE.1 - gamewin::STRIP_H - 14.0 - ROW_H;
const CREDIT: &str = "Changes save as you make them";

const OVERLAY_Y: f32 = 36.0;
const STORE_Y: f32 = OVERLAY_Y + ROW_H + 5.0;
const STARTUP_Y: f32 = STORE_Y + ROW_H + 5.0;
const HOTKEY_Y: f32 = STARTUP_Y + ROW_H + 11.0;
const DELAY_Y: f32 = HOTKEY_Y + ROW_H + 20.0;
const TRAINING_Y: f32 = DELAY_Y + ROW_H + 26.0;
/// The training block: the EXP switch on the left, the corner picker on the right.
const TRAINING_H: f32 = 58.0;
const TRAINING_SWITCH_W: f32 = 150.0;
/// The picker: a small game window whose corners are the choices.
const SCREEN: Area = (SIZE.0 - PAD - 8.0 - 108.0, TRAINING_Y + 4.0, 108.0, TRAINING_H - 8.0);
/// A corner's card in the picker, and how far its click area reaches around it.
const CORNER_CARD: (f32, f32) = (28.0, 14.0);
const CORNER_INSET: f32 = 4.0;
const CORNER_REACH: f32 = 4.0;
/// The picker's minimap, top left: the top-left card sits beside it, as in the game.
const MINI_MAP: (f32, f32) = (17.0, 12.0);
/// Its status bar's share of the width: narrower than the game's (~0.42) so the bottom cards stay clear of it, as
/// they do in the game, where the cards are narrower.
const PICKER_BAR_SHARE: f32 = 0.34;
const UPDATES_Y: f32 = TRAINING_Y + TRAINING_H + 24.0;
const NOTIFY_Y: f32 = UPDATES_Y + ROW_H + 5.0;
const NOTIFY_STORE_Y: f32 = NOTIFY_Y + ROW_H + 5.0;
const WEBHOOK_Y: f32 = NOTIFY_STORE_Y + ROW_H + 5.0;
const TELEGRAM_Y: f32 = WEBHOOK_Y + ROW_H + 24.0;
/// Width of the Paste / Test / Clear buttons beside the webhook and bot boxes.
const WEBHOOK_BUTTON_W: f32 = 52.0;
const WEBHOOK_BUTTON_GAP: f32 = 4.0;
const PHONE_Y: f32 = TELEGRAM_Y + ROW_H + 24.0;
/// The phone's buttons sit on a row of their own under its state: "Add a phone" needs the room.
const PHONE_BUTTONS_Y: f32 = PHONE_Y + ROW_H + 5.0;
const STATUS_Y: f32 = PHONE_BUTTONS_Y + ROW_H + 24.0;
const BUTTONS_Y: f32 = STATUS_Y + STATUS_H + 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Close,
    Overlay,
    StatusBar,
    Startup,
    Hotkey,
    ClearHotkey,
    Delay,
    /// Show the EXP rates on the training card.
    TrainingExp,
    /// Show the map drops panel beside it.
    MapDrops,
    /// Put the training card in this corner of the game.
    Corner(Corner),
    AutoUpdate,
    /// Tell about sold items.
    NotifySales,
    /// Tell when the store is detected open or closes.
    NotifyStore,
    /// Take the Discord webhook URL from the clipboard.
    PasteWebhook,
    /// Send a test message to it.
    TestWebhook,
    ClearWebhook,
    /// Take a Telegram bot's `sendMessage` URL (or its token alone) from the clipboard.
    PasteTelegram,
    /// Find the chat that wrote to the bot, then send a test message to it.
    TestTelegram,
    ClearTelegram,
    /// Show the pairing code (a new key the first time).
    ConnectPhone,
    /// Send a test notification to the paired phones.
    TestPhone,
    /// Remove the pairing at the relay; every phone is told.
    RemovePhone,
    /// The pairing panel's grey button: Hide code, Done, Close.
    PanelBack,
    /// Its orange one: I've scanned it, Send a test, Show it again.
    PanelMain,
    Check,
    Update,
}

impl Control {
    /// Tab order.
    pub const ORDER: [Self; 26] = [
        Self::Overlay,
        Self::StatusBar,
        Self::Startup,
        Self::Hotkey,
        Self::ClearHotkey,
        Self::Delay,
        Self::TrainingExp,
        Self::MapDrops,
        Self::Corner(Corner::TopLeft),
        Self::Corner(Corner::TopRight),
        Self::Corner(Corner::BottomLeft),
        Self::Corner(Corner::BottomRight),
        Self::AutoUpdate,
        Self::NotifySales,
        Self::NotifyStore,
        Self::PasteWebhook,
        Self::TestWebhook,
        Self::ClearWebhook,
        Self::PasteTelegram,
        Self::TestTelegram,
        Self::ClearTelegram,
        Self::ConnectPhone,
        Self::TestPhone,
        Self::RemovePhone,
        Self::Check,
        Self::Update,
    ];
    /// Tab order while the pairing panel is up: nothing behind it can be reached.
    pub const PANEL_ORDER: [Self; 2] = [Self::PanelBack, Self::PanelMain];

    /// The controls that can be reached, in Tab order.
    pub fn order(panel: bool) -> &'static [Self] {
        if panel { &Self::PANEL_ORDER } else { &Self::ORDER }
    }

    /// Where the control is, in design units.
    pub fn area(self) -> Area {
        let row = |y| (VALUE_X, y, VALUE_W, ROW_H);
        match self {
            Self::Close => (SIZE.0 - gamewin::BORDER - 18.0, gamewin::BORDER + 3.0, 15.0, 14.0),
            Self::Overlay => row(OVERLAY_Y),
            Self::StatusBar => row(STORE_Y),
            Self::Startup => row(STARTUP_Y),
            Self::Hotkey => (VALUE_X, HOTKEY_Y, VALUE_W - CLEAR_W - 6.0, ROW_H),
            Self::ClearHotkey => (SIZE.0 - PAD - CLEAR_W, HOTKEY_Y, CLEAR_W, ROW_H),
            Self::Delay => (VALUE_X + 8.0, DELAY_Y, VALUE_W - 92.0, ROW_H),
            Self::TrainingExp => (VALUE_X + 2.0, TRAINING_Y + 4.0, TRAINING_SWITCH_W, ROW_H - 2.0),
            Self::MapDrops => (VALUE_X + 2.0, TRAINING_Y + 4.0 + ROW_H - 1.0, TRAINING_SWITCH_W, ROW_H - 2.0),
            Self::Corner(corner) => {
                let (x, y, w, h) = corner_card(corner);
                (x - CORNER_REACH, y - CORNER_REACH, w + 2.0 * CORNER_REACH, h + 2.0 * CORNER_REACH)
            }
            Self::AutoUpdate => row(UPDATES_Y),
            Self::NotifySales => row(NOTIFY_Y),
            Self::NotifyStore => row(NOTIFY_STORE_Y),
            Self::PasteWebhook => row_button(WEBHOOK_Y, 2),
            Self::TestWebhook => row_button(WEBHOOK_Y, 1),
            Self::ClearWebhook => row_button(WEBHOOK_Y, 0),
            Self::PasteTelegram => row_button(TELEGRAM_Y, 2),
            Self::TestTelegram => row_button(TELEGRAM_Y, 1),
            Self::ClearTelegram => row_button(TELEGRAM_Y, 0),
            Self::ConnectPhone => (VALUE_X, PHONE_BUTTONS_Y, PHONE_CONNECT_W, ROW_H),
            Self::TestPhone => (VALUE_X + PHONE_CONNECT_W + 6.0, PHONE_BUTTONS_Y, 60.0, ROW_H),
            Self::RemovePhone => (VALUE_X + PHONE_CONNECT_W + 72.0, PHONE_BUTTONS_Y, 72.0, ROW_H),
            Self::PanelBack => (SIZE.0 / 2.0 - PANEL_BUTTON_W - 5.0, PANEL_BUTTONS_Y, PANEL_BUTTON_W, ROW_H),
            Self::PanelMain => (SIZE.0 / 2.0 + 5.0, PANEL_BUTTONS_Y, PANEL_BUTTON_W, ROW_H),
            Self::Check => (SIZE.0 - PAD - 216.0, BUTTONS_Y, 100.0, ROW_H),
            Self::Update => (SIZE.0 - PAD - 110.0, BUTTONS_Y, 110.0, ROW_H),
        }
    }
}

/// Where the picker draws `corner`'s card: in that corner of its game window, the top-left one beside the minimap.
fn corner_card(corner: Corner) -> Area {
    let (sx, sy, sw, sh) = SCREEN;
    let (w, h) = CORNER_CARD;
    let left = sx + CORNER_INSET + if corner == Corner::TopLeft { MINI_MAP.0 + 3.0 } else { 0.0 };
    let x = if corner.is_left() { left } else { sx + sw - CORNER_INSET - w };
    let y = if corner.is_top() { sy + CORNER_INSET } else { sy + sh - CORNER_INSET - h };
    (x, y, w, h)
}

/// A webhook or bot row's buttons counted from the right edge.
fn row_button(y: f32, from_right: usize) -> Area {
    let step = WEBHOOK_BUTTON_W + WEBHOOK_BUTTON_GAP;
    (SIZE.0 - PAD - WEBHOOK_BUTTON_W - step * from_right as f32, y, WEBHOOK_BUTTON_W, ROW_H)
}

fn value_box(y: f32) -> Area {
    let right = row_button(y, 2).0 - WEBHOOK_BUTTON_GAP;
    (VALUE_X, y, right - VALUE_X, ROW_H)
}

fn inside((x, y, w, h): Area, px: f32, py: f32) -> bool {
    px >= x && py >= y && px < x + w && py < y + h
}

/// The control under design point (x, y); only the panel's own while the pairing panel is up.
pub fn hit(x: f32, y: f32, panel: bool) -> Option<Control> {
    [Control::Close].into_iter().chain(Control::order(panel).iter().copied()).find(|c| inside(c.area(), x, y))
}

/// The title bar, where a drag moves the window.
pub fn is_title(x: f32, y: f32) -> bool {
    inside((0.0, 0.0, SIZE.0, gamewin::PANEL_TOP), x, y) && hit(x, y, false).is_none()
}

/// The expand delay a click or drag at design x sets, in the slider's tenth-of-a-second steps.
pub fn delay_at(x: f32) -> f64 {
    let fraction = f64::from(maple_ui::slider_fraction(Control::Delay.area(), x));
    (fraction * EXPAND_DELAY_RANGE_S.1 * SLIDER_STEPS).round() / SLIDER_STEPS
}

/// The delay one arrow-key step from `now`.
pub fn delay_step(now: f64, up: bool) -> f64 {
    let step = if up { 1.0 } else { -1.0 } / SLIDER_STEPS;
    ((now + step) * SLIDER_STEPS).round().clamp(0.0, EXPAND_DELAY_RANGE_S.1 * SLIDER_STEPS) / SLIDER_STEPS
}

/// Everything the window shows.
#[derive(Clone, Debug, Default)]
pub struct View<'a> {
    pub settings: Settings,
    /// The app's version.
    pub version: &'a str,
    /// What the update check last found ("" before it ran).
    pub update_text: &'a str,
    pub can_check: bool,
    pub can_update: bool,
    /// What the last webhook paste or test came to ("" before any).
    pub notify_note: &'a str,
    /// What the last bot token paste or test came to ("" before any).
    pub telegram_note: &'a str,
    /// The phone: its state, the pairing panel and the code.
    pub phone: Pairing,
    /// Seconds before the pairing code hides itself, while it is up.
    pub phone_left_s: Option<u32>,
    /// "Update now", or "View release" for a build that cannot replace itself.
    pub update_label: &'a str,
    /// Some(what is held so far) while the hotkey box listens for a combo.
    pub capturing: Option<&'a str>,
    pub hover: Option<Control>,
    pub pressed: Option<Control>,
    pub focus: Option<Control>,
}

impl View<'_> {
    fn touch(&self, control: Control) -> Touch {
        let disabled = match control {
            Control::Check => !self.can_check,
            Control::Update => !self.can_update,
            Control::ClearHotkey => self.settings.toggle_hotkey == 0 && self.capturing.is_none(),
            Control::TestWebhook | Control::ClearWebhook => self.settings.discord_webhook.is_empty(),
            Control::TestTelegram | Control::ClearTelegram => self.settings.telegram_webhook.is_empty(),
            Control::ConnectPhone => self.phone_connected(),
            Control::TestPhone => !self.phone_ready() || self.phone.busy.is_some(),
            Control::RemovePhone => self.settings.phone_key.is_empty() || self.phone.busy.is_some(),
            _ => false,
        };
        Touch { hover: self.hover == Some(control), pressed: self.pressed == Some(control) && !disabled, disabled }
    }

    /// One phone per pairing: once one is connected, Remove is the way to connect another.
    pub fn phone_connected(&self) -> bool {
        !self.settings.phone_key.is_empty() && self.phone.phones.unwrap_or(0) > 0
    }

    /// A test can go: a key, and a phone paired with it as far as is known.
    pub fn phone_ready(&self) -> bool {
        !self.settings.phone_key.is_empty() && self.phone.phones != Some(0)
    }
}

/// The whole window at `factor` device pixels per design unit.
pub fn render(fonts: &Fonts, view: &View<'_>, factor: f32) -> RgbaImage {
    let (w, h) = SIZE;
    let mut cv = Canvas::new(fonts, w, h, factor);
    gamewin::window(&mut cv, h, "MAPLELENS SETTINGS");
    maple_ui::close_box(&mut cv, Control::Close.area(), view.touch(Control::Close));

    let s = &view.settings;
    let toggles = [
        (Control::Overlay, "OVERLAYS", "Show price cards over item tooltips", s.overlay_enabled),
        (Control::StatusBar, "STORE", "Show the status bar (store, EXP bar, minimap)", s.store_status_bar),
        (Control::Startup, "STARTUP", "Launch MapleLens when Windows starts", s.launch_at_startup),
        (Control::AutoUpdate, "UPDATES", "Check for updates automatically", s.check_updates),
        (Control::NotifySales, "NOTIFY", "Tell me when an item sells", s.notify_sales),
        (Control::NotifyStore, "", "Tell me when my store opens or closes", s.notify_store),
    ];
    for (control, label, text, on) in toggles {
        let area = control.area();
        if !label.is_empty() {
            gamewin::pill(&mut cv, (PAD, area.1, LABEL_W, ROW_H), label, 11.0);
        }
        toggle(&mut cv, area, text, on, view.touch(control).hover);
    }

    hotkey(&mut cv, view);
    delay(&mut cv, view);
    training(&mut cv, view);
    webhook(&mut cv, view);
    telegram_bot(&mut cv, view);
    phone_rows(&mut cv, view);
    update_status(&mut cv, view);
    maple_ui::button(&mut cv, Control::Check.area(), "Check now", Tone::Grey, view.touch(Control::Check));
    maple_ui::button(&mut cv, Control::Update.area(), view.update_label, Tone::Orange, view.touch(Control::Update));

    let panel = view.phone.panel != Panel::Hidden;
    if panel {
        panel::draw(&mut cv, view);
    }
    if let Some(focus) = view.focus.filter(|f| Control::order(panel).contains(f)) {
        maple_ui::focus_ring(&mut cv, focus.area());
    }
    let strip_mid = h - gamewin::STRIP_H / 2.0 - 2.0;
    gamewin::centred(&mut cv, w / 2.0, strip_mid, CREDIT, 10.0, INK_DIM);
    RgbaImage::from_canvas(&cv)
}

/// A beige value box; lighter under the pointer, ringed in orange while it is the active one.
fn field(cv: &mut Canvas<'_>, (x, y, w, h): Area, hover: bool, active: bool) {
    if active {
        cv.rounded((x, y, w, h), 3.0, Some(ACCENT), None);
        cv.rounded((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 2.0, Some([255, 255, 255, 255]), None);
    } else if hover {
        cv.rounded((x, y, w, h), 2.0, Some(VALUE_EDGE), None);
        cv.rounded((x + 1.0, y + 1.0, w - 2.0, h - 2.0), 2.0, Some([239, 236, 226, 255]), None);
    } else {
        gamewin::value_box(cv, (x, y, w, h));
    }
}

fn toggle(cv: &mut Canvas<'_>, area: Area, text: &str, on: bool, hover: bool) {
    let (x, y, _, h) = area;
    field(cv, area, hover, false);
    maple_ui::check_box(cv, x + 7.0, y + (h - 14.0) / 2.0, on, hover);
    cv.text(x + 29.0, y + h / 2.0 + 4.0, text, Ink::new(11.5, INK), Anchor::LS);
}

fn note(cv: &mut Canvas<'_>, below: Area, text: &str) {
    cv.text(VALUE_X + 2.0, below.1 + below.3 + 12.0, text, Ink::new(NOTE_SIZE, INK_DIM), Anchor::LS);
}

fn hotkey(cv: &mut Canvas<'_>, view: &View<'_>) {
    let area = Control::Hotkey.area();
    let (x, y, _, h) = area;
    gamewin::pill(cv, (PAD, y, LABEL_W, h), "HOTKEY", 11.0);
    field(cv, area, view.touch(Control::Hotkey).hover, view.capturing.is_some());
    let (text, colour, hint) = match view.capturing {
        Some(held) if !held.is_empty() => {
            (held.to_string(), [178, 92, 0, 255], "Esc cancels, Backspace removes the hotkey")
        }
        Some(_) => ("Press Ctrl or Alt + a key".into(), [178, 92, 0, 255], "Esc cancels, Backspace removes the hotkey"),
        None => (crate::hotkey::name(view.settings.toggle_hotkey), INK, "Shows or hides the cards; click to change it"),
    };
    cv.text(x + 9.0, y + h / 2.0 + 4.0, &text, Ink::new(12.0, colour).bold(), Anchor::LS);
    let clear = Control::ClearHotkey;
    maple_ui::button(cv, clear.area(), "Clear", Tone::Grey, view.touch(clear));
    note(cv, area, hint);
}

fn delay(cv: &mut Canvas<'_>, view: &View<'_>) {
    let slider = Control::Delay.area();
    let area = (VALUE_X, slider.1, VALUE_W, ROW_H);
    gamewin::pill(cv, (PAD, area.1, LABEL_W, ROW_H), "DETAILS", 11.0);
    field(cv, area, false, false);
    let seconds = view.settings.expand_delay_s;
    let fraction = (seconds / EXPAND_DELAY_RANGE_S.1) as f32;
    maple_ui::slider(cv, slider, fraction, TICKS, view.touch(Control::Delay));
    let right = area.0 + area.2 - 8.0;
    cv.text(right, area.1 + ROW_H / 2.0 + 4.0, &delay_text(seconds), Ink::new(11.5, INK).bold(), Anchor::RS);
    note(cv, area, "How long to rest on an item before its card opens up");
}

/// The training card: what it shows, and the corner of the game it sits in, picked on a small drawing of the game.
fn training(cv: &mut Canvas<'_>, view: &View<'_>) {
    let s = &view.settings;
    let area = (VALUE_X, TRAINING_Y, VALUE_W, TRAINING_H);
    gamewin::pill(cv, (PAD, TRAINING_Y, LABEL_W, ROW_H), "TRAINING", 11.0);
    field(cv, area, false, false);
    switch(cv, view, Control::TrainingExp, s.training_card, "My EXP rates");
    switch(cv, view, Control::MapDrops, s.map_drops, "Map drops");
    picker(cv, view);
    let shown = if !(s.training_card || s.quest_helper || s.map_drops) {
        "Switched off: tick My EXP rates or Map drops to show them".to_string()
    } else if s.training_at.is_some() || s.drops_at.is_some() {
        "Dragged in the game: click a corner to put them back".to_string()
    } else {
        format!("In the game's {} corner: click another to move it", s.training_corner.name().to_lowercase())
    };
    let width = SIZE.0 - PAD - VALUE_X;
    cv.text(
        VALUE_X + 2.0,
        TRAINING_Y + TRAINING_H + 12.0,
        &fit(cv, &shown, NOTE_SIZE, width),
        Ink::new(NOTE_SIZE, INK_DIM),
        Anchor::LS,
    );
}

/// A check box and its label in the training block.
fn switch(cv: &mut Canvas<'_>, view: &View<'_>, control: Control, on: bool, label: &str) {
    let (x, y, w, h) = control.area();
    if view.touch(control).hover {
        cv.rounded((x, y, w, h), 2.0, Some([239, 236, 226, 255]), None);
    }
    maple_ui::check_box(cv, x + 5.0, y + (h - 14.0) / 2.0, on, view.touch(control).hover);
    cv.text(x + 27.0, y + h / 2.0 + 4.0, label, Ink::new(11.5, INK), Anchor::LS);
}

/// A small game window: sky, the minimap top left, the status bar along the bottom, and a card in each corner, the
/// chosen one lit up in the game's orange.
fn picker(cv: &mut Canvas<'_>, view: &View<'_>) {
    let (x, y, w, h) = SCREEN;
    cv.rounded((x, y, w, h), 4.0, Some([52, 66, 101, 255]), None);
    cv.gradient((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 3.0, &[(0.0, [118, 178, 236, 255]), (1.0, [196, 228, 250, 255])]);
    // A strip of ground, and the status bar centred on it.
    cv.rect(x + 2.0, y + h - 7.0, w - 4.0, 5.0, [124, 168, 82, 255]);
    let bar_w = w * PICKER_BAR_SHARE;
    cv.rounded((x + (w - bar_w) / 2.0, y + h - 9.0, bar_w, 7.0), 1.5, Some([58, 62, 74, 255]), None);
    for (i, colour) in [[226, 60, 60, 255], [64, 132, 228, 255], [220, 214, 72, 255]].into_iter().enumerate() {
        let gauge_w = (bar_w - 8.0) / 3.0;
        cv.rect(x + (w - bar_w) / 2.0 + 2.0 + i as f32 * (gauge_w + 2.0), y + h - 6.0, gauge_w, 2.0, colour);
    }
    let (mw, mh) = MINI_MAP;
    cv.rounded(
        (x + CORNER_INSET, y + CORNER_INSET, mw, mh),
        1.5,
        Some([236, 242, 250, 255]),
        Some([88, 108, 140, 255]),
    );
    cv.rect(x + CORNER_INSET + 3.0, y + CORNER_INSET + 5.0, mw - 6.0, 4.0, [150, 186, 214, 255]);
    let chosen = view.settings.training_corner;
    let off = !view.settings.training_card && !view.settings.quest_helper && !view.settings.map_drops;
    for corner in Corner::ALL {
        let control = Control::Corner(corner);
        let touch = view.touch(control);
        let (cx, cy, cw, ch) = corner_card(corner);
        if corner == chosen && !off {
            cv.rounded((cx, cy + 1.0, cw, ch), 3.0, Some(with_alpha([0, 0, 0, 255], 60)), None);
            cv.gradient((cx, cy, cw, ch), 3.0, &maple_ui::ORANGE_STOPS);
            cv.rounded((cx, cy, cw, ch), 3.0, None, Some(maple_ui::ORANGE_EDGE));
            // Two lines of text on the little card.
            cv.rect(cx + 5.0, cy + 4.0, cw - 11.0, 2.0, [255, 255, 255, 230]);
            cv.rect(cx + 5.0, cy + 8.5, cw - 16.0, 2.0, [255, 255, 255, 170]);
        } else {
            let fill = if touch.hover { 150 } else { 80 };
            let edge = if touch.hover { ACCENT } else { with_alpha([255, 255, 255, 255], 210) };
            cv.rounded((cx, cy, cw, ch), 3.0, Some(with_alpha([255, 255, 255, 255], fill)), Some(edge));
        }
    }
}

fn webhook(cv: &mut Canvas<'_>, view: &View<'_>) {
    let url = &view.settings.discord_webhook;
    // Only the end of the token shows: the URL is a secret.
    let value = (!url.is_empty()).then(|| {
        let tail = discord::masked(url).rsplit('…').next().unwrap_or_default().to_string();
        format!("Set · …{tail}")
    });
    let hint = if view.notify_note.is_empty() { "Copy a channel's webhook URL, then Paste" } else { view.notify_note };
    let buttons = [Control::PasteWebhook, Control::TestWebhook, Control::ClearWebhook];
    secret_row(cv, view, WEBHOOK_Y, "DISCORD", value, hint, buttons);
}

fn telegram_bot(cv: &mut Canvas<'_>, view: &View<'_>) {
    let url = &view.settings.telegram_webhook;
    let (bot, chat) = telegram::parse_webhook(url).unwrap_or_default();
    // The note below says what linking takes; the box has room for little more than this.
    let value = (!url.is_empty()).then(|| match chat {
        0 => "No chat yet".to_string(),
        _ => format!("Set · {}", telegram::masked(bot)),
    });
    let hint = match view.telegram_note {
        "" if url.is_empty() => "Copy your bot's sendMessage URL (or its token), then Paste",
        "" if chat == 0 => "Send /start to your bot in Telegram, then press Test",
        "" => "Messages go to the chat_id in the URL",
        note => note,
    };
    let buttons = [Control::PasteTelegram, Control::TestTelegram, Control::ClearTelegram];
    secret_row(cv, view, TELEGRAM_Y, "TELEGRAM", value, hint, buttons);
}

/// A row holding a secret: its label, what is set (None = "Not set"), Paste / Test / Clear, and a note below.
fn secret_row(
    cv: &mut Canvas<'_>,
    view: &View<'_>,
    y: f32,
    label: &str,
    value: Option<String>,
    hint: &str,
    [paste, test, clear]: [Control; 3],
) {
    let area = value_box(y);
    let (x, _, w, h) = area;
    gamewin::pill(cv, (PAD, y, LABEL_W, h), label, 11.0);
    field(cv, area, false, false);
    let (text, colour) = match value {
        Some(text) => (text, INK),
        None => ("Not set".to_string(), INK_DIM),
    };
    cv.text(x + 9.0, y + h / 2.0 + 4.0, &fit(cv, &text, 11.5, w - 14.0), Ink::new(11.5, colour).bold(), Anchor::LS);
    maple_ui::button(cv, paste.area(), "Paste", Tone::Orange, view.touch(paste));
    maple_ui::button(cv, test.area(), "Test", Tone::Grey, view.touch(test));
    maple_ui::button(cv, clear.area(), "Clear", Tone::Grey, view.touch(clear));
    let width = SIZE.0 - PAD - VALUE_X;
    cv.text(VALUE_X + 2.0, y + h + 12.0, &fit(cv, hint, NOTE_SIZE, width), Ink::new(NOTE_SIZE, INK_DIM), Anchor::LS);
}

fn phone_rows(cv: &mut Canvas<'_>, view: &View<'_>) {
    let has_key = !view.settings.phone_key.is_empty();
    let area = (VALUE_X, PHONE_Y, VALUE_W, ROW_H);
    let (x, y, _, h) = area;
    gamewin::pill(cv, (PAD, y, LABEL_W, h), "PHONE", 11.0);
    field(cv, area, false, false);
    let phones = view.phone.phones.unwrap_or(0);
    let dot = match (has_key, phones) {
        (false, _) => INK_DIM,
        (true, 0) => ACCENT,
        _ => panel::GREEN,
    };
    cv.circle(x + 12.0, y + h / 2.0, 4.0, dot, Some((with_alpha([0, 0, 0, 255], 60), 1.0)));
    let state = view.phone.state_text(has_key);
    cv.text(
        x + 22.0,
        y + h / 2.0 + 4.0,
        &state,
        Ink::new(11.5, if has_key { INK } else { INK_DIM }).bold(),
        Anchor::LS,
    );
    maple_ui::button(cv, Control::ConnectPhone.area(), "Connect", Tone::Orange, view.touch(Control::ConnectPhone));
    let test = if view.phone.busy == Some(Route::Test) { "Sending…" } else { "Test" };
    maple_ui::button(cv, Control::TestPhone.area(), test, Tone::Grey, view.touch(Control::TestPhone));
    let remove = if view.phone.busy == Some(Route::Remove) { "Removing…" } else { "Remove" };
    maple_ui::button(cv, Control::RemovePhone.area(), remove, Tone::Grey, view.touch(Control::RemovePhone));
    let hint = fit(cv, view.phone.hint(has_key), NOTE_SIZE, VALUE_W);
    cv.text(VALUE_X + 2.0, PHONE_BUTTONS_Y + ROW_H + 12.0, &hint, Ink::new(NOTE_SIZE, INK_DIM), Anchor::LS);
}

fn update_status(cv: &mut Canvas<'_>, view: &View<'_>) {
    let area = (VALUE_X, STATUS_Y, VALUE_W, STATUS_H);
    cv.rounded(area, 2.0, Some(VALUE_EDGE), None);
    cv.rounded((area.0 + 1.0, area.1 + 1.0, area.2 - 2.0, area.3 - 2.0), 2.0, Some(VALUE_FILL), None);
    let x = area.0 + 9.0;
    cv.text(x, area.1 + 16.0, &format!("MapleLens {}", view.version), Ink::new(11.5, INK).bold(), Anchor::LS);
    let status = if view.update_text.is_empty() { "Not checked yet" } else { view.update_text };
    let width = area.2 - 18.0;
    let mut lines = cv.wrap(status, 10.5, width, false);
    if lines.len() > 2 {
        lines.truncate(2);
        lines[1] = ellipsize(cv, &format!("{} …", lines[1]), 10.5, width);
    }
    for (i, line) in lines.iter().enumerate() {
        cv.text(x, area.1 + 30.0 + 12.0 * i as f32, line, Ink::new(10.5, INK_DIM), Anchor::LS);
    }
}

/// `text` as it is when it fits `width`, else cut to fit.
pub(super) fn fit(cv: &Canvas<'_>, text: &str, size: f32, width: f32) -> String {
    if cv.measure(text, size, false) <= width { text.to_string() } else { ellipsize(cv, text, size, width) }
}

/// `text` cut to fit `width`, ending in "…".
fn ellipsize(cv: &Canvas<'_>, text: &str, size: f32, width: f32) -> String {
    let mut text = text.trim_end_matches(" …").to_string();
    while !text.is_empty() && cv.measure(&format!("{text}…"), size, false) > width {
        text.pop();
    }
    format!("{}…", text.trim_end())
}
