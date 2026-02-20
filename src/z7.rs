#![allow(dead_code)]
extern crate alloc;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SevenZError {
    Truncated,
    BadMagic,
    BadOffset,
}

const SIG_LEN: usize = 32;

pub fn looks_like_7z(b: &[u8]) -> bool {
    b.get(0..6) == Some(b"7z\xBC\xAF'\x1C")
}

fn le_u64_at(b: &[u8], off: usize) -> Result<u64, SevenZError> {
    let bytes: [u8; 8] = b
        .get(off..off + 8)
        .ok_or(SevenZError::Truncated)?
        .try_into()
        .map_err(|_| SevenZError::Truncated)?;
    Ok(u64::from_le_bytes(bytes))
}

pub fn packed_streams_slice(payload: &[u8]) -> Result<&[u8], SevenZError> {
    if payload.len() < SIG_LEN {
        return Err(SevenZError::Truncated);
    }
    if !looks_like_7z(payload) {
        return Err(SevenZError::BadMagic);
    }
    let next_header_offset = le_u64_at(payload, 12)? as usize;
    let _next_header_size = le_u64_at(payload, 20)? as usize;
    let start = SIG_LEN;
    let end = start
        .checked_add(next_header_offset)
        .ok_or(SevenZError::BadOffset)?;
    if end > payload.len() {
        return Err(SevenZError::BadOffset);
    }

    Ok(&payload[start..end])
}
