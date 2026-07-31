//! Borrowed parser for Helio's symbolic indexed-draw replay plan v1.
//!
//! The plan carries artifact-local resource identifiers and ordinary wgpu
//! indexed-draw arguments. It deliberately contains no GPU virtual addresses
//! or pre-patched hardware command bytes; the TRUEOS renderer remains
//! responsible for allocation, relocation, command encoding, and release.
//!
//! The 64-byte little-endian header is:
//! `magic[8], version:u16, header_len:u16, total_len:u32,
//! command_count:u32, command_stride:u32, flags:u32,
//! source_render_ir_crc32:u32, vertex_buffer_id:u32, index_buffer_id:u32`,
//! followed by 24 reserved zero bytes. Each command is the canonical wgpu
//! 20-byte `DrawIndexedIndirectArgs` field sequence.

use core::{fmt, iter::FusedIterator};

use crate::render_ir::{DrawIndexed, ResourceId};

pub const MAGIC: [u8; 8] = *b"HELIORP\0";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 64;
pub const COMMAND_STRIDE: usize = 20;
pub const SECTION_NAME: &str = "render/replay-v1.bin";

/// A validated, zero-copy view of one Helio replay plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayPlan<'a> {
    bytes: &'a [u8],
    command_count: usize,
}

impl<'a> ReplayPlan<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_LEN || bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
            return Err(Error::BadMagic);
        }

        let version = read_u16(bytes, 8)?;
        if version != VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        if usize::from(read_u16(bytes, 10)?) != HEADER_LEN {
            return Err(Error::MalformedHeader);
        }
        if to_usize(read_u32(bytes, 12)?)? != bytes.len() {
            return Err(Error::LengthMismatch);
        }

        let command_count = to_usize(read_u32(bytes, 16)?)?;
        if command_count == 0 {
            return Err(Error::NoCommands);
        }
        let command_stride = to_usize(read_u32(bytes, 20)?)?;
        if command_stride != COMMAND_STRIDE {
            return Err(Error::UnsupportedCommandStride(command_stride));
        }
        let flags = read_u32(bytes, 24)?;
        if flags != 0 {
            return Err(Error::UnknownFlags(flags));
        }
        if bytes[40..HEADER_LEN].iter().any(|byte| *byte != 0) {
            return Err(Error::NonZeroReserved);
        }

        let vertex_buffer_id = read_u32(bytes, 32)?;
        let index_buffer_id = read_u32(bytes, 36)?;
        if vertex_buffer_id == 0 || index_buffer_id == 0 || vertex_buffer_id == index_buffer_id {
            return Err(Error::InvalidResourceId);
        }

        let payload_len = command_count
            .checked_mul(COMMAND_STRIDE)
            .ok_or(Error::LengthOverflow)?;
        let expected_len = HEADER_LEN
            .checked_add(payload_len)
            .ok_or(Error::LengthOverflow)?;
        if expected_len != bytes.len() {
            return Err(Error::LengthMismatch);
        }

        let plan = Self {
            bytes,
            command_count,
        };
        for (index, command) in plan.commands().enumerate() {
            if command.index_count == 0
                || command.instance_count == 0
                || command
                    .first_index
                    .checked_add(command.index_count)
                    .is_none()
                || command
                    .first_instance
                    .checked_add(command.instance_count)
                    .is_none()
            {
                return Err(Error::InvalidDraw(index));
            }
        }
        Ok(plan)
    }

    pub const fn command_count(&self) -> usize {
        self.command_count
    }

    /// CRC32 of the exact normalized Render IR section this plan was lowered from.
    pub fn source_render_ir_crc32(&self) -> u32 {
        read_u32(self.bytes, 28).expect("validated replay header")
    }

    pub fn vertex_buffer_id(&self) -> ResourceId {
        ResourceId(read_u32(self.bytes, 32).expect("validated replay header"))
    }

    pub fn index_buffer_id(&self) -> ResourceId {
        ResourceId(read_u32(self.bytes, 36).expect("validated replay header"))
    }

    pub fn commands(&self) -> Commands<'a> {
        Commands {
            bytes: &self.bytes[HEADER_LEN..],
            remaining: self.command_count,
            cursor: 0,
        }
    }
}

/// Exact-size iterator over the canonical 20-byte wgpu indexed-draw records.
#[derive(Clone, Debug)]
pub struct Commands<'a> {
    bytes: &'a [u8],
    remaining: usize,
    cursor: usize,
}

impl Iterator for Commands<'_> {
    type Item = DrawIndexed;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let base = self.cursor;
        self.cursor += COMMAND_STRIDE;
        self.remaining -= 1;
        Some(DrawIndexed {
            index_count: read_u32(self.bytes, base).expect("validated replay command"),
            instance_count: read_u32(self.bytes, base + 4).expect("validated replay command"),
            first_index: read_u32(self.bytes, base + 8).expect("validated replay command"),
            base_vertex: read_u32(self.bytes, base + 12).expect("validated replay command") as i32,
            first_instance: read_u32(self.bytes, base + 16).expect("validated replay command"),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for Commands<'_> {}
impl FusedIterator for Commands<'_> {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BadMagic,
    UnsupportedVersion(u16),
    MalformedHeader,
    LengthMismatch,
    LengthOverflow,
    OutOfBounds,
    NoCommands,
    UnsupportedCommandStride(usize),
    UnknownFlags(u32),
    NonZeroReserved,
    InvalidResourceId,
    InvalidDraw(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

fn to_usize(value: u32) -> Result<usize, Error> {
    usize::try_from(value).map_err(|_| Error::LengthOverflow)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{vec, vec::Vec};

    const COMMANDS: [DrawIndexed; 2] = [
        DrawIndexed {
            index_count: 36,
            instance_count: 1,
            first_index: 0,
            base_vertex: 0,
            first_instance: 0,
        },
        DrawIndexed {
            index_count: 12,
            instance_count: 4,
            first_index: 36,
            base_vertex: -3,
            first_instance: 7,
        },
    ];

    fn fixture(commands: &[DrawIndexed]) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_LEN + commands.len() * COMMAND_STRIDE];
        bytes[..8].copy_from_slice(&MAGIC);
        put_u16(&mut bytes, 8, VERSION);
        put_u16(&mut bytes, 10, HEADER_LEN as u16);
        let total_len = bytes.len() as u32;
        put_u32(&mut bytes, 12, total_len);
        put_u32(&mut bytes, 16, commands.len() as u32);
        put_u32(&mut bytes, 20, COMMAND_STRIDE as u32);
        put_u32(&mut bytes, 28, 0x1234_5678);
        put_u32(&mut bytes, 32, 1);
        put_u32(&mut bytes, 36, 2);
        for (index, command) in commands.iter().enumerate() {
            let base = HEADER_LEN + index * COMMAND_STRIDE;
            put_u32(&mut bytes, base, command.index_count);
            put_u32(&mut bytes, base + 4, command.instance_count);
            put_u32(&mut bytes, base + 8, command.first_index);
            put_u32(&mut bytes, base + 12, command.base_vertex as u32);
            put_u32(&mut bytes, base + 16, command.first_instance);
        }
        bytes
    }

    #[test]
    fn parses_exact_wgpu_indexed_draw_layout() {
        let bytes = fixture(&COMMANDS);
        let plan = ReplayPlan::parse(&bytes).unwrap();
        assert_eq!(plan.command_count(), 2);
        assert_eq!(plan.source_render_ir_crc32(), 0x1234_5678);
        assert_eq!(plan.vertex_buffer_id(), ResourceId(1));
        assert_eq!(plan.index_buffer_id(), ResourceId(2));
        assert_eq!(plan.commands().collect::<Vec<_>>(), COMMANDS);
        assert_eq!(plan.commands().len(), 2);

        assert_eq!(
            &bytes[HEADER_LEN..HEADER_LEN + COMMAND_STRIDE],
            &[36, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn every_truncation_is_rejected() {
        let bytes = fixture(&COMMANDS);
        for len in 0..bytes.len() {
            assert!(ReplayPlan::parse(&bytes[..len]).is_err(), "accepted len {len}");
        }
    }

    #[test]
    fn rejects_header_and_layout_changes() {
        let base = fixture(&COMMANDS);
        for (offset, value, expected) in [
            (8, 2, Error::UnsupportedVersion(2)),
            (10, 60, Error::MalformedHeader),
        ] {
            let mut bytes = base.clone();
            put_u16(&mut bytes, offset, value);
            assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), expected);
        }

        let mut bytes = base.clone();
        put_u32(&mut bytes, 12, base.len() as u32 - 1);
        assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::LengthMismatch);

        let mut bytes = base.clone();
        put_u32(&mut bytes, 20, 24);
        assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::UnsupportedCommandStride(24));

        let mut bytes = base.clone();
        put_u32(&mut bytes, 24, 1);
        assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::UnknownFlags(1));

        let mut bytes = base;
        bytes[63] = 1;
        assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::NonZeroReserved);
    }

    #[test]
    fn rejects_impossible_counts_resources_and_draws() {
        let base = fixture(&COMMANDS);

        let mut bytes = base.clone();
        put_u32(&mut bytes, 16, 0);
        assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::NoCommands);

        let mut bytes = base.clone();
        put_u32(&mut bytes, 16, u32::MAX);
        assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::LengthMismatch);

        for (vertex, index) in [(0, 2), (1, 0), (7, 7)] {
            let mut bytes = base.clone();
            put_u32(&mut bytes, 32, vertex);
            put_u32(&mut bytes, 36, index);
            assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::InvalidResourceId);
        }

        for field in [0, 4] {
            let mut bytes = base.clone();
            put_u32(&mut bytes, HEADER_LEN + field, 0);
            assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::InvalidDraw(0));
        }

        for (field, count_field) in [(8, 0), (16, 4)] {
            let mut bytes = base.clone();
            put_u32(&mut bytes, HEADER_LEN + field, u32::MAX);
            put_u32(&mut bytes, HEADER_LEN + count_field, 2);
            assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::InvalidDraw(0));
        }
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = fixture(&COMMANDS);
        bytes[0] ^= 1;
        assert_eq!(ReplayPlan::parse(&bytes).unwrap_err(), Error::BadMagic);
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
