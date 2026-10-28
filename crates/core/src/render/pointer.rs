//! The pointer MapleLens draws over its own cards: the game draws its cursor into its own frame, under the overlay, so
//! over a card there would be none. An arrow in the game's manner: white with a dark outline, greyer while pressed.

use super::RgbaImage;
use super::canvas::Canvas;
use super::font::Fonts;
use super::theme;

/// Where the arrow points, in design units from the picture's corner.
pub const TIP: (f32, f32) = (1.5, 1.5);
const SIZE: (f32, f32) = (17.0, 24.0);
const ARROW: [(f32, f32); 7] =
    [(1.5, 1.5), (1.5, 18.5), (5.6, 14.6), (8.6, 21.4), (11.8, 20.0), (8.9, 13.4), (14.4, 13.4)];
const OUTLINE: theme::Rgba = [28, 30, 44, 255];
const OUTLINE_W: f32 = 1.4;

/// The pointer at the cards' size (drawn CARD_SCALE * `ui_scale` times bigger).
pub fn render_pointer(fonts: &Fonts, pressed: bool, ui_scale: f32) -> RgbaImage {
    let mut cv = Canvas::new(fonts, SIZE.0, SIZE.1, theme::CARD_SCALE * ui_scale);
    let fill = if pressed { [206, 210, 226, 255] } else { theme::TEXT };
    cv.polygon(&ARROW, fill);
    // The lower half a shade darker, as the game's arrow is lit from above.
    cv.polygon(&[(1.5, 12.0), (1.5, 18.5), (5.6, 14.6), (7.5, 19.0), (8.9, 13.4), (6.0, 13.4)], [214, 218, 232, 255]);
    let outline: Vec<(f32, f32)> = ARROW.iter().chain(&ARROW[..1]).copied().collect();
    cv.polyline(&outline, OUTLINE, OUTLINE_W);
    RgbaImage::from_canvas(&cv)
}

/// How big the game draws its art against the UI scale: 1:1 at Ideal Ratio (UI scale 0.64), 1.5625x in Normal (1.0);
/// the status bar is 800 px wide at 1x and 1,250 at UI scale 1.
pub const ART_PER_UI_SCALE: f32 = 1250.0 / 800.0;

/// The game's own cursor (maplestory.io serves its frames: `UI/Basic.img/Cursor/<n>/0`), drawn as big as the game
/// draws it. The arrow above stands in until it is on disk.
pub fn render_game_cursor(fonts: &Fonts, image: &RgbaImage, ui_scale: f32) -> RgbaImage {
    let (w, h) = (image.width as f32, image.height as f32);
    let mut cv = Canvas::new(fonts, w, h, ui_scale * ART_PER_UI_SCALE);
    cv.image(0.0, 0.0, w, h, (image.width, image.height, &image.data));
    RgbaImage::from_canvas(&cv)
}

/// A cursor frame's hot spot from maplestory.io's `.../origin` answer: `{"value": {"x": 0, "y": 3}}`.
pub fn parse_origin(json: &str) -> Option<(f32, f32)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let at = value.get("value")?;
    Some((at.get("x")?.as_f64()? as f32, at.get("y")?.as_f64()? as f32))
}

#[cfg(test)]
mod tests {
    use super::parse_origin;

    #[test]
    fn the_hot_spot_is_read_from_the_origin() {
        assert_eq!(parse_origin(r#"{"children":[],"type":9,"value":{"x":0,"y":3,"isEmpty":false}}"#), Some((0.0, 3.0)));
        assert_eq!(parse_origin(r#"{"value":{}}"#), None);
        assert_eq!(parse_origin("<html>"), None);
    }
}
