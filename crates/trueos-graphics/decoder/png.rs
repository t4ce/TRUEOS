use core3::io::Cursor;

use alloc::vec::Vec;

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
        rgba: super::png_decode_pool::expand_indexed_png_to_rgba(
            width, height, bit_depth, indexed, palette, trns,
        )?,
    })
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
    let rgba = super::png_decode_pool::expand_png_output_to_rgba(
        info.color_type,
        info.bit_depth,
        info.width,
        info.height,
        rgba,
    )?;

    Ok(DecodedPng {
        width: info.width,
        height: info.height,
        rgba,
    })
}
