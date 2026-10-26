//! Windows' built-in OCR engine (Windows.Media.Ocr, no extra install): the fallback for letters the glyph
//! atlas has not learnt yet. Its answers, once the catalog confirms them, teach the atlas those letters.

use std::collections::HashMap;

use maplelens_core::image::GrayImage;
use maplelens_core::vision::Mask;
use maplelens_core::vision::map_tooltip::{LineOcr, TextLine};
use maplelens_core::vision::reader::{Ocr, OcrAnswer, ocr_image};
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::{OcrEngine, OcrResult};
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

pub struct WindowsOcr {
    engine: OcrEngine,
    /// Thin fonts (the name plate) need thicker strokes: the engine drops 1 px lines.
    thicken: bool,
}

// SAFETY: OcrEngine is an agile WinRT object; each WindowsOcr is created and used on one detection thread.
unsafe impl Send for WindowsOcr {}

impl WindowsOcr {
    /// None when no OCR language is installed (Settings > Time & language > Language).
    pub fn new(thicken: bool) -> Option<Self> {
        // SAFETY: per-thread WinRT initialisation; repeated calls on one thread are harmless.
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().ok()?;
        Some(Self { engine, thicken })
    }

    fn recognize_result(&self, image: &GrayImage) -> windows::core::Result<OcrResult> {
        let bgra: Vec<u8> = image.data.iter().flat_map(|&v| [v, v, v, 255]).collect();
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let buffer = writer.DetachBuffer()?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            image.width as i32,
            image.height as i32,
        )?;
        self.engine.RecognizeAsync(&bitmap)?.join()
    }

    /// Text in a dark-on-light image, as it is.
    pub fn recognize_gray(&self, image: &GrayImage) -> windows::core::Result<String> {
        Ok(self.recognize_result(image)?.Text()?.to_string())
    }

    /// Every line in a glyph mask with its position in the mask's pixels (the map helper's tooltips).
    fn lines(&self, mask: &Mask) -> windows::core::Result<Vec<TextLine>> {
        let result = self.recognize_result(&upscaled(mask))?;
        let back = |v: f32| ((v - OCR_MARGIN as f32) / OCR_UPSCALE as f32).round() as i32;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            let rects: Vec<_> = line.Words()?.into_iter().filter_map(|w| w.BoundingRect().ok()).collect();
            if rects.is_empty() {
                continue;
            }
            let left = rects.iter().map(|r| r.X).fold(f32::MAX, f32::min);
            let top = rects.iter().map(|r| r.Y).fold(f32::MAX, f32::min);
            let right = rects.iter().map(|r| r.X + r.Width).fold(f32::MIN, f32::max);
            let bottom = rects.iter().map(|r| r.Y + r.Height).fold(f32::MIN, f32::max);
            let (x, y) = (back(left), back(top));
            let (w, h) = (back(right) - x, back(bottom) - y);
            lines.push(TextLine::new(line.Text()?.to_string().trim(), x, y, w, h));
        }
        Ok(lines)
    }
}

/// Whether an OCR language is installed; tests that need OCR say so and skip without one (as on some CI images).
#[cfg(test)]
pub fn available() -> bool {
    let found = WindowsOcr::new(false).is_some();
    if !found {
        println!("no OCR language installed: skipped");
    }
    found
}

/// Shop and map text is upscaled like the Python reader did (bicubic, x3, a white margin): bilinear blurs the
/// corners that tell "D" from "O".
const OCR_UPSCALE: usize = 3;
const OCR_MARGIN: usize = 20;
const LINE_CACHE: usize = 64;

pub fn upscaled(mask: &Mask) -> GrayImage {
    let scaled = mask.to_ink_on_paper().upscaled_cubic(OCR_UPSCALE);
    let (w, h) = (scaled.width + 2 * OCR_MARGIN, scaled.height + 2 * OCR_MARGIN);
    let mut out = GrayImage { width: w, height: h, data: vec![255; w * h] };
    for y in 0..scaled.height {
        out.data[(y + OCR_MARGIN) * w + OCR_MARGIN..][..scaled.width]
            .copy_from_slice(&scaled.data[y * scaled.width..(y + 1) * scaled.width]);
    }
    out
}

/// Line OCR with its answers remembered by the mask's pixels (a resting cursor shows the same tooltip).
pub struct CachedLines {
    engine: Option<WindowsOcr>,
    cache: HashMap<Vec<u8>, Vec<TextLine>>,
}

impl CachedLines {
    pub fn new() -> Self {
        Self { engine: WindowsOcr::new(false), cache: HashMap::new() }
    }
}

impl LineOcr for CachedLines {
    fn read_lines(&mut self, mask: &Mask) -> Vec<TextLine> {
        let key = mask.key();
        if let Some(lines) = self.cache.get(&key) {
            return lines.clone();
        }
        let lines = match &self.engine {
            Some(engine) => engine.lines(mask).unwrap_or_else(|error| {
                crate::debug!("OCR failed: {error}");
                Vec::new()
            }),
            None => Vec::new(),
        };
        if self.cache.len() >= LINE_CACHE {
            self.cache.clear();
        }
        self.cache.insert(key, lines.clone());
        lines
    }
}

impl Ocr for WindowsOcr {
    fn recognize(&mut self, mask: &Mask) -> OcrAnswer {
        match self.recognize_gray(&ocr_image(mask, self.thicken)) {
            Ok(text) => OcrAnswer::Text(text),
            Err(error) => {
                crate::debug!("OCR failed: {error}");
                OcrAnswer::Text(String::new())
            }
        }
    }
}
