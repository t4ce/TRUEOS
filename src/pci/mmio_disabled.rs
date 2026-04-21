// ARMTODO: `pci::mmio` is currently an x86_64 paging implementation built on
// `x86_64` page-table types and `Cr3::read()`. Non-x86 bring-up needs a real
// platform page-table/MMIO mapper instead of this compile-time fence.

use core::ptr::NonNull;

#[derive(Debug)]
pub enum MapError {
    NoHhdm,
    NotPhysical,
    InvalidArgs,
    InvalidPointer,
    FrameAllocationFailed,
}

pub fn map_limine_addr_exact(_addr: u64, _size: usize) -> Result<NonNull<u8>, MapError> {
    Err(MapError::FrameAllocationFailed)
}

pub fn map_limine_struct<T>(_addr: u64) -> Result<NonNull<T>, MapError> {
    Err(MapError::FrameAllocationFailed)
}

pub fn map_limine_slice<T>(_addr: u64, _count: usize) -> Result<(NonNull<T>, usize), MapError> {
    Err(MapError::FrameAllocationFailed)
}

pub fn map_mmio_region(_phys_base: u64, _size: usize) -> Result<NonNull<u8>, MapError> {
    Err(MapError::FrameAllocationFailed)
}

pub fn map_mmio_region_exact(_phys_base: u64, _size: usize) -> Result<NonNull<u8>, MapError> {
    Err(MapError::FrameAllocationFailed)
}