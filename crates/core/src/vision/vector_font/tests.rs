use super::*;
use crate::image::decode_png;

#[test]
fn a_glyph_drawn_at_scale_one_on_whole_pixels_is_its_template_averaged_over_each_pixel() {
    let mut glyph = Glyph::blank(4.0);
    // One reference pixel of full ink, right of the pen on the baseline row.
    for row in BASE - R..BASE {
        for col in PEN..PEN + R {
            glyph.template[row * glyph.cols + col] = 1.0;
        }
    }
    let patch = draw(&glyph, 10.0, 20.0, 1.0);
    let at = |x: i32, y: i32| patch.get((x - patch.x0) as usize, (y - patch.y0) as usize);
    assert!((at(10, 19) - 1.0).abs() < 1e-5);
    assert!(at(9, 19).abs() < 1e-5 && at(11, 19).abs() < 1e-5 && at(10, 20).abs() < 1e-5);
    // Half a pixel to the right, the ink is shared by two pixels.
    let half = draw(&glyph, 10.5, 20.0, 1.0);
    let at = |x: i32, y: i32| half.get((x - half.x0) as usize, (y - half.y0) as usize);
    assert!((at(10, 19) - 0.5).abs() < 1e-5 && (at(11, 19) - 0.5).abs() < 1e-5);
}

#[test]
fn the_atlas_survives_its_files() {
    let mut font = VectorFont::default();
    let mut glyph = Glyph::blank(5.5);
    glyph.template[BASE * glyph.cols + PEN] = 0.6;
    font.glyphs.insert('a', glyph);
    font.glyphs.insert(' ', Glyph::blank(4.0));
    let (png, json) = font.to_files();
    let back = VectorFont::from_files(&png, &json).unwrap();
    assert_eq!(back.glyphs.len(), 2);
    assert!((back.glyphs[&'a'].template[BASE * back.glyphs[&'a'].cols + PEN] - 0.6).abs() < 0.01);
    assert_eq!(back.glyphs[&'a'].advance, 5.5);
}

/// How every corpus line holding a letter fits it and reads (`MAPLELENS_LETTER=R cargo test --release -p
/// maplelens-core letter_debug -- --ignored --nocapture`): finds a mislabelled line or a letter that learnt badly.
#[test]
#[ignore = "a look at the corpus"]
fn letter_debug() {
    let letter = std::env::var("MAPLELENS_LETTER").ok().and_then(|v| v.chars().next()).unwrap_or('R');
    let root = format!("{}/../../tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let corpus = std::fs::read_to_string(format!("{root}/shop_text.tsv")).unwrap();
    for row in corpus.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = row.split('\t').collect();
        if !f[7].contains(letter)
            || (f[2].parse::<f32>().unwrap() / 0.641 - (f[2].parse::<f32>().unwrap() / 0.641).round()).abs() < 0.02
        {
            continue;
        }
        let image = decode_png(&std::fs::read(format!("{root}/shop_{}.png", f[0])).unwrap()).unwrap();
        let n = |i: usize| f[i].parse::<usize>().unwrap();
        let crop = image.crop(n(3), n(4), n(3) + n(5), n(4) + n(6));
        let Some(line) = Line::new(&crop, f[2].parse().unwrap(), f[1] == "plate") else { continue };
        let read_as = read(&line, shop_font()).map(|r| r.text);
        let fit = fit_report(
            shop_font(),
            &[Sample { pixels: crop, scale: line.scale, light_text: f[1] == "plate", text: f[7].to_string() }],
        );
        let mine = fit.iter().find(|(c, ..)| *c == letter);
        println!("{} {} {:?}: {letter} {:?} read {:?}", f[0], f[2], f[7], mine, read_as);
    }
}

/// One corpus line's darkness, baseline and readings at nearby baselines (`MAPLELENS_LINE="24,762" cargo test
/// --release -p maplelens-core line_debug -- --ignored --nocapture`).
#[test]
#[ignore = "a look at the corpus"]
fn line_debug() {
    let wanted = std::env::var("MAPLELENS_LINE").unwrap_or_else(|_| "Smoggy has left the room.".into());
    let root = format!("{}/../../tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let corpus = std::fs::read_to_string(format!("{root}/shop_text.tsv")).unwrap();
    // `MAPLELENS_LINE_FIXTURE` picks the fixture when the same text is in several.
    let fixture = std::env::var("MAPLELENS_LINE_FIXTURE").unwrap_or_default();
    let row = corpus.lines().find(|l| l.ends_with(&format!("\t{wanted}")) && l.starts_with(&fixture)).unwrap();
    let f: Vec<&str> = row.split('\t').collect();
    let image = decode_png(&std::fs::read(format!("{root}/shop_{}.png", f[0])).unwrap()).unwrap();
    let n = |i: usize| f[i].parse::<usize>().unwrap();
    let crop = image.crop(n(3), n(4), n(3) + n(5), n(4) + n(6));
    let line = Line::new(&crop, f[2].parse().unwrap(), false).unwrap();
    for y in 0..line.height {
        println!(
            "{:2} {}",
            y,
            (0..line.width.min(200))
                .map(|x| if line.get(x, y) > 0.5 {
                    '#'
                } else if line.get(x, y) > 0.1 {
                    '+'
                } else {
                    '.'
                })
                .collect::<String>()
        );
    }
    println!("estimated baseline {}", line.base);
    for d in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
        let mut l = line.clone();
        l.base += d;
        println!("base {:.2}: {:?}", l.base, read::read_at(&l, shop_font()).map(|r| (r.text, r.cost)));
    }
    println!("read: {:?}", read(&line, shop_font()).map(|r| (r.text, r.cost)));
    println!("stage ms (draw, baseline, path): {:?}", read::stage_times(&line, shop_font()));
}
