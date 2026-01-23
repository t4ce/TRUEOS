use fatfs::{format_volume, FileSystem, FormatVolumeOptions, FsOptions, IoBase, Read, Seek, SeekFrom, Write};
use core::fmt;

use crate::disc::block;

mod payload {
    include!(concat!(env!("OUT_DIR"), "/install_payload.rs"));
}

fn decode_xor(src: &[u8], key: u8) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(src.len());
    out.extend(src.iter().map(|b| b ^ key));
    out
}

pub fn install_bios_mbr(handle: block::DeviceHandle) -> Result<(), InstallError> {
    install_bios_mbr_with_progress(handle, || {})
}

pub fn install_bios_mbr_with_progress<F>(handle: block::DeviceHandle, mut progress_tick: F) -> Result<(), InstallError>
where
    F: FnMut(),
{
    install_bios_mbr_impl(handle, &mut progress_tick, &mut |_| {}, InstallMode::Fresh)
}

pub fn install_bios_mbr_with_progress_and_status<F, S>(
    handle: block::DeviceHandle,
    mut progress_tick: F,
    mut status: S,
) -> Result<(), InstallError>
where
    F: FnMut(),
    S: FnMut(fmt::Arguments<'_>),
{
    install_bios_mbr_impl(handle, &mut progress_tick, &mut status, InstallMode::Fresh)
}

pub fn install_bios_mbr_migrate_superfloppy(handle: block::DeviceHandle) -> Result<(), InstallError> {
    install_bios_mbr_migrate_superfloppy_with_progress(handle, || {})
}

pub fn install_bios_mbr_migrate_superfloppy_with_progress<F>(handle: block::DeviceHandle, mut progress_tick: F) -> Result<(), InstallError>
where
    F: FnMut(),
{
    install_bios_mbr_impl(handle, &mut progress_tick, &mut |_| {}, InstallMode::MigrateInPlace)
}

pub fn install_bios_mbr_migrate_superfloppy_with_progress_and_status<F, S>(
    handle: block::DeviceHandle,
    mut progress_tick: F,
    mut status: S,
) -> Result<(), InstallError>
where
    F: FnMut(),
    S: FnMut(fmt::Arguments<'_>),
{
    install_bios_mbr_impl(handle, &mut progress_tick, &mut status, InstallMode::MigrateInPlace)
}

#[derive(Debug)]
pub enum InstallError {
    NotWritable,
    UnsupportedBlockSize(u32),
    MediaTooSmall,
    NotSuperfloppy,
    TailNotFree,
    MissingExecutableFile,
    Block(block::Error),
    Fat(fatfs::Error<block::Error>),
}

impl From<block::Error> for InstallError {
    fn from(e: block::Error) -> Self {
        InstallError::Block(e)
    }
}

impl From<fatfs::Error<block::Error>> for InstallError {
    fn from(e: fatfs::Error<block::Error>) -> Self {
        InstallError::Fat(e)
    }
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

    fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { alloc::alloc::dealloc(self.ptr, self.layout) };
        }
    }
}

struct SliceBlockIo {
    dev: block::DeviceHandle,
    base_lba: u64,
    blocks: u64,
    pos: u64,
    block_size: usize,
    scratch: AlignedBuf,
}

impl SliceBlockIo {
    fn new(dev: block::DeviceHandle, base_lba: u64, blocks: u64) -> Result<Self, InstallError> {
        let info = dev.info();
        let block_size = info.block_size as usize;
        if info.block_size != 512 {
            return Err(InstallError::UnsupportedBlockSize(info.block_size));
        }
        let align = info.dma_alignment.max(1) as usize;
        let mut scratch = AlignedBuf::new(block_size, align).ok_or(block::Error::DmaUnavailable)?;
        scratch.as_mut_slice().fill(0);
        Ok(Self {
            dev,
            base_lba,
            blocks,
            pos: 0,
            block_size,
            scratch,
        })
    }

    fn cap_bytes(&self) -> u64 {
        self.blocks.saturating_mul(self.block_size as u64)
    }

    fn read_block(&mut self, lba: u64) -> Result<(), block::Error> {
        self.dev.read_blocks(self.base_lba + lba, self.scratch.as_mut_slice())
    }
}

impl IoBase for SliceBlockIo {
    type Error = block::Error;
}

impl Read for SliceBlockIo {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut total = 0usize;
        while total < buf.len() {
            let lba = self.pos / (self.block_size as u64);
            let block_off = (self.pos % (self.block_size as u64)) as usize;

            self.read_block(lba)?;

            let avail = self.block_size - block_off;
            let want = core::cmp::min(avail, buf.len() - total);
            buf[total..total + want]
                .copy_from_slice(&self.scratch.as_mut_slice()[block_off..block_off + want]);

            total += want;
            self.pos = self.pos.wrapping_add(want as u64);
        }

        Ok(total)
    }
}

impl Write for SliceBlockIo {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.dev.supports_write() {
            return Err(block::Error::NotSupported);
        }

        let mut total = 0usize;
        while total < buf.len() {
            let lba = self.pos / (self.block_size as u64);
            let block_off = (self.pos % (self.block_size as u64)) as usize;
            let avail = self.block_size - block_off;
            let want = core::cmp::min(avail, buf.len() - total);

            if want != self.block_size {
                self.read_block(lba)?;
            }

            self.scratch.as_mut_slice()[block_off..block_off + want]
                .copy_from_slice(&buf[total..total + want]);
            self.dev.write_blocks(self.base_lba + lba, self.scratch.as_mut_slice())?;

            total += want;
            self.pos = self.pos.wrapping_add(want as u64);
        }

        Ok(total)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.dev.flush()
    }
}

impl Seek for SliceBlockIo {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let cap = self.cap_bytes() as i64;
        let next = match pos {
            SeekFrom::Start(off) => {
                if off > self.cap_bytes() {
                    return Err(block::Error::OutOfBounds);
                }
                off as i64
            }
            SeekFrom::End(delta) => cap.checked_add(delta).ok_or(block::Error::OutOfBounds)?,
            SeekFrom::Current(delta) => (self.pos as i64)
                .checked_add(delta)
                .ok_or(block::Error::OutOfBounds)?,
        };

        if next < 0 {
            return Err(block::Error::OutOfBounds);
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    (value + (align - 1)) / align * align
}

fn write_mbr_partition_entry(mbr: &mut [u8; 512], start_lba: u32, sectors: u32) {
    // One partition, FAT32 LBA, bootable.
    let p = 0x1be;
    mbr[p + 0] = 0x80; // active
    mbr[p + 1] = 0xFE;
    mbr[p + 2] = 0xFF;
    mbr[p + 3] = 0xFF;
    mbr[p + 4] = 0x0C; // FAT32 LBA
    mbr[p + 5] = 0xFE;
    mbr[p + 6] = 0xFF;
    mbr[p + 7] = 0xFF;
    mbr[p + 8..p + 12].copy_from_slice(&start_lba.to_le_bytes());
    mbr[p + 12..p + 16].copy_from_slice(&sectors.to_le_bytes());

    // Clear remaining entries.
    for i in 1..4 {
        let off = 0x1be + i * 16;
        mbr[off..off + 16].fill(0);
    }

    mbr[510] = 0x55;
    mbr[511] = 0xAA;
}

#[derive(Clone, Copy, Debug)]
enum InstallMode {
    Fresh,
    /// Convert an existing FAT superfloppy at LBA0 into a partitioned disk by
    /// shifting the volume forward by 2048 sectors, validated so the last 2048
    /// sectors contain no allocated clusters.
    MigrateInPlace,
}

const MIGRATE_SHIFT_LBA: u64 = 2048;

#[derive(Clone, Copy, Debug)]
enum FatKind {
    Fat16,
    Fat32,
}

#[derive(Clone, Copy, Debug)]
struct Bpb {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    fats: u8,
    root_entry_count: u16,
    total_sectors: u32,
    sectors_per_fat: u32,
    fat_kind: FatKind,
    fs_info_sector: u16,
    backup_boot_sector: u16,
}

fn read_u16_le(bs: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bs[off], bs[off + 1]])
}

fn read_u32_le(bs: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bs[off], bs[off + 1], bs[off + 2], bs[off + 3]])
}

fn parse_bpb(bs: &[u8; 512]) -> Result<Bpb, InstallError> {
    if bs[510] != 0x55 || bs[511] != 0xAA {
        return Err(InstallError::NotSuperfloppy);
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

    if bytes_per_sector != 512 || sectors_per_cluster == 0 || fats == 0 || total_sectors == 0 || sectors_per_fat == 0 {
        return Err(InstallError::NotSuperfloppy);
    }

    // Compute FAT type based on cluster count.
    let root_dir_sectors = ((root_entry_count as u32 * 32) + (bytes_per_sector as u32 - 1)) / (bytes_per_sector as u32);
    let first_data_sector = (reserved_sectors as u32) + (fats as u32) * sectors_per_fat + root_dir_sectors;
    if first_data_sector >= total_sectors {
        return Err(InstallError::NotSuperfloppy);
    }
    let data_sectors = total_sectors - first_data_sector;
    let cluster_count = data_sectors / (sectors_per_cluster as u32);
    let fat_kind = if cluster_count < 65525 { FatKind::Fat16 } else { FatKind::Fat32 };

    let fs_info_sector = if matches!(fat_kind, FatKind::Fat32) {
        read_u16_le(bs, 48)
    } else {
        0
    };
    let backup_boot_sector = if matches!(fat_kind, FatKind::Fat32) {
        read_u16_le(bs, 50)
    } else {
        0
    };

    Ok(Bpb {
        bytes_per_sector,
        sectors_per_cluster,
        reserved_sectors,
        fats,
        root_entry_count,
        total_sectors,
        sectors_per_fat,
        fat_kind,
        fs_info_sector,
        backup_boot_sector,
    })
}

struct FatCache {
    handle: block::DeviceHandle,
    fat_start_lba: u64,
    align: usize,
    cached_lba: u64,
    cached_valid: bool,
    buf: AlignedBuf,
}

impl FatCache {
    fn new(handle: block::DeviceHandle, fat_start_lba: u64, align: usize) -> Result<Self, InstallError> {
        let buf = AlignedBuf::new(512, align).ok_or(block::Error::DmaUnavailable)?;
        Ok(Self {
            handle,
            fat_start_lba,
            align,
            cached_lba: 0,
            cached_valid: false,
            buf,
        })
    }

    fn load_sector(&mut self, lba: u64) -> Result<(), InstallError> {
        if self.cached_valid && self.cached_lba == lba {
            return Ok(());
        }
        self.handle.read_blocks(self.fat_start_lba + lba, self.buf.as_mut_slice())?;
        self.cached_lba = lba;
        self.cached_valid = true;
        Ok(())
    }

    fn fat16_entry(&mut self, cluster: u32) -> Result<u16, InstallError> {
        let byte_off = (cluster as u64) * 2;
        let sec = byte_off / 512;
        let off = (byte_off % 512) as usize;
        self.load_sector(sec)?;
        Ok(read_u16_le(self.buf.as_slice(), off))
    }

    fn fat32_entry(&mut self, cluster: u32) -> Result<u32, InstallError> {
        let byte_off = (cluster as u64) * 4;
        let sec = byte_off / 512;
        let off = (byte_off % 512) as usize;
        self.load_sector(sec)?;
        Ok(read_u32_le(self.buf.as_slice(), off) & 0x0FFF_FFFF)
    }
}

fn validate_tail_free<F>(handle: block::DeviceHandle, bpb: &Bpb, shift_lba: u64, progress_tick: &mut F) -> Result<(), InstallError>
where
    F: FnMut(),
{
    let info = handle.info();
    if bpb.total_sectors as u64 != info.block_count {
        // Superfloppy that doesn't span the whole disk image is not supported by this in-place shifter.
        return Err(InstallError::NotSuperfloppy);
    }
    if (bpb.total_sectors as u64) <= shift_lba {
        return Err(InstallError::MediaTooSmall);
    }

    let new_total_sectors = (bpb.total_sectors as u64) - shift_lba;

    let root_dir_sectors = ((bpb.root_entry_count as u64 * 32) + (bpb.bytes_per_sector as u64 - 1)) / (bpb.bytes_per_sector as u64);
    let first_data_sector = (bpb.reserved_sectors as u64) + (bpb.fats as u64) * (bpb.sectors_per_fat as u64) + root_dir_sectors;
    if new_total_sectors <= first_data_sector {
        // After shifting by 1MiB, the resulting volume would not have a valid data region.
        return Err(InstallError::MediaTooSmall);
    }
    let data_sectors_old = (bpb.total_sectors as u64) - first_data_sector;
    let data_sectors_new = new_total_sectors - first_data_sector;

    let spc = bpb.sectors_per_cluster as u64;
    let clusters_old = data_sectors_old / spc;
    let clusters_new = data_sectors_new / spc;

    if clusters_new >= clusters_old {
        return Ok(());
    }

    let first_removed_cluster = 2u32 + (clusters_new as u32);
    let last_cluster = 2u32 + (clusters_old as u32) - 1;

    let fat_start_lba = bpb.reserved_sectors as u64;
    let align = info.dma_alignment.max(1) as usize;
    let mut fat = FatCache::new(handle, fat_start_lba, align)?;

    let mut checked = 0u32;
    for cl in first_removed_cluster..=last_cluster {
        let used = match bpb.fat_kind {
            FatKind::Fat16 => fat.fat16_entry(cl)? != 0,
            FatKind::Fat32 => fat.fat32_entry(cl)? != 0,
        };
        if used {
            return Err(InstallError::TailNotFree);
        }
        checked = checked.wrapping_add(1);
        if (checked & 0x0FFF) == 0 {
            (progress_tick)();
        }
    }

    Ok(())
}

fn compute_used_end_lba_exclusive<F>(handle: block::DeviceHandle, bpb: &Bpb, progress_tick: &mut F) -> Result<u64, InstallError>
where
    F: FnMut(),
{
    let info = handle.info();
    if bpb.total_sectors as u64 != info.block_count {
        return Err(InstallError::NotSuperfloppy);
    }

    let root_dir_sectors =
        ((bpb.root_entry_count as u64 * 32) + (bpb.bytes_per_sector as u64 - 1)) / (bpb.bytes_per_sector as u64);
    let first_data_sector =
        (bpb.reserved_sectors as u64) + (bpb.fats as u64) * (bpb.sectors_per_fat as u64) + root_dir_sectors;
    if first_data_sector >= (bpb.total_sectors as u64) {
        return Err(InstallError::NotSuperfloppy);
    }

    let spc = bpb.sectors_per_cluster as u64;
    if spc == 0 {
        return Err(InstallError::NotSuperfloppy);
    }

    let data_sectors = (bpb.total_sectors as u64) - first_data_sector;
    let clusters = data_sectors / spc;
    if clusters == 0 {
        // Metadata-only volume.
        return Ok(first_data_sector);
    }

    let last_cluster = 2u32 + (clusters as u32) - 1;
    let fat_start_lba = bpb.reserved_sectors as u64;
    let align = info.dma_alignment.max(1) as usize;
    let mut fat = FatCache::new(handle, fat_start_lba, align)?;

    let mut max_used: u32 = 0;
    let mut checked: u32 = 0;
    for cl in 2u32..=last_cluster {
        let used = match bpb.fat_kind {
            FatKind::Fat16 => fat.fat16_entry(cl)? != 0,
            FatKind::Fat32 => fat.fat32_entry(cl)? != 0,
        };
        if used {
            max_used = cl;
        }
        checked = checked.wrapping_add(1);
        if (checked & 0x3FFF) == 0 {
            (progress_tick)();
        }
    }

    if max_used < 2 {
        return Ok(first_data_sector);
    }
    let rel = (max_used - 2) as u64;
    let end = first_data_sector + (rel + 1) * spc;
    Ok(core::cmp::min(end, bpb.total_sectors as u64))
}

fn shift_volume_forward_in_place<F>(
    handle: block::DeviceHandle,
    shift_lba: u64,
    src_blocks: u64,
    progress_tick: &mut F,
) -> Result<(), InstallError>
where
    F: FnMut(),
{
    let info = handle.info();
    let align = info.dma_alignment.max(1) as usize;

    if src_blocks == 0 {
        (progress_tick)();
        return Ok(());
    }
    if src_blocks > info.block_count {
        return Err(InstallError::MediaTooSmall);
    }
    if src_blocks > info.block_count.saturating_sub(shift_lba) {
        return Err(InstallError::MediaTooSmall);
    }

    let max_bytes = info.max_transfer_bytes.max(4096) as usize;
    let mut chunk_bytes = (256 * 1024).min(max_bytes);
    chunk_bytes = (chunk_bytes / 512) * 512;
    if chunk_bytes == 0 {
        chunk_bytes = 512;
    }

    let mut buf = AlignedBuf::new(chunk_bytes, align).ok_or(block::Error::DmaUnavailable)?;

    let chunk_blocks = (chunk_bytes / 512) as u64;

    let mut remaining = src_blocks;
    while remaining > 0 {
        let blocks = core::cmp::min(chunk_blocks, remaining);
        let src_lba = remaining - blocks;
        let dst_lba = src_lba + shift_lba;
        let bytes = (blocks as usize) * 512;

        handle.read_blocks(src_lba, &mut buf.as_mut_slice()[..bytes])?;
        handle.write_blocks(dst_lba, &buf.as_slice()[..bytes])?;

        remaining = src_lba;
        if (remaining & 0x3FFF) == 0 {
            (progress_tick)();
        }
    }

    handle.flush()?;
    (progress_tick)();
    Ok(())
}

fn patch_fat32_fsinfo_unknown(handle: block::DeviceHandle, lba: u64, align: usize) -> Result<(), InstallError> {
    let mut s = AlignedBuf::new(512, align).ok_or(block::Error::DmaUnavailable)?;
    handle.read_blocks(lba, s.as_mut_slice())?;

    // FSInfo signatures.
    if read_u32_le(s.as_slice(), 0) != 0x4161_5252 || read_u32_le(s.as_slice(), 484) != 0x6141_7272 {
        return Ok(());
    }

    // Mark free count and next free as unknown.
    s.as_mut_slice()[488..492].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    s.as_mut_slice()[492..496].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    handle.write_blocks(lba, s.as_slice())?;
    Ok(())
}

fn patch_boot_sector_sizes(handle: block::DeviceHandle, boot_lba: u64, bpb: &Bpb, new_total_sectors: u32, hidden_sectors: u32, align: usize) -> Result<(), InstallError> {
    let mut bs = AlignedBuf::new(512, align).ok_or(block::Error::DmaUnavailable)?;
    handle.read_blocks(boot_lba, bs.as_mut_slice())?;
    if bs.as_slice()[510] != 0x55 || bs.as_slice()[511] != 0xAA {
        return Err(InstallError::NotSuperfloppy);
    }

    // BPB_HiddSec
    bs.as_mut_slice()[28..32].copy_from_slice(&hidden_sectors.to_le_bytes());

    // BPB_TotSec16 / BPB_TotSec32
    let tot16 = read_u16_le(bs.as_slice(), 19);
    if tot16 != 0 {
        bs.as_mut_slice()[19..21].copy_from_slice(&(new_total_sectors as u16).to_le_bytes());
    } else {
        bs.as_mut_slice()[32..36].copy_from_slice(&new_total_sectors.to_le_bytes());
    }

    handle.write_blocks(boot_lba, bs.as_slice())?;
    Ok(())
}

fn write_boot_files<F>(handle: block::DeviceHandle, part_start_lba: u64, part_sectors: u64, progress_tick: &mut F) -> Result<(), InstallError>
where
    F: FnMut(),
{
    let mut part_io = SliceBlockIo::new(handle, part_start_lba, part_sectors)?;
    let fs = FileSystem::new(part_io, FsOptions::new())?;
    {
        let root = fs.root_dir();

        // limine-bios.sys
        {
            let bios_sys = decode_xor(payload::LIMINE_BIOS_SYS_XOR, payload::INSTALL_XOR_KEY);
            let mut f = root.create_file("limine-bios.sys")?;
            f.seek(SeekFrom::Start(0))?;
            f.truncate()?;
            f.write_all(&bios_sys)?;
            f.flush()?;
        }
        (progress_tick)();

        // limine.conf
        {
            let mut f = root.create_file("limine.conf")?;
            f.seek(SeekFrom::Start(0))?;
            f.truncate()?;
            f.write_all(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/limine.conf")))?;
            f.flush()?;
        }
        (progress_tick)();

        // Kernel image: copy the executable file bytes Limine provided (the file we booted).
        let exe = crate::limine::executable_file_bytes().ok_or(InstallError::MissingExecutableFile)?;
        {
            let mut f = root.create_file("TRUEOS.elf")?;
            f.seek(SeekFrom::Start(0))?;
            f.truncate()?;
            // Write in chunks to keep things responsive.
            for (i, chunk) in exe.chunks(64 * 1024).enumerate() {
                f.write_all(chunk)?;

                // Spin every ~256KiB.
                if (i & 3) == 0 {
                    (progress_tick)();
                }
            }
            f.flush()?;
        }
    }

    fs.unmount()?;
    handle.flush()?;
    (progress_tick)();
    Ok(())
}

fn install_bios_mbr_impl<F, S>(
    handle: block::DeviceHandle,
    progress_tick: &mut F,
    status: &mut S,
    mode: InstallMode,
) -> Result<(), InstallError>
where
    F: FnMut(),
    S: FnMut(fmt::Arguments<'_>),
{
    let info = handle.info();

    if !handle.supports_write() {
        return Err(InstallError::NotWritable);
    }
    if info.block_size != 512 {
        return Err(InstallError::UnsupportedBlockSize(info.block_size));
    }

    let bootloader = decode_xor(payload::LIMINE_BIOS_HDD_BIN_XOR, payload::INSTALL_XOR_KEY);
    if bootloader.len() < 512 {
        return Err(InstallError::MediaTooSmall);
    }

    // Stage2 gets written right after the MBR.
    let stage2_bytes = &bootloader[512..];
    let stage2_sectors = ((stage2_bytes.len() as u64) + 511) / 512;

    let part_start_lba = match mode {
        InstallMode::Fresh => {
            // Choose a partition start that leaves enough post-MBR gap for stage2.
            let min_part_start = 1 + stage2_sectors;
            core::cmp::max(2048u64, align_up(min_part_start, 2048))
        }
        InstallMode::MigrateInPlace => MIGRATE_SHIFT_LBA,
    };

    if stage2_sectors >= part_start_lba {
        return Err(InstallError::MediaTooSmall);
    }

    if info.block_count <= part_start_lba + 2048 {
        return Err(InstallError::MediaTooSmall);
    }

    let part_sectors = info.block_count - part_start_lba;
    if part_sectors > (u32::MAX as u64) {
        return Err(InstallError::MediaTooSmall);
    }

    let align = info.dma_alignment.max(1) as usize;

    if matches!(mode, InstallMode::MigrateInPlace) {
        // Validate & relocate existing superfloppy FAT volume so it becomes the partition contents.
        let mut bs = [0u8; 512];
        {
            let mut tmp = AlignedBuf::new(512, align).ok_or(block::Error::DmaUnavailable)?;
            handle.read_blocks(0, tmp.as_mut_slice())?;
            bs.copy_from_slice(tmp.as_slice());
        }
        let bpb = parse_bpb(&bs)?;
        validate_tail_free(handle, &bpb, part_start_lba, progress_tick)?;
        let used_end = compute_used_end_lba_exclusive(handle, &bpb, progress_tick)?;
        let src_blocks = core::cmp::min(used_end, info.block_count.saturating_sub(part_start_lba));

        // Tell the user how much data we will actually relocate.
        // We move the prefix [0..src_blocks) forward by MIGRATE_SHIFT_LBA sectors.
        let bytes = src_blocks.saturating_mul(info.block_size as u64);
        let mib_x10 = bytes.saturating_mul(10) / (1024 * 1024);
        (status)(format_args!(
            "install: migrate: shifting {} sectors ({}.{:01} MiB) forward by {} sectors ({} MiB)",
            src_blocks,
            mib_x10 / 10,
            (mib_x10 % 10) as u8,
            MIGRATE_SHIFT_LBA,
            (MIGRATE_SHIFT_LBA * (info.block_size as u64)) / (1024 * 1024)
        ));
        shift_volume_forward_in_place(handle, part_start_lba, src_blocks, progress_tick)?;

        // Patch boot sector(s) inside the shifted volume.
        let new_total = (info.block_count - part_start_lba) as u32;
        patch_boot_sector_sizes(handle, part_start_lba, &bpb, new_total, part_start_lba as u32, align)?;
        if bpb.backup_boot_sector != 0 {
            let bk_lba = part_start_lba + (bpb.backup_boot_sector as u64);
            if bk_lba < info.block_count {
                let _ = patch_boot_sector_sizes(handle, bk_lba, &bpb, new_total, part_start_lba as u32, align);
            }
        }
        if matches!(bpb.fat_kind, FatKind::Fat32) && bpb.fs_info_sector != 0 {
            let fsinfo_lba = part_start_lba + (bpb.fs_info_sector as u64);
            if fsinfo_lba < info.block_count {
                let _ = patch_fat32_fsinfo_unknown(handle, fsinfo_lba, align);
            }
        }

        handle.flush()?;
        (progress_tick)();
    }

    // Build the MBR sector based on Limine's bootsector template.
    let mut mbr = [0u8; 512];
    mbr.copy_from_slice(&bootloader[..512]);

    // Disk signature (simple, deterministic-ish).
    let sig = (info.id.raw() as u32).wrapping_mul(0x9E3779B1).wrapping_add(0x1234_5678);
    mbr[0x1b8..0x1bc].copy_from_slice(&sig.to_le_bytes());

    // Write Limine stage2 location into the bootsector.
    let stage2_loc_bytes: u64 = 512;
    mbr[0x1a4..0x1ac].copy_from_slice(&stage2_loc_bytes.to_le_bytes());

    write_mbr_partition_entry(&mut mbr, part_start_lba as u32, part_sectors as u32);

    // Write MBR.
    let mut lba0 = AlignedBuf::new(512, align).ok_or(block::Error::DmaUnavailable)?;
    lba0.as_mut_slice().copy_from_slice(&mbr);
    handle.write_blocks(0, lba0.as_slice())?;
    (progress_tick)();

    // Write stage2 (immediately after MBR).
    let mut tmp = AlignedBuf::new(512, align).ok_or(block::Error::DmaUnavailable)?;
    for (i, chunk) in stage2_bytes.chunks(512).enumerate() {
        tmp.as_mut_slice().fill(0);
        tmp.as_mut_slice()[..chunk.len()].copy_from_slice(chunk);
        handle.write_blocks(1 + i as u64, tmp.as_slice())?;

        // Spin roughly once per 32 sectors to avoid spamming.
        if (i & 31) == 0 {
            (progress_tick)();
        }
    }

    handle.flush()?;
    (progress_tick)();

    if matches!(mode, InstallMode::Fresh) {
        // Format a fresh filesystem on the first partition.
        let mut part_io = SliceBlockIo::new(handle, part_start_lba, part_sectors)?;
        format_volume(&mut part_io, FormatVolumeOptions::new())?;
        (progress_tick)();
        // Drop part_io before re-opening it for FileSystem::new below.
    }

    // Populate (or update) the filesystem.
    write_boot_files(handle, part_start_lba, part_sectors, progress_tick)?;

    Ok(())
}
