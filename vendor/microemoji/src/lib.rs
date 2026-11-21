//! Borrowed color glyph outlines with streaming SVG and line geometry.
#![no_std]
#![forbid(unsafe_code)]

mod bounds;
pub mod raster_line;
pub mod raster_mask;
pub mod raster_stroke;
pub mod svg;

const HEADER_SIZE: usize = 16;
const ENTRY_SIZE: usize = 16;
const LAYER_HEADER_SIZE: usize = 8;

/// A font-space point with signed 26.6 coordinates and an upward Y axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// A command in a closed, filled contour. Curves list controls before the endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Move(Point),
    Line(Point),
    Quad(Point, Point),
    Cubic(Point, Point, Point),
    Close,
}

/// Shared metrics in whole font units, relative to the baseline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metrics {
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
}

/// The component that failed validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Header,
    Version,
    Metrics,
    Index,
    Payload,
    Path,
}

impl core::fmt::Display for Error {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        out.write_str(match self {
            Self::Header => "invalid microemoji header",
            Self::Version => "unsupported microemoji version",
            Self::Metrics => "invalid microemoji metrics",
            Self::Index => "invalid microemoji index",
            Self::Payload => "invalid microemoji glyph payload",
            Self::Path => "invalid microemoji contour",
        })
    }
}

impl core::error::Error for Error {}

/// A validated version 1 bundle that borrows its underlying bytes.
#[derive(Clone, Copy, Debug)]
pub struct Bundle<'a> {
    bytes: &'a [u8],
    count: usize,
    metrics: Metrics,
}

impl<'a> Bundle<'a> {
    /// Validate the entire bundle before exposing its index or contours.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_SIZE || &bytes[..4] != b"MEOJ" {
            return Err(Error::Header);
        }
        if u16_at(bytes, 4) != 1 {
            return Err(Error::Version);
        }
        let metrics = Metrics {
            units_per_em: u16_at(bytes, 6),
            ascender: i16::from_le_bytes(bytes[8..10].try_into().unwrap()),
            descender: i16::from_le_bytes(bytes[10..12].try_into().unwrap()),
        };
        if metrics.units_per_em == 0 || metrics.ascender <= metrics.descender {
            return Err(Error::Metrics);
        }
        let count = u32_at(bytes, 12) as usize;
        let mut next_offset = count
            .checked_mul(ENTRY_SIZE)
            .and_then(|size| HEADER_SIZE.checked_add(size))
            .filter(|&end| end <= bytes.len())
            .ok_or(Error::Index)?;
        let mut previous = None;
        for index in 0..count {
            let entry = entry_at(bytes, index);
            let scalar = char::from_u32(u32_at(entry, 0)).ok_or(Error::Index)?;
            if previous.is_some_and(|value| scalar <= value) {
                return Err(Error::Index);
            }
            previous = Some(scalar);
            let offset = u32_at(entry, 4) as usize;
            let len = u32_at(entry, 8) as usize;
            let layer_count = u16_at(entry, 14);
            if offset != next_offset || layer_count == 0 {
                return Err(Error::Payload);
            }
            let mut payload = bytes
                .get(offset..)
                .and_then(|b| b.get(..len))
                .ok_or(Error::Payload)?;
            for _ in 0..layer_count {
                let layer = Layer::read(&mut payload).ok_or(Error::Payload)?;
                layer.validate()?;
            }
            if !payload.is_empty() {
                return Err(Error::Payload);
            }
            next_offset = offset + len;
        }
        if next_offset != bytes.len() {
            return Err(Error::Payload);
        }
        Ok(Self {
            bytes,
            count,
            metrics,
        })
    }

    pub fn metrics(self) -> Metrics {
        self.metrics
    }

    /// Find one glyph by Unicode scalar with a binary search of the index.
    pub fn glyph(self, scalar: char) -> Option<Glyph<'a>> {
        let mut low = 0;
        let mut high = self.count;
        while low < high {
            let mid = low + (high - low) / 2;
            let entry = entry_at(self.bytes, mid);
            match u32_at(entry, 0).cmp(&(scalar as u32)) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => return Some(self.read_glyph(entry)),
            }
        }
        None
    }

    /// Visit every glyph in ascending Unicode order.
    pub fn glyphs(self) -> impl ExactSizeIterator<Item = Glyph<'a>> {
        (0..self.count).map(move |index| self.read_glyph(entry_at(self.bytes, index)))
    }

    fn read_glyph(self, entry: &[u8]) -> Glyph<'a> {
        let start = u32_at(entry, 4) as usize;
        let len = u32_at(entry, 8) as usize;
        Glyph {
            scalar: char::from_u32(u32_at(entry, 0)).unwrap(),
            advance: u16_at(entry, 12),
            metrics: self.metrics,
            payload: &self.bytes[start..start + len],
        }
    }
}

/// A glyph's metrics and borrowed color layers.
#[derive(Clone, Copy, Debug)]
pub struct Glyph<'a> {
    scalar: char,
    advance: u16,
    metrics: Metrics,
    payload: &'a [u8],
}

impl<'a> Glyph<'a> {
    pub fn scalar(self) -> char {
        self.scalar
    }

    /// Horizontal advance in whole font units.
    pub fn advance(self) -> u16 {
        self.advance
    }

    pub fn metrics(self) -> Metrics {
        self.metrics
    }

    /// Layers in painting order, starting with the backmost one.
    pub fn layers(self) -> Layers<'a> {
        Layers(self.payload)
    }
}

/// A single paint layer with straight RGBA and nonzero winding.
#[derive(Clone, Copy, Debug)]
pub struct Layer<'a> {
    color: [u8; 4],
    path: &'a [u8],
}

impl<'a> Layer<'a> {
    fn read(bytes: &mut &'a [u8]) -> Option<Self> {
        let (header, rest) = bytes.split_at_checked(LAYER_HEADER_SIZE)?;
        let (path, rest) = rest.split_at_checked(u32_at(header, 4) as usize)?;
        *bytes = rest;
        Some(Self {
            color: header[..4].try_into().unwrap(),
            path,
        })
    }

    fn validate(self) -> Result<(), Error> {
        let mut remaining = self.path;
        if remaining.is_empty() {
            return Err(Error::Path);
        }
        let mut open = false;
        while !remaining.is_empty() {
            match read_command(&mut remaining).ok_or(Error::Path)? {
                Command::Move(_) if !open => open = true,
                Command::Close if open => open = false,
                Command::Line(_) | Command::Quad(..) | Command::Cubic(..) if open => {}
                _ => return Err(Error::Path),
            }
        }
        if open {
            return Err(Error::Path);
        }
        Ok(())
    }

    pub fn color(self) -> [u8; 4] {
        self.color
    }

    pub fn commands(self) -> Commands<'a> {
        Commands(self.path)
    }
}

/// Iterator over the paint layers of a glyph.
#[derive(Clone, Debug)]
pub struct Layers<'a>(&'a [u8]);

impl<'a> Iterator for Layers<'a> {
    type Item = Layer<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        Layer::read(&mut self.0)
    }
}

/// Iterator over a layer's commands in source order.
#[derive(Clone, Debug)]
pub struct Commands<'a>(&'a [u8]);

impl Iterator for Commands<'_> {
    type Item = Command;

    fn next(&mut self) -> Option<Self::Item> {
        read_command(&mut self.0)
    }
}

fn read_command(bytes: &mut &[u8]) -> Option<Command> {
    let (&opcode, rest) = bytes.split_first()?;
    *bytes = rest;
    let mut point = || {
        let (xy, rest) = bytes.split_at_checked(8)?;
        *bytes = rest;
        Some(Point {
            x: i32::from_le_bytes(xy[..4].try_into().unwrap()),
            y: i32::from_le_bytes(xy[4..].try_into().unwrap()),
        })
    };
    match opcode {
        0 => Some(Command::Move(point()?)),
        1 => Some(Command::Line(point()?)),
        2 => Some(Command::Quad(point()?, point()?)),
        3 => Some(Command::Cubic(point()?, point()?, point()?)),
        4 => Some(Command::Close),
        _ => None,
    }
}

fn entry_at(bytes: &[u8], index: usize) -> &[u8] {
    let start = HEADER_SIZE + index * ENTRY_SIZE;
    &bytes[start..start + ENTRY_SIZE]
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
