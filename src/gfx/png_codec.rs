use alloc::vec::Vec;
use core2::io::Cursor;

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
