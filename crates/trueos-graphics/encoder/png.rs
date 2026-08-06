//! Minimal PNG encoder for RGB and RGBA output.

use alloc::vec::Vec;

use crc32fast::Hasher as Crc32;
use miniz_oxide::deflate::compress_to_vec_zlib;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PngEncodeError {
    InvalidDimensions,
    BufferTooSmall,
    DimensionTooLarge,
}

impl PngEncodeError {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn code(self) -> i32 {
        match self {
            Self::InvalidDimensions => -20,
            Self::BufferTooSmall => -21,
            Self::DimensionTooLarge => -22,
        }
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn encode_rgb_u32_png(
    width: u32,
    height: u32,
    pixels: &[u32],
) -> Result<Vec<u8>, PngEncodeError> {
    let width_usize = usize::try_from(width).map_err(|_| PngEncodeError::DimensionTooLarge)?;
    let height_usize = usize::try_from(height).map_err(|_| PngEncodeError::DimensionTooLarge)?;
    if width_usize == 0 || height_usize == 0 {
        return Err(PngEncodeError::InvalidDimensions);
    }
    let pixel_count = checked_pixel_count(width_usize, height_usize)?;
    if pixels.len() < pixel_count {
        return Err(PngEncodeError::BufferTooSmall);
    }

    let mut filtered = Vec::with_capacity(checked_filtered_len(width_usize, height_usize, 3)?);
    for y in 0..height_usize {
        let row = &pixels[y * width_usize..(y + 1) * width_usize];
        filtered.push(0);
        for &pixel in row {
            filtered.push(((pixel >> 16) & 0xFF) as u8);
            filtered.push(((pixel >> 8) & 0xFF) as u8);
            filtered.push((pixel & 0xFF) as u8);
        }
    }

    encode_filtered_png(width, height, 2, filtered.as_slice())
}

pub(crate) fn encode_rgb8_png(
    width: u32,
    height: u32,
    rgb: &[u8],
    stride_bytes: usize,
) -> Result<Vec<u8>, PngEncodeError> {
    let width_usize = usize::try_from(width).map_err(|_| PngEncodeError::DimensionTooLarge)?;
    let height_usize = usize::try_from(height).map_err(|_| PngEncodeError::DimensionTooLarge)?;
    if width_usize == 0 || height_usize == 0 {
        return Err(PngEncodeError::InvalidDimensions);
    }
    let row_bytes = width_usize
        .checked_mul(3)
        .ok_or(PngEncodeError::DimensionTooLarge)?;
    if stride_bytes < row_bytes {
        return Err(PngEncodeError::BufferTooSmall);
    }
    let need = if height_usize == 0 {
        0
    } else {
        stride_bytes
            .checked_mul(height_usize.saturating_sub(1))
            .and_then(|base| base.checked_add(row_bytes))
            .ok_or(PngEncodeError::DimensionTooLarge)?
    };
    if rgb.len() < need {
        return Err(PngEncodeError::BufferTooSmall);
    }

    let mut filtered = Vec::with_capacity(checked_filtered_len(width_usize, height_usize, 3)?);
    for y in 0..height_usize {
        let row_start = y
            .checked_mul(stride_bytes)
            .ok_or(PngEncodeError::DimensionTooLarge)?;
        filtered.push(0);
        filtered.extend_from_slice(&rgb[row_start..row_start + row_bytes]);
    }

    encode_filtered_png(width, height, 2, filtered.as_slice())
}

pub(crate) fn encode_rgba8_png(
    width: u32,
    height: u32,
    rgba: &[u8],
    stride_bytes: usize,
) -> Result<Vec<u8>, PngEncodeError> {
    let width_usize = usize::try_from(width).map_err(|_| PngEncodeError::DimensionTooLarge)?;
    let height_usize = usize::try_from(height).map_err(|_| PngEncodeError::DimensionTooLarge)?;
    if width_usize == 0 || height_usize == 0 {
        return Err(PngEncodeError::InvalidDimensions);
    }
    let row_bytes = width_usize
        .checked_mul(4)
        .ok_or(PngEncodeError::DimensionTooLarge)?;
    if stride_bytes < row_bytes {
        return Err(PngEncodeError::BufferTooSmall);
    }
    let need = stride_bytes
        .checked_mul(height_usize.saturating_sub(1))
        .and_then(|base| base.checked_add(row_bytes))
        .ok_or(PngEncodeError::DimensionTooLarge)?;
    if rgba.len() < need {
        return Err(PngEncodeError::BufferTooSmall);
    }

    let mut filtered = Vec::with_capacity(checked_filtered_len(width_usize, height_usize, 4)?);
    for y in 0..height_usize {
        let row_start = y
            .checked_mul(stride_bytes)
            .ok_or(PngEncodeError::DimensionTooLarge)?;
        filtered.push(0);
        filtered.extend_from_slice(&rgba[row_start..row_start + row_bytes]);
    }

    encode_filtered_png(width, height, 6, filtered.as_slice())
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn checked_pixel_count(width: usize, height: usize) -> Result<usize, PngEncodeError> {
    width
        .checked_mul(height)
        .ok_or(PngEncodeError::DimensionTooLarge)
}

fn checked_filtered_len(
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
) -> Result<usize, PngEncodeError> {
    let row = width
        .checked_mul(bytes_per_pixel)
        .and_then(|bytes| bytes.checked_add(1))
        .ok_or(PngEncodeError::DimensionTooLarge)?;
    row.checked_mul(height)
        .ok_or(PngEncodeError::DimensionTooLarge)
}

fn encode_filtered_png(
    width: u32,
    height: u32,
    color_type: u8,
    filtered_pixels: &[u8],
) -> Result<Vec<u8>, PngEncodeError> {
    let compressed = compress_to_vec_zlib(filtered_pixels, 6);
    let mut png = Vec::with_capacity(
        8usize
            .saturating_add(25)
            .saturating_add(12)
            .saturating_add(compressed.len())
            .saturating_add(12),
    );
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = Vec::with_capacity(13);
    push_be_u32(&mut ihdr, width);
    push_be_u32(&mut ihdr, height);
    ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]);
    append_png_chunk(&mut png, b"IHDR", ihdr.as_slice());
    append_png_chunk(&mut png, b"IDAT", compressed.as_slice());
    append_png_chunk(&mut png, b"IEND", &[]);
    Ok(png)
}

fn push_be_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn append_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    push_be_u32(out, data.len() as u32);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);

    let mut crc = Crc32::new();
    crc.update(kind);
    crc.update(data);
    push_be_u32(out, crc.finalize());
}
