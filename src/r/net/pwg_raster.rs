//! Validation for the app-composed, exact-size A4 PWG Raster print contract.
//!
//! Page layout and compression belong to the producing app. The spooler accepts
//! only complete 300-dpi sRGB A4 pages, never partial uploads or arbitrary PDL.

pub(crate) const MIME_TYPE: &str = "image/pwg-raster";
pub(crate) const MIN_DOCUMENT_BYTES: usize = 4 + PAGE_HEADER_BYTES + 1;
pub(crate) const MAX_DOCUMENT_BYTES: usize = 32 * 1024 * 1024;
const PAGE_HEADER_BYTES: usize = 1_796;
const A4_WIDTH: usize = 2_480;
const A4_HEIGHT: usize = 3_508;
const MAX_PAGES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InvalidPwg {
    Size,
    Header,
    Raster,
}

pub(crate) fn validate_a4(document: &[u8]) -> Result<usize, InvalidPwg> {
    if document.len() < MIN_DOCUMENT_BYTES || document.len() > MAX_DOCUMENT_BYTES {
        return Err(InvalidPwg::Size);
    }
    if !document.starts_with(b"RaS2") {
        return Err(InvalidPwg::Header);
    }
    let mut cursor = 4usize;
    let mut pages = 0usize;
    while cursor < document.len() {
        if pages == MAX_PAGES {
            return Err(InvalidPwg::Size);
        }
        let header = document
            .get(cursor..cursor + PAGE_HEADER_BYTES)
            .ok_or(InvalidPwg::Header)?;
        if header_u32(header, 276) != 300 || header_u32(header, 280) != 300
            || header_u32(header, 372) != A4_WIDTH as u32 || header_u32(header, 376) != A4_HEIGHT as u32
            || header_u32(header, 384) != 8 || header_u32(header, 388) != 24
            || header_u32(header, 392) != A4_WIDTH as u32 * 3 || header_u32(header, 396) != 0
            || header_u32(header, 400) != 19 || header_u32(header, 420) != 3
            || header_u32(header, 368) != 0 // no printer-specific media type
            || !header[1732..1796].starts_with(b"iso_a4_210x297mm\0")
        {
            return Err(InvalidPwg::Header);
        }
        cursor += PAGE_HEADER_BYTES;
        let mut rows = 0usize;
        while rows < A4_HEIGHT {
            let repetitions = usize::from(*document.get(cursor).ok_or(InvalidPwg::Raster)?) + 1;
            cursor += 1;
            if repetitions > A4_HEIGHT - rows {
                return Err(InvalidPwg::Raster);
            }
            let mut pixels = 0usize;
            while pixels < A4_WIDTH {
                let packet = *document.get(cursor).ok_or(InvalidPwg::Raster)?;
                cursor += 1;
                // The producer uses the same whole-RGB PackBits subset as the
                // proven Gridpaper encoder. 128 is not emitted by that encoder.
                if packet == 128 {
                    return Err(InvalidPwg::Raster);
                }
                let (count, bytes) = if packet <= 127 {
                    (usize::from(packet) + 1, 3)
                } else {
                    let count = 257 - usize::from(packet);
                    (count, count * 3)
                };
                if count > A4_WIDTH - pixels {
                    return Err(InvalidPwg::Raster);
                }
                let end = cursor.checked_add(bytes).ok_or(InvalidPwg::Raster)?;
                if end > document.len() {
                    return Err(InvalidPwg::Raster);
                }
                cursor = end;
                pixels += count;
            }
            rows += repetitions;
        }
        pages += 1;
    }
    if pages == 0 {
        return Err(InvalidPwg::Raster);
    }
    Ok(pages)
}

fn header_u32(header: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(header[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
#[path = "pwg_raster_tests.rs"]
pub(crate) mod tests;
