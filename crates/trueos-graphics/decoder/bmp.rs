use alloc::vec;
use alloc::vec::Vec;

const FILE_HEADER_BYTES: usize = 14;
const INFO_HEADER_BYTES: usize = 40;
const BI_RGB: u32 = 0;
const MAX_DIMENSION: u32 = 8_192;
const MAX_RGBA_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedBmp {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BmpDecodeError {
    Invalid,
    Unsupported,
    LimitExceeded,
}

/// Decode the canonical browser-facing BMP subset into tightly packed RGBA8.
///
/// V1 accepts only uncompressed indexed 8-bit, 24-bit BGR, and 32-bit
/// BGRX/BGRA BITMAPINFOHEADER images. Both bottom-up and top-down row order are handled;
/// every source row is validated against its four-byte BMP stride before any
/// pixel is read.
pub fn decode_bmp_rgba(bytes: &[u8]) -> Result<DecodedBmp, BmpDecodeError> {
    if bytes.len() < FILE_HEADER_BYTES + INFO_HEADER_BYTES || &bytes[..2] != b"BM" {
        return Err(BmpDecodeError::Invalid);
    }
    let declared_file_size = le_u32(bytes, 2)? as usize;
    let data_offset = le_u32(bytes, 10)? as usize;
    let dib_size = le_u32(bytes, 14)? as usize;
    if dib_size < INFO_HEADER_BYTES
        || FILE_HEADER_BYTES.checked_add(dib_size).is_none()
        || declared_file_size > bytes.len()
        || data_offset < FILE_HEADER_BYTES + dib_size
        || data_offset >= declared_file_size
    {
        return Err(BmpDecodeError::Invalid);
    }

    let width_i = le_i32(bytes, 18)?;
    let height_i = le_i32(bytes, 22)?;
    let planes = le_u16(bytes, 26)?;
    let bits = le_u16(bytes, 28)?;
    let compression = le_u32(bytes, 30)?;
    let clr_used = le_u32(bytes, 46)?;
    if width_i <= 0 || height_i == 0 || planes != 1 {
        return Err(BmpDecodeError::Invalid);
    }
    if compression != BI_RGB || !matches!(bits, 8 | 24 | 32) {
        return Err(BmpDecodeError::Unsupported);
    }

    let width = width_i as u32;
    let height = height_i.unsigned_abs();
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(BmpDecodeError::LimitExceeded);
    }
    let palette_entries = if bits == 8 {
        let entries = if clr_used != 0 { clr_used } else { 1 << bits };
        if entries == 0 || entries > 256 {
            return Err(BmpDecodeError::Invalid);
        }
        entries as usize
    } else {
        0
    };
    let palette_bytes = palette_entries
        .checked_mul(4)
        .ok_or(BmpDecodeError::LimitExceeded)?;
    let palette_start = FILE_HEADER_BYTES + dib_size;
    let palette_end = palette_start
        .checked_add(palette_bytes)
        .ok_or(BmpDecodeError::LimitExceeded)?;
    if palette_end > data_offset || palette_end > declared_file_size || palette_end > bytes.len() {
        return Err(BmpDecodeError::Invalid);
    }
    let bytes_per_pixel = if bits == 8 { 1 } else { usize::from(bits / 8) };
    let row_bytes = (width as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or(BmpDecodeError::LimitExceeded)?;
    let row_stride = row_bytes
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or(BmpDecodeError::LimitExceeded)?;
    let source_bytes = row_stride
        .checked_mul(height as usize)
        .ok_or(BmpDecodeError::LimitExceeded)?;
    let source_end = data_offset
        .checked_add(source_bytes)
        .ok_or(BmpDecodeError::LimitExceeded)?;
    if source_end > declared_file_size || source_end > bytes.len() {
        return Err(BmpDecodeError::Invalid);
    }
    let rgba_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|len| *len <= MAX_RGBA_BYTES)
        .ok_or(BmpDecodeError::LimitExceeded)?;
    let mut rgba = vec![0u8; rgba_len];
    let top_down = height_i < 0;
    for destination_y in 0..height as usize {
        let source_y = if top_down {
            destination_y
        } else {
            height as usize - 1 - destination_y
        };
        let source_row = data_offset + source_y * row_stride;
        let destination_row = destination_y * width as usize * 4;
        for x in 0..width as usize {
            let source = source_row + x * bytes_per_pixel;
            let destination = destination_row + x * 4;
            if bits == 8 {
                let index = usize::from(bytes[source]);
                if index >= palette_entries {
                    return Err(BmpDecodeError::Invalid);
                }
                let entry = palette_start + index * 4;
                rgba[destination] = bytes[entry + 2];
                rgba[destination + 1] = bytes[entry + 1];
                rgba[destination + 2] = bytes[entry];
                rgba[destination + 3] = 255;
            } else {
                rgba[destination] = bytes[source + 2];
                rgba[destination + 1] = bytes[source + 1];
                rgba[destination + 2] = bytes[source];
                // BI_RGB 32-bit alpha is not consistently authored. Preserve it
                // when non-zero, otherwise treat the conventional BGRX byte as
                // opaque so common Windows assets do not become transparent.
                rgba[destination + 3] = if bytes_per_pixel == 4 && bytes[source + 3] != 0 {
                    bytes[source + 3]
                } else {
                    255
                };
            }
        }
    }
    Ok(DecodedBmp {
        width,
        height,
        rgba,
    })
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, BmpDecodeError> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or(BmpDecodeError::Invalid)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, BmpDecodeError> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(BmpDecodeError::Invalid)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn le_i32(bytes: &[u8], offset: usize) -> Result<i32, BmpDecodeError> {
    le_u32(bytes, offset).map(|value| i32::from_le_bytes(value.to_le_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp24(width: i32, height: i32, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; 54];
        bytes[0..2].copy_from_slice(b"BM");
        let size = 54 + pixels.len();
        bytes[2..6].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&width.to_le_bytes());
        bytes[22..26].copy_from_slice(&height.to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
        bytes.extend_from_slice(pixels);
        bytes
    }

    fn bmp8(width: i32, height: i32, clr_used: u32, palette: &[[u8; 4]], pixels: &[u8]) -> Vec<u8> {
        let data_offset = 14 + 40 + palette.len() * 4;
        let mut bytes = vec![0u8; data_offset];
        bytes[0..2].copy_from_slice(b"BM");
        let size = data_offset + pixels.len();
        bytes[2..6].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&(data_offset as u32).to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&width.to_le_bytes());
        bytes[22..26].copy_from_slice(&height.to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
        bytes[46..50].copy_from_slice(&clr_used.to_le_bytes());
        for (index, entry) in palette.iter().enumerate() {
            bytes[54 + index * 4..58 + index * 4].copy_from_slice(entry);
        }
        bytes.extend_from_slice(pixels);
        bytes
    }

    #[test]
    fn decodes_bottom_up_bgr_with_stride_padding() {
        let bytes = bmp24(
            1,
            2,
            &[
                30, 20, 10, 0, // bottom row
                60, 50, 40, 0, // top row
            ],
        );
        let decoded = decode_bmp_rgba(&bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (1, 2));
        assert_eq!(decoded.rgba, [40, 50, 60, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn rejects_compressed_or_truncated_inputs() {
        let mut compressed = bmp24(1, 1, &[3, 2, 1, 0]);
        compressed[30..34].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(decode_bmp_rgba(&compressed), Err(BmpDecodeError::Unsupported));
        let truncated = bmp24(1, 1, &[3, 2]);
        assert_eq!(decode_bmp_rgba(&truncated), Err(BmpDecodeError::Invalid));
    }

    #[test]
    fn decodes_8bit_bottom_up_palette_and_padding() {
        let bytes = bmp8(
            3,
            2,
            2,
            &[[10, 20, 30, 99], [40, 50, 60, 88]],
            &[
                1, 0, 1, 0, // bottom row, one padding byte
                0, 1, 0, 0, // top row, one padding byte
            ],
        );
        let decoded = decode_bmp_rgba(&bytes).unwrap();
        assert_eq!(
            decoded.rgba,
            [
                30, 20, 10, 255, 60, 50, 40, 255, 30, 20, 10, 255, 60, 50, 40, 255, 30, 20, 10,
                255, 60, 50, 40, 255
            ]
        );
    }

    #[test]
    fn decodes_8bit_top_down_and_default_palette() {
        let mut palette = [[0u8; 4]; 256];
        palette[0] = [1, 2, 3, 200];
        palette[1] = [4, 5, 6, 201];
        let bytes = bmp8(2, -2, 0, &palette, &[0, 1, 0, 0, 1, 0, 0, 0]);
        let decoded = decode_bmp_rgba(&bytes).unwrap();
        assert_eq!(decoded.rgba, [3, 2, 1, 255, 6, 5, 4, 255, 6, 5, 4, 255, 3, 2, 1, 255]);
    }

    #[test]
    fn rejects_palette_index_outside_explicit_palette() {
        let bytes = bmp8(1, 1, 1, &[[1, 2, 3, 0]], &[1, 0, 0, 0]);
        assert_eq!(decode_bmp_rgba(&bytes), Err(BmpDecodeError::Invalid));
    }

    #[test]
    fn rejects_truncated_palette_and_pixel_rows() {
        let mut palette = [[0u8; 4]; 2];
        palette[1] = [1, 2, 3, 0];
        let bytes = bmp8(1, 1, 2, &palette, &[1, 0, 0, 0]);
        let mut truncated_palette = bytes.clone();
        truncated_palette.drain(55..59);
        assert_eq!(decode_bmp_rgba(&truncated_palette), Err(BmpDecodeError::Invalid));
        let truncated_pixels = &bytes[..bytes.len() - 1];
        assert_eq!(decode_bmp_rgba(truncated_pixels), Err(BmpDecodeError::Invalid));
    }

    #[test]
    fn accepts_trailing_input_padding() {
        let mut bytes = bmp8(1, 1, 1, &[[9, 8, 7, 0]], &[0, 0, 0, 0]);
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        assert_eq!(decode_bmp_rgba(&bytes).unwrap().rgba, [7, 8, 9, 255]);
    }
}
