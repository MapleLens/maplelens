//! The training card's reader on its own thread: the game window's frames (Windows Graphics Capture, so EXP keeps
//! counting while the game is behind another window) into the core's `TrainingWatcher`, its numbers out to the UI,
//! and the sessions saved to training.json. The potions' icons arrive from `potions` as they load.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::GrayImage;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::training::session::Tracker;
use maplelens_core::training::watcher::{Training, TrainingWatcher};
use maplelens_core::vision::reader::{ImageOcr, NoOcr, OcrAnswer};

use crate::shop::{Source, grab};
use crate::{game, ocr, paths, potions, wgc};

/// The watcher decides what is due; this is only how often it is asked.
const POLL_S: f64 = 0.5;
const NO_GAME_RETRY_S: f64 = 2.0;
/// Sessions are written this often while they change (and at once on a new session, a pause, or quitting).
const SAVE_EVERY_S: u64 = 60;
const OCR_CACHE: usize = 64;
/// The minimap's letters are ~13 px tall at UI scale 1 and 8 at Ideal Ratio; Windows OCR wants them bigger, and
/// with paper around them: tight to the letters, it read nothing at Ideal Ratio (measured on a live capture).
const OCR_UPSCALE: usize = 3;
const OCR_PADDING: usize = 30;

/// What the UI asks of the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainingCommand {
    SetPaused(bool),
    NewSession,
}

/// Windows OCR of the minimap's lines, remembered by their pixels (the same map name is read every two seconds).
pub(crate) struct CachedOcr {
    engine: ocr::WindowsOcr,
    cache: HashMap<(usize, Vec<u8>), String>,
}

impl ImageOcr for CachedOcr {
    fn recognize_image(&mut self, image: &GrayImage) -> OcrAnswer {
        let key = (image.width, image.data.clone());
        if let Some(text) = self.cache.get(&key) {
            return OcrAnswer::Text(text.clone());
        }
        let text = self.engine.recognize_gray(&padded(&image.upscaled_cubic(OCR_UPSCALE))).unwrap_or_else(|error| {
            crate::debug!("OCR failed: {error}");
            String::new()
        });
        if self.cache.len() >= OCR_CACHE {
            self.cache.clear();
        }
        self.cache.insert(key, text.clone());
        OcrAnswer::Text(text)
    }
}

/// `image` on a white border.
fn padded(image: &GrayImage) -> GrayImage {
    let (w, h) = (image.width + 2 * OCR_PADDING, image.height + 2 * OCR_PADDING);
    let mut out = GrayImage { width: w, height: h, data: vec![255; w * h] };
    for y in 0..image.height {
        out.data[(y + OCR_PADDING) * w + OCR_PADDING..][..image.width]
            .copy_from_slice(&image.data[y * image.width..(y + 1) * image.width]);
    }
    out
}

pub(crate) fn reader() -> Box<dyn ImageOcr + Send> {
    match ocr::WindowsOcr::new(false) {
        Some(engine) => Box::new(CachedOcr { engine, cache: HashMap::new() }),
        None => {
            crate::warn!("Windows OCR unavailable: the training card cannot read map names");
            Box::new(NoOcr)
        }
    }
}

fn load() -> Tracker {
    std::fs::read_to_string(paths::training_file()).map_or_else(|_| Tracker::new(), |text| Tracker::from_json(&text))
}

fn save(tracker: &Tracker) {
    if let Err(error) = paths::write_atomic(&paths::training_file(), tracker.to_json().as_bytes()) {
        crate::error!("could not save training sessions: {error}");
    }
}

/// The numbers, and the game window they were read from (a raw handle, which can cross threads; 0 for none).
pub type TrainingUpdate = (isize, Training);

/// Read until the app quits (`updates` or `commands` closes).
pub fn run(updates: Sender<TrainingUpdate>, commands: Receiver<TrainingCommand>) {
    let mut watcher = TrainingWatcher::new(reader(), load());
    let icons = potions::spawn();
    let mut source: Option<Source> = None;
    let mut saved_at = Instant::now();
    loop {
        for (id, name, png) in icons.try_iter() {
            if !watcher.add_potion_icon(id, &name, &png) {
                crate::debug!("potion icon {id} ({name}) unreadable");
            }
        }
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    let now = paths::now_unix();
                    match command {
                        TrainingCommand::SetPaused(paused) => watcher.tracker_mut().set_paused(paused),
                        TrainingCommand::NewSession => watcher.tracker_mut().new_session(now),
                    }
                    save(watcher.tracker());
                    let hwnd = source.as_ref().map_or(0, |s| s.hwnd.0 as isize);
                    if updates.send((hwnd, watcher.training(now))).is_err() {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return save(watcher.tracker()),
            }
        }
        let delay = match game::find_game_window() {
            None => {
                source = None;
                NO_GAME_RETRY_S
            }
            Some(hwnd) => {
                if source.as_ref().is_none_or(|s| s.hwnd != hwnd) {
                    source = Some(open(hwnd));
                }
                let now = paths::now_unix();
                if let Some(frame) = source.as_mut().and_then(grab) {
                    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
                    let bounds = Rect::new(0, 0, frame.width as i32, frame.height as i32);
                    if watcher.step(&mut screen, bounds, now)
                        && updates.send((hwnd.0 as isize, watcher.training(now))).is_err()
                    {
                        return save(watcher.tracker());
                    }
                }
                POLL_S
            }
        };
        if saved_at.elapsed() >= Duration::from_secs(SAVE_EVERY_S) {
            saved_at = Instant::now();
            save(watcher.tracker());
        }
        std::thread::sleep(Duration::from_secs_f64(delay));
    }
}

fn open(hwnd: windows::Win32::Foundation::HWND) -> Source {
    let capture = wgc::WindowCapture::new(hwnd)
        .inspect_err(|error| crate::debug!("training: window capture unavailable ({error}); reading while in front"))
        .ok();
    Source { hwnd, capture }
}
