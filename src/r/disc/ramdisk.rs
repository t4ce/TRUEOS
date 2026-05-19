use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};

use crate::disc::block;

const RAMDISK_CHUNK_BYTES: usize = 64 * 1024;

pub struct RamdiskDevice {
    len_bytes: usize,
    chunks: BTreeMap<u64, Box<[u8]>>,
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

        Ok(Self {
            len_bytes,
            chunks: BTreeMap::new(),
            block_size,
            block_count,
        })
    }

    fn read_range(&self, start: usize, dst: &mut [u8]) -> Result<(), block::Error> {
        let stop = start
            .checked_add(dst.len())
            .ok_or(block::Error::InvalidParam)?;
        if stop > self.len_bytes {
            return Err(block::Error::OutOfBounds);
        }
        dst.fill(0);

        let mut src_off = start;
        let mut dst_off = 0usize;
        while dst_off < dst.len() {
            let chunk_idx = (src_off / RAMDISK_CHUNK_BYTES) as u64;
            let chunk_off = src_off % RAMDISK_CHUNK_BYTES;
            let take = core::cmp::min(RAMDISK_CHUNK_BYTES - chunk_off, dst.len() - dst_off);
            if let Some(chunk) = self.chunks.get(&chunk_idx) {
                dst[dst_off..dst_off + take].copy_from_slice(&chunk[chunk_off..chunk_off + take]);
            }
            src_off = src_off.saturating_add(take);
            dst_off = dst_off.saturating_add(take);
        }
        Ok(())
    }

    fn write_range(&mut self, start: usize, src: &[u8]) -> Result<(), block::Error> {
        let stop = start
            .checked_add(src.len())
            .ok_or(block::Error::InvalidParam)?;
        if stop > self.len_bytes {
            return Err(block::Error::OutOfBounds);
        }

        let mut src_off = 0usize;
        let mut dst_off = start;
        while src_off < src.len() {
            let chunk_idx = (dst_off / RAMDISK_CHUNK_BYTES) as u64;
            let chunk_off = dst_off % RAMDISK_CHUNK_BYTES;
            let take = core::cmp::min(RAMDISK_CHUNK_BYTES - chunk_off, src.len() - src_off);
            let chunk = self
                .chunks
                .entry(chunk_idx)
                .or_insert_with(|| vec![0u8; RAMDISK_CHUNK_BYTES].into_boxed_slice());
            chunk[chunk_off..chunk_off + take].copy_from_slice(&src[src_off..src_off + take]);
            src_off = src_off.saturating_add(take);
            dst_off = dst_off.saturating_add(take);
        }
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

    fn read_blocks_into<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
        dst: &'a mut [u8],
    ) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            if blocks == 0 {
                return if dst.is_empty() {
                    Ok(())
                } else {
                    Err(block::Error::InvalidParam)
                };
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
            if dst.len() != len {
                return Err(block::Error::InvalidParam);
            }
            self.read_range(start, dst)?;
            Ok(())
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
