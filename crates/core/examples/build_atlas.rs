//! Rebuild the bundled glyph atlases from the labelled fixtures in `vision::samples`, and report how well each
//! sample reads with an atlas built from all the *other* samples (a fair test of unseen captures).
//!
//! `cargo run -p maplelens-core --example build_atlas`

use std::path::{Path, PathBuf};

use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::vision::glyphs::Atlas;
use maplelens_core::vision::samples::{Font, SAMPLES, Sample};
use maplelens_core::vision::{Mask, info_window, reader};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mask(sample: &Sample) -> Mask {
    let bytes = std::fs::read(root().join("tests/fixtures").join(sample.fixture)).expect("fixture exists");
    let image: RgbImage = decode_png(&bytes).expect("fixture is a PNG");
    let (x, y) = sample.origin;
    match sample.font {
        Font::Title => {
            let tooltip = image.crop(x, y, image.width, image.height);
            reader::title_mask(&tooltip, sample.width, sample.lines).expect("title area is inside the fixture")
        }
        Font::Name => {
            let window = maplelens_core::geometry::Rect::new(x as i32, y as i32, 0, 0);
            let b = info_window::name_box(window, 1.0);
            info_window::name_mask(&image.crop(b.x as usize, b.y as usize, b.right() as usize, b.bottom() as usize))
        }
    }
}

fn empty(font: Font) -> Atlas {
    match font {
        Font::Title => Atlas::empty(18, 6, 7),
        Font::Name => Atlas::empty(14, 4, 99),
    }
}

fn main() {
    for font in [Font::Title, Font::Name] {
        let samples: Vec<&Sample> = SAMPLES.iter().filter(|s| s.font == font).collect();
        let masks: Vec<Mask> = samples.iter().map(|s| mask(s)).collect();
        let mut atlas = empty(font);
        for (sample, mask) in samples.iter().zip(&masks) {
            let added = atlas.learn(mask, sample.text);
            println!("{font:?} learn {:<45} +{added}", sample.text);
        }
        for (i, sample) in samples.iter().enumerate() {
            let mut others = empty(font);
            for (j, (s, m)) in samples.iter().zip(&masks).enumerate() {
                if j != i {
                    others.learn(m, s.text);
                }
            }
            let held_out = others.read(&masks[i]);
            let full = atlas.read(&masks[i]);
            println!("{font:?} read {:<45} full={:?} held-out={:?}", sample.text, full.text, held_out.text);
        }
        let name = match font {
            Font::Title => "title",
            Font::Name => "name",
        };
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("assets/glyphs/{name}.json"));
        std::fs::write(&path, atlas.to_json() + "\n").expect("atlas written");
        println!("wrote {} ({} glyphs)", path.display(), atlas.glyphs.len());
    }
}
