//! Time `find_tooltips` and the Character Info search on frames from a folder: `cargo run --release -p
//! maplelens-core --example bench_find -- FRAMES_DIR [SCALE]`.

use std::time::Instant;

use maplelens_core::image::decode_png;
use maplelens_core::vision::{find::find_tooltips, info_window::find_window};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("frames dir");
    let scale: f32 = args.next().map_or(0.9, |s| s.parse().expect("scale"));
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    let (mut find_ms, mut info_ms, mut n) = (Vec::new(), Vec::new(), 0);
    for path in files {
        let Ok(frame) = decode_png(&std::fs::read(&path).unwrap()) else { continue };
        let start = Instant::now();
        let spots = find_tooltips(&frame, scale);
        find_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        let gray = frame.to_gray();
        let start = Instant::now();
        let _ = find_window(&gray, scale);
        info_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        n += 1;
        println!(
            "{}: find {:.1} ms ({} spots), info {:.1} ms",
            path.file_name().unwrap().to_string_lossy(),
            find_ms.last().unwrap(),
            spots.len(),
            info_ms.last().unwrap()
        );
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    println!(
        "{n} frames: find mean {:.1} ms max {:.1}; info mean {:.1} ms",
        mean(&find_ms),
        find_ms.iter().copied().fold(0.0, f64::max),
        mean(&info_ms)
    );
}
