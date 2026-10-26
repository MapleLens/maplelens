//! Proof of concept: reading the shop's text lines with the glyph matcher against the current mechanism (Windows OCR).
//!
//! Step 1 (`poc_extract`): the distinct text lines (chat lines and the three lines of each Sold Items row) of a folder
//! of captured shop frames, each with what Windows OCR reads and how long it took, written to `MAPLELENS_POC_OUT`
//! (`lines.json`, and `png/<id>.png` as the browser's OCR is fed them).
//! Step 2 (`poc_atlas`): the same lines read by an atlas learnt from labelled lines (cross-validated), with timings.
//!
//! `MAPLELENS_SHOP_FRAMES=<folder> MAPLELENS_POC_OUT=<folder> cargo test --release -p maplelens poc_extract -- --ignored`

use std::collections::HashMap;
use std::time::Instant;

use maplelens_core::image::{GrayImage, RgbImage, decode_png, encode_png};
use maplelens_core::shop::layout::{self, View};
use maplelens_core::shop::lines::Line;
use maplelens_core::shop::{chatlog, soldlist};
use maplelens_core::vision::Mask;
use maplelens_core::vision::reader::ocr_image;
use serde_json::{Value, json};

use crate::ocr;

/// The margin OCR is given around a line (as the watcher gives it).
const PAD: usize = 4;

pub(crate) fn padded(mask: &Mask) -> Mask {
    let mut out = Mask::new(mask.width + 2 * PAD, mask.height + 2 * PAD);
    for y in 0..mask.height {
        for x in 0..mask.width {
            out.set(x + PAD, y + PAD, mask.get(x, y));
        }
    }
    out
}

fn rows_of(mask: &Mask) -> Vec<String> {
    (0..mask.height).map(|y| (0..mask.width).map(|x| if mask.get(x, y) { '#' } else { '.' }).collect()).collect()
}

/// The grey values of `area` of `frame`, one string per row, two hex digits a pixel.
fn gray_rows(frame: &RgbImage, (x, y, w, h): (i32, i32, i32, i32)) -> Vec<String> {
    let gray = frame.to_gray();
    (y.max(0)..(y + h).min(gray.height as i32))
        .map(|row| {
            (x.max(0)..(x + w).min(gray.width as i32))
                .map(|col| format!("{:02x}", gray.get(col as usize, row as usize)))
                .collect()
        })
        .collect()
}

fn gray_to_rgb(image: &GrayImage) -> RgbImage {
    let mut out = RgbImage::new(image.width, image.height);
    for y in 0..image.height {
        for x in 0..image.width {
            let v = image.get(x, y);
            out.set_pixel(x, y, [v, v, v]);
        }
    }
    out
}

struct Sample {
    kind: &'static str,
    mask: Mask,
    /// The line's window of the frame in grey (`gray[y]` is one hex pair per pixel), for trying other thresholds.
    gray: Vec<String>,
    frame: String,
    seen: u32,
}

/// Every line the watcher would read in `frame`, with its kind.
fn lines_of(frame: &RgbImage) -> Vec<(&'static str, Line)> {
    let Some(located) = layout::locate(frame, None, &soldlist::has_list) else { return Vec::new() };
    let (anchors, view) = (located.anchors, View { frame, scale: located.scale });
    let mut out = Vec::new();
    if let Some(enter) = anchors.enter {
        for line in chatlog::read_lines(&view, enter).unwrap_or_default() {
            out.push(("chat", line));
        }
    }
    if let Some(sold_x) = anchors.sold_x {
        for row in soldlist::read_rows(&view, sold_x).unwrap_or_default() {
            if row.is_total {
                continue;
            }
            for (i, line) in row.lines.into_iter().enumerate() {
                out.push((["sold_item", "sold_buyer", "sold_qty"][i.min(2)], line));
            }
        }
    }
    out
}

#[test]
#[ignore = "needs MAPLELENS_SHOP_FRAMES, MAPLELENS_POC_OUT and Windows OCR"]
fn poc_extract() {
    let (Ok(folder), Ok(out)) = (std::env::var("MAPLELENS_SHOP_FRAMES"), std::env::var("MAPLELENS_POC_OUT")) else {
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    let mut distinct: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut samples: Vec<Sample> = Vec::new();
    for path in files.iter().filter(|p| p.extension().is_some_and(|e| e == "png")) {
        let Ok(frame) = decode_png(&std::fs::read(path).unwrap()) else { continue };
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        for (kind, line) in lines_of(&frame) {
            if line.glyphs.width == 0 {
                continue;
            }
            let key = line.glyphs.key();
            match distinct.get(&key) {
                Some(&i) => samples[i].seen += 1,
                None => {
                    distinct.insert(key, samples.len());
                    let gray = line.source.map(|(area, _)| gray_rows(&frame, area)).unwrap_or_default();
                    samples.push(Sample { kind, mask: line.glyphs, gray, frame: name.clone(), seen: 1 });
                }
            }
        }
    }
    std::fs::create_dir_all(format!("{out}/png")).unwrap();
    let engine = ocr::WindowsOcr::new(false).expect("Windows OCR");
    // A warm-up read, so the first line does not pay for loading the engine.
    let _ = engine.recognize_gray(&ocr::upscaled(&padded(&samples[0].mask)));
    let mut entries = Vec::new();
    let (mut total_ms, mut kinds) = (0.0, HashMap::<&str, u32>::new());
    for (id, sample) in samples.iter().enumerate() {
        let input = padded(&sample.mask);
        let start = Instant::now();
        let text = engine.recognize_gray(&ocr::upscaled(&input)).map(|t| t.trim().to_string()).unwrap_or_default();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        total_ms += ms;
        *kinds.entry(sample.kind).or_default() += 1;
        std::fs::write(format!("{out}/png/{id}.png"), encode_png(&gray_to_rgb(&ocr_image(&input, false)))).unwrap();
        entries.push(json!({
            "id": id, "kind": sample.kind, "frame": sample.frame, "seen": sample.seen,
            "windows_ocr": text, "windows_ms": ms, "rows": rows_of(&sample.mask), "gray": sample.gray,
        }));
    }
    std::fs::write(format!("{out}/lines.json"), serde_json::to_string(&Value::Array(entries)).unwrap()).unwrap();
    eprintln!(
        "{} distinct lines {kinds:?}; Windows OCR {:.1} ms a line",
        samples.len(),
        total_ms / samples.len() as f64
    );
}
