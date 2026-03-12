use alloc::vec::Vec;

use miniz_oxide::inflate::decompress_to_vec_zlib;

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

const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[inline]
fn be_u32(bytes: &[u8]) -> Option<u32> {
    let src = bytes.get(..4)?;
    Some(u32::from_be_bytes([src[0], src[1], src[2], src[3]]))
}

#[inline]
fn be_u16(bytes: &[u8]) -> Option<u16> {
    let src = bytes.get(..2)?;
    Some(u16::from_be_bytes([src[0], src[1]]))
}

#[inline]
fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let a = a as i32;
    let b = b as i32;
    let c = c as i32;
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

fn unfilter_scanlines(
    filtered: &[u8],
    width: usize,
    height: usize,
    bpp: usize,
) -> Result<Vec<u8>, PngDecodeError> {
    let row_stride = width.checked_mul(bpp).ok_or(PngDecodeError::DecodeFailed)?;
    let expected = height
        .checked_mul(row_stride + 1)
        .ok_or(PngDecodeError::DecodeFailed)?;
    if filtered.len() < expected {
        return Err(PngDecodeError::DecodeFailed);
    }

    let mut out = vec![0u8; height * row_stride];
    for y in 0..height {
        let src_row = y * (row_stride + 1);
        let dst_row = y * row_stride;
        let filter = filtered[src_row];
        let src = &filtered[src_row + 1..src_row + 1 + row_stride];
        let (prev_rows, tail) = out.split_at_mut(dst_row);
        let dst = &mut tail[..row_stride];
        let prev = if y == 0 {
            None
        } else {
            Some(&prev_rows[dst_row - row_stride..dst_row])
        };

        match filter {
            0 => dst.copy_from_slice(src),
            1 => {
                for i in 0..row_stride {
                    let left = if i >= bpp { dst[i - bpp] } else { 0 };
                    dst[i] = src[i].wrapping_add(left);
                }
            }
            2 => {
                for i in 0..row_stride {
                    let up = prev.map(|row| row[i]).unwrap_or(0);
                    dst[i] = src[i].wrapping_add(up);
                }
            }
            3 => {
                for i in 0..row_stride {
                    let left = if i >= bpp { dst[i - bpp] } else { 0 };
                    let up = prev.map(|row| row[i]).unwrap_or(0);
                    let avg = ((left as u16 + up as u16) / 2) as u8;
                    dst[i] = src[i].wrapping_add(avg);
                }
            }
            4 => {
                for i in 0..row_stride {
                    let left = if i >= bpp { dst[i - bpp] } else { 0 };
                    let up = prev.map(|row| row[i]).unwrap_or(0);
                    let up_left = if i >= bpp {
                        prev.map(|row| row[i - bpp]).unwrap_or(0)
                    } else {
                        0
                    };
                    dst[i] = src[i].wrapping_add(paeth_predictor(left, up, up_left));
                }
            }
            _ => return Err(PngDecodeError::Unsupported),
        }
    }

    Ok(out)
}

pub fn decode_png_rgba(bytes: &[u8]) -> Result<DecodedPng, PngDecodeError> {
    if bytes.len() < PNG_SIG.len() || &bytes[..PNG_SIG.len()] != PNG_SIG {
        return Err(PngDecodeError::Invalid);
    }

    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut compression = 0u8;
    let mut filter = 0u8;
    let mut interlace = 0u8;
    let mut idat = Vec::new();
    let mut palette = Vec::new();
    let mut trns = Vec::new();
    let mut saw_ihdr = false;

    let mut off = PNG_SIG.len();
    while off + 12 <= bytes.len() {
        let len = be_u32(&bytes[off..off + 4]).ok_or(PngDecodeError::Invalid)? as usize;
        let kind = &bytes[off + 4..off + 8];
        let data_start = off + 8;
        let data_end = data_start.checked_add(len).ok_or(PngDecodeError::Invalid)?;
        let chunk_end = data_end.checked_add(4).ok_or(PngDecodeError::Invalid)?;
        if chunk_end > bytes.len() {
            return Err(PngDecodeError::Invalid);
        }
        let data = &bytes[data_start..data_end];

        match kind {
            b"IHDR" => {
                if len != 13 || saw_ihdr {
                    return Err(PngDecodeError::Invalid);
                }
                width = be_u32(&data[0..4]).ok_or(PngDecodeError::Invalid)?;
                height = be_u32(&data[4..8]).ok_or(PngDecodeError::Invalid)?;
                bit_depth = data[8];
                color_type = data[9];
                compression = data[10];
                filter = data[11];
                interlace = data[12];
                saw_ihdr = true;
            }
            b"PLTE" => palette.extend_from_slice(data),
            b"tRNS" => trns.extend_from_slice(data),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }

        off = chunk_end;
    }

    if !saw_ihdr || width == 0 || height == 0 || idat.is_empty() {
        return Err(PngDecodeError::Invalid);
    }
    if compression != 0 || filter != 0 || interlace != 0 || bit_depth != 8 {
        return Err(PngDecodeError::Unsupported);
    }

    let samples_per_pixel = match color_type {
        0 => 1usize,
        2 => 3usize,
        3 => 1usize,
        4 => 2usize,
        6 => 4usize,
        _ => return Err(PngDecodeError::Unsupported),
    };
    let width_usize = width as usize;
    let height_usize = height as usize;
    let row_bpp = samples_per_pixel;
    let inflated = decompress_to_vec_zlib(&idat).map_err(|_| PngDecodeError::DecodeFailed)?;
    let raw = unfilter_scanlines(&inflated, width_usize, height_usize, row_bpp)?;

    let pixel_count = width_usize
        .checked_mul(height_usize)
        .ok_or(PngDecodeError::DecodeFailed)?;
    let mut rgba = vec![0u8; pixel_count * 4];

    match color_type {
        0 => {
            let transparent = if trns.len() >= 2 {
                be_u16(&trns[..2]).map(|v| v as u8)
            } else {
                None
            };
            for i in 0..pixel_count {
                let gray = raw[i];
                let dst = i * 4;
                rgba[dst] = gray;
                rgba[dst + 1] = gray;
                rgba[dst + 2] = gray;
                rgba[dst + 3] = if transparent == Some(gray) { 0 } else { 255 };
            }
        }
        2 => {
            let transparent = if trns.len() >= 6 {
                Some([
                    be_u16(&trns[0..2]).ok_or(PngDecodeError::Invalid)? as u8,
                    be_u16(&trns[2..4]).ok_or(PngDecodeError::Invalid)? as u8,
                    be_u16(&trns[4..6]).ok_or(PngDecodeError::Invalid)? as u8,
                ])
            } else {
                None
            };
            for i in 0..pixel_count {
                let src = i * 3;
                let dst = i * 4;
                let rgb = [raw[src], raw[src + 1], raw[src + 2]];
                rgba[dst] = rgb[0];
                rgba[dst + 1] = rgb[1];
                rgba[dst + 2] = rgb[2];
                rgba[dst + 3] = if transparent == Some(rgb) { 0 } else { 255 };
            }
        }
        3 => {
            if palette.is_empty() || palette.len() % 3 != 0 {
                return Err(PngDecodeError::Invalid);
            }
            for i in 0..pixel_count {
                let idx = raw[i] as usize;
                let src = idx.checked_mul(3).ok_or(PngDecodeError::DecodeFailed)?;
                if src + 2 >= palette.len() {
                    return Err(PngDecodeError::Invalid);
                }
                let dst = i * 4;
                rgba[dst] = palette[src];
                rgba[dst + 1] = palette[src + 1];
                rgba[dst + 2] = palette[src + 2];
                rgba[dst + 3] = trns.get(idx).copied().unwrap_or(255);
            }
        }
        4 => {
            for i in 0..pixel_count {
                let src = i * 2;
                let dst = i * 4;
                let gray = raw[src];
                rgba[dst] = gray;
                rgba[dst + 1] = gray;
                rgba[dst + 2] = gray;
                rgba[dst + 3] = raw[src + 1];
            }
        }
        6 => {
            if rgba.len() != raw.len() {
                return Err(PngDecodeError::DecodeFailed);
            }
            rgba.copy_from_slice(&raw);
        }
        _ => return Err(PngDecodeError::Unsupported),
    }

    Ok(DecodedPng {
        width,
        height,
        rgba,
    })
}
