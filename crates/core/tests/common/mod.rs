//! Shared helpers: real game captures from `tests/fixtures`, the card fonts, and a fixed clock.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::OnceLock;

use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::market::time::parse_iso;
use maplelens_core::render::font::Fonts;

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixture(name: &str) -> RgbImage {
    let bytes = std::fs::read(repo().join("tests/fixtures").join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    decode_png(&bytes).expect("fixture is a PNG")
}

/// A player's name a local capture shows (`tests/fixtures/names.txt`, `key=name` lines): real names stay out of git
/// with the captures.
pub fn local_name(key: &str) -> String {
    let path = repo().join("tests/fixtures/names.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("no {key}= in {}", path.display()))
        .trim()
        .to_string()
}

/// `name` from (x, y) on, like the Python tests' `load(name, x, y)`.
pub fn fixture_from(name: &str, x: usize, y: usize) -> RgbImage {
    let image = fixture(name);
    image.crop(x, y, image.width, image.height)
}

/// Tahoma where Windows has it, else the open look-alike the web app ships.
pub fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let tahoma = (std::fs::read("C:/Windows/Fonts/tahoma.ttf"), std::fs::read("C:/Windows/Fonts/tahomabd.ttf"));
        let (regular, bold) = match tahoma {
            (Ok(r), Ok(b)) => (r, b),
            _ => {
                let dir = repo().join("docs/app/fonts");
                (
                    std::fs::read(dir.join("DejaVuSansCondensed.ttf")).expect("fallback font"),
                    std::fs::read(dir.join("DejaVuSansCondensed-Bold.ttf")).expect("fallback bold font"),
                )
            }
        };
        Fonts::from_bytes(regular, bold).expect("fonts parse")
    })
}

/// 2026-10-22T00:00:00Z, the clock every card test uses.
pub fn now() -> f64 {
    parse_iso("2026-10-22T00:00:00Z").expect("valid date")
}
