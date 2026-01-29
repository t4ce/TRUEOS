use alloc::{string::String, vec, vec::Vec};
use core::{char::decode_utf16, cmp, convert::TryInto};

use crate::disc::block::{
    self, BlockDevice, DeviceDescriptor, DeviceHandle, DeviceKind, DiscId, Error, Result,
};

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const GPT_HEADER_LBA: u64 = 1;
const GPT_MIN_HEADER_SIZE: u32 = 92;
const GPT_MIN_ENTRY_SIZE: u32 = 128;
const GPT_MAX_ENTRY_SIZE: u32 = 512;
const GPT_MAX_ENTRIES: u32 = 256;
const GPT_PARTITION_NAME_BYTES: usize = 72;
const GPT_MAX_TABLE_BYTES: usize = (GPT_MAX_ENTRY_SIZE as usize) * (GPT_MAX_ENTRIES as usize);

// Standard EFI System Partition type GUID.
// C12A7328-F81F-11D2-BA4B-00A0C93EC93B
pub const GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

// Linux filesystem data partition (widely accepted "basic data" analogue).
// 0FC63DAF-8483-4772-8E79-3D69D8477DE4
pub const GPT_TYPE_LINUX_FILESYSTEM_BYTES: [u8; 16] = [
    0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4,
];

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

#[derive(Clone, Copy, Debug)]
pub struct TrueosBootLayout {
    pub esp: BlockRange,
    pub trueos: BlockRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Guid {
    bytes: [u8; 16],
}

impl Guid {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.bytes
    }
}

impl core::fmt::Display for Guid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let b = &self.bytes;
        write!(
            f,
            "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            b[3],
            b[2],
            b[1],
            b[0],
            b[5],
            b[4],
            b[7],
            b[6],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15]
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockRange {
    start_lba: u64,
    blocks: u64,
}

impl BlockRange {
    pub fn from_bounds(first_lba: u64, last_lba_inclusive: u64) -> Result<Self> {
        if last_lba_inclusive < first_lba {
            return Err(Error::InvalidParam);
        }
        let blocks = last_lba_inclusive - first_lba + 1;
        Ok(Self {
            start_lba: first_lba,
            blocks,
        })
    }

    pub fn block_count(&self) -> u64 {
        self.blocks
    }

    pub fn first_lba(&self) -> u64 {
        self.start_lba
    }

    pub fn last_lba(&self) -> u64 {
        self.start_lba + self.blocks - 1
    }

    pub fn translate(&self, relative_lba: u64, blocks: u64) -> Result<u64> {
        if blocks == 0 {
            if relative_lba > self.blocks {
                return Err(Error::OutOfBounds);
            }
            return Ok(self.start_lba + relative_lba);
        }

        if relative_lba >= self.blocks {
            return Err(Error::OutOfBounds);
        }

        let end = relative_lba.checked_add(blocks).ok_or(Error::OutOfBounds)?;
        if end > self.blocks {
            return Err(Error::OutOfBounds);
        }

        Ok(self.start_lba + relative_lba)
    }
}

#[derive(Clone, Debug)]
pub struct PartitionInfo {
    pub index: u32,
    pub type_guid: Guid,
    pub unique_guid: Guid,
    pub range: BlockRange,
    pub attributes: u64,
    pub name: Option<String>,
}

impl PartitionInfo {
    pub fn block_count(&self) -> u64 {
        self.range.block_count()
    }

    pub fn first_lba(&self) -> u64 {
        self.range.first_lba()
    }

    pub fn last_lba(&self) -> u64 {
        self.range.last_lba()
    }
}

#[derive(Clone, Debug)]
pub struct RegisteredPartition {
    pub id: DiscId,
    pub info: PartitionInfo,
}

pub struct PartitionBlockDevice {
    parent: DeviceHandle,
    range: BlockRange,
    block_size: u32,
    max_transfer_bytes: u64,
    dma_alignment: u32,
    writable: bool,
}

impl PartitionBlockDevice {
    pub fn new(parent: DeviceHandle, range: BlockRange) -> Self {
        let parent_info = parent.info();
        Self {
            parent,
            range,
            block_size: parent_info.block_size,
            max_transfer_bytes: parent_info.max_transfer_bytes,
            dma_alignment: parent_info.dma_alignment,
            writable: parent_info.writable,
        }
    }
}

impl BlockDevice for PartitionBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.range.block_count()
    }

    fn read_blocks(&mut self, lba: u64, buf: &mut [u8]) -> Result<()> {
        let blocks = blocks_from_len(buf.len(), self.block_size)?;
        let translated = self.range.translate(lba, blocks)?;
        self.parent.read_blocks(translated, buf)
    }

    fn write_blocks(&mut self, lba: u64, buf: &[u8]) -> Result<()> {
        if !self.writable {
            return Err(Error::NotSupported);
        }
        let blocks = blocks_from_len(buf.len(), self.block_size)?;
        let translated = self.range.translate(lba, blocks)?;
        self.parent.write_blocks(translated, buf)
    }

    fn dma_alignment_bytes(&self) -> u32 {
        self.dma_alignment
    }

    fn max_transfer_bytes(&self) -> u64 {
        self.max_transfer_bytes
    }

    fn supports_write(&self) -> bool {
        self.writable
    }
}

pub fn read_gpt_partitions(device: DeviceHandle) -> Result<Vec<PartitionInfo>> {
    let device_info = device.info();
    let block_size = device_info.block_size as usize;
    if block_size < GPT_MIN_HEADER_SIZE as usize {
        return Err(Error::Corrupted);
    }

    let mut header_buf = vec![0u8; block_size];
    device.read_blocks(GPT_HEADER_LBA, &mut header_buf)?;
    if &header_buf[..GPT_SIGNATURE.len()] != GPT_SIGNATURE {
        return Err(Error::Corrupted);
    }

    let header_size = u32::from_le_bytes(header_buf[12..16].try_into().unwrap());
    if header_size < GPT_MIN_HEADER_SIZE || header_size as usize > block_size {
        return Err(Error::Corrupted);
    }

    let entries_lba = u64::from_le_bytes(header_buf[72..80].try_into().unwrap());
    let entry_count = u32::from_le_bytes(header_buf[80..84].try_into().unwrap());
    let entry_size = u32::from_le_bytes(header_buf[84..88].try_into().unwrap());
    if entry_size < GPT_MIN_ENTRY_SIZE || entry_size > GPT_MAX_ENTRY_SIZE || (entry_size % 8) != 0 {
        return Err(Error::Corrupted);
    }
    if entry_count == 0 {
        return Ok(Vec::new());
    }
    if entry_count > GPT_MAX_ENTRIES {
        return Err(Error::Corrupted);
    }

    let first_usable = u64::from_le_bytes(header_buf[40..48].try_into().unwrap());
    let last_usable = u64::from_le_bytes(header_buf[48..56].try_into().unwrap());
    let table_bytes = (entry_size as usize) * (entry_count as usize);
    if table_bytes > GPT_MAX_TABLE_BYTES {
        return Err(Error::Corrupted);
    }

    let mut table = vec![0u8; align_up(table_bytes, block_size)];
    let blocks_to_read = table.len() / block_size;
    let table_span = entries_lba
        .checked_add(blocks_to_read as u64)
        .ok_or(Error::Corrupted)?;
    if entries_lba < 2 || table_span > device_info.block_count {
        return Err(Error::Corrupted);
    }

    for i in 0..blocks_to_read {
        let lba = entries_lba + i as u64;
        let slice = &mut table[i * block_size..(i + 1) * block_size];
        device.read_blocks(lba, slice)?;
    }

    let mut partitions = Vec::new();
    for idx in 0..entry_count as usize {
        let offset = idx * entry_size as usize;
        let entry = &table[offset..offset + entry_size as usize];
        if entry[..16].iter().all(|b| *b == 0) {
            continue;
        }

        let type_guid = Guid::from_bytes(entry[0..16].try_into().unwrap());
        let unique_guid = Guid::from_bytes(entry[16..32].try_into().unwrap());
        let first_lba = u64::from_le_bytes(entry[32..40].try_into().unwrap());
        let last_lba = u64::from_le_bytes(entry[40..48].try_into().unwrap());
        if first_lba == 0 || last_lba < first_lba {
            continue;
        }
        if first_lba < first_usable || last_lba > last_usable {
            continue;
        }
        if last_lba >= device_info.block_count {
            continue;
        }

        let range = BlockRange::from_bounds(first_lba, last_lba)?;
        let attrs = u64::from_le_bytes(entry[48..56].try_into().unwrap());
        let name_end = cmp::min(entry.len(), 56 + GPT_PARTITION_NAME_BYTES);
        let name = decode_name(&entry[56..name_end]);

        partitions.push(PartitionInfo {
            index: idx as u32,
            type_guid,
            unique_guid,
            range,
            attributes: attrs,
            name,
        });
    }

    Ok(partitions)
}

pub fn register_gpt_partitions(device: DeviceHandle) -> Result<Vec<RegisteredPartition>> {
    let parent_info = device.info();
    let partitions = read_gpt_partitions(device)?;
    let mut registered = Vec::with_capacity(partitions.len());

    for part in partitions {
        let mut descriptor = DeviceDescriptor::new(DeviceKind::Partition).with_parent(device.id());
        descriptor.label = part.name.clone();
        descriptor.pci = parent_info.pci;
        if !parent_info.writable {
            descriptor = descriptor.mark_read_only();
        }

        let child = PartitionBlockDevice::new(device, part.range);
        let handle = block::register_device(descriptor, child);
        registered.push(RegisteredPartition {
            id: handle.id(),
            info: part,
        });
    }

    Ok(registered)
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

fn write_blocks_aligned(device: DeviceHandle, lba: u64, buf: &[u8]) -> Result<()> {
    let info = device.info();
    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(buf.len(), align).ok_or(Error::DmaUnavailable)?;
    tmp.as_mut_slice().copy_from_slice(buf);
    device.write_blocks(lba, tmp.as_mut_slice())
}

/// Create a fresh GPT partition table with:
/// - Partition 1: ESP (FAT32) for UEFI boot (Limine BOOTX64.EFI, config, payload)
/// - Partition 2: TRUEOS data partition (TRUEOSFS superblock at start)
///
/// Returns the computed on-disk LBA ranges (absolute, on the parent disk).
pub fn write_trueos_bootable_gpt_layout(
    device: DeviceHandle,
    esp_size_mib: u64,
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

    write_blocks_aligned(device, 0, &pmbr)?;

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
        write_blocks_aligned(device, lba, &entries[start..end])?;
    }
    write_blocks_aligned(device, GPT_HEADER_LBA, &header)?;

    // Write backup table + backup header
    let backup_entries_lba = last_lba.saturating_sub(table_lbas);
    for i in 0..table_lbas as usize {
        let lba = backup_entries_lba + i as u64;
        let start = i * 512;
        let end = start + 512;
        write_blocks_aligned(device, lba, &entries[start..end])?;
    }

    let mut backup_header = header;
    backup_header[24..32].copy_from_slice(&last_lba.to_le_bytes());
    backup_header[32..40].copy_from_slice(&GPT_HEADER_LBA.to_le_bytes());
    backup_header[72..80].copy_from_slice(&backup_entries_lba.to_le_bytes());
    backup_header[16..20].fill(0);
    let backup_crc = crc32_ieee(&backup_header[..GPT_MIN_HEADER_SIZE as usize]);
    backup_header[16..20].copy_from_slice(&backup_crc.to_le_bytes());
    write_blocks_aligned(device, last_lba, &backup_header)?;

    device.flush()?;

    Ok(TrueosBootLayout {
        esp: BlockRange::from_bounds(esp_first, esp_last)?,
        trueos: BlockRange::from_bounds(trueos_first, trueos_last)?,
    })
}

fn decode_name(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }

    let words = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|w| *w != 0);

    let mut out = String::new();
    for decoded in decode_utf16(words) {
        match decoded {
            Ok(ch) => out.push(ch),
            Err(_) => return None,
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn blocks_from_len(len: usize, block_size: u32) -> Result<u64> {
    if len == 0 {
        return Ok(0);
    }
    if len % block_size as usize != 0 {
        return Err(Error::InvalidParam);
    }
    Ok((len / block_size as usize) as u64)
}

fn align_up(value: usize, align: usize) -> usize {
    if value == 0 {
        return 0;
    }
    ((value + align - 1) / align) * align
}
