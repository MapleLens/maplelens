//! Find the Character Info window and read whose it is.

use super::{Screen, grab_normalised};
use crate::config;
use std::collections::VecDeque;
use std::sync::Arc;

use crate::geometry::{Rect, UiScale, scaled};
use crate::vision::Mask;
use crate::vision::gaps::fill_gaps;
use crate::vision::info_window;
use crate::vision::pixel_font::{PixelFont, small_name_font};
use crate::vision::reader::TextReader;

/// The Character Info window is open on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct InfoWindow {
    /// Whose it is; None while the name plate is unreadable or two reads in a row have not yet agreed.
    pub name: Option<String>,
    /// The name exactly as read (for confirming a read the rank lookup proves right).
    pub read: String,
    /// The window in screen coordinates.
    pub rect: Rect,
    /// Game client area in screen coordinates.
    pub bounds: Rect,
    /// UI size relative to the measured resolution.
    pub scale: f32,
}

pub struct InfoDetector {
    reader: TextReader,
    window: Option<Rect>,
    ui: Arc<UiScale>,
    /// The previous read of the name plate.
    last_read: Option<String>,
    /// The last read two polls in a row agreed on: a window still appearing or being dragged reads as junk for a
    /// poll ("Innut" between two "Cubix"), and each junk name would cost a rank lookup and flash its answer.
    settled: Option<String>,
    /// The small UI's name font, read exactly (`vision::pixel_font`).
    pixel_font: PixelFont,
    /// Recent small-UI reads and their plates, so a name the rank lookup confirms can teach its letters.
    recent_plates: VecDeque<(String, Mask)>,
    pixel_learnt: bool,
}

impl InfoDetector {
    pub fn new(reader: TextReader) -> Self {
        Self {
            reader,
            window: None,
            ui: Arc::default(),
            last_read: None,
            settled: None,
            pixel_font: small_name_font(),
            recent_plates: VecDeque::new(),
            pixel_learnt: false,
        }
    }

    /// Read the small UI's names with this font (the bundled one merged with letters learnt here).
    pub fn with_pixel_font(mut self, font: PixelFont) -> Self {
        self.pixel_font = font;
        self
    }

    pub fn pixel_font(&self) -> &PixelFont {
        &self.pixel_font
    }

    /// The rank lookup found `read` to be `truth`: learn the letters it shows.
    pub fn confirm(&mut self, read: &str, truth: &str) {
        if let Some((_, plate)) = self.recent_plates.iter().find(|(r, _)| r == read)
            && self.pixel_font.learn(plate, truth) > 0
        {
            self.pixel_learnt = true;
        }
        self.reader.confirm(read, truth);
    }

    /// True once since the pixel font last learnt something (the caller saves it then).
    pub fn take_pixel_learnt(&mut self) -> bool {
        std::mem::take(&mut self.pixel_learnt)
    }

    /// Read the UI scale the `HoverDetector` measures (see `HoverDetector::ui_scale`).
    pub fn with_ui_scale(mut self, ui: Arc<UiScale>) -> Self {
        self.ui = ui;
        self
    }

    pub fn reader_mut(&mut self) -> &mut TextReader {
        &mut self.reader
    }

    /// Forget the window (the game lost focus or closed; it may come back elsewhere).
    pub fn reset(&mut self) {
        self.window = None;
        (self.last_read, self.settled) = (None, None);
    }

    /// One pass while the game is in front. Searching the whole game is the expensive part, so it only runs
    /// while the window is not known; once found it is re-checked in place, which is a tiny grab.
    pub fn step(&mut self, screen: &mut dyn Screen, bounds: Rect) -> Option<InfoWindow> {
        let scale = self.ui.of(bounds);
        self.step_at_scale(screen, bounds, scale)
    }

    /// `step` with the game's UI scale given rather than derived from the client height.
    pub fn step_at_scale(&mut self, screen: &mut dyn Screen, bounds: Rect, scale: f32) -> Option<InfoWindow> {
        if let Some(window) = self.window
            && !still_there(screen, window, scale)
        {
            self.window = None;
        }
        if self.window.is_none() {
            (self.last_read, self.settled) = (None, None);
            self.window = search(screen, bounds, scale);
        }
        let window = self.window?;
        let area = info_window::name_box(window, scale);
        let read = if scale < PIXEL_NAME_BELOW {
            self.read_pixel_name(&screen.grab(area))
        } else {
            self.reader.read_mask(&info_window::name_mask(&grab_normalised(screen, area, scale)))
        };
        if self.last_read.as_deref() == Some(read.as_str()) {
            self.settled = Some(read.clone());
        }
        self.last_read = Some(read);
        let read = self.settled.clone().unwrap_or_default();
        Some(InfoWindow { name: info_window::clean_name(&read), read, rect: window, bounds, scale })
    }
}

impl InfoDetector {
    /// The small UI's name: exactly from the pixel font, OCR only for the pieces it does not know yet. The 1x
    /// atlas never sees this font: on it, it finds only look-alikes, and letters learnt from it misread every later
    /// name (22 names: 20 right with OCR alone, 9 with letters learnt that way).
    fn read_pixel_name(&mut self, plate: &crate::image::RgbImage) -> String {
        let mask = info_window::name_mask(plate);
        let (exact, unknown) = self.pixel_font.read(&mask);
        let read = if unknown == 0 {
            exact
        } else {
            let enlarged = info_window::name_mask(&plate.upscaled_nearest(PIXEL_NAME_BLOCKS));
            let ocr = self.reader.ocr_mask(&enlarged).unwrap_or_default();
            fill_gaps(&exact, &ocr)
        };
        if self.recent_plates.len() >= RECENT_PLATES {
            self.recent_plates.pop_front();
        }
        self.recent_plates.push_back((read.clone(), mask));
        read
    }
}

/// Plates remembered for a rank lookup to confirm: lookups are paced, so a few windows may queue up.
const RECENT_PLATES: usize = 16;

/// Below this UI scale the game draws names in a crisp pixel font with 1 px strokes, which smooth resampling
/// smears: at Ideal Ratio "xXFishXx" read as "u*FishKK".
const PIXEL_NAME_BELOW: f32 = 0.9;

/// The pixel font is enlarged in whole blocks for OCR. Measured on 22 names at Ideal Ratio: 3x reads 20, 4x 16,
/// 5x 20 with one more wrong, 6x 17; 2x reads nothing.
const PIXEL_NAME_BLOCKS: usize = 3;

fn still_there(screen: &mut dyn Screen, window: Rect, scale: f32) -> bool {
    let title = screen.grab(info_window::title_box(window, scale));
    info_window::is_window(&title.to_gray(), scale)
}

fn search(screen: &mut dyn Screen, bounds: Rect, scale: f32) -> Option<Rect> {
    let gray = screen.grab(bounds).to_gray();
    let (x, y) = info_window::find_window(&gray, scale)?;
    let (w, h) = config::INFO_WINDOW_SIZE;
    Some(Rect::new(bounds.x + x, bounds.y + y, scaled(w, scale), scaled(h, scale)))
}

#[cfg(test)]
mod tests {
    use super::fill_gaps;

    #[test]
    fn ocr_counts_only_where_it_agrees_with_the_letters_the_font_is_sure_of() {
        // "j" for "i" in HHai: the font knows "HHa" and the "i"; OCR's reading does not fit.
        assert_eq!(fill_gaps("HHa?", "HHaj"), "HHaj", "one gap, one letter: OCR fills it");
        assert_eq!(fill_gaps("?ande?", "Xanderz"), "Xanderz", "gaps of touching letters");
        assert_eq!(fill_gaps("Che?esChef", "ChefYesChef"), "ChefYesChef");
        // OCR misread a letter the font is sure of: keep the font's letters, OCR's only in the gap.
        assert_eq!(fill_gaps("Sha?kChili", "SharkChiIi"), "SharkChili");
        assert_eq!(fill_gaps("?", "Cubix"), "Cubix");
    }
}
