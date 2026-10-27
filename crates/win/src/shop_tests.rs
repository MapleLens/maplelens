//! The Store Permit watcher with Windows' real OCR on real captures, and the window capture it reads from.

use std::time::{Duration, Instant};

use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::settings::Settings;
use maplelens_core::shop::parse::parse_sale;
use maplelens_core::shop::watcher::ShopWatcher;
use maplelens_core::shop::{ShopEvent, soldlist};
use maplelens_core::vision::Mask;

use crate::settings_window::SettingsWindow;
use crate::{app, icon, shop, wgc};

/// A shop fixture (a crop of a 1920x1200 client) put back where it was cut from.
fn frame(name: &str, origin: (usize, usize)) -> RgbImage {
    let path = format!("{}/../../tests/fixtures/shop_{name}.png", env!("CARGO_MANIFEST_DIR"));
    let crop = decode_png(&std::fs::read(&path).unwrap()).unwrap();
    let mut canvas = RgbImage::new(1920, 1200);
    for y in 0..crop.height {
        for x in 0..crop.width {
            canvas.set_pixel(origin.0 + x, origin.1 + y, crop.pixel(x, y));
        }
    }
    canvas
}

fn feed(frames: &[&str]) -> Vec<ShopEvent> {
    let mut watcher = ShopWatcher::new(shop::reader());
    let mut events = Vec::new();
    for (i, name) in frames.iter().enumerate() {
        let image = frame(name, (340, 235));
        for repeat in 0..2 {
            events.extend(watcher.update(&image, (i * 2 + repeat) as f64));
        }
    }
    events.retain(|e| *e != ShopEvent::StoreOpened);
    events
}

/// A player's name a local capture shows (`tests/fixtures/names.txt`, `key=name` lines): real names stay out of git
/// with the captures.
fn local_name(key: &str) -> String {
    let path = format!("{}/../../tests/fixtures/names.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("no {key}= in {path}"))
        .trim()
        .to_string()
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn sales_are_read() {
    if !crate::ocr::available() {
        return;
    }
    let events = feed(&["025123", "030620"]);
    // What the counter gained may or may not be read from these crops; the sale itself must be.
    let sales: Vec<(&str, &str, u64)> = events
        .iter()
        .filter_map(|e| match e {
            ShopEvent::Sale { item, buyer, quantity, .. } => Some((item.as_str(), buyer.as_str(), *quantity)),
            _ => None,
        })
        .collect();
    assert_eq!(sales, [("Arrows for Bows", "TranzMog", 1000); 2]);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn chat_enter_and_leave_are_read() {
    if !crate::ocr::available() {
        return;
    }
    let events: Vec<ShopEvent> =
        feed(&["025420", "025520"]).into_iter().filter(|e| !matches!(e, ShopEvent::LostSync(_))).collect();
    assert_eq!(events, [ShopEvent::PlayerEntered("DreadLancer".into()), ShopEvent::PlayerLeft("DreadLancer".into())]);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_sale_in_a_shop_dragged_elsewhere_is_read() {
    if !crate::ocr::available() {
        return;
    }
    let image = frame("sale", (440, 160));
    let view = maplelens_core::shop::layout::View { frame: &image, scale: 1.0 };
    let rows = soldlist::read_rows(&view, (1632, 173)).unwrap();
    let mut read = shop::reader();
    let texts: Vec<String> = rows[0]
        .lines
        .iter()
        .map(|line| {
            let glyphs = &line.glyphs;
            let mut padded = Mask::new(glyphs.width + 8, glyphs.height + 8);
            for y in 0..glyphs.height {
                for x in 0..glyphs.width {
                    padded.set(x + 4, y + 4, glyphs.get(x, y));
                }
            }
            read(&padded).unwrap_or_default()
        })
        .collect();
    assert_eq!(
        parse_sale(&texts[0], &texts[1], &texts[2]),
        ShopEvent::Sale {
            item: "Blue Snail Shell".into(),
            buyer: local_name("buyer"),
            quantity: 1,
            mesos: None,
            item_id: None
        }
    );
}

#[test]
fn window_capture_delivers_frames_of_a_window() {
    // Our own settings window stands in for the game (system windows like the taskbar cannot be captured).
    if !wgc::WindowCapture::supported() {
        println!("window capture is not supported on this machine: skipped");
        return;
    }
    let settings = SettingsWindow::new(icon::load(None), false);
    settings.show(Settings::default()).unwrap();
    app::tests_pump();
    let mut capture = wgc::WindowCapture::new(settings.hwnd()).unwrap_or_else(|e| panic!("window capture: {e:?}"));
    let deadline = Instant::now() + Duration::from_secs(3);
    let frame = loop {
        app::tests_pump();
        if let Some(frame) = capture.latest().unwrap() {
            break frame;
        }
        assert!(Instant::now() < deadline, "no frame within 3 s");
        std::thread::sleep(Duration::from_millis(50));
    };
    // The client area only: the settings window's client size, no title bar.
    let client = crate::game::client_rect(settings.hwnd()).unwrap();
    assert_eq!((frame.width as i32, frame.height as i32), (client.w, client.h));
    assert!(frame.data.iter().any(|&v| v != frame.data[0]), "a real picture, not a blank surface");
}

/// Live check against the running game (`cargo test -p maplelens live_game -- --ignored`): captures the game
/// window, even while it is covered, saves the frame to target\live_game_<time>.png (a candidate fixture) and
/// prints what the watcher makes of it at the measured UI scale.
#[test]
#[ignore = "needs MapleStory running"]
fn live_game_window_capture() {
    let hwnd = crate::game::find_game_window().expect("MapleStory is running");
    let mut capture = wgc::WindowCapture::new(hwnd).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let frame = loop {
        if let Some(frame) = capture.latest().unwrap() {
            break frame;
        }
        assert!(Instant::now() < deadline, "no frame within 5 s (minimized?)");
        std::thread::sleep(Duration::from_millis(100));
    };
    let client = crate::game::client_rect(hwnd).unwrap();
    assert_eq!((frame.width as i32, frame.height as i32), (client.w, client.h));
    let out = format!("{}/../../target/live_game_{}.png", env!("CARGO_MANIFEST_DIR"), crate::paths::now_unix() as u64);
    std::fs::write(&out, maplelens_core::image::encode_png(&frame)).unwrap();
    let ui = maplelens_core::geometry::UiScale::default();
    for t in 0..maplelens_core::config::SCALE_VOTE_MIN_READINGS {
        shop::measure_ui(&ui, &frame, client, t as f64);
    }
    let mut watcher = ShopWatcher::new(shop::reader());
    let mut events = Vec::new();
    for t in 0..3 {
        events.extend(watcher.update(&frame, t as f64));
    }
    println!(
        "{}x{} at UI scale {:.3} (shop: {:?}) saved to {out}",
        frame.width,
        frame.height,
        ui.of(client),
        watcher.scale()
    );
    println!("shop seen: {}, status {:?}, events {events:?}", watcher.sees_shop(), watcher.status());
}

/// What the watcher reads off the newest live capture (`live_game_window_capture` first): chat lines and the meso
/// counter, through Windows OCR. `cargo test -p maplelens live_shop_reads -- --ignored --nocapture`
#[test]
#[ignore = "needs a live capture of an open store"]
fn live_shop_reads() {
    use maplelens_core::shop::{chatlog, layout};
    let dir = format!("{}/../../target", env!("CARGO_MANIFEST_DIR"));
    let newest = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("live_game_")))
        .max()
        .expect("a live capture");
    let frame = decode_png(&std::fs::read(&newest).unwrap()).unwrap();
    let located = layout::locate(&frame, None, &soldlist::has_list).expect("the shop window");
    let (anchors, view) = (located.anchors, layout::View { frame: &frame, scale: located.scale });
    println!("{} at {:.3}: {anchors:?}", newest.display(), located.scale);
    let mut read = shop::reader();
    let enter = anchors.enter.expect("the shop window");
    for line in chatlog::read_lines(&view, enter).expect("a readable chat log") {
        let g = &line.glyphs;
        let mut padded = Mask::new(g.width + 8, g.height + 8);
        for y in 0..g.height {
            for x in 0..g.width {
                padded.set(x + 4, y + 4, g.get(x, y));
            }
        }
        println!("chat: {:?}", maplelens_core::shop::parse::parse_chat(&read(&padded).unwrap_or_default()));
    }
    let balance = view.crop(layout::balance(enter)).to_gray();
    let ink =
        Mask { width: balance.width, height: balance.height, bits: balance.data.iter().map(|&v| v < 150).collect() };
    println!("balance: {:?}", read(&ink));
    if let Some(sold_x) = anchors.sold_x {
        println!("sold rows: {}", soldlist::read_rows(&view, sold_x).map_or(0, |r| r.len()));
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn ideal_ratio_chat_is_read_from_the_unscaled_pixels() {
    if !crate::ocr::available() {
        return;
    }
    // Captured live at UI scale 0.64. Read from the enlarged frame, "DIAksdiw has left the room." came out as
    // "DIAksdi%'it ..."; the game's own small glyphs read cleanly.
    use maplelens_core::shop::{chatlog, layout, lines, parse};
    let native = frame("ideal_ratio_open", (600, 470));
    let located = layout::locate(&native, None, &soldlist::has_list).expect("the shop window");
    let (anchors, view) = (located.anchors, layout::View { frame: &native, scale: located.scale });
    let mut read = shop::reader();
    let events: Vec<ShopEvent> = chatlog::read_lines(&view, anchors.enter.expect("the shop window"))
        .expect("a readable chat log")
        .iter()
        .filter_map(|line| {
            let (area, grey_only) = line.source.expect("lines know where they are");
            let glyphs = lines::glyphs_at(&view, area, grey_only);
            let mut padded = Mask::new(glyphs.width + 8, glyphs.height + 8);
            for y in 0..glyphs.height {
                for x in 0..glyphs.width {
                    padded.set(x + 4, y + 4, glyphs.get(x, y));
                }
            }
            parse::parse_chat(&read(&padded).unwrap_or_default())
        })
        .collect();
    let names: Vec<String> = events
        .iter()
        .map(|e| match e {
            ShopEvent::PlayerEntered(n) => format!("+{n}"),
            ShopEvent::PlayerLeft(n) => format!("-{n}"),
            other => format!("{other:?}"),
        })
        .collect();
    // The meso counter: OCR reads "5,ggo", which only digits can be.
    let balance = lines::glyphs_at(&view, layout::balance(anchors.enter.unwrap()), false);
    let mut padded = Mask::new(balance.width + 8, balance.height + 8);
    for y in 0..balance.height {
        for x in 0..balance.width {
            padded.set(x + 4, y + 4, balance.get(x, y));
        }
    }
    assert_eq!(maplelens_core::shop::watcher::counter_value(&read(&padded).unwrap_or_default()), Some(5990));
    // SusanWWWWW is ambiguous even to the eye at this size; every other name must come out exactly.
    let exact: Vec<&String> = names.iter().filter(|n| !n.contains("Susan")).collect();
    assert_eq!(
        exact,
        [
            &format!("+{}", local_name("owner")),
            "+DIAksdiw",
            "-DIAksdiw",
            "+DIAksdiw",
            "-DIAksdiw",
            "+ElowenX",
            "-ElowenX",
            "+Tommimi",
            "-Tommimi"
        ],
        "{names:?}"
    );
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_owner_is_read_from_the_status_bar_with_windows_ocr() {
    if !crate::ocr::available() {
        return;
    }
    let mut image = frame("sale", (440, 160));
    let path = format!("{}/../../tests/fixtures/hud_status_bar.png", env!("CARGO_MANIFEST_DIR"));
    let bar = decode_png(&std::fs::read(path).unwrap()).unwrap();
    for y in 1000..1200 {
        for x in 0..1920 {
            image.set_pixel(x, y, [96, 150, 70]); // scenery, not black: the bar finder needs its outline against it
        }
    }
    for y in 0..bar.height {
        for x in 0..bar.width {
            image.set_pixel(335 + x, 1094 + y, bar.pixel(x, y));
        }
    }
    let mut watcher = ShopWatcher::new(shop::reader());
    for t in 0..6 {
        watcher.update(&image, f64::from(t));
    }
    let context = watcher.context();
    assert_eq!(context.owner, Some(local_name("owner")), "{context:?}");
    assert!(context.balance.is_some_and(|b| b > 1_000), "{context:?}");
}

/// Every text line of the live store read by the shop's own font and by Windows OCR, side by side, with timings:
/// `cargo test --release -p maplelens live_shop_text -- --ignored --nocapture` (MapleStory running with a store open).
#[test]
#[ignore = "needs MapleStory running with a store open"]
fn live_shop_text() {
    use maplelens_core::shop::layout::{self, View};
    use maplelens_core::shop::{chatlog, lines, text};
    let hwnd = crate::game::find_game_window().expect("MapleStory is running");
    let mut capture = wgc::WindowCapture::new(hwnd).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let frame = loop {
        if let Some(frame) = capture.latest().unwrap() {
            break frame;
        }
        assert!(Instant::now() < deadline, "no frame within 5 s (minimized?)");
        std::thread::sleep(Duration::from_millis(100));
    };
    let out = format!("{}/../../target/live_text_{}.png", env!("CARGO_MANIFEST_DIR"), crate::paths::now_unix() as u64);
    std::fs::write(&out, maplelens_core::image::encode_png(&frame)).unwrap();
    let located = layout::locate(&frame, None, &soldlist::has_list).expect("a store window");
    let view = View { frame: &frame, scale: located.scale };
    println!("{}x{} at UI scale {:.3}, saved to {out}", frame.width, frame.height, located.scale);
    let enter = located.anchors.enter.unwrap();
    let mut areas: Vec<(&str, layout::Area)> = Vec::new();
    for line in chatlog::read_lines(&view, enter).unwrap_or_default() {
        areas.extend(line.source.map(|(a, _)| ("chat", a)));
    }
    if let Some(sold_x) = located.anchors.sold_x {
        for row in soldlist::read_rows(&view, sold_x).unwrap_or_default() {
            for line in row.lines {
                areas.extend(line.source.map(|(a, _)| ("sold", a)));
            }
        }
    }
    areas.push(("counter", layout::balance(enter)));
    let mut read = shop::reader();
    for (kind, area) in areas {
        let start = Instant::now();
        let glyph = text::read_area(&view, area, false);
        let glyph_ms = start.elapsed().as_secs_f64() * 1000.0;
        let ink = lines::glyphs_at(&view, area, kind != "counter");
        let mut padded = Mask::new(ink.width + 8, ink.height + 8);
        for y in 0..ink.height {
            for x in 0..ink.width {
                padded.set(x + 4, y + 4, ink.get(x, y));
            }
        }
        let start = Instant::now();
        let ocr = read(&padded).unwrap_or_default();
        let ocr_ms = start.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{kind:8} font {:<34} ({glyph_ms:5.1} ms)  OCR {ocr:<34} ({ocr_ms:4.1} ms)",
            format!("{:?}", glyph.map(|t| t.text).unwrap_or_default())
        );
    }
}
