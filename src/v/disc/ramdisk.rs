use alloc::{boxed::Box, string::String, vec::Vec};
use core::ptr;

use crate::disc::block;
use crate::v::disc::pmm::BigMem;

pub struct RamdiskDevice {
    backing: BigMem,
    block_size: u32,
    block_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrueosPrivateError {
    Create(block::Error),
    Format(block::Error),
    Validate(block::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrueosPublicError {
    Create(block::Error),
    Format(block::Error),
    Validate(block::Error),
}

impl RamdiskDevice {
    fn new(size_bytes: u64, block_size: u32) -> Result<Self, block::Error> {
        if block_size == 0 {
            return Err(block::Error::InvalidParam);
        }
        let bs = block_size as u64;
        if size_bytes < bs {
            return Err(block::Error::InvalidParam);
        }
        let block_count = size_bytes / bs;
        if block_count == 0 {
            return Err(block::Error::InvalidParam);
        }
        let bytes = block_count
            .checked_mul(bs)
            .ok_or(block::Error::InvalidParam)?;
        let len_bytes = usize::try_from(bytes).map_err(|_| block::Error::InvalidParam)?;
        let backing = BigMem::new_zeroed(len_bytes)?;
        Ok(Self {
            backing,
            block_size,
            block_count,
        })
    }

    fn read_range(&self, start: usize, dst: &mut [u8]) -> Result<(), block::Error> {
        let stop = start
            .checked_add(dst.len())
            .ok_or(block::Error::InvalidParam)?;
        if stop > self.backing.len() {
            return Err(block::Error::OutOfBounds);
        }
        unsafe {
            ptr::copy_nonoverlapping(
                self.backing.as_ptr().add(start),
                dst.as_mut_ptr(),
                dst.len(),
            )
        };
        Ok(())
    }

    fn write_range(&mut self, start: usize, src: &[u8]) -> Result<(), block::Error> {
        let stop = start
            .checked_add(src.len())
            .ok_or(block::Error::InvalidParam)?;
        if stop > self.backing.len() {
            return Err(block::Error::OutOfBounds);
        }
        unsafe {
            ptr::copy_nonoverlapping(
                src.as_ptr(),
                self.backing.as_mut_ptr().add(start),
                src.len(),
            )
        };
        Ok(())
    }
}

impl block::BlockDevice for RamdiskDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
    ) -> block::BoxFuture<'a, block::Result<Vec<u8>>> {
        Box::pin(async move {
            if blocks == 0 {
                return Ok(Vec::new());
            }
            let bs = self.block_size as usize;
            let blocks_u64 = blocks as u64;
            let end = lba
                .checked_add(blocks_u64)
                .ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            let start = usize::try_from(lba)
                .ok()
                .and_then(|v| v.checked_mul(bs))
                .ok_or(block::Error::InvalidParam)?;
            let len = blocks.checked_mul(bs).ok_or(block::Error::InvalidParam)?;
            let mut out = vec![0u8; len];
            self.read_range(start, &mut out)?;
            Ok(out)
        })
    }

    fn write_blocks<'a>(
        &'a mut self,
        lba: u64,
        buf: &'a [u8],
    ) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            let bs = self.block_size as usize;
            if bs == 0 || !buf.len().is_multiple_of(bs) {
                return Err(block::Error::InvalidParam);
            }
            let blocks = (buf.len() / bs) as u64;
            if blocks == 0 {
                return Ok(());
            }

            let end = lba.checked_add(blocks).ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            let start = usize::try_from(lba)
                .ok()
                .and_then(|v| v.checked_mul(bs))
                .ok_or(block::Error::InvalidParam)?;
            self.write_range(start, buf)
        })
    }

    fn dma_alignment_bytes(&self) -> u32 {
        1
    }

    fn max_transfer_bytes(&self) -> u64 {
        1024 * 1024
    }

    fn supports_write(&self) -> bool {
        true
    }
}

fn create_labeled(
    size_bytes: u64,
    block_size: u32,
    label: impl Into<String>,
) -> Result<block::DeviceHandle, block::Error> {
    let dev = RamdiskDevice::new(size_bytes, block_size)?;
    let desc = block::DeviceDescriptor::new(block::DeviceKind::Ramdisk).with_label(label);
    Ok(block::register_device(desc, dev))
}

pub async fn create_trueos_private(
    size_bytes: u64,
    block_size: u32,
    label: impl Into<String>,
) -> Result<block::DeviceHandle, TrueosPrivateError> {
    let disk = create_labeled(size_bytes, block_size, label).map_err(TrueosPrivateError::Create)?;
    crate::v::fs::trueosfs::format_blank_force_async(disk)
        .await
        .map_err(TrueosPrivateError::Format)?;
    crate::v::fs::trueosfs::validate_private_medium_async(disk, 0)
        .await
        .map_err(TrueosPrivateError::Validate)?;
    Ok(disk)
}

pub async fn create_trueos_public(
    size_bytes: u64,
    block_size: u32,
    label: impl Into<String>,
) -> Result<block::DeviceHandle, TrueosPublicError> {
    let disk = create_labeled(size_bytes, block_size, label).map_err(TrueosPublicError::Create)?;
    crate::v::fs::trueosfs::format_blank_force_async(disk)
        .await
        .map_err(TrueosPublicError::Format)?;
    crate::v::fs::trueosfs::validate_public_medium_async(disk, 0)
        .await
        .map_err(TrueosPublicError::Validate)?;
    Ok(disk)
}
