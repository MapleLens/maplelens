//! Where the Store Permit's parts sit on screen.
//!
//! Everything is measured at UI scale 1 (a 1920x1200 client) and stored as offsets from two anchors, because the
//! shop and its Sold Items dialog can each be dragged: the chat's ENTER button and the close box of the Sold Items
//! dialog. Both are placed from the colour markers of `markers`, which also give the scale the UI is drawn at; a
//! `View` then reads any part of the frame in reference pixels, resampling only that part.

use std::sync::OnceLock;

use super::markers::{self, ShopMarker};
use crate::image::{GrayImage, RgbImage, decode_png};

pub type Point = (i32, i32);

/// A box in reference pixels: x, y, width, height.
pub type Area = (i32, i32, i32, i32);

/// The ENTER anchor from the SOLD ITEMS button's top-left, and the Sold Items anchor from its close box's top-left.
const ENTER_FROM_BUTTON: Point = (488, 504);
const SOLD_FROM_BOX: Point = (-2, -2);
/// The chat page, white between blue-tinted borders: where its rows are sampled (from the ENTER anchor), and its
/// width between the borders at UI scale 1. It is the longest straight span in the window, so it measures the
/// scale ~4x finer than the button's width.
const PAGE_ROWS: (i32, i32) = (-330, -40);
const PAGE_PROBE_X: i32 = -137;
const PAGE_W: f32 = 402.5;
/// A page edge measured on fewer rows than this (the rest covered) is not trusted.
const PAGE_MIN_ROWS: usize = 8;
/// The page measure only refines the buttons' (each to a pixel); it may differ from it by at most this.
const REFINE_MAX: f32 = 0.03;
/// A new measure this close to the held scale is the same scale: frames jitter by a pixel, and a scale that moved
/// would move every anchor's reference position with it (and read as a dragged window).
const HOLD: f32 = 0.012;
/// A marker found this close (frame px) to where it was is where it was: the pointer over a button moves its found
/// edge by a pixel, and a moved anchor reads as a dragged window (which forgets the lists).
const JITTER: i32 = 2;
/// The page measure is good to a pixel in ~400 (0.25%): a scale that close to 1 is the reference size itself, which
/// is then read exactly, without resampling.
const SNAP_TO_ONE: f32 = 0.003;

pub fn template(name: &str) -> RgbImage {
    let bytes: &[u8] = match name {
        "label_close_store" => include_bytes!("../../assets/shop/label_close_store.png"),
        "label_sold_items" => include_bytes!("../../assets/shop/label_sold_items.png"),
        "total_label" => include_bytes!("../../assets/shop/total_label.png"),
        _ => unreachable!("unknown shop template {name}"),
    };
    decode_png(bytes).expect("bundled template is a PNG")
}

pub fn total_label() -> &'static GrayImage {
    static LABEL: OnceLock<GrayImage> = OnceLock::new();
    LABEL.get_or_init(|| template("total_label").to_gray())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Anchors {
    /// Top-left of the ENTER button; None while the shop window is not on screen.
    pub enter: Option<Point>,
    /// Top-left of the Sold Items close box; None while that dialog is closed.
    pub sold_x: Option<Point>,
}

/// A frame as captured, read in reference pixels: reference (x, y) is frame (x, y) * scale.
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub frame: &'a RgbImage,
    pub scale: f32,
}

impl View<'_> {
    fn unscaled(&self) -> bool {
        (self.scale - 1.0).abs() < 0.002
    }

    /// The frame's size in reference pixels.
    pub fn size(&self) -> (i32, i32) {
        ((self.frame.width as f32 / self.scale) as i32, (self.frame.height as f32 / self.scale) as i32)
    }

    /// `area` in reference pixels, clipped to the frame like a numpy slice (negative or overhanging edges are cut,
    /// never wrapped). Only this part is resampled, area-averaged when shrinking and bilinear when growing.
    pub fn crop(&self, (x, y, w, h): Area) -> RgbImage {
        let (rw, rh) = self.size();
        let (x0, y0) = (x.clamp(0, rw), y.clamp(0, rh));
        let (x1, y1) = ((x + w).clamp(0, rw), (y + h).clamp(0, rh));
        if x1 <= x0 || y1 <= y0 {
            return RgbImage::new(0, 0);
        }
        if self.unscaled() {
            return self.frame.crop(x0 as usize, y0 as usize, x1 as usize, y1 as usize);
        }
        // One frame pixel of margin each side, so the resampling filter sees what lies beyond the edge.
        let s = self.scale;
        let fx0 = ((x0 as f32 * s).floor() as usize).saturating_sub(1);
        let fy0 = ((y0 as f32 * s).floor() as usize).saturating_sub(1);
        let fx1 = (((x1 as f32 * s).ceil() as usize) + 1).min(self.frame.width);
        let fy1 = (((y1 as f32 * s).ceil() as usize) + 1).min(self.frame.height);
        let part = self.frame.crop(fx0, fy0, fx1, fy1);
        let (pw, ph) = (((fx1 - fx0) as f32 / s).round() as usize, ((fy1 - fy0) as f32 / s).round() as usize);
        let resized = part.resized_to(pw.max(1), ph.max(1));
        let (ox, oy) = ((x0 as f32 - fx0 as f32 / s).round() as usize, (y0 as f32 - fy0 as f32 / s).round() as usize);
        resized.crop(ox, oy, (ox + (x1 - x0) as usize).min(pw), (oy + (y1 - y0) as usize).min(ph))
    }

    /// Frame pixels of a reference area: where its text is read at the game's own resolution.
    pub fn frame_area(&self, (x, y, w, h): Area) -> (usize, usize, usize, usize) {
        let s = self.scale;
        let start = |v: i32| (v as f32 * s).floor().max(0.0) as usize;
        let end = |v: i32| (v as f32 * s).ceil().max(0.0) as usize;
        (start(x), start(y), end(x + w).min(self.frame.width), end(y + h).min(self.frame.height))
    }
}

/// Where the shop is on one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    pub marker: ShopMarker,
    /// The UI scale everything is read at (held while measures agree).
    pub scale: f32,
    /// The Sold Items close box in frame pixels.
    pub sold_box: Option<(i32, i32)>,
    pub anchors: Anchors,
}

/// The chat page's width in frame pixels between its borders, measured along several rows; None when too few rows
/// show both borders.
fn page_width(frame: &RgbImage, marker: ShopMarker, scale: f32) -> Option<f32> {
    let at = |dx: i32, dy: i32| {
        let enter = (ENTER_FROM_BUTTON.0 + dx, ENTER_FROM_BUTTON.1 + dy);
        (marker.x + (enter.0 as f32 * scale).round() as i32, marker.y + (enter.1 as f32 * scale).round() as i32)
    };
    // Page pixels are neutral (white, grey text); its borders are blue-tinted.
    let page = |x: i32, y: i32| {
        x >= 0 && (x as usize) < frame.width && {
            let [r, _, b] = frame.pixel(x as usize, y as usize);
            (b as i32 - r as i32) < 12
        }
    };
    let (y0, y1) = (at(0, PAGE_ROWS.0).1, at(0, PAGE_ROWS.1).1);
    let probe = at(PAGE_PROBE_X, 0).0;
    let limit = (PAGE_W * scale * 1.2) as i32;
    let mut widths: Vec<i32> = (y0..y1)
        .step_by(3)
        .filter(|&y| y >= 0 && (y as usize) < frame.height && page(probe, y))
        .filter_map(|y| {
            let left = (1..limit).find(|&d| !page(probe - d, y))?;
            let right = (1..limit).find(|&d| !page(probe + d, y))?;
            Some(left + right - 1)
        })
        .collect();
    if widths.len() < PAGE_MIN_ROWS {
        return None;
    }
    widths.sort_unstable();
    Some(widths[widths.len() / 2] as f32)
}

/// True when the shop window seems to be still where it was even though its buttons were not found (a tooltip or
/// the pointer over them): its chat page is still there, as wide as before.
pub fn window_remains(frame: &RgbImage, previous: &Located) -> bool {
    page_width(frame, previous.marker, previous.scale)
        .is_some_and(|w| (w / (PAGE_W * previous.scale) - 1.0).abs() < 0.02)
}

/// The shop and its Sold Items dialog on `frame`; `previous` is where they were (looked at first, and its scale
/// kept while the measures agree with it).
pub fn locate(
    frame: &RgbImage,
    previous: Option<&Located>,
    has_list: &dyn Fn(&View<'_>, Point) -> bool,
) -> Option<Located> {
    let (marker, measured) = measure(frame, previous)?;
    let scale = match previous {
        Some(p) if same_scale(measured, p.scale) => p.scale,
        _ => measured,
    };
    Some(place(frame, marker, scale, previous, has_list))
}

/// The shop's markers on `frame` and the scale this frame alone measures (the buttons' width, refined by the chat
/// page's); `previous` is where to look first.
pub fn measure(frame: &RgbImage, previous: Option<&Located>) -> Option<(ShopMarker, f32)> {
    let marker = markers::find_shop(frame, previous.map(|p| p.marker))?;
    let mut scale = marker.scale;
    if let Some(width) = page_width(frame, marker, scale) {
        let measured = width / PAGE_W;
        if (measured / scale - 1.0).abs() <= REFINE_MAX {
            scale = measured;
        }
    }
    if (scale - 1.0).abs() <= SNAP_TO_ONE {
        scale = 1.0;
    }
    Some((marker, scale))
}

/// Is a measure of `measured` the held scale `held`? (See `HOLD`.)
pub fn same_scale(measured: f32, held: f32) -> bool {
    (measured / held - 1.0).abs() <= HOLD
}

/// The shop at `marker`, read at `scale` (decided by the caller from `measure`); `previous` is where it was.
pub fn place(
    frame: &RgbImage,
    mut marker: ShopMarker,
    scale: f32,
    previous: Option<&Located>,
    has_list: &dyn Fn(&View<'_>, Point) -> bool,
) -> Located {
    let near = |a: (i32, i32), b: (i32, i32)| (a.0 - b.0).abs() <= JITTER && (a.1 - b.1).abs() <= JITTER;
    if let Some(p) = previous
        && scale == p.scale
        && near((marker.x, marker.y), (p.marker.x, p.marker.y))
    {
        marker = p.marker;
    }
    let view = View { frame, scale };
    let to_reference = |(x, y): (i32, i32), (dx, dy): Point| {
        ((x as f32 / scale).round() as i32 + dx, (y as f32 / scale).round() as i32 + dy)
    };
    let held = previous.filter(|p| p.scale == scale).and_then(|p| p.sold_box);
    let sold_box = markers::find_sold(frame, scale, held, &|at| has_list(&view, to_reference(at, SOLD_FROM_BOX)))
        .map(|at| held.filter(|&h| near(at, h)).unwrap_or(at));
    Located {
        marker,
        scale,
        sold_box,
        anchors: Anchors {
            enter: Some(to_reference((marker.x, marker.y), ENTER_FROM_BUTTON)),
            sold_x: sold_box.map(|at| to_reference(at, SOLD_FROM_BOX)),
        },
    }
}

// Offsets from the anchors, in reference pixels.

/// The rows area: icon cell through the text, between the dialog's header and its bottom edge.
pub fn sold_list((x, y): Point) -> Area {
    (x - 322, y + 34, 310, 380)
}

/// Scrollbar pixel column and the [start, end) rows of its track.
pub fn sold_scroll((x, y): Point) -> (i32, (i32, i32)) {
    (x + 8, (y + 57, y + 377))
}

pub fn chat_log((x, y): Point) -> Area {
    (x - 340, y - 354, 406, 334)
}

pub fn chat_scroll((x, y): Point) -> (i32, (i32, i32)) {
    (x + 82, (y - 340, y - 39))
}

/// The shop owner's name plate, under their character, left of the meso counter (light letters on dark blue).
pub fn owner_name((x, y): Point) -> Area {
    (x - 690, y - 389, 128, 17)
}

/// The shop owner's meso counter; it changes on every sale.
pub fn balance((x, y): Point) -> Area {
    (x - 511, y - 391, 141, 21)
}
