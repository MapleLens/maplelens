//! Learn the shop's anti-aliased font from the labelled corpus (`tests/fixtures/shop_text.tsv`) and write it to
//! `assets/glyphs/shop_vector.{png,json}`:
//! `cargo run --release -p maplelens-core --example build_shop_font -- [--init FONT.ttf] [--rounds N] [--holdout SCALE]`.
//!
//! `--init` starts from a font file's outlines (any Arial-like font on the developer's machine; only what is learnt
//! from the game's pixels is written), otherwise from the bundled atlas. `--holdout` trains without the lines at that
//! UI scale and reports how they read instead of writing anything; `--fold K/N` holds out the lines of every N-th
//! name (from fold K), names the font never saw, which is what a new visitor is. `--check` reads every line and
//! writes nothing (with `--rounds 0`: how the bundled font reads its corpus). `--corpus LINES.tsv` (repeatable) adds
//! lines whose first column is a whole frame's path, as `examples/scene_lines` writes them from rendered scenes;
//! `--no-fixtures` leaves the fixtures' corpus out (to score a held-out corpus alone); `--bitmap-only` relearns only
//! the bitmap table (`shop_small.json`).

use std::collections::HashMap;

use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::shop::text::settle_bars;
use maplelens_core::vision::pixel_font::{line_mask, small_name_font};
use maplelens_core::vision::vector_font::{Line, Sample, VectorFont, fit_report, read, shop_font, train};

/// Lines of one text at one scale the templates learn from, at most.
const MAX_REPEATS: usize = 2;

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
    let mut extra: Vec<String> = Vec::new();
    let mut no_fixtures = false;
    let mut bitmap_only = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--init" => init = args.next(),
            "--rounds" => rounds = args.next().and_then(|v| v.parse().ok()).expect("--rounds N"),
            "--holdout" => holdout = args.next().and_then(|v| v.parse::<f32>().ok()),
            "--check" => check = true,
            "--corpus" => extra.push(args.next().expect("--corpus LINES.tsv")),
            "--no-fixtures" => no_fixtures = true,
            "--bitmap-only" => bitmap_only = true,
            "--fold" => {
                fold = args.next().and_then(|v| {
                    v.split_once('/').map(|(k, n)| (k.parse::<u64>().unwrap(), n.parse::<u64>().unwrap()))
                });
            }
            other => panic!("unknown argument {other}"),
        }
    }
    let mut corpus = if no_fixtures {
        String::new()
    } else {
        std::fs::read_to_string(format!("{root}/tests/fixtures/shop_text.tsv")).unwrap()
    };
    for path in &extra {
        corpus.push('\n');
        corpus.push_str(&std::fs::read_to_string(path).unwrap());
    }
    let mut images: HashMap<String, RgbImage> = HashMap::new();
    let mut samples = Vec::new();
    let mut repeats: HashMap<(String, i32), usize> = HashMap::new();
    let mut bitmap = Vec::new();
    for row in corpus.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = row.split('\t').collect();
        let scale: f32 = f[2].parse().unwrap();
        // Rows come grouped by frame: only the current one is kept (thousands of whole frames do not fit).
        if images.len() > 1 && !images.contains_key(f[0]) {
            images.clear();
        }
        let image = images.entry(f[0].to_string()).or_insert_with(|| {
            // A corpus of whole frames names them by path; the fixtures' by their name.
            let path = if f[0].ends_with(".png") {
                f[0].to_string()
            } else {
                format!("{root}/tests/fixtures/shop_{}.png", f[0])
            };
            decode_png(&std::fs::read(path).unwrap()).unwrap()
        });
        let n = |i: usize| f[i].parse::<usize>().unwrap();
        let (x, y, w, h) = (n(3), n(4), n(5), n(6));
        if bitmap_scale(scale) {
            let block = (scale / 0.641).round() as usize;
            bitmap.push((
                line_mask(&image.crop(x, y, x + w, y + h), block, matches!(f[1], "plate" | "hud")),
                f[7].to_string(),
            ));
            continue;
        }
        // The meso counter is another font (Tahoma): its shapes go to the exact table above, not to these templates.
        // A text seen twice at a scale teaches the templates little a third time ("Number purchased: 1").
        let seen = repeats.entry((f[7].to_string(), (scale * 20.0).round() as i32)).or_insert(0);
        *seen += 1;
        if f[1] == "counter" || *seen > MAX_REPEATS {
            continue;
        }
        samples.push(Sample {
            pixels: image.crop(x, y, x + w, y + h),
            scale,
            light_text: matches!(f[1], "plate" | "hud"),
            text: f[7].to_string(),
        });
    }
    // A fold holds out every line of some texts (names never seen in training), grouped by the name in the line.
    let group = |text: &str| {
        let key = text.split(" has ").next().unwrap_or(text).trim_start_matches("Buyer: ");
        key.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
    };
    // The bitmap font (Ideal Ratio): read with the name table as it is, then learn the letters these lines add.
    // Learnt on top of the shipped table, by vote: one line whose pieces happen to number its letters must not teach a
    // wrong shape (first come, first kept had read worse than the table it replaced).
    let mut table = maplelens_core::vision::pixel_font::shop_small_font();
    let before = table.len();
    let read_all = |table: &maplelens_core::vision::pixel_font::PixelFont| {
        bitmap.iter().filter(|(m, t)| m.as_ref().is_some_and(|m| table.read_line(m).0 == *t)).count()
    };
    let exact_before = read_all(&table);
    let voters: Vec<_> = bitmap.iter().filter_map(|(m, t)| Some((m.clone()?, t.replace(' ', "")))).collect();
    for _ in 0..3 {
        table.learn_by_vote(&voters, 2);
    }
    println!(
        "bitmap font: {} lines, {exact_before} read exactly with the shipped table ({before} shapes), {} after learning ({} shapes)",
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
    if bitmap_only {
        let added = table.added_since(&small_name_font());
        std::fs::write(format!("{root}/crates/core/assets/glyphs/shop_small.json"), added.to_json()).unwrap();
        println!("written shop_small.json ({} shapes)", added.len());
        return;
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
    // As the watcher reads a line: the bundled tables first (the bitmap font, shrunk back at the default sizes),
    // then the drawn templates.
    let (mut w_exact, mut w_wrong) = (0, 0);
    for s in report {
        let got = maplelens_core::shop::text::read_pixels(&s.pixels, s.scale, s.light_text).map(|t| t.text);
        match got {
            Some(g) if g == s.text => w_exact += 1,
            Some(g) if !fits(&g.chars().collect::<Vec<_>>(), &s.text.chars().collect::<Vec<_>>()) => {
                w_wrong += 1;
                println!("  WW {:.2} {:?} -> {g:?}", s.scale, s.text);
            }
            _ => {}
        }
    }
    println!("as the watcher reads them (bundled fonts): {w_exact} exact, {w_wrong} WRONG");
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
