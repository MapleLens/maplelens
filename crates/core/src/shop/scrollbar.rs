//! Where a dialog's scroll thumb sits, so a list is only trusted while it shows its live end.

use super::layout::View;

/// Enabled scroll parts are blue (B well above R); a disabled bar is grey.
const BLUE: i32 = 40;
/// The thumb rests a few pixels short of its arrow button.
const END_SLACK: usize = 6;
/// The track is looked at this much (reference px) beyond its ends, so the arrow buttons always reach into the strip
/// and are told from the thumb by touching its ends: a track end placed a pixel onto an arrow (a 0.2% scale error in
/// a browser's frame) read the down arrow's blue as the thumb at the start, and the live chat as scrolled back.
const ARROW_REACH: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thumb {
    /// No thumb: the list fits, nothing to scroll.
    None,
    Start,
    Middle,
    End,
}

/// The thumb in reference pixel column `column`, between the [start, end) rows of `track`. A column off the
/// frame reads as Middle: a list we cannot see the bar of is not trusted.
pub fn thumb_position(view: &View<'_>, column: i32, (start, end): (i32, i32)) -> Thumb {
    let start = (start - ARROW_REACH).max(0);
    let strip = view.crop((column, start, 1, end + ARROW_REACH - start));
    if strip.width != 1 || column < 0 {
        return Thumb::Middle;
    }
    let blue = |y: usize| {
        let [r, _, b] = strip.pixel(0, y);
        b as i32 - r as i32 > BLUE
    };
    // Blue runs (start, end exclusive); those touching the strip's ends are the arrow buttons.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for y in (0..strip.height).filter(|&y| blue(y)) {
        match runs.last_mut() {
            Some(run) if run.1 == y => run.1 = y + 1,
            _ => runs.push((y, y + 1)),
        }
    }
    runs.retain(|&(a, b)| a > 0 && b < strip.height);
    let slack = END_SLACK + 2 * ARROW_REACH as usize;
    match (runs.first(), runs.last()) {
        (None, _) | (_, None) => Thumb::None,
        (Some(&(first, _)), _) if first <= slack => Thumb::Start,
        (_, Some(&(_, last))) if last + slack >= strip.height => Thumb::End,
        _ => Thumb::Middle,
    }
}
