//! How the shop watcher reads the lines of a labelled corpus (`examples/scene_lines` or `shop_lines` format, first
//! column a frame's path): `cargo run --release -p maplelens-core --example read_corpus -- LINES.tsv`. Prints each
//! line read wrong and the counts (exact; unknown letters only; wrong), by the bundled fonts alone (no OCR).

use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::shop::layout::View;
use maplelens_core::shop::text::read_area;

fn main() {
    let path = std::env::args().nth(1).expect("a corpus .tsv");
    let (mut exact, mut gaps, mut wrong, mut total) = (0, 0, 0, 0);
    let mut frame: Option<(String, RgbImage)> = None;
    for row in std::fs::read_to_string(&path).unwrap().lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = row.split('\t').collect();
        if f.len() < 8 {
            continue;
        }
        if frame.as_ref().is_none_or(|(p, _)| p != f[0]) {
            frame = Some((f[0].to_string(), decode_png(&std::fs::read(f[0]).unwrap()).unwrap()));
        }
        let image = &frame.as_ref().unwrap().1;
        let scale: f32 = f[2].parse().unwrap();
        let n = |i: usize| f[i].parse::<f32>().unwrap();
        // The box back in reference pixels, as the watcher holds it.
        let r = |v: f32| (v / scale).round() as i32;
        let area = (r(n(3)), r(n(4)), r(n(5)), r(n(6)));
        let view = View { frame: image, scale };
        let read = read_area(&view, area, matches!(f[1], "plate" | "hud")).map(|t| t.text).unwrap_or_default();
        let truth = f[7];
        total += 1;
        if read == truth {
            exact += 1;
        } else if read.contains('?') && fits(&read, truth) {
            gaps += 1;
        } else {
            wrong += 1;
            println!("  XX {scale:.2} {truth:?} -> {read:?}");
        }
    }
    println!("{total} lines: {exact} exact, {gaps} with unknown letters only, {wrong} wrong");
}

/// Does `text` fit `pattern`, where '?' stands for one to six letters?
fn fits(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('?', rest)) => (1..=6.min(t.len())).any(|n| go(rest, &t[n..])),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    go(&pattern.chars().collect::<Vec<_>>(), &text.chars().collect::<Vec<_>>())
}
