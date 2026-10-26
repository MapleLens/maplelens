//! The small status pill shown at the bottom-left of the game while in our own store.

use super::RgbaImage;
use super::canvas::{Anchor, Canvas, Ink};
use super::font::Fonts;
use super::theme::{self, Rgba};
use crate::geometry::Rect;
use crate::shop::ShopStatus;

const BRAND: &str = "MapleLens";
const HEIGHT: f32 = 24.0;
const PAD: f32 = 9.0;
const ICON: f32 = 12.0;
const SIZE: f32 = 11.0;
const GAP: f32 = 6.0;
/// From the client's bottom-left corner: clear of the status HUD that starts ~340 px in.
const MARGIN: (f32, f32) = (10.0, 8.0);
const GREY: Rgba = [170, 176, 205, 255];

#[derive(Clone, Copy)]
enum Icon {
    Check,
    Warn,
    Dot,
}

/// status -> (icon, colour, text). The icon is drawn, not typed: Tahoma has no reliable check or warning glyphs.
fn look(status: ShopStatus) -> Option<(Icon, Rgba, &'static str)> {
    Some(match status {
        ShopStatus::Hidden => return None,
        ShopStatus::Watching => (Icon::Check, theme::UP, "Store detected"),
        ShopStatus::NeedSoldWindow => (Icon::Warn, theme::WARN, "Open the Sold Items window"),
        ShopStatus::Scrolled => (Icon::Warn, theme::WARN, "Scroll the lists back to the newest"),
        ShopStatus::Covered => (Icon::Warn, theme::WARN, "Shop window is partly covered"),
        ShopStatus::NotOpen => (Icon::Dot, GREY, "Store not open"),
    })
}

pub fn text_for(status: ShopStatus) -> Option<&'static str> {
    look(status).map(|(_, _, text)| text)
}

/// How the status reads at a glance, for a status line that is not drawn like the pill (the browser's): "ok" (all
/// detected), "warn" (something to do) or "idle" (nothing wrong, nothing to watch); None when there is nothing to say.
pub fn tone_for(status: ShopStatus) -> Option<&'static str> {
    look(status).map(|(icon, ..)| match icon {
        Icon::Check => "ok",
        Icon::Warn => "warn",
        Icon::Dot => "idle",
    })
}

fn icon(cv: &mut Canvas<'_>, kind: Icon, x: f32, y: f32, colour: Rgba) {
    match kind {
        Icon::Check => cv.polyline(&[(x + 2.0, y + 6.0), (x + 5.0, y + 9.0), (x + 10.0, y + 3.0)], colour, 2.0),
        Icon::Warn => {
            cv.polyline(
                &[(x + 6.0, y + 1.0), (x + 11.0, y + 10.0), (x + 1.0, y + 10.0), (x + 6.0, y + 1.0)],
                colour,
                1.0,
            );
            cv.rect(x + 5.5, y + 4.0, 1.0, 3.0, colour);
            cv.rect(x + 5.5, y + 8.0, 1.0, 1.0, colour);
        }
        Icon::Dot => cv.circle(x + 6.0, y + 6.0, 3.0, colour, None),
    }
}

/// The pill for `status` at the game's UI scale, or None when there is nothing to show.
pub fn render_status(fonts: &Fonts, status: ShopStatus, ui_scale: f32) -> Option<RgbaImage> {
    let (kind, colour, text) = look(status)?;
    let probe = Canvas::new(fonts, 1.0, 1.0, 1.0);
    let width =
        PAD + ICON + GAP + probe.measure(text, SIZE, true) + GAP + probe.measure(BRAND, SIZE - 1.0, false) + PAD;
    let mut cv = Canvas::new(fonts, width, HEIGHT, ui_scale);
    cv.rounded(
        (0.5, 0.5, width - 1.0, HEIGHT - 1.0),
        HEIGHT / 2.0,
        Some(theme::PANEL_FILL),
        Some(theme::with_alpha(colour, 170)),
    );
    icon(&mut cv, kind, PAD, (HEIGHT - ICON) / 2.0, colour);
    let x = PAD + ICON + GAP;
    let x = x + cv.text(x, HEIGHT / 2.0, text, Ink::new(SIZE, colour).bold(), Anchor::LM) + GAP;
    cv.text(x, HEIGHT / 2.0, BRAND, Ink::new(SIZE - 1.0, theme::TEXT_FAINT), Anchor::LM);
    Some(RgbaImage::from_canvas(&cv))
}

/// Top-left of a pill of `size` at the bottom-left corner of the game's client area.
pub fn place_status(client: Rect, size: (i32, i32), ui_scale: f32) -> (i32, i32) {
    let (mx, my) = ((MARGIN.0 * ui_scale).round() as i32, (MARGIN.1 * ui_scale).round() as i32);
    (client.x + mx, client.y + client.h - size.1 - my)
}
