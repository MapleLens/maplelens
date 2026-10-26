//! User settings and their JSON form. A missing, unreadable or partly invalid file never stops the app: bad
//! fields fall back to defaults one by one, unknown ones are ignored.

use serde_json::{Map, Value, json};

/// Win32 hotkey-control encoding: virtual key in the low byte, HOTKEYF_* modifiers (shift 1, ctrl 2, alt 4)
/// above it. Ctrl+Alt+M.
pub const DEFAULT_HOTKEY: u32 = ((0x02 | 0x04) << 8) | b'M' as u32;
pub const EXPAND_DELAY_RANGE_S: (f64, f64) = (0.0, 3.0);

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub launch_at_startup: bool,
    pub overlay_enabled: bool,
    /// 0 = no hotkey.
    pub toggle_hotkey: u32,
    /// How long the pointer rests on an item before the glance card expands.
    pub expand_delay_s: f64,
    /// Look for a newer release at start-up and daily.
    pub check_updates: bool,
    /// The Store Permit watcher's status pill at the game's bottom-left.
    pub store_status_bar: bool,
    /// A Discord channel's webhook URL, where sold items are reported; empty = off.
    pub discord_webhook: String,
    /// A Telegram bot's `sendMessage` URL with its `chat_id`, which gets the same messages; empty = off. Without the
    /// chat until the player tests it after writing to the bot.
    pub telegram_webhook: String,
    /// Tell about sold items (when a connector is set).
    pub notify_sales: bool,
    /// Tell when the store is detected open or closes.
    pub notify_store: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launch_at_startup: false,
            overlay_enabled: true,
            toggle_hotkey: DEFAULT_HOTKEY,
            expand_delay_s: 1.2,
            check_updates: true,
            store_status_bar: true,
            discord_webhook: String::new(),
            telegram_webhook: String::new(),
            notify_sales: true,
            notify_store: true,
        }
    }
}

impl Settings {
    pub fn from_json(text: &str) -> Self {
        let mut settings = Self::default();
        let Ok(Value::Object(raw)) = serde_json::from_str::<Value>(text) else { return settings };
        let flag = |key: &str| raw.get(key).and_then(Value::as_bool);
        if let Some(v) = flag("launch_at_startup") {
            settings.launch_at_startup = v;
        }
        if let Some(v) = flag("overlay_enabled") {
            settings.overlay_enabled = v;
        }
        if let Some(v) = flag("check_updates") {
            settings.check_updates = v;
        }
        if let Some(v) = flag("store_status_bar") {
            settings.store_status_bar = v;
        }
        if let Some(v) = flag("notify_sales") {
            settings.notify_sales = v;
        }
        if let Some(v) = flag("notify_store") {
            settings.notify_store = v;
        }
        // Only a real Discord webhook is kept: anything else typed or pasted is dropped here, not later.
        if let Some(url) = raw.get("discord_webhook").and_then(Value::as_str)
            && let Some(valid) = crate::notify::discord::valid_webhook(url)
        {
            settings.discord_webhook = valid.to_string();
        }
        if let Some(url) = raw.get("telegram_webhook").and_then(Value::as_str)
            && let Some((token, chat)) = crate::notify::telegram::parse_webhook(url)
        {
            settings.telegram_webhook = crate::notify::telegram::webhook_url(token, chat);
        }
        if let Some(v) = raw.get("toggle_hotkey").and_then(Value::as_u64).filter(|&v| v <= 0xFFFF) {
            settings.toggle_hotkey = v as u32;
        }
        if let Some(v) = raw.get("expand_delay_s").filter(|v| v.is_number()).and_then(Value::as_f64)
            && (EXPAND_DELAY_RANGE_S.0..=EXPAND_DELAY_RANGE_S.1).contains(&v)
        {
            settings.expand_delay_s = v;
        }
        settings
    }

    pub fn to_json(&self) -> String {
        let mut map = Map::new();
        map.insert("launch_at_startup".into(), json!(self.launch_at_startup));
        map.insert("overlay_enabled".into(), json!(self.overlay_enabled));
        map.insert("toggle_hotkey".into(), json!(self.toggle_hotkey));
        map.insert("expand_delay_s".into(), json!(self.expand_delay_s));
        map.insert("check_updates".into(), json!(self.check_updates));
        map.insert("store_status_bar".into(), json!(self.store_status_bar));
        map.insert("notify_sales".into(), json!(self.notify_sales));
        map.insert("notify_store".into(), json!(self.notify_store));
        map.insert("discord_webhook".into(), json!(self.discord_webhook));
        map.insert("telegram_webhook".into(), json!(self.telegram_webhook));
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }
}

impl Settings {
    /// What the notification connectors are built from.
    pub fn notify(&self) -> crate::notify::NotifySettings {
        let (bot, chat) = crate::notify::telegram::parse_webhook(&self.telegram_webhook).unwrap_or_default();
        crate::notify::NotifySettings {
            discord_webhook: self.discord_webhook.clone(),
            telegram_bot: bot.to_string(),
            telegram_chat: chat,
            sales: self.notify_sales,
            store: self.notify_store,
        }
    }
}

/// "instantly" / "after 1.2 s": the expand delay as the settings window shows it.
pub fn delay_text(seconds: f64) -> String {
    if seconds <= 0.0 { "instantly".into() } else { format!("after {seconds:.1} s") }
}
