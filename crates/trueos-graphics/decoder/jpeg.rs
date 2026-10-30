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
    decode_jpeg_rgba_with_limit(bytes, None)
}

pub(crate) fn decode_jpeg_rgba_bounded(
    bytes: &[u8], max_bytes: usize, max_dimension: u32,
) -> Result<DecodedJpeg, JpegDecodeError> {
    decode_jpeg_rgba_with_limit(bytes, Some((max_bytes, max_dimension)))
}

fn decode_jpeg_rgba_with_limit(
    bytes: &[u8], limit: Option<(usize, u32)>,
) -> Result<DecodedJpeg, JpegDecodeError> {
    let mut options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_use_unsafe(true);
    if let Some((_, max_dimension)) = limit {
        options = options.set_max_width(max_dimension as usize)
            .set_max_height(max_dimension as usize).jpeg_set_max_scans(64);
    }
    if !JPEG_DECODE_GATE_LOGGED.swap(true, Ordering::AcqRel) {
        let layout = super::jpeg_layout::classify_jpeg_layout(bytes);
        crate::log!(
            "jpeg: zune gate use_unsafe={} use_avx2={} bytes={} layout={} sof={}x{} components={} prove=logo-zune-gate\n",
            options.use_unsafe(),
            options.use_avx2(),
            bytes.len(),
            layout.sampling.as_str(),
            layout.width,
            layout.height,
            layout.components,
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
    if let Some((max_bytes, max_dimension)) = limit {
        if width > max_dimension || height > max_dimension
            || (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)).is_none_or(|n| n > max_bytes)
        { return Err(JpegDecodeError::Unsupported); }
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
