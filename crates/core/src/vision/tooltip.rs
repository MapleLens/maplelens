//! Is an item tooltip showing at the spot where the game draws it, and how wide is it? (pure)
//!
//! The game always draws the tooltip's top-left at a fixed offset from the mouse (`config::TOOLTIP_OFFSET`),
//! so nothing is searched for. The tooltip fill is slate blue (about RGB 75,78,127); between the title and
//! the description of every tooltip there is a band of pure fill, which is what we look for.

use crate::image::RgbImage;

const BLUE_OVER_RED: (i16, i16) = (35, 75);
const GREEN_OVER_RED_MAX: i16 = 12;
const BLUE_RANGE: (i16, i16) = (90, 160);
/// The fill is translucent: over a dark map it is dark and keeps some of the backdrop's hue, (18,19,60) over a castle
/// wall and (34,21,60) over brown brick (seen on 1080p streams), where the standard test above finds nothing. Blue
/// still leads every other channel by a good margin there.
const DARK_BLUE_MAX: i16 = 100;
const DARK_BLUE_MIN: i16 = 50;
const DARK_BLUE_LEAD: i16 = 22;
/// Over a warm backdrop (Perion's orange rock, and smoke drifting over it) the fill turns a dull purple, (78,65,93),
/// (55,36,68) and (83,77,116) on a live capture at Ideal Ratio: blue leads red by only 10-35 and red rises over green,
/// which neither test above takes, so the browser's search found no tooltip there at all.
const WARM_BLUE: (i16, i16) = (60, 125);
/// Blue's lead over red there; a far bigger lead is a saturated blue (scenery or another window), not this blend.
const WARM_LEAD: (i16, i16) = (8, 40);
const WARM_GREEN_MIN: i16 = 30;
const BAND_ROWS: (usize, usize) = (34, 42);
const BAND_COLS: (usize, usize) = (150, 300);
/// Low on purpose: fill blended over bright windows drifts out of the colour range.
const BAND_MIN_FILL: f32 = 0.5;

/// Does `image` (starting at the expected tooltip top-left corner) show the tooltip's fill band?
pub fn tooltip_present(image: &RgbImage) -> bool {
    let band = image.crop(BAND_COLS.0, BAND_ROWS.0, BAND_COLS.1, BAND_ROWS.1);
    if band.is_empty() {
        return false;
    }
    let fill = band.data.as_chunks::<3>().0.iter().filter(|p| is_fill(p[0], p[1], p[2])).count();
    fill as f32 / (band.width * band.height) as f32 >= BAND_MIN_FILL
}

#[inline]
pub(crate) fn is_fill(r: u8, g: u8, b: u8) -> bool {
    let (r, g, b) = (r as i16, g as i16, b as i16);
    let blue_lift = b - r;
    let dark = (DARK_BLUE_MIN..=DARK_BLUE_MAX).contains(&b) && b - r.max(g) >= DARK_BLUE_LEAD;
    let warm = (WARM_BLUE.0..=WARM_BLUE.1).contains(&b)
        && (WARM_LEAD.0..=WARM_LEAD.1).contains(&blue_lift)
        && r >= g
        && g >= WARM_GREEN_MIN;
    let standard = blue_lift > BLUE_OVER_RED.0
        && blue_lift < BLUE_OVER_RED.1
        && (g - r).abs() < GREEN_OVER_RED_MAX
        && b > BLUE_RANGE.0
        && b < BLUE_RANGE.1;
    dark || warm || standard
}

// -- width ---------------------------------------------------------------------------------------------------
/// Below the title, above where the shortest tooltip ends.
const LIFT_ROWS: (usize, usize) = (14, 118);
/// Compare each column with the one this far to its left (inside the tooltip).
const LIFT_INSIDE_PX: usize = 5;
/// And with the one this far in, past the border's own blur.
const LIFT_NEAR_PX: usize = 2;
/// A border pixel is at least this much brighter than the fill next to it.
const LIFT_MIN: i16 = 30;
/// Where to look for the border around a candidate width.
const EDGE_WINDOW_PX: (i32, i32) = (-8, 6);
/// Share of rows with a bright column: the true border runs the full height.
const MIN_EDGE_SCORE: f32 = 0.5;
/// The winning width must beat the other by this much, else we are not sure.
const MIN_MARGIN: f32 = 0.2;

/// How strongly a vertical border line stands at about `width` px from the tooltip's left edge (0..1), by the darkest
/// channel.
fn edge_score(image: &RgbImage, width: i32) -> f32 {
    edge_score_by(image, width, |[r, g, b]| r.min(g).min(b) as i16)
}

/// The same, by the channel `level` picks.
fn edge_score_by(image: &RgbImage, width: i32, level: impl Fn([u8; 3]) -> i16) -> f32 {
    let (y0, y1) = (LIFT_ROWS.0.min(image.height), LIFT_ROWS.1.min(image.height));
    let rows = y1 - y0;
    if rows == 0 {
        return 0.0;
    }
    // Darkest channel: the border is light grey, the fill is blue, so this separates them on almost any backdrop.
    let darkest = |x: usize, y: usize| -> i16 { level(image.pixel(x, y)) };
    let mut best = 0.0f32;
    for x in (width + EDGE_WINDOW_PX.0)..(width + EDGE_WINDOW_PX.1) {
        if x < LIFT_INSIDE_PX as i32 || x as usize >= image.width {
            continue;
        }
        let x = x as usize;
        // Against the darker of two pixels inside: the fill is translucent, so where the scenery behind it changes
        // just inside the border (a character's hair, a window's edge), the one further in can be lighter than the
        // border itself.
        let inside = |y: usize| darkest(x - LIFT_INSIDE_PX, y).min(darkest(x - LIFT_NEAR_PX, y));
        let lifted = (y0..y1).filter(|&y| darkest(x, y) - inside(y) >= LIFT_MIN).count();
        best = best.max(lifted as f32 / rows as f32);
    }
    best
}

/// Is the whole strip where the right border of each of `widths` would be too dark to show one (a tooltip over a
/// black area)? Then `measure_width` has nothing to read, and not finding a border is no evidence against it.
pub fn right_edge_unreadable(image: &RgbImage, widths: &[i32]) -> bool {
    let (y0, y1) = (LIFT_ROWS.0.min(image.height), LIFT_ROWS.1.min(image.height));
    let dark = |x: usize, y: usize| image.pixel(x, y).into_iter().min().is_some_and(|v| (v as i16) < UNREADABLE_EDGE);
    // A width whose border would lie beyond the image (a tooltip the game clamped to the frame's right edge, its right
    // border on the last column or past it) has no strip to read and says nothing; at least one width must.
    let strips: Vec<_> = widths
        .iter()
        .map(|&w| ((w + EDGE_WINDOW_PX.0).max(0) as usize)..((w + EDGE_WINDOW_PX.1).max(0) as usize).min(image.width))
        .filter(|xs| !xs.is_empty())
        .collect();
    !strips.is_empty()
        && strips.into_iter().all(|xs| {
            let (mut dark_px, mut total) = (0usize, 0usize);
            for x in xs {
                for y in y0..y1 {
                    total += 1;
                    dark_px += dark(x, y) as usize;
                }
            }
            total > 0 && dark_px as f32 >= UNREADABLE_SHARE * total as f32
        })
}

/// Darkest channel under which a border cannot be told from the fill beside it, and the share of the strip that
/// must be that dark.
const UNREADABLE_EDGE: i16 = 60;
const UNREADABLE_SHARE: f32 = 0.7;

/// Which of `widths` has a right border by its brightest channel, the way `measure_width` tells them apart. Over a warm
/// backdrop the translucent border takes the backdrop's colour, light in red and as dark as the fill in blue
/// ((169,100,49) beside (58,41,73) over Perion's rock): the darkest channel shows no line there, the brightest does.
pub fn measure_width_lit(image: &RgbImage, widths: &[i32]) -> Option<i32> {
    let scores = widths.iter().map(|&w| (edge_score_by(image, w, |[r, g, b]| r.max(g).max(b) as i16), w));
    pick_width(scores.collect())
}

/// Which of the game's known tooltip `widths` this tooltip has, or None when it cannot be told.
///
/// Tooltips are only ever a few fixed widths (narrow for equipment, wide for everything else). Finding the
/// border anywhere on a busy backdrop is unreliable, but asking "is the border at 370 or at 450?" is not:
/// the right one scores 0.55-1.0 on real captures, the wrong one at most 0.34.
pub fn measure_width(image: &RgbImage, widths: &[i32]) -> Option<i32> {
    pick_width(widths.iter().map(|&w| (edge_score(image, w), w)).collect())
}

/// The width whose border scores best, if it is plainly there and plainly beats the other.
fn pick_width(mut scores: Vec<(f32, i32)>) -> Option<i32> {
    // Highest score first; on a tie the larger width wins, as Python's reverse tuple sort did.
    scores.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    let (best, width) = *scores.first()?;
    let runner_up = scores.get(1).map_or(0.0, |s| s.0);
    (best >= MIN_EDGE_SCORE && best - runner_up >= MIN_MARGIN).then_some(width)
}
