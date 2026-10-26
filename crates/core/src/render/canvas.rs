//! A small drawing toolkit on a transparent RGBA pixmap. Layout happens in design units (the 1x card);
//! everything is drawn `factor` times bigger, so text and curves are sharp at the size they are shown.

use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

use super::font::Fonts;
use super::theme::{self, Rgba};

/// Where (x, y) sits on the text, like Pillow's anchors: horizontal l/m/r, vertical a(scender)/m(iddle)/s(baseline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    LA,
    LM,
    LS,
    MA,
    MS,
    RA,
    RM,
    RS,
}

/// How text is drawn: size in pixels per em, colour and weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub size: f32,
    pub colour: Rgba,
    pub bold: bool,
}

impl Ink {
    pub const fn new(size: f32, colour: Rgba) -> Self {
        Self { size, colour, bold: false }
    }

    pub const fn bold(self) -> Self {
        Self { bold: true, ..self }
    }
}

pub struct Canvas<'f> {
    pub pixmap: Pixmap,
    /// Size in design units.
    pub width: f32,
    pub height: f32,
    /// Device pixels per design unit.
    pub factor: f32,
    pub fonts: &'f Fonts,
}

fn paint(colour: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(colour[0], colour[1], colour[2], colour[3]);
    paint.anti_alias = true;
    paint
}

/// Round half to even, as Python's `round` sized the cards (371 * 1.5 = 556.5 -> 556).
fn round_even(value: f32) -> f32 {
    let rounded = value.round();
    if (value - value.trunc()).abs() == 0.5 && rounded % 2.0 != 0.0 { rounded - value.signum() } else { rounded }
}

/// Rounded rectangle path (cubic corners), radius clamped to half the shorter side.
pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = r * 0.447_715; // 1 - 0.552285: cubic control offset for a quarter circle
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - k, y, x + w, y + k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - k, x + w - k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + k, y + h, x, y + h - k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + k, x + k, y, x + r, y);
    pb.close();
    pb.finish()
}

impl<'f> Canvas<'f> {
    pub fn new(fonts: &'f Fonts, width: f32, height: f32, factor: f32) -> Self {
        let (w, h) = ((round_even(width * factor) as u32).max(1), (round_even(height * factor) as u32).max(1));
        Self { pixmap: Pixmap::new(w, h).expect("non-zero canvas"), width, height, factor, fonts }
    }

    fn transform(&self) -> Transform {
        Transform::from_scale(self.factor, self.factor)
    }

    pub fn fill_path(&mut self, path: &Path, colour: Rgba) {
        let t = self.transform();
        self.pixmap.fill_path(path, &paint(colour), FillRule::Winding, t, None);
    }

    pub fn stroke_path(&mut self, path: &Path, colour: Rgba, width: f32) {
        let stroke = Stroke { width, line_join: LineJoin::Round, line_cap: LineCap::Round, ..Stroke::default() };
        let t = self.transform();
        self.pixmap.stroke_path(path, &paint(colour), &stroke, t, None);
    }

    /// Filled rectangle covering x..x+w, y..y+h.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, colour: Rgba) {
        if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
            self.fill_path(&PathBuilder::from_rect(rect), colour);
        }
    }

    /// Rounded box (x, y, w, h) with an optional 1 px border drawn just inside its edge.
    pub fn rounded(&mut self, (x, y, w, h): (f32, f32, f32, f32), r: f32, fill: Option<Rgba>, edge: Option<Rgba>) {
        if let Some(fill) = fill
            && let Some(path) = rounded_rect(x, y, w, h, r)
        {
            self.fill_path(&path, fill);
        }
        if let Some(edge) = edge
            && let Some(path) = rounded_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, (r - 0.5).max(0.0))
        {
            self.stroke_path(&path, edge, 1.0);
        }
    }

    /// Rounded box shaded top to bottom through `stops` (offset 0..=1, colour), like a CSS linear-gradient.
    pub fn gradient(&mut self, (x, y, w, h): (f32, f32, f32, f32), r: f32, stops: &[(f32, Rgba)]) {
        let Some(path) = rounded_rect(x, y, w, h, r) else { return };
        let stops = stops
            .iter()
            .map(|&(at, c)| tiny_skia::GradientStop::new(at, tiny_skia::Color::from_rgba8(c[0], c[1], c[2], c[3])))
            .collect();
        let (top, bottom) = (tiny_skia::Point::from_xy(x, y), tiny_skia::Point::from_xy(x, y + h));
        let Some(shader) =
            tiny_skia::LinearGradient::new(top, bottom, stops, tiny_skia::SpreadMode::Pad, Transform::identity())
        else {
            return;
        };
        let paint = Paint { shader, anti_alias: true, ..Paint::default() };
        let t = self.transform();
        self.pixmap.fill_path(&path, &paint, FillRule::Winding, t, None);
    }

    /// Text ringed by `edge` one unit thick: the game's outlined button lettering.
    pub fn outlined(&mut self, x: f32, y: f32, text: &str, ink: Ink, edge: Rgba, anchor: Anchor) -> f32 {
        for (dx, dy) in
            [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)]
        {
            self.text(x + dx, y + dy, text, Ink { colour: edge, ..ink }, anchor);
        }
        self.text(x, y, text, ink, anchor)
    }

    pub fn polygon(&mut self, points: &[(f32, f32)], colour: Rgba) {
        let mut pb = PathBuilder::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        pb.close();
        if let Some(path) = pb.finish() {
            self.fill_path(&path, colour);
        }
    }

    pub fn polyline(&mut self, points: &[(f32, f32)], colour: Rgba, width: f32) {
        let mut pb = PathBuilder::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        if let Some(path) = pb.finish() {
            self.stroke_path(&path, colour, width);
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, fill: Rgba, edge: Option<(Rgba, f32)>) {
        if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
            self.fill_path(&path, fill);
            if let Some((colour, width)) = edge {
                self.stroke_path(&path, colour, width);
            }
        }
    }

    /// Straight-alpha RGBA image drawn with its top-left at (x, y), `w` x `h` design units.
    pub fn image(&mut self, x: f32, y: f32, w: f32, h: f32, (iw, ih, rgba): (usize, usize, &[u8])) {
        let Some(mut source) = Pixmap::new(iw as u32, ih as u32) else { return };
        for (dst, px) in source.pixels_mut().iter_mut().zip(rgba.as_chunks::<4>().0) {
            *dst = tiny_skia::ColorU8::from_rgba(px[0], px[1], px[2], px[3]).premultiply();
        }
        let sx = w * self.factor / iw as f32;
        let sy = h * self.factor / ih as f32;
        let t = Transform::from_row(sx, 0.0, 0.0, sy, x * self.factor, y * self.factor);
        let quality = PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..PixmapPaint::default() };
        self.pixmap.draw_pixmap(0, 0, source.as_ref(), &quality, t, None);
    }

    /// Composite another canvas's pixels (same factor) with its top-left at design (x, y).
    pub fn composite(&mut self, other: &Canvas<'_>, x: f32, y: f32) {
        let (dx, dy) = ((x * self.factor).round() as i32, (y * self.factor).round() as i32);
        self.pixmap.draw_pixmap(dx, dy, other.pixmap.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }

    /// Composite the rows of `other` (same size and factor) from design row `top` down, in place.
    pub fn composite_below(&mut self, other: &Canvas<'_>, top: f32) {
        let y = (top * self.factor).round() as i32;
        let rows = (self.pixmap.height() as i32 - y).min(other.pixmap.height() as i32 - y);
        let rect = tiny_skia::IntRect::from_xywh(0, y, other.pixmap.width(), rows.max(1) as u32);
        if let Some(part) = rect.and_then(|r| other.pixmap.clone_rect(r)) {
            self.pixmap.draw_pixmap(0, y, part.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }

    fn blend(&mut self, px: i32, py: i32, colour: Rgba, coverage: f32) {
        let (w, h) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        if px < 0 || py < 0 || px >= w || py >= h || coverage <= 0.0 {
            return;
        }
        let alpha = colour[3] as f32 / 255.0 * coverage.min(1.0);
        let pixel = &mut self.pixmap.pixels_mut()[(py * w + px) as usize];
        let keep = 1.0 - alpha;
        let mix = |src: u8, dst: u8| (src as f32 * alpha + dst as f32 * keep).round().min(255.0) as u8;
        let a = (alpha * 255.0 + pixel.alpha() as f32 * keep).round().min(255.0) as u8;
        let (r, g, b) = (mix(colour[0], pixel.red()), mix(colour[1], pixel.green()), mix(colour[2], pixel.blue()));
        // Premultiplied channels never exceed alpha; rounding could, which tiny-skia rejects.
        *pixel = tiny_skia::PremultipliedColorU8::from_rgba(r.min(a), g.min(a), b.min(a), a).unwrap_or(*pixel);
    }

    /// Draw text and return its width in design units (rounded, as layout uses it).
    pub fn text(&mut self, x: f32, y: f32, text: &str, ink: Ink, anchor: Anchor) -> f32 {
        self.draw_text(x, y, text, ink, anchor, false)
    }

    /// Text with hard, unsmoothed edges (the game's blocky window titles).
    pub fn text_aliased(&mut self, x: f32, y: f32, text: &str, ink: Ink, anchor: Anchor) -> f32 {
        self.draw_text(x, y, text, ink, anchor, true)
    }

    fn draw_text(&mut self, x: f32, y: f32, text: &str, ink: Ink, anchor: Anchor, aliased: bool) -> f32 {
        let width = self.fonts.advance(text, ink.size, ink.bold);
        let (ascent, descent) = self.fonts.metrics(ink.size, ink.bold);
        let left = match anchor {
            Anchor::LA | Anchor::LM | Anchor::LS => x,
            Anchor::MA | Anchor::MS => x - width / 2.0,
            Anchor::RA | Anchor::RM | Anchor::RS => x - width,
        };
        let baseline = match anchor {
            Anchor::LA | Anchor::MA | Anchor::RA => y + ascent,
            Anchor::LM | Anchor::RM => y + (ascent + descent) / 2.0,
            Anchor::LS | Anchor::MS | Anchor::RS => y,
        };
        let factor = self.factor;
        let mut plots = Vec::new();
        self.fonts.rasterize(text, ink.size, ink.bold, (left * factor, baseline * factor), factor, |px, py, c| {
            plots.push((px, py, c));
        });
        for (px, py, c) in plots {
            let coverage = if aliased { if c >= 0.5 { 1.0 } else { 0.0 } } else { c };
            self.blend(px, py, ink.colour, coverage);
        }
        width.round()
    }

    pub fn measure(&self, text: &str, size: f32, bold: bool) -> f32 {
        self.fonts.advance(text, size, bold).round()
    }

    /// Greedy word wrap.
    pub fn wrap(&self, text: &str, size: f32, max_width: f32, bold: bool) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            let trial = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && self.measure(&trial, size, bold) > max_width {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = trial;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }

    /// Rounded label; returns its width. With `right_align`, x is the chip's right edge.
    pub fn chip(&mut self, x: f32, y: f32, text: &str, ink: Ink, right_align: bool, height: f32) -> f32 {
        let width = self.measure(text, ink.size, ink.bold) + 14.0;
        let left = if right_align { x - width } else { x };
        self.rounded((left, y, width, height), 5.0, Some(theme::CHIP_FILL), Some(theme::CHIP_EDGE));
        self.text(left + 7.0, y + height / 2.0, text, ink, Anchor::LM);
        width
    }

    /// Solid-tinted status label, e.g. a green "+8.6%", 20 px tall. `arrow` Some(up) adds an up/down triangle.
    pub fn pill(&mut self, x: f32, y: f32, text: &str, colour: Rgba, arrow: Option<bool>) -> f32 {
        let (height, size) = (20.0, 11.0);
        let width = self.pill_width(text, size, arrow.is_some());
        let (tint, edge) = (theme::with_alpha(colour, 55), theme::with_alpha(colour, 150));
        self.rounded((x, y, width, height), height / 2.0, Some(tint), Some(edge));
        let mut left = x + 8.0;
        if let Some(up) = arrow {
            self.arrow(left, y + (height / 2.0).floor() - 4.0, up, colour, 8.0);
            left += 14.0;
        }
        self.text(left, y + height / 2.0, text, Ink::new(size, colour).bold(), Anchor::LM);
        width
    }

    pub fn pill_width(&self, text: &str, size: f32, arrow: bool) -> f32 {
        self.measure(text, size, true) + 16.0 + if arrow { 14.0 } else { 0.0 }
    }

    /// Filled triangle (Tahoma has no reliable arrow glyphs).
    pub fn arrow(&mut self, x: f32, y: f32, up: bool, colour: Rgba, size: f32) {
        let points = if up {
            [(x, y + size), (x + size, y + size), (x + size / 2.0, y)]
        } else {
            [(x, y), (x + size, y), (x + size / 2.0, y + size)]
        };
        self.polygon(&points, colour);
    }

    /// A hairline across the canvas, `pad` in from each side.
    pub fn divider(&mut self, y: f32, pad: f32) {
        let width = self.width;
        self.rect(pad, y, width - 2.0 * pad, 1.0, theme::DIVIDER);
    }

    /// The game's tooltip frame: translucent fill, outer line, light inner line.
    pub fn frame(&mut self, height: f32) {
        let w = self.width;
        self.rect(0.0, 0.0, w, height, theme::PANEL_FILL);
        if let Some(path) = rounded_rect(0.5, 0.5, w - 1.0, height - 1.0, 0.0) {
            self.stroke_path(&path, theme::EDGE, 1.0);
        }
        if let Some(path) = rounded_rect(2.5, 2.5, w - 5.0, height - 5.0, 0.0) {
            self.stroke_path(&path, theme::EDGE_INNER, 1.0);
        }
    }

    /// Straight (unpremultiplied) RGBA bytes of the canvas.
    pub fn to_rgba(&self) -> Vec<u8> {
        self.pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect()
    }
}
