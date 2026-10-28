//! Frames of the running game for a golden set: `MAPLELENS_LIVE_OUT=<folder> [MAPLELENS_LIVE_N=60] [MAPLELENS_LIVE_EVERY=1]
//! cargo test --release -p maplelens live_frames -- --ignored --nocapture` saves a client frame every MAPLELENS_LIVE_EVERY seconds, taken
//! through Windows Graphics Capture as the shop watcher takes them (the game may be covered; nothing is sent to it).

use std::time::Duration;

use maplelens_core::image::encode_png;

use crate::{game, wgc};

#[test]
#[ignore = "needs the running game"]
fn live_frames() {
    let Ok(out) = std::env::var("MAPLELENS_LIVE_OUT") else { return };
    let count: usize = std::env::var("MAPLELENS_LIVE_N").ok().and_then(|n| n.parse().ok()).unwrap_or(60);
    let every: f64 = std::env::var("MAPLELENS_LIVE_EVERY").ok().and_then(|n| n.parse().ok()).unwrap_or(1.0);
    std::fs::create_dir_all(&out).unwrap();
    let hwnd = game::find_game_window().expect("MapleStory running");
    let mut capture = wgc::WindowCapture::new(hwnd).expect("window capture");
    let mut saved = 0;
    while saved < count {
        std::thread::sleep(Duration::from_secs_f64(every));
        if let Some(frame) = capture.latest().unwrap() {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            std::fs::write(format!("{out}/live_{stamp}.png"), encode_png(&frame)).unwrap();
            saved += 1;
        }
    }
}
