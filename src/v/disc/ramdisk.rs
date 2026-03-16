#![allow(dead_code)]
// Is okay but currently not wired, was wired and tested as virtual blockdevice
// and proven trueosfs host mount.

use alloc::{boxed::Box, string::String, vec, vec::Vec};

use crate::disc::block;

pub struct RamdiskDevice {
    data: Vec<u8>,
    block_size: u32,
    block_count: u64,
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
        let alloc_len = usize::try_from(bytes).map_err(|_| block::Error::InvalidParam)?;
        let data = vec![0u8; alloc_len];
        Ok(Self {
            data,
            block_size,
            block_count,
        })
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
            let stop = start.checked_add(len).ok_or(block::Error::InvalidParam)?;
            if stop > self.data.len() {
                return Err(block::Error::OutOfBounds);
            }

            Ok(self.data[start..stop].to_vec())
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
            let stop = start
                .checked_add(buf.len())
                .ok_or(block::Error::InvalidParam)?;
            if stop > self.data.len() {
                return Err(block::Error::OutOfBounds);
            }

            self.data[start..stop].copy_from_slice(buf);
            Ok(())
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

pub fn create(size_bytes: u64, block_size: u32) -> Result<block::DeviceHandle, block::Error> {
    let dev = RamdiskDevice::new(size_bytes, block_size)?;
    let desc = block::DeviceDescriptor::new(block::DeviceKind::Ramdisk)
        .with_label(String::from("ramdisk"));
    Ok(block::register_device(desc, dev))
}
