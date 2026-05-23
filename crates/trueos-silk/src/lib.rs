#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SilkStatus {
    Ok = 0,
    Overflow = 1,
    OutOfBounds = 2,
    Exhausted = 3,
    BadAlign = 4,
    Full = 5,
    Empty = 6,
    Corrupt = 7,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Span {
    pub addr: u64,
    pub len: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SpanResult {
    pub status: SilkStatus,
    pub span: Span,
}

impl Span {
    pub const EMPTY: Self = Self { addr: 0, len: 0 };

    pub const fn new(addr: u64, len: u64) -> Self {
        Self { addr, len }
    }

    pub const fn checked(addr: u64, len: u64, bound: u64) -> SpanResult {
        let Some(end) = addr.checked_add(len) else {
            return SpanResult::err(SilkStatus::Overflow);
        };

        if end > bound {
            return SpanResult::err(SilkStatus::OutOfBounds);
        }

        SpanResult {
            status: SilkStatus::Ok,
            span: Self { addr, len },
        }
    }

    pub const fn end(self) -> SpanEnd {
        let Some(end) = self.addr.checked_add(self.len) else {
            return SpanEnd {
                status: SilkStatus::Overflow,
                value: 0,
            };
        };

        SpanEnd {
            status: SilkStatus::Ok,
            value: end,
        }
    }
}

impl SpanResult {
    pub const fn err(status: SilkStatus) -> Self {
        Self {
            status,
            span: Span::EMPTY,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SpanEnd {
    pub status: SilkStatus,
    pub value: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Arena {
    pub base: u64,
    pub len: u64,
    pub cursor: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ArenaResult {
    pub status: SilkStatus,
    pub span: Span,
}

impl Arena {
    pub const fn new(base: u64, len: u64) -> Self {
        Self {
            base,
            len,
            cursor: 0,
        }
    }

    pub const fn from_span(span: Span) -> Self {
        Self::new(span.addr, span.len)
    }

    pub const fn remaining(&self) -> u64 {
        self.len.saturating_sub(self.cursor)
    }

    pub const fn reset(&mut self) {
        self.cursor = 0;
    }

    pub const fn alloc(&mut self, len: u64) -> ArenaResult {
        self.alloc_aligned(len, 1)
    }

    pub const fn alloc_aligned(&mut self, len: u64, align: u64) -> ArenaResult {
        if align == 0 || !align.is_power_of_two() {
            return ArenaResult::err(SilkStatus::BadAlign);
        }

        let Some(absolute_cursor) = self.base.checked_add(self.cursor) else {
            return ArenaResult::err(SilkStatus::Overflow);
        };

        let Some(aligned_absolute) = align_up(absolute_cursor, align) else {
            return ArenaResult::err(SilkStatus::Overflow);
        };

        let aligned_cursor = aligned_absolute - self.base;
        let Some(end_cursor) = aligned_cursor.checked_add(len) else {
            return ArenaResult::err(SilkStatus::Overflow);
        };

        if end_cursor > self.len {
            return ArenaResult::err(SilkStatus::Exhausted);
        }

        self.cursor = end_cursor;
        ArenaResult {
            status: SilkStatus::Ok,
            span: Span {
                addr: aligned_absolute,
                len,
            },
        }
    }
}

impl ArenaResult {
    pub const fn err(status: SilkStatus) -> Self {
        Self {
            status,
            span: Span::EMPTY,
        }
    }
}

const fn align_up(value: u64, align: u64) -> Option<u64> {
    let mask = align - 1;
    let Some(biased) = value.checked_add(mask) else {
        return None;
    };

    Some(biased & !mask)
}

pub const RING_HEADER_LEN: u64 = 32;
pub const RING_ALIGN: u64 = 16;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RingArtifact {
    pub name: &'static str,
    pub layout: RingLayout,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RingLayout {
    pub header_len: u64,
    pub data_offset: u64,
    pub capacity: u64,
    pub total_len: u64,
    pub align: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RingArtifactResult {
    pub status: SilkStatus,
    pub artifact: RingArtifact,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RingState {
    pub read: u64,
    pub write: u64,
    pub len: u64,
    pub capacity: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RingSnapshot {
    pub read: u64,
    pub write: u64,
    pub len: u64,
    pub capacity: u64,
}

pub struct RingBinding<'a> {
    pub artifact: RingArtifact,
    pub span: Span,
    pub state: RingState,
    data: &'a mut [u8],
}

impl RingArtifact {
    pub const EMPTY: Self = Self {
        name: "",
        layout: RingLayout::EMPTY,
    };

    pub const fn u8(name: &'static str, capacity: u64) -> RingArtifactResult {
        if capacity == 0 {
            return RingArtifactResult::err(SilkStatus::Exhausted);
        }

        let Some(total_len) = RING_HEADER_LEN.checked_add(capacity) else {
            return RingArtifactResult::err(SilkStatus::Overflow);
        };

        RingArtifactResult {
            status: SilkStatus::Ok,
            artifact: Self {
                name,
                layout: RingLayout {
                    header_len: RING_HEADER_LEN,
                    data_offset: RING_HEADER_LEN,
                    capacity,
                    total_len,
                    align: RING_ALIGN,
                },
            },
        }
    }
}

impl RingLayout {
    pub const EMPTY: Self = Self {
        header_len: 0,
        data_offset: 0,
        capacity: 0,
        total_len: 0,
        align: 0,
    };
}

impl RingArtifactResult {
    pub const fn err(status: SilkStatus) -> Self {
        Self {
            status,
            artifact: RingArtifact::EMPTY,
        }
    }
}

impl<'a> RingBinding<'a> {
    pub fn bind(
        artifact: RingArtifact,
        span: Span,
        data: &'a mut [u8],
    ) -> Result<Self, SilkStatus> {
        if artifact.layout.align == 0 || !artifact.layout.align.is_power_of_two() {
            return Err(SilkStatus::BadAlign);
        }
        if span.addr & (artifact.layout.align - 1) != 0 {
            return Err(SilkStatus::BadAlign);
        }
        if span.len < artifact.layout.total_len {
            return Err(SilkStatus::OutOfBounds);
        }
        if artifact.layout.capacity > usize::MAX as u64 {
            return Err(SilkStatus::OutOfBounds);
        }
        if data.len() < artifact.layout.capacity as usize {
            return Err(SilkStatus::OutOfBounds);
        }

        Ok(Self {
            artifact,
            span,
            state: RingState {
                read: 0,
                write: 0,
                len: 0,
                capacity: artifact.layout.capacity,
            },
            data,
        })
    }

    pub fn push(&mut self, byte: u8) -> Result<(), SilkStatus> {
        self.validate()?;
        if self.state.len == self.state.capacity {
            return Err(SilkStatus::Full);
        }

        self.data[self.state.write as usize] = byte;
        self.state.write = (self.state.write + 1) % self.state.capacity;
        self.state.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Result<u8, SilkStatus> {
        self.validate()?;
        if self.state.len == 0 {
            return Err(SilkStatus::Empty);
        }

        let byte = self.data[self.state.read as usize];
        self.state.read = (self.state.read + 1) % self.state.capacity;
        self.state.len -= 1;
        Ok(byte)
    }

    pub fn validate(&self) -> Result<RingSnapshot, SilkStatus> {
        if self.state.capacity != self.artifact.layout.capacity {
            return Err(SilkStatus::Corrupt);
        }
        if self.state.capacity == 0 || self.state.len > self.state.capacity {
            return Err(SilkStatus::Corrupt);
        }
        if self.state.capacity > usize::MAX as u64 {
            return Err(SilkStatus::OutOfBounds);
        }
        if self.state.read >= self.state.capacity || self.state.write >= self.state.capacity {
            return Err(SilkStatus::Corrupt);
        }
        if self.data.len() < self.state.capacity as usize {
            return Err(SilkStatus::OutOfBounds);
        }

        Ok(RingSnapshot {
            read: self.state.read,
            write: self.state.write,
            len: self.state.len,
            capacity: self.state.capacity,
        })
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MachineOpKind {
    AddU64 = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MachineOpArtifact {
    pub name: &'static str,
    pub kind: MachineOpKind,
    pub input_count: u8,
    pub output_count: u8,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MachineOpResult {
    pub status: SilkStatus,
    pub value: u64,
}

impl MachineOpArtifact {
    pub const fn add_u64(name: &'static str) -> Self {
        Self {
            name,
            kind: MachineOpKind::AddU64,
            input_count: 2,
            output_count: 1,
        }
    }

    pub fn run_add_u64(self, lhs: u64, rhs: u64) -> MachineOpResult {
        if self.kind != MachineOpKind::AddU64 || self.input_count != 2 || self.output_count != 1 {
            return MachineOpResult {
                status: SilkStatus::Corrupt,
                value: 0,
            };
        }

        MachineOpResult {
            status: SilkStatus::Ok,
            value: machine_add_u64(lhs, rhs),
        }
    }
}

#[inline(always)]
fn machine_add_u64(lhs: u64, rhs: u64) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let mut value = lhs;
        unsafe {
            core::arch::asm!(
                "add {0}, {1}",
                inout(reg) value,
                in(reg) rhs,
                options(nomem, nostack)
            );
        }
        value
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        lhs.wrapping_add(rhs)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub arena: ArenaBinding,
    pub path: ConstBinding,
    pub read: FsReadStep,
    pub log: LogWriteStep,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArenaBinding {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstBinding {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FsReadStep {
    pub name: String,
    pub path: String,
    pub arena: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogWriteStep {
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseError {
    MissingLine(&'static str),
    UnexpectedLine(usize),
    BadArena,
    BadSize,
    BadConst,
    BadFsRead,
    BadLogWrite,
    NameMismatch(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacementProgram {
    pub steps: Vec<PlacementStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacementStep {
    pub artifact: String,
    pub arena: String,
    pub align: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlacementError {
    UnexpectedLine(usize),
    BadPlace,
    BadAlign,
}

pub fn parse_plan(source: &str) -> Result<Plan, ParseError> {
    let mut arena = None;
    let mut path = None;
    let mut read = None;
    let mut log = None;

    for (idx, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("arena ") {
            if arena.is_some() {
                return Err(ParseError::UnexpectedLine(idx + 1));
            }
            arena = Some(parse_arena(line)?);
        } else if line.contains(" = const ") {
            if path.is_some() {
                return Err(ParseError::UnexpectedLine(idx + 1));
            }
            path = Some(parse_const(line)?);
        } else if line.contains(" = fs.read ") {
            if read.is_some() {
                return Err(ParseError::UnexpectedLine(idx + 1));
            }
            read = Some(parse_fs_read(line)?);
        } else if line.starts_with("log.write ") {
            if log.is_some() {
                return Err(ParseError::UnexpectedLine(idx + 1));
            }
            log = Some(parse_log_write(line)?);
        } else {
            return Err(ParseError::UnexpectedLine(idx + 1));
        }
    }

    let arena = arena.ok_or(ParseError::MissingLine("arena"))?;
    let path = path.ok_or(ParseError::MissingLine("const"))?;
    let read = read.ok_or(ParseError::MissingLine("fs.read"))?;
    let log = log.ok_or(ParseError::MissingLine("log.write"))?;

    if read.path != path.name {
        return Err(ParseError::NameMismatch("fs.read path"));
    }
    if read.arena != arena.name {
        return Err(ParseError::NameMismatch("fs.read arena"));
    }
    if log.source != read.name {
        return Err(ParseError::NameMismatch("log.write source"));
    }

    Ok(Plan {
        arena,
        path,
        read,
        log,
    })
}

pub fn parse_placement_program(source: &str) -> Result<PlacementProgram, PlacementError> {
    let mut steps = Vec::new();

    for (idx, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if !line.starts_with("place ") {
            return Err(PlacementError::UnexpectedLine(idx + 1));
        }
        steps.push(parse_place(line)?);
    }

    Ok(PlacementProgram { steps })
}

fn parse_place(line: &str) -> Result<PlacementStep, PlacementError> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("place") {
        return Err(PlacementError::BadPlace);
    }
    let Some(artifact) = parts.next() else {
        return Err(PlacementError::BadPlace);
    };
    if parts.next() != Some("in") {
        return Err(PlacementError::BadPlace);
    }
    let Some(arena) = parts.next() else {
        return Err(PlacementError::BadPlace);
    };
    if parts.next() != Some("align") {
        return Err(PlacementError::BadPlace);
    }
    let Some(align) = parts.next() else {
        return Err(PlacementError::BadPlace);
    };
    if parts.next().is_some() {
        return Err(PlacementError::BadPlace);
    }

    let align = align.parse::<u64>().map_err(|_| PlacementError::BadAlign)?;
    if align == 0 || !align.is_power_of_two() {
        return Err(PlacementError::BadAlign);
    }

    Ok(PlacementStep {
        artifact: artifact.to_string(),
        arena: arena.to_string(),
        align,
    })
}

fn parse_arena(line: &str) -> Result<ArenaBinding, ParseError> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("arena") {
        return Err(ParseError::BadArena);
    }
    let Some(name) = parts.next() else {
        return Err(ParseError::BadArena);
    };
    let Some(size) = parts.next() else {
        return Err(ParseError::BadArena);
    };
    if parts.next().is_some() {
        return Err(ParseError::BadArena);
    }
    Ok(ArenaBinding {
        name: name.to_string(),
        size: parse_size(size)?,
    })
}

fn parse_const(line: &str) -> Result<ConstBinding, ParseError> {
    let Some((name, rest)) = line.split_once(" = const ") else {
        return Err(ParseError::BadConst);
    };
    let value = parse_quoted(rest.trim())?;
    Ok(ConstBinding {
        name: name.trim().to_string(),
        value,
    })
}

fn parse_fs_read(line: &str) -> Result<FsReadStep, ParseError> {
    let Some((name, rest)) = line.split_once(" = fs.read ") else {
        return Err(ParseError::BadFsRead);
    };
    let mut parts = rest.split_whitespace();
    let Some(path) = parts.next() else {
        return Err(ParseError::BadFsRead);
    };
    if parts.next() != Some("using") {
        return Err(ParseError::BadFsRead);
    }
    let Some(arena) = parts.next() else {
        return Err(ParseError::BadFsRead);
    };
    if parts.next().is_some() {
        return Err(ParseError::BadFsRead);
    }
    Ok(FsReadStep {
        name: name.trim().to_string(),
        path: path.to_string(),
        arena: arena.to_string(),
    })
}

fn parse_log_write(line: &str) -> Result<LogWriteStep, ParseError> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("log.write") {
        return Err(ParseError::BadLogWrite);
    }
    let Some(source) = parts.next() else {
        return Err(ParseError::BadLogWrite);
    };
    if parts.next().is_some() {
        return Err(ParseError::BadLogWrite);
    }
    Ok(LogWriteStep {
        source: source.to_string(),
    })
}

fn parse_size(text: &str) -> Result<u64, ParseError> {
    let (digits, mul) = if let Some(digits) = text.strip_suffix(['k', 'K']) {
        (digits, 1024)
    } else if let Some(digits) = text.strip_suffix(['m', 'M']) {
        (digits, 1024 * 1024)
    } else {
        (text, 1)
    };
    let value = digits.parse::<u64>().map_err(|_| ParseError::BadSize)?;
    value.checked_mul(mul).ok_or(ParseError::BadSize)
}

fn parse_quoted(text: &str) -> Result<String, ParseError> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return Err(ParseError::BadConst);
    }
    Ok(text[1..text.len() - 1].to_string())
}
