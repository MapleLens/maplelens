//! Plain pixel buffers: RGB frames from the screen and 8-bit grey planes for the vision code.
//!
//! Captures are small (the top of a tooltip is 743x120), so crops copy instead of borrowing: it keeps
//! every vision function a simple `&RgbImage -> answer` with no lifetimes to thread around.

/// 8-bit RGB pixels, row-major, no padding.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RgbImage {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// One 8-bit channel per pixel, row-major.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrayImage {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl RgbImage {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, data: vec![0; width * height * 3] }
    }

    pub fn filled(width: usize, height: usize, rgb: [u8; 3]) -> Self {
        Self { width, height, data: rgb.repeat(width * height) }
    }

    /// From 4-byte pixels in the given channel order (`bgra` for GDI captures, else RGBA from a browser canvas).
    pub fn from_4ch(width: usize, height: usize, pixels: &[u8], bgra: bool) -> Self {
        let mut data = Vec::with_capacity(width * height * 3);
        for px in pixels.as_chunks::<4>().0.iter().take(width * height) {
            if bgra {
                data.extend_from_slice(&[px[2], px[1], px[0]]);
            } else {
                data.extend_from_slice(&px[..3]);
            }
        }
        Self { width, height, data }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    #[inline]
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        let i = (y * self.width + x) * 3;
        self.data[i..i + 3].copy_from_slice(&rgb);
    }

    /// The part of the image inside x0..x1, y0..y1, clipped to the image like a numpy slice.
    pub fn crop(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Self {
        let (x1, y1) = (x1.min(self.width), y1.min(self.height));
        let (x0, y0) = (x0.min(x1), y0.min(y1));
        let width = x1 - x0;
        let mut data = Vec::with_capacity(width * (y1 - y0) * 3);
        for y in y0..y1 {
            let row = (y * self.width + x0) * 3;
            data.extend_from_slice(&self.data[row..row + width * 3]);
        }
        Self { width, height: y1 - y0, data }
    }

    /// Rows `y0..` (clipped), all columns.
    pub fn rows_from(&self, y0: usize) -> Self {
        self.crop(0, y0, self.width, self.height)
    }

    /// Luma like OpenCV's BGR2GRAY (same fixed-point weights), so thresholds measured with it still hold.
    pub fn to_gray(&self) -> GrayImage {
        let data = self.data.as_chunks::<3>().0.iter().map(|p| luma(p[0], p[1], p[2])).collect();
        GrayImage { width: self.width, height: self.height, data }
    }

    /// Resize by `factor` with area averaging when shrinking and bilinear interpolation when growing.
    pub fn resized(&self, factor: f32) -> Self {
        let width = ((self.width as f32 * factor).round() as usize).max(1);
        let height = ((self.height as f32 * factor).round() as usize).max(1);
        self.resized_to(width, height)
    }

    /// Area averaging when shrinking, bilinear when growing.
    pub fn resized_to(&self, width: usize, height: usize) -> Self {
        Self { width, height, data: resample(&self.data, self.width, self.height, 3, width, height) }
    }

    /// Enlarged by a whole `factor`, each pixel becoming a block: pixel fonts keep even strokes.
    pub fn upscaled_nearest(&self, factor: usize) -> Self {
        let (width, height) = (self.width * factor, self.height * factor);
        let mut data = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&self.pixel(x / factor, y / factor));
            }
        }
        Self { width, height, data }
    }
}

#[inline]
pub fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((r as u32 * 4899 + g as u32 * 9617 + b as u32 * 1868 + 8192) >> 14) as u8
}

impl GrayImage {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, data: vec![0; width * height] }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.width + x]
    }

    pub fn crop(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Self {
        let (x1, y1) = (x1.min(self.width), y1.min(self.height));
        let (x0, y0) = (x0.min(x1), y0.min(y1));
        let width = x1 - x0;
        let mut data = Vec::with_capacity(width * (y1 - y0));
        for y in y0..y1 {
            data.extend_from_slice(&self.data[y * self.width + x0..y * self.width + x0 + width]);
        }
        Self { width, height: y1 - y0, data }
    }

    pub fn resized(&self, factor: f32) -> Self {
        let width = ((self.width as f32 * factor).round() as usize).max(1);
        let height = ((self.height as f32 * factor).round() as usize).max(1);
        self.resized_to(width, height)
    }

    /// Enlarged by a whole `factor` with OpenCV's INTER_CUBIC (a = -0.75, edges replicated): crisper glyph corners
    /// than bilinear, which OCR engines read better.
    pub fn upscaled_cubic(&self, factor: usize) -> Self {
        let weights = |t: f32| {
            let a = -0.75f32;
            let w = |x: f32| {
                let x = x.abs();
                if x <= 1.0 {
                    ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    ((a * x - 5.0 * a) * x + 8.0 * a) * x - 4.0 * a
                } else {
                    0.0
                }
            };
            [w(1.0 + t), w(t), w(1.0 - t), w(2.0 - t)]
        };
        let taps = |source: usize, target: usize| -> Vec<([usize; 4], [f32; 4])> {
            (0..target)
                .map(|i| {
                    let centre = (i as f32 + 0.5) / factor as f32 - 0.5;
                    let first = centre.floor();
                    let at = |k: f32| (first + k).clamp(0.0, (source - 1) as f32) as usize;
                    ([at(-1.0), at(0.0), at(1.0), at(2.0)], weights(centre - first))
                })
                .collect()
        };
        let (width, height) = (self.width * factor, self.height * factor);
        if self.width == 0 || self.height == 0 {
            return Self::new(width, height);
        }
        let (xs, ys) = (taps(self.width, width), taps(self.height, height));
        let mut wide = vec![0.0f32; self.height * width];
        for y in 0..self.height {
            for (x, (index, weight)) in xs.iter().enumerate() {
                wide[y * width + x] = (0..4).map(|k| self.get(index[k], y) as f32 * weight[k]).sum();
            }
        }
        let mut out = Self::new(width, height);
        for (y, (index, weight)) in ys.iter().enumerate() {
            for x in 0..width {
                let v: f32 = (0..4).map(|k| wide[index[k] * width + x] * weight[k]).sum();
                out.data[y * width + x] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
        out
    }

    /// Area averaging when shrinking (no aliasing on thin glyph strokes), bilinear when growing.
    pub fn resized_to(&self, width: usize, height: usize) -> Self {
        Self { width, height, data: resample(&self.data, self.width, self.height, 1, width, height) }
    }
}

/// For each output index along one axis: the first source index it reads and the weights of the sources
/// from there on. A box filter over the covered source pixels when shrinking, linear between the two nearest
/// when growing.
fn taps(source: usize, target: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = source as f32 / target as f32;
    (0..target)
        .map(|i| {
            if scale >= 1.0 {
                let (start, end) = (i as f32 * scale, (i as f32 + 1.0) * scale);
                let first = start.floor() as usize;
                let last = ((end - 1e-4).floor() as usize).min(source - 1);
                let weights: Vec<f32> = (first..=last)
                    .map(|p| ((p as f32 + 1.0).min(end) - (p as f32).max(start)).max(0.0) / scale)
                    .collect();
                (first, weights)
            } else {
                let centre = ((i as f32 + 0.5) * scale - 0.5).clamp(0.0, (source - 1) as f32);
                let first = centre.floor() as usize;
                let f = centre - first as f32;
                if first + 1 < source { (first, vec![1.0 - f, f]) } else { (first, vec![1.0]) }
            }
        })
        .collect()
}

/// Separable resize of interleaved 8-bit pixels with `channels` channels. Each source row is filtered horizontally
/// once, when the first output row needs it, and dropped when no later one does: a full game frame stays a few rows
/// of floats instead of a whole float copy of the image.
fn resample(data: &[u8], width: usize, height: usize, channels: usize, out_w: usize, out_h: usize) -> Vec<u8> {
    if width == 0 || height == 0 || out_w == 0 || out_h == 0 {
        return vec![0; out_w * out_h * channels];
    }
    let (xs, ys) = (taps(width, out_w), taps(height, out_h));
    let stride = out_w * channels;
    // Growing (the usual case: a small UI brought up to the reference size) reads exactly two source pixels.
    // (The last pixel may read just one; its pair repeats it with weight 0.)
    let pairs: Option<Vec<(usize, usize, f32, f32)>> = xs
        .iter()
        .map(|(first, w)| match w[..] {
            [w0, w1] => Some((*first, first + 1, w0, w1)),
            [w0] => Some((*first, *first, w0, 0.0)),
            _ => None,
        })
        .collect();
    let filter_row = |y: usize, out: &mut Vec<f32>| {
        out.clear();
        out.resize(stride, 0.0);
        let row = &data[y * width * channels..(y + 1) * width * channels];
        if let Some(pairs) = &pairs {
            for (out, &(first, second, w0, w1)) in out.chunks_exact_mut(channels).zip(pairs) {
                let (a, b) = (&row[first * channels..][..channels], &row[second * channels..][..channels]);
                for c in 0..channels {
                    out[c] = a[c] as f32 * w0 + b[c] as f32 * w1;
                }
            }
            return;
        }
        for (x, (first, weights)) in xs.iter().enumerate() {
            let out = &mut out[x * channels..][..channels];
            for (k, w) in weights.iter().enumerate() {
                let src = &row[(first + k) * channels..][..channels];
                for c in 0..channels {
                    out[c] += src[c] as f32 * w;
                }
            }
        }
    };
    // Filtered source rows (index, values), consecutive; output rows read them in increasing order.
    let mut rows: std::collections::VecDeque<(usize, Vec<f32>)> = std::collections::VecDeque::new();
    let mut spare: Vec<Vec<f32>> = Vec::new();
    let mut out = vec![0u8; out_w * out_h * channels];
    let mut acc = vec![0.0f32; stride];
    for (y, (first, weights)) in ys.iter().enumerate() {
        while rows.front().is_some_and(|(i, _)| i < first) {
            spare.extend(rows.pop_front().map(|(_, values)| values));
        }
        let wanted = first + weights.len();
        let mut next = rows.back().map_or(*first, |(i, _)| i + 1);
        while next < wanted {
            let mut values = spare.pop().unwrap_or_default();
            filter_row(next, &mut values);
            rows.push_back((next, values));
            next += 1;
        }
        let target = &mut out[y * stride..(y + 1) * stride];
        if let [w0, w1] = weights[..] {
            for ((t, a), b) in target.iter_mut().zip(&rows[0].1).zip(&rows[1].1) {
                *t = ((a * w0 + b * w1).clamp(0.0, 255.0) + 0.5) as u8;
            }
            continue;
        }
        acc.fill(0.0);
        for (k, w) in weights.iter().enumerate() {
            for (a, s) in acc.iter_mut().zip(&rows[k].1) {
                *a += s * w;
            }
        }
        for (t, a) in target.iter_mut().zip(&acc) {
            // Same as round() for these non-negative sums, and far cheaper over a whole frame.
            *t = (a.clamp(0.0, 255.0) + 0.5) as u8;
        }
    }
    out
}

/// Decode a PNG (8-bit RGB, RGBA, grey or grey+alpha) into RGB, dropping alpha.
pub fn decode_png(bytes: &[u8]) -> Result<RgbImage, png::DecodingError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = info.color_type.samples();
    let mut data = Vec::with_capacity(w * h * 3);
    for px in buf[..info.buffer_size()].chunks_exact(channels) {
        match channels {
            1 | 2 => data.extend_from_slice(&[px[0], px[0], px[0]]),
            _ => data.extend_from_slice(&px[..3]),
        }
    }
    Ok(RgbImage { width: w, height: h, data })
}

/// Decode a PNG keeping its alpha channel, as straight (not premultiplied) RGBA.
pub fn decode_png_rgba(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), png::DecodingError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    let channels = info.color_type.samples();
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    for px in buf[..info.buffer_size()].chunks_exact(channels) {
        match channels {
            1 => rgba.extend_from_slice(&[px[0], px[0], px[0], 255]),
            2 => rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]),
            3 => rgba.extend_from_slice(&[px[0], px[1], px[2], 255]),
            _ => rgba.extend_from_slice(&px[..4]),
        }
    }
    Ok((info.width as usize, info.height as usize, rgba))
}

/// Encode RGB as PNG (debug captures that become test fixtures).
pub fn encode_png(image: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width as u32, image.height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = encoder.write_header() {
            let _ = writer.write_image_data(&image.data);
        }
    }
    out
}
