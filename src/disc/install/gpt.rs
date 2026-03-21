use alloc::{vec, vec::Vec};

use crate::disc::block::{DeviceHandle, Error, Result};
use crate::r::disc::partition::{
    BlockRange, GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES, GPT_TYPE_LINUX_FILESYSTEM_BYTES,
    TrueosBootLayout,
};

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const GPT_HEADER_LBA: u64 = 1;
const GPT_MIN_HEADER_SIZE: u32 = 92;

const GPT_PARTITION_NAME_BYTES: usize = 72;

const GPT_DEFAULT_ENTRY_COUNT: u32 = 128;
const GPT_DEFAULT_ENTRY_SIZE: u32 = 128;
const GPT_DEFAULT_TABLE_LBA: u64 = 2;

const GPT_PROTECTIVE_MBR_SIGNATURE: u16 = 0xAA55;

const GPT_ALIGN_LBA: u64 = 2048; // 1MiB @ 512B sectors

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartitionSize {
    /// Fixed size in MiB.
    Mib(u64),
    /// Consume the remaining usable LBAs.
    ///
    /// Constraint: if present, this must be the last partition.
    Remaining,
}

#[derive(Clone, Copy, Debug)]
pub struct GptPartitionSpec<'a> {
    pub type_guid: [u8; 16],
    pub name: &'a str,
    pub size: PartitionSize,
    pub attributes: u64,
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

fn align_up_u64(value: u64, align: u64) -> u64 {
    if align <= 1 {
        return value;
    }
    let rem = value % align;
    if rem == 0 {
        value
    } else {
        value + (align - rem)
    }
}

fn crc32_ieee(bytes: &[u8]) -> u32 {
    // CRC-32 (IEEE 802.3), reflected.
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn fill_guid_bytes(out: &mut [u8; 16]) {
    out.fill(0);
    if crate::rng::fill_bytes(out) {
        // Ensure it's not all-zero.
        if out.iter().all(|b| *b == 0) {
            out[0] = 1;
        }
        // UUID v4-ish bits (helps tooling, not required for GPT).
        out[6] = (out[6] & 0x0F) | 0x40;
        out[8] = (out[8] & 0x3F) | 0x80;
        return;
    }

    // Fallback: deterministic non-zero marker.
    out[0] = 1;
}

fn write_utf16le_fixed(dst: &mut [u8], s: &str) {
    dst.fill(0);
    let mut off = 0;
    for w in s.encode_utf16() {
        if off + 2 > dst.len() {
            break;
        }
        let b = w.to_le_bytes();
        dst[off] = b[0];
        dst[off + 1] = b[1];
        off += 2;
    }
}

async fn write_blocks_aligned_with_log(
    device: DeviceHandle,
    lba: u64,
    buf: &[u8],
    log: &mut dyn FnMut(&str),
) -> Result<()> {
    let info = device.info();
    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(buf.len(), align).ok_or(Error::DmaUnavailable)?;
    tmp.as_mut_slice().copy_from_slice(buf);
    match device.write_blocks(lba, tmp.as_mut_slice()).await {
        Ok(()) => Ok(()),
        Err(e) => {
            log(alloc::format!(
                "install: gpt: write failed lba={} bytes={} err={:?}",
                lba,
                buf.len(),
                e
            )
            .as_str());
            Err(e)
        }
    }
}

fn mib_to_blocks_512(mib: u64) -> u64 {
    // 1MiB / 512B = 2048 sectors.
    mib.saturating_mul(2048)
}

fn validate_partition_specs(parts: &[GptPartitionSpec<'_>]) -> Result<()> {
    if parts.is_empty() {
        return Err(Error::InvalidParam);
    }
    if (parts.len() as u32) > GPT_DEFAULT_ENTRY_COUNT {
        return Err(Error::OutOfBounds);
    }

    // At most one Remaining, and it must be last.
    let mut remaining_idx: Option<usize> = None;
    for (i, p) in parts.iter().enumerate() {
        if matches!(p.size, PartitionSize::Remaining) {
            if remaining_idx.is_some() {
                return Err(Error::InvalidParam);
            }
            remaining_idx = Some(i);
        }
    }
    if let Some(i) = remaining_idx
        && i + 1 != parts.len()
    {
        return Err(Error::InvalidParam);
    }

    Ok(())
}

/// Create a fresh GPT partition table with an arbitrary list of partitions.
///
/// Notes/constraints (current implementation):
/// - Assumes 512-byte LBAs.
/// - Uses a fixed partition entry array size (128 entries x 128 bytes).
/// - Partitions are allocated sequentially, aligned to 1MiB boundaries.
/// - `PartitionSize::Remaining` (if used) must be last.
///
/// Returns computed absolute LBA ranges for each partition, in the same order as `parts`.
pub async fn write_gpt_layout_with_log(
    device: DeviceHandle,
    parts: &[GptPartitionSpec<'_>],
    log: &mut dyn FnMut(&str),
) -> Result<Vec<BlockRange>> {
    if device.parent().is_some() {
        return Err(Error::InvalidParam);
    }
    if !device.supports_write() {
        return Err(Error::NotSupported);
    }

    validate_partition_specs(parts)?;

    let info = device.info();
    if info.block_size != 512 {
        // Simplify: GPT+FAT32 writer currently assumes 512-byte LBAs.
        return Err(Error::NotSupported);
    }
    if info.block_count < 10_000 {
        return Err(Error::OutOfBounds);
    }

    let last_lba = info.block_count.saturating_sub(1);

    let entry_count = GPT_DEFAULT_ENTRY_COUNT;
    let entry_size = GPT_DEFAULT_ENTRY_SIZE;
    let table_bytes = (entry_count as usize) * (entry_size as usize);
    let table_lbas = (table_bytes as u64).div_ceil(512);

    let first_usable = GPT_DEFAULT_TABLE_LBA + table_lbas;
    let last_usable = last_lba.saturating_sub(table_lbas).saturating_sub(1);
    if first_usable >= last_usable {
        return Err(Error::OutOfBounds);
    }

    // Allocate partition ranges.
    let mut ranges: Vec<BlockRange> = Vec::with_capacity(parts.len());
    let mut cur = align_up_u64(first_usable, GPT_ALIGN_LBA);
    for (idx, p) in parts.iter().enumerate() {
        if cur < first_usable {
            return Err(Error::OutOfBounds);
        }
        if cur > last_usable {
            return Err(Error::OutOfBounds);
        }

        let (first, last) = match p.size {
            PartitionSize::Mib(mib) => {
                let blocks = mib_to_blocks_512(mib);
                if blocks == 0 {
                    return Err(Error::InvalidParam);
                }
                let last = cur
                    .checked_add(blocks)
                    .ok_or(Error::OutOfBounds)?
                    .saturating_sub(1);
                (cur, last)
            }
            PartitionSize::Remaining => {
                if idx + 1 != parts.len() {
                    return Err(Error::InvalidParam);
                }
                (cur, last_usable)
            }
        };

        if last < first || last > last_usable {
            return Err(Error::OutOfBounds);
        }

        ranges.push(BlockRange::from_bounds(first, last)?);

        // Next partition start (aligned) unless this was the last partition.
        if idx + 1 != parts.len() {
            cur = align_up_u64(last.saturating_add(1), GPT_ALIGN_LBA);
        }
    }

    // Protective MBR @ LBA0
    let mut pmbr = [0u8; 512];
    // Partition entry 0 @ offset 446
    // status
    pmbr[446] = 0x00;
    // first CHS (ignored)
    pmbr[447] = 0x00;
    pmbr[448] = 0x02;
    pmbr[449] = 0x00;
    // type
    pmbr[450] = 0xEE;
    // last CHS (ignored)
    pmbr[451] = 0xFF;
    pmbr[452] = 0xFF;
    pmbr[453] = 0xFF;
    // first LBA
    pmbr[454..458].copy_from_slice(&1u32.to_le_bytes());
    // sectors (clamp to u32::MAX)
    let mbr_sectors = core::cmp::min(info.block_count - 1, u32::MAX as u64) as u32;
    pmbr[458..462].copy_from_slice(&mbr_sectors.to_le_bytes());
    pmbr[510..512].copy_from_slice(&GPT_PROTECTIVE_MBR_SIGNATURE.to_le_bytes());

    write_blocks_aligned_with_log(device, 0, &pmbr, log).await?;

    // Partition entry array (fixed-size, with our entries at the start).
    let mut entries = vec![0u8; table_bytes];
    for (idx, p) in parts.iter().enumerate() {
        let off = idx * (entry_size as usize);

        // type GUID
        entries[off..off + 16].copy_from_slice(&p.type_guid);

        // unique GUID
        let mut unique = [0u8; 16];
        fill_guid_bytes(&mut unique);
        entries[off + 16..off + 32].copy_from_slice(&unique);

        // first/last LBA
        let r = ranges[idx];
        entries[off + 32..off + 40].copy_from_slice(&r.first_lba().to_le_bytes());
        entries[off + 40..off + 48].copy_from_slice(&r.last_lba().to_le_bytes());

        // attributes
        entries[off + 48..off + 56].copy_from_slice(&p.attributes.to_le_bytes());

        // name (UTF-16LE, fixed field)
        write_utf16le_fixed(
            &mut entries[off + 56..off + 56 + GPT_PARTITION_NAME_BYTES],
            p.name,
        );
    }

    let entries_crc = crc32_ieee(&entries);

    // GPT header (primary)
    let mut header = [0u8; 512];
    header[..8].copy_from_slice(GPT_SIGNATURE);
    header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    header[12..16].copy_from_slice(&GPT_MIN_HEADER_SIZE.to_le_bytes());
    // header[16..20] crc filled later
    // header[20..24] reserved
    header[24..32].copy_from_slice(&GPT_HEADER_LBA.to_le_bytes());
    header[32..40].copy_from_slice(&last_lba.to_le_bytes());
    header[40..48].copy_from_slice(&first_usable.to_le_bytes());
    header[48..56].copy_from_slice(&last_usable.to_le_bytes());
    let mut disk_guid = [0u8; 16];
    fill_guid_bytes(&mut disk_guid);
    header[56..72].copy_from_slice(&disk_guid);
    header[72..80].copy_from_slice(&GPT_DEFAULT_TABLE_LBA.to_le_bytes());
    header[80..84].copy_from_slice(&entry_count.to_le_bytes());
    header[84..88].copy_from_slice(&entry_size.to_le_bytes());
    header[88..92].copy_from_slice(&entries_crc.to_le_bytes());

    // CRC over first header_size bytes with crc field zero.
    header[16..20].fill(0);
    let header_crc = crc32_ieee(&header[..GPT_MIN_HEADER_SIZE as usize]);
    header[16..20].copy_from_slice(&header_crc.to_le_bytes());

    // Write primary table
    for i in 0..table_lbas as usize {
        let lba = GPT_DEFAULT_TABLE_LBA + i as u64;
        let start = i * 512;
        let end = start + 512;
        write_blocks_aligned_with_log(device, lba, &entries[start..end], log).await?;
    }
    write_blocks_aligned_with_log(device, GPT_HEADER_LBA, &header, log).await?;

    // Write backup table + backup header
    let backup_entries_lba = last_lba.saturating_sub(table_lbas);
    for i in 0..table_lbas as usize {
        let lba = backup_entries_lba + i as u64;
        let start = i * 512;
        let end = start + 512;
        write_blocks_aligned_with_log(device, lba, &entries[start..end], log).await?;
    }

    let mut backup_header = header;
    backup_header[24..32].copy_from_slice(&last_lba.to_le_bytes());
    backup_header[32..40].copy_from_slice(&GPT_HEADER_LBA.to_le_bytes());
    backup_header[72..80].copy_from_slice(&backup_entries_lba.to_le_bytes());
    backup_header[16..20].fill(0);
    let backup_crc = crc32_ieee(&backup_header[..GPT_MIN_HEADER_SIZE as usize]);
    backup_header[16..20].copy_from_slice(&backup_crc.to_le_bytes());
    write_blocks_aligned_with_log(device, last_lba, &backup_header, log).await?;

    device.flush().await?;

    Ok(ranges)
}

/// Create a fresh GPT partition table with:
/// - Partition 1: ESP (FAT32) for UEFI boot (Limine BOOTX64.EFI, config)
/// - Partition 2: TRUEOS data partition (TRUEOSFS superblock at start)
///
/// Returns the computed on-disk LBA ranges (absolute, on the parent disk).
pub async fn write_trueos_bootable_gpt_layout_with_log(
    device: DeviceHandle,
    esp_size_mib: u64,
    log: &mut dyn FnMut(&str),
) -> Result<TrueosBootLayout> {
    if device.parent().is_some() {
        return Err(Error::InvalidParam);
    }
    if !device.supports_write() {
        return Err(Error::NotSupported);
    }

    // FAT32 practical minimum: 65_536 clusters at 512B sectors is a decent floor.
    let esp_blocks = mib_to_blocks_512(esp_size_mib);
    if esp_blocks < 65_536 {
        return Err(Error::InvalidParam);
    }

    let parts = [
        GptPartitionSpec {
            type_guid: GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES,
            name: "TRUEOS ESP",
            size: PartitionSize::Mib(esp_size_mib),
            attributes: 0,
        },
        GptPartitionSpec {
            type_guid: GPT_TYPE_LINUX_FILESYSTEM_BYTES,
            name: "TRUEOS",
            size: PartitionSize::Remaining,
            attributes: 0,
        },
    ];

    let ranges = write_gpt_layout_with_log(device, &parts, log).await?;
    if ranges.len() != 2 {
        return Err(Error::Corrupted);
    }

    Ok(TrueosBootLayout {
        esp: ranges[0],
        trueos: ranges[1],
    })
}
