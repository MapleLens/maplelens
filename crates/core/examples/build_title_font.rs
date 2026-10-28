//! Learn the tooltip titles' bold bitmap font from rendered scenes (`scripts/scenes.py`, whose labels give every
//! title's exact text and box) and write `assets/glyphs/title_small.json`:
//! `cargo run --release -p maplelens-core --example build_title_font -- SCENES_DIR... [--check]`.
//! Learns from the training split (scene id not a multiple of 5) and reports how the held-out split's titles read,
//! cut out the way the reader cuts them (`title_font::title_band`). `--check` only reads, with the bundled table.

use maplelens_core::image::decode_png;
use maplelens_core::shop::text::settle_bars;
use maplelens_core::vision::pixel_font::PixelFont;
use maplelens_core::vision::title_font::{SPACE_GAP, read_title, title_band, title_font, title_mask};
use serde_json::Value;

/// A title this wide (UI scale 1) may run past the tooltip's edge, which cuts it: not a whole word to learn from.
const LONGEST_WHOLE: f64 = 400.0;

/// The game's UI scale for a scene's window, as the app measures it.
fn ui_scale(d: &Value) -> f32 {
    let (w, h) = (d["width"].as_u64().unwrap_or(0), d["height"].as_u64().unwrap_or(0));
    let ideal = d["ui_size"] == "ideal";
    if w < 1920 || h < 1080 || (ideal && h < 1536) {
        0.641
    } else if ideal {
        1.282
    } else {
        h as f32 / 1200.0
    }
}

struct Title {
    id: u64,
    pixels: maplelens_core::image::RgbImage,
    scale: f32,
    text: String,
    /// The tooltip's top-left corner and its native crop, when the scene says where the mouse was.
    tooltip: Option<maplelens_core::image::RgbImage>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let dirs: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    assert!(!dirs.is_empty(), "a scenes folder");
    let mut titles = Vec::new();
    for dir in dirs {
        for line in std::fs::read_to_string(format!("{dir}/labels.jsonl")).unwrap().lines() {
            let Ok(label) = serde_json::from_str::<Value>(line) else { continue };
            let Some(item) = label["item"].as_str() else { continue };
            let d = &label["display"];
            // The renderer draws Ideal Ratio at 1440 px high at 1x, where the game draws 2x.
            if d["ui_size"] == "ideal" && d["height"].as_u64().is_some_and(|h| (1440..1536).contains(&h)) {
                continue;
            }
            let scale = ui_scale(&label["display"]);
            let Ok(frame) =
                decode_png(&std::fs::read(format!("{dir}/{}", label["files"][0].as_str().unwrap())).unwrap())
            else {
                continue;
            };
            let prefix: String = item.chars().take(6).collect();
            let Some(t) = label["texts"][0].as_array().and_then(|ts| {
                ts.iter()
                    // The title's own colour (white, or an enhanced item's orange), not a store list's black row.
                    .filter(|t| t[0].as_str().is_some_and(|s| s.starts_with(&prefix)) && t[2] != "0xFF000000")
                    .min_by_key(|t| t[1][1].as_i64())
            }) else {
                continue;
            };
            let b: Vec<i64> = t[1].as_array().unwrap().iter().filter_map(Value::as_i64).collect();
            if b[2] as f64 / scale as f64 > LONGEST_WHOLE {
                continue;
            }
            let m = (4.0 * scale).ceil() as i64;
            let (x0, y0) = ((b[0] - m).max(0) as usize, (b[1] - m).max(0) as usize);
            let (x1, y1) =
                (((b[0] + b[2] + m) as usize).min(frame.width), ((b[1] + b[3] + m) as usize).min(frame.height));
            let tooltip = label["mouse"].as_object().and_then(|mouse| {
                let tx = mouse["x"].as_i64()? + (scale as f64).round() as i64;
                let ty = mouse["y"].as_i64()? + (32.0 * scale as f64).round() as i64;
                // Only where the game opened it: not slid by a clamp.
                (b[0] > tx && b[1] > ty && (b[1] - ty) as f64 / (scale as f64) < 40.0).then(|| {
                    frame.crop(
                        tx as usize,
                        ty as usize,
                        frame.width,
                        (ty as usize + (120.0 * scale) as usize).min(frame.height),
                    )
                })
            });
            titles.push(Title {
                id: label["id"].as_u64().unwrap_or(0),
                pixels: frame.crop(x0, y0, x1, y1),
                scale,
                text: t[0].as_str().unwrap().trim().to_string(),
                tooltip,
            });
        }
    }
    let (eval, train): (Vec<&Title>, Vec<&Title>) = titles.iter().partition(|t| t.id.is_multiple_of(5));
    let mut font = if check { title_font().clone() } else { PixelFont::default() };
    if !check {
        let samples: Vec<_> =
            train.iter().filter_map(|t| Some((title_mask(&t.pixels, t.scale)?, t.text.replace(' ', "")))).collect();
        // Single letters first; then, with them known, the shapes of letters drawn touching.
        for round in 0..3 {
            let added = font.learn_by_vote(&samples, 2);
            println!("round {round}: {added} shapes added ({} in all)", font.len());
        }
    }
    let (mut exact, mut gaps, mut wrong, mut none, mut banded, mut band_exact) = (0, 0, 0, 0, 0, 0);
    for t in &eval {
        let got = title_mask(&t.pixels, t.scale).map(|m| {
            let (text, n) = font.read_spaced(&m, SPACE_GAP);
            (settle_bars(&text), n)
        });
        match got {
            Some((text, 0)) if text == t.text => exact += 1,
            Some((_, n)) if n > 0 => gaps += 1,
            Some((text, _)) => {
                wrong += 1;
                println!("  XX {:.2} {:?} -> {text:?}", t.scale, t.text);
            }
            None => none += 1,
        }
        // As the hover detector cuts it out of the tooltip.
        if let Some(tooltip) = &t.tooltip {
            banded += 1;
            let read = read_with(&font, tooltip, t.scale, tooltip_width(tooltip, t.scale));
            if read.as_deref() == Some(t.text.as_str()) {
                band_exact += 1;
            } else if std::env::var("TITLE_DEBUG").is_ok() {
                let band = title_band(tooltip, t.scale, tooltip_width(tooltip, t.scale), 0);
                let mask = band.as_ref().and_then(|b| title_mask(b, t.scale));
                println!(
                    "  band {:.2} {:?} -> {read:?} {:?}",
                    t.scale,
                    t.text,
                    mask.as_ref().map(|m| font.read_spaced(m, SPACE_GAP))
                );
                if std::env::var("TITLE_DEBUG").as_deref() == Ok(t.text.as_str())
                    && let Some(m) = title_mask(&t.pixels, t.scale)
                {
                    println!("  label crop:");
                    for y in 0..m.height {
                        println!(
                            "    {}",
                            (0..m.width.min(120)).map(|x| if m.get(x, y) { '#' } else { '.' }).collect::<String>()
                        );
                    }
                }
                if let Some(m) = mask.filter(|_| std::env::var("TITLE_DEBUG").as_deref() == Ok(t.text.as_str())) {
                    for y in 0..m.height {
                        println!(
                            "    {}",
                            (0..m.width.min(120)).map(|x| if m.get(x, y) { '#' } else { '.' }).collect::<String>()
                        );
                    }
                }
            }
        }
    }
    println!(
        "{} held-out titles: {exact} exact, {gaps} with unknown letters, {wrong} WRONG, {none} unread; cut as the reader \
         cuts them: {band_exact} of {banded} exact",
        eval.len()
    );
    if !check {
        let root = format!("{}/assets/glyphs/title_small.json", env!("CARGO_MANIFEST_DIR"));
        std::fs::write(&root, font.to_json()).unwrap();
        println!("written {root} ({} shapes)", font.len());
    }
}

/// The tooltip's width (UI scale 1) as the hover detector measures it.
fn tooltip_width(tooltip: &maplelens_core::image::RgbImage, scale: f32) -> usize {
    let normal = if (scale - 1.0).abs() > 0.01 { tooltip.resized(1.0 / scale) } else { tooltip.clone() };
    maplelens_core::vision::tooltip::measure_width(&normal, &maplelens_core::config::TOOLTIP_WIDTHS).unwrap_or(370)
        as usize
}

fn read_with(font: &PixelFont, tooltip: &maplelens_core::image::RgbImage, scale: f32, width: usize) -> Option<String> {
    if font == title_font() {
        return read_title(tooltip, scale, width, false).filter(|(_, n)| *n == 0).map(|(t, _)| t);
    }
    let band = title_band(tooltip, scale, width, 0)?;
    let (text, unknown) = font.read_spaced(&title_mask(&band, scale)?, SPACE_GAP);
    (unknown == 0).then(|| settle_bars(text.trim()))
}
