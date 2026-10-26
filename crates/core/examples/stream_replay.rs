//! Replay extracted stream frames through the no-mouse (browser) detectors and log every detection:
//! `cargo run --release -p maplelens-core --example stream_replay -- FRAMES_DIR [OUT_DIR]`.
//! Prints one TSV line per frame (name, ms, tooltip title, item, position, Character Info name) and saves a crop of
//! every tooltip found to OUT_DIR for a visual check.

use std::sync::Arc;
use std::time::Instant;

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::{decode_png, encode_png};
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::sample::sample_items;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::pipeline::info::InfoDetector;
use maplelens_core::vision::reader::{NoOcr, TextReader, name_atlas, title_atlas};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("frames dir");
    let out = args.next();
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    let mut hover = HoverDetector::new(
        Arc::new(Catalog::new(sample_items(), None)),
        TextReader::new(title_atlas(), Box::new(NoOcr)),
    );
    let mut info = InfoDetector::new(TextReader::new(name_atlas(), Box::new(NoOcr)));
    if let Some(out) = &out {
        std::fs::create_dir_all(out).unwrap();
    }
    for (i, path) in names.iter().enumerate() {
        let Ok(frame) = decode_png(&std::fs::read(path).unwrap()) else { continue };
        let bounds = Rect::new(0, 0, frame.width as i32, frame.height as i32);
        let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
        let start = Instant::now();
        let found = hover.step(&mut screen, bounds, i as f64);
        let hover_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let window = info.step(&mut screen, bounds);
        let info_ms = start.elapsed().as_secs_f64() * 1000.0;
        let stem = path.file_stem().unwrap().to_string_lossy();
        if let (Some(h), Some(out)) = (&found, &out)
            && let Some(at) = h.found_at
        {
            let (w, ht) = h.tooltip_size;
            let (x, y) = (at.x.max(0) as usize, at.y.max(0) as usize);
            let crop = frame.crop(x, y, (x + w as usize).min(frame.width), (y + ht as usize).min(frame.height));
            std::fs::write(format!("{out}/{stem}.png"), encode_png(&crop)).unwrap();
        }
        println!(
            "{stem}\t{hover_ms:.0}\t{info_ms:.0}\t{}\t{}\t{:?}\t{}",
            found.as_ref().map_or("", |h| h.title.as_str()),
            found.as_ref().and_then(|h| h.item.as_ref()).map_or("", |it| it.name.as_str()),
            found.as_ref().and_then(|h| h.found_at).map(|p| (p.x, p.y)),
            window.as_ref().map_or(String::new(), |w| format!("{:?}@{:?}", w.name, (w.rect.x, w.rect.y))),
        );
    }
}
