#![no_std]
#![forbid(unsafe_code)]

//! Zero-copy validation and loading of build-time Helio artifacts.
//!
//! This crate is deliberately independent of wgpu and of the TRUEOS GPU
//! driver. It validates the relocatable container and presents borrowed
//! sections to the render backend; it never treats captured IDs as addresses.

use core::{fmt, str};

pub mod render_ir;

/// Magic bytes at the start of every HELIOA container.
pub const MAGIC: [u8; 8] = *b"HELIOA\0\0";
pub const FORMAT_VERSION: u16 = 1;

const HEADER_LEN: usize = 32;
const ENTRY_FIXED_LEN: usize = 32;

/// Known HELIOA v1 section kinds. Unknown values remain inspectable so newer
/// optional sections do not make an older loader reject the whole artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SectionKind {
    Manifest,
    WgpuTrace,
    ShaderSource,
    IntelXeLpIsa,
    CompilerMetadata,
    NormalizedRenderIr,
    Unknown(u16),
}

impl SectionKind {
    pub const fn from_raw(raw: u16) -> Self {
        match raw {
            1 => Self::Manifest,
            2 => Self::WgpuTrace,
            3 => Self::ShaderSource,
            4 => Self::IntelXeLpIsa,
            5 => Self::CompilerMetadata,
            // Reserved for the normalized render IR being paved between Helio
            // and TRUEOS. Kept here as one point to align with its producer.
            6 => Self::NormalizedRenderIr,
            other => Self::Unknown(other),
        }
    }

    pub const fn raw(self) -> u16 {
        match self {
            Self::Manifest => 1,
            Self::WgpuTrace => 2,
            Self::ShaderSource => 3,
            Self::IntelXeLpIsa => 4,
            Self::CompilerMetadata => 5,
            Self::NormalizedRenderIr => 6,
            Self::Unknown(raw) => raw,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Section<'a> {
    pub kind: SectionKind,
    pub name: &'a str,
    pub data: &'a [u8],
}

/// A fully validated, borrowed HELIOA file.
///
/// Parsing performs no allocation. CRCs, names, bounds, duplicate names, and
/// overlapping payload ranges are checked before this value can be created.
#[derive(Clone, Copy, Debug)]
pub struct Artifact<'a> {
    bytes: &'a [u8],
    section_count: usize,
    payload_offset: usize,
}

impl<'a> Artifact<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_LEN || bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
            return Err(Error::BadMagic);
        }

        let version = read_u16(bytes, 8)?;
        if version != FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        if usize::from(read_u16(bytes, 10)?) != HEADER_LEN {
            return Err(Error::MalformedHeader);
        }

        let section_count =
            usize::try_from(read_u32(bytes, 12)?).map_err(|_| Error::OutOfBounds)?;
        let toc_len = to_usize(read_u64(bytes, 16)?)?;
        let payload_offset = to_usize(read_u64(bytes, 24)?)?;
        let toc_end = HEADER_LEN.checked_add(toc_len).ok_or(Error::OutOfBounds)?;
        if toc_end != payload_offset || payload_offset > bytes.len() {
            return Err(Error::MalformedHeader);
        }
        if section_count > toc_len / ENTRY_FIXED_LEN {
            return Err(Error::MalformedHeader);
        }

        let artifact = Self {
            bytes,
            section_count,
            payload_offset,
        };

        // Walk once to prove the table layout and every individual entry.
        let mut entries = artifact.raw_entries();
        while let Some(entry) = entries.next() {
            let entry = entry?;
            if crc32fast::hash(entry.data) != entry.crc32 {
                return Err(Error::ChecksumMismatch);
            }
        }
        if entries.cursor != payload_offset {
            return Err(Error::MalformedHeader);
        }

        // Duplicate names and aliases between section payloads are rejected.
        // This is O(n²), intentionally trading tiny build artifacts' parse
        // time for a zero-allocation kernel-side API.
        let mut outer_index = 0usize;
        let mut outer = artifact.raw_entries();
        while let Some(left) = outer.next() {
            let left = left?;
            let mut inner_index = 0usize;
            let mut inner = artifact.raw_entries();
            while let Some(right) = inner.next() {
                let right = right?;
                if inner_index > outer_index {
                    if left.name == right.name {
                        return Err(Error::DuplicateName);
                    }
                    if ranges_overlap(left.offset, left.data.len(), right.offset, right.data.len())
                    {
                        return Err(Error::OverlappingSections);
                    }
                }
                inner_index = inner_index.checked_add(1).ok_or(Error::OutOfBounds)?;
            }
            outer_index = outer_index.checked_add(1).ok_or(Error::OutOfBounds)?;
        }

        if artifact.section("manifest.json").map(|s| s.kind) != Some(SectionKind::Manifest) {
            return Err(Error::MissingManifest);
        }
        Ok(artifact)
    }

    pub const fn section_count(&self) -> usize {
        self.section_count
    }

    pub fn sections(&self) -> Sections<'a> {
        Sections {
            inner: self.raw_entries(),
            failed: false,
        }
    }

    pub fn section(&self, name: &str) -> Option<Section<'a>> {
        self.sections().find(|section| section.name == name)
    }

    pub fn first_section_of_kind(&self, kind: SectionKind) -> Option<Section<'a>> {
        self.sections().find(|section| section.kind == kind)
    }

    pub fn require_kind(&self, kind: SectionKind) -> Result<Section<'a>, Error> {
        self.first_section_of_kind(kind)
            .ok_or(Error::MissingSection)
    }

    pub fn require(&self, required: RequiredSection<'_>) -> Result<Section<'a>, Error> {
        let section = self.section(required.name).ok_or(Error::MissingSection)?;
        if section.kind != required.kind {
            return Err(Error::WrongSectionKind {
                expected: required.kind,
                actual: section.kind,
            });
        }
        Ok(section)
    }

    pub fn require_all(&self, required: &[RequiredSection<'_>]) -> Result<(), Error> {
        for item in required {
            self.require(*item)?;
        }
        Ok(())
    }

    /// Opens the normalized render program section after its kind is checked.
    pub fn render_program(&self) -> Result<render_ir::Program<'a>, Error> {
        self.render_program_named(render_ir::SECTION_NAME)
    }

    pub fn render_program_named(
        &self,
        section_name: &str,
    ) -> Result<render_ir::Program<'a>, Error> {
        let section =
            self.require(RequiredSection::new(section_name, SectionKind::NormalizedRenderIr))?;
        render_ir::Program::parse(section.data).map_err(Error::InvalidRenderIr)
    }

    fn raw_entries(&self) -> RawEntries<'a> {
        RawEntries {
            bytes: self.bytes,
            payload_offset: self.payload_offset,
            remaining: self.section_count,
            cursor: HEADER_LEN,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequiredSection<'a> {
    pub name: &'a str,
    pub kind: SectionKind,
}

impl<'a> RequiredSection<'a> {
    pub const fn new(name: &'a str, kind: SectionKind) -> Self {
        Self { name, kind }
    }
}

pub struct Sections<'a> {
    inner: RawEntries<'a>,
    failed: bool,
}

impl<'a> Iterator for Sections<'a> {
    type Item = Section<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.inner.next()? {
            Ok(entry) => Some(Section {
                kind: entry.kind,
                name: entry.name,
                data: entry.data,
            }),
            Err(_) => {
                // Artifact::parse proved this iterator cannot fail. Refuse to
                // yield data if invariants are somehow violated regardless.
                self.failed = true;
                None
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.inner.remaining, Some(self.inner.remaining))
    }
}

impl ExactSizeIterator for Sections<'_> {}

#[derive(Clone, Copy)]
struct RawEntry<'a> {
    kind: SectionKind,
    name: &'a str,
    offset: usize,
    data: &'a [u8],
    crc32: u32,
}

struct RawEntries<'a> {
    bytes: &'a [u8],
    payload_offset: usize,
    remaining: usize,
    cursor: usize,
}

impl<'a> Iterator for RawEntries<'a> {
    type Item = Result<RawEntry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let result = self.read_one();
        Some(result)
    }
}

impl<'a> RawEntries<'a> {
    fn read_one(&mut self) -> Result<RawEntry<'a>, Error> {
        let fixed_end = self
            .cursor
            .checked_add(ENTRY_FIXED_LEN)
            .ok_or(Error::OutOfBounds)?;
        if fixed_end > self.payload_offset {
            return Err(Error::OutOfBounds);
        }

        let name_len = usize::from(read_u16(self.bytes, self.cursor)?);
        let kind = SectionKind::from_raw(read_u16(self.bytes, self.cursor + 2)?);
        let offset = to_usize(read_u64(self.bytes, self.cursor + 8)?)?;
        let len = to_usize(read_u64(self.bytes, self.cursor + 16)?)?;
        let crc32 = read_u32(self.bytes, self.cursor + 24)?;
        let name_end = fixed_end.checked_add(name_len).ok_or(Error::OutOfBounds)?;
        if name_end > self.payload_offset {
            return Err(Error::OutOfBounds);
        }
        let name =
            str::from_utf8(&self.bytes[fixed_end..name_end]).map_err(|_| Error::InvalidName)?;
        validate_name(name)?;

        let data_end = offset.checked_add(len).ok_or(Error::OutOfBounds)?;
        if offset < self.payload_offset || data_end > self.bytes.len() {
            return Err(Error::OutOfBounds);
        }
        let data = &self.bytes[offset..data_end];
        self.cursor = align_8(name_end).ok_or(Error::OutOfBounds)?;
        if self.cursor > self.payload_offset {
            return Err(Error::OutOfBounds);
        }
        Ok(RawEntry {
            kind,
            name,
            offset,
            data,
            crc32,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BadMagic,
    UnsupportedVersion(u16),
    MalformedHeader,
    OutOfBounds,
    InvalidName,
    DuplicateName,
    OverlappingSections,
    ChecksumMismatch,
    MissingManifest,
    MissingSection,
    WrongSectionKind {
        expected: SectionKind,
        actual: SectionKind,
    },
    InvalidRenderIr(render_ir::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

fn validate_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > usize::from(u16::MAX)
        || name.starts_with('/')
        || name.contains("..")
        || name.contains('\\')
    {
        return Err(Error::InvalidName);
    }
    Ok(())
}

fn ranges_overlap(a_start: usize, a_len: usize, b_start: usize, b_len: usize) -> bool {
    if a_len == 0 || b_len == 0 {
        return false;
    }
    let a_end = a_start + a_len;
    let b_end = b_start + b_len;
    a_start < b_end && b_start < a_end
}

fn align_8(value: usize) -> Option<usize> {
    value.checked_add(7).map(|v| v & !7)
}

fn to_usize(value: u64) -> Result<usize, Error> {
    usize::try_from(value).map_err(|_| Error::OutOfBounds)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let raw = bytes
        .get(offset..offset.checked_add(2).ok_or(Error::OutOfBounds)?)
        .ok_or(Error::OutOfBounds)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let raw = bytes
        .get(offset..offset.checked_add(4).ok_or(Error::OutOfBounds)?)
        .ok_or(Error::OutOfBounds)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    let raw = bytes
        .get(offset..offset.checked_add(8).ok_or(Error::OutOfBounds)?)
        .ok_or(Error::OutOfBounds)?;
    Ok(u64::from_le_bytes([
        raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
    ]))
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
