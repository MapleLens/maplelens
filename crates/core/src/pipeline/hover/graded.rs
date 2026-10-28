//! A scroll whose grade the tooltip's edge cut off its title ("Two-handed Blunt Weapon Attack Scr"): its description
//! says the grade (`catalog::Grade`), read line by line.

use std::sync::Arc;

use super::HoverDetector;
use crate::geometry::{Point, Rect, scaled};
use crate::market::catalog::Grade;
use crate::market::models::Item;
use crate::pipeline::{Screen, normalised};
use crate::shop::text::read_pixels;
use crate::vision::Mask;

/// The description's lines, from a tooltip's corner at UI scale 1 (measured: text from x 139, line tops at rows 60,
/// 85, ... 185, the last two a Chaos scroll's warning), each cut a little above and below, short of the right border.
const LEFT: i32 = 130;
const RIGHT: i32 = 445;
const FIRST_LINE: i32 = 57;
const LINE_PITCH: i32 = 25;
const LINE_H: i32 = 24;
const LINES: i32 = 6;
/// A line whose brightest pixel stands this little above its median holds no text.
const MIN_CONTRAST: u16 = 60;

impl HoverDetector {
    /// The item of a cut graded scroll's title `read`, its tooltip's top-left corner at `corner` (screen pixels).
    pub(super) fn graded(
        &mut self,
        screen: &mut dyn Screen,
        corner: Point,
        bounds: Rect,
        scale: f32,
        read: &str,
    ) -> Option<Arc<Item>> {
        let grades = self.catalog.grades(read)?;
        let (x0, x1) = (corner.x + scaled(LEFT, scale), (corner.x + scaled(RIGHT, scale)).min(bounds.right()));
        if x0 < bounds.x || x1 <= x0 {
            return None;
        }
        // Read twice, by the shop's fonts (the same UI font) and by OCR on each line: either may lose the rate's
        // digits; two grades that disagree name none.
        let (mut by_font, mut by_ocr) = (String::new(), String::new());
        for k in 0..LINES {
            let y0 = corner.y + scaled(FIRST_LINE + k * LINE_PITCH, scale);
            let y1 = (y0 + scaled(LINE_H, scale)).min(bounds.bottom());
            if y0 < bounds.y || y1 <= y0 {
                break;
            }
            let native = screen.grab(Rect::new(x0, y0, x1 - x0, y1 - y0));
            if let Some(read) = read_pixels(&native, scale, true) {
                by_font.push_str(&read.text);
                by_font.push(' ');
            }
            let line = normalised(&native, scale).to_gray();
            // Light text on the tooltip's fill, whatever shows through it: cut halfway between the line's own paper
            // (its median) and ink (its brightest), not at a fixed level that loses this thin font's strokes.
            let mut sorted = line.data.clone();
            sorted.sort_unstable();
            let (paper, ink) = (sorted[sorted.len() / 2] as u16, *sorted.last().unwrap_or(&0) as u16);
            if ink < paper + MIN_CONTRAST {
                continue;
            }
            let cut = ((paper + ink) / 2) as u8;
            let mask =
                Mask { width: line.width, height: line.height, bits: line.data.iter().map(|&v| v > cut).collect() };
            self.reader.allow_ocr(true);
            if let Some(read) = self.reader.ocr_mask(&mask) {
                by_ocr.push_str(&read);
                by_ocr.push(' ');
            }
        }
        let grade = match (Grade::from_description(&by_font), Grade::from_description(&by_ocr)) {
            (Some(a), Some(b)) if a != b => return None,
            (a, b) => a.or(b)?,
        };
        grades.into_iter().find(|(g, _)| *g == grade).map(|(_, item)| item)
    }
}
