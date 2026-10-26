//! Wires the detection threads to the overlay windows on the UI thread.

mod follow;
mod map;
mod settings;
mod status;
mod updates;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use maplelens_core::config;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::sample::sample_items;
use maplelens_core::pipeline::hover::Hover;
use maplelens_core::pipeline::info::InfoWindow;
use maplelens_core::ranking::{RankCache, Status};
use maplelens_core::render::RgbaImage;
use maplelens_core::render::font::Fonts;
use maplelens_core::settings::Settings;
use maplelens_core::shop::ShopStatus;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};

use crate::layered::LayeredWindow;
use crate::notify::{Notifier, TestAnswer};
use crate::settings_window::{SettingsEvent, SettingsWindow};
use crate::shop::ShopUpdate;
use crate::tray::{Tray, TrayCommand};
use crate::update::check::UpdateChecker;
use crate::update::install::InstallEvent;
use crate::update::{UpdateState, net};
use crate::{icon, maps, paths, refresh, shop, update, workers};

const CARD_CACHE_SIZE: usize = 64;

/// The card's typeface: Tahoma, the game's UI face, with Windows' other UI faces as fallbacks.
pub fn system_fonts() -> Option<Fonts> {
    let dir = std::env::var_os("WINDIR")
        .map_or_else(|| Path::new("C:\\Windows").join("Fonts"), |w| Path::new(&w).join("Fonts"));
    [("tahoma.ttf", "tahomabd.ttf"), ("segoeui.ttf", "segoeuib.ttf"), ("arial.ttf", "arialbd.ttf")]
        .into_iter()
        .find_map(|(regular, bold)| {
            Fonts::from_bytes(std::fs::read(dir.join(regular)).ok()?, std::fs::read(dir.join(bold)).ok()?).ok()
        })
}

/// Prices from the cached snapshot, or the built-in demo items until one is available.
pub fn build_catalog(snapshot: &Path) -> Catalog {
    match refresh::load_catalog(snapshot) {
        Ok(catalog) => {
            crate::info!("loaded {} items from {}", catalog.len(), snapshot.display());
            catalog
        }
        Err(error) => {
            crate::info!("{error}; using the built-in demo items until prices are downloaded");
            Catalog::new(sample_items(), None)
        }
    }
}

#[cfg(test)]
pub fn tests_pump() {
    pump_messages();
}

/// Handle pending window messages so Windows does not consider the overlay unresponsive.
fn pump_messages() {
    let mut msg = MSG::default();
    // SAFETY: standard message loop on the thread that owns our windows.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Cached price cards by (title, expanded, UI scale bits, compare), least recently used first.
type CardCache = Vec<((String, bool, u32, bool), Arc<RgbaImage>)>;
/// The rank panel last drawn, by (name, status, rank's last check, UI scale bits).
type RankCard = Option<((String, Status, Option<String>, u32), Arc<RgbaImage>)>;
/// The status pill last rendered, by (status, UI scale bits); None inside when the status has no pill.
type StatusCard = Option<((ShopStatus, u32), Option<Arc<RgbaImage>>)>;

struct Shown {
    title: String,
    expanded: bool,
}

pub struct App {
    fonts: Fonts,
    catalog: Arc<Catalog>,
    settings: Settings,
    start: Instant,
    window: LayeredWindow,
    hover: Option<Hover>,
    seen_at: f64,
    shown: Option<Shown>,
    since: f64,
    cards: CardCache,
    rank_window: LayeredWindow,
    info: Option<InfoWindow>,
    info_seen_at: f64,
    rank_card: RankCard,
    ranks: Arc<Mutex<RankCache>>,
    shop_events: Receiver<ShopUpdate>,
    shop: ShopUpdate,
    status_window: LayeredWindow,
    status_card: StatusCard,
    map: map::MapUi,
    confirmed: Option<String>,
    enabled: Arc<AtomicBool>,
    tray: Tray,
    settings_window: SettingsWindow,
    /// Sends sold-item notifications; the settings window's Test button goes through it too.
    notifier: Notifier,
    /// The answer to a test message on its way back.
    notify_test: Option<Receiver<TestAnswer>>,
    hover_events: Receiver<Option<Hover>>,
    info_events: Receiver<Option<InfoWindow>>,
    catalog_updates: Receiver<Catalog>,
    to_hover: Sender<Arc<Catalog>>,
    confirmations: Sender<(String, String)>,
    updates: UpdateChecker,
    update_state: UpdateState,
    installer: Option<Receiver<InstallEvent>>,
    /// This exe, which an update replaces.
    exe: PathBuf,
    /// Set once a new exe has been swapped in: start it after this one has shut down.
    relaunch: Option<PathBuf>,
    quit: bool,
}

impl App {
    pub fn new(catalog: Catalog, auto_refresh: bool, debug: bool) -> windows::core::Result<Self> {
        let settings = Settings::from_json(&std::fs::read_to_string(paths::settings_file()).unwrap_or_default());
        settings::sync_startup(settings.launch_at_startup); // also repairs the entry if the exe was moved
        let fonts =
            system_fonts().ok_or_else(|| windows::core::Error::new(windows::core::HRESULT(-1), "no usable font"))?;
        let catalog = Arc::new(catalog);
        let enabled = Arc::new(AtomicBool::new(settings.overlay_enabled));
        let start = Instant::now();
        let clock: Arc<dyn Fn() -> f64 + Send + Sync> = Arc::new(move || start.elapsed().as_secs_f64());
        let ranks = Arc::new(Mutex::new(RankCache::default()));

        let (hover_tx, hover_events) = mpsc::channel();
        let (to_hover, hover_catalogs) = mpsc::channel();
        // Measured off the status bar by the hover thread, read by both detectors ("UI Size: Ideal Ratio").
        let ui = Arc::new(maplelens_core::geometry::UiScale::default());
        workers::spawn_hover(catalog.clone(), hover_catalogs, hover_tx, (enabled.clone(), ui.clone()), debug);
        let (info_tx, info_events) = mpsc::channel();
        let (confirmations, confirm_rx) = mpsc::channel();
        workers::spawn_info(info_tx, confirm_rx, enabled.clone(), ui.clone(), debug);
        // World-map tooltips: their own scanner, lookups (maplestory.io) and card.
        let (map_tx, map_events) = mpsc::channel();
        let (reject_tx, rejects) = mpsc::channel();
        let item_showing = Arc::new(AtomicBool::new(false));
        let flags = maps::ScanFlags { enabled: enabled.clone(), item_showing: item_showing.clone(), ui: ui.clone() };
        maps::spawn_scanner(map_tx, rejects, flags, debug);
        let map = map::MapUi::new(map_events, reject_tx, item_showing, maps::service())?;
        // The Store Permit watcher always runs (the pill needs it); --debug also prints its events, and sold items
        // go to the notification connectors (Discord, Telegram).
        let notifier = Notifier::spawn(settings.notify());
        let (shop_notifier, shop_catalog) = (notifier.clone(), catalog.clone());
        let (shop_tx, shop_events) = mpsc::channel();
        std::thread::Builder::new()
            .name("shop-watcher".into())
            .spawn(move || {
                shop::run(ui, Some(shop_tx), shop::Emit::new(debug, debug, Some(shop_notifier)), shop_catalog);
            })
            .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))?;
        workers::spawn_ranks(ranks.clone(), clock);
        let (catalog_tx, catalog_updates) = mpsc::channel();
        if auto_refresh {
            refresh::spawn(paths::snapshot_file(), catalog_tx);
        }

        let exe = update::install::current_exe();
        let updates = UpdateChecker::spawn(
            env!("CARGO_PKG_VERSION"),
            settings.check_updates,
            Box::new(net::get_bytes),
            update::PUBLIC_KEY,
        );
        let icon = icon::load(None);
        let mut app = Self {
            fonts,
            catalog,
            settings: settings.clone(),
            start,
            window: LayeredWindow::new()?,
            hover: None,
            seen_at: 0.0,
            shown: None,
            since: 0.0,
            cards: Vec::new(),
            rank_window: LayeredWindow::new()?,
            info: None,
            info_seen_at: 0.0,
            rank_card: None,
            ranks,
            shop_events,
            shop: ShopUpdate { hwnd: 0, status: ShopStatus::Hidden },
            status_window: LayeredWindow::new()?,
            status_card: None,
            map,
            confirmed: None,
            enabled,
            tray: Tray::new(icon)?,
            notifier,
            notify_test: None,
            settings_window: SettingsWindow::new(icon, update::install::can_self_update(&exe)),
            hover_events,
            info_events,
            catalog_updates,
            to_hover,
            confirmations,
            updates,
            update_state: UpdateState::default(),
            installer: None,
            exe,
            relaunch: None,
            quit: false,
        };
        app.apply_overlay_enabled(settings.overlay_enabled);
        app.register_hotkey(settings.toggle_hotkey);
        Ok(app)
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Run until Exit (or a finished update). Returns the exe to start afterwards, if an update was installed.
    pub fn run(&mut self) -> Option<PathBuf> {
        // Windows' default timer granularity is ~15.6 ms, which makes sleep() coarse and jittery.
        // SAFETY: paired with timeEndPeriod below.
        unsafe { timeBeginPeriod(1) };
        while !self.quit {
            pump_messages();
            self.handle_commands();
            self.tick();
            std::thread::sleep(Duration::from_secs_f64(config::UI_TICK_S));
        }
        // SAFETY: pairs the timeBeginPeriod above.
        unsafe { timeEndPeriod(1) };
        self.relaunch.take()
    }

    fn handle_commands(&mut self) {
        for command in self.tray.take_commands() {
            match command {
                TrayCommand::Toggle => {
                    let overlay_enabled = !self.settings.overlay_enabled;
                    self.update_settings(Settings { overlay_enabled, ..self.settings.clone() });
                }
                TrayCommand::Settings => {
                    if let Err(error) = self.settings_window.show(self.settings.clone()) {
                        crate::error!("could not open the settings window: {error}");
                    }
                }
                TrayCommand::Update => self.start_update(),
                TrayCommand::Exit => self.quit = true,
            }
        }
        for event in self.settings_window.take_events() {
            match event {
                SettingsEvent::Changed(new) => self.update_settings(new),
                // While the hotkey box has focus the combo must reach it, not fire the toggle.
                SettingsEvent::Capturing(capturing) => {
                    self.tray.set_hotkey(if capturing { 0 } else { self.settings.toggle_hotkey });
                }
                SettingsEvent::TestNotify(service, settings) => {
                    self.notify_test = Some(self.notifier.test(service, settings));
                }
                SettingsEvent::CheckUpdates => self.check_for_update(),
                SettingsEvent::Update => self.start_update(),
            }
        }
    }

    fn tick(&mut self) {
        if let Some(answer) = self.notify_test.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.notify_test = None;
            // A bot's chat found by the test is kept, for the bot it was found with.
            if let Some(chat) = answer.telegram_chat
                && !self.settings.telegram_bot.is_empty()
            {
                self.update_settings(Settings { telegram_chat: chat, ..self.settings.clone() });
            }
            self.settings_window.set_notify_note(answer.service, answer.text);
        }
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.follow_updates())).is_err() {
            crate::error!("update tick failed");
        }
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.follow_status())).is_err() {
            crate::error!("status pill tick failed");
        }
        while let Ok(catalog) = self.catalog_updates.try_recv() {
            let catalog = Arc::new(catalog);
            let _ = self.to_hover.send(catalog.clone());
            self.catalog = catalog;
            self.cards.clear();
            self.hover = None;
            self.shown = None;
        }
        if !self.settings.overlay_enabled {
            // The idle threads keep posting "nothing".
            while self.hover_events.try_recv().is_ok() {}
            while self.info_events.try_recv().is_ok() {}
            self.map.drain();
            return;
        }
        // One bad frame must never kill the overlay.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.follow_mouse())).is_err() {
            crate::error!("overlay tick failed");
        }
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.follow_info_window())).is_err() {
            crate::error!("rank panel tick failed");
        }
        let now = self.now();
        let item_showing = self.hover.is_some();
        let (map, fonts) = (&mut self.map, &self.fonts);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            map.set_item_showing(item_showing);
            map.tick(fonts, now);
        }))
        .is_err()
        {
            crate::error!("map card tick failed");
        }
    }
}
