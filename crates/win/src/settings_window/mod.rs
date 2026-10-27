//! The settings window, drawn like one of the game's own windows (`render::settings_view`) on a layered window,
//! with our own pointer and keyboard handling; changes apply as soon as they are made. Created lazily on first
//! `show`; closing it only hides it so it can be reopened.

use std::cell::RefCell;
use std::rc::Rc;

use maplelens_core::notify::NotifySettings;
use maplelens_core::notify::pairing::{Pairing, Panel};
use maplelens_core::render::font::Fonts;
use maplelens_core::render::settings_view::{self, Control, View};
use maplelens_core::settings::Settings;
use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetSystemMetrics, HICON, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW, SetForegroundWindow,
    ShowWindow, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_POPUP, WS_SYSMENU,
};

use crate::notify::Service;
use crate::update::UpdateState;
use crate::{layered, winclass};

mod input;
#[cfg(test)]
mod tests;

/// Windows' reference DPI: design units are pixels at 96 DPI.
const BASE_DPI: f32 = 96.0;

/// What happened in the window since the app last asked.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsEvent {
    Changed(Settings),
    /// The hotkey box started (true) or stopped (false) listening: the app releases the global hotkey meanwhile so
    /// the combo reaches the box.
    Capturing(bool),
    /// "Test" beside the webhook or the bot: send a test message there with these settings.
    TestNotify(Service, NotifySettings),
    /// A press in the phone's row or the pairing panel; the app holds the pairing and talks to the relay.
    Phone(PhoneAction),
    /// "Check now".
    CheckUpdates,
    /// "Update now" (or "View release" for a build that cannot update itself).
    Update,
}

/// What a phone control asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhoneAction {
    /// Show the pairing code (Connect, Add a phone, Show it again); a key is made the first time.
    ShowCode,
    /// "I've scanned it": ask the relay now.
    Scanned,
    Test,
    Remove,
    /// Hide code, Done, Close, Esc, or the window closing: the code must not stay up unseen.
    HidePanel,
}

#[derive(Default)]
struct State {
    hwnd: HWND,
    fonts: Option<Fonts>,
    /// Device pixels per design unit (the window's DPI / 96).
    factor: f32,
    self_update: bool,
    current: Settings,
    update: UpdateState,
    hover: Option<Control>,
    pressed: Option<Control>,
    focus: Option<Control>,
    /// The focus ring shows once the keyboard is used, as on the web.
    show_focus: bool,
    /// Some(the modifiers held so far) while the hotkey box listens.
    capturing: Option<String>,
    /// What the last webhook paste or test came to.
    notify_note: String,
    /// What the last bot token paste or test came to.
    telegram_note: String,
    /// The phone's pairing, as the app last told it.
    phone: Pairing,
    phone_left_s: Option<u32>,
    dragging: bool,
    tracking_leave: bool,
    events: Vec<SettingsEvent>,
}

pub struct SettingsWindow {
    icon: HICON,
    state: Rc<RefCell<State>>,
}

/// What the picture depends on, besides the update state: a message that leaves it alone needs no redraw.
type Look = (Settings, [Option<Control>; 3], bool, Option<String>, String, String, Pairing, Option<u32>);

impl State {
    fn look(&self) -> Look {
        (
            self.current.clone(),
            [self.hover, self.pressed, self.focus],
            self.show_focus,
            self.capturing.clone(),
            self.notify_note.clone(),
            self.telegram_note.clone(),
            self.phone.clone(),
            self.phone_left_s,
        )
    }

    fn paint(&self) {
        self.paint_at(None);
    }

    /// Draw the window, its top-left moved to `at` if given.
    fn paint_at(&self, at: Option<(i32, i32)>) {
        let Some(fonts) = &self.fonts else { return };
        if self.hwnd.is_invalid() {
            return;
        }
        let (text, can_check, can_update) = self.update.describe();
        let view = View {
            settings: self.current.clone(),
            notify_note: &self.notify_note,
            telegram_note: &self.telegram_note,
            phone: self.phone.clone(),
            phone_left_s: self.phone_left_s,
            version: env!("CARGO_PKG_VERSION"),
            update_text: &text,
            can_check,
            can_update,
            update_label: if self.self_update { "Update now" } else { "View release" },
            capturing: self.capturing.as_deref(),
            hover: self.hover,
            pressed: self.pressed,
            focus: self.focus.filter(|_| self.show_focus),
        };
        layered::blit(self.hwnd, &settings_view::render(fonts, &view, self.factor), at);
    }

    fn design(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (x / self.factor, y / self.factor)
    }

    fn changed(&mut self) {
        self.events.push(SettingsEvent::Changed(self.current.clone()));
    }

    fn start_capture(&mut self) {
        if self.capturing.is_none() {
            self.capturing = Some(String::new());
            self.events.push(SettingsEvent::Capturing(true));
        }
    }

    fn stop_capture(&mut self) {
        if self.capturing.take().is_some() {
            self.events.push(SettingsEvent::Capturing(false));
        }
    }

    /// Whether the pairing panel covers the window.
    fn panel(&self) -> bool {
        self.phone.panel != Panel::Hidden
    }

    fn hide(&mut self) {
        self.stop_capture();
        if self.panel() {
            self.events.push(SettingsEvent::Phone(PhoneAction::HidePanel));
        }
        (self.hover, self.pressed, self.dragging) = (None, None, false);
        // SAFETY: our own window.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn set_delay(&mut self, seconds: f64) {
        if seconds != self.current.expand_delay_s {
            self.current.expand_delay_s = seconds;
            self.changed();
        }
    }

    /// The webhook URL on the clipboard, if it is a Discord one.
    fn paste_webhook(&mut self) {
        let text = crate::clipboard::text().unwrap_or_default();
        match maplelens_core::notify::discord::valid_webhook(&text) {
            Some(url) => {
                self.current.discord_webhook = url.to_string();
                self.notify_note = "Saved. Press Test to check it".into();
                self.changed();
            }
            None => self.notify_note = "The clipboard has no Discord webhook URL".into(),
        }
    }

    /// A bot's `sendMessage` URL on the clipboard, or its token alone (then the chat is found by Test).
    fn paste_telegram(&mut self) {
        use maplelens_core::notify::telegram;
        let text = crate::clipboard::text().unwrap_or_default();
        match telegram::parse_webhook(&text) {
            Some((token, chat)) => {
                let url = telegram::webhook_url(token, chat);
                if url != self.current.telegram_webhook {
                    self.current.telegram_webhook = url;
                    self.changed();
                }
                self.telegram_note = if chat == 0 {
                    "Saved. Send /start to your bot in Telegram, then press Test".into()
                } else {
                    "Saved. Press Test to check it".into()
                };
            }
            None => self.telegram_note = "The clipboard has no Telegram bot URL or token".into(),
        }
    }

    /// The answer to a test, shown under its row.
    fn set_note(&mut self, service: Service, note: String) {
        match service {
            Service::Discord => self.notify_note = note,
            Service::Telegram => self.telegram_note = note,
        }
    }

    fn activate(&mut self, control: Control) {
        let (_, can_check, can_update) = self.update.describe();
        let s = &mut self.current;
        match control {
            Control::Close => return self.hide(),
            Control::Overlay => s.overlay_enabled = !s.overlay_enabled,
            Control::StatusBar => s.store_status_bar = !s.store_status_bar,
            Control::Startup => s.launch_at_startup = !s.launch_at_startup,
            Control::AutoUpdate => s.check_updates = !s.check_updates,
            Control::NotifySales => s.notify_sales = !s.notify_sales,
            Control::NotifyStore => s.notify_store = !s.notify_store,
            Control::TrainingExp => s.training_card = !s.training_card,
            Control::TrainingQuest => s.quest_helper = !s.quest_helper,
            Control::Corner(corner) if s.training_corner == corner => return,
            Control::Corner(corner) => s.training_corner = corner,
            Control::PasteWebhook => return self.paste_webhook(),
            Control::TestWebhook => {
                if self.current.discord_webhook.is_empty() {
                    return;
                }
                self.notify_note = "Sending a test message…".into();
                return self.events.push(SettingsEvent::TestNotify(Service::Discord, self.current.notify()));
            }
            Control::ClearWebhook => {
                if !self.current.discord_webhook.is_empty() {
                    self.current.discord_webhook.clear();
                    self.notify_note = "Webhook removed".into();
                    self.changed();
                }
                return;
            }
            Control::PasteTelegram => return self.paste_telegram(),
            Control::TestTelegram => {
                if self.current.telegram_webhook.is_empty() {
                    return;
                }
                self.telegram_note = if self.current.notify().telegram_chat == 0 {
                    "Looking for your chat with the bot…".into()
                } else {
                    "Sending a test message…".into()
                };
                return self.events.push(SettingsEvent::TestNotify(Service::Telegram, self.current.notify()));
            }
            Control::ClearTelegram => {
                if !self.current.telegram_webhook.is_empty() {
                    self.current.telegram_webhook.clear();
                    self.telegram_note = "Bot removed".into();
                    self.changed();
                }
                return;
            }
            Control::ConnectPhone => {
                // One phone per pairing.
                if self.current.phone_key.is_empty() || self.phone.phones.unwrap_or(0) == 0 {
                    self.events.push(SettingsEvent::Phone(PhoneAction::ShowCode));
                }
                return;
            }
            Control::TestPhone => {
                if !self.current.phone_key.is_empty() && self.phone.phones != Some(0) && self.phone.busy.is_none() {
                    self.events.push(SettingsEvent::Phone(PhoneAction::Test));
                }
                return;
            }
            Control::RemovePhone => {
                if !self.current.phone_key.is_empty() && self.phone.busy.is_none() {
                    self.events.push(SettingsEvent::Phone(PhoneAction::Remove));
                }
                return;
            }
            Control::PanelBack => return self.events.push(SettingsEvent::Phone(PhoneAction::HidePanel)),
            Control::PanelMain => {
                let action = match self.phone.panel {
                    Panel::Code { .. } => PhoneAction::Scanned,
                    Panel::Connected if self.phone.busy.is_none() => PhoneAction::Test,
                    Panel::Connected | Panel::Hidden => return,
                    Panel::Expired => PhoneAction::ShowCode,
                };
                return self.events.push(SettingsEvent::Phone(action));
            }
            Control::Hotkey => return self.start_capture(),
            Control::ClearHotkey => {
                self.stop_capture();
                if self.current.toggle_hotkey != 0 {
                    self.current.toggle_hotkey = 0;
                    self.changed();
                }
                return;
            }
            Control::Delay => return,
            Control::Check if can_check => return self.events.push(SettingsEvent::CheckUpdates),
            Control::Update if can_update => return self.events.push(SettingsEvent::Update),
            Control::Check | Control::Update => return,
        }
        self.changed();
    }
}

impl SettingsWindow {
    pub fn new(icon: HICON, self_update: bool) -> Self {
        Self { icon, state: Rc::new(RefCell::new(State { self_update, ..State::default() })) }
    }

    /// Show the update status (kept for when the window is first opened).
    pub fn show_update_state(&self, update: &UpdateState) {
        let mut state = self.state.borrow_mut();
        state.update = update.clone();
        state.paint();
    }

    pub fn hwnd(&self) -> HWND {
        self.state.borrow().hwnd
    }

    pub fn set_notify_note(&self, service: Service, note: String) {
        let mut state = self.state.borrow_mut();
        state.set_note(service, note);
        state.paint();
    }

    /// The phone's pairing changed (or the code's countdown ticked).
    pub fn set_phone(&self, phone: &Pairing, left_s: Option<u32>) {
        let mut state = self.state.borrow_mut();
        if *phone == state.phone && left_s == state.phone_left_s {
            return;
        }
        let was_up = state.panel();
        state.phone = phone.clone();
        state.phone_left_s = left_s;
        // The keyboard's place moves into the panel and back to the row it came from.
        if state.panel() != was_up {
            state.focus = Some(if state.panel() { Control::PanelMain } else { Control::ConnectPhone });
        }
        state.paint();
    }

    pub fn take_events(&self) -> Vec<SettingsEvent> {
        std::mem::take(&mut self.state.borrow_mut().events)
    }

    pub fn show(&self, settings: Settings) -> windows::core::Result<()> {
        if self.state.borrow().hwnd.is_invalid() {
            self.create()?;
        }
        self.sync(settings);
        let hwnd = self.hwnd();
        // SAFETY: our own window.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(())
    }

    /// Redraw after a change made elsewhere (hotkey, tray menu); no-op while never opened.
    pub fn sync(&self, settings: Settings) {
        let mut state = self.state.borrow_mut();
        state.current = settings;
        state.paint();
    }

    fn create(&self) -> windows::core::Result<()> {
        let fonts = crate::app::system_fonts().ok_or_else(|| windows::core::Error::new(E_FAIL, "no UI font found"))?;
        let shared = self.state.clone();
        let handler: winclass::Handler = Box::new(move |hwnd, msg, wparam, lparam| {
            // Re-entrant messages (one sent while we hide the window) find the state already borrowed.
            shared.try_borrow_mut().ok().and_then(|mut state| state.on_message(hwnd, msg, wparam, lparam))
        });
        let hwnd = winclass::create_window(
            "MapleLensSettings",
            "MapleLens settings",
            (WS_EX_LAYERED | WS_EX_APPWINDOW, WS_POPUP | WS_SYSMENU),
            (0, 0, 1, 1),
            self.icon,
            handler,
        )?;
        let mut state = self.state.borrow_mut();
        // SAFETY: our own window.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        state.factor = if dpi == 0 { 1.0 } else { dpi as f32 / BASE_DPI };
        state.hwnd = hwnd;
        state.fonts = Some(fonts);
        // Centred on the main screen; the first picture gives the window its size.
        let (w, h) = (settings_view::SIZE.0 * state.factor, settings_view::SIZE.1 * state.factor);
        // SAFETY: screen metrics.
        let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        state.paint_at(Some(((sw - w as i32) / 2, (sh - h as i32) / 2)));
        Ok(())
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        let hwnd = self.hwnd();
        if !hwnd.is_invalid() {
            // SAFETY: our own window.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}
