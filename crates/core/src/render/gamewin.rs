//! Pieces of MapleStory's own windows (Character Info, Item List...): blue frame, white title bar, light inner
//! panel, pink label pills, beige value boxes.
//!
//! Every colour and size was measured pixel by pixel on a native screenshot of the Character Info window
//! (1x, no scaling), so the pieces line up with the real window when drawn beside it.

use super::canvas::{Anchor, Canvas, Ink};
use super::theme::Rgba;

/// Frame and bottom strip.
pub const BODY: Rgba = [156, 177, 200, 255];
/// Light line outside the frame.
pub const FRAME_OUTER: Rgba = [209, 219, 226, 255];
/// Dark blue line just inside it.
pub const FRAME_DARK: Rgba = [112, 133, 155, 255];
pub const FRAME_MID: Rgba = [146, 171, 196, 255];
pub const TITLE_FILL: Rgba = [255, 255, 255, 255];
/// Light area the rows sit on.
pub const PANEL: Rgba = [245, 245, 247, 255];
pub const PANEL_EDGE: Rgba = [170, 185, 203, 255];
pub const INK: Rgba = [36, 36, 36, 255];
pub const INK_DIM: Rgba = [104, 108, 118, 255];
pub const VALUE_FILL: Rgba = [226, 223, 212, 255];
pub const VALUE_EDGE: Rgba = [208, 210, 202, 255];
pub const PINK: Rgba = [204, 79, 130, 255];
pub const PINK_EDGE: Rgba = [166, 70, 110, 255];
pub const PINK_LIGHT: Rgba = [214, 109, 154, 255];
pub const PINK_INK: Rgba = [255, 233, 246, 255];

/// Frame thickness: the title bar and panel start this far in.
pub const BORDER: f32 = 6.0;
pub const TITLE_H: f32 = 20.0;
pub const CREDIT_H: f32 = 14.0;
/// Blue strip under the panel: gap, the data credit, the frame.
pub const STRIP_H: f32 = 3.0 + CREDIT_H + BORDER;
/// 2 px of blue between title bar and panel.
pub const PANEL_TOP: f32 = BORDER + TITLE_H + 2.0;
/// Content starts this far from the card edge (6 px inside the panel).
pub const PAD: f32 = BORDER + 6.0;
/// Panel top/bottom padding.
pub const PAD_V: f32 = 6.0;
pub const ROW_H: f32 = 22.0;
pub const ROW_GAP: f32 = 4.0;
/// Half of Tahoma's capital height / size: puts text's visual centre on a line.
const CAP: f32 = 0.36;
const PANEL_SHADE: Rgba = [210, 214, 219, 255];

/// Blue frame, white title bar and the light panel above the bottom strip.
pub fn window(cv: &mut Canvas<'_>, height: f32, title: &str) {
    let w = cv.width;
    cv.rounded((0.0, 0.0, w, height), 5.0, Some(FRAME_OUTER), None);
    cv.rounded((1.0, 1.0, w - 2.0, height - 2.0), 5.0, Some(FRAME_DARK), None);
    cv.rounded((2.0, 2.0, w - 4.0, height - 4.0), 4.0, Some(FRAME_MID), None);
    cv.rounded((3.0, 3.0, w - 6.0, height - 6.0), 3.0, Some(BODY), None);
    cv.rounded((BORDER, BORDER, w - 2.0 * BORDER, TITLE_H), 3.0, Some(TITLE_FILL), None);
    let (px0, py0, px1, py1) = (BORDER, PANEL_TOP, w - BORDER, height - STRIP_H);
    cv.rounded((px0, py0, px1 - px0, py1 - py0), 3.0, Some(PANEL_EDGE), None);
    cv.rect(px0 + 1.0, py0 + 1.0, px1 - px0 - 2.0, py1 - py0 - 2.0, PANEL);
    // A soft shade along the panel's bottom edge, like the game's.
    let shade_top = py1 - 1.0 - 5.0;
    for i in 0..5 {
        let t = i as f32 / 4.0;
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        let colour = [mix(PANEL[0], PANEL_SHADE[0]), mix(PANEL[1], PANEL_SHADE[1]), mix(PANEL[2], PANEL_SHADE[2]), 255];
        cv.rect(px0 + 1.0, shade_top + i as f32, px1 - px0 - 2.0, 1.0, colour);
    }
    tracked(cv, PAD, BORDER + TITLE_H / 2.0, title, 9.0, INK);
}

/// The window titles are blocky, hard-edged, wide capitals: bold, unsmoothed and letter-spaced.
pub fn tracked(cv: &mut Canvas<'_>, mut x: f32, y: f32, text: &str, size: f32, fill: Rgba) {
    let baseline = y + (CAP * size).round();
    for ch in text.chars() {
        let s = ch.to_string();
        cv.text_aliased(x, baseline, &s, Ink::new(size, fill).bold(), Anchor::LS);
        x += cv.measure(&s, size, true) + 1.0;
    }
}

/// Text whose capital letters are centred on (cx, cy).
pub fn centred(cv: &mut Canvas<'_>, cx: f32, cy: f32, text: &str, size: f32, fill: Rgba) {
    cv.text(cx, cy + CAP * size, text, Ink::new(size, fill), Anchor::MS);
}

/// Pink label (LEVEL, JOB...): dark pink outline, lighter inner line, flat fill, pale pink text.
pub fn pill(cv: &mut Canvas<'_>, (x, y, w, h): (f32, f32, f32, f32), text: &str, size: f32) {
    cv.rounded((x, y, w, h), 3.0, Some(PINK_EDGE), None);
    cv.rounded((x + 1.0, y + 1.0, w - 2.0, h - 2.0), 2.0, Some(PINK_LIGHT), None);
    cv.rounded((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 2.0, Some(PINK), None);
    centred(cv, x + w / 2.0, y + h / 2.0, text, size, PINK_INK);
}

pub fn value_box(cv: &mut Canvas<'_>, (x, y, w, h): (f32, f32, f32, f32)) {
    cv.rounded((x, y, w, h), 2.0, Some(VALUE_EDGE), None);
    cv.rounded((x + 1.0, y + 1.0, w - 2.0, h - 2.0), 2.0, Some(VALUE_FILL), None);
}

/// `LABEL | value` the way Character Info lists LEVEL / JOB / FAME; returns the y below it.
pub fn row(cv: &mut Canvas<'_>, y: f32, label: &str, value: &str, height: f32, value_size: f32) -> f32 {
    let label_w = 86.0;
    pill(cv, (PAD, y, label_w, height), label, 11.0);
    let box_x = PAD + label_w + 2.0;
    let box_w = cv.width - PAD - box_x;
    value_box(cv, (box_x, y, box_w, height));
    cv.text(box_x + 7.0, y + height / 2.0 + CAP * value_size, value, Ink::new(value_size, INK), Anchor::LS);
    y + height + ROW_GAP
}
