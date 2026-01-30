use alloc::vec;

use crate::disc::block::{DeviceHandle, Error, Result};
use crate::disc::partition::{
    BlockRange, TrueosBootLayout, GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES, GPT_TYPE_LINUX_FILESYSTEM_BYTES,
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
            log(
                alloc::format!(
                    "install: gpt: write failed lba={} bytes={} err={:?}",
                    lba,
                    buf.len(),
                    e
                )
                .as_str(),
            );
            Err(e)
        }
    }
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
    let table_lbas = (table_bytes as u64 + 511) / 512;

    let first_usable = GPT_DEFAULT_TABLE_LBA + table_lbas;
    let last_usable = last_lba.saturating_sub(table_lbas).saturating_sub(1);
    if first_usable >= last_usable {
        return Err(Error::OutOfBounds);
    }

    let esp_blocks = (esp_size_mib.saturating_mul(1024 * 1024)) / 512;
    if esp_blocks < 65_536 {
        // FAT32 practical minimum.
        return Err(Error::InvalidParam);
    }

    let esp_first = align_up_u64(first_usable, GPT_ALIGN_LBA);
    let esp_last = esp_first
        .checked_add(esp_blocks)
        .ok_or(Error::OutOfBounds)?
        .saturating_sub(1);
    let trueos_first = align_up_u64(esp_last.saturating_add(1), GPT_ALIGN_LBA);
    let trueos_last = last_usable;
    if esp_first < first_usable || esp_last >= trueos_first {
        return Err(Error::OutOfBounds);
    }
    if trueos_first >= trueos_last {
        return Err(Error::OutOfBounds);
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
    let mbr_sectors = core::cmp::min((info.block_count - 1) as u64, u32::MAX as u64) as u32;
    pmbr[458..462].copy_from_slice(&mbr_sectors.to_le_bytes());
    pmbr[510..512].copy_from_slice(&GPT_PROTECTIVE_MBR_SIGNATURE.to_le_bytes());

    write_blocks_aligned_with_log(device, 0, &pmbr, log).await?;

    // Partition entry array
    let mut entries = vec![0u8; table_bytes];

    // Entry 0: ESP
    {
        let off = 0usize;
        entries[off..off + 16].copy_from_slice(&GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES);
        let mut unique = [0u8; 16];
        fill_guid_bytes(&mut unique);
        entries[off + 16..off + 32].copy_from_slice(&unique);
        entries[off + 32..off + 40].copy_from_slice(&esp_first.to_le_bytes());
        entries[off + 40..off + 48].copy_from_slice(&esp_last.to_le_bytes());
        entries[off + 48..off + 56].copy_from_slice(&0u64.to_le_bytes());
        write_utf16le_fixed(&mut entries[off + 56..off + 56 + GPT_PARTITION_NAME_BYTES], "TRUEOS ESP");
    }

    // Entry 1: TRUEOS data
    {
        let off = entry_size as usize;
        entries[off..off + 16].copy_from_slice(&GPT_TYPE_LINUX_FILESYSTEM_BYTES);
        let mut unique = [0u8; 16];
        fill_guid_bytes(&mut unique);
        entries[off + 16..off + 32].copy_from_slice(&unique);
        entries[off + 32..off + 40].copy_from_slice(&trueos_first.to_le_bytes());
        entries[off + 40..off + 48].copy_from_slice(&trueos_last.to_le_bytes());
        entries[off + 48..off + 56].copy_from_slice(&0u64.to_le_bytes());
        write_utf16le_fixed(&mut entries[off + 56..off + 56 + GPT_PARTITION_NAME_BYTES], "TRUEOS");
    }

    let entries_crc = crc32_ieee(&entries);

    // GPT header (primary)
    let mut header = [0u8; 512];
    header[..8].copy_from_slice(GPT_SIGNATURE);
    header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    header[12..16].copy_from_slice(&(GPT_MIN_HEADER_SIZE as u32).to_le_bytes());
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

    Ok(TrueosBootLayout {
        esp: BlockRange::from_bounds(esp_first, esp_last)?,
        trueos: BlockRange::from_bounds(trueos_first, trueos_last)?,
    })
}
