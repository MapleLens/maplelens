//! The Store Permit watcher on its own thread: game window frames in (Windows Graphics Capture, so it works while
//! the game is covered), status out to the UI (the pill) and events out to the log. `maplelens --shop` runs it
//! alone in the console.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use maplelens_core::config;
use maplelens_core::geometry::{Rect, UiScale, scaled, ui_scale};
use maplelens_core::image::{RgbImage, encode_png};
use maplelens_core::market::catalog::Catalog;
use maplelens_core::notify::announce::Announcer;
use maplelens_core::settings::Settings;
use maplelens_core::shop::watcher::{ReadText, ShopWatcher};
use maplelens_core::shop::{ShopContext, ShopEvent, ShopStatus};
use maplelens_core::vision::Mask;
use maplelens_core::vision::hud::find_bar;
use windows::Win32::Foundation::HWND;

use crate::notify::Notifier;
use crate::{game, ocr, paths, wgc};

/// A shop changes a few times a minute at most, and a sale is believed once two polls agree (~2 s): a slower poll
/// would only add latency, a faster one only costs CPU (a frame is ~70 ms of work at UI scale 0.64).
const POLL_S: f64 = 1.0;
/// While no shop window is on screen each frame costs a full search (~220 ms at Ideal Ratio, where the frame is
/// first enlarged 1.56x), so look less often: every 2 s that was a tenth of a CPU core all session long.
const IDLE_POLL_S: f64 = 4.0;
const NO_GAME_RETRY_S: f64 = 2.0;
/// Remembered OCR answers (the same rows are read again whenever a list shifts).
const OCR_CACHE: usize = 256;
const EVENTS_LOG: &str = "shop_events.log";
const EVIDENCE_DIR: &str = "shop_live";

/// What the UI needs for the pill: the watched game window (as a raw handle, which can cross threads) and the
/// watcher's status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShopUpdate {
    pub hwnd: isize,
    pub status: ShopStatus,
}

/// Where events go: the log in `--debug`; with a console (`--shop`, or `--debug` from a terminal) also printed and
/// appended to debug\shop_events.log, with a capture of the frame for the cases worth a second look.
pub struct Emit {
    console: bool,
    debug_dir: Option<PathBuf>,
    /// Sold items and the store opening or closing are reported here (Discord).
    notifier: Option<Notifier>,
    announcer: std::sync::Mutex<Announcer>,
}

impl Emit {
    pub fn new(console: bool, debug: bool, notifier: Option<Notifier>) -> Self {
        Self {
            console,
            debug_dir: debug.then(|| PathBuf::from("debug")),
            notifier,
            announcer: std::sync::Mutex::default(),
        }
    }

    fn line(&self, message: &str) {
        crate::debug!("shop: {message}");
        if self.console {
            println!("{}  {message}", clock());
        }
        if let Some(dir) = &self.debug_dir
            && std::fs::create_dir_all(dir).is_ok()
            && let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(EVENTS_LOG))
        {
            let _ = writeln!(file, "{}  {message}", crate::log::timestamp());
        }
    }

    /// What just happened goes to the notification connectors (Discord), with whose store it is and its balance.
    fn notify(&self, events: &[ShopEvent], context: &ShopContext) {
        if let Some(notifier) = &self.notifier {
            let mut announcer = self.announcer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            notifier.send(announcer.feed(events, context, paths::now_unix()));
        }
    }

    fn event(&self, event: &ShopEvent, frame: &RgbImage) {
        self.line(&event.describe());
        if let Some(dir) = &self.debug_dir
            && event.wants_evidence()
        {
            let dir = dir.join(EVIDENCE_DIR);
            if std::fs::create_dir_all(&dir).is_ok() {
                let name = format!("{}_{:?}.png", crate::log::timestamp(), event)
                    .replace(|c: char| !c.is_ascii_alphanumeric() && c != '.', "_");
                let _ = std::fs::write(dir.join(name), encode_png(frame));
            }
        }
    }
}

fn clock() -> String {
    let seconds = paths::now_unix() as u64 % 86_400;
    format!("{:02}:{:02}:{:02} UTC", seconds / 3600, seconds / 60 % 60, seconds % 60)
}

/// Windows OCR of a glyph mask, cached by the mask's pixels.
pub fn reader() -> ReadText {
    let engine = ocr::WindowsOcr::new(false);
    if engine.is_none() {
        crate::warn!("Windows OCR unavailable: shop sales are counted but their text is not read");
    }
    let mut cache: HashMap<Vec<u8>, String> = HashMap::new();
    Box::new(move |mask: &Mask| {
        let key = mask.key();
        if let Some(text) = cache.get(&key) {
            return Some(text.clone());
        }
        let text = engine
            .as_ref()
            .and_then(|e| e.recognize_gray(&ocr::upscaled(mask)).ok())
            .map(|text| text.trim().to_string())
            .unwrap_or_default();
        if cache.len() >= OCR_CACHE {
            cache.clear();
        }
        cache.insert(key, text.clone());
        Some(text)
    })
}

/// The game window being watched and its capture.
struct Source {
    hwnd: HWND,
    capture: Option<wgc::WindowCapture>,
}

/// Watch until `updates` is closed (the app quit) or forever (`--shop`).
pub fn run(ui: Arc<UiScale>, updates: Option<Sender<ShopUpdate>>, emit: Emit, catalog: Arc<Catalog>) {
    let new_watcher = || {
        let mut watcher = ShopWatcher::new(reader());
        watcher.set_catalog(catalog.clone());
        watcher
    };
    let mut watcher = new_watcher();
    let mut source: Option<Source> = None;
    let mut last = ShopUpdate { hwnd: 0, status: ShopStatus::Hidden };
    let start = Instant::now();
    loop {
        let delay = match game::find_game_window() {
            None => {
                if source.take().is_some() {
                    emit.line("MapleStory closed; waiting for it to start");
                    watcher = new_watcher();
                }
                NO_GAME_RETRY_S
            }
            Some(hwnd) => {
                if source.as_ref().is_none_or(|s| s.hwnd != hwnd) {
                    source = Some(open(hwnd, &emit));
                }
                let source = source.as_mut().expect("just set");
                if let Some(frame) = grab(source) {
                    let client = game::client_rect(hwnd).unwrap_or_default();
                    measure_ui(&ui, &frame, client);
                    let events = watcher.update(&frame, start.elapsed().as_secs_f64());
                    for event in &events {
                        emit.event(event, &frame);
                    }
                    emit.notify(&events, &watcher.context());
                }
                if watcher.sees_shop() { POLL_S } else { IDLE_POLL_S }
            }
        };
        let now = ShopUpdate {
            hwnd: source.as_ref().map_or(0, |s| s.hwnd.0 as isize),
            status: if source.is_some() { watcher.status() } else { ShopStatus::Hidden },
        };
        if now != last {
            last = now;
            if let Some(updates) = &updates
                && updates.send(now).is_err()
            {
                return; // the app has quit
            }
        }
        std::thread::sleep(Duration::from_secs_f64(delay));
    }
}

fn open(hwnd: HWND, emit: &Emit) -> Source {
    emit.line("Watching MapleStory (open your shop and its Sold Items window)");
    let capture = match wgc::WindowCapture::new(hwnd) {
        Ok(capture) => Some(capture),
        Err(error) => {
            crate::warn!("window capture unavailable ({error}); the shop is only watched while the game is in front");
            None
        }
    };
    Source { hwnd, capture }
}

/// The newest client frame: from the window capture, or (without it) from the screen while the game is in front.
fn grab(source: &mut Source) -> Option<RgbImage> {
    match &mut source.capture {
        Some(capture) => capture.latest().unwrap_or_else(|error| {
            crate::debug!("window capture failed: {error}");
            None
        }),
        None => {
            use maplelens_core::pipeline::Screen;
            let client = game::client_rect(source.hwnd).filter(|_| game::is_foreground(source.hwnd))?;
            Some(crate::capture::GdiScreen::new().grab(client))
        }
    }
}

/// The game's UI scale from its status bar, which the frame shows even while the game is covered ("UI Size:
/// Ideal Ratio" keeps the UI small whatever the window). A hidden or covered bar keeps the last measurement.
pub(crate) fn measure_ui(ui: &UiScale, frame: &RgbImage, client: Rect) {
    if (frame.width as i32, frame.height as i32) != (client.w, client.h) {
        return;
    }
    let height = frame.height.min(scaled(config::STATUS_BAR_STRIP_HEIGHT, ui_scale(client)).max(0) as usize);
    if let Some(bar) = find_bar(&frame.rows_from(frame.height - height)) {
        ui.set(client, bar.width as f32 / config::STATUS_BAR_WIDTH as f32);
    }
}

/// `maplelens --shop`: no overlay, no prices; events to the console and debug\.
pub fn console() {
    println!("Store Permit watcher. Ctrl+C to stop.");
    // Sold items go to Discord here too, when the settings have a webhook: that is how it is tried without the app.
    let settings = Settings::from_json(&std::fs::read_to_string(paths::settings_file()).unwrap_or_default());
    let notifier = Notifier::spawn(settings.notify());
    let catalog = crate::refresh::load_catalog(&paths::snapshot_file())
        .unwrap_or_else(|_| Catalog::new(maplelens_core::market::sample::sample_items(), None));
    run(Arc::new(UiScale::default()), None, Emit::new(true, true, Some(notifier)), Arc::new(catalog));
}
