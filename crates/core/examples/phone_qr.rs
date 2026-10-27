//! The website's QR code to the phone app (`docs/assets/phone-qr.svg`), in the same design as the pairing codes the
//! web app draws (docs/app/mapleqr.js: navy dots, maple-orange finder buttons, the meso coin in the middle), from the
//! same encoder: `cargo run -p maplelens-core --example phone_qr > docs/assets/phone-qr.svg`.

use std::fmt::Write;

use maplelens_core::notify::phone::{PHONE_PAGE, qr_modules};
use maplelens_core::render::maple_qr::{QUIET, alignment_centres, in_finder, logo_span};

const INK: &str = "#2c3247";
const PAPER: &str = "#ffffff";
const ORANGE_TOP: &str = "#d0700a";
const ORANGE_BOTTOM: &str = "#8a4300";
const EDGE: &str = "#ffad1f";
/// The coin, as pixel-art rectangles (an SVG shown as an image cannot load another file, so it is copied in).
const MESO: &str = include_str!("../../../docs/assets/meso.svg");
/// The coin's size in its own pixels (meso.svg's viewBox).
const MESO_SIZE: (f64, f64) = (23.0, 24.0);

fn main() {
    let (width, modules) = qr_modules(PHONE_PAGE).expect("the page address fits a QR code");
    let size = (width + 2 * QUIET) as f64;
    let at = |n: usize| (n + QUIET) as f64;
    let span = logo_span(width);
    let from = (width - span) / 2;
    let finder = |x: usize, y: usize| in_finder(x, y, width);
    let under_logo = |x: usize, y: usize| x >= from && x < from + span && y >= from && y < from + span;
    let marks = alignment_centres(width);
    let in_mark = |x: usize, y: usize| marks.iter().any(|&(cx, cy)| x.abs_diff(cx) <= 2 && y.abs_diff(cy) <= 2);

    let mut svg = String::new();
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\">\
         <defs><linearGradient id=\"b\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">\
         <stop offset=\"0\" stop-color=\"{ORANGE_TOP}\"/><stop offset=\"1\" stop-color=\"{ORANGE_BOTTOM}\"/></linearGradient></defs>\
         <rect x=\"0.3\" y=\"0.3\" width=\"{w}\" height=\"{w}\" rx=\"3\" fill=\"{PAPER}\" stroke=\"{EDGE}\" stroke-width=\"0.6\"/>",
        w = size - 0.6
    );
    // Every dot in one path: two half-circle arcs each, touching its neighbours (mapleqr.js `DOT`).
    let mut dots = String::new();
    for (i, _) in modules.iter().enumerate().filter(|(_, dark)| **dark) {
        let (x, y) = (i % width, i / width);
        if finder(x, y) || in_mark(x, y) || under_logo(x, y) {
            continue;
        }
        let (cx, cy, r) = (at(x) + 0.5, at(y) + 0.5, 0.5);
        let _ = write!(dots, "M{:.2} {cy:.2}a{r} {r} 0 1 0 {:.2} 0a{r} {r} 0 1 0 -{:.2} 0z", cx - r, 2.0 * r, 2.0 * r);
    }
    let _ = write!(svg, "<path d=\"{dots}\" fill=\"{INK}\"/>");
    for (cx, cy) in [(0, 0), (width - 7, 0), (0, width - 7)] {
        let _ = write!(
            svg,
            "<rect x=\"{}\" y=\"{}\" width=\"7\" height=\"7\" rx=\"2.2\" fill=\"{INK}\"/>\
             <rect x=\"{}\" y=\"{}\" width=\"5\" height=\"5\" rx=\"1.5\" fill=\"{PAPER}\"/>\
             <rect x=\"{}\" y=\"{}\" width=\"3\" height=\"3\" rx=\"0.9\" fill=\"url(#b)\"/>",
            at(cx),
            at(cy),
            at(cx + 1),
            at(cy + 1),
            at(cx + 2),
            at(cy + 2)
        );
    }
    for &(cx, cy) in marks.iter().filter(|&&(x, y)| !under_logo(x, y)) {
        let _ = write!(
            svg,
            "<rect x=\"{}\" y=\"{}\" width=\"5\" height=\"5\" rx=\"1.4\" fill=\"{INK}\"/>\
             <rect x=\"{}\" y=\"{}\" width=\"3\" height=\"3\" rx=\"0.8\" fill=\"{PAPER}\"/>\
             <rect x=\"{}\" y=\"{}\" width=\"1\" height=\"1\" rx=\"0.3\" fill=\"{INK}\"/>",
            at(cx - 2),
            at(cy - 2),
            at(cx - 1),
            at(cy - 1),
            at(cx),
            at(cy)
        );
    }
    let tile = at(from) + 0.4;
    let tile_size = span as f64 - 0.8;
    let _ = write!(
        svg,
        "<rect x=\"{tile}\" y=\"{tile}\" width=\"{tile_size}\" height=\"{tile_size}\" rx=\"1.6\" fill=\"{PAPER}\" \
         stroke=\"{EDGE}\" stroke-width=\"0.5\"/>"
    );
    let inner = tile_size - 2.4;
    let k = inner / MESO_SIZE.0.max(MESO_SIZE.1);
    let (cw, ch) = (MESO_SIZE.0 * k, MESO_SIZE.1 * k);
    let coin: String = MESO.lines().filter(|line| line.trim_start().starts_with("<rect")).collect();
    let _ = write!(
        svg,
        "<g transform=\"translate({:.3} {:.3}) scale({k:.4})\" shape-rendering=\"crispEdges\">{coin}</g></svg>",
        tile + (tile_size - cw) / 2.0,
        tile + (tile_size - ch) / 2.0
    );
    println!("{svg}");
}
