//! OCR in the browser is asynchronous (Tesseract runs in its own worker), while the reader asks synchronously.
//! The queue bridges the two: an unknown mask is queued and answered "pending"; the page drains the queue,
//! recognises the images and posts the answers back; the next frame finds them here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use maplelens_core::image::GrayImage;
use maplelens_core::vision::Mask;
use maplelens_core::vision::reader::{ImageOcr, Ocr, OcrAnswer, ocr_image};

/// Masks waiting at once; more means the page is not keeping up and older ones are stale anyway.
const MAX_WAITING: usize = 4;
/// Grey lines (the minimap's title) are this small; Tesseract wants letters ~30 px tall.
const IMAGE_UPSCALE: usize = 3;
/// Paper around a grey line: OCR read nothing of the minimap's 1x letters held tight (measured with Windows OCR).
const IMAGE_PADDING: usize = 30;
/// Answers remembered (the same rows are read again whenever a list shifts); more are dropped all at once.
const MAX_ANSWERS: usize = 1024;
/// Grey lines this alike (mean difference per pixel, of 255) are one line: the browser's frames are video and never
/// give the same pixels twice, so the minimap's title went to Tesseract on every look (keeping it busy while a
/// tooltip's title waited, and bringing a new misread to settle each time).
const SAME_LINE_DIFF: f64 = 6.0;
/// Grey lines answered lately, kept to compare new ones with (the minimap's two, with room for a map change).
const RECENT_LINES: usize = 6;
static NEXT_ID: Mutex<u32> = Mutex::new(1);

pub struct OcrQueue {
    thicken: bool,
    max_waiting: usize,
    /// Off until the page says it can recognise images (the player ticked the box): jobs nobody will answer only
    /// pile up, in the engine and in the page (a minute of a compressed stream queued hundreds of megabytes).
    enabled: bool,
    answers: HashMap<Vec<u8>, String>,
    waiting: Vec<(u32, Vec<u8>, GrayImage)>,
    /// Grey lines answered lately (as asked, before enlarging) and their answers, newest last.
    lines: Vec<(GrayImage, String)>,
    /// Grey lines waiting, by job id, to join `lines` once answered.
    asked_lines: Vec<(u32, GrayImage)>,
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
            lines: Vec::new(),
            asked_lines: Vec::new(),
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
        if let Some(index) = self.asked_lines.iter().position(|(i, _)| *i == id) {
            let (_, line) = self.asked_lines.remove(index);
            if self.lines.len() >= RECENT_LINES {
                self.lines.remove(0);
            }
            self.lines.push((line, text.to_string()));
        }
    }

    fn ask(&mut self, mask: &Mask) -> OcrAnswer {
        let thicken = self.thicken;
        self.ask_key(mask.key(), || ocr_image(mask, thicken))
    }

    /// A grey line (dark ink on white), enlarged and padded for Tesseract; the answer of one alike it, when there is.
    fn ask_image(&mut self, image: &GrayImage) -> OcrAnswer {
        if let Some((_, text)) = self.lines.iter().rev().find(|(line, _)| alike(line, image)) {
            return OcrAnswer::Text(text.clone());
        }
        let waiting = |id: &u32| self.waiting.iter().any(|(w, ..)| w == id);
        if self.asked_lines.iter().any(|(id, line)| waiting(id) && alike(line, image)) {
            return OcrAnswer::Pending; // one alike it is being read already
        }
        let mut key = image.data.clone();
        key.extend((image.width as u32).to_le_bytes());
        let before = self.waiting.last().map(|(id, ..)| *id);
        let answer = self.ask_key(key, || {
            let big = image.upscaled_cubic(IMAGE_UPSCALE);
            let (w, h) = (big.width + 2 * IMAGE_PADDING, big.height + 2 * IMAGE_PADDING);
            let mut out = GrayImage { width: w, height: h, data: vec![255; w * h] };
            for y in 0..big.height {
                out.data[(y + IMAGE_PADDING) * w + IMAGE_PADDING..][..big.width]
                    .copy_from_slice(&big.data[y * big.width..(y + 1) * big.width]);
            }
            out
        });
        // A new job: remember its line, to compare later ones with once it is answered.
        if let Some((id, ..)) = self.waiting.last().filter(|(id, ..)| Some(*id) != before) {
            self.asked_lines.retain(|(i, _)| self.waiting.iter().any(|(w, ..)| w == i));
            self.asked_lines.push((*id, image.clone()));
        }
        answer
    }

    fn ask_key(&mut self, key: Vec<u8>, image: impl FnOnce() -> GrayImage) -> OcrAnswer {
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
            self.waiting.push((id.unwrap_or(0), key, image()));
        }
        OcrAnswer::Pending
    }
}

/// The same grey line, give or take a video frame's noise?
fn alike(a: &GrayImage, b: &GrayImage) -> bool {
    if a.width != b.width || a.height != b.height || a.data.is_empty() {
        return false;
    }
    let diff: u64 = a.data.iter().zip(&b.data).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    diff as f64 / a.data.len() as f64 <= SAME_LINE_DIFF
}

/// The reader's view of a queue.
pub struct QueuedOcr(pub Arc<Mutex<OcrQueue>>);

impl Ocr for QueuedOcr {
    fn recognize(&mut self, mask: &Mask) -> OcrAnswer {
        self.0.lock().map_or(OcrAnswer::Unavailable, |mut queue| queue.ask(mask))
    }
}

/// The training reader's view of a queue (the minimap's lines).
pub struct QueuedImageOcr(pub Arc<Mutex<OcrQueue>>);

impl ImageOcr for QueuedImageOcr {
    fn recognize_image(&mut self, image: &GrayImage) -> OcrAnswer {
        self.0.lock().map_or(OcrAnswer::Unavailable, |mut queue| queue.ask_image(image))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(shift: u8) -> GrayImage {
        let data = (0..40u8).map(|i| if i % 5 == 0 { 40 } else { 255 - shift }).collect();
        GrayImage { width: 20, height: 2, data }
    }

    #[test]
    fn a_line_alike_one_read_or_being_read_is_not_sent_again() {
        let queue = OcrQueue::shared(false);
        let mut queue = queue.lock().unwrap();
        queue.set_enabled(true);
        assert_eq!(queue.ask_image(&line(0)), OcrAnswer::Pending);
        // The next video frame of the same title: no second job while the first is read.
        assert_eq!(queue.ask_image(&line(3)), OcrAnswer::Pending);
        assert_eq!(queue.waiting(), 1);
        let id = queue.jobs().next().unwrap().0;
        queue.answer(id, "Henesys");
        assert_eq!(queue.ask_image(&line(4)), OcrAnswer::Text("Henesys".into()));
        // Another title is read anew.
        let other = GrayImage { data: line(0).data.iter().rev().copied().collect(), ..line(0) };
        assert_eq!(queue.ask_image(&other), OcrAnswer::Pending);
        assert_eq!(queue.waiting(), 1);
    }
}
