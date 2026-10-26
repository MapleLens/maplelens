//! Background threads. Detection threads talk to the UI only through channels (UI calls stay on the main
//! thread); each finds the game window itself and goes quiet whenever the game is not in front.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use maplelens_core::config;
use maplelens_core::geometry::{Rect, UiScale, scaled};
use maplelens_core::image::encode_png;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::pipeline::Screen;
use maplelens_core::pipeline::hover::{Hover, HoverDetector};
use maplelens_core::pipeline::info::{InfoDetector, InfoWindow};
use maplelens_core::ranking::{Fetched, RankCache, look_alikes};
use maplelens_core::vision::glyphs::Atlas;
use maplelens_core::vision::pixel_font::{PixelFont, small_name_font};
use maplelens_core::vision::reader::{NoOcr, Ocr, TextReader, name_atlas, title_atlas};
use windows::Win32::Foundation::HWND;

use crate::capture::GdiScreen;
use crate::{game, net, ocr, paths};

fn seconds(s: f64) -> Duration {
    Duration::from_secs_f64(s)
}

/// The bundled atlas plus whatever this machine has learnt before.
fn atlas(bundled: Atlas, font: &str) -> Atlas {
    let mut atlas = bundled;
    if let Ok(text) = std::fs::read_to_string(paths::learnt_glyphs(font))
        && let Ok(learnt) = Atlas::from_json(&text)
    {
        atlas.merge(&learnt);
    }
    atlas
}

fn reader(bundled: Atlas, font: &str, thicken: bool) -> TextReader {
    let ocr: Box<dyn Ocr + Send> = match ocr::WindowsOcr::new(thicken) {
        Some(engine) => Box::new(engine),
        None => {
            crate::warn!("Windows OCR unavailable (no OCR language installed): unknown letters stay unread");
            Box::new(NoOcr)
        }
    };
    TextReader::new(atlas(bundled, font), ocr)
}

/// The small UI's name font: the bundled letters and those learnt on this computer from confirmed names.
fn pixel_font() -> PixelFont {
    let mut font = small_name_font();
    if let Ok(text) = std::fs::read_to_string(paths::learnt_glyphs(PIXEL_FONT_FILE))
        && let Some(learnt) = PixelFont::from_json(&text)
    {
        font.merge(&learnt);
    }
    font
}

const PIXEL_FONT_FILE: &str = "name-small";

fn save_pixel_font(font: &PixelFont) {
    let path = paths::learnt_glyphs(PIXEL_FONT_FILE);
    match paths::write_atomic(&path, font.to_json().as_bytes()) {
        Ok(()) => crate::info!("learnt new small name letters ({} pieces)", font.len()),
        Err(error) => crate::warn!("could not save learnt letters to {}: {error}", path.display()),
    }
}

fn save_learnt(reader: &mut TextReader, font: &str) {
    if reader.take_learned() {
        let path = paths::learnt_glyphs(font);
        match paths::write_atomic(&path, reader.atlas().to_json().as_bytes()) {
            Ok(()) => crate::info!("learnt new {font} letters ({} glyphs)", reader.atlas().glyphs.len()),
            Err(error) => crate::warn!("could not save learnt letters to {}: {error}", path.display()),
        }
    }
}

/// The game window while it is in front: None means "not now" and the caller waits `delay`.
#[derive(Default)]
pub(crate) struct GameWatch {
    hwnd: Option<HWND>,
}

impl GameWatch {
    pub(crate) fn front(&mut self) -> Result<HWND, f64> {
        if !self.hwnd.is_some_and(game::is_alive) {
            self.hwnd = game::find_game_window();
        }
        let hwnd = self.hwnd.ok_or(config::GAME_LOST_RETRY_S)?;
        if game::is_foreground(hwnd) { Ok(hwnd) } else { Err(config::POLL_BACKGROUND_S) }
    }
}

/// Mouse -> tooltip -> item, sent as `Some(Hover)` / `None` (no tooltip) on `events`.
pub fn spawn_hover(
    catalog: Arc<Catalog>,
    updates: Receiver<Arc<Catalog>>,
    events: Sender<Option<Hover>>,
    (enabled, ui): (Arc<AtomicBool>, Arc<UiScale>),
    debug: bool,
) {
    std::thread::Builder::new()
        .name("hover-pipeline".into())
        .spawn(move || {
            let mut detector =
                HoverDetector::new(catalog, reader(title_atlas(), "title", false)).with_ui_scale(ui.clone());
            let mut logged_scale = 0.0f32;
            if debug {
                detector.captures = Some(Vec::new());
            }
            let (mut screen, mut watch) = (GdiScreen::new(), GameWatch::default());
            let start = Instant::now();
            loop {
                while let Ok(catalog) = updates.try_recv() {
                    detector.set_catalog(catalog);
                }
                let wait =
                    if !enabled.load(Ordering::Relaxed) { Err(config::POLL_BACKGROUND_S) } else { watch.front() };
                let (event, delay) = match wait {
                    Err(delay) => (None, delay), // never draw over other apps
                    Ok(hwnd) => match game::client_rect(hwnd) {
                        Some(bounds) => {
                            let hover = detector.step(&mut screen, bounds, start.elapsed().as_secs_f64());
                            let scale = ui.of(bounds);
                            if (scale - logged_scale).abs() > 0.005 {
                                crate::info!("UI scale {scale:.3} (client {}x{})", bounds.w, bounds.h);
                                logged_scale = scale;
                            }
                            let delay = if hover.is_some() { config::POLL_S } else { config::HOVER_IDLE_POLL_S };
                            (hover, delay)
                        }
                        None => (None, config::POLL_BACKGROUND_S),
                    },
                };
                if events.send(event).is_err() {
                    return; // the app has quit
                }
                save_learnt(detector.reader_mut(), "title");
                if let Some(captures) = detector.captures.as_mut() {
                    for (name, image) in captures.drain(..) {
                        save_capture(&name, &encode_png(&image));
                    }
                }
                std::thread::sleep(seconds(delay));
            }
        })
        .expect("spawning the hover thread");
}

/// Debug aid: keep the raw capture of each new tooltip in debug/ (the source of test fixtures).
fn save_capture(name: &str, png: &[u8]) {
    let file: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let dir = Path::new("debug");
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(dir.join(format!("{}.png", if file.is_empty() { "untitled" } else { &file })), png);
    }
}

/// Debug aid: each new read of a Character Info window, as the game drew it, in debug/names/ (the name corpus).
fn save_info_window(screen: &mut GdiScreen, info: &InfoWindow) {
    let (w, h) = config::INFO_WINDOW_SIZE;
    let area = Rect::new(info.rect.x, info.rect.y, scaled(w, info.scale), scaled(h, info.scale));
    let file: String = info.read.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let dir = Path::new("debug").join("names");
    if std::fs::create_dir_all(&dir).is_ok() {
        let stamp = (paths::now_unix() * 1000.0) as u64;
        let _ = std::fs::write(dir.join(format!("{stamp}_{file}.png")), encode_png(&screen.grab(area)));
    }
}

/// The Character Info window and whose it is, sent as `Some(InfoWindow)` / `None` on `events`.
pub fn spawn_info(
    events: Sender<Option<InfoWindow>>,
    confirmations: Receiver<(String, String)>,
    enabled: Arc<AtomicBool>,
    ui: Arc<UiScale>,
    debug: bool,
) {
    std::thread::Builder::new()
        .name("info-pipeline".into())
        .spawn(move || {
            // The UI scale is measured by the hover thread, off the status bar.
            let mut detector =
                InfoDetector::new(reader(name_atlas(), "name", true)).with_ui_scale(ui).with_pixel_font(pixel_font());
            let (mut screen, mut watch) = (GdiScreen::new(), GameWatch::default());
            let mut shown: Option<Option<String>> = None; // what the log last said: no window, or a window and its name
            loop {
                // A rank lookup that found the name proves the read right: learn its letters.
                while let Ok((read, truth)) = confirmations.try_recv() {
                    detector.confirm(&read, &truth);
                }
                save_learnt(detector.reader_mut(), "name");
                if detector.take_pixel_learnt() {
                    save_pixel_font(detector.pixel_font());
                }
                let wait = if enabled.load(Ordering::Relaxed) { watch.front() } else { Err(config::POLL_BACKGROUND_S) };
                let (event, delay) = match wait {
                    Err(delay) => {
                        detector.reset();
                        (None, delay)
                    }
                    Ok(hwnd) => match game::client_rect(hwnd).and_then(|bounds| detector.step(&mut screen, bounds)) {
                        Some(info) => (Some(info), config::INFO_POLL_S),
                        None => (None, config::INFO_SEARCH_S),
                    },
                };
                let now_shown = event.as_ref().map(|info| info.name.clone());
                if debug
                    && let Some(info) = &event
                    && now_shown != shown
                {
                    save_info_window(&mut screen, info);
                }
                if now_shown != shown {
                    match &event {
                        Some(info) => crate::debug!("character info: {:?} (read {:?})", info.name, info.read),
                        None => crate::debug!("character info closed"),
                    }
                    shown = now_shown;
                }
                if events.send(event).is_err() {
                    return;
                }
                std::thread::sleep(seconds(delay));
            }
        })
        .expect("spawning the info thread");
}

/// Other spellings tried when classicrank.com does not know a read name (each one is another request).
const LOOK_ALIKE_TRIES: usize = 3;

/// The rank of `name`, or of an I/l look-alike when the name as read is unknown (`ranking::look_alikes`).
fn fetch_rank_spellings(name: &str) -> Fetched {
    let fetched = net::fetch_rank(name);
    if !matches!(fetched, Fetched::Missing) {
        return fetched;
    }
    for other in look_alikes(name, LOOK_ALIKE_TRIES) {
        std::thread::sleep(seconds(config::RANK_REQUEST_GAP_S));
        let again = net::fetch_rank(&other);
        if !matches!(again, Fetched::Missing) {
            crate::debug!("rank {name}: found as {other}");
            return again;
        }
    }
    Fetched::Missing
}

/// Works through the rank cache's queue, one request at a time, paced under the API's rate limit.
pub fn spawn_ranks(cache: Arc<std::sync::Mutex<RankCache>>, clock: Arc<dyn Fn() -> f64 + Send + Sync>) {
    std::thread::Builder::new()
        .name("rank-fetch".into())
        .spawn(move || {
            loop {
                let due = cache.lock().unwrap_or_else(|e| e.into_inner()).next_due();
                match due {
                    Some(name) => {
                        let fetched = fetch_rank_spellings(&name);
                        let outcome = match &fetched {
                            Fetched::Found(_) => "found".to_string(),
                            Fetched::Missing => "not ranked".to_string(),
                            Fetched::Failed(retry) => format!("failed, retry in {retry:.0} s"),
                        };
                        crate::debug!("rank {name}: {outcome}");
                        cache.lock().unwrap_or_else(|e| e.into_inner()).store(&name, fetched, clock());
                        std::thread::sleep(seconds(config::RANK_REQUEST_GAP_S));
                    }
                    None => std::thread::sleep(seconds(0.1)),
                }
            }
        })
        .expect("spawning the rank thread");
}
