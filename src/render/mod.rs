/// A 1-bit pixel buffer representing what the e-ink display shows.
///
/// Pixels are packed 8 per byte, row-major. Bit 7 of each byte is the
/// leftmost pixel in the group. A set bit (1) is black; clear (0) is white.
#[derive(Debug, Clone)]
pub struct PixelBuffer {
    pub width: u32,
    pub height: u32,
    /// Raw pixel data: ceil(width * height / 8) bytes.
    pub pixels: Vec<u8>,
}

impl PixelBuffer {
    /// Create a new all-white (zero) pixel buffer.
    pub fn new(width: u32, height: u32) -> Self {
        let byte_count = (width * height).div_ceil(8) as usize;
        PixelBuffer {
            width,
            height,
            pixels: vec![0u8; byte_count],
        }
    }

    /// Set a single pixel. `true` = black, `false` = white.
    pub fn set_pixel(&mut self, x: u32, y: u32, black: bool) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = (y * self.width + x) as usize;
        let byte = idx / 8;
        let bit = 7 - (idx % 8);
        if black {
            self.pixels[byte] |= 1 << bit;
        } else {
            self.pixels[byte] &= !(1 << bit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_all_white() {
        let buf = PixelBuffer::new(800, 480);
        assert_eq!(buf.width, 800);
        assert_eq!(buf.height, 480);
        assert!(buf.pixels.iter().all(|&b| b == 0));
    }

    #[test]
    fn buffer_byte_count() {
        // 800 × 480 = 384 000 pixels → 48 000 bytes
        let buf = PixelBuffer::new(800, 480);
        assert_eq!(buf.pixels.len(), 48_000);
    }

    #[test]
    fn set_pixel_marks_byte() {
        let mut buf = PixelBuffer::new(16, 1);
        buf.set_pixel(0, 0, true);
        assert_eq!(buf.pixels[0] & 0x80, 0x80);
        buf.set_pixel(0, 0, false);
        assert_eq!(buf.pixels[0] & 0x80, 0x00);
    }

    #[test]
    fn out_of_bounds_set_is_noop() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.set_pixel(100, 100, true);
    }
}
