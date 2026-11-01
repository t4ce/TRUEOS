//! Bounded static ICO subset: PNG frames and bottom-up BI_RGB 32-bit DIBs.
//! The largest supported directory frame is selected deterministically. No
//! cursor, palette, bitfield, compressed DIB or legacy screen-XOR rendering.
use alloc::{vec, vec::Vec};

const MAX_ENTRIES: usize = 256;
const MAX_DIMENSION: u32 = 256;
const MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IcoDecodeError {
    Invalid,
    Unsupported,
    LimitExceeded,
    DecodeFailed,
}
impl IcoDecodeError {
    pub(crate) const fn code(self) -> i32 {
        match self {
            Self::Invalid => -1,
            Self::Unsupported => -8,
            Self::LimitExceeded => -9,
            Self::DecodeFailed => -6,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameKind {
    Png,
    Dib32,
}

pub(crate) struct IconFrame<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    bytes: &'a [u8],
    kind: FrameKind,
}
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct DecodedIco {
    pub(crate) png_frame: bool,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

fn le16(bytes: &[u8], offset: usize) -> Result<u16, IcoDecodeError> {
    let bytes = bytes
        .get(offset..offset + 2)
        .ok_or(IcoDecodeError::Invalid)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}
fn le32(bytes: &[u8], offset: usize) -> Result<u32, IcoDecodeError> {
    let bytes = bytes
        .get(offset..offset + 4)
        .ok_or(IcoDecodeError::Invalid)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Inspect complete directory/frame bounds before allocating output pixels.
/// Unsupported entries can coexist with supported ones; malformed directory
/// ranges are rejected even when another entry could have decoded.
pub(crate) fn inspect_ico(bytes: &[u8]) -> Result<IconFrame<'_>, IcoDecodeError> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(IcoDecodeError::LimitExceeded);
    }
    if bytes.get(..4) != Some(b"\0\0\x01\0".as_slice()) {
        return Err(IcoDecodeError::Invalid);
    }
    let count = usize::from(le16(bytes, 4)?);
    if count == 0 {
        return Err(IcoDecodeError::Invalid);
    }
    if count > MAX_ENTRIES {
        return Err(IcoDecodeError::LimitExceeded);
    }
    let directory_end = 6 + count * 16;
    if directory_end > bytes.len() {
        return Err(IcoDecodeError::Invalid);
    }
    let mut best: Option<IconFrame<'_>> = None;
    let mut last_error = IcoDecodeError::Unsupported;
    for index in 0..count {
        let entry = &bytes[6 + index * 16..6 + (index + 1) * 16];
        if entry[3] != 0 {
            return Err(IcoDecodeError::Invalid);
        }
        let width = if entry[0] == 0 {
            256
        } else {
            u32::from(entry[0])
        };
        let height = if entry[1] == 0 {
            256
        } else {
            u32::from(entry[1])
        };
        let length = le32(entry, 8)? as usize;
        let offset = le32(entry, 12)? as usize;
        let end = offset.checked_add(length).ok_or(IcoDecodeError::Invalid)?;
        if offset < directory_end || length == 0 || end > bytes.len() {
            return Err(IcoDecodeError::Invalid);
        }
        match inspect_frame(&bytes[offset..end], width, height) {
            Ok(frame) => {
                let score = (frame.width * frame.height, frame.kind == FrameKind::Png);
                if best
                    .as_ref()
                    .is_none_or(|old| score > (old.width * old.height, old.kind == FrameKind::Png))
                {
                    best = Some(frame);
                }
            }
            Err(error) => last_error = error,
        }
    }
    best.ok_or(last_error)
}

fn inspect_frame(bytes: &[u8], width: u32, height: u32) -> Result<IconFrame<'_>, IcoDecodeError> {
    let kind = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        // Like Hull's plain PNG admission, require a complete static stream;
        // the existing PNG decoder subsequently checks CRCs and pixel coding.
        if bytes.len() < 33 || &bytes[8..16] != b"\0\0\0\rIHDR" {
            return Err(IcoDecodeError::Invalid);
        }
        let pw = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let ph = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if pw != width || ph != height || pw > MAX_DIMENSION || ph > MAX_DIMENSION {
            return Err(IcoDecodeError::Invalid);
        }
        let mut offset = 8usize;
        let mut ended = false;
        while offset < bytes.len() {
            let header = bytes
                .get(offset..offset + 8)
                .ok_or(IcoDecodeError::Invalid)?;
            let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
            let end = offset
                .checked_add(length)
                .and_then(|n| n.checked_add(12))
                .ok_or(IcoDecodeError::Invalid)?;
            if end > bytes.len() {
                return Err(IcoDecodeError::Invalid);
            }
            if matches!(&header[4..8], b"acTL" | b"fcTL" | b"fdAT") {
                return Err(IcoDecodeError::Unsupported);
            }
            if &header[4..8] == b"IHDR" && offset != 8 {
                return Err(IcoDecodeError::Invalid);
            }
            if &header[4..8] == b"IEND" {
                if length != 0 || end != bytes.len() {
                    return Err(IcoDecodeError::Invalid);
                }
                ended = true;
                break;
            }
            offset = end;
        }
        if !ended {
            return Err(IcoDecodeError::Invalid);
        }
        FrameKind::Png
    } else {
        if le32(bytes, 0)? != 40 {
            return Err(IcoDecodeError::Unsupported);
        }
        if bytes.len() < 40 {
            return Err(IcoDecodeError::Invalid);
        }
        let dw = le32(bytes, 4)? as i32;
        let dh = le32(bytes, 8)? as i32;
        if dw <= 0 || dh <= 0 || dh % 2 != 0 || dw as u32 != width || dh as u32 / 2 != height {
            return Err(IcoDecodeError::Invalid);
        }
        if le16(bytes, 12)? != 1
            || le16(bytes, 14)? != 32
            || le32(bytes, 16)? != 0
            || le32(bytes, 32)? != 0
        {
            return Err(IcoDecodeError::Unsupported);
        }
        let xor_bytes = width as usize * height as usize * 4;
        let mask_stride = (width as usize + 31) / 32 * 4;
        let expected = 40 + xor_bytes + mask_stride * height as usize;
        if bytes.len() != expected {
            return Err(IcoDecodeError::Invalid);
        }
        FrameKind::Dib32
    };
    Ok(IconFrame {
        width,
        height,
        bytes,
        kind,
    })
}

pub(crate) fn decode_ico_rgba(bytes: &[u8]) -> Result<DecodedIco, IcoDecodeError> {
    let frame = inspect_ico(bytes)?;
    if frame.kind == FrameKind::Png {
        let decoded =
            super::png::decode_png_rgba_bounded(frame.bytes, 256 * 256 * 4, MAX_DIMENSION)
                .map_err(|_| IcoDecodeError::DecodeFailed)?;
        if (decoded.width, decoded.height) != (frame.width, frame.height) {
            return Err(IcoDecodeError::Invalid);
        }
        return Ok(DecodedIco {
            png_frame: true,
            width: decoded.width,
            height: decoded.height,
            rgba: decoded.rgba,
        });
    }
    let width = frame.width as usize;
    let height = frame.height as usize;
    let source = &frame.bytes[40..40 + width * height * 4];
    let mask = &frame.bytes[40 + width * height * 4..];
    // Windows interprets all-zero alpha as legacy 0RGB; any nonzero alpha
    // selects ARGB for the entire frame, including its fully transparent cells.
    // See https://devblogs.microsoft.com/oldnewthing/20101021-00/?p=12483
    let has_alpha = source.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mask_stride = (width + 31) / 32 * 4;
    let mut rgba = vec![0; width * height * 4];
    for y in 0..height {
        let source_y = height - 1 - y;
        for x in 0..width {
            let src = (source_y * width + x) * 4;
            let dst = (y * width + x) * 4;
            rgba[dst..dst + 3].copy_from_slice(&[source[src + 2], source[src + 1], source[src]]);
            rgba[dst + 3] = if has_alpha {
                source[src + 3]
            } else if mask[source_y * mask_stride + x / 8] & (0x80 >> (x % 8)) != 0 {
                0
            } else {
                255
            };
        }
    }
    Ok(DecodedIco {
        png_frame: false,
        width: frame.width,
        height: frame.height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dib(width: u32, height: u32, pixels: &[u8], mask: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 40];
        bytes[..4].copy_from_slice(&40u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&width.to_le_bytes());
        bytes[8..12].copy_from_slice(&(height * 2).to_le_bytes());
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(pixels);
        bytes.extend_from_slice(mask);
        bytes
    }
    fn container(frames: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = vec![0; 6 + frames.len() * 16];
        bytes[2] = 1;
        bytes[4..6].copy_from_slice(&(frames.len() as u16).to_le_bytes());
        for (i, (w, h, data)) in frames.iter().enumerate() {
            let offset = bytes.len();
            let entry = &mut bytes[6 + i * 16..22 + i * 16];
            entry[0] = *w as u8;
            entry[1] = *h as u8;
            entry[4..6].copy_from_slice(&1u16.to_le_bytes());
            entry[6..8].copy_from_slice(&32u16.to_le_bytes());
            entry[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
            entry[12..16].copy_from_slice(&(offset as u32).to_le_bytes());
            bytes.extend_from_slice(data);
        }
        bytes
    }
    #[test]
    fn legacy_zero_alpha_uses_bottom_up_and_mask_with_padded_rows() {
        let pixels = [3, 2, 1, 0, 6, 5, 4, 0, 9, 8, 7, 0, 12, 11, 10, 0];
        let bytes = container(&[(2, 2, dib(2, 2, &pixels, &[0x80, 0, 0, 0, 0x40, 0, 0, 0]))]);
        let image = decode_ico_rgba(&bytes).unwrap();
        assert_eq!((image.width, image.height, image.png_frame), (2, 2, false));
        assert_eq!(
            image.rgba,
            [7, 8, 9, 255, 10, 11, 12, 0, 1, 2, 3, 0, 4, 5, 6, 255]
        );
    }
    #[test]
    fn argb_preserves_zero_and_partial_alpha_instead_of_using_legacy_mask() {
        let bytes = container(&[(
            2,
            1,
            dib(2, 1, &[3, 2, 1, 0, 6, 5, 4, 128], &[0xff, 0, 0, 0]),
        )]);
        let image = decode_ico_rgba(&bytes).unwrap();
        assert_eq!(image.rgba, [1, 2, 3, 0, 4, 5, 6, 128]);
    }
    #[test]
    fn common_dimensions_include_zero_directory_encoding_for_256() {
        for dimension in [16, 32, 256] {
            let pixels = [1, 2, 3, 255].repeat(dimension * dimension);
            let mask = vec![0; (dimension + 31) / 32 * 4 * dimension];
            let bytes = container(&[(
                dimension as u32,
                dimension as u32,
                dib(dimension as u32, dimension as u32, &pixels, &mask),
            )]);
            let image = decode_ico_rgba(&bytes).unwrap();
            assert_eq!(
                (image.width, image.height),
                (dimension as u32, dimension as u32)
            );
            assert_eq!(image.rgba.len(), dimension * dimension * 4);
            assert!(
                image
                    .rgba
                    .chunks_exact(4)
                    .all(|pixel| pixel == [3, 2, 1, 255])
            );
        }
    }
    #[test]
    fn selection_skips_unsupported_frames_and_prefers_largest_supported() {
        let valid = dib(2, 1, &[3, 2, 1, 255, 6, 5, 4, 255], &[0; 4]);
        let mut unsupported = valid.clone();
        unsupported[14..16].copy_from_slice(&8u16.to_le_bytes());
        let smaller = dib(1, 1, &[9, 8, 7, 255], &[0; 4]);
        let bytes = container(&[(2, 1, unsupported), (1, 1, smaller), (2, 1, valid)]);
        let image = decode_ico_rgba(&bytes).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
    }
    #[test]
    fn rejects_cursor_bad_ranges_mismatched_extents_and_truncated_mask() {
        let bytes = container(&[(1, 1, dib(1, 1, &[3, 2, 1, 255], &[0; 4]))]);
        for length in 0..bytes.len() {
            assert!(
                decode_ico_rgba(&bytes[..length]).is_err(),
                "accepted prefix {length}"
            );
        }
        let mut bad = bytes.clone();
        bad[2] = 2;
        assert!(decode_ico_rgba(&bad).is_err());
        let mut bad = bytes.clone();
        bad[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_ico_rgba(&bad).is_err());
        let mut bad = bytes.clone();
        bad[6] = 2;
        assert!(decode_ico_rgba(&bad).is_err());
        let mut bad = bytes.clone();
        bad[22 + 8..22 + 12].copy_from_slice(&3u32.to_le_bytes());
        assert!(decode_ico_rgba(&bad).is_err());
        let mut bad = bytes.clone();
        bad[4..6].copy_from_slice(&257u16.to_le_bytes());
        assert_eq!(decode_ico_rgba(&bad), Err(IcoDecodeError::LimitExceeded));
    }
}
