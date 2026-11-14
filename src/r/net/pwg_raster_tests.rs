use super::*;
use alloc::{vec, vec::Vec};

pub(crate) fn document(pages: usize) -> Vec<u8> {
    let mut out = b"RaS2".to_vec();
    for _ in 0..pages {
        let mut header = vec![0u8; PAGE_HEADER_BYTES];
        for (offset, value) in [
            (276, 300u32),
            (280, 300),
            (372, 2480),
            (376, 3508),
            (384, 8),
            (388, 24),
            (392, 7440),
            (396, 0),
            (400, 19),
            (420, 3),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        header[1732..1749].copy_from_slice(b"iso_a4_210x297mm\0");
        out.extend_from_slice(&header);
        let mut rows = A4_HEIGHT;
        while rows != 0 {
            let repeat = rows.min(256);
            out.push((repeat - 1) as u8);
            let mut pixels = A4_WIDTH;
            while pixels != 0 {
                let count = pixels.min(128);
                out.extend_from_slice(&[(count - 1) as u8, 255, 255, 255]);
                pixels -= count;
            }
            rows -= repeat;
        }
    }
    out
}

#[test]
fn validates_complete_single_and_multipage_a4_rasters() {
    assert_eq!(validate_a4(&document(1)), Ok(1));
    assert_eq!(validate_a4(&document(3)), Ok(3));
}

#[test]
fn rejects_wrong_geometry_resolution_color_and_media() {
    for offset in [276, 280, 372, 376, 384, 388, 392, 396, 400, 420, 1732] {
        let mut bytes = document(1);
        bytes[4 + offset] ^= 1;
        assert!(validate_a4(&bytes).is_err(), "offset={offset}");
    }
    let mut bytes = document(1);
    bytes[4 + 276..4 + 280].copy_from_slice(&600u32.to_be_bytes());
    assert_eq!(validate_a4(&bytes), Err(InvalidPwg::Header));
}

#[test]
fn rejects_truncation_extra_bytes_and_corrupt_packets() {
    let bytes = document(1);
    for length in [0, 3, 4, 1799, 1800, bytes.len() - 1] {
        assert!(validate_a4(&bytes[..length]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(validate_a4(&trailing).is_err());
    let mut bad_packet = bytes.clone();
    bad_packet[4 + PAGE_HEADER_BYTES + 1] = 128;
    assert_eq!(validate_a4(&bad_packet), Err(InvalidPwg::Raster));
    // Turn the final 48-pixel packet into 128 pixels; no line may overrun.
    let mut wide = bytes;
    wide[4 + PAGE_HEADER_BYTES + 1 + 19 * 4] = 127;
    assert_eq!(validate_a4(&wide), Err(InvalidPwg::Raster));
}

#[test]
fn bounds_document_and_page_count_without_decoding_allocations() {
    assert_eq!(validate_a4(&document(MAX_PAGES)), Ok(MAX_PAGES));
    assert_eq!(validate_a4(&document(MAX_PAGES + 1)), Err(InvalidPwg::Size));
    assert_eq!(
        validate_a4(&vec![0; MAX_DOCUMENT_BYTES + 1]),
        Err(InvalidPwg::Size)
    );
}

#[test]
fn rejects_row_repetition_overrunning_the_page() {
    let mut bytes = document(1);
    // A white row uses one repeat byte and 20 four-byte RGB runs.
    let last_repeat = 4 + PAGE_HEADER_BYTES + (A4_HEIGHT / 256) * 81;
    bytes[last_repeat] = 255;
    assert_eq!(validate_a4(&bytes), Err(InvalidPwg::Raster));
}
