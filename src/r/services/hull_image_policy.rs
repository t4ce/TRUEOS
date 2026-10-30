//! Pure admission and cache policy for Hull's finite raster projection.
use alloc::{format, string::String};
use sha2::{Digest, Sha256};

pub(super) const MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub(super) const MAX_SOURCE_DIMENSION: u32 = 8192;
pub(super) const MAX_TARGET_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_TARGET_DIMENSION: u32 = 2048;
pub(super) const MAX_URL_BYTES: usize = 4096;
pub(super) const ERR_INVALID: i32 = -1;
pub(super) const ERR_TOO_LARGE: i32 = -9;
pub(super) const ERR_UNSUPPORTED: i32 = -8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SourceInfo {
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

pub(super) fn extension(format: u32) -> Option<&'static str> {
    match format {
        1 => Some("jpg"),
        2 => Some("png"),
        3 => Some("bmp"),
        _ => None,
    }
}

pub(super) fn digest(bytes: &[u8]) -> String {
    use core::fmt::Write;
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

/// Fragmentless HTTPS URL; query bytes remain part of both fetch and cache key.
/// This finite transport accepts DNS/IPv4 authorities, matching the HTTPS client.
pub(crate) fn canonical_url(raw: &str) -> Result<String, i32> {
    if raw.len() > MAX_URL_BYTES
        || raw
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return Err(ERR_INVALID);
    }
    let raw = raw.split('#').next().ok_or(ERR_INVALID)?;
    let rest = raw.strip_prefix("https://").ok_or(ERR_UNSUPPORTED)?;
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority.is_empty() || authority.contains('@') {
        return Err(ERR_INVALID);
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port.parse::<u16>().map_err(|_| ERR_INVALID)?)),
        None => (authority, None),
    };
    if host.is_empty()
        || host.len() > 253
        || host.starts_with('.')
        || host.ends_with('.')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        || port == Some(0)
    {
        return Err(ERR_INVALID);
    }
    let authority = match port.filter(|p| *p != 443) {
        Some(port) => format!("{}:{port}", host.to_ascii_lowercase()),
        None => host.to_ascii_lowercase(),
    };
    let suffix = &rest[end..];
    Ok(if suffix.starts_with('/') {
        format!("https://{authority}{suffix}")
    } else {
        format!("https://{authority}/{suffix}")
    })
}

pub(super) fn valid_page_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host != "."
        && host != ".."
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
}

pub(super) fn target_extent(width: u32, height: u32) -> Result<(u32, u32, usize), i32> {
    if width == 0 || height == 0 {
        return Err(ERR_INVALID);
    }
    let longest = width.max(height);
    let (mut w, mut h) = if longest > MAX_TARGET_DIMENSION {
        (
            ((width as u64 * MAX_TARGET_DIMENSION as u64) / longest as u64).max(1) as u32,
            ((height as u64 * MAX_TARGET_DIMENSION as u64) / longest as u64).max(1) as u32,
        )
    } else {
        (width, height)
    };
    // Account for the UI4 64-byte row stride and 4-KiB DMA alignment, not just pixels.
    loop {
        let bytes = (((w as usize * 4 + 63) & !63) * h as usize + 4095) & !4095;
        if bytes <= MAX_TARGET_BYTES {
            return Ok((w, h, bytes));
        }
        w = (w * 7 / 8).max(1);
        h = (h * 7 / 8).max(1);
    }
}

fn bounded(format: u32, width: u32, height: u32) -> Result<SourceInfo, i32> {
    if width == 0 || height == 0 {
        return Err(ERR_INVALID);
    }
    if width > MAX_SOURCE_DIMENSION
        || height > MAX_SOURCE_DIMENSION
        || (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .is_none_or(|n| n > MAX_SOURCE_BYTES)
    {
        return Err(ERR_TOO_LARGE);
    }
    Ok(SourceInfo {
        format,
        width,
        height,
    })
}

/// Inspect format and dimensions without allocating a decoded frame. Decoders
/// still validate the complete bitstream, and returned dimensions must match.
pub(super) fn preflight(bytes: &[u8]) -> Result<SourceInfo, i32> {
    if bytes.is_empty() {
        return Err(ERR_INVALID);
    }
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(ERR_TOO_LARGE);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if bytes.len() < 33 || &bytes[8..16] != b"\0\0\0\rIHDR" {
            return Err(ERR_INVALID);
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        let info = bounded(2, width, height)?;
        let mut offset = 8usize;
        let mut ended = false;
        while offset < bytes.len() {
            let header = bytes.get(offset..offset + 8).ok_or(ERR_INVALID)?;
            let len = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
            let end = offset
                .checked_add(len)
                .and_then(|n| n.checked_add(12))
                .ok_or(ERR_INVALID)?;
            if end > bytes.len() {
                return Err(ERR_INVALID);
            }
            // Static PNG only. Reject animated subframes before a decoder can
            // allocate from a second, different extent or expand animation data.
            if matches!(&header[4..8], b"acTL" | b"fcTL" | b"fdAT") {
                return Err(ERR_UNSUPPORTED);
            }
            if &header[4..8] == b"IHDR" && offset != 8 {
                return Err(ERR_INVALID);
            }
            if &header[4..8] == b"IEND" {
                ended = true;
                break;
            }
            offset = end;
        }
        return if ended { Ok(info) } else { Err(ERR_INVALID) };
    }
    if bytes.starts_with(b"BM") {
        if bytes.len() < 54 {
            return Err(ERR_INVALID);
        }
        let width = i32::from_le_bytes(bytes[18..22].try_into().unwrap());
        let height = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
        if width <= 0 || height == 0 {
            return Err(ERR_INVALID);
        }
        return bounded(3, width as u32, height.unsigned_abs());
    }
    if bytes.starts_with(b"\xff\xd8") {
        let mut offset = 2usize;
        while offset < bytes.len() {
            if bytes[offset] != 0xff {
                return Err(ERR_INVALID);
            }
            while bytes.get(offset) == Some(&0xff) {
                offset += 1;
            }
            let marker = *bytes.get(offset).ok_or(ERR_INVALID)?;
            offset += 1;
            if matches!(marker, 0xd9 | 0xda | 0x00) {
                return Err(ERR_INVALID);
            }
            if matches!(marker, 0x01 | 0xd0..=0xd7) {
                continue;
            }
            let raw = bytes.get(offset..offset + 2).ok_or(ERR_INVALID)?;
            let len = u16::from_be_bytes(raw.try_into().unwrap()) as usize;
            if len < 2 || offset.checked_add(len).is_none_or(|n| n > bytes.len()) {
                return Err(ERR_INVALID);
            }
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                if len < 8 {
                    return Err(ERR_INVALID);
                }
                if !matches!(marker, 0xc0 | 0xc1 | 0xc2) {
                    return Err(ERR_UNSUPPORTED);
                }
                let height =
                    u16::from_be_bytes(bytes[offset + 3..offset + 5].try_into().unwrap()) as u32;
                let width =
                    u16::from_be_bytes(bytes[offset + 5..offset + 7].try_into().unwrap()) as u32;
                return bounded(1, width, height);
            }
            offset += len;
        }
        return Err(ERR_INVALID);
    }
    Err(ERR_UNSUPPORTED)
}

pub(super) fn metadata(url: &str, format: u32, bytes: &[u8]) -> String {
    format!(
        "HULL-IMAGE-1\n{url}\n{format}\n{}\n{}\n",
        bytes.len(),
        digest(bytes)
    )
}

pub(super) fn cache_descriptor<'a>(metadata: &'a [u8], url: &str) -> Option<(u32, usize, &'a str)> {
    let text = core::str::from_utf8(metadata).ok()?;
    let mut fields = text.lines();
    if fields.next()? != "HULL-IMAGE-1" || fields.next()? != url {
        return None;
    }
    let format = fields.next()?.parse().ok()?;
    extension(format)?;
    let len = fields.next()?.parse().ok()?;
    if len == 0 || len > MAX_ENCODED_BYTES {
        return None;
    }
    let hash = fields.next()?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || fields.next().is_some() {
        return None;
    }
    Some((format, len, hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    #[test]
    fn url_keys_keep_query_and_remove_fragments() {
        let a = canonical_url("https://EXAMPLE.test:443/image.png?q=1#part").unwrap();
        assert_eq!(a, "https://example.test/image.png?q=1");
        assert_eq!(
            canonical_url("https://example.test?q=1").unwrap(),
            "https://example.test/?q=1"
        );
        assert_ne!(
            digest(a.as_bytes()),
            digest(b"https://example.test/image.png?q=2")
        );
        for raw in [
            "http://x/a",
            "https://x@evil/a",
            "https://x/a\nHeader:evil",
            "https:///a",
        ] {
            assert!(canonical_url(raw).is_err());
        }
    }
    #[test]
    fn malformed_and_enormous_source_headers_are_rejected_without_pixels() {
        let mut bmp = vec![0; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[18..22].copy_from_slice(&8192i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&8192i32.to_le_bytes());
        assert_eq!(preflight(&bmp), Err(ERR_TOO_LARGE));
        bmp[18..22].copy_from_slice(&1i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-2i32).to_le_bytes());
        assert_eq!(
            preflight(&bmp),
            Ok(SourceInfo {
                format: 3,
                width: 1,
                height: 2
            })
        );
        let jpeg = [0xff, 0xd8, 0xff, 0xc0, 0, 8, 8, 0x20, 0, 0x20, 0, 3];
        assert_eq!(preflight(&jpeg), Err(ERR_TOO_LARGE));
        assert_eq!(preflight(&jpeg[..10]), Err(ERR_INVALID));
    }
    #[test]
    fn targets_always_fit_padded_allocation_budget() {
        for (w, h) in [
            (1, 1),
            (2048, 2048),
            (u32::MAX, u32::MAX),
            (1, u32::MAX),
            (u32::MAX, 1),
            (600, 400),
        ] {
            let (w, h, bytes) = target_extent(w, h).unwrap();
            assert!(w > 0 && h > 0 && w <= 2048 && h <= 2048 && bytes <= MAX_TARGET_BYTES);
        }
    }
    #[test]
    fn metadata_requires_full_url_format_length_and_digest() {
        let m = metadata("https://a/image?q=1", 2, b"data");
        assert_eq!(
            cache_descriptor(m.as_bytes(), "https://a/image?q=1").map(|v| (v.0, v.1)),
            Some((2, 4))
        );
        assert!(cache_descriptor(m.as_bytes(), "https://a/image?q=2").is_none());
        assert!(
            cache_descriptor(b"HULL-IMAGE-1\nhttps://a\n2\n999999999\nabc\n", "https://a")
                .is_none()
        );
    }
}
