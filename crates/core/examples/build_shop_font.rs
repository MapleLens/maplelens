//! Learn the shop's anti-aliased font from the labelled corpus (`tests/fixtures/shop_text.tsv`) and write it to
//! `assets/glyphs/shop_vector.{png,json}`:
//! `cargo run --release -p maplelens-core --example build_shop_font -- [--init FONT.ttf] [--rounds N] [--holdout SCALE]`.
//!
//! `--init` starts from a font file's outlines (any Arial-like font on the developer's machine; only what is learnt
//! from the game's pixels is written), otherwise from the bundled atlas. `--holdout` trains without the lines at that
//! UI scale and reports how they read instead of writing anything; `--fold K/N` holds out the lines of every N-th
//! name (from fold K), names the font never saw, which is what a new visitor is. `--check` reads every line and
//! writes nothing (with `--rounds 0`: how the bundled font reads its corpus).

use std::collections::HashMap;

use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::shop::text::settle_bars;
use maplelens_core::vision::pixel_font::{line_mask, small_name_font};
use maplelens_core::vision::vector_font::{Line, Sample, VectorFont, fit_report, read, shop_font, train};

/// UI scales the game draws its bitmap font at (Ideal Ratio and its whole multiples): not this font.
fn bitmap_scale(scale: f32) -> bool {
    let k = (scale / 0.641).round();
    k >= 1.0 && (scale / (0.641 * k) - 1.0).abs() < 0.01
}

/// Does `text` fit `pattern`, where '?' stands for one to six letters?
fn fits(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('?', rest)) => (1..=6.min(text.len())).any(|n| fits(rest, &text[n..])),
        Some((c, rest)) => text.first() == Some(c) && fits(rest, &text[1..]),
    }
}

fn main() {
    let root = format!("{}/../..", env!("CARGO_MANIFEST_DIR"));
    let mut args = std::env::args().skip(1);
    let (mut init, mut rounds, mut holdout, mut fold, mut check) = (None, 6usize, None, None, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--init" => init = args.next(),
            "--rounds" => rounds = args.next().and_then(|v| v.parse().ok()).expect("--rounds N"),
            "--holdout" => holdout = args.next().and_then(|v| v.parse::<f32>().ok()),
            "--check" => check = true,
            "--fold" => {
                fold = args.next().and_then(|v| {
                    v.split_once('/').map(|(k, n)| (k.parse::<u64>().unwrap(), n.parse::<u64>().unwrap()))
                });
            }
            other => panic!("unknown argument {other}"),
        }
    }
    let corpus = std::fs::read_to_string(format!("{root}/tests/fixtures/shop_text.tsv")).unwrap();
    let mut images: HashMap<String, RgbImage> = HashMap::new();
    let mut samples = Vec::new();
    let mut bitmap = Vec::new();
    for row in corpus.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = row.split('\t').collect();
        let scale: f32 = f[2].parse().unwrap();
        let image = images.entry(f[0].to_string()).or_insert_with(|| {
            decode_png(&std::fs::read(format!("{root}/tests/fixtures/shop_{}.png", f[0])).unwrap()).unwrap()
        });
        let n = |i: usize| f[i].parse::<usize>().unwrap();
        let (x, y, w, h) = (n(3), n(4), n(5), n(6));
        if bitmap_scale(scale) {
            let block = (scale / 0.641).round() as usize;
            bitmap.push((line_mask(&image.crop(x, y, x + w, y + h), block, f[1] == "plate"), f[7].to_string()));
            continue;
        }
        samples.push(Sample {
            pixels: image.crop(x, y, x + w, y + h),
            scale,
            light_text: f[1] == "plate",
            text: f[7].to_string(),
        });
    }
    // A fold holds out every line of some texts (names never seen in training), grouped by the name in the line.
    let group = |text: &str| {
        let key = text.split(" has ").next().unwrap_or(text).trim_start_matches("Buyer: ");
        key.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
    };
    // The bitmap font (Ideal Ratio): read with the name table as it is, then learn the letters these lines add.
    let mut table = small_name_font();
    let before = table.len();
    let read_all = |table: &maplelens_core::vision::pixel_font::PixelFont| {
        bitmap.iter().filter(|(m, t)| m.as_ref().is_some_and(|m| table.read_line(m).0 == *t)).count()
    };
    let exact_before = read_all(&table);
    for _ in 0..3 {
        for (mask, text) in &bitmap {
            if let Some(mask) = mask {
                table.learn(mask, &text.replace(' ', ""));
            }
        }
    }
    println!(
        "bitmap font: {} lines, {exact_before} read exactly with the name table ({before} shapes), {} after learning ({} shapes)",
        bitmap.len(),
        read_all(&table),
        table.len()
    );
    for (mask, text) in &bitmap {
        let got = mask.as_ref().map(|m| settle_bars(&table.read_line(m).0)).unwrap_or_default();
        if got != *text {
            println!("  bitmap {text:?} -> {got:?}");
        }
    }
    let held = |s: &Sample| {
        holdout.is_some_and(|h| (s.scale - h).abs() < 0.01) || fold.is_some_and(|(k, n)| group(&s.text) % n == k)
    };
    let (test, learn): (Vec<Sample>, Vec<Sample>) = samples.into_iter().partition(held);
    let start = match &init {
        Some(path) => {
            let mut chars: Vec<char> = learn.iter().flat_map(|s| s.text.chars()).collect();
            chars.push(' ');
            chars.sort_unstable();
            chars.dedup();
            VectorFont::from_ttf(&std::fs::read(path).unwrap(), chars).expect("a font file")
        }
        None => shop_font().clone(),
    };
    println!("learning {} letters from {} lines ({} held out)", start.glyphs.len(), learn.len(), test.len());
    let begun = std::time::Instant::now();
    let (font, costs) = train(start, &learn, rounds);
    println!("alignment cost by round: {costs:.0?} ({:.0?})", begun.elapsed());
    let mut fits_by_letter = fit_report(&font, &learn);
    fits_by_letter.sort_by(|a, b| b.2.total_cmp(&a.2));
    println!(
        "worst-fitting letters (letter, instances, error per column): {:.2?}",
        &fits_by_letter[..fits_by_letter.len().min(16)]
    );
    let evaluating = holdout.is_some() || fold.is_some();
    let report = if evaluating { &test } else { &learn };
    let (mut exact, mut safe, mut wrong) = (0, 0, 0);
    let begun = std::time::Instant::now();
    for s in report {
        let Some(line) = Line::new(&s.pixels, s.scale, s.light_text) else { continue };
        let got = read(&line, &font).map(|r| settle_bars(&r.text)).unwrap_or_default();
        let (g, t): (Vec<char>, Vec<char>) = (got.chars().collect(), s.text.chars().collect());
        if got == s.text {
            exact += 1;
        } else if fits(&g, &t) {
            safe += 1;
            println!("  ?  {:.2} {:?} -> {got:?}", s.scale, s.text);
        } else {
            wrong += 1;
            println!("  XX {:.2} {:?} -> {got:?}", s.scale, s.text);
        }
    }
    let per_line = begun.elapsed() / report.len().max(1) as u32;
    println!(
        "{} lines: {exact} exact, {safe} with unknown letters only, {wrong} WRONG ({per_line:?} a line)",
        report.len()
    );
    if !evaluating && !check {
        let (png, json) = font.to_files();
        std::fs::write(format!("{root}/crates/core/assets/glyphs/shop_vector.png"), png).unwrap();
        std::fs::write(format!("{root}/crates/core/assets/glyphs/shop_vector.json"), json).unwrap();
        let added = table.added_since(&small_name_font());
        std::fs::write(format!("{root}/crates/core/assets/glyphs/shop_small.json"), added.to_json()).unwrap();
        println!("written assets/glyphs/shop_vector.{{png,json}} and shop_small.json ({} shapes)", added.len());
    }
}
