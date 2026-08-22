#![no_std]
#![deny(unsafe_code)]

//! Zero-copy parser for the exact resident Kokoro `voices-v1.0.bin` archive.
//!
//! This is intentionally not a general ZIP or NPY implementation. It admits
//! the pinned archive's 54 sorted, stored `<voice>.npy` entries and validates
//! every central/local header, ZIP64 local-size extra, CRC-32, NPY header,
//! finite float payload, and the whole-archive SHA-256 before returning borrowed
//! views. Decoding copies only one selected `[f32; 256]` style row into
//! caller-owned storage.

use sha2::{Digest, Sha256};

pub const PINNED_ARCHIVE_BYTES: usize = 28_214_398;
pub const PINNED_ENTRY_COUNT: usize = 54;
pub const PINNED_DIRECTORY_OFFSET: usize = 28_211_232;
pub const PINNED_DIRECTORY_BYTES: usize = 3_144;
pub const PINNED_ARCHIVE_SHA256: [u8; 32] = [
    0xbc, 0xa6, 0x10, 0xb8, 0x30, 0x8e, 0x8d, 0x99, 0xf3, 0x2e, 0x6f, 0xe4, 0x19, 0x7e, 0x7e, 0xc0,
    0x16, 0x79, 0x26, 0x4e, 0xfe, 0xd0, 0xca, 0xc9, 0x14, 0x0f, 0xe9, 0xc2, 0x9f, 0x1f, 0xbf, 0x7d,
];
pub const PINNED_VOICE_NAMES: [&str; PINNED_ENTRY_COUNT] = [
    "af_alloy",
    "af_aoede",
    "af_bella",
    "af_heart",
    "af_jessica",
    "af_kore",
    "af_nicole",
    "af_nova",
    "af_river",
    "af_sarah",
    "af_sky",
    "am_adam",
    "am_echo",
    "am_eric",
    "am_fenrir",
    "am_liam",
    "am_michael",
    "am_onyx",
    "am_puck",
    "am_santa",
    "bf_alice",
    "bf_emma",
    "bf_isabella",
    "bf_lily",
    "bm_daniel",
    "bm_fable",
    "bm_george",
    "bm_lewis",
    "ef_dora",
    "em_alex",
    "em_santa",
    "ff_siwis",
    "hf_alpha",
    "hf_beta",
    "hm_omega",
    "hm_psi",
    "if_sara",
    "im_nicola",
    "jf_alpha",
    "jf_gongitsune",
    "jf_nezumi",
    "jf_tebukuro",
    "jm_kumo",
    "pf_dora",
    "pm_alex",
    "pm_santa",
    "zf_xiaobei",
    "zf_xiaoni",
    "zf_xiaoxiao",
    "zf_xiaoyi",
    "zm_yunjian",
    "zm_yunxi",
    "zm_yunxia",
    "zm_yunyang",
];

pub const STYLES_PER_VOICE: usize = 510;
pub const STYLE_CHANNELS: usize = 1;
pub const STYLE_WIDTH: usize = 256;
pub const STYLE_BYTES: usize = STYLE_WIDTH * core::mem::size_of::<f32>();
pub const NPY_HEADER_BYTES: usize = 128;
pub const NPY_PAYLOAD_BYTES: usize = STYLES_PER_VOICE * STYLE_CHANNELS * STYLE_BYTES;
pub const NPY_FILE_BYTES: usize = NPY_HEADER_BYTES + NPY_PAYLOAD_BYTES;

const LOCAL_FIXED_BYTES: usize = 30;
const CENTRAL_FIXED_BYTES: usize = 46;
const EOCD_BYTES: usize = 22;
const LOCAL_ZIP64_EXTRA_BYTES: usize = 20;
const MAX_STYLE_INDEX: usize = STYLES_PER_VOICE - 1;

const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const ZIP64_EXTRA_ID: u16 = 0x0001;
const ZIP64_PLACEHOLDER: u32 = u32::MAX;
const STORED_METHOD: u16 = 0;
const ZIP_VERSION_45: u16 = 45;

const NPY_HEADER: &[u8; 118] =
    b"{'descr': '<f4', 'fortran_order': False, 'shape': (510, 1, 256), }                                                   \n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    ArchiveSizeMismatch,
    ArchiveDigestMismatch,
    EndOfCentralDirectory,
    MultiDiskArchive,
    EntryCountMismatch,
    DirectoryBounds,
    DirectoryLayout,
    CentralHeader,
    LocalHeader,
    UnsupportedVersion,
    EncryptedEntry,
    DataDescriptor,
    UnsupportedFlags,
    UnsupportedMethod,
    UnexpectedExtra,
    UnexpectedComment,
    SizeMismatch,
    OffsetMismatch,
    InvalidName,
    DuplicateOrUnsortedName,
    MalformedZip64Extra,
    LocalCentralMismatch,
    CrcMismatch,
    InvalidNpy,
    NonFiniteStyle,
    VoiceNotFound,
    Truncated,
}

/// Fully validated borrowed archive.
#[derive(Clone, Copy, Debug)]
pub struct VoiceArchive<'a> {
    bytes: &'a [u8],
    directory_offset: usize,
    directory_end: usize,
}

impl<'a> VoiceArchive<'a> {
    /// Validate the complete pinned archive before exposing any entry.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() != PINNED_ARCHIVE_BYTES {
            return Err(Error::ArchiveSizeMismatch);
        }
        let eocd_offset = bytes
            .len()
            .checked_sub(EOCD_BYTES)
            .ok_or(Error::EndOfCentralDirectory)?;
        if read_u32(bytes, eocd_offset)? != EOCD_SIGNATURE {
            return Err(Error::EndOfCentralDirectory);
        }
        if read_u16(bytes, eocd_offset + 4)? != 0 || read_u16(bytes, eocd_offset + 6)? != 0 {
            return Err(Error::MultiDiskArchive);
        }
        let entries_on_disk = read_u16(bytes, eocd_offset + 8)? as usize;
        let entries = read_u16(bytes, eocd_offset + 10)? as usize;
        if entries_on_disk != PINNED_ENTRY_COUNT || entries != PINNED_ENTRY_COUNT {
            return Err(Error::EntryCountMismatch);
        }
        let directory_bytes = read_u32(bytes, eocd_offset + 12)? as usize;
        let directory_offset = read_u32(bytes, eocd_offset + 16)? as usize;
        if directory_bytes != PINNED_DIRECTORY_BYTES || directory_offset != PINNED_DIRECTORY_OFFSET
        {
            return Err(Error::DirectoryLayout);
        }
        if read_u16(bytes, eocd_offset + 20)? != 0 {
            return Err(Error::UnexpectedComment);
        }
        let directory_end = directory_offset
            .checked_add(directory_bytes)
            .ok_or(Error::DirectoryBounds)?;
        if directory_end != eocd_offset {
            return Err(Error::DirectoryBounds);
        }

        let archive = Self {
            bytes,
            directory_offset,
            directory_end,
        };
        archive.validate_entries()?;
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if digest != PINNED_ARCHIVE_SHA256 {
            return Err(Error::ArchiveDigestMismatch);
        }
        Ok(archive)
    }

    pub const fn len(self) -> usize {
        PINNED_ENTRY_COUNT
    }

    pub const fn is_empty(self) -> bool {
        false
    }

    /// Iterate entries in their validated strict lexical order.
    pub const fn voices(self) -> VoiceIter<'a> {
        VoiceIter {
            bytes: self.bytes,
            cursor: self.directory_offset,
            directory_end: self.directory_end,
            remaining: PINNED_ENTRY_COUNT,
        }
    }

    /// Find an exact case-sensitive voice stem such as `af_heart`.
    pub fn lookup(self, voice: &str) -> Result<Voice<'a>, Error> {
        validate_voice_stem(voice.as_bytes())?;
        for candidate in self.voices() {
            let candidate = candidate?;
            match candidate.name().as_bytes().cmp(voice.as_bytes()) {
                core::cmp::Ordering::Less => {}
                core::cmp::Ordering::Equal => return Ok(candidate),
                core::cmp::Ordering::Greater => break,
            }
        }
        Err(Error::VoiceNotFound)
    }

    fn validate_entries(self) -> Result<(), Error> {
        let mut cursor = self.directory_offset;
        let mut expected_local_offset = 0usize;
        let mut previous_name: Option<&[u8]> = None;
        for expected_name in PINNED_VOICE_NAMES {
            let record = central_record(self.bytes, cursor, self.directory_end)?;
            if record.local_offset != expected_local_offset {
                return Err(Error::OffsetMismatch);
            }
            if previous_name.is_some_and(|previous| previous >= record.stem.as_bytes()) {
                return Err(Error::DuplicateOrUnsortedName);
            }
            if record.stem != expected_name {
                return Err(Error::InvalidName);
            }
            let local = local_npy(self.bytes, record, self.directory_offset)?;
            validate_npy(local.npy)?;
            if crc32(local.npy) != record.crc32 {
                return Err(Error::CrcMismatch);
            }
            expected_local_offset = local.end;
            previous_name = Some(record.stem.as_bytes());
            cursor = record.next;
        }
        if cursor != self.directory_end || expected_local_offset != self.directory_offset {
            return Err(Error::DirectoryLayout);
        }
        Ok(())
    }
}

/// One zero-copy voice entry.
#[derive(Clone, Copy, Debug)]
pub struct Voice<'a> {
    name: &'a str,
    npy: &'a [u8],
    payload: &'a [u8],
    crc32: u32,
}

impl<'a> Voice<'a> {
    pub const fn name(self) -> &'a str {
        self.name
    }

    pub const fn npy_bytes(self) -> &'a [u8] {
        self.npy
    }

    pub const fn crc32(self) -> u32 {
        self.crc32
    }

    /// Decode the host-compatible style row selected by original phoneme count.
    ///
    /// Counts above 509 clamp to 509. The complete row is checked for finite
    /// values before `output` is modified.
    pub fn decode_style(
        self,
        original_phoneme_count: usize,
        output: &mut [f32; STYLE_WIDTH],
    ) -> Result<usize, Error> {
        let index = style_index(original_phoneme_count);
        let start = index * STYLE_BYTES;
        let row = self
            .payload
            .get(start..start + STYLE_BYTES)
            .ok_or(Error::InvalidNpy)?;
        let encoded_values = encoded_f32s(row)?;
        for &encoded in encoded_values {
            let value = f32::from_le_bytes(encoded);
            if !value.is_finite() {
                return Err(Error::NonFiniteStyle);
            }
        }
        for (destination, &encoded) in output.iter_mut().zip(encoded_values) {
            *destination = f32::from_le_bytes(encoded);
        }
        Ok(index)
    }
}

/// Host-compatible voice embedding row selection.
pub const fn style_index(original_phoneme_count: usize) -> usize {
    if original_phoneme_count > MAX_STYLE_INDEX {
        MAX_STYLE_INDEX
    } else {
        original_phoneme_count
    }
}

/// Deterministic sorted archive iterator.
#[derive(Clone, Copy, Debug)]
pub struct VoiceIter<'a> {
    bytes: &'a [u8],
    cursor: usize,
    directory_end: usize,
    remaining: usize,
}

impl<'a> Iterator for VoiceIter<'a> {
    type Item = Result<Voice<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let record = match central_record(self.bytes, self.cursor, self.directory_end) {
            Ok(record) => record,
            Err(error) => {
                self.remaining = 0;
                return Some(Err(error));
            }
        };
        self.cursor = record.next;
        self.remaining -= 1;
        let local = match local_npy(self.bytes, record, PINNED_DIRECTORY_OFFSET) {
            Ok(local) => local,
            Err(error) => {
                self.remaining = 0;
                return Some(Err(error));
            }
        };
        Some(validate_npy_header(local.npy).map(|payload| Voice {
            name: record.stem,
            npy: local.npy,
            payload,
            crc32: record.crc32,
        }))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for VoiceIter<'_> {}

#[derive(Clone, Copy)]
struct CentralRecord<'a> {
    file_name: &'a [u8],
    stem: &'a str,
    crc32: u32,
    local_offset: usize,
    next: usize,
}

#[derive(Clone, Copy)]
struct LocalNpy<'a> {
    npy: &'a [u8],
    end: usize,
}

fn central_record<'a>(
    bytes: &'a [u8],
    offset: usize,
    directory_end: usize,
) -> Result<CentralRecord<'a>, Error> {
    let fixed_end = offset
        .checked_add(CENTRAL_FIXED_BYTES)
        .ok_or(Error::DirectoryBounds)?;
    if fixed_end > directory_end || read_u32(bytes, offset)? != CENTRAL_SIGNATURE {
        return Err(Error::CentralHeader);
    }
    if read_u16(bytes, offset + 6)? != ZIP_VERSION_45 {
        return Err(Error::UnsupportedVersion);
    }
    validate_flags(read_u16(bytes, offset + 8)?)?;
    if read_u16(bytes, offset + 10)? != STORED_METHOD {
        return Err(Error::UnsupportedMethod);
    }
    let crc32 = read_u32(bytes, offset + 16)?;
    if read_u32(bytes, offset + 20)? as usize != NPY_FILE_BYTES
        || read_u32(bytes, offset + 24)? as usize != NPY_FILE_BYTES
    {
        return Err(Error::SizeMismatch);
    }
    let name_length = read_u16(bytes, offset + 28)? as usize;
    let extra_length = read_u16(bytes, offset + 30)? as usize;
    let comment_length = read_u16(bytes, offset + 32)? as usize;
    if extra_length != 0 {
        return Err(Error::UnexpectedExtra);
    }
    if comment_length != 0 {
        return Err(Error::UnexpectedComment);
    }
    if read_u16(bytes, offset + 34)? != 0 {
        return Err(Error::MultiDiskArchive);
    }
    let local_offset = read_u32(bytes, offset + 42)?;
    if local_offset == ZIP64_PLACEHOLDER {
        return Err(Error::MalformedZip64Extra);
    }
    let name_start = fixed_end;
    let next = name_start
        .checked_add(name_length)
        .and_then(|value| value.checked_add(extra_length))
        .and_then(|value| value.checked_add(comment_length))
        .ok_or(Error::DirectoryBounds)?;
    if next > directory_end {
        return Err(Error::DirectoryBounds);
    }
    let file_name = bytes
        .get(name_start..name_start + name_length)
        .ok_or(Error::Truncated)?;
    let stem = validate_archive_name(file_name)?;
    Ok(CentralRecord {
        file_name,
        stem,
        crc32,
        local_offset: local_offset as usize,
        next,
    })
}

fn local_npy<'a>(
    bytes: &'a [u8],
    record: CentralRecord<'_>,
    directory_offset: usize,
) -> Result<LocalNpy<'a>, Error> {
    let offset = record.local_offset;
    let fixed_end = offset
        .checked_add(LOCAL_FIXED_BYTES)
        .ok_or(Error::Truncated)?;
    if fixed_end > directory_offset || read_u32(bytes, offset)? != LOCAL_SIGNATURE {
        return Err(Error::LocalHeader);
    }
    if read_u16(bytes, offset + 4)? != ZIP_VERSION_45 {
        return Err(Error::UnsupportedVersion);
    }
    validate_flags(read_u16(bytes, offset + 6)?)?;
    if read_u16(bytes, offset + 8)? != STORED_METHOD {
        return Err(Error::UnsupportedMethod);
    }
    if read_u32(bytes, offset + 14)? != record.crc32 {
        return Err(Error::LocalCentralMismatch);
    }
    if read_u32(bytes, offset + 18)? != ZIP64_PLACEHOLDER
        || read_u32(bytes, offset + 22)? != ZIP64_PLACEHOLDER
    {
        return Err(Error::MalformedZip64Extra);
    }
    let name_length = read_u16(bytes, offset + 26)? as usize;
    let extra_length = read_u16(bytes, offset + 28)? as usize;
    if name_length != record.file_name.len() || extra_length != LOCAL_ZIP64_EXTRA_BYTES {
        return Err(Error::LocalCentralMismatch);
    }
    let name_start = fixed_end;
    let extra_start = name_start
        .checked_add(name_length)
        .ok_or(Error::Truncated)?;
    let data_start = extra_start
        .checked_add(extra_length)
        .ok_or(Error::Truncated)?;
    let data_end = data_start
        .checked_add(NPY_FILE_BYTES)
        .ok_or(Error::Truncated)?;
    if data_end > directory_offset {
        return Err(Error::Truncated);
    }
    if bytes.get(name_start..extra_start).ok_or(Error::Truncated)? != record.file_name {
        return Err(Error::LocalCentralMismatch);
    }
    let extra = bytes.get(extra_start..data_start).ok_or(Error::Truncated)?;
    validate_zip64_extra(extra)?;
    let npy = bytes.get(data_start..data_end).ok_or(Error::Truncated)?;
    Ok(LocalNpy { npy, end: data_end })
}

fn validate_flags(flags: u16) -> Result<(), Error> {
    if flags & 1 != 0 {
        return Err(Error::EncryptedEntry);
    }
    if flags & (1 << 3) != 0 {
        return Err(Error::DataDescriptor);
    }
    if flags != 0 {
        return Err(Error::UnsupportedFlags);
    }
    Ok(())
}

fn validate_zip64_extra(extra: &[u8]) -> Result<(), Error> {
    if extra.len() != LOCAL_ZIP64_EXTRA_BYTES
        || read_u16(extra, 0)? != ZIP64_EXTRA_ID
        || read_u16(extra, 2)? != 16
        || read_u64(extra, 4)? != NPY_FILE_BYTES as u64
        || read_u64(extra, 12)? != NPY_FILE_BYTES as u64
    {
        return Err(Error::MalformedZip64Extra);
    }
    Ok(())
}

fn validate_archive_name(file_name: &[u8]) -> Result<&str, Error> {
    let stem = file_name.strip_suffix(b".npy").ok_or(Error::InvalidName)?;
    validate_voice_stem(stem)?;
    core::str::from_utf8(stem).map_err(|_| Error::InvalidName)
}

fn validate_voice_stem(stem: &[u8]) -> Result<(), Error> {
    if stem.len() < 4
        || stem.len() > 13
        || stem[2] != b'_'
        || !stem
            .iter()
            .copied()
            .all(|byte| byte == b'_' || byte.is_ascii_lowercase())
    {
        return Err(Error::InvalidName);
    }
    Ok(())
}

fn validate_npy(npy: &[u8]) -> Result<(), Error> {
    let payload = validate_npy_header(npy)?;
    for &encoded in encoded_f32s(payload)? {
        let value = f32::from_le_bytes(encoded);
        if !value.is_finite() {
            return Err(Error::NonFiniteStyle);
        }
    }
    Ok(())
}

fn validate_npy_header(npy: &[u8]) -> Result<&[u8], Error> {
    if npy.len() != NPY_FILE_BYTES
        || npy.get(..6) != Some(b"\x93NUMPY")
        || npy.get(6..8) != Some(&[1, 0])
        || read_u16(npy, 8)? as usize != NPY_HEADER.len()
        || npy.get(10..NPY_HEADER_BYTES) != Some(NPY_HEADER)
    {
        return Err(Error::InvalidNpy);
    }
    npy.get(NPY_HEADER_BYTES..).ok_or(Error::InvalidNpy)
}

fn encoded_f32s(bytes: &[u8]) -> Result<&[[u8; 4]], Error> {
    let (encoded, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err(Error::InvalidNpy);
    }
    Ok(encoded)
}

const fn make_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0usize;
    while index < table.len() {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 0 {
                value >> 1
            } else {
                (value >> 1) ^ 0xedb8_8320
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

const CRC32_TABLE: [u32; 256] = make_crc32_table();

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        let index = ((crc as u8) ^ byte) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    !crc
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let end = offset.checked_add(2).ok_or(Error::Truncated)?;
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..end)
            .ok_or(Error::Truncated)?
            .try_into()
            .map_err(|_| Error::Truncated)?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error::Truncated)?;
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..end)
            .ok_or(Error::Truncated)?
            .try_into()
            .map_err(|_| Error::Truncated)?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    let end = offset.checked_add(8).ok_or(Error::Truncated)?;
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..end)
            .ok_or(Error::Truncated)?
            .try_into()
            .map_err(|_| Error::Truncated)?,
    ))
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
