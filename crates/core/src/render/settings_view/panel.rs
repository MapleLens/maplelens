//! The pairing panel over the settings window: the QR code with the steps to scan it, then "connected" with a test a
//! click away, or "hidden" once the code's two minutes are up. Nothing behind it can be touched while it is up.

use super::{Control, ROW_H, SIZE, View, fit};
use crate::notify::pairing::{Panel, Route};
use crate::render::canvas::{Anchor, Canvas, Ink};
use crate::render::gamewin::{self, INK, INK_DIM, PAD, VALUE_EDGE, VALUE_FILL};
use crate::render::maple_qr;
use crate::render::maple_ui::{self, ACCENT, Tone};
use crate::render::theme::Rgba;

/// A paired phone's colour, in the state dot and the "connected" badge.
pub(super) const GREEN: Rgba = [46, 158, 84, 255];
const WHITE: Rgba = [255, 255, 255, 255];
const LEFT: f32 = PAD + 8.0;
const WIDTH: f32 = SIZE.0 - 2.0 * LEFT;
const TEXT: f32 = 11.5;
const LINE: f32 = 15.0;
/// The code's side: about 4 units a module for the pairing address (version 9, 61 modules with the quiet zone), big
/// enough for a phone held at arm's length from the monitor.
const QR_SIZE: f32 = 236.0;
/// The panels without a code start lower, nearer the middle of the sheet.
const SHORT_DROP: f32 = 70.0;

pub(super) fn draw(cv: &mut Canvas<'_>, view: &View<'_>) {
    let (top, bottom) = (gamewin::PANEL_TOP, SIZE.1 - gamewin::STRIP_H);
    let sheet = (gamewin::BORDER, top, SIZE.0 - 2.0 * gamewin::BORDER, bottom - top);
    cv.rounded(sheet, 3.0, Some(gamewin::PANEL_EDGE), None);
    cv.rect(sheet.0 + 1.0, sheet.1 + 1.0, sheet.2 - 2.0, sheet.3 - 2.0, gamewin::PANEL);
    let heading = match view.phone.panel {
        Panel::Code { .. } => "CONNECT YOUR PHONE",
        Panel::Connected => "PHONE CONNECTED",
        _ => "CODE HIDDEN",
    };
    gamewin::pill(cv, (PAD, top + 10.0, SIZE.0 - 2.0 * PAD, 24.0), heading, 11.0);
    let y = top + 46.0;
    let (back, main) = match view.phone.panel {
        Panel::Code { .. } => {
            code(cv, view, y);
            ("Hide code", "I've scanned it")
        }
        Panel::Connected => {
            connected(cv, view, y + SHORT_DROP);
            let main = if view.phone.busy == Some(Route::Test) { "Sending…" } else { "Send a test" };
            ("Done", main)
        }
        _ => {
            expired(cv, y + SHORT_DROP);
            ("Close", "Show it again")
        }
    };
    maple_ui::button(cv, Control::PanelBack.area(), back, Tone::Grey, view.touch(Control::PanelBack));
    let mut touch = view.touch(Control::PanelMain);
    touch.disabled = view.phone.busy.is_some();
    maple_ui::button(cv, Control::PanelMain.area(), main, Tone::Orange, touch);
}

/// A numbered step: an orange disc and the text wrapped beside it; returns the y below it.
fn step(cv: &mut Canvas<'_>, y: f32, number: u32, text: &str) -> f32 {
    cv.circle(LEFT + 9.0, y + 8.0, 9.0, ACCENT, None);
    cv.text(LEFT + 9.0, y + 12.0, &number.to_string(), Ink::new(11.0, WHITE).bold(), Anchor::MS);
    paragraph(cv, LEFT + 26.0, y, WIDTH - 26.0, text, INK) + 6.0
}

/// Wrapped text whose first line's top is at y; returns the y below it.
fn paragraph(cv: &mut Canvas<'_>, x: f32, y: f32, width: f32, text: &str, colour: Rgba) -> f32 {
    let lines = cv.wrap(text, TEXT, width, false);
    for (i, line) in lines.iter().enumerate() {
        cv.text(x, y + 12.0 + LINE * i as f32, line, Ink::new(TEXT, colour), Anchor::LS);
    }
    y + LINE * lines.len() as f32
}

/// A beige box with a coloured dot and one line: what is happening right now.
fn status(cv: &mut Canvas<'_>, y: f32, dot: Rgba, text: &str) -> f32 {
    let area = (LEFT, y, WIDTH, ROW_H);
    cv.rounded(area, 2.0, Some(VALUE_EDGE), None);
    cv.rounded((area.0 + 1.0, y + 1.0, area.2 - 2.0, ROW_H - 2.0), 2.0, Some(VALUE_FILL), None);
    cv.circle(LEFT + 12.0, y + ROW_H / 2.0, 4.0, dot, None);
    let text = fit(cv, text, TEXT, WIDTH - 30.0);
    cv.text(LEFT + 22.0, y + ROW_H / 2.0 + 4.0, &text, Ink::new(TEXT, INK).bold(), Anchor::LS);
    y + ROW_H
}

fn code(cv: &mut Canvas<'_>, view: &View<'_>, y: f32) {
    let mut y = step(cv, y, 1, "Point your phone's camera at this code");
    if let Some((width, modules)) = &view.phone.qr {
        maple_qr::draw(cv, ((SIZE.0 - QR_SIZE) / 2.0, y + 2.0), QR_SIZE, (*width, modules));
    }
    y += QR_SIZE + 12.0;
    y = step(cv, y, 2, "Open the link, then tap Turn on notifications and allow them");
    y = step(
        cv,
        y,
        3,
        "iPhone: add the page to your home screen first (Share, then Add to Home Screen) and open it there",
    );
    let left = view.phone_left_s.unwrap_or(0);
    let waiting = format!("Waiting for your phone · hides in {}:{:02}", left / 60, left % 60);
    let text = if view.phone.note.is_empty() { waiting.as_str() } else { view.phone.note.as_str() };
    y = status(cv, y + 4.0, ACCENT, text);
    paragraph(cv, LEFT, y + 4.0, WIDTH, "Anyone who scans this code gets your store's notifications.", INK_DIM);
}

fn connected(cv: &mut Canvas<'_>, view: &View<'_>, y: f32) {
    let (cx, cy) = (SIZE.0 / 2.0, y + 34.0);
    cv.circle(cx, cy, 28.0, GREEN, Some(([255, 255, 255, 200], 3.0)));
    cv.polyline(&[(cx - 12.0, cy + 1.0), (cx - 3.0, cy + 10.0), (cx + 13.0, cy - 9.0)], WHITE, 5.0);
    let y = y + 84.0;
    cv.text(cx, y, "Your phone is connected", Ink::new(15.0, INK).bold(), Anchor::MS);
    let mut y = y + 14.0;
    let lines = [
        "It buzzes when an item sells and when your store opens or closes: the NOTIFY boxes choose which.",
        "Its Store tab shows your store live while MapleLens runs: sales, visitors, chat and your balance.",
    ];
    for text in lines {
        y = paragraph(cv, LEFT, y, WIDTH, text, INK_DIM) + 8.0;
    }
    let (dot, text) = match view.phone.note.as_str() {
        "" | "Phone connected" => (GREEN, "Press Send a test to see one arrive"),
        note if note.starts_with("Sent") => (GREEN, note),
        note => (ACCENT, note),
    };
    status(cv, y + 4.0, dot, text);
}

fn expired(cv: &mut Canvas<'_>, y: f32) {
    let (cx, cy) = (SIZE.0 / 2.0, y + 34.0);
    cv.circle(cx, cy, 28.0, gamewin::FRAME_DARK, Some(([255, 255, 255, 200], 3.0)));
    cv.polyline(&[(cx, cy - 15.0), (cx, cy), (cx + 11.0, cy + 7.0)], WHITE, 4.0);
    let y = y + 84.0;
    cv.text(SIZE.0 / 2.0, y, "The code hid itself", Ink::new(15.0, INK).bold(), Anchor::MS);
    let text = "It shows for two minutes at most: anyone who sees it, on a stream or in a screenshot, could connect \
                their phone. No phone connected in that time. Show it again when your phone is ready.";
    paragraph(cv, LEFT, y + 14.0, WIDTH, text, INK_DIM);
}
