//! Bounded POSIX tar/PAX container. LZ4 remains an independent codec layer.
extern crate alloc;
use crate::z7::{SevenZEntry, SevenZSourceEntry};
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Unsupported,
    Limit,
}
const BLOCK: usize = 512;
fn number(bytes: &[u8]) -> Result<usize, Error> {
    let text = core::str::from_utf8(bytes)
        .map_err(|_| Error::Invalid)?
        .trim_matches(['\0', ' ']);
    if text.is_empty() {
        return Ok(0);
    }
    usize::from_str_radix(text, 8).map_err(|_| Error::Invalid)
}
fn text(bytes: &[u8]) -> Result<&str, Error> {
    core::str::from_utf8(bytes.split(|b| *b == 0).next().unwrap_or(&[])).map_err(|_| Error::Invalid)
}
fn octal(dst: &mut [u8], value: usize) -> Result<(), Error> {
    let encoded = format!("{:0width$o}", value, width = dst.len() - 1);
    if encoded.len() != dst.len() - 1 {
        return Err(Error::Limit);
    }
    dst[..encoded.len()].copy_from_slice(encoded.as_bytes());
    Ok(())
}
fn member(out: &mut Vec<u8>, name: &str, kind: u8, data: &[u8]) -> Result<(), Error> {
    if name.len() > 100 {
        return Err(Error::Limit);
    }
    let mut header = [0u8; BLOCK];
    header[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut header[100..108], 0o644)?;
    octal(&mut header[108..116], 0)?;
    octal(&mut header[116..124], 0)?;
    octal(&mut header[124..136], data.len())?;
    octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|b| *b as usize).sum();
    octal(&mut header[148..155], checksum)?;
    header[155] = b' ';
    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(BLOCK), 0);
    Ok(())
}
fn pax_record(key: &str, value: &str) -> String {
    let tail = format!(" {key}={value}\n");
    let mut size = tail.len() + 1;
    loop {
        let updated = tail.len() + size.to_string().len();
        if updated == size {
            return format!("{size}{tail}");
        }
        size = updated;
    }
}
/// Deterministic tar with standard PAX paths and an optional namespaced content
/// identity field. Other tar readers may ignore that field and read every file.
pub fn pack(entries: &[SevenZSourceEntry<'_>], max_bytes: usize) -> Result<Vec<u8>, Error> {
    let mut sorted: Vec<_> = entries.iter().collect();
    sorted.sort_by_key(|entry| entry.name);
    let mut out = Vec::new();
    for entry in sorted {
        if entry.name.is_empty() || entry.name.len() > 1024 || entry.name.contains(['\0', '\n']) {
            return Err(Error::Invalid);
        }
        let mut pax = pax_record("path", entry.name);
        if let Some(raw) = entry.content_type_raw {
            pax.push_str(&pax_record("TRUEOS.content_type", &raw.to_string()));
        }
        let needed = 512usize
            .checked_add(pax.len().next_multiple_of(BLOCK))
            .and_then(|n| n.checked_add(512 + entry.bytes.len().next_multiple_of(BLOCK)))
            .and_then(|n| n.checked_add(out.len() + 1024))
            .ok_or(Error::Limit)?;
        if needed > max_bytes {
            return Err(Error::Limit);
        }
        member(&mut out, "PaxHeader", b'x', pax.as_bytes())?;
        member(&mut out, "file", b'0', entry.bytes)?;
    }
    out.resize(out.len() + 1024, 0);
    Ok(out)
}
#[derive(Default)]
struct Pax {
    path: Option<String>,
    content_type: Option<u32>,
    size: Option<usize>,
}
fn pax(data: &[u8]) -> Result<Pax, Error> {
    let mut fields = Pax::default();
    let mut pos = 0usize;
    while pos < data.len() {
        let space = data[pos..]
            .iter()
            .position(|b| *b == b' ')
            .ok_or(Error::Invalid)?
            + pos;
        let size: usize = core::str::from_utf8(&data[pos..space])
            .map_err(|_| Error::Invalid)?
            .parse()
            .map_err(|_| Error::Invalid)?;
        let end = pos.checked_add(size).ok_or(Error::Limit)?;
        if end <= space + 1 || end > data.len() || data[end - 1] != b'\n' {
            return Err(Error::Invalid);
        }
        let record = core::str::from_utf8(&data[space + 1..end - 1]).map_err(|_| Error::Invalid)?;
        let (key, value) = record.split_once('=').ok_or(Error::Invalid)?;
        match key {
            "path" => {
                if value.len() > 1024 || fields.path.is_some() {
                    return Err(Error::Invalid);
                }
                fields.path = Some(String::from(value));
            }
            "size" => {
                if fields.size.is_some() {
                    return Err(Error::Invalid);
                }
                fields.size = Some(value.parse().map_err(|_| Error::Invalid)?);
            }
            "TRUEOS.content_type" => {
                if fields.content_type.is_some() {
                    return Err(Error::Invalid);
                }
                fields.content_type = Some(value.parse().map_err(|_| Error::Invalid)?);
            }
            key if key.starts_with("GNU.sparse") => return Err(Error::Unsupported),
            _ => {}
        }
        pos = end;
    }
    Ok(fields)
}
pub fn unpack(
    input: &[u8],
    max_entries: usize,
    max_file: usize,
    max_total: usize,
) -> Result<Vec<SevenZEntry>, Error> {
    let mut pos = 0usize;
    let mut total = 0usize;
    let mut out = Vec::new();
    let mut pending = None;
    while let Some(header) = input.get(pos..pos + BLOCK) {
        if header.iter().all(|b| *b == 0) {
            if pending.is_some() || input.len() - pos < 1024 || input[pos..].iter().any(|b| *b != 0)
            {
                return Err(Error::Invalid);
            }
            return Ok(out);
        }
        let checksum: usize = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    *b as usize
                }
            })
            .sum();
        if number(&header[148..156])? != checksum {
            return Err(Error::Invalid);
        }
        let kind = header[156];
        let mut fields: Pax = if kind == b'x' {
            Pax::default()
        } else {
            pending.take().unwrap_or_default()
        };
        let size = fields.size.unwrap_or(number(&header[124..136])?);
        pos += BLOCK;
        let end = pos.checked_add(size).ok_or(Error::Limit)?;
        let data = input.get(pos..end).ok_or(Error::Invalid)?;
        pos = end.checked_add(511).ok_or(Error::Limit)? / 512 * 512;
        if pos > input.len() {
            return Err(Error::Invalid);
        }
        if kind == b'x' {
            if pending.is_some() || size > 16384 {
                return Err(Error::Limit);
            }
            pending = Some(pax(data)?);
            continue;
        }
        if kind == b'5' {
            if size != 0 {
                return Err(Error::Invalid);
            }
            continue;
        }
        // Links, devices, global PAX and sparse records are not regular files.
        if kind != 0 && kind != b'0' {
            return Err(Error::Unsupported);
        }
        if size > max_file || out.len() >= max_entries {
            return Err(Error::Limit);
        }
        total = total.checked_add(size).ok_or(Error::Limit)?;
        if total > max_total {
            return Err(Error::Limit);
        }
        let name = match fields.path.take() {
            Some(path) => path,
            None => {
                let name = text(&header[..100])?;
                let prefix = if &header[257..263] == b"ustar\0" {
                    text(&header[345..500])?
                } else {
                    ""
                };
                if prefix.is_empty() {
                    String::from(name)
                } else {
                    format!("{prefix}/{name}")
                }
            }
        };
        out.push(SevenZEntry {
            name,
            bytes: data.to_vec(),
            content_type_raw: fields.content_type,
        });
    }
    Err(Error::Invalid)
}
