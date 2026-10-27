//! Detection on frames of a live stream (extracted with ffmpeg), through the browser's no-mouse path with Windows
//! OCR: `MAPLELENS_FRAMES=<folder of PNGs> [MAPLELENS_GAME_RECT=x,y,w,h] [MAPLELENS_OUT=<folder>]
//! cargo test --release -p maplelens stream_bench -- --ignored --nocapture`. Prints a TSV line per frame (title as
//! read, item, where, Character Info name, UI scale; MAPLELENS_UI_SCALE overrides the measured one) and saves every tooltip crop to MAPLELENS_OUT for a visual check.
//! Uses the local price snapshot when there is one, else the demo items.

use std::sync::Arc;
use std::time::Instant;

use maplelens_core::config;
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::{decode_png, encode_png};
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::sample::sample_items;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::pipeline::info::InfoDetector;
use maplelens_core::vision::reader::{TextReader, name_atlas, title_atlas};

use crate::{ocr, paths, refresh};

#[test]
#[ignore = "needs a folder of stream frames"]
fn stream_bench() {
    let Ok(folder) = std::env::var("MAPLELENS_FRAMES") else {
        println!("set MAPLELENS_FRAMES to a folder of frames");
        return;
    };
    let out = std::env::var("MAPLELENS_OUT").ok();
    // Compressed video blurs the status bar's 1 px outline, so the app's measurement of the UI size may fail.
    let ui_scale: Option<f32> = std::env::var("MAPLELENS_UI_SCALE").ok().and_then(|v| v.parse().ok());
    // A fixed mouse position, to test the app's own path (tooltip at mouse + offset) rather than the browser search.
    let cursor: Option<Point> = std::env::var("MAPLELENS_CURSOR").ok().and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some(Point { x: x.trim().parse().ok()?, y: y.trim().parse().ok()? })
    });
    let rect: Option<Vec<i32>> = std::env::var("MAPLELENS_GAME_RECT")
        .ok()
        .map(|v| v.split(',').map(|n| n.trim().parse().expect("x,y,w,h")).collect());
    let catalog = refresh::load_catalog(&paths::snapshot_file()).unwrap_or_else(|_| Catalog::new(sample_items(), None));
    let mut hover = HoverDetector::new(
        Arc::new(catalog),
        TextReader::new(title_atlas(), Box::new(ocr::WindowsOcr::new(false).expect("an OCR language"))),
    );
    // The app's own path, for tooltips the search found: the mouse put where the game would have it (the tooltip
    // at mouse + offset), which must read the same item.
    let mut app = HoverDetector::new(
        hover.catalog().clone(),
        TextReader::new(title_atlas(), Box::new(ocr::WindowsOcr::new(false).expect("an OCR language"))),
    );
    let mut info = InfoDetector::new(TextReader::new(
        name_atlas(),
        Box::new(ocr::WindowsOcr::new(false).expect("an OCR language")),
    ));
    let mut files: Vec<_> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    if let Some(out) = &out {
        std::fs::create_dir_all(out).unwrap();
    }
    for (i, path) in files.iter().enumerate() {
        let Ok(mut frame) = decode_png(&std::fs::read(path).unwrap()) else { continue };
        if let Some(r) = &rect {
            frame = frame.crop(r[0] as usize, r[1] as usize, (r[0] + r[2]) as usize, (r[1] + r[3]) as usize);
        }
        let bounds = Rect::new(0, 0, frame.width as i32, frame.height as i32);
        if let Some(scale) = ui_scale {
            // Given by hand: as many readings as settle it at once.
            for _ in 0..config::SCALE_VOTE_MIN_READINGS {
                hover.ui_scale().set(bounds, scale, i as f64);
            }
        }
        let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor };
        let start = Instant::now();
        let found = hover.step(&mut screen, bounds, i as f64);
        let hover_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let window = info.step(&mut screen, bounds);
        let info_ms = start.elapsed().as_secs_f64() * 1000.0;
        let stem = path.file_stem().unwrap().to_string_lossy();
        if let (Some(h), Some(out), Some(at)) = (&found, &out, found.as_ref().and_then(|h| h.found_at)) {
            let (w, ht) = h.tooltip_size;
            let (x, y) = (at.x.max(0) as usize, at.y.max(0) as usize);
            let crop = frame.crop(x, y, (x + w as usize).min(frame.width), (y + ht as usize).min(frame.height));
            std::fs::write(format!("{out}/{stem}.png"), encode_png(&crop)).unwrap();
        }
        let mouse = found.as_ref().and_then(|h| h.found_at.map(|at| (h, at))).map(|(h, at)| {
            let offset = config::TOOLTIP_OFFSET;
            let scale = h.scale;
            let cursor = Point {
                x: at.x - maplelens_core::geometry::scaled(offset.0, scale),
                y: at.y - maplelens_core::geometry::scaled(offset.1, scale),
            };
            let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: Some(cursor) };
            // Three frames: a title counts once read the same twice, an unknown one only after several reads.
            let mut read = None;
            for k in 0..3 {
                read = app.step_at_scale(&mut screen, bounds, scale, (i * 3 + k) as f64);
            }
            read.and_then(|r| r.item.map(|it| it.name.clone())).unwrap_or_default()
        });
        println!(
            "{stem}\t{hover_ms:.0}\t{info_ms:.0}\t{}\t{}\t{:?}\t{}\t{:.3}\t{}",
            found.as_ref().map_or("", |h| h.title.as_str()),
            found.as_ref().and_then(|h| h.item.as_ref()).map_or("", |it| it.name.as_str()),
            found.as_ref().and_then(|h| h.found_at).map(|p| (p.x, p.y)),
            window.as_ref().map_or(String::new(), |w| format!("{:?}@{:?}", w.name, (w.rect.x, w.rect.y))),
            hover.ui_scale().of(bounds),
            mouse.unwrap_or_default(),
        );
    }
}

/// How often a misread name names the wrong item, on the local snapshot's own names: every item with one letter
/// dropped, doubled, or swapped for a look-alike. A name that finds nothing costs a card; a wrong one a wrong price.
#[test]
#[ignore = "needs the local price snapshot"]
fn wrong_items_under_ocr_noise() {
    let Ok(catalog) = refresh::load_catalog(&paths::snapshot_file()) else { return };
    let look_alike = |c: char| match c {
        'l' => Some('I'),
        'I' => Some('l'),
        'o' => Some('0'),
        '0' => Some('o'),
        't' => Some('f'),
        'f' => Some('t'),
        'i' => Some('l'),
        's' => Some('5'),
        'e' => Some('c'),
        _ => None,
    };
    let (mut total, mut missed, mut wrong_count, mut wrong) = (0usize, 0usize, 0usize, Vec::new());
    for item in catalog.items() {
        let chars: Vec<char> = item.name.chars().collect();
        let mut variants: Vec<String> = Vec::new();
        for i in 0..chars.len() {
            let mut dropped = chars.clone();
            dropped.remove(i);
            variants.push(dropped.into_iter().collect());
            let mut doubled = chars.clone();
            doubled.insert(i, chars[i]);
            variants.push(doubled.into_iter().collect());
            if let Some(l) = look_alike(chars[i]) {
                let mut swapped = chars.clone();
                swapped[i] = l;
                variants.push(swapped.into_iter().collect());
            }
        }
        for variant in variants {
            total += 1;
            match catalog.lookup(&variant) {
                None => missed += 1,
                Some(found) if found.name == item.name => {}
                Some(found) => {
                    wrong_count += 1;
                    if wrong.len() < 40 {
                        wrong.push(format!("{} -> {} (read {variant:?})", item.name, found.name));
                    }
                }
            }
        }
    }
    println!("{total} misreads: {missed} found nothing, {wrong_count} named another item");
    for line in &wrong {
        println!("  {line}");
    }
}

/// Like `wrong_items_under_ocr_noise`, with several misreads in one name (OCR gets "Iron Ore" as "Iran Qre"): for 2, 3 and
/// 4 edits per name drawn from a fixed random sequence, how many names are found, lost, or named as another item, and
/// what a lookup costs. The same edits for every run, so changes to the matching can be compared.
#[test]
#[ignore = "needs the local price snapshot"]
fn wrong_items_under_heavier_noise() {
    let Ok(catalog) = refresh::load_catalog(&paths::snapshot_file()) else { return };
    let confusable: [&str; 9] = ["oOQ0aC", "lI1iJt", "eco", "fts", "nmhr", "uvy", "dbp", "gq9", "S5"];
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    for edits in [2usize, 3, 4] {
        let (mut total, mut found, mut missed, mut wrong, mut shown) = (0usize, 0usize, 0usize, 0usize, 0);
        let mut spent = std::time::Duration::ZERO;
        for item in catalog.items() {
            for _ in 0..4 {
                let mut chars: Vec<char> = item.name.chars().collect();
                for _ in 0..edits {
                    if chars.len() < 4 {
                        break;
                    }
                    let at = next(chars.len());
                    match next(4) {
                        0 | 1 => {
                            let group = confusable.iter().find(|g| g.contains(chars[at]));
                            let pool: Vec<char> = match group {
                                Some(g) => g.chars().filter(|&c| c != chars[at]).collect(),
                                None => vec!['e', 'a', 'n', 'r'],
                            };
                            chars[at] = pool[next(pool.len())];
                        }
                        2 => {
                            chars.remove(at);
                        }
                        _ => chars.insert(at, ['i', 'l', 'e', 't', 'r'][next(5)]),
                    }
                }
                let read: String = chars.into_iter().collect();
                total += 1;
                let start = Instant::now();
                let result = catalog.lookup(&read);
                spent += start.elapsed();
                match result {
                    None => missed += 1,
                    Some(got) if got.name == item.name => found += 1,
                    Some(got) => {
                        wrong += 1;
                        if shown < 6 {
                            shown += 1;
                            println!("  wrong: {} -> {} (read {read:?})", item.name, got.name);
                        }
                    }
                }
            }
        }
        println!(
            "{edits} edits: {total} reads, {found} found ({:.1}%), {missed} nothing, {wrong} wrong ({:.2}%); {:.2} ms a lookup",
            100.0 * found as f64 / total as f64,
            100.0 * wrong as f64 / total as f64,
            spent.as_secs_f64() * 1000.0 / total as f64
        );
    }
}

/// What one pass of the hover loop costs with the game in front and no tooltip: a real GDI grab of the tooltip
/// area plus the detector's step on it (the loop runs this every few milliseconds, all day).
#[test]
#[ignore = "needs a desktop session"]
fn idle_hover_pass_cost() {
    use crate::capture::GdiScreen;
    use maplelens_core::pipeline::Screen;
    let catalog = refresh::load_catalog(&paths::snapshot_file()).unwrap_or_else(|_| Catalog::new(sample_items(), None));
    let mut hover = HoverDetector::new(
        Arc::new(catalog),
        TextReader::new(title_atlas(), Box::new(ocr::WindowsOcr::new(false).expect("an OCR language"))),
    );
    let mut screen = GdiScreen::new();
    let bounds = Rect::new(0, 0, 1920, 1080);
    let area = Rect::new(500, 400, 743, 120);
    let start = Instant::now();
    for _ in 0..200 {
        let _ = screen.grab(area);
    }
    println!("GDI grab of 743x120: {:.2} ms", start.elapsed().as_secs_f64() * 5.0);
    let mut live = |cursor: Point| {
        let start = Instant::now();
        for i in 0..200 {
            let mut s = GdiScreen::new();
            let mut counted = CursorAt(&mut s, cursor);
            let _ = hover.step_at_scale(&mut counted, bounds, 0.9, i as f64 * 0.01);
        }
        start.elapsed().as_secs_f64() * 5.0
    };
    println!("hover pass over the live desktop: {:.2} ms", live(Point { x: 600, y: 300 }));
}

struct CursorAt<'a>(&'a mut crate::capture::GdiScreen, Point);

impl maplelens_core::pipeline::Screen for CursorAt<'_> {
    fn grab(&mut self, area: Rect) -> maplelens_core::image::RgbImage {
        self.0.grab(area)
    }
    fn cursor(&mut self) -> Option<Point> {
        Some(self.1)
    }
}
