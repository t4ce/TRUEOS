extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::num::NonZeroU64;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SevenZError {
    Truncated,
    BadMagic,
    BadOffset,
    BadHeader,
    BadCrc,
    Unsupported,
    DecodeFailed,
}

const LZMA2_DICT_PROP_1_MIB: u8 = 16;

const SIG_LEN: usize = 32;

const K_END: u8 = 0x00;
const K_HEADER: u8 = 0x01;
const K_ARCHIVE_PROPERTIES: u8 = 0x02;
const K_ADDITIONAL_STREAMS_INFO: u8 = 0x03;
const K_MAIN_STREAMS_INFO: u8 = 0x04;
const K_FILES_INFO: u8 = 0x05;
const K_PACK_INFO: u8 = 0x06;
const K_UNPACK_INFO: u8 = 0x07;
const K_SUB_STREAMS_INFO: u8 = 0x08;
const K_SIZE: u8 = 0x09;
const K_CRC: u8 = 0x0A;
const K_FOLDER: u8 = 0x0B;
const K_CODERS_UNPACK_SIZE: u8 = 0x0C;
const K_NUM_UNPACK_STREAM: u8 = 0x0D;
const K_EMPTY_STREAM: u8 = 0x0E;
const K_EMPTY_FILE: u8 = 0x0F;
const K_ANTI: u8 = 0x10;
const K_NAME: u8 = 0x11;
const K_ENCODED_HEADER: u8 = 0x17;

const METHOD_COPY: &[u8] = &[0x00];
const METHOD_LZMA: &[u8] = &[0x03, 0x01, 0x01];
const METHOD_LZMA2: &[u8] = &[0x21];

fn push_variable_u64(out: &mut Vec<u8>, value: u64) {
    if value < 0x80 {
        out.push(value as u8);
        return;
    }

    for bytes in 1..8 {
        let limit = 1u64 << (7 * (bytes + 1));
        if value < limit {
            let mut first = 0u8;
            for bit in 0..bytes {
                first |= 0x80 >> bit;
            }
            let high_bits = (value >> (8 * bytes)) as u8;
            first |= high_bits & ((0x80 >> bytes) - 1);
            out.push(first);
            for idx in 0..bytes {
                out.push((value >> (8 * idx)) as u8);
            }
            return;
        }
    }

    out.push(0xFF);
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_property(out: &mut Vec<u8>, property: u8, body: &[u8]) {
    out.push(property);
    push_variable_u64(out, body.len() as u64);
    out.extend_from_slice(body);
}

fn push_utf16le_name(body: &mut Vec<u8>, name: &str) {
    body.push(0);
    for unit in name.encode_utf16() {
        body.extend_from_slice(&unit.to_le_bytes());
    }
    body.extend_from_slice(&0u16.to_le_bytes());
}

fn build_single_file_header(
    name: &str,
    packed_len: usize,
    unpacked_len: usize,
    unpack_crc: u32,
) -> Vec<u8> {
    let mut header = Vec::new();
    header.push(K_HEADER);
    header.push(K_MAIN_STREAMS_INFO);

    header.push(K_PACK_INFO);
    push_variable_u64(&mut header, 0);
    push_variable_u64(&mut header, 1);
    header.push(K_SIZE);
    push_variable_u64(&mut header, packed_len as u64);
    header.push(K_END);

    header.push(K_UNPACK_INFO);
    header.push(K_FOLDER);
    push_variable_u64(&mut header, 1);
    header.push(0);
    push_variable_u64(&mut header, 1);
    header.push(0x21);
    header.push(METHOD_LZMA2[0]);
    push_variable_u64(&mut header, 1);
    header.push(LZMA2_DICT_PROP_1_MIB);
    header.push(K_CODERS_UNPACK_SIZE);
    push_variable_u64(&mut header, unpacked_len as u64);
    header.push(K_CRC);
    header.push(1);
    header.extend_from_slice(&unpack_crc.to_le_bytes());
    header.push(K_END);

    header.push(K_END);

    header.push(K_FILES_INFO);
    push_variable_u64(&mut header, 1);
    let mut names = Vec::new();
    push_utf16le_name(&mut names, name);
    push_property(&mut header, K_NAME, names.as_slice());
    header.push(K_END);

    header.push(K_END);
    header
}

fn lzma2_compress_to_vec(bytes: &[u8]) -> Result<Vec<u8>, SevenZError> {
    let mut options = lzma_rust2::Lzma2Options::with_preset(1);
    options.lzma_options.dict_size = 1 << 20;
    options.set_chunk_size(NonZeroU64::new(256 * 1024));

    let mut writer = lzma_rust2::Lzma2Writer::new(Vec::new(), options);
    lzma_rust2::Write::write_all(&mut writer, bytes).map_err(|_| SevenZError::DecodeFailed)?;
    writer.finish().map_err(|_| SevenZError::DecodeFailed)
}

pub fn compress_single_file_to_vec(name: &str, bytes: &[u8]) -> Result<Vec<u8>, SevenZError> {
    let packed = lzma2_compress_to_vec(bytes)?;
    let unpack_crc = crc32fast::hash(bytes);
    let header = build_single_file_header(name, packed.len(), bytes.len(), unpack_crc);
    let next_header_crc = crc32fast::hash(header.as_slice());
    let next_header_offset = packed.len() as u64;
    let next_header_size = header.len() as u64;

    let mut start_header = Vec::with_capacity(20);
    start_header.extend_from_slice(&next_header_offset.to_le_bytes());
    start_header.extend_from_slice(&next_header_size.to_le_bytes());
    start_header.extend_from_slice(&next_header_crc.to_le_bytes());
    let start_header_crc = crc32fast::hash(start_header.as_slice());

    let mut out = Vec::with_capacity(SIG_LEN + packed.len() + header.len());
    out.extend_from_slice(b"7z\xBC\xAF'\x1C");
    out.extend_from_slice(&[0, 4]);
    out.extend_from_slice(&start_header_crc.to_le_bytes());
    out.extend_from_slice(start_header.as_slice());
    out.extend_from_slice(packed.as_slice());
    out.extend_from_slice(header.as_slice());
    Ok(out)
}

#[derive(Copy, Clone)]
enum Method {
    Copy,
    Lzma { props: u8, dict_size: u32 },
    Lzma2 { dict_size: u32 },
}

struct ArchiveShape<'a> {
    name: String,
    packed_stream: &'a [u8],
    method: Method,
    folder_unpacked_size: usize,
    folder_crc: Option<u32>,
    substream_offset: usize,
    substream_size: usize,
    substream_crc: Option<u32>,
}

pub struct SevenZEntry {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub struct SevenZEntryInfo {
    pub name: String,
    pub unpacked_size: usize,
    pub crc: Option<u32>,
}

struct FolderInfo {
    method: Method,
    unpacked_size: u64,
    unpack_crc: Option<u32>,
    num_unpack_sub_streams: usize,
    substream_sizes: Vec<u64>,
    substream_crcs: Vec<Option<u32>>,
}

struct FilesInfo {
    names: Vec<String>,
    stream_indices: Vec<Option<usize>>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_u8(&mut self) -> Result<u8, SevenZError> {
        let b = *self.bytes.get(self.pos).ok_or(SevenZError::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], SevenZError> {
        let end = self.pos.checked_add(len).ok_or(SevenZError::BadOffset)?;
        let out = self
            .bytes
            .get(self.pos..end)
            .ok_or(SevenZError::Truncated)?;
        self.pos = end;
        Ok(out)
    }

    fn read_u32_le(&mut self) -> Result<u32, SevenZError> {
        let bytes: [u8; 4] = self
            .read_exact(4)?
            .try_into()
            .map_err(|_| SevenZError::Truncated)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_variable_u64(&mut self) -> Result<u64, SevenZError> {
        let first = self.read_u8()? as u64;
        let mut mask = 0x80_u64;
        let mut value = 0u64;
        for i in 0..8 {
            if (first & mask) == 0 {
                return Ok(value | ((first & (mask - 1)) << (8 * i)));
            }
            let b = self.read_u8()? as u64;
            value |= b << (8 * i);
            mask >>= 1;
        }
        Ok(value)
    }

    fn read_len(&mut self) -> Result<usize, SevenZError> {
        let v = self.read_variable_u64()?;
        usize::try_from(v).map_err(|_| SevenZError::BadOffset)
    }

    fn read_property_body(&mut self) -> Result<Cursor<'a>, SevenZError> {
        let len = self.read_len()?;
        Ok(Cursor::new(self.read_exact(len)?))
    }

    fn skip_all(&mut self) {
        self.pos = self.bytes.len();
    }

    fn is_empty(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

fn le_u32_at(b: &[u8], off: usize) -> Result<u32, SevenZError> {
    let bytes: [u8; 4] = b
        .get(off..off + 4)
        .ok_or(SevenZError::Truncated)?
        .try_into()
        .map_err(|_| SevenZError::Truncated)?;
    Ok(u32::from_le_bytes(bytes))
}

fn le_u64_at(b: &[u8], off: usize) -> Result<u64, SevenZError> {
    let bytes: [u8; 8] = b
        .get(off..off + 8)
        .ok_or(SevenZError::Truncated)?
        .try_into()
        .map_err(|_| SevenZError::Truncated)?;
    Ok(u64::from_le_bytes(bytes))
}

fn lzma2_dict_size(properties: &[u8]) -> Result<u32, SevenZError> {
    let bits = *properties.first().ok_or(SevenZError::BadHeader)? as u32;
    if bits > 40 {
        return Err(SevenZError::Unsupported);
    }
    if bits == 40 {
        return Ok(0xFFFF_FFFF);
    }
    Ok((2 | (bits & 1)) << (bits / 2 + 11))
}

fn parse_method(method_id: &[u8], properties: &[u8]) -> Result<Method, SevenZError> {
    if method_id == METHOD_COPY {
        return Ok(Method::Copy);
    }
    if method_id == METHOD_LZMA {
        let props = *properties.first().ok_or(SevenZError::BadHeader)?;
        let dict_bytes: [u8; 4] = properties
            .get(1..5)
            .ok_or(SevenZError::BadHeader)?
            .try_into()
            .map_err(|_| SevenZError::BadHeader)?;
        return Ok(Method::Lzma {
            props,
            dict_size: u32::from_le_bytes(dict_bytes),
        });
    }
    if method_id == METHOD_LZMA2 {
        return Ok(Method::Lzma2 {
            dict_size: lzma2_dict_size(properties)?,
        });
    }
    Err(SevenZError::Unsupported)
}

fn read_bits(reader: &mut Cursor<'_>, size: usize) -> Result<Vec<bool>, SevenZError> {
    let mut out = Vec::with_capacity(size);
    let mut mask = 0u8;
    let mut cache = 0u8;
    for _ in 0..size {
        if mask == 0 {
            mask = 0x80;
            cache = reader.read_u8()?;
        }
        out.push((cache & mask) != 0);
        mask >>= 1;
    }
    Ok(out)
}

fn read_all_or_bits(reader: &mut Cursor<'_>, size: usize) -> Result<Vec<bool>, SevenZError> {
    if reader.read_u8()? != 0 {
        return Ok(vec![true; size]);
    }
    read_bits(reader, size)
}

fn read_encoded_header(payload: &[u8], encoded_header: &[u8]) -> Result<Vec<u8>, SevenZError> {
    let mut reader = Cursor::new(encoded_header);

    if reader.read_u8()? != K_PACK_INFO {
        return Err(SevenZError::BadHeader);
    }
    let (pack_pos, pack_sizes) = parse_pack_info(&mut reader)?;
    if pack_sizes.len() != 1 {
        return Err(SevenZError::Unsupported);
    }

    if reader.read_u8()? != K_UNPACK_INFO {
        return Err(SevenZError::BadHeader);
    }
    let folders = parse_unpack_info(&mut reader)?;
    if folders.len() != 1 {
        return Err(SevenZError::Unsupported);
    }

    let mut nid = reader.read_u8()?;
    let mut folders = folders;
    if nid == K_SUB_STREAMS_INFO {
        parse_sub_streams_info(&mut reader, folders.as_mut_slice())?;
        nid = reader.read_u8()?;
    }
    if nid != K_END || !reader.is_empty() {
        return Err(SevenZError::BadHeader);
    }

    let pack_pos = usize::try_from(pack_pos).map_err(|_| SevenZError::BadOffset)?;
    let pack_size = usize::try_from(pack_sizes[0]).map_err(|_| SevenZError::BadOffset)?;
    let packed_start = SIG_LEN
        .checked_add(pack_pos)
        .ok_or(SevenZError::BadOffset)?;
    let packed_end = packed_start
        .checked_add(pack_size)
        .ok_or(SevenZError::BadOffset)?;
    let packed_stream = payload
        .get(packed_start..packed_end)
        .ok_or(SevenZError::BadOffset)?;
    let folder = &folders[0];

    extract_archive_shape_to_vec(&ArchiveShape {
        name: String::new(),
        packed_stream,
        method: folder.method,
        folder_unpacked_size: usize::try_from(folder.unpacked_size)
            .map_err(|_| SevenZError::BadOffset)?,
        folder_crc: folder.unpack_crc,
        substream_offset: 0,
        substream_size: usize::try_from(folder.unpacked_size)
            .map_err(|_| SevenZError::BadOffset)?,
        substream_crc: folder.unpack_crc,
    })
}

fn read_next_header(payload: &[u8]) -> Result<Vec<u8>, SevenZError> {
    if payload.len() < SIG_LEN {
        return Err(SevenZError::Truncated);
    }
    if !looks_like_7z(payload) {
        return Err(SevenZError::BadMagic);
    }

    let start_header_crc = le_u32_at(payload, 8)?;
    let start_header = payload.get(12..32).ok_or(SevenZError::Truncated)?;
    if crc32fast::hash(start_header) != start_header_crc {
        return Err(SevenZError::BadCrc);
    }

    let next_header_offset =
        usize::try_from(le_u64_at(payload, 12)?).map_err(|_| SevenZError::BadOffset)?;
    let next_header_size =
        usize::try_from(le_u64_at(payload, 20)?).map_err(|_| SevenZError::BadOffset)?;
    let next_header_crc = le_u32_at(payload, 28)?;

    let start = SIG_LEN
        .checked_add(next_header_offset)
        .ok_or(SevenZError::BadOffset)?;
    let end = start
        .checked_add(next_header_size)
        .ok_or(SevenZError::BadOffset)?;
    let next_header = payload.get(start..end).ok_or(SevenZError::BadOffset)?;
    if crc32fast::hash(next_header) != next_header_crc {
        return Err(SevenZError::BadCrc);
    }
    match next_header.first().copied() {
        Some(K_HEADER) => Ok(next_header.to_vec()),
        Some(K_ENCODED_HEADER) => read_encoded_header(payload, next_header.get(1..).unwrap_or(&[])),
        _ => Err(SevenZError::Unsupported),
    }
}

fn parse_folder(reader: &mut Cursor<'_>) -> Result<FolderInfo, SevenZError> {
    let num_coders = reader.read_len()?;
    if num_coders != 1 {
        return Err(SevenZError::Unsupported);
    }

    let bits = reader.read_u8()?;
    let id_size = (bits & 0x0F) as usize;
    let is_simple = (bits & 0x10) == 0;
    let has_attributes = (bits & 0x20) != 0;
    let has_alt_methods = (bits & 0x80) != 0;
    if has_alt_methods {
        return Err(SevenZError::Unsupported);
    }

    let method_id = reader.read_exact(id_size)?;
    let (num_in_streams, num_out_streams) = if is_simple {
        (1u64, 1u64)
    } else {
        (reader.read_variable_u64()?, reader.read_variable_u64()?)
    };
    if num_in_streams != 1 || num_out_streams != 1 {
        return Err(SevenZError::Unsupported);
    }

    let properties = if has_attributes {
        let len = reader.read_len()?;
        reader.read_exact(len)?
    } else {
        &[]
    };

    Ok(FolderInfo {
        method: parse_method(method_id, properties)?,
        unpacked_size: 0,
        unpack_crc: None,
        num_unpack_sub_streams: 1,
        substream_sizes: Vec::new(),
        substream_crcs: Vec::new(),
    })
}

fn parse_pack_info(reader: &mut Cursor<'_>) -> Result<(u64, Vec<u64>), SevenZError> {
    let pack_pos = reader.read_variable_u64()?;
    let num_pack_streams = reader.read_len()?;

    let mut pack_sizes = None;
    loop {
        let nid = reader.read_u8()?;
        match nid {
            K_SIZE => {
                let mut sizes = Vec::with_capacity(num_pack_streams);
                for _ in 0..num_pack_streams {
                    sizes.push(reader.read_variable_u64()?);
                }
                pack_sizes = Some(sizes);
            }
            K_CRC => {
                let defined = read_all_or_bits(reader, num_pack_streams)?;
                for has_crc in defined {
                    if has_crc {
                        let _ = reader.read_u32_le()?;
                    }
                }
            }
            K_END => break,
            _ => return Err(SevenZError::Unsupported),
        }
    }

    Ok((pack_pos, pack_sizes.ok_or(SevenZError::BadHeader)?))
}

fn parse_unpack_info(reader: &mut Cursor<'_>) -> Result<Vec<FolderInfo>, SevenZError> {
    if reader.read_u8()? != K_FOLDER {
        return Err(SevenZError::BadHeader);
    }
    let num_folders = reader.read_len()?;
    if reader.read_u8()? != 0 {
        return Err(SevenZError::Unsupported);
    }

    let mut folders = Vec::with_capacity(num_folders);
    for _ in 0..num_folders {
        folders.push(parse_folder(reader)?);
    }

    if reader.read_u8()? != K_CODERS_UNPACK_SIZE {
        return Err(SevenZError::BadHeader);
    }
    for folder in &mut folders {
        folder.unpacked_size = reader.read_variable_u64()?;
    }

    loop {
        let nid = reader.read_u8()?;
        match nid {
            K_CRC => {
                let defined = read_all_or_bits(reader, folders.len())?;
                for (folder, has_crc) in folders.iter_mut().zip(defined) {
                    if has_crc {
                        folder.unpack_crc = Some(reader.read_u32_le()?);
                    }
                }
            }
            K_END => return Ok(folders),
            _ => return Err(SevenZError::Unsupported),
        }
    }
}

fn parse_sub_streams_info(
    reader: &mut Cursor<'_>,
    folders: &mut [FolderInfo],
) -> Result<(), SevenZError> {
    let mut nid = reader.read_u8()?;

    if nid == K_NUM_UNPACK_STREAM {
        for folder in folders.iter_mut() {
            folder.num_unpack_sub_streams = reader.read_len()?;
            if folder.num_unpack_sub_streams == 0 {
                return Err(SevenZError::BadHeader);
            }
        }
        nid = reader.read_u8()?;
    }

    if nid == K_SIZE {
        for folder in folders.iter_mut() {
            folder.substream_sizes.clear();
            let extra_sizes = folder.num_unpack_sub_streams.saturating_sub(1);
            let mut sum = 0u64;
            for _ in 0..extra_sizes {
                let size = reader.read_variable_u64()?;
                sum = sum.checked_add(size).ok_or(SevenZError::BadOffset)?;
                folder.substream_sizes.push(size);
            }
            if folder.num_unpack_sub_streams > 1 {
                let last = folder
                    .unpacked_size
                    .checked_sub(sum)
                    .ok_or(SevenZError::BadHeader)?;
                folder.substream_sizes.push(last);
            }
        }
        nid = reader.read_u8()?;
    }

    if nid == K_CRC {
        let num_digests = folders
            .iter()
            .map(|folder| {
                if folder.num_unpack_sub_streams == 1 && folder.unpack_crc.is_some() {
                    0
                } else {
                    folder.num_unpack_sub_streams
                }
            })
            .sum();
        let defined = read_all_or_bits(reader, num_digests)?;
        let mut digest_idx = 0usize;
        for folder in folders.iter_mut() {
            folder.substream_crcs.clear();
            if folder.num_unpack_sub_streams == 1 {
                if let Some(crc) = folder.unpack_crc {
                    folder.substream_crcs.push(Some(crc));
                    continue;
                }
            }
            for _ in 0..folder.num_unpack_sub_streams {
                let has_crc = *defined.get(digest_idx).ok_or(SevenZError::BadHeader)?;
                digest_idx += 1;
                let crc = if has_crc {
                    Some(reader.read_u32_le()?)
                } else {
                    None
                };
                folder.substream_crcs.push(crc);
            }
        }
        nid = reader.read_u8()?;
    }

    if nid != K_END {
        return Err(SevenZError::BadHeader);
    }
    Ok(())
}

fn finalize_sub_streams(folders: &mut [FolderInfo]) -> Result<(), SevenZError> {
    for folder in folders {
        if folder.num_unpack_sub_streams == 0 {
            return Err(SevenZError::BadHeader);
        }
        if folder.substream_sizes.is_empty() {
            if folder.num_unpack_sub_streams != 1 {
                return Err(SevenZError::BadHeader);
            }
            folder.substream_sizes.push(folder.unpacked_size);
        }
        if folder.substream_sizes.len() != folder.num_unpack_sub_streams {
            return Err(SevenZError::BadHeader);
        }
        let total = folder
            .substream_sizes
            .iter()
            .try_fold(0u64, |sum, size| sum.checked_add(*size))
            .ok_or(SevenZError::BadOffset)?;
        if total != folder.unpacked_size {
            return Err(SevenZError::BadHeader);
        }
        if folder.substream_crcs.is_empty() {
            if folder.num_unpack_sub_streams == 1 {
                folder.substream_crcs.push(folder.unpack_crc);
            } else {
                folder
                    .substream_crcs
                    .resize(folder.num_unpack_sub_streams, None);
            }
        }
        if folder.substream_crcs.len() != folder.num_unpack_sub_streams {
            return Err(SevenZError::BadHeader);
        }
    }
    Ok(())
}

fn parse_utf16_names(body: &mut Cursor<'_>, num_files: usize) -> Result<Vec<String>, SevenZError> {
    if body.read_u8()? != 0 {
        return Err(SevenZError::Unsupported);
    }

    let mut names = Vec::with_capacity(num_files);
    let mut cur = Vec::new();
    while names.len() < num_files {
        let lo = body.read_u8()? as u16;
        let hi = body.read_u8()? as u16;
        let ch = lo | (hi << 8);
        if ch == 0 {
            let name = String::from_utf16(&cur).map_err(|_| SevenZError::Unsupported)?;
            names.push(name);
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    Ok(names)
}

fn parse_files_info(reader: &mut Cursor<'_>) -> Result<FilesInfo, SevenZError> {
    let num_files = reader.read_len()?;

    let mut names = Vec::new();
    let mut empty_streams = vec![false; num_files];
    let mut empty_files = Vec::new();
    let mut anti_files = Vec::new();
    loop {
        let prop = reader.read_u8()?;
        if prop == K_END {
            break;
        }

        let mut body = reader.read_property_body()?;
        match prop {
            K_EMPTY_STREAM => {
                empty_streams = read_bits(&mut body, num_files)?;
            }
            K_EMPTY_FILE => {
                let empty_count = empty_streams.iter().filter(|v| **v).count();
                empty_files = read_bits(&mut body, empty_count)?;
            }
            K_ANTI => {
                let empty_count = empty_streams.iter().filter(|v| **v).count();
                anti_files = read_bits(&mut body, empty_count)?;
            }
            K_NAME => {
                names = parse_utf16_names(&mut body, num_files)?;
            }
            _ => {
                body.skip_all();
            }
        }
    }

    if names.is_empty() {
        names.resize_with(num_files, String::new);
    }

    let mut stream_indices = Vec::with_capacity(num_files);
    let mut next_stream = 0usize;
    let mut empty_idx = 0usize;
    for is_empty_stream in empty_streams {
        if is_empty_stream {
            let _is_empty_file = empty_files.get(empty_idx).copied().unwrap_or(false);
            let is_anti = anti_files.get(empty_idx).copied().unwrap_or(false);
            empty_idx += 1;
            if is_anti {
                return Err(SevenZError::Unsupported);
            }
            stream_indices.push(None);
        } else {
            stream_indices.push(Some(next_stream));
            next_stream = next_stream.checked_add(1).ok_or(SevenZError::BadOffset)?;
        }
    }

    Ok(FilesInfo {
        names,
        stream_indices,
    })
}

fn parse_archive(payload: &[u8]) -> Result<Vec<ArchiveShape<'_>>, SevenZError> {
    let next_header = read_next_header(payload)?;
    let mut reader = Cursor::new(next_header.as_slice());

    if reader.read_u8()? != K_HEADER {
        return Err(SevenZError::Unsupported);
    }

    let mut pack_pos = None;
    let mut pack_sizes = Vec::new();
    let mut folders = Vec::new();

    let mut nid = reader.read_u8()?;
    if nid == K_ARCHIVE_PROPERTIES {
        loop {
            let prop = reader.read_u8()?;
            if prop == K_END {
                break;
            }
            let mut body = reader.read_property_body()?;
            body.skip_all();
        }
        nid = reader.read_u8()?;
    }

    if nid == K_ADDITIONAL_STREAMS_INFO {
        return Err(SevenZError::Unsupported);
    }

    if nid == K_MAIN_STREAMS_INFO {
        let mut stream_nid = reader.read_u8()?;
        if stream_nid == K_PACK_INFO {
            let (parsed_pack_pos, parsed_pack_sizes) = parse_pack_info(&mut reader)?;
            pack_pos = Some(parsed_pack_pos);
            pack_sizes = parsed_pack_sizes;
            stream_nid = reader.read_u8()?;
        }

        if stream_nid != K_UNPACK_INFO {
            return Err(SevenZError::BadHeader);
        }
        folders = parse_unpack_info(&mut reader)?;
        stream_nid = reader.read_u8()?;

        if stream_nid == K_SUB_STREAMS_INFO {
            parse_sub_streams_info(&mut reader, folders.as_mut_slice())?;
            stream_nid = reader.read_u8()?;
        }

        if stream_nid != K_END {
            return Err(SevenZError::BadHeader);
        }

        nid = reader.read_u8()?;
    }

    if nid != K_FILES_INFO {
        return Err(SevenZError::BadHeader);
    }
    let files = parse_files_info(&mut reader)?;

    if reader.read_u8()? != K_END {
        return Err(SevenZError::BadHeader);
    }
    if !reader.is_empty() {
        return Err(SevenZError::Unsupported);
    }

    finalize_sub_streams(folders.as_mut_slice())?;

    let pack_pos = usize::try_from(pack_pos.ok_or(SevenZError::BadHeader)?)
        .map_err(|_| SevenZError::BadOffset)?;
    if pack_sizes.len() != folders.len() {
        return Err(SevenZError::Unsupported);
    }

    let mut pack_offsets = Vec::with_capacity(pack_sizes.len());
    let mut next_pack_offset = SIG_LEN
        .checked_add(pack_pos)
        .ok_or(SevenZError::BadOffset)?;
    for size in &pack_sizes {
        let size = usize::try_from(*size).map_err(|_| SevenZError::BadOffset)?;
        pack_offsets.push((next_pack_offset, size));
        next_pack_offset = next_pack_offset
            .checked_add(size)
            .ok_or(SevenZError::BadOffset)?;
    }

    let mut stream_map = Vec::new();
    for (folder_index, folder) in folders.iter().enumerate() {
        let mut offset = 0u64;
        for substream_index in 0..folder.num_unpack_sub_streams {
            let size = *folder
                .substream_sizes
                .get(substream_index)
                .ok_or(SevenZError::BadHeader)?;
            let crc = *folder
                .substream_crcs
                .get(substream_index)
                .ok_or(SevenZError::BadHeader)?;
            stream_map.push((folder_index, offset, size, crc));
            offset = offset.checked_add(size).ok_or(SevenZError::BadOffset)?;
        }
    }

    let mut archives = Vec::new();
    for (file_index, stream_index) in files.stream_indices.iter().enumerate() {
        let Some(stream_index) = *stream_index else {
            continue;
        };
        let (folder_index, substream_offset, substream_size, substream_crc) =
            *stream_map.get(stream_index).ok_or(SevenZError::BadHeader)?;
        let folder = folders.get(folder_index).ok_or(SevenZError::BadHeader)?;
        let (packed_start, pack_size) = *pack_offsets
            .get(folder_index)
            .ok_or(SevenZError::BadHeader)?;
        let packed_end = packed_start
            .checked_add(pack_size)
            .ok_or(SevenZError::BadOffset)?;
        let packed_stream = payload
            .get(packed_start..packed_end)
            .ok_or(SevenZError::BadOffset)?;

        archives.push(ArchiveShape {
            name: files
                .names
                .get(file_index)
                .cloned()
                .ok_or(SevenZError::BadHeader)?,
            packed_stream,
            method: folder.method,
            folder_unpacked_size: usize::try_from(folder.unpacked_size)
                .map_err(|_| SevenZError::BadOffset)?,
            folder_crc: folder.unpack_crc,
            substream_offset: usize::try_from(substream_offset)
                .map_err(|_| SevenZError::BadOffset)?,
            substream_size: usize::try_from(substream_size).map_err(|_| SevenZError::BadOffset)?,
            substream_crc,
        });
    }

    Ok(archives)
}

fn parse_single_file_archive(payload: &[u8]) -> Result<ArchiveShape<'_>, SevenZError> {
    let mut archives = parse_archive(payload)?;
    if archives.len() != 1 {
        return Err(SevenZError::Unsupported);
    }
    Ok(archives.remove(0))
}

pub fn looks_like_7z(b: &[u8]) -> bool {
    b.get(0..6) == Some(b"7z\xBC\xAF'\x1C")
}

pub fn lzma2_decompress_to_vec(data: &[u8], dict_size: u32) -> Result<Vec<u8>, SevenZError> {
    let mut reader = lzma_rust2::Lzma2Reader::new(data, dict_size, None);
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];

    loop {
        let got =
            lzma_rust2::Read::read(&mut reader, &mut buf).map_err(|_| SevenZError::DecodeFailed)?;
        if got == 0 {
            break;
        }
        out.extend_from_slice(&buf[..got]);
    }

    Ok(out)
}

pub fn lzma_decompress_to_vec(
    data: &[u8],
    props: u8,
    dict_size: u32,
    unpacked_size: usize,
) -> Result<Vec<u8>, SevenZError> {
    let mut reader =
        lzma_rust2::LzmaReader::new_with_props(data, unpacked_size as u64, props, dict_size, None)
            .map_err(|_| SevenZError::DecodeFailed)?;
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];

    loop {
        let got =
            lzma_rust2::Read::read(&mut reader, &mut buf).map_err(|_| SevenZError::DecodeFailed)?;
        if got == 0 {
            break;
        }
        out.extend_from_slice(&buf[..got]);
    }

    Ok(out)
}

pub fn extract_single_file_to_vec(payload: &[u8]) -> Result<Vec<u8>, SevenZError> {
    let archive = parse_single_file_archive(payload)?;
    extract_archive_shape_to_vec(&archive)
}

fn decode_folder_to_vec(archive: &ArchiveShape<'_>) -> Result<Vec<u8>, SevenZError> {
    let out = match archive.method {
        Method::Copy => archive.packed_stream.to_vec(),
        Method::Lzma { props, dict_size } => lzma_decompress_to_vec(
            archive.packed_stream,
            props,
            dict_size,
            archive.folder_unpacked_size,
        )?,
        Method::Lzma2 { dict_size } => lzma2_decompress_to_vec(archive.packed_stream, dict_size)?,
    };

    if out.len() != archive.folder_unpacked_size {
        return Err(SevenZError::DecodeFailed);
    }
    if let Some(expected_crc) = archive.folder_crc {
        if crc32fast::hash(&out) != expected_crc {
            return Err(SevenZError::BadCrc);
        }
    }

    Ok(out)
}

fn extract_archive_substream_to_vec(
    archive: &ArchiveShape<'_>,
    folder: &[u8],
) -> Result<Vec<u8>, SevenZError> {
    let end = archive
        .substream_offset
        .checked_add(archive.substream_size)
        .ok_or(SevenZError::BadOffset)?;
    let slice = folder
        .get(archive.substream_offset..end)
        .ok_or(SevenZError::BadOffset)?;
    if let Some(expected_crc) = archive.substream_crc {
        if crc32fast::hash(slice) != expected_crc {
            return Err(SevenZError::BadCrc);
        }
    }

    Ok(slice.to_vec())
}

fn extract_archive_shape_to_vec(archive: &ArchiveShape<'_>) -> Result<Vec<u8>, SevenZError> {
    let folder = decode_folder_to_vec(archive)?;
    extract_archive_substream_to_vec(archive, folder.as_slice())
}

pub fn extract_all_to_vec(payload: &[u8]) -> Result<Vec<SevenZEntry>, SevenZError> {
    let archives = parse_archive(payload)?;
    let mut out = Vec::with_capacity(archives.len());
    let mut cached_ptr: *const u8 = core::ptr::null();
    let mut cached_len = 0usize;
    let mut cached_folder = Vec::new();

    for archive in &archives {
        if archive.packed_stream.as_ptr() != cached_ptr || archive.packed_stream.len() != cached_len
        {
            cached_folder = decode_folder_to_vec(archive)?;
            cached_ptr = archive.packed_stream.as_ptr();
            cached_len = archive.packed_stream.len();
        }
        out.push(SevenZEntry {
            name: archive.name.clone(),
            bytes: extract_archive_substream_to_vec(archive, cached_folder.as_slice())?,
        });
    }
    Ok(out)
}

pub fn list_entries(payload: &[u8]) -> Result<Vec<SevenZEntryInfo>, SevenZError> {
    let archives = parse_archive(payload)?;
    let mut out = Vec::with_capacity(archives.len());
    for archive in &archives {
        out.push(SevenZEntryInfo {
            name: archive.name.clone(),
            unpacked_size: archive.substream_size,
            crc: archive.substream_crc,
        });
    }
    Ok(out)
}

pub fn extract_file_to_vec(payload: &[u8], wanted_name: &str) -> Result<Vec<u8>, SevenZError> {
    let archives = parse_archive(payload)?;
    let mut suffix = String::from("/");
    suffix.push_str(wanted_name);

    for archive in &archives {
        if archive.name == wanted_name || archive.name.ends_with(suffix.as_str()) {
            return extract_archive_shape_to_vec(archive);
        }
    }

    Err(SevenZError::BadHeader)
}
