//! Ring artifacts: fixed queues over explicit memory.
//!
//! Rings show up everywhere below a classical runtime: device queues, log
//! streams, scheduler mailboxes, and producer/consumer buffers. The machine
//! version is simple arithmetic over read/write cursors, but that simplicity is
//! exactly why it deserves an artifact: the layout and invariants can be named,
//! placed, bound, mutated, and validated without asking upper layers to juggle
//! raw offsets every time.

use crate::{SilkStatus, Span};

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
