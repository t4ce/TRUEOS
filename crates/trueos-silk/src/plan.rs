//! Plan and placement artifacts.
//!
//! Before a compiler adapter exists, Silk still needs a small textual way to
//! describe machine-near intent: allocate an arena, name a path, read bytes into
//! a buffer, write a log, place artifacts with alignment. This file is not meant
//! to be the final language. It is the bootstrapping layer that lets TRUEOS
//! exercise artifact facts before a classical toolchain learns to emit them.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

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
