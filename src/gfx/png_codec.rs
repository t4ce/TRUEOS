use alloc::vec::Vec;
use core3::io::Cursor;

pub struct DecodedPng {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub enum PngDecodeError {
    Invalid,
    Unsupported,
    DecodeFailed,
}

impl PngDecodeError {
    pub const fn code(&self) -> i32 {
        match self {
            Self::Invalid => -7,
            Self::Unsupported => -8,
            Self::DecodeFailed => -9,
        }
    }
}

fn indexed_row_bytes(width: u32, bit_depth: png::BitDepth) -> Result<usize, PngDecodeError> {
    let bits_per_pixel = match bit_depth {
        png::BitDepth::One => 1usize,
        png::BitDepth::Two => 2usize,
        png::BitDepth::Four => 4usize,
        png::BitDepth::Eight => 8usize,
        png::BitDepth::Sixteen => return Err(PngDecodeError::Unsupported),
    };
    let width = width as usize;
    Ok(width.saturating_mul(bits_per_pixel).saturating_add(7) / 8)
}

fn indexed_sample(bits: &[u8], bit_depth: png::BitDepth, x: usize) -> Result<u8, PngDecodeError> {
    match bit_depth {
        png::BitDepth::One => {
            let byte = *bits.get(x / 8).ok_or(PngDecodeError::DecodeFailed)?;
            Ok((byte >> (7 - (x % 8))) & 0x01)
        }
        png::BitDepth::Two => {
            let byte = *bits.get(x / 4).ok_or(PngDecodeError::DecodeFailed)?;
            Ok((byte >> (6 - ((x % 4) * 2))) & 0x03)
        }
        png::BitDepth::Four => {
            let byte = *bits.get(x / 2).ok_or(PngDecodeError::DecodeFailed)?;
            Ok(if (x & 1) == 0 { byte >> 4 } else { byte & 0x0F })
        }
        png::BitDepth::Eight => bits.get(x).copied().ok_or(PngDecodeError::DecodeFailed),
        png::BitDepth::Sixteen => Err(PngDecodeError::Unsupported),
    }
}

fn expand_indexed_png_to_rgba(
    width: u32,
    height: u32,
    bit_depth: png::BitDepth,
    packed_indices: &[u8],
    palette: &[u8],
    trns: Option<&[u8]>,
) -> Result<Vec<u8>, PngDecodeError> {
    if palette.len() < 3 {
        return Err(PngDecodeError::Invalid);
    }

    let row_bytes = indexed_row_bytes(width, bit_depth)?;
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let expected = row_bytes.saturating_mul(height as usize);
    if packed_indices.len() < expected {
        return Err(PngDecodeError::DecodeFailed);
    }

    let mut rgba = Vec::with_capacity(pixel_count.saturating_mul(4));
    for y in 0..height as usize {
        let row_start = y.saturating_mul(row_bytes);
        let row = &packed_indices[row_start..row_start + row_bytes];
        for x in 0..width as usize {
            let sample = indexed_sample(row, bit_depth, x)? as usize;
            let palette_idx = sample.saturating_mul(3);
            let rgb = palette
                .get(palette_idx..palette_idx + 3)
                .ok_or(PngDecodeError::DecodeFailed)?;
            let alpha = trns
                .and_then(|table| table.get(sample).copied())
                .unwrap_or(0xFF);
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    Ok(rgba)
}

fn decode_indexed_png_rgba(bytes: &[u8]) -> Result<DecodedPng, PngDecodeError> {
    let cursor = Cursor::new(bytes);
    let mut decoder = png::Decoder::new(cursor);
    decoder.set_transformations(png::Transformations::STRIP_16);

    let mut reader = decoder.read_info().map_err(|_| PngDecodeError::Invalid)?;
    let info = reader.info();
    let width = info.width;
    let height = info.height;
    let bit_depth = info.bit_depth;
    let palette = info
        .palette
        .as_deref()
        .ok_or(PngDecodeError::Invalid)?
        .to_vec();
    let trns = info.trns.as_deref().map(|table| table.to_vec());

    let out_len = reader
        .output_buffer_size()
        .ok_or(PngDecodeError::DecodeFailed)?;
    let mut indexed = vec![0u8; out_len];
    let frame = reader
        .next_frame(&mut indexed)
        .map_err(|_| PngDecodeError::DecodeFailed)?;
    indexed.truncate(frame.buffer_size());

    Ok(DecodedPng {
        width,
        height,
        rgba: expand_indexed_png_to_rgba(
            width,
            height,
            bit_depth,
            indexed.as_slice(),
            palette.as_slice(),
            trns.as_deref(),
        )?,
    })
}

fn expand_png_output_to_rgba(
    color_type: png::ColorType,
    bit_depth: png::BitDepth,
    pixels: &[u8],
) -> Result<Vec<u8>, PngDecodeError> {
    if bit_depth != png::BitDepth::Eight {
        return Err(PngDecodeError::Unsupported);
    }

    match color_type {
        png::ColorType::Rgba => Ok(pixels.to_vec()),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((pixels.len() / 3) * 4);
            for chunk in pixels.chunks_exact(3) {
                out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 0xFF]);
            }
            Ok(out)
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity(pixels.len() * 4);
            for &v in pixels {
                out.extend_from_slice(&[v, v, v, 0xFF]);
            }
            Ok(out)
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity((pixels.len() / 2) * 4);
            for chunk in pixels.chunks_exact(2) {
                out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            Ok(out)
        }
        png::ColorType::Indexed => Err(PngDecodeError::Unsupported),
    }
}

pub fn decode_png_rgba(bytes: &[u8]) -> Result<DecodedPng, PngDecodeError> {
    let mut probe = png::Decoder::new(Cursor::new(bytes));
    let is_indexed = probe
        .read_header_info()
        .map_err(|_| PngDecodeError::Invalid)?
        .color_type
        == png::ColorType::Indexed;
    if is_indexed {
        return decode_indexed_png_rgba(bytes);
    }

    let cursor = Cursor::new(bytes);
    let mut decoder = png::Decoder::new(cursor);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);

    let mut reader = decoder.read_info().map_err(|_| PngDecodeError::Invalid)?;
    let out_len = reader
        .output_buffer_size()
        .ok_or(PngDecodeError::DecodeFailed)?;
    let mut rgba = vec![0u8; out_len];

    let info = reader
        .next_frame(&mut rgba)
        .map_err(|_| PngDecodeError::DecodeFailed)?;
    rgba.truncate(info.buffer_size());
    let rgba = expand_png_output_to_rgba(info.color_type, info.bit_depth, &rgba)?;

    Ok(DecodedPng {
        width: info.width,
        height: info.height,
        rgba,
    })
}
