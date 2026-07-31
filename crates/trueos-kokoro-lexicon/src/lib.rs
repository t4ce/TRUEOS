#![no_std]
#![deny(unsafe_code)]

//! Allocation-free parser and lookup engine for TRUEOS's sealed Misaki US
//! pronunciation overlay.
//!
//! The JSON dictionaries are compiled offline. Runtime parsing borrows the
//! complete artifact, validates its canonical layout and SHA-256 seal, and
//! performs binary searches directly over fixed-width records.

use core::{cmp::Ordering, str};

use sha2::{Digest, Sha256};
use trueos_kokoro_g2p::PronunciationLookup;

const MAGIC: &[u8; 8] = b"TRKLEX1\0";
const VERSION: u16 = 1;
const HEADER_BYTES: usize = 256;
const ENTRY_RECORD_BYTES: usize = 12;
const VARIANT_RECORD_BYTES: usize = 16;
const FLAG_POS_VARIANTS: u32 = 1;
const SUPPORTED_FLAGS: u32 = FLAG_POS_VARIANTS;

const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 500_000;
const MAX_VARIANTS: usize = 4_096;
const MAX_WORD_BYTES: usize = 256;
const MAX_PRONUNCIATION_BYTES: usize = 512;
const MAX_TAG_BYTES: usize = 32;

const DIGEST_OFFSET: usize = 72;
const DIGEST_END: usize = DIGEST_OFFSET + 32;
const SILVER_DIGEST_OFFSET: usize = 104;
const GOLD_DIGEST_OFFSET: usize = 136;
const LICENSE_DIGEST_OFFSET: usize = 168;
const SOURCE_COMMIT_OFFSET: usize = 200;
const HEADER_RESERVED_OFFSET: usize = 220;

/// TRUEOSFS-relative path consumed by the model-residency service.
pub const PINNED_US_PATH: &str = "models/kokoro/misaki-us.klex";
pub const PINNED_US_ENTRIES: usize = 389_904;
pub const PINNED_US_VARIANTS: usize = 41;
pub const PINNED_US_BYTES: usize = 15_844_468;
pub const PINNED_US_SHA256: [u8; 32] = [
    0xdf, 0x5e, 0x2a, 0x52, 0x11, 0x0c, 0x70, 0xc3, 0xb0, 0x4a, 0x72, 0x2b, 0xb2, 0x4f, 0xc4, 0xfa,
    0x59, 0xf2, 0x45, 0x7d, 0xcb, 0x7b, 0x4b, 0x3a, 0x5c, 0x11, 0x0f, 0xf6, 0x0a, 0x4c, 0xa0, 0x3b,
];

pub const MISAKI_US_SILVER_SHA256: [u8; 32] = [
    0x57, 0xca, 0xe2, 0xa1, 0xa9, 0xd7, 0x3c, 0xe2, 0x19, 0xad, 0x91, 0x42, 0xb0, 0xd9, 0x04, 0x91,
    0x4a, 0x02, 0x28, 0xcb, 0x1b, 0xab, 0xce, 0x20, 0xe5, 0xbf, 0xd4, 0xe1, 0xb1, 0x30, 0x7e, 0xe4,
];
pub const MISAKI_US_GOLD_SHA256: [u8; 32] = [
    0xbb, 0x83, 0xc8, 0x99, 0xd8, 0xdb, 0xfa, 0x16, 0x0f, 0xa0, 0x56, 0x61, 0xbe, 0xa0, 0x52, 0xba,
    0xcf, 0xee, 0xce, 0x9b, 0x63, 0x98, 0x51, 0x66, 0x23, 0x34, 0xe8, 0x50, 0x02, 0xee, 0x8a, 0xd9,
];
pub const MISAKI_LICENSE_SHA256: [u8; 32] = [
    0x1b, 0xea, 0x4b, 0x79, 0xe6, 0x60, 0xb7, 0x47, 0x7e, 0xa5, 0x91, 0x9b, 0xed, 0x59, 0x44, 0xd9,
    0x70, 0xc8, 0x65, 0x31, 0xb5, 0x08, 0xbd, 0x1d, 0x53, 0x83, 0x09, 0xc0, 0xd1, 0x2e, 0x88, 0x58,
];
/// Raw SHA-1 object ID for misaki-rs commit
/// `7bbe06cacd9102d8a0d9e338a3711ae7208de0ad`.
pub const MISAKI_SOURCE_COMMIT: [u8; 20] = [
    0x7b, 0xbe, 0x06, 0xca, 0xcd, 0x91, 0x02, 0xd8, 0xa0, 0xd9, 0xe3, 0x38, 0xa3, 0x71, 0x1a, 0xe7,
    0x20, 0x8d, 0xe0, 0xad,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexiconError {
    ArtifactTooLarge,
    TruncatedHeader,
    BadMagic,
    UnsupportedVersion,
    BadHeaderSize,
    UnsupportedFlags,
    BadRecordSize,
    NonZeroReserved,
    CountLimit,
    ArithmeticOverflow,
    OffsetMismatch,
    SizeMismatch,
    MissingProvenance,
    ArtifactDigestMismatch,
    EmptyWord,
    EmptyPronunciation,
    EmptyVariantTag,
    WordTooLong,
    PronunciationTooLong,
    VariantTagTooLong,
    StringOutOfBounds,
    InvalidUtf8,
    NonCanonicalStringPool,
    UnsortedOrDuplicateWord,
    InvalidVariantEntry,
    UnsortedOrDuplicateVariant,
    PinnedSizeMismatch,
    PinnedCountMismatch,
    PinnedProvenanceMismatch,
    PinnedDigestMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Provenance {
    pub silver_sha256: [u8; 32],
    pub gold_sha256: [u8; 32],
    pub license_sha256: [u8; 32],
    pub source_commit: [u8; 20],
}

#[derive(Clone, Copy)]
struct EntryRecord {
    word_offset: usize,
    pronunciation_offset: usize,
    word_len: usize,
    pronunciation_len: usize,
}

#[derive(Clone, Copy)]
struct VariantRecord {
    entry_index: usize,
    tag_offset: usize,
    pronunciation_offset: usize,
    tag_len: usize,
    pronunciation_len: usize,
}

/// A validated, zero-copy view over one canonical KLEX v1 artifact.
pub struct Lexicon<'a> {
    bytes: &'a [u8],
    entries: &'a [u8],
    variants: &'a [u8],
    strings: &'a [u8],
    entry_count: usize,
    variant_count: usize,
    provenance: Provenance,
}

impl<'a> Lexicon<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, LexiconError> {
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(LexiconError::ArtifactTooLarge);
        }
        if bytes.len() < HEADER_BYTES {
            return Err(LexiconError::TruncatedHeader);
        }
        if bytes.get(..MAGIC.len()) != Some(MAGIC) {
            return Err(LexiconError::BadMagic);
        }
        if read_u16(bytes, 8)? != VERSION {
            return Err(LexiconError::UnsupportedVersion);
        }
        if usize::from(read_u16(bytes, 10)?) != HEADER_BYTES {
            return Err(LexiconError::BadHeaderSize);
        }
        let flags = read_u32(bytes, 12)?;
        if flags & !SUPPORTED_FLAGS != 0 {
            return Err(LexiconError::UnsupportedFlags);
        }
        let entry_count = read_count(bytes, 16, MAX_ENTRIES)?;
        let variant_count = read_count(bytes, 20, MAX_VARIANTS)?;
        if usize::from(read_u16(bytes, 24)?) != ENTRY_RECORD_BYTES
            || usize::from(read_u16(bytes, 26)?) != VARIANT_RECORD_BYTES
        {
            return Err(LexiconError::BadRecordSize);
        }
        if read_u32(bytes, 28)? != 0
            || bytes[HEADER_RESERVED_OFFSET..HEADER_BYTES]
                .iter()
                .any(|&byte| byte != 0)
        {
            return Err(LexiconError::NonZeroReserved);
        }
        let expected_flags = if variant_count == 0 {
            0
        } else {
            FLAG_POS_VARIANTS
        };
        if flags != expected_flags {
            return Err(LexiconError::UnsupportedFlags);
        }

        let entries_offset = read_offset(bytes, 32)?;
        let variants_offset = read_offset(bytes, 40)?;
        let strings_offset = read_offset(bytes, 48)?;
        let declared_file_bytes = read_offset(bytes, 56)?;
        let declared_string_bytes = read_offset(bytes, 64)?;
        let entry_bytes = entry_count
            .checked_mul(ENTRY_RECORD_BYTES)
            .ok_or(LexiconError::ArithmeticOverflow)?;
        let variant_bytes = variant_count
            .checked_mul(VARIANT_RECORD_BYTES)
            .ok_or(LexiconError::ArithmeticOverflow)?;
        let expected_variants_offset = HEADER_BYTES
            .checked_add(entry_bytes)
            .ok_or(LexiconError::ArithmeticOverflow)?;
        let expected_strings_offset = expected_variants_offset
            .checked_add(variant_bytes)
            .ok_or(LexiconError::ArithmeticOverflow)?;
        let expected_file_bytes = expected_strings_offset
            .checked_add(declared_string_bytes)
            .ok_or(LexiconError::ArithmeticOverflow)?;
        if entries_offset != HEADER_BYTES
            || variants_offset != expected_variants_offset
            || strings_offset != expected_strings_offset
        {
            return Err(LexiconError::OffsetMismatch);
        }
        if declared_file_bytes != bytes.len() || expected_file_bytes != bytes.len() {
            return Err(LexiconError::SizeMismatch);
        }

        let stored_digest = array_at::<32>(bytes, DIGEST_OFFSET)?;
        let provenance = Provenance {
            silver_sha256: array_at(bytes, SILVER_DIGEST_OFFSET)?,
            gold_sha256: array_at(bytes, GOLD_DIGEST_OFFSET)?,
            license_sha256: array_at(bytes, LICENSE_DIGEST_OFFSET)?,
            source_commit: array_at(bytes, SOURCE_COMMIT_OFFSET)?,
        };
        if provenance.silver_sha256.iter().all(|&byte| byte == 0)
            || provenance.gold_sha256.iter().all(|&byte| byte == 0)
            || provenance.license_sha256.iter().all(|&byte| byte == 0)
            || provenance.source_commit.iter().all(|&byte| byte == 0)
        {
            return Err(LexiconError::MissingProvenance);
        }
        let mut hasher = Sha256::new();
        hasher.update(&bytes[..DIGEST_OFFSET]);
        hasher.update([0u8; 32]);
        hasher.update(&bytes[DIGEST_END..]);
        let computed_digest: [u8; 32] = hasher.finalize().into();
        if computed_digest != stored_digest {
            return Err(LexiconError::ArtifactDigestMismatch);
        }

        let entries = bytes
            .get(entries_offset..variants_offset)
            .ok_or(LexiconError::SizeMismatch)?;
        let variants = bytes
            .get(variants_offset..strings_offset)
            .ok_or(LexiconError::SizeMismatch)?;
        let strings = bytes
            .get(strings_offset..)
            .ok_or(LexiconError::SizeMismatch)?;
        let lexicon = Self {
            bytes,
            entries,
            variants,
            strings,
            entry_count,
            variant_count,
            provenance,
        };
        lexicon.validate_records()?;
        Ok(lexicon)
    }

    pub fn parse_pinned_us(bytes: &'a [u8]) -> Result<Self, LexiconError> {
        if bytes.len() != PINNED_US_BYTES {
            return Err(LexiconError::PinnedSizeMismatch);
        }
        let lexicon = Self::parse(bytes)?;
        if lexicon.entry_count != PINNED_US_ENTRIES || lexicon.variant_count != PINNED_US_VARIANTS {
            return Err(LexiconError::PinnedCountMismatch);
        }
        if lexicon.provenance
            != (Provenance {
                silver_sha256: MISAKI_US_SILVER_SHA256,
                gold_sha256: MISAKI_US_GOLD_SHA256,
                license_sha256: MISAKI_LICENSE_SHA256,
                source_commit: MISAKI_SOURCE_COMMIT,
            })
        {
            return Err(LexiconError::PinnedProvenanceMismatch);
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if digest != PINNED_US_SHA256 {
            return Err(LexiconError::PinnedDigestMismatch);
        }
        Ok(lexicon)
    }

    pub const fn entry_count(&self) -> usize {
        self.entry_count
    }

    pub const fn variant_count(&self) -> usize {
        self.variant_count
    }

    pub const fn resident_bytes(&self) -> usize {
        self.bytes.len()
    }

    pub const fn provenance(&self) -> Provenance {
        self.provenance
    }

    pub fn get(&self, word: &str) -> Option<&'a str> {
        let mut low = 0usize;
        let mut high = self.entry_count;
        while low < high {
            let middle = low + (high - low) / 2;
            let record = self.entry_record(middle)?;
            let candidate = self.string(record.word_offset, record.word_len)?;
            match candidate.cmp(word) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => {
                    return self.string(record.pronunciation_offset, record.pronunciation_len);
                }
            }
        }
        None
    }

    /// Return one sorted default record for exhaustive offline/runtime audits.
    pub fn entry_at(&self, index: usize) -> Option<(&'a str, &'a str)> {
        let record = self.entry_record(index)?;
        Some((
            self.string(record.word_offset, record.word_len)?,
            self.string(record.pronunciation_offset, record.pronunciation_len)?,
        ))
    }

    /// Look up a preserved non-default Misaki POS/tag pronunciation.
    pub fn get_variant(&self, word: &str, tag: &str) -> Option<&'a str> {
        let entry_index = self.entry_index(word)?;
        let mut low = 0usize;
        let mut high = self.variant_count;
        while low < high {
            let middle = low + (high - low) / 2;
            let record = self.variant_record(middle)?;
            let candidate_tag = self.string(record.tag_offset, record.tag_len)?;
            match record
                .entry_index
                .cmp(&entry_index)
                .then_with(|| candidate_tag.cmp(tag))
            {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => {
                    return self.string(record.pronunciation_offset, record.pronunciation_len);
                }
            }
        }
        None
    }

    /// Return one sorted `(word, tag, pronunciation)` variant record.
    pub fn variant_at(&self, index: usize) -> Option<(&'a str, &'a str, &'a str)> {
        let record = self.variant_record(index)?;
        let entry = self.entry_record(record.entry_index)?;
        Some((
            self.string(entry.word_offset, entry.word_len)?,
            self.string(record.tag_offset, record.tag_len)?,
            self.string(record.pronunciation_offset, record.pronunciation_len)?,
        ))
    }

    fn validate_records(&self) -> Result<(), LexiconError> {
        let mut expected_pool_offset = 0usize;
        let mut previous_word: Option<&str> = None;
        for index in 0..self.entry_count {
            let record = self
                .entry_record(index)
                .ok_or(LexiconError::StringOutOfBounds)?;
            if record.word_len == 0 {
                return Err(LexiconError::EmptyWord);
            }
            if record.pronunciation_len == 0 {
                return Err(LexiconError::EmptyPronunciation);
            }
            if record.word_len > MAX_WORD_BYTES {
                return Err(LexiconError::WordTooLong);
            }
            if record.pronunciation_len > MAX_PRONUNCIATION_BYTES {
                return Err(LexiconError::PronunciationTooLong);
            }
            if record.word_offset != expected_pool_offset
                || record.pronunciation_offset
                    != record
                        .word_offset
                        .checked_add(record.word_len)
                        .ok_or(LexiconError::ArithmeticOverflow)?
            {
                return Err(LexiconError::NonCanonicalStringPool);
            }
            let word = self
                .string_checked(record.word_offset, record.word_len)
                .ok_or(LexiconError::InvalidUtf8)?;
            self.string_checked(record.pronunciation_offset, record.pronunciation_len)
                .ok_or(LexiconError::InvalidUtf8)?;
            if previous_word.is_some_and(|previous| previous >= word) {
                return Err(LexiconError::UnsortedOrDuplicateWord);
            }
            previous_word = Some(word);
            expected_pool_offset = record
                .pronunciation_offset
                .checked_add(record.pronunciation_len)
                .ok_or(LexiconError::ArithmeticOverflow)?;
        }

        let mut previous_variant: Option<(usize, &str)> = None;
        for index in 0..self.variant_count {
            let record = self
                .variant_record(index)
                .ok_or(LexiconError::StringOutOfBounds)?;
            if record.entry_index >= self.entry_count {
                return Err(LexiconError::InvalidVariantEntry);
            }
            if record.tag_len == 0 {
                return Err(LexiconError::EmptyVariantTag);
            }
            if record.pronunciation_len == 0 {
                return Err(LexiconError::EmptyPronunciation);
            }
            if record.tag_len > MAX_TAG_BYTES {
                return Err(LexiconError::VariantTagTooLong);
            }
            if record.pronunciation_len > MAX_PRONUNCIATION_BYTES {
                return Err(LexiconError::PronunciationTooLong);
            }
            if record.tag_offset != expected_pool_offset
                || record.pronunciation_offset
                    != record
                        .tag_offset
                        .checked_add(record.tag_len)
                        .ok_or(LexiconError::ArithmeticOverflow)?
            {
                return Err(LexiconError::NonCanonicalStringPool);
            }
            let tag = self
                .string_checked(record.tag_offset, record.tag_len)
                .ok_or(LexiconError::InvalidUtf8)?;
            self.string_checked(record.pronunciation_offset, record.pronunciation_len)
                .ok_or(LexiconError::InvalidUtf8)?;
            if previous_variant.is_some_and(|(entry, previous_tag)| {
                (entry, previous_tag) >= (record.entry_index, tag)
            }) {
                return Err(LexiconError::UnsortedOrDuplicateVariant);
            }
            previous_variant = Some((record.entry_index, tag));
            expected_pool_offset = record
                .pronunciation_offset
                .checked_add(record.pronunciation_len)
                .ok_or(LexiconError::ArithmeticOverflow)?;
        }
        if expected_pool_offset != self.strings.len() {
            return Err(LexiconError::NonCanonicalStringPool);
        }
        Ok(())
    }

    fn entry_index(&self, word: &str) -> Option<usize> {
        let mut low = 0usize;
        let mut high = self.entry_count;
        while low < high {
            let middle = low + (high - low) / 2;
            let record = self.entry_record(middle)?;
            match self.string(record.word_offset, record.word_len)?.cmp(word) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => return Some(middle),
            }
        }
        None
    }

    fn entry_record(&self, index: usize) -> Option<EntryRecord> {
        let offset = index.checked_mul(ENTRY_RECORD_BYTES)?;
        let end = offset.checked_add(ENTRY_RECORD_BYTES)?;
        let record = self.entries.get(offset..end)?;
        Some(EntryRecord {
            word_offset: read_u32_unchecked(record, 0) as usize,
            word_len: usize::from(read_u16_unchecked(record, 4)),
            pronunciation_len: usize::from(read_u16_unchecked(record, 6)),
            pronunciation_offset: read_u32_unchecked(record, 8) as usize,
        })
    }

    fn variant_record(&self, index: usize) -> Option<VariantRecord> {
        let offset = index.checked_mul(VARIANT_RECORD_BYTES)?;
        let end = offset.checked_add(VARIANT_RECORD_BYTES)?;
        let record = self.variants.get(offset..end)?;
        Some(VariantRecord {
            entry_index: read_u32_unchecked(record, 0) as usize,
            tag_offset: read_u32_unchecked(record, 4) as usize,
            pronunciation_offset: read_u32_unchecked(record, 8) as usize,
            tag_len: usize::from(read_u16_unchecked(record, 12)),
            pronunciation_len: usize::from(read_u16_unchecked(record, 14)),
        })
    }

    fn string(&self, offset: usize, len: usize) -> Option<&'a str> {
        // Every record and string was validated by `parse`; retain checked
        // access here so lookup remains safe even if internals later change.
        self.string_checked(offset, len)
    }

    fn string_checked(&self, offset: usize, len: usize) -> Option<&'a str> {
        let end = offset.checked_add(len)?;
        str::from_utf8(self.strings.get(offset..end)?).ok()
    }
}

impl PronunciationLookup for Lexicon<'_> {
    fn lookup(&self, word: &str) -> Option<&str> {
        self.get(word)
    }
}

fn read_count(bytes: &[u8], offset: usize, maximum: usize) -> Result<usize, LexiconError> {
    let count = read_u32(bytes, offset)? as usize;
    if count > maximum {
        return Err(LexiconError::CountLimit);
    }
    Ok(count)
}

fn read_offset(bytes: &[u8], offset: usize) -> Result<usize, LexiconError> {
    usize::try_from(read_u64(bytes, offset)?).map_err(|_| LexiconError::ArithmeticOverflow)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, LexiconError> {
    let encoded = array_at(bytes, offset)?;
    Ok(u16::from_le_bytes(encoded))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, LexiconError> {
    let encoded = array_at(bytes, offset)?;
    Ok(u32::from_le_bytes(encoded))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, LexiconError> {
    let encoded = array_at(bytes, offset)?;
    Ok(u64::from_le_bytes(encoded))
}

fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], LexiconError> {
    let end = offset
        .checked_add(N)
        .ok_or(LexiconError::ArithmeticOverflow)?;
    bytes
        .get(offset..end)
        .ok_or(LexiconError::TruncatedHeader)?
        .try_into()
        .map_err(|_| LexiconError::TruncatedHeader)
}

fn read_u16_unchecked(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32_unchecked(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
