use alloc::{boxed::Box, string::String, vec::Vec};

use crate::disc::block;

pub struct RamdiskDevice {
    backing: Vec<u8>,
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

        let mut backing = Vec::new();
        backing
            .try_reserve_exact(len_bytes)
            .map_err(|_| block::Error::NotReady)?;
        backing.resize(len_bytes, 0);

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
        dst.copy_from_slice(&self.backing[start..stop]);
        Ok(())
    }

    fn write_range(&mut self, start: usize, src: &[u8]) -> Result<(), block::Error> {
        let stop = start
            .checked_add(src.len())
            .ok_or(block::Error::InvalidParam)?;
        if stop > self.backing.len() {
            return Err(block::Error::OutOfBounds);
        }
        self.backing[start..stop].copy_from_slice(src);
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
    user_visible: bool,
    label: impl Into<String>,
) -> Result<block::DeviceHandle, block::Error> {
    let dev = RamdiskDevice::new(size_bytes, block_size)?;
    let mut desc = block::DeviceDescriptor::new(block::DeviceKind::Ramdisk).with_label(label);
    if !user_visible {
        desc = desc.mark_internal_hidden();
    }
    Ok(block::register_device(desc, dev))
}

pub async fn create_trueos_private(
    size_bytes: u64,
    block_size: u32,
    label: impl Into<String>,
) -> Result<block::DeviceHandle, TrueosPrivateError> {
    let disk =
        create_labeled(size_bytes, block_size, false, label).map_err(TrueosPrivateError::Create)?;
    crate::r::fs::trueosfs::format_blank_force_async(disk)
        .await
        .map_err(TrueosPrivateError::Format)?;
    crate::r::fs::trueosfs::validate_private_medium_async(disk, 0)
        .await
        .map_err(TrueosPrivateError::Validate)?;
    Ok(disk)
}

pub async fn create_trueos_public(
    size_bytes: u64,
    block_size: u32,
    label: impl Into<String>,
) -> Result<block::DeviceHandle, TrueosPublicError> {
    let disk =
        create_labeled(size_bytes, block_size, true, label).map_err(TrueosPublicError::Create)?;
    crate::r::fs::trueosfs::format_blank_force_async(disk)
        .await
        .map_err(TrueosPublicError::Format)?;
    crate::r::fs::trueosfs::validate_public_medium_async(disk, 0)
        .await
        .map_err(TrueosPublicError::Validate)?;
    Ok(disk)
}
