//! The website's QR code to the phone app (`docs/assets/phone-qr.svg`), drawn by the same encoder as the pairing codes:
//! `cargo run -p maplelens-core --example phone_qr > docs/assets/phone-qr.svg`.

use maplelens_core::notify::phone::{PHONE_PAGE, qr_modules};

/// Scanners expect four light modules around the code.
const QUIET: usize = 4;

fn main() {
    let (width, modules) = qr_modules(PHONE_PAGE).expect("the page address fits a QR code");
    let size = width + 2 * QUIET;
    // One path of unit squares: crisp at any size, a few hundred bytes.
    let mut path = String::new();
    for (i, _) in modules.iter().enumerate().filter(|(_, dark)| **dark) {
        path.push_str(&format!("M{} {}h1v1h-1z", i % width + QUIET, i / width + QUIET));
    }
    println!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" shape-rendering=\"crispEdges\">\
         <rect width=\"{size}\" height=\"{size}\" fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>"
    );
}
