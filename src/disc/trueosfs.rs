use alloc::{boxed::Box, string::String, vec::Vec};

use crate::disc::{block, partition};
use core::hint::spin_loop;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use trueos_math::Tree;

// Standard EFI System Partition type GUID.
// C12A7328-F81F-11D2-BA4B-00A0C93EC93B
const GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrueosFsPlacement {
    pub bootable: bool,
    pub super_lba: u64,
    pub data_lba: u64,
    pub data_end_lba_exclusive: Option<u64>,
}

/// TRUEOSFS per-disk root filetree cache.
///
/// This is intentionally limited in scope: it is *not* a page cache.
/// It is meant to memoize directory/file listings once we implement them.
const TRUEOSFS_TREE_CAP: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrueosFsTreeKind {
    Root,
    Dir,
    File,
    /// Represents a nested TRUEOSFS inside the tree (future mountpoint concept).
    TrueosFs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrueosFsTreeEntry {
    pub kind: TrueosFsTreeKind,
    pub name: String,
}

pub type TrueosFsTree = Tree<TrueosFsTreeEntry, TRUEOSFS_TREE_CAP>;

struct RootMount {
    disk_id: block::DiscId,
    placement: TrueosFsPlacement,
    seq: u32,
    tree: Option<Box<TrueosFsTree>>,
}

static ROOT_SEQ: AtomicU32 = AtomicU32::new(0);
static ROOTS: Mutex<Vec<RootMount>> = Mutex::new(Vec::new());

/// Ensure a single TRUEOSFS root exists for this *whole disk*.
///
/// Returns:
/// - `Ok(Some(disk_id))` if the disk contains TRUEOSFS and is now registered
/// - `Ok(None)` if the disk does not contain TRUEOSFS
/// - `Err(_)` on I/O or invalid param
pub fn mount_root(disk: block::DeviceHandle) -> Result<Option<block::DiscId>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let Some(placement) = locate(disk)? else {
        return Ok(None);
    };

    let disk_id = disk.id();

    let mut roots = ROOTS.lock();
    if roots.iter().any(|m| m.disk_id == disk_id) {
        return Ok(Some(disk_id));
    }

    // Seed an initial tree skeleton. Directory enumeration will fill this later.
    let mut tree = TrueosFsTree::new();
    let root = match tree.add_root(TrueosFsTreeEntry {
        kind: TrueosFsTreeKind::Root,
        name: String::from("trueosfs"),
    }) {
        Some(id) => id,
        None => {
            // Capacity too small; still register without a cache.
            roots.push(RootMount {
                disk_id,
                placement,
                seq: ROOT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1),
                tree: None,
            });
            return Ok(Some(disk_id));
        }
    };
    let _ = tree.add_child(
        root,
        TrueosFsTreeEntry {
            kind: TrueosFsTreeKind::Dir,
            name: String::from("/"),
        },
    );

    roots.push(RootMount {
        disk_id,
        placement,
        seq: ROOT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1),
        tree: Some(Box::new(tree)),
    });

    Ok(Some(disk_id))
}

pub fn roots_len() -> usize {
    ROOTS.lock().len()
}

pub fn root_seq(disk_id: block::DiscId) -> Option<u32> {
    let roots = ROOTS.lock();
    roots.iter().find(|m| m.disk_id == disk_id).map(|m| m.seq)
}

pub fn with_root_tree<R>(disk_id: block::DiscId, f: impl FnOnce(u32, &TrueosFsPlacement, &TrueosFsTree) -> R) -> Option<R> {
    let roots = ROOTS.lock();
    roots
        .iter()
        .find(|m| m.disk_id == disk_id)
        .and_then(|m| m.tree.as_deref().map(|t| f(m.seq, &m.placement, t)))
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

fn looks_like_trueos_superblock(block0: &[u8]) -> bool {
    block0.len() >= 8 && &block0[0..8] == &trueos_fs::MAGIC
}

fn spin_wait_ms(ms: u64) {
    let hz = embassy_time_driver::TICK_HZ as u64;
    let start = embassy_time_driver::now();
    let delta_ticks = if hz == 0 {
        0
    } else {
        // Round up to at least one tick when ms>0.
        let ticks = (ms.saturating_mul(hz) + 999) / 1000;
        if ms > 0 { ticks.max(1) } else { 0 }
    };
    let deadline = start.saturating_add(delta_ticks);
    while embassy_time_driver::now() < deadline {
        crate::time::poll_executor();
        spin_loop();
    }
}

fn is_transient_io(e: block::Error) -> bool {
    matches!(e, block::Error::NotReady | block::Error::Timeout | block::Error::Io)
}

fn read_blocks_aligned_retry(
    handle: block::DeviceHandle,
    lba: u64,
    blocks: usize,
    attempts: u8,
) -> Result<Vec<u8>, block::Error> {
    let mut last: Option<block::Error> = None;
    let mut i = 0u8;
    while i < attempts {
        match read_blocks_aligned(handle, lba, blocks) {
            Ok(v) => return Ok(v),
            Err(e) if is_transient_io(e) => {
                last = Some(e);
                // Give USB storage some time to become ready after heavy writes.
                spin_wait_ms(10);
            }
            Err(e) => return Err(e),
        }
        i = i.wrapping_add(1);
    }
    Err(last.unwrap_or(block::Error::Io))
}

/// Find where TRUEOSFS lives on a whole disk.
///
/// - Bootable disks: TRUEOSFS is expected to live inside a GPT partition (ESP exists elsewhere).
/// - Data-only disks: TRUEOSFS may live at LBA0 (superfloppy-style).
pub fn locate(handle: block::DeviceHandle) -> Result<Option<TrueosFsPlacement>, block::Error> {
    if handle.parent().is_some() {
        return Ok(None);
    }

    // Prefer GPT-partitioned layouts (bootable-capable).
    {
        let mut tries = 0u8;
        while tries < 5 {
            match partition::read_gpt_partitions(handle) {
                Ok(parts) => {
                    let mut has_esp = false;
                    for p in parts.iter() {
                        if p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES {
                            has_esp = true;
                        }

                        // Our superblock is at the start of the TRUEOS data partition.
                        if let Ok(p0) = read_blocks_aligned_retry(handle, p.range.first_lba(), 1, 3) {
                            if looks_like_trueos_superblock(&p0) {
                                let super_lba = p.range.first_lba();
                                let end_lba_exclusive = p.range.last_lba().saturating_add(1);
                                return Ok(Some(TrueosFsPlacement {
                                    bootable: has_esp,
                                    super_lba,
                                    data_lba: trueos_fs::data_lba_from_super(super_lba),
                                    data_end_lba_exclusive: Some(end_lba_exclusive),
                                }));
                            }
                        }
                    }
                    break;
                }
                Err(e) if is_transient_io(e) => {
                    spin_wait_ms(10);
                }
                Err(_) => break,
            }
            tries = tries.wrapping_add(1);
        }
    }

    // Fallback: superblock at LBA0 (data-only images/disks).
    let bs0 = read_blocks_aligned_retry(handle, 0, 1, 3)?;
    if looks_like_trueos_superblock(&bs0) {
        return Ok(Some(TrueosFsPlacement {
            bootable: false,
            super_lba: 0,
            data_lba: trueos_fs::data_lba_from_super(0),
            data_end_lba_exclusive: None,
        }));
    }

    Ok(None)
}

pub fn format_blank(handle: block::DeviceHandle) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    // If the disk is GPT-partitioned and has an ESP, do NOT clobber LBA0.
    // Only format an existing TRUEOS data partition (bootable layout).
    if let Ok(parts) = partition::read_gpt_partitions(handle) {
        let has_esp = parts
            .iter()
            .any(|p| p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES);

        if has_esp {
            if let Some(loc) = locate(handle)? {
                return format_blank_at(handle, loc.super_lba);
            }
            return Err(block::Error::NotSupported);
        }
    }

    // Data-only (superblock at LBA0).
    format_blank_at(handle, 0)
}

/// Format TRUEOSFS at the start of an already-created partition.
///
/// This is intended for installer code that first creates a GPT layout and then
/// formats the TRUEOS data partition without clobbering LBA0 of the whole disk.
pub fn format_blank_partition(partition: block::DeviceHandle) -> Result<(), block::Error> {
    if partition.parent().is_none() {
        return Err(block::Error::InvalidParam);
    }
    if !partition.supports_write() {
        return Err(block::Error::NotSupported);
    }
    format_blank_at(partition, 0)
}

fn format_blank_at(handle: block::DeviceHandle, super_lba: u64) -> Result<(), block::Error> {

    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        (info.max_transfer_bytes as usize / bs).max(1)
    } else {
        1
    };
    let blocks = core::cmp::min(8usize, max_blocks);
    let bytes = bs.saturating_mul(blocks);

    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(bytes, align).ok_or(block::Error::DmaUnavailable)?;
    let buf = tmp.as_mut_slice();
    buf.fill(0);

    trueos_fs::write_blank_superblock(&mut buf[..bs]);

    handle.write_blocks(super_lba, buf)?;
    handle.flush()?;

    // Best-effort end-to-end NVMe sanity check: write a tiny payload into the data region
    // and read it back. This keeps validation at the block device layer (no extra shell cmds)
    // and avoids clobbering partition tables/boot sectors.
    let info = handle.info();
    if info.kind == block::DeviceKind::Nvme {
        let data_lba = trueos_fs::data_lba_from_super(super_lba);
        // Need at least 2 blocks available in the data region.
        if data_lba.saturating_add(2) <= info.block_count {
            let blocks_verify = core::cmp::min(2usize, max_blocks);
            let bytes_verify = bs.saturating_mul(blocks_verify);
            let mut tmp2 = AlignedBuf::new(bytes_verify, align).ok_or(block::Error::DmaUnavailable)?;
            let w = tmp2.as_mut_slice();
            w.fill(0);
            let tag = b"TRUEOSFS-NVME-VERIFY";
            let n = core::cmp::min(tag.len(), bs);
            w[..n].copy_from_slice(&tag[..n]);
            // Put some changing bytes in the second block too.
            if blocks_verify > 1 {
                for (i, b) in w[bs..(2 * bs)].iter_mut().enumerate() {
                    *b = (i as u8).wrapping_mul(17).wrapping_add(0x5A);
                }
            }

            handle.write_blocks(data_lba, w)?;
            handle.flush()?;

            let mut rtmp = AlignedBuf::new(bytes_verify, align).ok_or(block::Error::DmaUnavailable)?;
            let r = rtmp.as_mut_slice();
            r.fill(0);
            handle.read_blocks(data_lba, r)?;
            if r != w {
                return Err(block::Error::Corrupted);
            }
        }
    }
    Ok(())
}
