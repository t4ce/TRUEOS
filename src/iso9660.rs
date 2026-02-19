#![allow(dead_code)]

// Minimal ISO9660 reader for extracting a few known files from an in-memory ISO.
//
// Intended use: `update` downloads the installer ISO and extracts
// `/TRUEOS.elf` and `/EFI/BOOT/BOOTX64.EFI` without needing archive support.
//
// This is intentionally not a full ISO9660 + Rock Ridge implementation.

extern crate alloc;

use alloc::string::String;

const ISO_SECTOR_BYTES: usize = 2048;
const PVD_SECTOR: usize = 16;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IsoError {
    Truncated,
    BadMagic,
    BadPath,
    NotFound,
}

#[derive(Copy, Clone, Debug)]
struct DirRec<'a> {
    extent_lba: u32,
    data_len: u32,
    flags: u8,
    file_id: &'a [u8],
}

impl<'a> DirRec<'a> {
    fn is_dir(&self) -> bool {
        (self.flags & 0x02) != 0
    }

    fn normalized_name(&self) -> Option<String> {
        // Special entries: 0 and 1 are '.' and '..'
        if self.file_id.len() == 1 && self.file_id[0] == 0 {
            return Some(String::from("."));
        }
        if self.file_id.len() == 1 && self.file_id[0] == 1 {
            return Some(String::from(".."));
        }

        // ISO9660 identifier is usually ASCII with optional ";1" version.
        // Normalize by:
        // - trimming at ';'
        // - uppercasing ASCII
        let mut end = self.file_id.len();
        for (i, &b) in self.file_id.iter().enumerate() {
            if b == b';' {
                end = i;
                break;
            }
        }
        let name = &self.file_id[..end];
        if name.is_empty() {
            return None;
        }

        let mut out = String::new();
        for &b in name {
            let c = b as char;
            if c.is_ascii() {
                out.push(c.to_ascii_uppercase());
            } else {
                // Keep parser ASCII-only.
                return None;
            }
        }
        Some(out)
    }
}

fn pvd(iso: &[u8]) -> Result<&[u8], IsoError> {
    let start = PVD_SECTOR
        .checked_mul(ISO_SECTOR_BYTES)
        .ok_or(IsoError::Truncated)?;
    let end = start
        .checked_add(ISO_SECTOR_BYTES)
        .ok_or(IsoError::Truncated)?;
    let pvd = iso.get(start..end).ok_or(IsoError::Truncated)?;

    // Type 1, magic "CD001", version 1.
    if pvd.get(0) != Some(&1u8) {
        return Err(IsoError::BadMagic);
    }
    if pvd.get(1..6) != Some(b"CD001") {
        return Err(IsoError::BadMagic);
    }
    if pvd.get(6) != Some(&1u8) {
        return Err(IsoError::BadMagic);
    }

    Ok(pvd)
}

fn parse_le_u32(b: &[u8]) -> Option<u32> {
    let b: [u8; 4] = b.get(0..4)?.try_into().ok()?;
    Some(u32::from_le_bytes(b))
}

fn parse_dir_rec<'a>(buf: &'a [u8], off: usize) -> Option<(DirRec<'a>, usize)> {
    let len = *buf.get(off)? as usize;
    if len == 0 {
        return None;
    }
    let rec = buf.get(off..off + len)?;

    // ISO9660 directory record layout:
    // 0: len, 1: ext attr len,
    // 2..10: extent (both-endian), 10..18: data len (both-endian),
    // 25: file flags,
    // 32: file id len, 33..: file id.
    let extent_lba = parse_le_u32(rec.get(2..6)?)?;
    let data_len = parse_le_u32(rec.get(10..14)?)?;
    let flags = *rec.get(25)?;
    let file_id_len = *rec.get(32)? as usize;
    let file_id = rec.get(33..33 + file_id_len)?;

    Some((
        DirRec {
            extent_lba,
            data_len,
            flags,
            file_id,
        },
        len,
    ))
}

fn read_dir_bytes<'a>(iso: &'a [u8], extent_lba: u32, data_len: u32) -> Result<&'a [u8], IsoError> {
    let start = (extent_lba as usize)
        .checked_mul(ISO_SECTOR_BYTES)
        .ok_or(IsoError::Truncated)?;
    let end = start
        .checked_add(data_len as usize)
        .ok_or(IsoError::Truncated)?;
    iso.get(start..end).ok_or(IsoError::Truncated)
}

pub fn looks_like_iso9660(iso: &[u8]) -> bool {
    // Primary Volume Descriptor magic: type=1, "CD001", version=1 at sector 16.
    let off = PVD_SECTOR * ISO_SECTOR_BYTES;
    let pvd = match iso.get(off..off + ISO_SECTOR_BYTES) {
        Some(v) => v,
        None => return false,
    };
    pvd.get(0) == Some(&1u8) && pvd.get(1..6) == Some(b"CD001") && pvd.get(6) == Some(&1u8)
}

/// Locate an ISO9660 image embedded inside a larger blob.
///
/// This is used for the update payload: `TrueOS.7z` is generated in *store*
/// mode (Copy, non-solid), so it contains the raw ISO bytes verbatim.
/// We avoid implementing full 7z decompression by scanning for the Primary
/// Volume Descriptor signature and validating it.
pub fn find_embedded_iso9660_start(blob: &[u8]) -> Option<usize> {
    // PVD signature is at ISO offset 16*2048 and begins with: 0x01 "CD001" 0x01
    const SIG: &[u8; 7] = b"\x01CD001\x01";
    let pvd_off = PVD_SECTOR * ISO_SECTOR_BYTES;

    if blob.len() < pvd_off + SIG.len() {
        return None;
    }

    // Naive scan is fine at ~40MB.
    let mut i = 0usize;
    while i + SIG.len() <= blob.len() {
        if &blob[i..i + SIG.len()] == SIG {
            // Candidate ISO start.
            if i >= pvd_off {
                let start = i - pvd_off;
                // Prefer sector alignment.
                if start % ISO_SECTOR_BYTES == 0 && looks_like_iso9660(&blob[start..]) {
                    return Some(start);
                }
                // Fallback: accept unaligned starts if validation passes.
                if looks_like_iso9660(&blob[start..]) {
                    return Some(start);
                }
            }
            // Continue searching; false positives are possible.
        }
        i += 1;
    }
    None
}

fn find_in_dir<'a>(dir: &'a [u8], want_upper: &str) -> Option<DirRec<'a>> {
    let mut off = 0usize;
    while off < dir.len() {
        let len = *dir.get(off).unwrap_or(&0) as usize;
        if len == 0 {
            // Advance to next sector boundary.
            let next = (off + ISO_SECTOR_BYTES) & !(ISO_SECTOR_BYTES - 1);
            if next <= off {
                break;
            }
            off = next;
            continue;
        }

        let (rec, adv) = parse_dir_rec(dir, off)?;
        off = off.saturating_add(adv);

        let Some(name) = rec.normalized_name() else {
            continue;
        };
        if name == want_upper {
            return Some(rec);
        }
    }
    None
}

fn split_components_upper(path: &str) -> Result<alloc::vec::Vec<String>, IsoError> {
    let mut p = path.trim();
    if p.is_empty() {
        return Err(IsoError::BadPath);
    }
    if let Some(rest) = p.strip_prefix('/') {
        p = rest;
    }

    let mut out = alloc::vec::Vec::new();
    for part in p.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(IsoError::BadPath);
        }
        let mut s = String::new();
        for ch in part.chars() {
            if !ch.is_ascii() {
                return Err(IsoError::BadPath);
            }
            s.push(ch.to_ascii_uppercase());
        }
        out.push(s);
    }
    Ok(out)
}

/// Extract a file from an ISO9660 image into a slice.
///
/// `path` is a Unix-like path, e.g. `/TRUEOS.elf` or `/EFI/BOOT/BOOTX64.EFI`.
pub fn file_slice<'a>(iso: &'a [u8], path: &str) -> Result<&'a [u8], IsoError> {
    let pvd = pvd(iso)?;

    // Root directory record is at byte offset 156 in the PVD.
    let root_off = 156usize;
    let (root, _adv) = parse_dir_rec(pvd, root_off).ok_or(IsoError::BadMagic)?;

    let comps = split_components_upper(path)?;
    if comps.is_empty() {
        return Err(IsoError::BadPath);
    }

    // Walk directories.
    let mut cur = root;
    for (i, comp) in comps.iter().enumerate() {
        let is_last = i + 1 == comps.len();

        let dir_bytes = read_dir_bytes(iso, cur.extent_lba, cur.data_len)?;
        let Some(found) = find_in_dir(dir_bytes, comp.as_str()) else {
            return Err(IsoError::NotFound);
        };

        if is_last {
            // File or directory.
            if found.is_dir() {
                return Err(IsoError::NotFound);
            }
            let start = (found.extent_lba as usize)
                .checked_mul(ISO_SECTOR_BYTES)
                .ok_or(IsoError::Truncated)?;
            let end = start
                .checked_add(found.data_len as usize)
                .ok_or(IsoError::Truncated)?;
            return iso.get(start..end).ok_or(IsoError::Truncated);
        }

        if !found.is_dir() {
            return Err(IsoError::NotFound);
        }
        cur = found;
    }

    Err(IsoError::NotFound)
}
