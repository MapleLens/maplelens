//! Every distinct text line the shop watcher reads in the shop fixtures, as boxes in the fixture's own pixels, for
//! labelling the shop font's training corpus (`tests/fixtures/shop_text.tsv`):
//! `cargo run -p maplelens-core --example shop_lines -- OUT_DIR [FRAMES_DIR]`. Writes OUT_DIR/lines.tsv (fixture,
//! kind, scale, x, y, w, h, and the text the shop's fonts read there) and one PNG per line to look at. With FRAMES_DIR, the whole
//! game frames in it are listed instead (live captures to cut new fixtures from).

use std::collections::HashSet;

use maplelens_core::image::{RgbImage, decode_png, encode_png};
use maplelens_core::shop::layout::{self, View};
use maplelens_core::shop::{chatlog, soldlist, text};

/// Where each fixture was cut out of its client: (client size, origin of the crop).
fn placement(name: &str) -> ((usize, usize), (usize, usize)) {
    match name {
        "sale" => ((1920, 1200), (440, 160)),
        "inventory_open_edge" | "sold_closed" | "sold_opened_after_sale" => ((1920, 1200), (440, 150)),
        "buttons_setup" => ((1920, 1200), (361, 300)),
        "buttons_visiting" => ((1920, 1200), (357, 296)),
        "ideal_ratio_open" => ((1920, 1200), (600, 470)),
        "live_1440" | "live_1440_tooltip" => ((2560, 1440), (700, 280)),
        "live_1080_ideal" => ((1920, 1080), (560, 300)),
        "live_1080" | "live_1080_chat" => ((1920, 1080), (560, 200)),
        "live_768" => ((1366, 768), (370, 140)),
        "live_1200_ideal_sales" => ((1920, 1200), (700, 360)),
        _ => ((1920, 1200), (340, 235)),
    }
}

fn main() {
    let out = std::env::args().nth(1).expect("an output folder");
    std::fs::create_dir_all(&out).unwrap();
    let frames = std::env::args().nth(2);
    let dir = frames.clone().unwrap_or_else(|| format!("{}/../../tests/fixtures", env!("CARGO_MANIFEST_DIR")));
    let prefix = if frames.is_some() { "" } else { "shop_" };
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_prefix(prefix)?.strip_suffix(".png").map(str::to_string))
        .collect();
    names.sort();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut tsv = String::from("fixture\tkind\tscale\tx\ty\tw\th\ttext\n");
    let mut count = 0;
    for name in names {
        let crop = decode_png(&std::fs::read(format!("{dir}/{prefix}{name}.png")).unwrap()).unwrap();
        let ((cw, ch), (ox, oy)) =
            if frames.is_some() { ((crop.width, crop.height), (0, 0)) } else { placement(&name) };
        let mut frame = RgbImage::new(cw, ch);
        for y in 0..crop.height.min(ch - oy) {
            for x in 0..crop.width.min(cw - ox) {
                frame.set_pixel(ox + x, oy + y, crop.pixel(x, y));
            }
        }
        let Some(located) = layout::locate(&frame, None, &soldlist::has_list) else { continue };
        let view = View { frame: &frame, scale: located.scale };
        let mut areas: Vec<(&str, layout::Area)> = Vec::new();
        let enter = located.anchors.enter.expect("a located shop has ENTER");
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
        for (kind, area) in areas {
            let (x0, y0, x1, y1) = view.frame_area(area);
            let line = frame.crop(x0, y0, x1, y1);
            if line.is_empty() || !seen.insert(line.data.clone()) {
                continue;
            }
            count += 1;
            let id = format!("{count:03}_{name}_{kind}");
            std::fs::write(format!("{out}/{id}.png"), encode_png(&line.upscaled_nearest(3))).unwrap();
            let (fx, fy) = (x0 as i64 - ox as i64, y0 as i64 - oy as i64);
            // The text column holds what the shop's fonts read: check it by eye, then correct it into a label.
            let read = text::read_area(&view, area, kind == "plate").map(|t| t.text).unwrap_or_default();
            tsv.push_str(&format!(
                "{name}\t{kind}\t{:.4}\t{fx}\t{fy}\t{}\t{}\t{read}\n",
                located.scale,
                x1 - x0,
                y1 - y0
            ));
        }
    }
    std::fs::write(format!("{out}/lines.tsv"), tsv).unwrap();
    println!("{count} distinct lines written to {out}");
}
