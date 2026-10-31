//! Tiny geometry primitives shared by capture, vision and overlay.

use crate::scale_vote::ScaleVote;

/// Axis-aligned rectangle in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// Is `point` on one of the rectangle's pixels (its right and bottom edges are outside)?
    pub const fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }
}

/// A point in screen pixels (the mouse, a window corner).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Every pixel constant in `config` was measured at this client height (1920x1200). The game draws its UI
/// proportionally to the window height (measured at 2560x1440: equipment tooltip 442 px = 370 * 1.2, compare
/// tooltip offset 442 = 373 * 1.2), so other resolutions scale those numbers by height / this.
pub const REFERENCE_HEIGHT: i32 = 1200;

/// How much bigger (or smaller) than at the measured resolution the game draws its UI in `client`.
pub fn ui_scale(client: Rect) -> f32 {
    client.h as f32 / REFERENCE_HEIGHT as f32
}

/// MapleLens's cards are drawn this much bigger than the game's UI where "UI Size: Ideal Ratio" keeps that UI small:
/// at 1:1 on a 1200 px tall screen their text was too small to read at a glance (the player asked for 1.25x).
pub const IDEAL_RATIO_CARDS: f32 = 1.25;
/// Ideal Ratio is told by the UI drawn at most this share of what the window's height would draw (`ui_scale`, as
/// Default Ratio does): 0.64 against 1.0 at 1200p, 0.9 at 1080p. Where both draw alike (768p, 0.64) or Ideal Ratio
/// draws bigger (its 2x at 1440p, 1.28 against 1.2), the UI is not small and the cards keep its size.
const SMALL_UI: f32 = 0.9;

/// The UI scale MapleLens draws its cards at over a game drawing its UI at `scale` in `client`.
pub fn card_scale(client: Rect, scale: f32) -> f32 {
    if scale < ui_scale(client) * SMALL_UI { scale * IDEAL_RATIO_CARDS } else { scale }
}

/// A capture of the same game window can be this much shorter: the browser guesses the height of the window's title
/// bar (0, 31, 39 or 47 px) and cycles through those guesses while nothing is detected. A real resize changes more,
/// or changes the width.
const SAME_CLIENT_SLACK_PX: i32 = 48;

/// The UI scale the game draws at, measured off its status bar (`vision::hud`) by the detectors and read by all.
///
/// The window height alone is wrong in "UI Size: Ideal Ratio", where the UI stays the same size at any window
/// size, so `ui_scale` is only the fallback until the bar's readings have settled for this client (or while it is
/// hidden). The readings are voted over a few seconds (`ScaleVote`): one frame never decides.
#[derive(Debug, Default)]
pub struct UiScale {
    measured: std::sync::Mutex<Measured>,
}

#[derive(Debug, Default)]
struct Measured {
    /// The client the readings were taken on.
    client: Option<(i32, i32)>,
    vote: ScaleVote,
}

fn same_client(a: (i32, i32), b: (i32, i32)) -> bool {
    a.0 == b.0 && (a.1 - b.1).abs() <= SAME_CLIENT_SLACK_PX
}

impl UiScale {
    pub fn of(&self, client: Rect) -> f32 {
        let measured = self.measured.lock().unwrap_or_else(|e| e.into_inner());
        match (measured.client, measured.vote.settled()) {
            (Some(size), Some(scale)) if same_client(size, (client.w, client.h)) => scale,
            _ => ui_scale(client),
        }
    }

    /// A reading of the bar on `client` at `now` (seconds on a clock every caller shares).
    pub fn set(&self, client: Rect, scale: f32, now: f64) {
        let mut measured = self.measured.lock().unwrap_or_else(|e| e.into_inner());
        let size = (client.w, client.h);
        if !measured.client.is_some_and(|c| same_client(c, size)) {
            *measured = Measured { client: Some(size), vote: ScaleVote::default() };
        }
        measured.vote.push(scale, now);
    }
}

/// A measured pixel constant at UI scale `scale`, rounded to the nearest pixel.
pub fn scaled(value: i32, scale: f32) -> i32 {
    (value as f32 * scale).round() as i32
}
