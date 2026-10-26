//! How well tooltip titles are read, on a folder of real captures (the `debug\` output of `--debug` sessions, which
//! are not committed: they are players' screens). Run with
//! `MAPLELENS_CORPUS=<folder> cargo test --release -p maplelens reading_bench -- --ignored --nocapture`;
//! it writes target\reading_bench.tsv: per capture, what the glyph atlas alone, the app's full path (atlas, OCR
//! fallback, learning) and Windows OCR alone read, and which catalog item each names.

use std::sync::Arc;

use maplelens_core::config;
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::decode_png;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::pipeline::info::InfoDetector;
use maplelens_core::vision::glyphs::Atlas;
use maplelens_core::vision::info_window;
use maplelens_core::vision::pixel_font::PixelFont;
use maplelens_core::vision::reader::{
    NoOcr, Ocr, OcrAnswer, TextReader, bullet_end, name_atlas, title_atlas, title_mask, title_mask_anchored,
};
use maplelens_core::vision::tooltip::{measure_width, tooltip_present};

use crate::{ocr, paths, refresh};

fn cell(text: &str) -> String {
    text.replace(['\t', '\n'], " ")
}

#[test]
#[ignore = "needs a folder of real captures and the local price snapshot"]
fn reading_bench() {
    let Ok(folder) = std::env::var("MAPLELENS_CORPUS") else {
        println!("set MAPLELENS_CORPUS to a folder of tooltip captures");
        return;
    };
    let catalog = Arc::new(refresh::load_catalog(&paths::snapshot_file()).expect("the local price snapshot"));
    let mut files: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();
    let width = config::TOOLTIP_TITLE_WIDTH as usize;
    // MAPLELENS_ATLAS: read with another title atlas (one learnt by `learn_from_corpus`).
    let atlas = || match std::env::var("MAPLELENS_ATLAS") {
        Ok(path) => Atlas::from_json(&std::fs::read_to_string(path).unwrap()).unwrap(),
        Err(_) => title_atlas(),
    };
    let mut atlas_only = TextReader::new(atlas(), Box::new(NoOcr));
    let mut app = HoverDetector::new(
        catalog.clone(),
        TextReader::new(atlas(), Box::new(ocr::WindowsOcr::new(false).expect("an OCR language"))),
    );
    let mut engine = ocr::WindowsOcr::new(false).expect("an OCR language");
    let item = |text: &str| catalog.lookup(text).map_or(String::new(), |i| i.name.clone());
    let mut rows = vec!["file\tatlas\tatlas_item\tapp_read\tapp_item\tocr\tocr_item\tbullet\tfill\twidth".to_string()];
    for (i, path) in files.iter().enumerate() {
        let image = decode_png(&std::fs::read(path).unwrap()).unwrap();
        let atlas = atlas_only.read_title(&image, width, 1);
        let mut screen = FrameScreen {
            frame: &image,
            origin: Point { x: 0, y: 0 },
            cursor: Some(Point { x: -config::TOOLTIP_OFFSET.0, y: -config::TOOLTIP_OFFSET.1 }),
        };
        let hover = app.step_at_scale(&mut screen, Rect::new(0, 0, 1920, 1200), 1.0, i as f64);
        let app_read = hover.as_ref().map_or(String::new(), |h| h.title.clone());
        let app_item = hover.and_then(|h| h.item).map_or(String::new(), |i| i.name.clone());
        let ocr = match title_mask(&image, width, 1).map(|mask| engine.recognize(&mask)) {
            Some(OcrAnswer::Text(text)) => text.trim().to_string(),
            _ => String::new(),
        };
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        rows.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            cell(&name),
            cell(&atlas),
            item(&atlas),
            cell(&app_read),
            app_item,
            cell(&ocr),
            item(&ocr),
            bullet_end(&image, 0).is_some(),
            tooltip_present(&image),
            measure_width(&image, &config::TOOLTIP_WIDTHS).map_or(String::new(), |w| w.to_string())
        ));
    }
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/reading_bench.tsv");
    std::fs::write(out, rows.join("\n")).unwrap();
    println!("{} captures -> {out}", files.len());
}

/// Learns the bundled title atlas further from labelled captures (`MAPLELENS_LABELS`: file<TAB>item) and writes
/// target\corpus_atlas_train.json (learnt from items whose name hashes even, to test on the others) and
/// target\corpus_atlas_all.json.
#[test]
#[ignore = "needs a folder of real captures and their labels"]
fn learn_from_corpus() {
    let (Ok(folder), Ok(labels)) = (std::env::var("MAPLELENS_CORPUS"), std::env::var("MAPLELENS_LABELS")) else {
        println!("set MAPLELENS_CORPUS and MAPLELENS_LABELS");
        return;
    };
    let width = config::TOOLTIP_TITLE_WIDTH as usize;
    let even = |name: &str| name.bytes().map(u32::from).sum::<u32>() % 2 == 0;
    let (mut train, mut all) = (title_atlas(), title_atlas());
    let (mut learnt_train, mut learnt_all) = (0, 0);
    for line in std::fs::read_to_string(labels).unwrap().lines() {
        let Some((file, item)) = line.split_once('\t') else { continue };
        if item.is_empty() {
            continue;
        }
        let image = decode_png(&std::fs::read(format!("{folder}/{file}")).unwrap()).unwrap();
        // Only bullet-anchored titles teach, as in the app.
        let Some((mask, true)) = title_mask_anchored(&image, width, 1, 0) else { continue };
        learnt_all += all.learn(&mask, item);
        if even(item) {
            learnt_train += train.learn(&mask, item);
        }
    }
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target");
    std::fs::write(format!("{dir}/corpus_atlas_train.json"), train.to_json()).unwrap();
    std::fs::write(format!("{dir}/corpus_atlas_all.json"), all.to_json()).unwrap();
    let letters = |atlas: &Atlas| {
        let mut chars: Vec<char> = atlas.glyphs.iter().map(|g| g.ch).collect();
        chars.sort_unstable();
        chars.dedup();
        chars.into_iter().collect::<String>()
    };
    println!("train: +{learnt_train} variants, letters {}", letters(&train));
    println!("all:   +{learnt_all} variants, {} glyphs, letters {}", all.glyphs.len(), letters(&all));
}

/// Debug aid: the title mask of one capture (`MAPLELENS_PROBE`=path) as text, its baselines and its read.
#[test]
#[ignore = "debug aid"]
fn probe_title() {
    let Ok(path) = std::env::var("MAPLELENS_PROBE") else { return };
    let image = decode_png(&std::fs::read(path).unwrap()).unwrap();
    let mask = title_mask(&image, config::TOOLTIP_TITLE_WIDTH as usize, 1).unwrap();
    let clean = maplelens_core::vision::segment::without_rules(&mask);
    for y in 0..clean.height {
        let row: String = (0..clean.width.min(160)).map(|x| if clean.get(x, y) { '#' } else { '.' }).collect();
        let count = (0..clean.width).filter(|&x| clean.get(x, y)).count();
        println!("{y:2} {count:3} {row}");
    }
    println!(
        "bullet {:?} crafted bullet {:?} fill {}",
        maplelens_core::vision::reader::find_bullet(&image, 0),
        maplelens_core::vision::reader::find_bullet(&image, config::TOOLTIP_CRAFTED_SHIFT as usize),
        tooltip_present(&image)
    );
    println!("baselines {:?}", maplelens_core::vision::segment::baselines(&clean));
    println!("read {:?}", title_atlas().read(&mask));
    let catalog = refresh::load_catalog(&paths::snapshot_file()).unwrap();
    let mut reader = TextReader::new(title_atlas(), Box::new(ocr::WindowsOcr::new(false).unwrap()));
    let width = config::TOOLTIP_TITLE_WIDTH as usize;
    for (name, text) in [
        ("plain", reader.read_title(&image, width, 1)),
        ("crafted", reader.read_title_at(&image, width, 1, config::TOOLTIP_CRAFTED_SHIFT as usize)),
        ("wrapped", reader.read_title(&image, width, 2)),
    ] {
        let item = catalog.lookup(&text).map(|i| i.name.clone());
        println!("{name}: {text:?} -> {item:?} (atlas proof {})", reader.read_by_atlas(&text));
    }
}

/// How well Character Info names are read, on a folder of saved windows (`--debug` writes them to debug\names\) and
/// their labels (`MAPLELENS_NAME_LABELS`: file<TAB>name). Each window image is read as a frame at the UI scale in
/// `MAPLELENS_NAME_SCALE` (default 0.64), with a fresh reader per window so one read cannot teach the next.
#[test]
#[ignore = "needs a folder of saved Character Info windows and their labels"]
fn name_bench() {
    let (Ok(folder), Ok(labels)) = (std::env::var("MAPLELENS_NAMES"), std::env::var("MAPLELENS_NAME_LABELS")) else {
        println!("set MAPLELENS_NAMES and MAPLELENS_NAME_LABELS");
        return;
    };
    let scale: f32 = std::env::var("MAPLELENS_NAME_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(0.64);
    let (mut right, mut wrong, mut missed) = (0, 0, 0);
    for line in std::fs::read_to_string(labels).unwrap().lines() {
        let Some((file, truth)) = line.split_once('\t') else { continue };
        let image = decode_png(&std::fs::read(format!("{folder}/{file}")).unwrap()).unwrap();
        let mut screen = FrameScreen { frame: &image, origin: Point { x: 0, y: 0 }, cursor: None };
        let ocr = ocr::WindowsOcr::new(true).expect("an OCR language");
        let atlas = match std::env::var("MAPLELENS_NAME_ATLAS") {
            Ok(path) => Atlas::from_json(&std::fs::read_to_string(path).unwrap()).unwrap(),
            Err(_) => name_atlas(),
        };
        let mut detector = InfoDetector::new(TextReader::new(atlas, Box::new(ocr)));
        let bounds = Rect::new(0, 0, image.width as i32, image.height as i32);
        detector.step_at_scale(&mut screen, bounds, scale); // a name counts once two reads agree
        let window = detector.step_at_scale(&mut screen, bounds, scale);
        let name = window.as_ref().and_then(|w| w.name.clone());
        let verdict = match name.as_deref() {
            Some(n) if n == truth => {
                right += 1;
                "ok"
            }
            Some(_) => {
                wrong += 1;
                "WRONG"
            }
            None => {
                missed += 1;
                "missed"
            }
        };
        let read = window.map_or_else(|| "(no window)".to_string(), |w| w.read);
        println!("{verdict:6} {truth:14} read {read:?}");
    }
    println!("names: {right} right, {wrong} wrong, {missed} missed");
}

/// Learns the small-UI name font (`assets/glyphs/name_small.json`) from labelled Character Info windows (as for
/// `name_bench`) and writes target\name_small.json; prints how each name reads with letters learnt from the others.
#[test]
#[ignore = "needs a folder of saved Character Info windows and their labels"]
fn learn_pixel_names() {
    let (Ok(folder), Ok(labels)) = (std::env::var("MAPLELENS_NAMES"), std::env::var("MAPLELENS_NAME_LABELS")) else {
        println!("set MAPLELENS_NAMES and MAPLELENS_NAME_LABELS");
        return;
    };
    let scale = 0.64;
    let (mut plates, mut images) = (Vec::new(), Vec::new());
    for line in std::fs::read_to_string(labels).unwrap().lines() {
        let Some((file, truth)) = line.split_once('\t') else { continue };
        let image = decode_png(&std::fs::read(format!("{folder}/{file}")).unwrap()).unwrap();
        let Some((x, y)) = info_window::find_window(&image.to_gray(), scale) else { continue };
        let b = info_window::name_box(Rect::new(x, y, 0, 0), scale);
        let plate = image.crop(b.x as usize, b.y as usize, b.right() as usize, b.bottom() as usize);
        plates.push((truth.to_string(), info_window::name_mask(&plate)));
        images.push((file.to_string(), image));
    }
    // Twice: a piece of touching letters is learnt once the letters around it are known.
    let learn = |font: &mut PixelFont, skip: Option<usize>| {
        for _ in 0..2 {
            for (j, (t, m)) in plates.iter().enumerate() {
                if Some(j) != skip {
                    font.learn(m, t);
                }
            }
        }
    };
    let mut all = PixelFont::default();
    learn(&mut all, None);
    let (mut whole, mut wrong) = (0, 0);
    let (mut app_right, mut app_wrong, mut app_missed) = (0, 0, 0);
    for (i, (truth, mask)) in plates.iter().enumerate() {
        let mut others = PixelFont::default();
        learn(&mut others, Some(i));
        let (held_out, unknown) = others.read(mask);
        whole += usize::from(unknown == 0 && held_out == *truth);
        // Read in full yet different: the dangerous kind of error (an unknown piece only sends the name to OCR).
        wrong += usize::from(unknown == 0 && held_out != *truth);
        // The app's whole path with that font: exact letters, OCR for the pieces it does not know.
        let (file, image) = &images[i];
        let mut screen = FrameScreen { frame: image, origin: Point { x: 0, y: 0 }, cursor: None };
        let ocr = ocr::WindowsOcr::new(true).expect("an OCR language");
        let mut detector = InfoDetector::new(TextReader::new(name_atlas(), Box::new(ocr))).with_pixel_font(others);
        let bounds = Rect::new(0, 0, image.width as i32, image.height as i32);
        detector.step_at_scale(&mut screen, bounds, scale);
        let app = detector.step_at_scale(&mut screen, bounds, scale).and_then(|w| w.name);
        match app.as_deref() {
            Some(n) if n == truth => app_right += 1,
            Some(n) => {
                app_wrong += 1;
                println!("  app read {n:?} for {truth} ({file})");
            }
            None => app_missed += 1,
        }
        println!("{truth:14} full {:?} held-out {held_out:?}", all.read(mask).0);
    }
    println!("app path, held-out font: {app_right} right, {app_wrong} wrong, {app_missed} no name");
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/name_small.json");
    std::fs::write(out, all.to_json() + "\n").unwrap();
    println!(
        "{} plates; held-out: {whole} read whole, {wrong} read whole but wrong; {} pieces",
        plates.len(),
        all.len()
    );
}
