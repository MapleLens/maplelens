//! The training card's reader with the real Windows OCR: the minimap's map names on real captures.

use maplelens_core::image::decode_png;
use maplelens_core::vision::minimap::find_title;
use maplelens_core::vision::reader::OcrAnswer;

fn fixture(name: &str) -> maplelens_core::image::RgbImage {
    let path = format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    decode_png(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn windows_ocr_reads_the_minimap_title() {
    if !crate::ocr::available() {
        return;
    }
    let mut ocr = crate::training::reader();
    for (name, scale) in [
        ("minimap_free_market_1920x1200.png", 1.0),
        ("minimap_free_market_8_1920x1200.png", 1.0),
        ("minimap_ideal_ratio_1920x1200.png", 0.64),
    ] {
        let title = find_title(&fixture(name), scale).expect("a minimap");
        let mut read = |image| match ocr.recognize_image(image) {
            OcrAnswer::Text(text) => text.trim().to_string(),
            other => panic!("{other:?}"),
        };
        assert_eq!(read(&title.street), "Hidden Street", "{name}");
        // Windows OCR leaves out the room number ("<3>"): the map is still the Free Market.
        assert!(read(&title.map).starts_with("Free Market"), "{name}");
    }
}

/// `MAPLELENS_MINIMAP=<png> [MAPLELENS_UI_SCALE=0.64] cargo test -p maplelens minimap_ocr_on -- --ignored --nocapture`:
/// what the training reader makes of a minimap capture (its top-left corner), and which quest stage that is.
#[test]
#[ignore]
fn minimap_ocr_on() {
    let path = std::env::var("MAPLELENS_MINIMAP").expect("MAPLELENS_MINIMAP");
    let image = decode_png(&std::fs::read(path).unwrap()).unwrap();
    let scale = std::env::var("MAPLELENS_UI_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let title = find_title(&image, scale).expect("a minimap");
    let mut ocr = crate::training::reader();
    for line in [&title.street, &title.map] {
        let text = match ocr.recognize_image(line) {
            OcrAnswer::Text(text) => text,
            other => format!("{other:?}"),
        };
        println!("{text:?} -> stage {:?}", maplelens_core::training::kpq::stage_of(&text));
    }
}

/// Live check against the running game (`cargo test -p maplelens live_training -- --ignored --nocapture`): reads the
/// game window for 15 s as the training thread does (window capture, so it may be covered) and prints the EXP and
/// map it settles on; the last frame and the card that would show go to target\live_training_*.png.
#[test]
#[ignore = "needs MapleStory running"]
fn live_training() {
    use std::time::{Duration, Instant};

    use maplelens_core::geometry::{Point, Rect};
    use maplelens_core::pipeline::FrameScreen;
    use maplelens_core::render::training_card::{Show, render_training_card};
    use maplelens_core::training::session::Tracker;
    use maplelens_core::training::watcher::TrainingWatcher;

    let hwnd = crate::game::find_game_window().expect("MapleStory is running");
    let mut capture = crate::wgc::WindowCapture::new(hwnd).unwrap();
    let mut watcher = TrainingWatcher::new(crate::training::reader(), Tracker::new());
    let start = Instant::now();
    let mut last = None;
    while start.elapsed() < Duration::from_secs(15) {
        if let Some(frame) = capture.latest().unwrap() {
            let now = crate::paths::now_unix();
            let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
            watcher.step(&mut screen, Rect::new(0, 0, frame.width as i32, frame.height as i32), now);
            last = Some(frame);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let frame = last.expect("no frame within 15 s (minimized?)");
    let now = crate::paths::now_unix();
    let training = watcher.training(now);
    println!(
        "{}x{}: EXP {:?}, map {:?}, quest {:?}",
        frame.width, frame.height, training.exp.reading, training.exp.map, training.quest
    );
    let target = format!("{}/../../target", env!("CARGO_MANIFEST_DIR"));
    std::fs::write(format!("{target}/live_training_frame.png"), maplelens_core::image::encode_png(&frame)).unwrap();
    let fonts = crate::app::system_fonts().unwrap();
    if let Some(card) = render_training_card(&fonts, &training, Show { exp: true, quest: true, strip: None }, now, 1.0)
    {
        println!("card {}x{}", card.width, card.height);
    }
}

/// `cargo test -p maplelens live_potion_icons -- --ignored --nocapture`: the potion list and icons download (or come
/// from the cache) and every icon reads.
#[test]
#[ignore = "needs the network"]
fn live_potion_icons() {
    let icons = crate::potions::spawn();
    let mut set = maplelens_core::vision::item_icon::IconSet::default();
    for (id, name, png) in icons.iter() {
        assert!(set.add_png(id, &name, &png), "{id} {name}");
    }
    println!("{} icons", set.len());
    assert!(set.len() >= 50, "{}", set.len());
}

/// `cargo test -p maplelens live_quickslots -- --ignored --nocapture`: what the potion reader makes of the running
/// game's quickslots (window capture, so the game may be covered): each item's total.
#[test]
#[ignore = "needs MapleStory running"]
fn live_quickslots() {
    use maplelens_core::training::potions::PotionWatcher;
    use maplelens_core::vision::hud::find_bar;
    use maplelens_core::vision::item_icon::IconSet;

    let mut icons = IconSet::default();
    for (id, name, png) in crate::potions::spawn().iter() {
        icons.add_png(id, &name, &png);
    }
    let hwnd = crate::game::find_game_window().expect("MapleStory is running");
    let mut capture = crate::wgc::WindowCapture::new(hwnd).unwrap();
    let mut watcher = PotionWatcher::default();
    for look in 0..6 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let Some(frame) = capture.latest().unwrap() else { continue };
        let top = frame.height.saturating_sub(300);
        let strip = frame.rows_from(top);
        let Some(bar) = find_bar(&strip) else { continue };
        let started = std::time::Instant::now();
        let totals = watcher.look(&strip, bar, (0, 0), &mut icons, look as f64 * 0.5).clone();
        let named: Vec<String> =
            totals.iter().map(|(id, n)| format!("{} x{n}", icons.icon(*id).map_or("?", |i| i.name.as_str()))).collect();
        println!("look {look} ({:?}): {named:?}", started.elapsed());
    }
}

/// `cargo test -p maplelens live_towns -- --ignored --nocapture`: the town list (`training::towns`) against MapleStory
/// Classic World's map data (maplestory.io, every map's details: about 35 MB). No map with monsters may be taken for a
/// town (a player training there would lose the card), and every name listed must be a town without monsters; the
/// data's towns the list lacks are printed, for a new region's towns to be added.
#[test]
#[ignore = "needs the network"]
fn live_towns() {
    use maplelens_core::training::map_name::same_place;
    use maplelens_core::training::towns::{TOWNS, is_town};
    use serde_json::Value;

    // Classic World's own data, which the item pictures come from too (the map helper's is GMS v40's).
    let get = |path: &str| -> Option<Value> {
        let url = format!("{}/{path}", maplelens_core::config::ITEM_API);
        let mut response = crate::net::agent(60).get(&url).call().ok()?;
        if response.status().as_u16() != 200 {
            return None;
        }
        let text = response.body_mut().with_config().limit(16 << 20).read_to_string().ok()?;
        serde_json::from_str(&text).ok()
    };
    let list = get("map?count=2000").expect("the map list");
    let ids: Vec<u64> = list.as_array().expect("a list").iter().filter_map(|m| m["id"].as_u64()).collect();
    let maps: Vec<Value> =
        crate::maps::client::in_parallel(&ids, |id| get(&format!("map/{id}"))).into_iter().flatten().collect();
    println!("{} maps of {} listed", maps.len(), ids.len());
    assert!(maps.len() * 10 >= ids.len() * 9, "too few maps answered");
    let name = |m: &Value| m["name"].as_str().unwrap_or_default().to_string();
    let quiet = |m: &Value| m["mobs"].as_array().is_none_or(Vec::is_empty);
    let town = |m: &Value| m["isTown"].as_bool() == Some(true) && quiet(m);
    let wrong: Vec<String> = maps.iter().filter(|m| !quiet(m) && is_town(&name(m))).map(name).collect();
    assert!(wrong.is_empty(), "maps with monsters taken for towns: {wrong:?}");
    for listed in TOWNS {
        assert!(maps.iter().any(|m| town(m) && same_place(listed, &name(m))), "{listed}: no such town in the data");
    }
    for m in maps.iter().filter(|m| town(m) && !is_town(&name(m))) {
        println!("  a town not listed: {} ({})", name(m), m["id"]);
    }
}
