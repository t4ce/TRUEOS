use crate::disc::block;

use crate::disc::{fat32, partition, trueosfs};

const PAYLOAD_HDR_MAGIC: &[u8; 8] = b"TRUEPLD0";
const PAYLOAD_HDR_VERSION: u32 = 1;

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

pub fn write_image_to_lba0(handle: block::DeviceHandle, image: &[u8]) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let blocks_needed: u64 = ((image.len() as u64) + (bs as u64) - 1) / (bs as u64);
    if blocks_needed == 0 {
        return Err(block::Error::InvalidParam);
    }
    if blocks_needed > info.block_count {
        return Err(block::Error::OutOfBounds);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        ((info.max_transfer_bytes as usize) / bs).max(1)
    } else {
        1
    };

    // Keep chunks reasonably sized to avoid long stalls on slower devices.
    let chunk_blocks = core::cmp::min(max_blocks, 256);
    let chunk_bytes = bs.saturating_mul(chunk_blocks);

    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(chunk_bytes, align).ok_or(block::Error::DmaUnavailable)?;
    let buf = tmp.as_mut_slice();

    let mut lba: u64 = 0;
    let mut off: usize = 0;
    while lba < blocks_needed {
        let remaining_blocks = (blocks_needed - lba) as usize;
        let this_blocks = core::cmp::min(chunk_blocks, remaining_blocks);
        let this_bytes = bs.saturating_mul(this_blocks);

        buf[..this_bytes].fill(0);
        let take = core::cmp::min(this_bytes, image.len().saturating_sub(off));
        if take > 0 {
            buf[..take].copy_from_slice(&image[off..off + take]);
        }

        handle.write_blocks(lba, &buf[..this_bytes])?;

        lba += this_blocks as u64;
        off = off.saturating_add(take);
    }

    handle.flush()?;
    Ok(())
}

pub fn write_payload_to_trueos_data(
    handle: block::DeviceHandle,
    payload: &[u8],
) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    let Some(placement) = crate::disc::trueosfs::locate(handle)? else {
        return Err(block::Error::NotSupported);
    };

    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    // First data block stores a small header; payload starts at the next block.
    let start_lba = placement.data_lba;
    let payload_lba = start_lba.saturating_add(1);

    let payload_blocks: u64 = ((payload.len() as u64) + (bs as u64) - 1) / (bs as u64);
    let blocks_needed = 1u64.saturating_add(payload_blocks);

    if blocks_needed == 0 {
        return Err(block::Error::InvalidParam);
    }

    let end_lba_exclusive = start_lba.saturating_add(blocks_needed);
    let limit_lba_exclusive = placement.data_end_lba_exclusive.unwrap_or(info.block_count);
    if end_lba_exclusive > limit_lba_exclusive {
        return Err(block::Error::OutOfBounds);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        ((info.max_transfer_bytes as usize) / bs).max(1)
    } else {
        1
    };
    let chunk_blocks = core::cmp::min(max_blocks, 256);
    let chunk_bytes = bs.saturating_mul(chunk_blocks);

    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(chunk_bytes, align).ok_or(block::Error::DmaUnavailable)?;
    let buf = tmp.as_mut_slice();

    // Write header block.
    buf[..bs].fill(0);
    buf[..PAYLOAD_HDR_MAGIC.len()].copy_from_slice(PAYLOAD_HDR_MAGIC);
    buf[8..12].copy_from_slice(&PAYLOAD_HDR_VERSION.to_le_bytes());
    buf[12..20].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    handle.write_blocks(start_lba, &buf[..bs])?;

    // Write payload blocks.
    let mut lba = payload_lba;
    let mut off: usize = 0;
    let mut remaining_blocks = payload_blocks;
    while remaining_blocks > 0 {
        let this_blocks = core::cmp::min(chunk_blocks as u64, remaining_blocks) as usize;
        let this_bytes = bs.saturating_mul(this_blocks);

        buf[..this_bytes].fill(0);
        let take = core::cmp::min(this_bytes, payload.len().saturating_sub(off));
        if take > 0 {
            buf[..take].copy_from_slice(&payload[off..off + take]);
        }

        handle.write_blocks(lba, &buf[..this_bytes])?;

        lba = lba.saturating_add(this_blocks as u64);
        off = off.saturating_add(take);
        remaining_blocks = remaining_blocks.saturating_sub(this_blocks as u64);
    }

    handle.flush()?;
    Ok(())
}

pub fn install_bootable_uefi_gpt(
    disk: block::DeviceHandle,
    bootx64_efi: &[u8],
    kernel_elf: &[u8],
    payload_iso: &[u8],
) -> Result<(), block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !disk.supports_write() {
        return Err(block::Error::NotSupported);
    }

    // 256 MiB ESP: enough for BOOTX64 + kernel + payload.
    let layout = partition::write_trueos_bootable_gpt_layout(disk, 256)?;

    // Create in-memory partition devices matching the written ranges.
    let parent_id = disk.id();
    let parent_info = disk.info();

    let esp_handle = {
        let mut d = block::DeviceDescriptor::new(block::DeviceKind::Partition).with_parent(parent_id);
        d.label = Some("TRUEOS ESP".into());
        d.pci = parent_info.pci;
        if !parent_info.writable {
            d = d.mark_read_only();
        }
        let dev = partition::PartitionBlockDevice::new(disk, layout.esp);
        block::register_device(d, dev)
    };

    let trueos_handle = {
        let mut d = block::DeviceDescriptor::new(block::DeviceKind::Partition).with_parent(parent_id);
        d.label = Some("TRUEOS".into());
        d.pci = parent_info.pci;
        if !parent_info.writable {
            d = d.mark_read_only();
        }
        let dev = partition::PartitionBlockDevice::new(disk, layout.trueos);
        block::register_device(d, dev)
    };

    // Generate an on-disk limine.conf that points to the files we write into the ESP.
    // Note: we use short 8.3 names in FAT32 to avoid long filename complexity.
    let limine_conf = b"timeout: 0\n\
default_entry: 1\n\
\n\
/TRUEOS\n\
protocol: limine\n\
kernel_path: boot():/TRUEOS.ELF\n\
resolution: 1920x1080x32\n\
module_path: boot():/install/PAYLOAD.ISO\n\
module_string: trueos.install.payload\n\n";

    fat32::format_and_populate_esp_fat32(
        esp_handle,
        fat32::EspImage {
            bootx64_efi,
            kernel_elf,
            payload_iso,
            limine_conf,
        },
    )?;

    // Format TRUEOSFS at the start of the TRUEOS data partition.
    trueosfs::format_blank_partition(trueos_handle)?;

    Ok(())
}