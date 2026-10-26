//! Find the game's Character Info window by its title bar, and cut out the player's name plate (pure).

use std::sync::OnceLock;

use crate::config;
use crate::geometry::{Rect, scaled};
use crate::image::{GrayImage, RgbImage, decode_png};
use crate::vision::ncc::{Template, matches};

const TITLE_PNG: &[u8] = include_bytes!("../../assets/character_info_title.png");
/// The title text as the game draws it in "Ideal Ratio" (UI scale 0.64): the small UI has its own lettering, which
/// the 1x title shrunk to that size matches at only 0.79.
const TITLE_IDEAL_RATIO_PNG: &[u8] = include_bytes!("../../assets/character_info_title_ideal_ratio.png");
/// The right of the title bar and the tab row in "Ideal Ratio", what the search looks for there. The 81x11 title
/// is too small for the search pyramid to shrink: searching the game for it took 110-230 ms, four times a second
/// while no window is open (most of a CPU core). This is found in under 60 ms and is the same on every window; the
/// left of the title bar is not (a tall hat or a balloon covers it). Other windows' tabs look alike (up to 0.79),
/// so a hit must also show the title text.
const BAR_IDEAL_RATIO_PNG: &[u8] = include_bytes!("../../assets/character_info_bar_ideal_ratio.png");
/// The title text is dark on white, so a good match scores ~1.0 (0.91 at 1080p, where the template is resampled
/// to the game's size); other UI never gets near this (best seen: 0.56).
const MATCH_MIN: f32 = 0.85;
/// The search pyramid's coarsest template: rows of 4 still tell the tabs from other UI (measured on six frames:
/// 3 rows lost the window in a busy one).
const SEARCH_COARSEST: (usize, usize) = (24, 4);
/// Search hits worth checking for the title text.
const SEARCH_HITS: usize = 4;
/// Grey levels under/over which a title pixel is text (dark) or paper (white).
const INK_MAX: u8 = 110;
const PAPER_MIN: u8 = 200;
/// A patch must have this share of the template's ink and paper: a flat sky or cloud scored 0.85+ against the
/// title on compressed 1080p stream frames, because normalised correlation ignores how little contrast is there.
const INK_PAPER_SHARE: f32 = 0.5;

/// Dark and bright pixel shares of the `width` x `height` patch of `gray` at (x, y).
fn ink_paper(gray: &GrayImage, x: usize, y: usize, width: usize, height: usize) -> (f32, f32) {
    let (mut ink, mut paper) = (0usize, 0usize);
    for row in 0..height {
        for &v in &gray.data[(y + row) * gray.width + x..][..width] {
            ink += (v <= INK_MAX) as usize;
            paper += (v >= PAPER_MIN) as usize;
        }
    }
    let n = (width * height).max(1) as f32;
    (ink as f32 / n, paper as f32 / n)
}

/// Does the patch at (x, y) show as much ink and paper as the title does?
fn has_title_contrast(gray: &GrayImage, title: &GrayImage, x: usize, y: usize) -> bool {
    let (want_ink, want_paper) = ink_paper(title, 0, 0, title.width, title.height);
    let (ink, paper) = ink_paper(gray, x, y, title.width, title.height);
    ink >= want_ink * INK_PAPER_SHARE && paper >= want_paper * INK_PAPER_SHARE
}

/// The name is white on a mid-grey plate (<= 140).
const NAME_TEXT_MIN: u8 = 200;

/// A capture and where its top-left sits in the window, at the UI scale it was taken at.
struct Part {
    image: GrayImage,
    offset: (i32, i32),
}

/// Bundled captures of the window at one UI scale: the title text (which proves it is this window) and what the
/// search looks for.
struct Captures {
    at: f32,
    title: Part,
    search: Part,
}

fn captures() -> &'static [Captures; 2] {
    static CAPTURES: OnceLock<[Captures; 2]> = OnceLock::new();
    CAPTURES.get_or_init(|| {
        let load = |png| decode_png(png).expect("bundled window capture is a valid PNG").to_gray();
        let title = Part { image: load(TITLE_PNG), offset: config::INFO_TITLE_OFFSET };
        let search = Part { image: load(TITLE_PNG), offset: config::INFO_TITLE_OFFSET };
        let ideal_title = Part { image: load(TITLE_IDEAL_RATIO_PNG), offset: (6, 6) };
        let ideal_search = Part { image: load(BAR_IDEAL_RATIO_PNG), offset: (125, 4) };
        [Captures { at: 1.0, title, search }, Captures { at: 0.64, title: ideal_title, search: ideal_search }]
    })
}

/// A part as the game would draw it at `scale` (see `geometry::ui_scale`), from the captures taken nearest that
/// scale, and where it sits in the window. Only those are used: the others do not match well and double the cost.
fn part(scale: f32, which: impl Fn(&Captures) -> &Part) -> (GrayImage, (i32, i32)) {
    let nearest = |at: f32| (scale / at).ln().abs();
    let set = captures().iter().min_by(|a, b| nearest(a.at).total_cmp(&nearest(b.at))).expect("two capture sets");
    let (part, factor) = (which(set), scale / set.at);
    let image = if (factor - 1.0).abs() > 0.01 { part.image.resized(factor) } else { part.image.clone() };
    let offset = ((part.offset.0 as f32 * factor).round() as i32, (part.offset.1 as f32 * factor).round() as i32);
    (image, offset)
}

/// Screen rectangle of the title text of a window whose top-left is `window`.
pub fn title_box(window: Rect, scale: f32) -> Rect {
    let (t, (dx, dy)) = part(scale, |c| &c.title);
    Rect::new(window.x + dx, window.y + dy, t.width as i32, t.height as i32)
}

/// Top-left of the Character Info window inside `gray` (a capture of the game), or None.
pub fn find_window(gray: &GrayImage, scale: f32) -> Option<(i32, i32)> {
    let (search, (sx, sy)) = part(scale, |c| &c.search);
    let (title, (tx, ty)) = part(scale, |c| &c.title);
    let template = Template::new(&title);
    let title_image = title;
    matches(gray, &search, MATCH_MIN, SEARCH_HITS, SEARCH_COARSEST).into_iter().find_map(|(_, x, y)| {
        let (wx, wy) = (x as i32 - sx, y as i32 - sy);
        let (ux, uy) = (wx + tx, wy + ty);
        let fits = ux >= 0
            && uy >= 0
            && ux as usize + template.width <= gray.width
            && uy as usize + template.height <= gray.height;
        (fits
            && template.score_at(gray, ux as usize, uy as usize) >= MATCH_MIN
            && has_title_contrast(gray, &title_image, ux as usize, uy as usize))
        .then_some((wx, wy))
    })
}

/// Does a capture of exactly the title area still show the title (i.e. the window has not moved)?
pub fn is_window(title_gray: &GrayImage, scale: f32) -> bool {
    let (template, _) = part(scale, |c| &c.title);
    (title_gray.width, title_gray.height) == (template.width, template.height)
        && Template::new(&template).score_at(title_gray, 0, 0) >= MATCH_MIN
        && has_title_contrast(title_gray, &template, 0, 0)
}

pub fn name_box(window: Rect, scale: f32) -> Rect {
    let (x, y, w, h) = config::INFO_NAME_BOX;
    Rect::new(window.x + scaled(x, scale), window.y + scaled(y, scale), scaled(w, scale), scaled(h, scale))
}

/// Glyph mask (true = text) of a capture of the name plate.
pub fn name_mask(plate: &RgbImage) -> crate::vision::Mask {
    let gray = plate.to_gray();
    crate::vision::Mask::threshold(&gray, NAME_TEXT_MIN)
}

/// The read text as a character name, or None if it cannot be one (MapleStory names are 2-13 letters/digits).
pub fn clean_name(text: &str) -> Option<String> {
    let squashed: String = text.chars().filter(|c| *c != ' ').collect();
    let valid = (2..=13).contains(&squashed.chars().count()) && squashed.chars().all(|c| c.is_ascii_alphanumeric());
    valid.then_some(squashed)
}
