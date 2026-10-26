//! Time the shop watcher on whole game frames: `cargo run --release -p maplelens-core --example bench_shop -- FRAME.png`.

use std::time::Instant;

use maplelens_core::image::decode_png;
use maplelens_core::shop::watcher::ShopWatcher;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("a frame");
    let frame = decode_png(&std::fs::read(&path).unwrap()).unwrap();
    let mut watcher = ShopWatcher::new(Box::new(|_| Some(String::new())));
    // First look: a full-frame search. Later looks: the anchors are followed.
    for i in 0..6 {
        let start = Instant::now();
        let events = watcher.update(&frame, i as f64);
        println!(
            "look {i}: {:.1} ms, shop at scale {:?}, status {:?}, {events:?}",
            start.elapsed().as_secs_f64() * 1000.0,
            watcher.scale(),
            watcher.status()
        );
    }
    let mut cold = ShopWatcher::new(Box::new(|_| Some(String::new())));
    let start = Instant::now();
    cold.update(&frame, 0.0);
    println!("cold look: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
}
