use crate::disc::block;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatVolumeLayout {
    /// FAT boot sector is at LBA0.
    ///
    /// Note: `whole_disk` is true only when the BPB total sector count matches the device size.
    FatAtLba0 { total_sectors: u32, whole_disk: bool },

    /// MBR at LBA0, with a FAT partition whose boot sector is at `start_lba`.
    MbrPartition { start_lba: u32, sectors: u32, part_type: u8 },

    /// GPT partitioned disk, with a partition whose first LBA contains a FAT boot sector.
    GptPartition { start_lba: u64, sectors: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeError {
    UnsupportedBlockSize(u32),
    DeviceIo(block::Error),
    UnknownLayout,
}

impl From<block::Error> for ProbeError {
    fn from(value: block::Error) -> Self {
        ProbeError::DeviceIo(value)
    }
}


fn read_u16_le(bs: &[u8; 512], off: usize) -> u16 {
    u16::from_le_bytes([bs[off], bs[off + 1]])
}

fn read_u32_le(bs: &[u8; 512], off: usize) -> u32 {
    u32::from_le_bytes([bs[off], bs[off + 1], bs[off + 2], bs[off + 3]])
}

fn read_u64_le(bs: &[u8; 512], off: usize) -> u64 {
    u64::from_le_bytes([
        bs[off],
        bs[off + 1],
        bs[off + 2],
        bs[off + 3],
        bs[off + 4],
        bs[off + 5],
        bs[off + 6],
        bs[off + 7],
    ])
}

fn is_valid_sectors_per_cluster(v: u8) -> bool {
    matches!(v, 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128)
}

fn looks_like_fat_boot_sector(bs: &[u8; 512]) -> Option<u32> {
    // Signature.
    if bs[510] != 0x55 || bs[511] != 0xAA {
        return None;
    }

    // Jump instruction is very commonly EB xx 90 or E9 xx xx.
    let j0 = bs[0];
    if !(j0 == 0xEB || j0 == 0xE9) {
        return None;
    }

    let bytes_per_sector = read_u16_le(bs, 11);
    let sectors_per_cluster = bs[13];
    let reserved_sectors = read_u16_le(bs, 14);
    let fats = bs[16];
    let root_entry_count = read_u16_le(bs, 17);
    let tot16 = read_u16_le(bs, 19);
    let tot32 = read_u32_le(bs, 32);
    let total_sectors = if tot16 != 0 { tot16 as u32 } else { tot32 };

    let spf16 = read_u16_le(bs, 22);
    let spf32 = read_u32_le(bs, 36);
    let sectors_per_fat = if spf16 != 0 { spf16 as u32 } else { spf32 };

    if bytes_per_sector != 512 {
        return None;
    }
    if !is_valid_sectors_per_cluster(sectors_per_cluster) {
        return None;
    }
    if reserved_sectors == 0 {
        return None;
    }
    if fats == 0 || fats > 4 {
        return None;
    }
    if total_sectors == 0 || sectors_per_fat == 0 {
        return None;
    }

    // Basic sanity: compute first data sector like in the FAT spec.
    let root_dir_sectors = ((root_entry_count as u32 * 32) + (bytes_per_sector as u32 - 1)) / (bytes_per_sector as u32);
    let first_data_sector = (reserved_sectors as u32) + (fats as u32) * sectors_per_fat + root_dir_sectors;
    if first_data_sector >= total_sectors {
        return None;
    }

    Some(total_sectors)
}

fn is_fat_mbr_type(t: u8) -> bool {
    matches!(t,
        0x01 | // FAT12
        0x04 | 0x06 | // FAT16
        0x0B | 0x0C | // FAT32
        0x0E   // FAT16 LBA
    )
}

/// Probe for a FAT volume on a raw block device.
///
/// We support three layouts:
/// - FAT superfloppy (FAT boot sector at LBA0)
/// - MBR-partitioned disk with a FAT partition (boot sector at partition start)
/// - GPT-partitioned disk with a FAT partition (boot sector at partition start)
pub async fn probe_fat_volume(handle: block::DeviceHandle) -> Result<FatVolumeLayout, ProbeError> {
    let info = handle.info();
    if info.block_size != 512 {
        return Err(ProbeError::UnsupportedBlockSize(info.block_size));
    }

    let lba0_vec = handle.read_blocks(0, 1).await?;
    if lba0_vec.len() < 512 {
        return Err(ProbeError::DeviceIo(block::Error::Io));
    }
    let mut lba0 = [0u8; 512];
    lba0.copy_from_slice(&lba0_vec[..512]);

    if let Some(total_sectors) = looks_like_fat_boot_sector(&lba0) {
        let whole_disk = (total_sectors as u64) == info.block_count;
        return Ok(FatVolumeLayout::FatAtLba0 {
            total_sectors,
            whole_disk,
        });
    }

    // Try GPT: read header at LBA1 and scan partition entries for a FAT boot sector.
    // CRC validation is intentionally skipped for now; we only use this to find the FAT volume.
    if info.block_count >= 2 {
        let hdr_vec = handle.read_blocks(1, 1).await?;
        if hdr_vec.len() < 512 {
            return Err(ProbeError::DeviceIo(block::Error::Io));
        }
        let mut hdr = [0u8; 512];
        hdr.copy_from_slice(&hdr_vec[..512]);

        if &hdr[0..8] == b"EFI PART" {
            let entries_lba = read_u64_le(&hdr, 0x48);
            let num_entries = read_u32_le(&hdr, 0x50) as usize;
            let entry_size = read_u32_le(&hdr, 0x54) as usize;

            if entries_lba != 0
                && num_entries != 0
                && entry_size >= 56
                && entry_size <= 512
                && entries_lba < info.block_count
            {
                let scan_count = core::cmp::min(num_entries, 256);
                for i in 0..scan_count {
                    let entry_off = (i as u64) * (entry_size as u64);
                    let lba = entries_lba + (entry_off / 512);
                    let off = (entry_off % 512) as usize;
                    if lba >= info.block_count {
                        break;
                    }

                    // Read just the GPT entry header (first 56 bytes) so we can
                    // parse fields without holding a borrow of the DMA buffer.
                    let mut entry_hdr = [0u8; 56];
                    if off + 56 <= 512 {
                        let v = handle.read_blocks(lba, 1).await?;
                        if v.len() < 512 {
                            return Err(ProbeError::DeviceIo(block::Error::Io));
                        }
                        entry_hdr.copy_from_slice(&v[off..off + 56]);
                    } else {
                        if lba.saturating_add(1) >= info.block_count {
                            break;
                        }
                        let v = handle.read_blocks(lba, 2).await?;
                        if v.len() < 1024 {
                            return Err(ProbeError::DeviceIo(block::Error::Io));
                        }
                        entry_hdr.copy_from_slice(&v[off..off + 56]);
                    }

                    // Unused GPT entry has an all-zero type GUID.
                    if entry_hdr[..16].iter().all(|&b| b == 0) {
                        continue;
                    }

                    let first_lba = u64::from_le_bytes([
                        entry_hdr[32],
                        entry_hdr[33],
                        entry_hdr[34],
                        entry_hdr[35],
                        entry_hdr[36],
                        entry_hdr[37],
                        entry_hdr[38],
                        entry_hdr[39],
                    ]);
                    let last_lba = u64::from_le_bytes([
                        entry_hdr[40],
                        entry_hdr[41],
                        entry_hdr[42],
                        entry_hdr[43],
                        entry_hdr[44],
                        entry_hdr[45],
                        entry_hdr[46],
                        entry_hdr[47],
                    ]);
                    if first_lba == 0 || last_lba == 0 || first_lba > last_lba {
                        continue;
                    }
                    if first_lba >= info.block_count || last_lba >= info.block_count {
                        continue;
                    }

                    let bs_vec = handle.read_blocks(first_lba, 1).await?;
                    if bs_vec.len() < 512 {
                        return Err(ProbeError::DeviceIo(block::Error::Io));
                    }
                    let mut bs = [0u8; 512];
                    bs.copy_from_slice(&bs_vec[..512]);
                    if looks_like_fat_boot_sector(&bs).is_some() {
                        let sectors = last_lba
                            .saturating_sub(first_lba)
                            .saturating_add(1);
                        return Ok(FatVolumeLayout::GptPartition {
                            start_lba: first_lba,
                            sectors,
                        });
                    }
                }
            }
        }
    }

    // Try MBR: look for a FAT-type partition and validate the partition boot sector.
    if lba0[510] != 0x55 || lba0[511] != 0xAA {
        return Err(ProbeError::UnknownLayout);
    }

    for i in 0..4usize {
        let off = 0x1BE + i * 16;
        let part_type = lba0[off + 4];
        let start_lba = read_u32_le(&lba0, off + 8);
        let sectors = read_u32_le(&lba0, off + 12);

        if part_type == 0 || start_lba == 0 || sectors == 0 {
            continue;
        }
        if !is_fat_mbr_type(part_type) {
            continue;
        }
        let start_lba_u64 = start_lba as u64;
        let sectors_u64 = sectors as u64;
        if start_lba_u64 >= info.block_count {
            continue;
        }
        if start_lba_u64.saturating_add(sectors_u64) > info.block_count {
            continue;
        }

        // Validate partition boot sector is FAT.
        let bs_vec = handle.read_blocks(start_lba_u64, 1).await?;
        if bs_vec.len() < 512 {
            return Err(ProbeError::DeviceIo(block::Error::Io));
        }
        let mut bs = [0u8; 512];
        bs.copy_from_slice(&bs_vec[..512]);
        if looks_like_fat_boot_sector(&bs).is_some() {
            return Ok(FatVolumeLayout::MbrPartition {
                start_lba,
                sectors,
                part_type,
            });
        }
    }

    Err(ProbeError::UnknownLayout)
}

/// Convert a detected FAT layout into a slice (base LBA, number of blocks) to use for mounting.
pub fn fat_slice_for_mount(layout: FatVolumeLayout, disk_blocks: u64) -> (u64, u64) {
    match layout {
        FatVolumeLayout::FatAtLba0 { total_sectors, .. } => (0, core::cmp::min(total_sectors as u64, disk_blocks)),
        FatVolumeLayout::MbrPartition { start_lba, sectors, .. } => {
            let base = start_lba as u64;
            let blocks = core::cmp::min(sectors as u64, disk_blocks.saturating_sub(base));
            (base, blocks)
        }
        FatVolumeLayout::GptPartition { start_lba, sectors } => {
            let base = start_lba;
            let blocks = core::cmp::min(sectors, disk_blocks.saturating_sub(base));
            (base, blocks)
        }
    }
}
