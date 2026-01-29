use alloc::vec::Vec;

use crate::disc::block;
use crate::disc::{partition, trueosfs};
use crate::install_assets;

mod fat32;
mod gpt;

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

fn write_blocks_aligned(handle: block::DeviceHandle, lba: u64, buf: &[u8]) -> Result<(), block::Error> {
    let info = handle.info();
    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(buf.len(), align).ok_or(block::Error::DmaUnavailable)?;
    tmp.as_mut_slice().copy_from_slice(buf);
    handle.write_blocks(lba, tmp.as_mut_slice())
}

fn write_bytes_at_lba(handle: block::DeviceHandle, start_lba: u64, bytes: &[u8]) -> Result<(), block::Error> {
    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    let blocks_needed: u64 = ((bytes.len() as u64) + (bs as u64) - 1) / (bs as u64);
    if blocks_needed == 0 {
        return Ok(());
    }
    if start_lba.saturating_add(blocks_needed) > info.block_count {
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

    let mut lba = start_lba;
    let mut off: usize = 0;
    let mut remaining_blocks = blocks_needed;
    while remaining_blocks > 0 {
        let this_blocks = core::cmp::min(chunk_blocks as u64, remaining_blocks) as usize;
        let this_bytes = bs.saturating_mul(this_blocks);

        buf[..this_bytes].fill(0);
        let take = core::cmp::min(this_bytes, bytes.len().saturating_sub(off));
        if take > 0 {
            buf[..take].copy_from_slice(&bytes[off..off + take]);
        }

        handle.write_blocks(lba, &buf[..this_bytes])?;

        // Keep the system responsive while we do large synchronous transfers.
        crate::time::poll_executor();

        lba = lba.saturating_add(this_blocks as u64);
        off = off.saturating_add(take);
        remaining_blocks = remaining_blocks.saturating_sub(this_blocks as u64);
    }
    Ok(())
}

fn install_limine_bios_stages(
    disk: block::DeviceHandle,
    stage2_loc_bytes: u64,
    limine_hdd_bin: &[u8],
) -> Result<(), block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !disk.supports_write() {
        return Err(block::Error::NotSupported);
    }

    let info = disk.info();
    if info.block_size != 512 {
        return Err(block::Error::NotSupported);
    }
    if stage2_loc_bytes % 512 != 0 {
        return Err(block::Error::InvalidParam);
    }
    if limine_hdd_bin.len() < 512 {
        return Err(block::Error::InvalidParam);
    }

    // Preserve timestamp and partition table, like limine's own bios-install.
    let orig0 = read_blocks_aligned(disk, 0, 1)?;
    if orig0.len() < 512 {
        return Err(block::Error::InvalidParam);
    }
    let mut timestamp = [0u8; 6];
    timestamp.copy_from_slice(&orig0[218..224]);
    let mut orig_mbr = [0u8; 70];
    orig_mbr.copy_from_slice(&orig0[440..510]);

    // Write bootsector (first 512 bytes of limine-bios-hdd.bin) to LBA0.
    write_blocks_aligned(disk, 0, &limine_hdd_bin[0..512])?;

    // Write remainder of stage2 to the requested location.
    let stage2 = &limine_hdd_bin[512..];
    let stage2_lba = stage2_loc_bytes / 512;
    write_bytes_at_lba(disk, stage2_lba, stage2)?;

    // Patch stage2 location in the bootsector and restore timestamp + partition table.
    let mut bs0 = read_blocks_aligned(disk, 0, 1)?;
    if bs0.len() < 512 {
        return Err(block::Error::InvalidParam);
    }
    bs0[0x1a4..0x1a4 + 8].copy_from_slice(&stage2_loc_bytes.to_le_bytes());
    bs0[218..224].copy_from_slice(&timestamp);
    bs0[440..510].copy_from_slice(&orig_mbr);
    write_blocks_aligned(disk, 0, &bs0[..512])?;

    disk.flush()?;
    Ok(())
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
) -> Result<(), block::Error> {
    install_bootable_uefi_gpt_with_log(disk, bootx64_efi, kernel_elf, &mut |_| {})
}

pub fn install_bootable_uefi_gpt_with_log(
    disk: block::DeviceHandle,
    bootx64_efi: &[u8],
    kernel_elf: &[u8],
    log: &mut dyn FnMut(&str),
) -> Result<(), block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !disk.supports_write() {
        return Err(block::Error::NotSupported);
    }

    // Size ESP to actually fit the files (+ slack), rather than hardcoding 256MiB.
    // We keep a minimum to avoid pathological tiny ESPs.
    // Installed TRUEOS does not need the installer payload on the ESP.
    let esp_bytes_needed = (bootx64_efi.len() as u64)
        .saturating_add(kernel_elf.len() as u64)
        .saturating_add(64 * 1024) // limine.conf + dir entries
        .saturating_add(16 * 1024 * 1024); // slack for FAT tables/rounding

    let mut esp_mib = (esp_bytes_needed + (1024 * 1024 - 1)) / (1024 * 1024);
    // Round up to 32MiB increments for nicer alignment.
    esp_mib = ((esp_mib + 31) / 32) * 32;
    esp_mib = core::cmp::max(64, esp_mib);

    log(alloc::format!("install: esp size target={} MiB", esp_mib).as_str());

    // If this disk already contains a TRUEOSFS partition inside a GPT layout, preserve it.
    // We only refresh the ESP boot files/config and (re)install Limine BIOS stages.
    log("install: stage=probe_trueosfs");
    let existing_trueosfs = match trueosfs::locate(disk) {
        Ok(v) => v,
        Err(e) => {
            log(alloc::format!("install: trueosfs::locate failed ({:?})", e).as_str());
            return Err(e);
        }
    };

    log("install: stage=probe_gpt");
    let existing_parts = partition::read_gpt_partitions(disk).ok();

    let mut preserve_trueosfs = false;
    let mut bios_boot_start_lba: Option<u64> = None;

    // Create in-memory partition devices matching the intended ranges.
    let parent_id = disk.id();
    let parent_info = disk.info();

    let (esp_range, trueos_range) = if let (Some(loc), Some(parts)) = (existing_trueosfs, existing_parts) {
        let esp = parts
            .iter()
            .find(|p| p.type_guid.as_bytes() == &partition::GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES)
            .map(|p| p.range);
        let trueos = parts
            .iter()
            .find(|p| p.range.first_lba() == loc.super_lba)
            .map(|p| p.range);
        bios_boot_start_lba = parts
            .iter()
            .find(|p| p.type_guid.as_bytes() == &partition::GPT_TYPE_BIOS_BOOT_PARTITION_BYTES)
            .map(|p| p.range.first_lba());

        if let (Some(esp), Some(trueos)) = (esp, trueos) {
            preserve_trueosfs = true;
            (esp, trueos)
        } else {
            log("install: stage=write_gpt (refresh)");
            let layout = match gpt::write_trueos_bootable_gpt_layout_with_log(disk, esp_mib, log) {
                Ok(v) => v,
                Err(e) => {
                    log(alloc::format!("install: gpt write failed ({:?})", e).as_str());
                    return Err(e);
                }
            };
            bios_boot_start_lba = Some(layout.bios_boot.first_lba());
            (layout.esp, layout.trueos)
        }
    } else {
        log("install: stage=write_gpt (fresh)");
        let layout = match gpt::write_trueos_bootable_gpt_layout_with_log(disk, esp_mib, log) {
            Ok(v) => v,
            Err(e) => {
                log(alloc::format!("install: gpt write failed ({:?})", e).as_str());
                return Err(e);
            }
        };
        bios_boot_start_lba = Some(layout.bios_boot.first_lba());
        (layout.esp, layout.trueos)
    };

    let esp_handle = {
        let mut d = block::DeviceDescriptor::new(block::DeviceKind::Partition).with_parent(parent_id);
        d.label = Some("TRUEOS ESP".into());
        d.pci = parent_info.pci;
        if !parent_info.writable {
            d = d.mark_read_only();
        }
        let dev = partition::PartitionBlockDevice::new(disk, esp_range);
        block::register_device(d, dev)
    };

    let trueos_handle = {
        let mut d = block::DeviceDescriptor::new(block::DeviceKind::Partition).with_parent(parent_id);
        d.label = Some("TRUEOS".into());
        d.pci = parent_info.pci;
        if !parent_info.writable {
            d = d.mark_read_only();
        }
        let dev = partition::PartitionBlockDevice::new(disk, trueos_range);
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
resolution: 1920x1080x32\n\n";

    let limine_bios_sys = install_assets::limine_bios_sys();
    let limine_hdd_bin = install_assets::limine_bios_hdd_bin();

    log("install: stage=format_esp_fat32");
    if let Err(e) = fat32::format_and_populate_esp_fat32(
        esp_handle,
        fat32::EspImage {
            bootx64_efi,
            kernel_elf,
            limine_bios_sys: Some(&limine_bios_sys),
            payload_iso: None,
            limine_conf,
        },
    ) {
        log(alloc::format!("install: fat32 format/populate failed ({:?})", e).as_str());
        return Err(e);
    }

    // Install Limine BIOS stages (MBR + stage2), using a BIOS boot partition on GPT.
    if let Some(start_lba) = bios_boot_start_lba {
        log("install: stage=install_limine_bios");
        let stage2_loc_bytes = start_lba.saturating_mul(512);
        if let Err(e) = install_limine_bios_stages(disk, stage2_loc_bytes, &limine_hdd_bin) {
            log(alloc::format!(
                "install: limine bios stage install failed ({:?}) (stage2_loc_bytes=0x{:X})",
                e, stage2_loc_bytes
            )
            .as_str());
            return Err(e);
        }
    }

    // Only format TRUEOSFS on first install. Re-installs preserve existing TRUEOSFS content.
    if !preserve_trueosfs {
        log("install: stage=format_trueosfs");
        if let Err(e) = trueosfs::format_blank_partition(trueos_handle) {
            log(alloc::format!("install: trueosfs format failed ({:?})", e).as_str());
            return Err(e);
        }
    }

    Ok(())
}