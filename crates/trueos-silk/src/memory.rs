//! Memory artifacts: spans, arenas, and fixed buffers.
//!
//! The first hardware fact an OS must respect is that addresses are ranges, not
//! wishes. x86_64 loads, stores, DMA windows, descriptor tables, and cache-line
//! sensitive structures all care about size, alignment, and overflow. These
//! artifacts exist so later layers can ask for checked memory shapes instead of
//! redoing pointer arithmetic around every raw machine operation.

use crate::SilkStatus;

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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BufferArtifact {
    pub name: &'static str,
    pub len: u64,
    pub align: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BufferBinding {
    pub artifact: BufferArtifact,
    pub span: Span,
}

impl BufferArtifact {
    pub const fn bytes(name: &'static str, len: u64, align: u64) -> Self {
        Self { name, len, align }
    }

    pub fn bind(self, span: Span, data_len: usize) -> Result<BufferBinding, SilkStatus> {
        if self.align == 0 || !self.align.is_power_of_two() {
            return Err(SilkStatus::BadAlign);
        }
        if span.addr & (self.align - 1) != 0 {
            return Err(SilkStatus::BadAlign);
        }
        if self.len > usize::MAX as u64 {
            return Err(SilkStatus::OutOfBounds);
        }
        if span.len < self.len || data_len < self.len as usize {
            return Err(SilkStatus::OutOfBounds);
        }

        Ok(BufferBinding {
            artifact: self,
            span,
        })
    }
}
