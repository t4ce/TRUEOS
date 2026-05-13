use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

static JPEG_DECODE_GATE_LOGGED: AtomicBool = AtomicBool::new(false);

pub struct DecodedJpeg {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub enum JpegDecodeError {
    Invalid,
    Unsupported,
    DecodeFailed,
}

impl JpegDecodeError {
    pub const fn code(&self) -> i32 {
        match self {
            Self::Invalid => -7,
            Self::Unsupported => -8,
            Self::DecodeFailed => -9,
        }
    }
}

pub fn decode_jpeg_rgba(bytes: &[u8]) -> Result<DecodedJpeg, JpegDecodeError> {
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_use_unsafe(true);
    if !JPEG_DECODE_GATE_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log!(
            "jpeg: zune gate use_unsafe={} use_avx2={} bytes={} prove=logo-zune-gate\n",
            options.use_unsafe(),
            options.use_avx2(),
            bytes.len(),
        );
    }
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|_| JpegDecodeError::Invalid)?;
    let info = decoder.info().ok_or(JpegDecodeError::Invalid)?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    if width == 0 || height == 0 {
        return Err(JpegDecodeError::Invalid);
    }

    let rgba = decoder
        .decode()
        .map_err(|_| JpegDecodeError::DecodeFailed)?;
    if rgba.len()
        != (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    {
        return Err(JpegDecodeError::Unsupported);
    }

    Ok(DecodedJpeg {
        width,
        height,
        rgba,
    })
}
