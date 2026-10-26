//! Time the no-mouse (browser) detection path on a full game frame: `cargo run --release -p maplelens-core --example bench_search`.

use std::sync::Arc;
use std::time::Instant;

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::sample::sample_items;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::pipeline::info::InfoDetector;
use maplelens_core::vision::reader::{NoOcr, TextReader, name_atlas, title_atlas};

fn fixture(name: &str) -> RgbImage {
    decode_png(&std::fs::read(format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
}

fn main() {
    let (tooltip, backdrop) = (fixture("tooltip_native_over_sky.png"), fixture("character_info_masterball.png"));
    for (w, h) in [(1920usize, 1200usize), (1920, 1080), (2560, 1440)] {
        let mut frame = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                frame.set_pixel(x, y, backdrop.pixel(x % backdrop.width, y % backdrop.height));
            }
        }
        // The tooltip as the game would draw it at this resolution.
        let scale = h as f32 / 1200.0;
        let drawn = tooltip.crop(0, 0, 452, 180).resized(scale);
        for y in 0..drawn.height {
            for x in 0..drawn.width {
                frame.set_pixel(600 + x, 400 + y, drawn.pixel(x, y));
            }
        }
        let frame = frame;
        let bounds = Rect::new(0, 0, w as i32, h as i32);
        let mut hover = HoverDetector::new(
            Arc::new(Catalog::new(sample_items(), None)),
            TextReader::new(title_atlas(), Box::new(NoOcr)),
        );
        let mut info = InfoDetector::new(TextReader::new(name_atlas(), Box::new(NoOcr)));
        let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
        let start = Instant::now();
        let mut found = None;
        for i in 0..10 {
            found = hover.step(&mut screen, bounds, i as f64).map(|h| h.title);
        }
        let hover_ms = start.elapsed().as_secs_f64() * 100.0;
        let start = Instant::now();
        info.step(&mut screen, bounds);
        let info_ms = start.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{w}x{h}: tooltip search {hover_ms:.1} ms/frame (found {found:?}), info window search {info_ms:.1} ms"
        );
    }
}
