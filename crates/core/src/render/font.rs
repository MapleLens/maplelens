//! The card's typeface: Tahoma (the game's UI face) where the platform has it, a look-alike elsewhere.

use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont, point};

/// Regular and bold faces of one family.
#[derive(Clone)]
pub struct Fonts {
    regular: FontArc,
    bold: FontArc,
}

/// One positioned glyph, in design units.
pub struct Placed {
    pub id: GlyphId,
    pub x: f32,
}

impl Fonts {
    pub fn from_bytes(regular: Vec<u8>, bold: Vec<u8>) -> Result<Self, ab_glyph::InvalidFont> {
        Ok(Self { regular: FontArc::try_from_vec(regular)?, bold: FontArc::try_from_vec(bold)? })
    }

    pub fn face(&self, bold: bool) -> &FontArc {
        if bold { &self.bold } else { &self.regular }
    }

    /// The ab_glyph scale for a font size in pixels per em (how Pillow and CSS size fonts).
    pub fn scale(&self, size: f32, bold: bool) -> PxScale {
        let face = self.face(bold);
        let em = face.units_per_em().unwrap_or(2048.0);
        PxScale::from(size * face.height_unscaled() / em)
    }

    /// (ascent, descent) in pixels at `size`; descent is negative.
    pub fn metrics(&self, size: f32, bold: bool) -> (f32, f32) {
        let scaled = self.face(bold).as_scaled(self.scale(size, bold));
        (scaled.ascent(), scaled.descent())
    }

    /// Glyphs of `text` with their pen positions (kerning applied), and the total advance.
    pub fn layout(&self, text: &str, size: f32, bold: bool) -> (Vec<Placed>, f32) {
        let scaled = self.face(bold).as_scaled(self.scale(size, bold));
        let mut x = 0.0;
        let mut previous: Option<GlyphId> = None;
        let mut placed = Vec::with_capacity(text.len());
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            if let Some(prev) = previous {
                x += scaled.kern(prev, id);
            }
            placed.push(Placed { id, x });
            x += scaled.h_advance(id);
            previous = Some(id);
        }
        (placed, x)
    }

    /// Advance width of `text` in pixels at `size`.
    pub fn advance(&self, text: &str, size: f32, bold: bool) -> f32 {
        self.layout(text, size, bold).1
    }

    /// Rasterise `text` with its baseline-left at device pixel (`x`, `baseline`), scaled by `factor`; calls
    /// `plot(px, py, coverage)` for every covered device pixel.
    pub fn rasterize(
        &self,
        text: &str,
        size: f32,
        bold: bool,
        (x, baseline): (f32, f32),
        factor: f32,
        mut plot: impl FnMut(i32, i32, f32),
    ) {
        let (glyphs, _) = self.layout(text, size, bold);
        let face = self.face(bold);
        let scale = self.scale(size * factor, bold);
        for g in glyphs {
            let glyph = g.id.with_scale_and_position(scale, point(x + g.x * factor, baseline));
            if let Some(outline) = face.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    plot(bounds.min.x as i32 + gx as i32, bounds.min.y as i32 + gy as i32, coverage);
                });
            }
        }
    }
}
