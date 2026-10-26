//! Drawing the overlays: the price card, the rank panel and the store status pill, as straight-alpha RGBA images.

pub mod canvas;
pub mod card;
pub mod details;
pub mod font;
pub mod gamewin;
pub mod glance;
pub mod map_card;
pub mod maple_ui;
pub mod origin;
pub mod range_bar;
pub mod rank_card;
pub mod settings_view;
pub mod status_pill;
pub mod theme;

/// A rendered overlay: straight (not premultiplied) RGBA, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl RgbaImage {
    pub fn from_canvas(canvas: &canvas::Canvas<'_>) -> Self {
        Self { width: canvas.pixmap.width() as usize, height: canvas.pixmap.height() as usize, data: canvas.to_rgba() }
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }

    /// Premultiplied BGRA, what a Windows layered window takes.
    pub fn premultiplied_bgra(&self) -> Vec<u8> {
        self.data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = p[3] as u32;
                let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                [pm(p[2]), pm(p[1]), pm(p[0]), p[3]]
            })
            .collect()
    }

    /// Bounding box (x0, y0, x1, y1) of pixels with any alpha, or None for a blank image.
    pub fn alpha_bbox(&self) -> Option<(usize, usize, usize, usize)> {
        let mut bbox: Option<(usize, usize, usize, usize)> = None;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.pixel(x, y)[3] > 0 {
                    bbox = Some(match bbox {
                        None => (x, y, x + 1, y + 1),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                    });
                }
            }
        }
        bbox
    }
}
