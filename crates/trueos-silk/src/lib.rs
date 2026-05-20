#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SilkStatus {
    Ok = 0,
    Overflow = 1,
    OutOfBounds = 2,
    Exhausted = 3,
    BadAlign = 4,
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
