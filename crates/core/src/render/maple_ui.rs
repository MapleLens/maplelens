//! The game's controls for windows MapleLens draws itself (the settings window): the orange OK button and its grey
//! twin, check boxes, a slider and the title bar's close box. Colours follow the site's maple.css, which took them
//! from the client, so the app and the site look like one thing.

use super::canvas::{Anchor, Canvas, Ink};
use super::gamewin;
use super::theme::{Rgba, with_alpha};

pub type Area = (f32, f32, f32, f32);

/// How a control is being touched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Touch {
    pub hover: bool,
    pub pressed: bool,
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// The game's "OK": the action the window is for.
    Orange,
    Grey,
}

const ORANGE: [Rgba; 3] = [[255, 224, 122, 255], [255, 173, 31, 255], [240, 138, 0, 255]];
/// The orange button's face top to bottom, for other things drawn in it.
pub const ORANGE_STOPS: [(f32, Rgba); 3] = [(0.0, ORANGE[0]), (0.55, ORANGE[1]), (1.0, ORANGE[2])];
pub const ORANGE_EDGE: Rgba = [154, 82, 0, 255];
const GREY: [Rgba; 3] = [[255, 255, 255, 255], [227, 232, 241, 255], [201, 208, 222, 255]];
const GREY_EDGE: Rgba = [77, 86, 112, 255];
const GREY_INK: Rgba = [44, 50, 71, 255];
const WHITE: Rgba = [255, 255, 255, 255];
/// The site's focus outline: the keyboard's place must show.
pub const FOCUS: Rgba = [29, 91, 184, 255];
/// A highlighted edge: the box the pointer is over, or the hotkey box while it listens.
pub const ACCENT: Rgba = [240, 138, 0, 255];
/// The button's 3D lip under its face.
const LIP: f32 = 2.0;

/// Lighter by `amount` (0..1), for hover.
fn lift(colour: Rgba, amount: f32) -> Rgba {
    let up = |c: u8| (c as f32 + (255.0 - c as f32) * amount).round() as u8;
    [up(colour[0]), up(colour[1]), up(colour[2]), colour[3]]
}

fn shade(colours: [Rgba; 3], touch: Touch) -> Vec<(f32, Rgba)> {
    let amount = if touch.hover && !touch.pressed { 0.25 } else { 0.0 };
    let stops = [0.0, 0.55, 1.0];
    stops.into_iter().zip(colours).map(|(at, c)| (at, lift(c, amount))).collect()
}

/// A raised, rounded button with a lip below it; pressing pushes it down onto the lip. `area` includes the lip.
pub fn button(cv: &mut Canvas<'_>, (x, y, w, h): Area, label: &str, tone: Tone, touch: Touch) {
    let tone = if touch.disabled { Tone::Grey } else { tone };
    let (colours, edge) = match tone {
        Tone::Orange => (ORANGE, ORANGE_EDGE),
        // White barely lightens: the grey button shows the pointer with the accent edge instead.
        Tone::Grey if touch.hover && !touch.disabled => (GREY, ACCENT),
        Tone::Grey => (GREY, GREY_EDGE),
    };
    let face_h = h - LIP;
    let sink = if touch.pressed && !touch.disabled { LIP } else { 0.0 };
    let alpha = |c: Rgba| if touch.disabled { with_alpha(c, 130) } else { c };
    cv.rounded((x, y + LIP, w, face_h), 5.0, Some(alpha(edge)), None);
    let face = (x, y + sink, w, face_h);
    cv.rounded(face, 5.0, Some(alpha(edge)), None);
    let stops: Vec<(f32, Rgba)> = shade(colours, touch).into_iter().map(|(at, c)| (at, alpha(c))).collect();
    cv.gradient((x + 1.0, y + sink + 1.0, w - 2.0, face_h - 2.0), 4.0, &stops);
    // The white shine along the top inside edge.
    cv.rect(x + 4.0, y + sink + 2.0, w - 8.0, 1.0, with_alpha(WHITE, if touch.disabled { 60 } else { 150 }));
    let (cx, cy) = (x + w / 2.0, y + sink + face_h / 2.0);
    let ink = Ink::new(12.0, WHITE).bold();
    match tone {
        Tone::Orange => {
            cv.outlined(cx, cy + 4.0, label, ink, edge, Anchor::MS);
        }
        Tone::Grey => {
            let colour = if touch.disabled { gamewin::INK_DIM } else { GREY_INK };
            cv.text(cx, cy + 5.0, label, Ink::new(12.0, WHITE).bold(), Anchor::MS);
            cv.text(cx, cy + 4.0, label, Ink { colour, ..ink }, Anchor::MS);
        }
    }
}

/// A 14-unit check box at (x, y): white when off, the orange button's face with a white tick when on.
pub fn check_box(cv: &mut Canvas<'_>, x: f32, y: f32, checked: bool, hover: bool) {
    const SIZE: f32 = 14.0;
    if checked {
        cv.rounded((x, y, SIZE, SIZE), 3.0, Some(ORANGE_EDGE), None);
        cv.gradient(
            (x + 1.0, y + 1.0, SIZE - 2.0, SIZE - 2.0),
            2.0,
            &shade(ORANGE, Touch { hover, ..Touch::default() }),
        );
        let tick = [(x + 3.5, y + 7.5), (x + 6.0, y + 10.0), (x + 10.5, y + 4.0)];
        cv.polyline(&tick, ORANGE_EDGE, 3.4);
        cv.polyline(&tick, WHITE, 1.8);
    } else {
        let edge = if hover { ACCENT } else { gamewin::FRAME_DARK };
        cv.rounded((x, y, SIZE, SIZE), 3.0, Some(edge), None);
        cv.gradient((x + 1.0, y + 1.0, SIZE - 2.0, SIZE - 2.0), 2.0, &[(0.0, [226, 230, 238, 255]), (1.0, WHITE)]);
    }
}

const KNOB_W: f32 = 11.0;

/// Where along a slider across `area` the design x falls (0..=1): the knob's centre travels the track.
pub fn slider_fraction((x, _, w, _): Area, at: f32) -> f32 {
    ((at - x - KNOB_W / 2.0) / (w - KNOB_W)).clamp(0.0, 1.0)
}

/// A horizontal slider across `area` at `fraction` (0..=1) of its range, with a tick per `ticks` step.
pub fn slider(cv: &mut Canvas<'_>, (x, y, w, h): Area, fraction: f32, ticks: usize, touch: Touch) {
    let (knob_w, knob_h) = (KNOB_W, 16.0);
    let track_x = x + knob_w / 2.0;
    let track_w = w - knob_w;
    let mid = y + h / 2.0 - 2.0;
    cv.rounded((track_x - 1.0, mid - 3.0, track_w + 2.0, 6.0), 3.0, Some(gamewin::PANEL_EDGE), None);
    cv.rounded((track_x, mid - 2.0, track_w, 4.0), 2.0, Some(gamewin::PANEL), None);
    let at = track_x + track_w * fraction.clamp(0.0, 1.0);
    if at > track_x + 1.0 {
        cv.gradient((track_x, mid - 2.0, at - track_x, 4.0), 2.0, &shade(ORANGE, Touch::default()));
    }
    for i in 0..=ticks {
        let tx = (track_x + track_w * i as f32 / ticks as f32).round();
        cv.rect(tx, mid + 6.0, 1.0, 3.0, gamewin::PANEL_EDGE);
    }
    let knob = (at - knob_w / 2.0, mid - knob_h / 2.0, knob_w, knob_h);
    cv.rounded(knob, 3.0, Some(ORANGE_EDGE), None);
    let (kx, ky, kw, kh) = knob;
    cv.gradient((kx + 1.0, ky + 1.0, kw - 2.0, kh - 2.0), 2.0, &shade(ORANGE, Touch { pressed: false, ..touch }));
    for dx in [-2.0, 0.0, 2.0] {
        cv.rect((at + dx - 0.5).round(), ky + 5.0, 1.0, kh - 10.0, with_alpha(ORANGE_EDGE, 170));
    }
}

/// The title bar's small close box.
pub fn close_box(cv: &mut Canvas<'_>, (x, y, w, h): Area, touch: Touch) {
    let (fill, edge, ink) = if touch.hover || touch.pressed {
        ([[255, 138, 138, 255], [224, 64, 64, 255]], [150, 30, 30, 255], WHITE)
    } else {
        ([[250, 251, 253, 255], gamewin::BODY], gamewin::FRAME_DARK, [70, 80, 104, 255])
    };
    cv.rounded((x, y, w, h), 3.0, Some(edge), None);
    cv.gradient((x + 1.0, y + 1.0, w - 2.0, h - 2.0), 2.0, &[(0.0, fill[0]), (1.0, fill[1])]);
    let (cx, cy, arm) = (x + w / 2.0, y + h / 2.0, 3.0);
    cv.polyline(&[(cx - arm, cy - arm), (cx + arm, cy + arm)], ink, 1.6);
    cv.polyline(&[(cx - arm, cy + arm), (cx + arm, cy - arm)], ink, 1.6);
}

/// The keyboard focus: a blue ring just outside `area`.
pub fn focus_ring(cv: &mut Canvas<'_>, (x, y, w, h): Area) {
    if let Some(path) = super::canvas::rounded_rect(x - 2.5, y - 2.5, w + 5.0, h + 5.0, 5.0) {
        cv.stroke_path(&path, FOCUS, 1.5);
    }
}
