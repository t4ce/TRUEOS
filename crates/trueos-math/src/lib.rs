#![feature(f128)]
#![cfg_attr(not(test), no_std)]

#[cfg(any(feature = "alloc", test))]
extern crate alloc;

pub mod tree;
pub use tree::{Children, NodeId, Tree};

pub mod complex;
pub use complex::Complex;

pub mod matrix;
pub use matrix::{Matrix, Vector};

#[inline]
pub fn sin_f32(x: f32) -> f32 {
    libm::sinf(x)
}

#[inline]
pub fn cos_f32(x: f32) -> f32 {
    libm::cosf(x)
}

#[inline]
pub fn acos_f32(x: f32) -> f32 {
    libm::acosf(x)
}

#[inline]
pub fn asin_f32(x: f32) -> f32 {
    libm::asinf(x)
}

#[inline]
pub fn log2_f32(x: f32) -> f32 {
    libm::log2f(x)
}

// Kernel-side C math ABI symbols needed by linked no_std code. Keep this list
// intentionally narrow; trueos-math owns these f32 wrappers over libm.
#[unsafe(no_mangle)]
pub extern "C" fn sinf(x: f32) -> f32 {
    sin_f32(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn cosf(x: f32) -> f32 {
    cos_f32(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn acosf(x: f32) -> f32 {
    acos_f32(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn asinf(x: f32) -> f32 {
    asin_f32(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log2f(x: f32) -> f32 {
    log2_f32(x)
}

pub(crate) mod ascii_tree;

#[cfg(any(feature = "alloc", test))]
pub(crate) mod html_tree;

#[cfg(any(feature = "alloc", test))]
pub mod pbltree;

#[cfg(any(feature = "alloc", test))]
pub use pbltree::{BPlusTree, Iter as BPlusTreeIter};

#[cfg(any(feature = "alloc", test))]
pub mod bst_arena;

#[cfg(any(feature = "alloc", test))]
pub mod bst;

#[cfg(any(feature = "alloc", test))]
pub use bst::BstMap;

#[cfg(any(feature = "alloc", test))]
pub mod avl;

#[cfg(any(feature = "alloc", test))]
pub use avl::AvlTree;

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

#[derive(Clone, Copy, Debug, PartialEq)]
struct Complex128 {
    re: f128,
    im: f128,
}

impl Complex128 {
    #[inline]
    const fn new(re: f128, im: f128) -> Self {
        Self { re, im }
    }

    #[inline]
    fn square(self) -> Self {
        Self {
            re: (self.re * self.re) - (self.im * self.im),
            im: 2.0f128 * self.re * self.im,
        }
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    #[inline]
    fn magnitude_squared(self) -> f128 {
        (self.re * self.re) + (self.im * self.im)
    }
}

#[inline]
fn mandelbrot_escape_depth_f128(c: Complex128, max_iter: u32) -> u32 {
    let mut z = Complex128::new(0.0f128, 0.0f128);
    let mut iter = 0;

    while iter < max_iter && z.magnitude_squared() <= 4.0f128 {
        z = z.square().add(c);
        iter += 1;
    }

    iter
}

#[inline]
fn render_mandelbrot_bw_view_f128(
    buffer: &mut [u8],
    width: usize,
    height: usize,
    max_iter: u32,
    real_min: f128,
    real_max: f128,
    imag_top: f128,
    imag_bottom: f128,
) {
    let real_scale = if width > 1 {
        (real_max - real_min) / (width as f128 - 1.0f128)
    } else {
        0.0f128
    };
    let imag_scale = if height > 1 {
        (imag_bottom - imag_top) / (height as f128 - 1.0f128)
    } else {
        0.0f128
    };

    for y in 0..height {
        let imag = imag_top + imag_scale * y as f128;
        for x in 0..width {
            let real = real_min + real_scale * x as f128;
            let c = Complex128::new(real, imag);
            let depth = mandelbrot_escape_depth_f128(c, max_iter);
            let idx = y * width + x;
            buffer[idx] = if depth >= max_iter { 0 } else { 0xFF };
        }
    }
}

#[inline]
fn render_mandelbrot_rgb32_view_f128(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    max_iter: u32,
    real_min: f128,
    real_max: f128,
    imag_top: f128,
    imag_bottom: f128,
) {
    let real_scale = if width > 1 {
        (real_max - real_min) / (width as f128 - 1.0f128)
    } else {
        0.0f128
    };
    let imag_scale = if height > 1 {
        (imag_bottom - imag_top) / (height as f128 - 1.0f128)
    } else {
        0.0f128
    };

    let denom = max_iter.max(1);
    for y in 0..height {
        let imag = imag_top + imag_scale * y as f128;
        for x in 0..width {
            let real = real_min + real_scale * x as f128;
            let c = Complex128::new(real, imag);
            let depth = mandelbrot_escape_depth_f128(c, max_iter);
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

    render_mandelbrot_bw_view_f128(
        buffer, width, height, max_iter, -1.0f128, 1.0f128, -1.0f128, 1.0f128,
    );
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

    render_mandelbrot_rgb32_view_f128(
        buffer, width, height, max_iter, -1.0f128, 1.0f128, -1.0f128, 1.0f128,
    );
}

/// Renders a grayscale Mandelbrot image into an RGB32 buffer using an explicit complex-plane view.
///
/// The top row maps to `imag_top` and the bottom row maps to `imag_bottom`.
pub fn render_mandelbrot_rgb32_view(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    max_iter: u32,
    real_min: f64,
    real_max: f64,
    imag_top: f64,
    imag_bottom: f64,
) {
    if width == 0 || height == 0 {
        return;
    }
    let expected = width.saturating_mul(height);
    if buffer.len() != expected {
        return;
    }

    render_mandelbrot_rgb32_view_f128(
        buffer,
        width,
        height,
        max_iter,
        real_min as f128,
        real_max as f128,
        imag_top as f128,
        imag_bottom as f128,
    );
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
