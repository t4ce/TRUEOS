//! Minimal PWG Raster producer for one portrait A4 sRGB page.

extern crate alloc;

use alloc::{vec, vec::Vec};

const DPI: u32 = 300;
const A4_WIDTH_TENTH_MM: u32 = crate::r::gridpaper_service::A4_WIDTH_MM * 10;
const A4_HEIGHT_TENTH_MM: u32 = crate::r::gridpaper_service::A4_HEIGHT_MM * 10;
const GRID_WIDTH_TENTH_MM: u32 = crate::r::gridpaper_service::GRID_WIDTH_MM * 10;
const GRID_HEIGHT_TENTH_MM: u32 = crate::r::gridpaper_service::GRID_HEIGHT_MM * 10;
const RULER_GUTTER_TENTH_MM: u32 = crate::r::gridpaper_service::RULER_GUTTER_MM * 10;
const SURFACE_LEFT_TENTH_MM: u32 =
    (A4_WIDTH_TENTH_MM - GRID_WIDTH_TENTH_MM) / 2 - RULER_GUTTER_TENTH_MM;
const SURFACE_TOP_TENTH_MM: u32 =
    (A4_HEIGHT_TENTH_MM - GRID_HEIGHT_TENTH_MM) / 2 - RULER_GUTTER_TENTH_MM;
const SURFACE_WIDTH_TENTH_MM: u32 = GRID_WIDTH_TENTH_MM + RULER_GUTTER_TENTH_MM;
const SURFACE_HEIGHT_TENTH_MM: u32 = GRID_HEIGHT_TENTH_MM + RULER_GUTTER_TENTH_MM;
const PAGE_HEADER_BYTES: usize = 1_796;
const MAX_DOCUMENT_BYTES: usize = 7 * 1024 * 1024;

pub(crate) const MIME_TYPE: &str = "image/pwg-raster";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EncodeError {
    InvalidFrame,
    TooLarge,
}

pub(crate) fn encode_gridpaper_a4(
    source_width: u32,
    source_height: u32,
    rgba_premultiplied: &[u8],
) -> Result<Vec<u8>, EncodeError> {
    let source_pixels = usize::try_from(source_width)
        .ok()
        .and_then(|width| width.checked_mul(source_height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(EncodeError::InvalidFrame)?;
    if source_width == 0 || source_height == 0 || rgba_premultiplied.len() < source_pixels {
        return Err(EncodeError::InvalidFrame);
    }

    let page_width = tenth_mm_to_pixels(A4_WIDTH_TENTH_MM);
    let page_height = tenth_mm_to_pixels(A4_HEIGHT_TENTH_MM);
    let surface_left = tenth_mm_to_pixels(SURFACE_LEFT_TENTH_MM);
    let surface_top = tenth_mm_to_pixels(SURFACE_TOP_TENTH_MM);
    let surface_right =
        tenth_mm_to_pixels(SURFACE_LEFT_TENTH_MM.saturating_add(SURFACE_WIDTH_TENTH_MM));
    let surface_bottom =
        tenth_mm_to_pixels(SURFACE_TOP_TENTH_MM.saturating_add(SURFACE_HEIGHT_TENTH_MM));
    let surface_width = surface_right.saturating_sub(surface_left).max(1);
    let surface_height = surface_bottom.saturating_sub(surface_top).max(1);

    let mut document = Vec::with_capacity(512 * 1024);
    document.extend_from_slice(b"RaS2");
    document.extend_from_slice(&page_header(page_width, page_height));

    let mut row = vec![255u8; page_width as usize * 3];
    let mut previous = Vec::new();
    let mut repetitions = 0u16;
    for y in 0..page_height {
        row.fill(255);
        if (surface_top..surface_bottom).contains(&y) {
            let source_y = ((u64::from(y - surface_top) * u64::from(source_height))
                / u64::from(surface_height))
            .min(u64::from(source_height - 1)) as usize;
            for x in surface_left..surface_right.min(page_width) {
                let source_x = ((u64::from(x - surface_left) * u64::from(source_width))
                    / u64::from(surface_width))
                .min(u64::from(source_width - 1)) as usize;
                let source = (source_y * source_width as usize + source_x) * 4;
                let destination = x as usize * 3;
                let alpha_inverse = 255u16.saturating_sub(rgba_premultiplied[source + 3] as u16);
                row[destination] =
                    (rgba_premultiplied[source] as u16 + alpha_inverse).min(255) as u8;
                row[destination + 1] =
                    (rgba_premultiplied[source + 1] as u16 + alpha_inverse).min(255) as u8;
                row[destination + 2] =
                    (rgba_premultiplied[source + 2] as u16 + alpha_inverse).min(255) as u8;
            }
        }

        if previous == row && repetitions < 256 {
            repetitions += 1;
            continue;
        }
        if repetitions != 0 {
            append_compressed_lines(&mut document, repetitions, &previous)?;
        }
        previous.clear();
        previous.extend_from_slice(&row);
        repetitions = 1;
    }
    if repetitions != 0 {
        append_compressed_lines(&mut document, repetitions, &previous)?;
    }
    Ok(document)
}

fn page_header(width: u32, height: u32) -> [u8; PAGE_HEADER_BYTES] {
    let mut header = [0u8; PAGE_HEADER_BYTES];
    write_cstring(&mut header[0..64], "PwgRaster");
    write_cstring(&mut header[192..256], "text-and-graphics");
    write_u32(&mut header, 276, DPI);
    write_u32(&mut header, 280, DPI);
    write_u32(&mut header, 308, 0); // short-edge first
    write_u32(&mut header, 352, 595); // A4 width in PostScript points
    write_u32(&mut header, 356, 842); // A4 height in PostScript points
    write_u32(&mut header, 372, width);
    write_u32(&mut header, 376, height);
    write_u32(&mut header, 384, 8); // BitsPerColor
    write_u32(&mut header, 388, 24); // BitsPerPixel
    write_u32(&mut header, 392, width.saturating_mul(3));
    write_u32(&mut header, 396, 0); // chunky pixels
    write_u32(&mut header, 400, 19); // sRGB
    write_u32(&mut header, 420, 3); // colorants
    write_u32(&mut header, 452, 1); // one page
    write_i32(&mut header, 456, 1); // normal cross-feed
    write_i32(&mut header, 460, 1); // normal feed
    write_u32(&mut header, 484, 4); // normal quality
    write_cstring(&mut header[1668..1732], "perceptual");
    write_cstring(&mut header[1732..1796], "iso_a4_210x297mm");
    header
}

fn append_compressed_lines(
    output: &mut Vec<u8>,
    repetitions: u16,
    rgb: &[u8],
) -> Result<(), EncodeError> {
    output.push((repetitions - 1) as u8);
    encode_packbits_rgb(rgb, output)?;
    (output.len() <= MAX_DOCUMENT_BYTES)
        .then_some(())
        .ok_or(EncodeError::TooLarge)
}

fn encode_packbits_rgb(rgb: &[u8], output: &mut Vec<u8>) -> Result<(), EncodeError> {
    if rgb.len() % 3 != 0 {
        return Err(EncodeError::InvalidFrame);
    }
    let pixels = rgb.len() / 3;
    let mut cursor = 0usize;
    while cursor < pixels {
        let repeated = repeated_pixels(rgb, cursor, pixels).min(128);
        if repeated >= 2 {
            output.push((repeated - 1) as u8);
            output.extend_from_slice(&rgb[cursor * 3..cursor * 3 + 3]);
            cursor += repeated;
            continue;
        }

        let literal_start = cursor;
        cursor += 1;
        while cursor < pixels
            && cursor - literal_start < 128
            && repeated_pixels(rgb, cursor, pixels) < 2
        {
            cursor += 1;
        }
        let literal_count = cursor - literal_start;
        if literal_count == 1 {
            output.push(0);
            output.extend_from_slice(&rgb[literal_start * 3..literal_start * 3 + 3]);
        } else {
            output.push((257usize - literal_count) as u8);
            output.extend_from_slice(&rgb[literal_start * 3..cursor * 3]);
        }
    }
    Ok(())
}

fn repeated_pixels(rgb: &[u8], start: usize, pixels: usize) -> usize {
    let sample = &rgb[start * 3..start * 3 + 3];
    let mut count = 1usize;
    while start + count < pixels
        && count < 128
        && &rgb[(start + count) * 3..(start + count) * 3 + 3] == sample
    {
        count += 1;
    }
    count
}

fn tenth_mm_to_pixels(tenth_mm: u32) -> u32 {
    rounded_ratio(u64::from(tenth_mm) * u64::from(DPI), 254)
}

fn rounded_ratio(numerator: u64, denominator: u64) -> u32 {
    u32::try_from((numerator + denominator / 2) / denominator).unwrap_or(u32::MAX)
}

fn write_u32(header: &mut [u8], offset: usize, value: u32) {
    header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

fn write_i32(header: &mut [u8], offset: usize, value: i32) {
    header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

fn write_cstring(destination: &mut [u8], value: &str) {
    let length = value.len().min(destination.len().saturating_sub(1));
    destination[..length].copy_from_slice(&value.as_bytes()[..length]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_header_is_network_order_srgb_300() {
        let width = tenth_mm_to_pixels(A4_WIDTH_TENTH_MM);
        let height = tenth_mm_to_pixels(A4_HEIGHT_TENTH_MM);
        let header = page_header(width, height);
        assert_eq!(&header[..10], b"PwgRaster\0");
        assert_eq!(&header[276..280], &300u32.to_be_bytes());
        assert_eq!(&header[372..376], &2480u32.to_be_bytes());
        assert_eq!(&header[376..380], &3508u32.to_be_bytes());
        assert_eq!(&header[400..404], &19u32.to_be_bytes());
    }

    #[test]
    fn expanded_grid_stays_centered_on_a4_with_ruler_gutter() {
        assert_eq!((GRID_WIDTH_TENTH_MM, GRID_HEIGHT_TENTH_MM), (1_950, 2_750));
        assert_eq!((SURFACE_LEFT_TENTH_MM, SURFACE_TOP_TENTH_MM), (35, 70));
        assert_eq!((SURFACE_WIDTH_TENTH_MM, SURFACE_HEIGHT_TENTH_MM), (1_990, 2_790));
        assert_eq!(
            SURFACE_LEFT_TENTH_MM + RULER_GUTTER_TENTH_MM,
            (A4_WIDTH_TENTH_MM - GRID_WIDTH_TENTH_MM) / 2,
        );
        assert_eq!(
            SURFACE_TOP_TENTH_MM + RULER_GUTTER_TENTH_MM,
            (A4_HEIGHT_TENTH_MM - GRID_HEIGHT_TENTH_MM) / 2,
        );
    }

    #[test]
    fn packbits_uses_whole_rgb_pixels() {
        let mut encoded = Vec::new();
        encode_packbits_rgb(&[255, 255, 255, 255, 255, 255, 1, 2, 3], &mut encoded).unwrap();
        assert_eq!(encoded, [1, 255, 255, 255, 0, 1, 2, 3]);
    }
}
