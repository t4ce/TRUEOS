#![cfg_attr(not(test), no_std)]

pub mod tree;

pub use tree::{NodeId, Tree};

/// Minimal complex number utilities tailored for kernel-side math use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    /// Creates a new complex number from real and imaginary parts.
    #[inline]
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// Squares the complex number.
    #[inline]
    pub fn square(self) -> Self {
        // (a + bi)^2 = (a^2 - b^2) + 2abi
        Self {
            re: (self.re * self.re) - (self.im * self.im),
            im: 2.0 * self.re * self.im,
        }
    }

    /// Adds another complex number.
    #[inline]
    pub fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    /// Returns the squared magnitude, which avoids the costly square-root.
    #[inline]
    pub fn magnitude_squared(self) -> f64 {
        (self.re * self.re) + (self.im * self.im)
    }
}

/// Returns the iteration count at which the point escapes the Mandelbrot set bounds.
pub fn mandelbrot_escape_depth(c: Complex, max_iter: u32) -> u32 {
    let mut z = Complex::new(0.0, 0.0);
    let mut iter = 0;

    while iter < max_iter && z.magnitude_squared() <= 4.0 {
        z = z.square().add(c);
        iter += 1;
    }

    iter
}

/// Renders a black-and-white Mandelbrot image into the supplied buffer.
///
/// * `buffer` - Target pixel buffer treated as a flat grayscale image.
/// * `width` / `height` - Dimensions of the buffer; must satisfy `width * height == buffer.len()`.
/// * `max_iter` - Maximum iterations per pixel.
pub fn render_mandelbrot(buffer: &mut [u8], width: usize, height: usize, max_iter: u32) {
    if width == 0 || height == 0 {
        return;
    }

    let expected = width.saturating_mul(height);
    if buffer.len() != expected {
        return;
    }

    let real_span = 2.0; // from -1.0 to 1.0
    let imag_span = 2.0;
    let width_scale = if width > 1 {
        real_span / (width as f64 - 1.0)
    } else {
        0.0
    };
    let height_scale = if height > 1 {
        imag_span / (height as f64 - 1.0)
    } else {
        0.0
    };

    for y in 0..height {
        let imag = -1.0 + height_scale * y as f64;
        for x in 0..width {
            let real = -1.0 + width_scale * x as f64;
            let c = Complex::new(real, imag);
            let depth = mandelbrot_escape_depth(c, max_iter);
            let idx = y * width + x;
            buffer[idx] = if depth >= max_iter { 0 } else { 0xFF };
        }
    }
}

/// Renders a grayscale Mandelbrot image into an RGB32 buffer (`0x00RRGGBB`).
///
/// No allocation; safe for early kernel use.
pub fn render_mandelbrot_rgb32(buffer: &mut [u32], width: usize, height: usize, max_iter: u32) {
    if width == 0 || height == 0 {
        return;
    }
    let expected = width.saturating_mul(height);
    if buffer.len() != expected {
        return;
    }

    let real_span = 2.0; // from -1.0 to 1.0
    let imag_span = 2.0;
    let width_scale = if width > 1 {
        real_span / (width as f64 - 1.0)
    } else {
        0.0
    };
    let height_scale = if height > 1 {
        imag_span / (height as f64 - 1.0)
    } else {
        0.0
    };

    let denom = max_iter.max(1);
    for y in 0..height {
        let imag = -1.0 + height_scale * y as f64;
        for x in 0..width {
            let real = -1.0 + width_scale * x as f64;
            let c = Complex::new(real, imag);
            let depth = mandelbrot_escape_depth(c, max_iter);
            let idx = y * width + x;

            let luma: u32 = if depth >= max_iter {
                0
            } else {
                (depth.saturating_mul(255) / denom) as u32
            };

            buffer[idx] = (luma << 16) | (luma << 8) | luma;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mandelbrot_smoke() {
        const WIDTH: usize = 256;
        const HEIGHT: usize = 256;
        const MAX_ITER: u32 = 10;

        let mut buffer = [0u8; WIDTH * HEIGHT];
        render_mandelbrot(&mut buffer, WIDTH, HEIGHT, MAX_ITER);

        let center_index = (HEIGHT / 2) * WIDTH + (WIDTH / 2);
        let corner_index = 0;

        assert_eq!(buffer[center_index], 0, "Center of the set should stay black");
        assert_eq!(buffer[corner_index], 0xFF, "Corner should escape quickly and be white");
    }
}
