//! Time the Character Info window search per frame size: `cargo run --release -p maplelens-core --example bench_info`.

use std::time::Instant;

use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::vision::info_window::find_window;

fn main() {
    let backdrop = decode_png(
        &std::fs::read(format!("{}/../../tests/fixtures/character_info_masterball.png", env!("CARGO_MANIFEST_DIR")))
            .unwrap(),
    )
    .unwrap();
    for (w, h) in [
        (1920usize, 1200usize),
        (1920, 1140),
        (1920, 1080),
        (1920, 1020),
        (1920, 960),
        (1920, 1260),
        (1920, 1320),
        (1920, 1380),
    ] {
        let mut frame = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                frame.set_pixel(x, y, backdrop.pixel(x % backdrop.width, y % backdrop.height));
            }
        }
        let gray = frame.to_gray();
        let scale = h as f32 / 1200.0;
        find_window(&gray, scale);
        let start = Instant::now();
        for _ in 0..10 {
            find_window(&gray, scale);
        }
        println!("{w}x{h} (scale {scale:.2}): {:.1} ms", start.elapsed().as_secs_f64() * 100.0);
    }
}
