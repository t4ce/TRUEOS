//! Native release transport shared by published builds and RAM-only updates.

use alloc::vec::Vec;

pub(crate) const MAX_COMPRESSED_BYTES: usize = 128 * 1024 * 1024;
const MAX_KERNEL_BYTES: usize = 256 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 32;

/// Validate the complete native boot bundle and retain its kernel for a swap.
/// Boot modules remain the running firmware boot's modules during a RAM update.
pub(crate) async fn kernel_from_archive(payload: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    if payload.len() > MAX_COMPRESSED_BYTES {
        return Err("compressed update bundle exceeds limit");
    }
    if !payload.starts_with(&crate::r::lz4::MAGIC) {
        return Err("update bundle is not an LZ4 frame");
    }
    let tar = crate::r::lz4::decompress_frame(payload, MAX_ARCHIVE_BYTES)
        .await
        .map_err(|_| "invalid or oversized update LZ4 frame")?;
    let entries = crate::r::tar::unpack(&tar, MAX_ENTRIES, MAX_KERNEL_BYTES, MAX_ARCHIVE_BYTES)
        .map_err(|_| "invalid or oversized update tar archive")?;
    drop(tar);

    let mut kernel = None;
    let mut bootloader = false;
    let mut firmware = false;
    for entry in entries {
        match entry.name.as_str() {
            "TRUEOS.elf" => {
                if kernel.is_some() {
                    return Err("duplicate TRUEOS.elf");
                }
                if !entry.bytes.starts_with(b"\x7fELF") {
                    return Err("TRUEOS.elf has no ELF magic");
                }
                kernel = Some(entry.bytes);
            }
            "EFI/BOOT/BOOTX64.EFI" => {
                if bootloader {
                    return Err("duplicate EFI/BOOT/BOOTX64.EFI");
                }
                if !entry.bytes.starts_with(b"MZ") {
                    return Err("BOOTX64.EFI has no PE magic");
                }
                bootloader = true;
            }
            "EFI/BOOT/tgl_guc_70.bin" => {
                if firmware {
                    return Err("duplicate EFI/BOOT/tgl_guc_70.bin");
                }
                if entry.bytes.is_empty() {
                    return Err("empty EFI/BOOT/tgl_guc_70.bin");
                }
                firmware = true;
            }
            _ => return Err("unexpected update archive member"),
        }
    }
    if !bootloader {
        return Err("update archive missing EFI/BOOT/BOOTX64.EFI");
    }
    if !firmware {
        return Err("update archive missing EFI/BOOT/tgl_guc_70.bin");
    }
    kernel.ok_or("update archive missing TRUEOS.elf")
}
