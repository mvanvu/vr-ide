//! The pixel buffer the backend draws into.

use crate::font::GlyphImage;
use crate::palette::Rgb;

/// A rectangle in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn right(&self) -> u32 {
        self.x + self.width
    }

    #[must_use]
    pub const fn bottom(&self) -> u32 {
        self.y + self.height
    }

    #[must_use]
    pub const fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    #[must_use]
    pub fn intersect(&self, other: &Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Self::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// 0RGB pixels, row-major, `width * height` long (the softbuffer layout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
}

impl Canvas {
    #[must_use]
    pub fn new(width: u32, height: u32, fill: Rgb) -> Self {
        Self {
            pixels: vec![fill; width as usize * height as usize],
            width,
            height,
        }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn bounds(&self) -> PixelRect {
        PixelRect::new(0, 0, self.width, self.height)
    }

    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// The pixel at (x, y); panics outside the canvas.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        assert!(
            x < self.width && y < self.height,
            "pixel outside the canvas"
        );
        self.pixels[(y * self.width + x) as usize]
    }

    /// Copies `rect` (clipped to the canvas) into a frame of the same size, e.g. a softbuffer
    /// buffer, so only damaged areas are copied.
    pub fn copy_rect_to(&self, rect: PixelRect, frame: &mut [u32]) {
        let rect = rect.intersect(&self.bounds());
        if rect.is_empty() || frame.len() < self.pixels.len() {
            return;
        }
        for y in rect.y..rect.bottom() {
            let start = (y * self.width + rect.x) as usize;
            let end = start + rect.width as usize;
            frame[start..end].copy_from_slice(&self.pixels[start..end]);
        }
    }

    pub(crate) fn fill(&mut self, color: Rgb) {
        self.pixels.fill(color);
    }

    pub(crate) fn fill_rect(&mut self, rect: PixelRect, color: Rgb) {
        let rect = rect.intersect(&self.bounds());
        for y in rect.y..rect.bottom() {
            let start = (y * self.width + rect.x) as usize;
            self.pixels[start..start + rect.width as usize].fill(color);
        }
    }

    /// Draws a glyph image whose origin is `(ox, oy)`, only inside `clip`.
    pub(crate) fn draw_glyph(
        &mut self,
        image: &GlyphImage,
        ox: i32,
        oy: i32,
        clip: PixelRect,
        fg: Rgb,
    ) {
        let clip = clip.intersect(&self.bounds());
        let left = i64::from(ox) + i64::from(image.left);
        let top = i64::from(oy) + i64::from(image.top);
        let x0 = left.max(i64::from(clip.x));
        let y0 = top.max(i64::from(clip.y));
        let x1 = (left + i64::from(image.width)).min(i64::from(clip.right()));
        let y1 = (top + i64::from(image.height)).min(i64::from(clip.bottom()));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let stride = self.width as usize;
        for y in y0..y1 {
            let src_row = (y - top) as usize * image.width as usize;
            let dst_row = y as usize * stride;
            for x in x0..x1 {
                let src = src_row + (x - left) as usize;
                let dst = &mut self.pixels[dst_row + x as usize];
                if image.color {
                    let p = &image.data[src * 4..src * 4 + 4];
                    let color = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
                    *dst = blend(*dst, color, p[3]);
                } else {
                    *dst = blend(*dst, fg, image.data[src]);
                }
            }
        }
    }
}

/// `dst` covered by `src` at coverage `alpha` (0..=255).
#[inline]
pub(crate) fn blend(dst: u32, src: u32, alpha: u8) -> u32 {
    match alpha {
        0 => dst,
        255 => src,
        a => {
            let a = u32::from(a) + 1;
            let na = 256 - a;
            let rb = ((src & 0xff_00ff) * a + (dst & 0xff_00ff) * na) >> 8;
            let g = ((src & 0x00_ff00) * a + (dst & 0x00_ff00) * na) >> 8;
            (rb & 0xff_00ff) | (g & 0x00_ff00)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_keeps_the_ends_and_mixes_in_between() {
        assert_eq!(blend(0x00_0000, 0xff_ffff, 0), 0);
        assert_eq!(blend(0x00_0000, 0xff_ffff, 255), 0xff_ffff);
        let half = blend(0x00_0000, 0xff_ffff, 128);
        assert!((0x7f..=0x81).contains(&(half & 0xff)), "{half:06x}");
        assert_eq!(half >> 24, 0);
    }

    #[test]
    fn copy_rect_copies_only_the_rect() {
        let mut c = Canvas::new(4, 3, 0x11);
        c.fill_rect(PixelRect::new(1, 1, 2, 1), 0x22);
        let mut frame = vec![0u32; 12];
        c.copy_rect_to(PixelRect::new(1, 1, 2, 1), &mut frame);
        assert_eq!(frame, [0, 0, 0, 0, 0, 0x22, 0x22, 0, 0, 0, 0, 0]);
    }
}
