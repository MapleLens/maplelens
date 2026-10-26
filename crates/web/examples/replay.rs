//! The browser engine over a folder of captured game frames, in time order: what the page would report for a store
//! watched that way. `cargo run --release -p maplelens-web --example replay -- FRAMES_DIR [--every N]`.
//!
//! Frames are named `shop_YYYYMMDD_HHMMSS.png` (the minutely captures); their times are the engine's clock, and each is
//! shown a few times a second apart, as the page's looks would see it. OCR answers nothing (the worst case: what the
//! game's own fonts read alone), so a sale or visitor with a letter the fonts do not know keeps a '?'.

use maplelens_core::image::decode_png;
use maplelens_web::Engine;
use serde_json::Value;

fn seconds(name: &str) -> Option<f64> {
    let stamp = name.strip_prefix("shop_")?.strip_suffix(".png")?;
    let (date, time) = stamp.split_once('_')?;
    let n = |s: &str, r: std::ops::Range<usize>| s.get(r)?.parse::<f64>().ok();
    Some(n(date, 6..8)? * 86_400.0 + n(time, 0..2)? * 3600.0 + n(time, 2..4)? * 60.0 + n(time, 4..6)?)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("a frames folder");
    let every: usize = match args.next().as_deref() {
        Some("--every") => args.next().and_then(|v| v.parse().ok()).unwrap_or(1),
        _ => 1,
    };
    let fonts = |f: &str| std::fs::read(format!("{}/../../docs/app/fonts/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut engine = Engine::new(fonts("DejaVuSansCondensed.ttf"), fonts("DejaVuSansCondensed-Bold.ttf"))
        .unwrap_or_else(|_| panic!("engine"));
    engine.set_shop(true);
    engine.set_ocr(true);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| seconds(n).is_some())
        .collect();
    names.sort();
    // OCR's answers come a look or more late in the page (Tesseract runs beside the engine): `REPLAY_OCR_LAG=N`.
    let lag: usize = std::env::var("REPLAY_OCR_LAG").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut waiting: std::collections::VecDeque<Vec<u32>> = std::collections::VecDeque::new();
    let mut last_status = String::new();
    let (mut looks, mut slowest) = (0usize, 0.0f64);
    for name in names.iter().step_by(every) {
        let Ok(frame) = decode_png(&std::fs::read(format!("{dir}/{name}")).unwrap()) else { continue };
        let rgba: Vec<u8> = frame.data.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        let t0 = seconds(name).unwrap();
        // A few looks at each frame, as the page would make while it is on screen.
        for k in 0..4 {
            let start = std::time::Instant::now();
            let state: Value = serde_json::from_str(&engine.process(
                &rgba,
                frame.width as u32,
                frame.height as u32,
                0,
                0.0,
                t0 + k as f64 * 1.1,
                1_761_000_000.0 + t0,
            ))
            .unwrap();
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            (looks, slowest) = (looks + 1, slowest.max(ms));
            // OCR answers nothing: every job is answered empty, so nothing waits for it.
            let jobs: Value = serde_json::from_str(&engine.ocr_jobs()).unwrap();
            waiting.push_back(jobs.as_array().unwrap().iter().map(|j| j["id"].as_u64().unwrap() as u32).collect());
            while waiting.len() > lag {
                for id in waiting.pop_front().unwrap() {
                    engine.ocr_answer(id, "");
                }
            }
            let shop = &state["shop"];
            let status = format!("{} | owner {} | balance {}", shop["status"]["text"], shop["owner"], shop["balance"]);
            if status != last_status {
                println!("{name} {ms:5.0} ms  status: {status}");
                last_status = status;
            }
            for event in shop["events"].as_array().unwrap() {
                println!("{name}           EVENT {event}");
            }
        }
    }
    println!("{} frames, {looks} looks, slowest look {slowest:.0} ms", names.len().div_ceil(every));
}
