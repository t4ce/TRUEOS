use alloc::vec::Vec;

use crate::disc::block;
use crate::disc::{partition, trueosfs};

use super::{fat32, gpt};

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
    // We only refresh the ESP boot files/config.
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

    log("install: stage=format_esp_fat32");
    log(
        alloc::format!(
            "install: esp range lba={} blocks={} (~{} MiB)",
            esp_range.first_lba(),
            esp_range.block_count(),
            (esp_range.block_count().saturating_mul(512) + (1024 * 1024 - 1)) / (1024 * 1024)
        )
        .as_str(),
    );
    if let Err(e) = fat32::format_and_populate_esp_with_log(
        esp_handle,
        fat32::EspImage {
            bootx64_efi,
            kernel_elf,
            payload_iso: None,
            limine_conf,
        },
        log,
    ) {
        log(alloc::format!("install: fat32 format/populate failed ({:?})", e).as_str());
        return Err(e);
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