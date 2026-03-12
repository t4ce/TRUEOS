use alloc::vec::Vec;
use core2::io::Cursor;

pub struct DecodedPng {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

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

    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(PngDecodeError::Unsupported);
    }

    Ok(DecodedPng {
        width: info.width,
        height: info.height,
        rgba,
    })
}
