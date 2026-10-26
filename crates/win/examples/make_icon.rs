//! Rebuild crates/win/assets/maplelens.ico from the meso coin sprite: `cargo run -p maplelens --example make_icon`.
//!
//! The sprite is 23x24 pixel art, so enlarging uses nearest-neighbour to keep the pixels crisp; only the 16 px
//! size (smaller than the sprite) is averaged down.

use std::path::Path;

use maplelens_core::image::decode_png_rgba;

const SIZES: [usize; 7] = [16, 24, 32, 48, 64, 128, 256];

/// The sprite centred on a transparent square.
fn square(width: usize, height: usize, rgba: &[u8]) -> (usize, Vec<u8>) {
    let side = width.max(height);
    let mut out = vec![0u8; side * side * 4];
    let (ox, oy) = ((side - width) / 2, (side - height) / 2);
    for y in 0..height {
        let src = &rgba[y * width * 4..(y + 1) * width * 4];
        out[((y + oy) * side + ox) * 4..][..width * 4].copy_from_slice(src);
    }
    (side, out)
}

fn resize(side: usize, rgba: &[u8], size: usize) -> Vec<u8> {
    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let pixel = if size >= side {
                let (sx, sy) = (x * side / size, y * side / size);
                rgba[(sy * side + sx) * 4..][..4].to_vec()
            } else {
                // Average the source pixels this one covers, weighting colour by alpha so edges do not go dark.
                let (x0, x1) = (x * side / size, ((x + 1) * side).div_ceil(size));
                let (y0, y1) = (y * side / size, ((y + 1) * side).div_ceil(size));
                let (mut sum, mut alpha, mut n) = ([0u32; 3], 0u32, 0u32);
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let p = &rgba[(sy * side + sx) * 4..][..4];
                        for c in 0..3 {
                            sum[c] += p[c] as u32 * p[3] as u32;
                        }
                        alpha += p[3] as u32;
                        n += 1;
                    }
                }
                let colour = |c: usize| sum[c].checked_div(alpha).map_or(0, |v| v as u8);
                vec![colour(0), colour(1), colour(2), (alpha / n.max(1)) as u8]
            };
            out[(y * size + x) * 4..][..4].copy_from_slice(&pixel);
        }
    }
    out
}

fn png(size: usize, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, size as u32, size as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("png header").write_image_data(rgba).expect("png data");
    out
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sprite = std::fs::read(root.join("../core/assets/meso.png")).expect("meso.png");
    let (w, h, rgba) = decode_png_rgba(&sprite).expect("meso.png decodes");
    let (side, square) = square(w, h, &rgba);
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&s| png(s, &resize(side, &square, s))).collect();
    // ICO: a 6-byte header, a 16-byte entry per image, then the images (PNG data is allowed since Vista).
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len();
    for (&size, image) in SIZES.iter().zip(&images) {
        let side = if size >= 256 { 0 } else { size as u8 };
        ico.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in &images {
        ico.extend_from_slice(image);
    }
    let path = root.join("assets/maplelens.ico");
    std::fs::write(&path, &ico).expect("icon written");
    println!("wrote {} ({} bytes, sizes {SIZES:?})", path.display(), ico.len());
}
