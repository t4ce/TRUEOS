//! Bounded HTTP content decoding after transfer framing has been removed.
extern crate alloc;
use alloc::{string::String, vec::Vec};

pub(crate) fn decode(
    encoding: Option<&str>,
    body: Vec<u8>,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let encoding = encoding.unwrap_or("identity").trim();
    if encoding.is_empty() || encoding.eq_ignore_ascii_case("identity") {
        return Ok(body);
    }
    if encoding.eq_ignore_ascii_case("gzip") || encoding.eq_ignore_ascii_case("x-gzip") {
        return gzip(&body, limit);
    }
    if encoding.eq_ignore_ascii_case("deflate") {
        return miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&body, limit)
            .map_err(|_| String::from("invalid or oversized deflate content"));
    }
    Err(alloc::format!("unsupported content encoding {}", encoding))
}

fn gzip(input: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let invalid = || String::from("invalid or oversized gzip content");
    if input.len() < 18 || input[..3] != [0x1f, 0x8b, 8] || input[3] & 0xe0 != 0 {
        return Err(invalid());
    }
    let flags = input[3];
    let end = input.len() - 8;
    let mut offset = 10usize;
    if flags & 4 != 0 {
        let size = input.get(offset..offset + 2).ok_or_else(invalid)?;
        offset += 2 + u16::from_le_bytes([size[0], size[1]]) as usize;
    }
    for flag in [8, 16] {
        if flags & flag != 0 {
            let tail = input.get(offset..end).ok_or_else(invalid)?;
            offset += tail.iter().position(|b| *b == 0).ok_or_else(invalid)? + 1;
        }
    }
    if flags & 2 != 0 {
        let crc = input.get(offset..offset + 2).ok_or_else(invalid)?;
        if u16::from_le_bytes([crc[0], crc[1]]) != crc32fast::hash(&input[..offset]) as u16 {
            return Err(invalid());
        }
        offset += 2;
    }
    let compressed = input.get(offset..end).ok_or_else(invalid)?;
    let size = u32::from_le_bytes(input[end + 4..].try_into().map_err(|_| invalid())?);
    if size as u64 > limit as u64 {
        return Err(invalid());
    }
    let body = miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, limit)
        .map_err(|_| invalid())?;
    let crc = u32::from_le_bytes(input[end..end + 4].try_into().map_err(|_| invalid())?);
    if body.len() as u32 != size || crc32fast::hash(&body) != crc {
        return Err(invalid());
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gzip_is_decoded_bounded_and_crc_checked() {
        let body = b"<!doctype html><html>test</html>";
        let mut gzip = alloc::vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
        gzip.extend(miniz_oxide::deflate::compress_to_vec(body, 6));
        gzip.extend(crc32fast::hash(body).to_le_bytes());
        gzip.extend((body.len() as u32).to_le_bytes());
        assert_eq!(decode(Some("gzip"), gzip.clone(), body.len()).unwrap(), body);
        assert!(decode(Some("gzip"), gzip.clone(), body.len() - 1).is_err());
        let crc_offset = gzip.len() - 8;
        gzip[crc_offset] ^= 1;
        assert!(decode(Some("gzip"), gzip, 1024).is_err());
        assert!(decode(Some("br"), Vec::new(), 1024).is_err());
    }
}
