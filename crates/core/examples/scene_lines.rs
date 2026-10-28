//! The shop font's corpus from rendered scenes (`scripts/scenes.py`): every line box the shop watcher reads in a
//! scene's frames, labelled with the text the renderer reports drawing there (its exact boxes, no eye needed):
//! `cargo run --release -p maplelens-core --example scene_lines -- SCENES_DIR OUT.tsv [--split train|eval|all]`.
//! Writes the corpus format of `tests/fixtures/shop_text.tsv`, with the frame's path in the first column
//! (`build_shop_font --corpus OUT.tsv` reads it). A box that holds no label, or more than one, is left out.

use std::collections::HashSet;

use maplelens_core::image::decode_png;
use maplelens_core::shop::layout::{self, View};
use maplelens_core::shop::{chatlog, soldlist};
use serde_json::Value;

/// A label belongs to a box when its ink's centre is inside it and this share of its width is.
const INSIDE: f64 = 0.8;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("a scenes folder");
    let out = args.next().expect("an output .tsv");
    let split = match (args.next().as_deref(), args.next()) {
        (Some("--split"), Some(s)) => s,
        _ => "train".to_string(),
    };
    let mut tsv = String::from("# lines of rendered scenes: frame, kind, UI scale, x, y, w, h, text\n");
    let (mut count, mut scenes) = (0, 0);
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    for line in std::fs::read_to_string(format!("{dir}/labels.jsonl")).unwrap().lines() {
        let Ok(label) = serde_json::from_str::<Value>(line) else { continue };
        let id = label["id"].as_u64().unwrap_or(0);
        let keep = match split.as_str() {
            "train" => id % 5 != 0,
            "eval" => id % 5 == 0,
            _ => true,
        };
        if !keep || !matches!(label["kind"].as_str(), Some("store" | "visitor")) {
            continue;
        }
        scenes += 1;
        for (k, file) in label["files"].as_array().unwrap().iter().enumerate() {
            let path = format!("{dir}/{}", file.as_str().unwrap());
            let Ok(frame) = decode_png(&std::fs::read(&path).unwrap_or_default()) else { continue };
            let Some(located) = layout::locate(&frame, None, &soldlist::has_list) else { continue };
            let view = View { frame: &frame, scale: located.scale };
            let Some(enter) = located.anchors.enter else { continue };
            let mut areas: Vec<(&str, layout::Area)> = Vec::new();
            for line in chatlog::read_lines(&view, enter).unwrap_or_default() {
                areas.extend(line.source.map(|(area, _)| ("chat", area)));
            }
            if let Some(sold_x) = located.anchors.sold_x {
                for row in soldlist::read_rows(&view, sold_x).unwrap_or_default() {
                    for line in row.lines {
                        areas.extend(line.source.map(|(area, _)| ("sold", area)));
                    }
                }
            }
            areas.push(("counter", layout::balance(enter)));
            areas.push(("plate", layout::owner_name(enter)));
            let texts = label["texts"][k].as_array().cloned().unwrap_or_default();
            for (kind, area) in areas {
                let (x0, y0, x1, y1) = view.frame_area(area);
                let inside: Vec<&str> = texts
                    .iter()
                    .filter_map(|t| {
                        let b: Vec<f64> = t[1].as_array()?.iter().filter_map(Value::as_f64).collect();
                        let (cx, cy) = (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0);
                        let covered = (b[0] + b[2]).min(x1 as f64) - b[0].max(x0 as f64);
                        let fits = cx >= x0 as f64
                            && cx < x1 as f64
                            && cy >= y0 as f64
                            && cy < y1 as f64
                            && covered >= INSIDE * b[2];
                        fits.then(|| t[0].as_str()).flatten()
                    })
                    .collect();
                let [text] = inside[..] else { continue };
                let crop = frame.crop(x0, y0, x1, y1);
                if crop.is_empty() || !seen.insert(crop.data.clone()) {
                    continue;
                }
                count += 1;
                tsv.push_str(&format!(
                    "{path}\t{kind}\t{:.4}\t{x0}\t{y0}\t{}\t{}\t{}\n",
                    located.scale,
                    x1 - x0,
                    y1 - y0,
                    text.trim()
                ));
            }
        }
    }
    std::fs::write(&out, tsv).unwrap();
    println!("{count} distinct lines from {scenes} scenes written to {out}");
}
