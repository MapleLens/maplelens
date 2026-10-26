//! Real-window and real-registry tests of the Windows layer (they need a desktop session, like CI's runners).

use std::cell::Cell;

use maplelens_core::geometry::Rect;
use maplelens_core::pipeline::Screen;
use maplelens_core::render::RgbaImage;
use maplelens_core::settings::Settings;
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsWindowVisible, SendMessageW, WM_CLOSE, WM_HOTKEY, WM_LBUTTONDBLCLK,
};

use crate::capture::GdiScreen;
use crate::layered::LayeredWindow;
use crate::settings_window::SettingsWindow;
use crate::tray::{Tray, TrayCommand, register_modifiers};
use crate::{app, icon, refresh, startup};

fn solid(w: usize, h: usize, rgba: [u8; 4]) -> RgbaImage {
    RgbaImage { width: w, height: h, data: rgba.repeat(w * h) }
}

fn rect_of(window: &LayeredWindow) -> (i32, i32, i32, i32) {
    let mut r = RECT::default();
    // SAFETY: our own window, valid out-parameter.
    unsafe { GetWindowRect(window.hwnd(), &mut r).unwrap() };
    (r.left, r.top, r.right, r.bottom)
}

fn visible(window: &LayeredWindow) -> bool {
    // SAFETY: plain query.
    unsafe { IsWindowVisible(window.hwnd()).as_bool() }
}

#[test]
fn overlay_places_moves_resizes_and_hides() {
    let mut window = LayeredWindow::new().unwrap();
    window.set_image(&solid(200, 100, [58, 62, 120, 205]), 600, 400);
    assert_eq!(rect_of(&window), (600, 400, 800, 500));
    assert!(visible(&window));
    window.move_to(700, 450);
    assert_eq!(rect_of(&window), (700, 450, 900, 550));
    window.set_image(&solid(300, 150, [255, 0, 0, 128]), 100, 120);
    assert_eq!(rect_of(&window), (100, 120, 400, 270));
    assert_eq!(window.rect(), Some(Rect::new(100, 120, 300, 150)));
    window.hide();
    assert!(!visible(&window));
    window.set_image(&solid(50, 50, [0, 0, 0, 255]), 10, 10); // shown again after a hide
    assert!(visible(&window));
}

/// Detection grabs the screen: if it saw the card it would read the card instead of the game.
#[test]
fn overlay_is_invisible_to_screen_capture() {
    let mut screen = GdiScreen::new();
    let area = Rect::new(400, 300, 200, 100);
    let before = screen.grab(area);
    let mut window = LayeredWindow::new().unwrap();
    window.set_image(&solid(200, 100, [255, 0, 255, 255]), 400, 300);
    app::tests_pump();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let after = screen.grab(area);
    let magenta = after.data.as_chunks::<3>().0.iter().filter(|p| **p == [255, 0, 255]).count();
    assert!(magenta * 2 < after.width * after.height, "the overlay shows up in screen captures");
    assert_eq!((before.width, before.height), (after.width, after.height));
}

#[test]
fn screen_grabs_have_the_asked_size() {
    let mut screen = GdiScreen::new();
    for (w, h) in [(743, 120), (743, 120), (10, 5), (0, 0)] {
        let image = screen.grab(Rect::new(0, 0, w, h));
        assert_eq!((image.width, image.height), (w as usize, h as usize));
    }
}

#[test]
fn double_click_on_the_tray_icon_opens_settings() {
    let tray = Tray::new(icon::load(None)).unwrap();
    tray.simulate(Tray::TRAY_MESSAGE, 1, WM_LBUTTONDBLCLK as isize);
    assert_eq!(tray.take_commands(), vec![TrayCommand::Settings]);
}

#[test]
fn hotkey_message_toggles_and_hotkeys_register() {
    let mut tray = Tray::new(icon::load(None)).unwrap();
    // Windows drops a WM_HOTKEY that was sent by hand to RegisterHotKey's queue, but our handler sees it.
    tray.simulate(WM_HOTKEY, 1, 0);
    assert_eq!(tray.take_commands(), vec![TrayCommand::Toggle]);
    assert!(tray.set_hotkey(0));
    assert!(tray.set_hotkey(((0x02 | 0x04) << 8) | 0x7B)); // Ctrl+Alt+F12: rarely taken
    assert!(tray.set_hotkey(0));
    assert!(!tray.hwnd().is_invalid());
}

#[test]
fn hotkey_control_flags_become_register_hotkey_flags() {
    let ctrl_alt = ((0x02 | 0x04) << 8) | b'M' as u32;
    assert_eq!(register_modifiers(ctrl_alt).0, 0x2 | 0x1 | 0x4000); // MOD_CONTROL | MOD_ALT | MOD_NOREPEAT
    assert_eq!(register_modifiers((0x01 << 8) | 0x70).0, 0x4 | 0x4000); // Shift+F1
}

#[test]
fn closing_the_settings_window_hides_it_and_it_can_reopen() {
    let window = SettingsWindow::new(icon::load(None), false);
    window.show(Settings::default()).unwrap();
    let hwnd = window.hwnd();
    // SAFETY: our own window.
    unsafe {
        assert!(IsWindowVisible(hwnd).as_bool());
        SendMessageW(hwnd, WM_CLOSE, None, None);
        assert!(!IsWindowVisible(hwnd).as_bool());
    }
    window.show(Settings { overlay_enabled: false, ..Settings::default() }).unwrap();
    // SAFETY: plain query.
    assert!(unsafe { IsWindowVisible(hwnd).as_bool() });
    // Filling the controls is not an edit.
    assert!(window.take_events().is_empty());
}

#[test]
fn startup_registry_roundtrip() {
    let name = "MapleLensTest";
    startup::set_enabled(true, name).unwrap();
    assert!(startup::is_enabled(name));
    startup::set_enabled(false, name).unwrap();
    assert!(!startup::is_enabled(name));
    startup::set_enabled(false, name).unwrap(); // removing twice is fine
    assert!(startup::launch_command().starts_with('"'));
}

#[test]
fn icon_loads() {
    assert!(!icon::load(Some(32)).is_invalid());
}

#[test]
fn fonts_are_found() {
    assert!(app::system_fonts().is_some());
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("maplelens-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const PAYLOAD: &str = r#"{"items": [{"name": "Handwritten demo", "price": 42}]}"#;

#[test]
fn missing_fresh_and_expired_cache() {
    let path = temp_dir("refresh").join("data").join("list.json");
    let calls = Cell::new(0);
    let fetch = || {
        calls.set(calls.get() + 1);
        Ok(PAYLOAD.to_string())
    };
    assert!(refresh::refresh_snapshot(&path, 10_000.0, &fetch).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), PAYLOAD);
    assert!(!refresh::refresh_snapshot(&path, 10_001.0, &fetch).unwrap());
    assert!(refresh::refresh_snapshot(&path, 10_000.0 + 3600.0, &fetch).unwrap());
    assert_eq!(calls.get(), 2);
    assert_eq!(refresh::load_catalog(&path).unwrap().lookup("Handwritten demo").and_then(|i| i.price), Some(42.0));
}

#[test]
fn invalid_responses_and_network_failures_keep_the_cache() {
    let dir = temp_dir("invalid");
    let path = dir.join("list.json");
    let old = r#"{"items": [{"name": "Demo", "price": 1}]}"#;
    for bad in ["<html>Cloudflare challenge</html>", "{}", r#"{"items": [{"name": null}]}"#] {
        std::fs::write(&path, old).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(std::time::UNIX_EPOCH).unwrap();
        let fetch = || Ok(bad.to_string());
        assert!(refresh::refresh_snapshot(&path, 10_000.0, &fetch).is_err(), "{bad}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary files left behind");
    }
    let offline = || Err("offline".into());
    assert!(refresh::refresh_snapshot(&path, 10_000.0, &offline).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
}

#[test]
fn a_cache_from_the_future_is_refreshed() {
    let path = temp_dir("future").join("list.json");
    std::fs::write(&path, PAYLOAD).unwrap();
    let future = std::time::UNIX_EPOCH + std::time::Duration::from_secs(20_000);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(future).unwrap();
    assert!(refresh::refresh_snapshot(&path, 10_000.0, &|| Ok(PAYLOAD.to_string())).unwrap());
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(modified.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), 10_000);
}

#[test]
fn missing_snapshot_falls_back_to_demo_items() {
    let catalog = app::build_catalog(&temp_dir("demo").join("none.json"));
    assert!(catalog.lookup("Emerald Ore").is_some() && catalog.generated_at.is_none());
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn windows_ocr_reads_what_the_atlas_does_not_know() {
    use maplelens_core::image::decode_png;
    use maplelens_core::vision::glyphs::Atlas;
    use maplelens_core::vision::reader::{Ocr, OcrAnswer, TextReader, title_mask};

    let Some(mut engine) = crate::ocr::WindowsOcr::new(false) else {
        eprintln!("no OCR language installed: skipped");
        return;
    };
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/tooltip_native_over_sky.png"))
        .unwrap();
    let mask = title_mask(&decode_png(&bytes).unwrap(), 450, 1).unwrap();
    assert_eq!(engine.recognize(&mask), OcrAnswer::Text("Emerald Ore".into()));
    // Through the reader: an empty atlas falls back to OCR for every letter.
    let mut reader = TextReader::new(Atlas::empty(18, 6, 7), Box::new(engine));
    assert_eq!(reader.read_mask(&mask), "Emerald Ore");
}

/// Live checks of the remote services (`cargo test -p maplelens live_services -- --ignored --nocapture`).
#[test]
#[ignore = "needs the network"]
fn live_services() {
    use crate::update::{PUBLIC_KEY, check::find_update, net::get_bytes};
    let update = find_update(env!("CARGO_PKG_VERSION"), &get_bytes, PUBLIC_KEY);
    println!("update check from {}: {update:?}", env!("CARGO_PKG_VERSION"));
    assert!(matches!(update, Ok(None)), "nothing newer than this build is released");
    let older = find_update("0.1.0", &get_bytes, PUBLIC_KEY);
    println!("update check from 0.1.0: {older:?}");
    if let Ok(name) = std::env::var("MAPLELENS_RANK_NAME") {
        println!("rank: {:?}", crate::net::fetch_rank(&name));
    }
}

/// The settings window as players see it, saved to target\settings_window.png for a visual check
/// (`cargo test -p maplelens preview_settings_window -- --ignored`).
#[test]
#[ignore = "writes a picture for a person to look at"]
fn preview_settings_window() {
    let settings = SettingsWindow::new(icon::load(None), true);
    settings.show(Settings::default()).unwrap();
    settings.show_update_state(&crate::update::UpdateState::default());
    let mut capture = crate::wgc::WindowCapture::new(settings.hwnd()).unwrap();
    // Let every control paint, then keep the newest frame.
    let mut frame = None;
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_millis(1500) || frame.is_none() {
        app::tests_pump();
        frame = capture.latest().unwrap().or(frame);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let frame = frame.expect("a frame");
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/settings_window.png");
    std::fs::write(out, maplelens_core::image::encode_png(&frame)).unwrap();
}
