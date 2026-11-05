//! SVG source recognition and the single raster-conversion request policy.
//! This recognizes a bounded document prefix; it does not parse or render SVG.
use alloc::{format, string::String};

use super::hull_image_policy as image;

const SVG_PREFIX_BYTES: usize = 16 * 1024;
const CONVERSION_PREFIX: &str = "https://wsrv.nl/?url=";

pub(super) fn svg_url(url: &str) -> bool {
    if url.len() > image::MAX_URL_BYTES {
        return false;
    }
    // An SVG-looking host or query is not an SVG path suffix. Relative paths
    // also work here; admission/conversion separately validate actual URLs.
    let authority_and_path = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .or_else(|| url.strip_prefix("//"));
    let path = if let Some(authority_and_path) = authority_and_path {
        let Some(start) = authority_and_path.find(['/', '?', '#']) else {
            return false;
        };
        let suffix = &authority_and_path[start..];
        if !suffix.starts_with('/') {
            return false;
        }
        suffix
    } else {
        url
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    [".svg", ".svgz"].iter().any(|extension| {
        path.get(path.len().saturating_sub(extension.len())..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(extension))
    })
}

fn xml_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn end_of(bytes: &[u8], marker: &[u8]) -> Option<usize> {
    bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|start| start + marker.len())
}

/// Skip a DOCTYPE without treating `>` in a quoted identifier or an internal
/// subset as its terminator. No entities are expanded or external DTDs loaded.
fn doctype_end(bytes: &[u8]) -> Option<usize> {
    let mut quote = None;
    let mut subset = 0usize;
    let mut index = 9; // after <!DOCTYPE
    while let Some(&byte) = bytes.get(index) {
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
        } else if bytes[index..].starts_with(b"<!--") {
            index += 4 + end_of(&bytes[index + 4..], b"-->")?;
            continue;
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'[' => subset += 1,
                b']' => subset = subset.checked_sub(1)?,
                b'>' if subset == 0 => return Some(index + 1),
                _ => {}
            }
        }
        index += 1;
    }
    None
}

pub(super) fn svg_document(bytes: &[u8]) -> bool {
    let mut prefix = &bytes[..bytes.len().min(SVG_PREFIX_BYTES)];
    if prefix.starts_with(b"\xEF\xBB\xBF") {
        prefix = &prefix[3..];
    }
    let mut declaration = false;
    let mut doctype = false;
    loop {
        prefix = prefix.trim_ascii_start();
        if prefix.starts_with(b"<!--") {
            let Some(end) = end_of(&prefix[4..], b"-->") else {
                return false;
            };
            prefix = &prefix[4 + end..];
        } else if !declaration
            && prefix.starts_with(b"<?xml")
            && prefix.get(5).copied().is_some_and(xml_space)
        {
            let Some(end) = end_of(&prefix[5..], b"?>") else {
                return false;
            };
            declaration = true;
            prefix = &prefix[5 + end..];
        } else if !doctype
            && prefix.starts_with(b"<!DOCTYPE")
            && prefix.get(9).copied().is_some_and(xml_space)
        {
            let Some(end) = doctype_end(prefix) else {
                return false;
            };
            doctype = true;
            prefix = &prefix[end..];
        } else {
            return prefix.starts_with(b"<svg")
                && prefix
                    .get(4)
                    .copied()
                    .is_some_and(|byte| xml_space(byte) || matches!(byte, b'>' | b'/'));
        }
    }
}

pub(super) fn conversion_url(source: &str, width: u32, height: u32) -> Result<String, i32> {
    let source = image::canonical_url(source)?;
    // A public conversion service cannot retrieve a local preview URL.
    if !source.starts_with("https://") {
        return Err(image::ERR_UNSUPPORTED);
    }
    let (width, height, _) = image::target_extent(width, height)?;
    let options = format!("&output=png&w={width}&h={height}&fit=inside&n=1");
    let unreserved =
        |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
    let encoded_len: usize = source
        .bytes()
        .map(|byte| if unreserved(byte) { 1 } else { 3 })
        .sum();
    let length = CONVERSION_PREFIX.len() + encoded_len + options.len();
    if length > image::MAX_URL_BYTES {
        return Err(image::ERR_TOO_LARGE);
    }
    let mut result = String::with_capacity(length);
    result.push_str(CONVERSION_PREFIX);
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in source.bytes() {
        if unreserved(byte) {
            result.push(byte as char);
        } else {
            result.push('%');
            result.push(HEX[(byte >> 4) as usize] as char);
            result.push(HEX[(byte & 15) as usize] as char);
        }
    }
    result.push_str(&options);
    Ok(result)
}
