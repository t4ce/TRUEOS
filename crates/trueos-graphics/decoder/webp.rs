//! Static WebP admission and the existing pure-Rust decoder, with no host codec.
use alloc::vec::Vec;
use core3::io::Cursor;

const MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;
const MAX_RGBA_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIMENSION: u32 = 8192;
const MAX_CHUNKS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebpDecodeError {
    Invalid,
    Unsupported,
    LimitExceeded,
    DecodeFailed,
}
impl WebpDecodeError {
    pub(crate) const fn code(self) -> i32 {
        match self {
            Self::Invalid => -1,
            Self::Unsupported => -8,
            Self::LimitExceeded => -9,
            Self::DecodeFailed => -6,
        }
    }
}
pub(crate) struct DecodedWebp {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}
fn le24(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}
fn extent(width: u32, height: u32) -> Result<(u32, u32), WebpDecodeError> {
    if width == 0 || height == 0 {
        return Err(WebpDecodeError::Invalid);
    }
    if width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) * 4 > MAX_RGBA_BYTES as u64
    {
        return Err(WebpDecodeError::LimitExceeded);
    }
    Ok((width, height))
}

/// Validate complete RIFF/chunk ranges and every compressed frame's dimensions
/// before constructing a decoder. Animation and mismatched VP8X/frame sizes
/// cannot allocate a second, unadmitted pixel extent. These are source/output
/// admission bounds, not a promise of exact total decoder scratch-heap usage.
pub(crate) fn inspect_webp(bytes: &[u8]) -> Result<(u32, u32), WebpDecodeError> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(WebpDecodeError::LimitExceeded);
    }
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(WebpDecodeError::Invalid);
    }
    let length = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if length < 4 || length.checked_add(8) != Some(bytes.len()) {
        return Err(WebpDecodeError::Invalid);
    }
    let mut offset = 12usize;
    let mut count = 0;
    let mut canvas = None;
    let mut frame = None;
    let mut alpha = false;
    while offset < bytes.len() {
        count += 1;
        if count > MAX_CHUNKS {
            return Err(WebpDecodeError::LimitExceeded);
        }
        let header = bytes
            .get(offset..offset + 8)
            .ok_or(WebpDecodeError::Invalid)?;
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(8)
            .and_then(|n| n.checked_add(length))
            .ok_or(WebpDecodeError::Invalid)?;
        let next = end
            .checked_add(length & 1)
            .ok_or(WebpDecodeError::Invalid)?;
        if next > bytes.len() {
            return Err(WebpDecodeError::Invalid);
        }
        let data = &bytes[offset + 8..end];
        match &header[..4] {
            b"ANIM" | b"ANMF" => return Err(WebpDecodeError::Unsupported),
            b"VP8X" => {
                if offset != 12
                    || canvas.is_some()
                    || frame.is_some()
                    || data.len() != 10
                    || data[0] & 0xc1 != 0
                    || data[1..4] != [0, 0, 0]
                {
                    return Err(WebpDecodeError::Invalid);
                }
                if data[0] & 2 != 0 {
                    return Err(WebpDecodeError::Unsupported);
                }
                canvas = Some(extent(le24(&data[4..7]) + 1, le24(&data[7..10]) + 1)?);
            }
            b"VP8 " => {
                if frame.is_some() || data.len() < 10 || data[3..6] != [0x9d, 0x01, 0x2a] {
                    return Err(WebpDecodeError::Invalid);
                }
                let tag = le24(&data[..3]);
                if tag & 1 != 0 || (tag >> 1) & 7 > 3 {
                    return Err(WebpDecodeError::Unsupported);
                }
                if tag & 16 == 0 || (tag >> 5) as usize > data.len() - 10 {
                    return Err(WebpDecodeError::Invalid);
                }
                let width = u16::from_le_bytes([data[6], data[7]]) & 0x3fff;
                let height = u16::from_le_bytes([data[8], data[9]]) & 0x3fff;
                frame = Some(extent(u32::from(width), u32::from(height))?);
            }
            b"VP8L" => {
                if frame.is_some() || alpha || data.len() < 5 || data[0] != 0x2f {
                    return Err(WebpDecodeError::Invalid);
                }
                let bits = u32::from_le_bytes(data[1..5].try_into().unwrap());
                if bits >> 29 != 0 {
                    return Err(WebpDecodeError::Unsupported);
                }
                frame = Some(extent((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)?);
            }
            b"ALPH" => {
                if canvas.is_none() || frame.is_some() || alpha || data.is_empty() {
                    return Err(WebpDecodeError::Invalid);
                }
                alpha = true;
            }
            _ => {}
        }
        offset = next;
    }
    let frame = frame.ok_or(WebpDecodeError::Invalid)?;
    if canvas.is_some_and(|canvas| canvas != frame) {
        return Err(WebpDecodeError::Invalid);
    }
    Ok(frame)
}

pub(crate) fn decode_webp_rgba(bytes: &[u8]) -> Result<DecodedWebp, WebpDecodeError> {
    let (width, height) = inspect_webp(bytes)?;
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))
        .map_err(|_| WebpDecodeError::DecodeFailed)?;
    if decoder.dimensions() != (width, height) || decoder.is_animated() {
        return Err(WebpDecodeError::Invalid);
    }
    decoder.set_memory_limit(MAX_RGBA_BYTES);
    let rgba_len = width as usize * height as usize * 4;
    let raw_len = decoder
        .output_buffer_size()
        .filter(|length| *length <= rgba_len)
        .ok_or(WebpDecodeError::LimitExceeded)?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(rgba_len)
        .map_err(|_| WebpDecodeError::LimitExceeded)?;
    rgba.resize(rgba_len, 0);
    decoder
        .read_image(&mut rgba[..raw_len])
        .map_err(|_| WebpDecodeError::DecodeFailed)?;
    if !decoder.has_alpha() {
        if raw_len != width as usize * height as usize * 3 {
            return Err(WebpDecodeError::Invalid);
        }
        // Expand RGB backwards inside the final RGBA allocation, retaining no
        // second whole-image output buffer in this adapter.
        for index in (0..width as usize * height as usize).rev() {
            let src = index * 3;
            let dst = index * 4;
            let pixel = [rgba[src], rgba[src + 1], rgba[src + 2], 255];
            rgba[dst..dst + 4].copy_from_slice(&pixel);
        }
    } else if raw_len != rgba_len {
        return Err(WebpDecodeError::Invalid);
    }
    Ok(DecodedWebp {
        width,
        height,
        rgba,
    })
}
