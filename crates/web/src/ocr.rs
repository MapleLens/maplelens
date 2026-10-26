//! OCR in the browser is asynchronous (Tesseract runs in its own worker), while the reader asks synchronously.
//! The queue bridges the two: an unknown mask is queued and answered "pending"; the page drains the queue,
//! recognises the images and posts the answers back; the next frame finds them here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use maplelens_core::image::GrayImage;
use maplelens_core::vision::Mask;
use maplelens_core::vision::reader::{Ocr, OcrAnswer, ocr_image};

/// Masks waiting at once; more means the page is not keeping up and older ones are stale anyway.
const MAX_WAITING: usize = 4;
/// Answers remembered (the same rows are read again whenever a list shifts); more are dropped all at once.
const MAX_ANSWERS: usize = 1024;
static NEXT_ID: Mutex<u32> = Mutex::new(1);

pub struct OcrQueue {
    thicken: bool,
    max_waiting: usize,
    /// Off until the page says it can recognise images (the player ticked the box): jobs nobody will answer only
    /// pile up, in the engine and in the page (a minute of a compressed stream queued hundreds of megabytes).
    enabled: bool,
    answers: HashMap<Vec<u8>, String>,
    waiting: Vec<(u32, Vec<u8>, GrayImage)>,
}

impl OcrQueue {
    pub fn shared(thicken: bool) -> Arc<Mutex<Self>> {
        Self::shared_with(thicken, MAX_WAITING)
    }

    /// A queue that holds `max_waiting` jobs at once (the shop's burst of lines needs more than a title does).
    pub fn shared_with(thicken: bool, max_waiting: usize) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            thicken,
            max_waiting,
            enabled: false,
            answers: HashMap::new(),
            waiting: Vec::new(),
        }))
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.waiting.clear();
        }
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    pub fn jobs(&self) -> impl Iterator<Item = (u32, &GrayImage)> {
        self.waiting.iter().map(|(id, _, image)| (*id, image))
    }

    pub fn job_image(&self, id: u32) -> Option<&GrayImage> {
        self.waiting.iter().find(|(i, ..)| *i == id).map(|(.., image)| image)
    }

    pub fn answer(&mut self, id: u32, text: &str) {
        if let Some(index) = self.waiting.iter().position(|(i, ..)| *i == id) {
            let (_, key, _) = self.waiting.remove(index);
            if self.answers.len() >= MAX_ANSWERS {
                self.answers.clear();
            }
            self.answers.insert(key, text.to_string());
        }
    }

    fn ask(&mut self, mask: &Mask) -> OcrAnswer {
        let key = mask.key();
        if let Some(text) = self.answers.get(&key) {
            return OcrAnswer::Text(text.clone());
        }
        if !self.enabled {
            return OcrAnswer::Unavailable;
        }
        if !self.waiting.iter().any(|(_, k, _)| *k == key) {
            if self.waiting.len() >= self.max_waiting {
                self.waiting.remove(0);
            }
            let id = NEXT_ID.lock().map(|mut n| {
                *n += 1;
                *n
            });
            self.waiting.push((id.unwrap_or(0), key, ocr_image(mask, self.thicken)));
        }
        OcrAnswer::Pending
    }
}

/// The reader's view of a queue.
pub struct QueuedOcr(pub Arc<Mutex<OcrQueue>>);

impl Ocr for QueuedOcr {
    fn recognize(&mut self, mask: &Mask) -> OcrAnswer {
        self.0.lock().map_or(OcrAnswer::Unavailable, |mut queue| queue.ask(mask))
    }
}
