use alloc::vec::Vec;

use crate::disc::{block, partition};

// Standard EFI System Partition type GUID.
// C12A7328-F81F-11D2-BA4B-00A0C93EC93B
const GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrueosFsPlacement {
    pub bootable: bool,
    pub super_lba: u64,
    pub data_lba: u64,
}

struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
    layout: alloc::alloc::Layout,
}

impl AlignedBuf {
    fn new(len: usize, align: usize) -> Option<Self> {
        let layout = alloc::alloc::Layout::from_size_align(len, align).ok()?;
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            return None;
        }
        Some(Self { ptr, len, layout })
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { alloc::alloc::dealloc(self.ptr, self.layout) };
        }
    }
}

fn read_blocks_aligned(handle: block::DeviceHandle, lba: u64, blocks: usize) -> Result<Vec<u8>, block::Error> {
    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    let bytes = bs.saturating_mul(blocks);
    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(bytes, align).ok_or(block::Error::DmaUnavailable)?;
    tmp.as_mut_slice().fill(0);
    handle.read_blocks(lba, tmp.as_mut_slice())?;
    Ok(tmp.as_mut_slice().to_vec())
}

fn looks_like_trueos_superblock(block0: &[u8]) -> bool {
    block0.len() >= 8 && &block0[0..8] == &trueos_fs::MAGIC
}

/// Find where TRUEOSFS lives on a whole disk.
///
/// - Bootable disks: TRUEOSFS is expected to live inside a GPT partition (ESP exists elsewhere).
/// - Data-only disks: TRUEOSFS may live at LBA0 (superfloppy-style).
pub fn locate(handle: block::DeviceHandle) -> Result<Option<TrueosFsPlacement>, block::Error> {
    if handle.parent().is_some() {
        return Ok(None);
    }

    // Prefer GPT-partitioned layouts (bootable-capable).
    if let Ok(parts) = partition::read_gpt_partitions(handle) {
        let mut has_esp = false;
        for p in parts.iter() {
            if p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES {
                has_esp = true;
            }

            // Our superblock is at the start of the TRUEOS data partition.
            if let Ok(p0) = read_blocks_aligned(handle, p.range.first_lba(), 1) {
                if looks_like_trueos_superblock(&p0) {
                    let super_lba = p.range.first_lba();
                    return Ok(Some(TrueosFsPlacement {
                        bootable: has_esp,
                        super_lba,
                        data_lba: trueos_fs::data_lba_from_super(super_lba),
                    }));
                }
            }
        }
    }

    // Fallback: superblock at LBA0 (data-only images/disks).
    let bs0 = read_blocks_aligned(handle, 0, 1)?;
    if looks_like_trueos_superblock(&bs0) {
        return Ok(Some(TrueosFsPlacement {
            bootable: false,
            super_lba: 0,
            data_lba: trueos_fs::data_lba_from_super(0),
        }));
    }

    Ok(None)
}

pub fn format_blank(handle: block::DeviceHandle) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    // If the disk is GPT-partitioned and has an ESP, do NOT clobber LBA0.
    // Only format an existing TRUEOS data partition (bootable layout).
    if let Ok(parts) = partition::read_gpt_partitions(handle) {
        let has_esp = parts
            .iter()
            .any(|p| p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES);

        if has_esp {
            if let Some(loc) = locate(handle)? {
                return format_blank_at(handle, loc.super_lba);
            }
            return Err(block::Error::NotSupported);
        }
    }

    // Data-only (superblock at LBA0).
    format_blank_at(handle, 0)
}

fn format_blank_at(handle: block::DeviceHandle, super_lba: u64) -> Result<(), block::Error> {

    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        (info.max_transfer_bytes as usize / bs).max(1)
    } else {
        1
    };
    let blocks = core::cmp::min(8usize, max_blocks);
    let bytes = bs.saturating_mul(blocks);

    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(bytes, align).ok_or(block::Error::DmaUnavailable)?;
    let buf = tmp.as_mut_slice();
    buf.fill(0);

    trueos_fs::write_blank_superblock(&mut buf[..bs]);

    handle.write_blocks(super_lba, buf)?;
    handle.flush()?;
    Ok(())
}
