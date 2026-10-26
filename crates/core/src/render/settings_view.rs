//! The settings window, drawn like one of the game's own windows (pure): what it looks like for a state, and which
//! control sits under a point. The Windows app shows the picture and feeds the clicks and keys back.

use super::RgbaImage;
use super::canvas::{Anchor, Canvas, Ink};
use super::font::Fonts;
use super::gamewin::{self, INK, INK_DIM, PAD, VALUE_EDGE, VALUE_FILL};
use super::maple_ui::{self, ACCENT, Area, Tone, Touch};
use crate::notify::{discord, telegram};
use crate::settings::{EXPAND_DELAY_RANGE_S, Settings, delay_text};

/// Size in design units (device pixels at 96 DPI).
pub const SIZE: (f32, f32) = (380.0, 536.0);
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
const CREDIT: &str = "Changes save as you make them";

const OVERLAY_Y: f32 = 36.0;
const STORE_Y: f32 = OVERLAY_Y + ROW_H + 5.0;
const STARTUP_Y: f32 = STORE_Y + ROW_H + 5.0;
const HOTKEY_Y: f32 = STARTUP_Y + ROW_H + 11.0;
const DELAY_Y: f32 = HOTKEY_Y + ROW_H + 20.0;
const UPDATES_Y: f32 = DELAY_Y + ROW_H + 26.0;
const NOTIFY_Y: f32 = UPDATES_Y + ROW_H + 5.0;
const NOTIFY_STORE_Y: f32 = NOTIFY_Y + ROW_H + 5.0;
const WEBHOOK_Y: f32 = NOTIFY_STORE_Y + ROW_H + 5.0;
const TELEGRAM_Y: f32 = WEBHOOK_Y + ROW_H + 24.0;
/// Width of the Paste / Test / Clear buttons beside the webhook and bot boxes.
const WEBHOOK_BUTTON_W: f32 = 52.0;
const WEBHOOK_BUTTON_GAP: f32 = 4.0;
const STATUS_Y: f32 = TELEGRAM_Y + ROW_H + 24.0;
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
    /// Take a Telegram bot token from the clipboard.
    PasteTelegram,
    /// Find the chat that wrote to the bot, then send a test message to it.
    TestTelegram,
    ClearTelegram,
    Check,
    Update,
}

impl Control {
    /// Tab order.
    pub const ORDER: [Self; 17] = [
        Self::Overlay,
        Self::StatusBar,
        Self::Startup,
        Self::Hotkey,
        Self::ClearHotkey,
        Self::Delay,
        Self::AutoUpdate,
        Self::NotifySales,
        Self::NotifyStore,
        Self::PasteWebhook,
        Self::TestWebhook,
        Self::ClearWebhook,
        Self::PasteTelegram,
        Self::TestTelegram,
        Self::ClearTelegram,
        Self::Check,
        Self::Update,
    ];

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
            Self::AutoUpdate => row(UPDATES_Y),
            Self::NotifySales => row(NOTIFY_Y),
            Self::NotifyStore => row(NOTIFY_STORE_Y),
            Self::PasteWebhook => row_button(WEBHOOK_Y, 2),
            Self::TestWebhook => row_button(WEBHOOK_Y, 1),
            Self::ClearWebhook => row_button(WEBHOOK_Y, 0),
            Self::PasteTelegram => row_button(TELEGRAM_Y, 2),
            Self::TestTelegram => row_button(TELEGRAM_Y, 1),
            Self::ClearTelegram => row_button(TELEGRAM_Y, 0),
            Self::Check => (SIZE.0 - PAD - 216.0, BUTTONS_Y, 100.0, ROW_H),
            Self::Update => (SIZE.0 - PAD - 110.0, BUTTONS_Y, 110.0, ROW_H),
        }
    }
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

/// The control under design point (x, y).
pub fn hit(x: f32, y: f32) -> Option<Control> {
    [Control::Close].into_iter().chain(Control::ORDER).find(|c| inside(c.area(), x, y))
}

/// The title bar, where a drag moves the window.
pub fn is_title(x: f32, y: f32) -> bool {
    inside((0.0, 0.0, SIZE.0, gamewin::PANEL_TOP), x, y) && hit(x, y).is_none()
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
            Control::TestTelegram | Control::ClearTelegram => self.settings.telegram_bot.is_empty(),
            _ => false,
        };
        Touch { hover: self.hover == Some(control), pressed: self.pressed == Some(control) && !disabled, disabled }
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
        (Control::StatusBar, "STORE", "Show the status bar in your own store", s.store_status_bar),
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
    webhook(&mut cv, view);
    telegram_bot(&mut cv, view);
    update_status(&mut cv, view);
    maple_ui::button(&mut cv, Control::Check.area(), "Check now", Tone::Grey, view.touch(Control::Check));
    maple_ui::button(&mut cv, Control::Update.area(), view.update_label, Tone::Orange, view.touch(Control::Update));

    if let Some(focus) = view.focus {
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
    let s = &view.settings;
    // The note below says what linking takes; the box has room for little more than this.
    let value = (!s.telegram_bot.is_empty()).then(|| match s.telegram_chat {
        0 => "Not linked yet".to_string(),
        _ => format!("Linked · {}", telegram::masked(&s.telegram_bot)),
    });
    let hint = match view.telegram_note {
        "" if s.telegram_bot.is_empty() => "Copy your bot's token from @BotFather, then Paste",
        "" if s.telegram_chat == 0 => "Send /start to your bot in Telegram, then press Test",
        "" => "Messages go to the chat that last wrote to the bot",
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
fn fit(cv: &Canvas<'_>, text: &str, size: f32, width: f32) -> String {
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
