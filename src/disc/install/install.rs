use crate::disc::block;
use crate::v::disc::partition;
use crate::v::fs::trueosfs;

use super::{fat32, gpt};

pub async fn install_bootable_uefi_gpt_with_log(
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
    let existing_trueosfs = match trueosfs::locate_async(disk).await {
        Ok(v) => v,
        Err(e) => {
            log(alloc::format!("install: trueosfs::locate failed ({:?}); continuing", e).as_str());
            // If the disk is temporarily not ready or I/O is failing, abort rather than
            // attempting a destructive repartition.
            if matches!(e, block::Error::NotReady | block::Error::Timeout | block::Error::Io) {
                return Err(e);
            }
            None
        }
    };

    log("install: stage=probe_gpt");
    let existing_parts = partition::read_gpt_partitions(disk).await.ok();

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
            let layout = match gpt::write_trueos_bootable_gpt_layout_with_log(disk, esp_mib, log).await {
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
        let layout = match gpt::write_trueos_bootable_gpt_layout_with_log(disk, esp_mib, log).await {
            Ok(v) => v,
            Err(e) => {
                log(alloc::format!("install: gpt write failed ({:?})", e).as_str());
                return Err(e);
            }
        };
        (layout.esp, layout.trueos)
    };

    log(
        alloc::format!(
            "install: preserve_trueosfs={} (esp lba={} blocks={}, trueos lba={} blocks={})",
            preserve_trueosfs,
            esp_range.first_lba(),
            esp_range.block_count(),
            trueos_range.first_lba(),
            trueos_range.block_count(),
        )
        .as_str(),
    );

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
            limine_conf,
        },
        log,
    )
    .await {
        log(alloc::format!("install: fat32 format/populate failed ({:?})", e).as_str());
        return Err(e);
    }

    // Only format TRUEOSFS on first install. Re-installs preserve existing TRUEOSFS content.
    if !preserve_trueosfs {
        log("install: stage=format_trueosfs");
        if let Err(e) = trueosfs::format_blank_partition_async(trueos_handle).await {
            log(alloc::format!("install: trueosfs format failed ({:?})", e).as_str());
            return Err(e);
        }
    }

    Ok(())
}