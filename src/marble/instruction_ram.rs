use alloc::vec::Vec;
use core::ptr;

use crate::pci::mmio;

use super::{Marble, MarbleGadget, MarbleTransform};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionRamWriteMarble {
    pub phys_base: u64,
    pub offset: usize,
    pub payload: Vec<u8>,
    pub verify_after_write: bool,
}

impl InstructionRamWriteMarble {
    pub fn new(phys_base: u64, offset: usize, payload: Vec<u8>) -> Self {
        Self {
            phys_base,
            offset,
            payload,
            verify_after_write: true,
        }
    }

    pub fn mapping_len(&self) -> Result<usize, InstructionRamError> {
        if self.payload.is_empty() {
            return Err(InstructionRamError::EmptyPayload);
        }

        self.offset
            .checked_add(self.payload.len())
            .ok_or(InstructionRamError::AddressOverflow)
    }
}

impl Marble for InstructionRamWriteMarble {
    fn kind(&self) -> &'static str {
        "instruction-ram-write"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstructionRamReceiptMarble {
    pub phys_base: u64,
    pub offset: usize,
    pub len: usize,
    pub verify_after_write: bool,
}

impl Marble for InstructionRamReceiptMarble {
    fn kind(&self) -> &'static str {
        "instruction-ram-receipt"
    }
}

#[derive(Debug)]
pub enum InstructionRamError {
    EmptyPayload,
    AddressOverflow,
    VerifyMismatch {
        index: usize,
        expected: u8,
        observed: u8,
    },
    Map(mmio::MapError),
}

impl From<mmio::MapError> for InstructionRamError {
    fn from(value: mmio::MapError) -> Self {
        Self::Map(value)
    }
}

#[derive(Default)]
pub struct InstructionRamScribe;

impl InstructionRamScribe {
    pub const fn new() -> Self {
        Self
    }
}

impl MarbleGadget for InstructionRamScribe {
    fn name(&self) -> &'static str {
        "instruction-ram-scribe"
    }
}

impl MarbleTransform<InstructionRamWriteMarble, InstructionRamReceiptMarble>
    for InstructionRamScribe
{
    type Error = InstructionRamError;

    fn transform(
        &mut self,
        marble: InstructionRamWriteMarble,
    ) -> Result<InstructionRamReceiptMarble, Self::Error> {
        let map_len = marble.mapping_len()?;
        let mapped = mmio::map_mmio_region_exact(marble.phys_base, map_len)?;
        let base = mapped.as_ptr();

        for (index, expected) in marble.payload.iter().copied().enumerate() {
            unsafe {
                ptr::write_volatile(base.add(marble.offset + index), expected);
            }
        }

        if marble.verify_after_write {
            for (index, expected) in marble.payload.iter().copied().enumerate() {
                let observed = unsafe { ptr::read_volatile(base.add(marble.offset + index)) };
                if observed != expected {
                    return Err(InstructionRamError::VerifyMismatch {
                        index,
                        expected,
                        observed,
                    });
                }
            }
        }

        Ok(InstructionRamReceiptMarble {
            phys_base: marble.phys_base,
            offset: marble.offset,
            len: marble.payload.len(),
            verify_after_write: marble.verify_after_write,
        })
    }
}

pub fn write_once(
    marble: InstructionRamWriteMarble,
) -> Result<InstructionRamReceiptMarble, InstructionRamError> {
    InstructionRamScribe::new().transform(marble)
}
