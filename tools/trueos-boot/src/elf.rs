//! The fixed-address ELF64 subset emitted by the TrueOS kernel linker.
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub virtual_address: u64,
    pub file_offset: usize,
    pub file_size: usize,
    pub memory_size: usize,
    pub flags: u32,
}

pub struct Image {
    pub entry: u64,
    pub base: u64,
    pub size: usize,
    pub requests: u64,
    pub requests_size: usize,
    pub segments: Vec<Segment>,
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(bytes.get(at..at + 2).ok_or("truncated ELF")?.try_into().unwrap()))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(bytes.get(at..at + 4).ok_or("truncated ELF")?.try_into().unwrap()))
}
fn u64_at(bytes: &[u8], at: usize) -> Result<u64, &'static str> {
    Ok(u64::from_le_bytes(bytes.get(at..at + 8).ok_or("truncated ELF")?.try_into().unwrap()))
}
fn table(bytes: &[u8], offset: u64, stride: usize, count: usize) -> Result<(), &'static str> {
    let length = stride.checked_mul(count).ok_or("ELF table overflow")?;
    let end = (offset as usize).checked_add(length).ok_or("ELF table overflow")?;
    if end > bytes.len() { return Err("ELF table outside file"); }
    Ok(())
}

pub fn parse(bytes: &[u8]) -> Result<Image, &'static str> {
    if bytes.get(..7) != Some(&b"\x7fELF\x02\x01\x01"[..])
        || u16_at(bytes, 16)? != 2 || u16_at(bytes, 18)? != 62
        || u32_at(bytes, 20)? != 1
    { return Err("expected an x86-64 executable ELF"); }
    let entry = u64_at(bytes, 24)?;
    let ph = u64_at(bytes, 32)?;
    let sh = u64_at(bytes, 40)?;
    let ph_stride = usize::from(u16_at(bytes, 54)?);
    let ph_count = usize::from(u16_at(bytes, 56)?);
    let sh_stride = usize::from(u16_at(bytes, 58)?);
    let sh_count = usize::from(u16_at(bytes, 60)?);
    let names_index = usize::from(u16_at(bytes, 62)?);
    if ph_stride != 56 || sh_stride != 64 || names_index >= sh_count {
        return Err("unsupported ELF tables");
    }
    table(bytes, ph, ph_stride, ph_count)?;
    table(bytes, sh, sh_stride, sh_count)?;
    let mut segments: Vec<Segment> = Vec::new();
    let mut base = u64::MAX;
    let mut end = 0u64;
    for index in 0..ph_count {
        let at = ph as usize + index * ph_stride;
        let kind = u32_at(bytes, at)?;
        if kind == 2 || kind == 3 { return Err("dynamic ELF is unsupported"); }
        if kind != 1 { continue; }
        let s = Segment {
            flags: u32_at(bytes, at + 4)?, file_offset: u64_at(bytes, at + 8)? as usize,
            virtual_address: u64_at(bytes, at + 16)?, file_size: u64_at(bytes, at + 32)? as usize,
            memory_size: u64_at(bytes, at + 40)? as usize,
        };
        let limit = s.virtual_address.checked_add(s.memory_size as u64).ok_or("ELF address overflow")?;
        if s.virtual_address < 0xffff_ffff_8000_0000 || limit < s.virtual_address
            || s.virtual_address & 4095 != 0 || s.file_size > s.memory_size
            || s.file_offset.checked_add(s.file_size).is_none_or(|n| n > bytes.len())
        { return Err("invalid TrueOS load segment"); }
        for previous in &segments {
            let previous_end = previous.virtual_address + previous.memory_size as u64;
            if s.virtual_address < previous_end && previous.virtual_address < limit {
                return Err("overlapping ELF segments");
            }
        }
        base = base.min(s.virtual_address);
        end = end.max(limit);
        segments.push(s);
    }
    if !segments.iter().any(|s| s.flags & 1 != 0 && entry >= s.virtual_address
        && entry < s.virtual_address + s.memory_size as u64)
    { return Err("ELF entry is outside executable segments"); }
    let names_header = sh as usize + names_index * sh_stride;
    let names_offset = u64_at(bytes, names_header + 24)? as usize;
    let names_len = u64_at(bytes, names_header + 32)? as usize;
    let names_end = names_offset.checked_add(names_len).ok_or("ELF name table overflow")?;
    let names = bytes.get(names_offset..names_end).ok_or("invalid ELF name table")?;
    let mut requests = None;
    for index in 0..sh_count {
        let at = sh as usize + index * sh_stride;
        let name_at = u32_at(bytes, at)? as usize;
        let tail = names.get(name_at..).ok_or("invalid ELF section name")?;
        let length = tail.iter().position(|&c| c == 0).ok_or("unterminated ELF section name")?;
        if &tail[..length] != b".limine_requests" { continue; }
        let address = u64_at(bytes, at + 16)?;
        let size = u64_at(bytes, at + 32)?;
        let limit = address.checked_add(size).ok_or("request section overflow")?;
        if requests.is_some() || address & 7 != 0 || !segments.iter().any(|s|
            s.flags & 2 != 0 && address >= s.virtual_address
                && limit <= s.virtual_address + s.memory_size as u64)
        { return Err("invalid request section"); }
        requests = Some((address, size as usize));
    }
    let (requests, requests_size) = requests.ok_or("missing kernel boot requests")?;
    let span = end.checked_sub(base).ok_or("empty ELF")?;
    if span == 0 || span > 1024 * 1024 * 1024 { return Err("kernel image exceeds loader limit"); }
    Ok(Image { entry, base, size: span as usize, requests, requests_size, segments })
}
