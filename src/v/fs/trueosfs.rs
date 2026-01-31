use alloc::{boxed::Box, string::String, vec::Vec};

use crate::disc::block;
use crate::v::disc::partition;
use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
use trueos_fs::BlockIo;
use trueos_math::{BPlusTree, Tree};

const TRUEOSFS_INDEX_M: usize = 16;
const TRUEOSFS_CHECKPOINT_EVERY: u32 = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IndexRef {
    kind: trueos_fs::LogKind,
    entry_lba: u64,
}

type TrueosFsIndex = BPlusTree<Vec<u8>, IndexRef, TRUEOSFS_INDEX_M>;

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
    index: Option<Box<TrueosFsIndex>>,
    writes_since_checkpoint: u32,
}

static ROOT_SEQ: AtomicU32 = AtomicU32::new(0);
static ROOTS: Mutex<Vec<RootMount>> = Mutex::new(Vec::new());

static BSP_SMOKE_REQUESTED: AtomicBool = AtomicBool::new(false);
static BSP_SMOKE_DONE: AtomicBool = AtomicBool::new(false);

static MOUNT_REQUESTED: AtomicBool = AtomicBool::new(false);
static MOUNT_QUEUE: Mutex<heapless::Vec<block::DeviceHandle, 8>> = Mutex::new(heapless::Vec::new());

/// Request that TRUEOSFS probing/mounting be performed asynchronously.
///
/// This is intended for driver hotplug contexts (e.g. USB mass-storage attach) where
/// blocking the executor can starve the USB xHCI poll tasks.
pub fn request_mount_root(disk: block::DeviceHandle) {
    if disk.parent().is_some() {
        return;
    }

    {
        let mut q = MOUNT_QUEUE.lock();
        if q.iter().any(|d| d.id() == disk.id()) {
            return;
        }
        let _ = q.push(disk);
    }

    MOUNT_REQUESTED.store(true, Ordering::Release);
}

/// Background task that performs deferred TRUEOSFS probing/mounting.
#[embassy_executor::task]
pub async fn mount_service_task() {
    loop {
        if MOUNT_REQUESTED.swap(false, Ordering::AcqRel) {
            // Allow the device to settle after registration.
            Timer::after(EmbassyDuration::from_millis(100)).await;

            let mut local: heapless::Vec<block::DeviceHandle, 8> = heapless::Vec::new();
            {
                let mut q = MOUNT_QUEUE.lock();
                while let Some(d) = q.pop() {
                    let _ = local.push(d);
                }
            }

            for disk in local.iter().copied() {
                // Best-effort: only log when we actually mount or error.
                match mount_root_async(disk).await {
                    Ok(Some(disk_id)) => {
                        crate::log!("trueosfs: mounted root disk_id={}\n", disk_id.raw());
                    }
                    Ok(None) => {}
                    Err(e) => {
                        crate::log!("trueosfs: mount error {:?}\n", e);
                    }
                }
            }
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

/// Request that the BSP TrueOSFS smoke test run once.
///
/// Safe to call from hotplug/driver contexts (e.g. USB mass-storage attach).
/// The actual smoke test runs in [`bsp_smoke_service_task`].
pub fn request_bsp_smoke_test() {
    BSP_SMOKE_REQUESTED.store(true, Ordering::Release);
}

/// Background task that waits for [`request_bsp_smoke_test`] and then executes
/// the BSP smoke test exactly once.
#[embassy_executor::task]
pub async fn bsp_smoke_service_task() {
    loop {
        if BSP_SMOKE_REQUESTED.swap(false, Ordering::AcqRel) {
            // Allow the USBMS device to settle after registration.
            Timer::after(EmbassyDuration::from_millis(100)).await;
            bsp_smoke_test_once_async().await;
            return;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

/// Async variant of [`format_blank`].
///
/// This avoids `block_on` and is safe to call from async contexts.
pub async fn format_blank_async(handle: block::DeviceHandle) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    // If the disk is GPT-partitioned and has an ESP, do NOT clobber LBA0.
    // Only format an existing TRUEOS data partition (bootable layout).
    if let Ok(parts) = partition::read_gpt_partitions(handle).await {
        let has_esp = parts
            .iter()
            .any(|p| p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES);

        if has_esp {
            if let Some(loc) = locate_async(handle).await? {
                return format_blank_at_async(handle, loc.super_lba).await;
            }
            return Err(block::Error::NotSupported);
        }
    }

    // Data-only (superblock at LBA0).
    format_blank_at_async(handle, 0).await
}

struct KernelBlockIo {
    handle: block::DeviceHandle,
}

impl KernelBlockIo {
    #[inline]
    fn new(handle: block::DeviceHandle) -> Self {
        Self { handle }
    }
}

impl trueos_fs::BlockIo for KernelBlockIo {
    type Error = block::Error;

    #[inline]
    fn block_size(&self) -> usize {
        self.handle.info().block_size as usize
    }

    #[inline]
    fn block_count(&self) -> u64 {
        self.handle.info().block_count
    }

    #[inline]
    fn max_transfer_bytes(&self) -> usize {
        let v = self.handle.info().max_transfer_bytes as usize;
        if v == 0 { 256 * 1024 } else { v }
    }

    async fn read_blocks(&self, lba: u64, blocks: usize) -> Result<Vec<u8>, block::Error> {
        if blocks == 0 {
            return Ok(Vec::new());
        }

        let info = self.handle.info();
        let bs = info.block_size as usize;
        if bs == 0 {
            return Err(block::Error::InvalidParam);
        }

        let max_blocks = if info.max_transfer_bytes > 0 {
            (info.max_transfer_bytes as usize / bs).max(1)
        } else {
            1
        };

        let mut out = Vec::with_capacity(bs.saturating_mul(blocks));
        let mut cur_lba = lba;
        let mut remaining = blocks;
        while remaining > 0 {
            let blocks_here = core::cmp::min(remaining, max_blocks);
            let tmp = self.handle.read_blocks(cur_lba, blocks_here).await?;
            out.extend_from_slice(&tmp);
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            remaining = remaining.saturating_sub(blocks_here);
        }

        Ok(out)
    }

    async fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), block::Error> {
        if buf.is_empty() {
            return Ok(());
        }
        let info = self.handle.info();
        let bs = info.block_size as usize;
        if bs == 0 || buf.len() % bs != 0 {
            return Err(block::Error::InvalidParam);
        }

        let max_blocks = if info.max_transfer_bytes > 0 {
            (info.max_transfer_bytes as usize / bs).max(1)
        } else {
            1
        };

        let mut cur_lba = lba;
        let mut off = 0usize;
        while off < buf.len() {
            let remaining = buf.len() - off;
            let blocks_here = core::cmp::min(max_blocks, remaining / bs);
            let bytes_here = blocks_here * bs;
            self.handle
                .write_blocks(cur_lba, &buf[off..off + bytes_here])
                .await?;
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            off = off.saturating_add(bytes_here);
        }

        Ok(())
    }

    #[inline]
    async fn flush(&self) -> Result<(), block::Error> {
        self.handle.flush().await
    }
}

#[inline]
fn map_engine_err(e: trueos_fs::FsError<block::Error>) -> block::Error {
    match e {
        trueos_fs::FsError::Device(e) => e,
        trueos_fs::FsError::InvalidParam => block::Error::InvalidParam,
        trueos_fs::FsError::Corrupted => block::Error::Corrupted,
    }
}

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

    {
        let roots = ROOTS.lock();
        if roots.iter().any(|m| m.disk_id == disk_id) {
            return Ok(Some(disk_id));
        }
    }

    // Seed an initial tree skeleton. Directory enumeration will fill this later.
    let mut tree = TrueosFsTree::new();
    let tree = match tree.add_root(TrueosFsTreeEntry {
        kind: TrueosFsTreeKind::Root,
        name: String::from("trueosfs"),
    }) {
        Some(root) => {
            let _ = tree.add_child(
                root,
                TrueosFsTreeEntry {
                    kind: TrueosFsTreeKind::Dir,
                    name: String::from("/"),
                },
            );
            Some(Box::new(tree))
        }
        None => None,
    };

    let (index, writes_since_checkpoint) = build_root_index(disk, &placement).unwrap_or((None, 0));

    let mut roots = ROOTS.lock();
    if roots.iter().any(|m| m.disk_id == disk_id) {
        return Ok(Some(disk_id));
    }
    roots.push(RootMount {
        disk_id,
        placement,
        seq: ROOT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1),
        tree,
        index,
        writes_since_checkpoint,
    });

    Ok(Some(disk_id))
}

/// Async variant of [`mount_root`].
///
/// Use this from async contexts to avoid `block_on` (which can starve other tasks such as USB polling).
pub async fn mount_root_async(disk: block::DeviceHandle) -> Result<Option<block::DiscId>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let Some(placement) = locate_async(disk).await? else {
        return Ok(None);
    };

    let disk_id = disk.id();

    {
        let roots = ROOTS.lock();
        if roots.iter().any(|m| m.disk_id == disk_id) {
            return Ok(Some(disk_id));
        }
    }

    // Seed an initial tree skeleton. Directory enumeration will fill this later.
    let mut tree = TrueosFsTree::new();
    let tree = match tree.add_root(TrueosFsTreeEntry {
        kind: TrueosFsTreeKind::Root,
        name: String::from("trueosfs"),
    }) {
        Some(root) => {
            let _ = tree.add_child(
                root,
                TrueosFsTreeEntry {
                    kind: TrueosFsTreeKind::Dir,
                    name: String::from("/"),
                },
            );
            Some(Box::new(tree))
        }
        None => None,
    };

    // IMPORTANT: do not call `build_root_index()` here.
    // `build_root_index()` uses the synchronous TRUEOSFS engine which calls into
    // `KernelBlockIo` and ultimately `crate::time::block_on(...)`. Doing that from
    // an async task can starve other async tasks (notably the xHCI poll task), which
    // can manifest as USB mass-storage transfers timing out due to missing completion
    // events.
    //
    // Index building is an optional cache; we can populate it later via a dedicated
    // async pipeline if needed.
    let index = None;
    let writes_since_checkpoint = 0;

    let mut roots = ROOTS.lock();
    if roots.iter().any(|m| m.disk_id == disk_id) {
        return Ok(Some(disk_id));
    }
    roots.push(RootMount {
        disk_id,
        placement,
        seq: ROOT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1),
        tree,
        index,
        writes_since_checkpoint,
    });

    Ok(Some(disk_id))
}

/// Async TRUEOSFS: write/replace a file.
///
/// Semantics match [`file_in`], but this avoids `block_on` and is safe to call from async contexts.
pub async fn file_in_async(
    disk: block::DeviceHandle,
    name: &str,
    bytes: &[u8],
) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate_async(disk).await? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    trueos_fs::write_file(&io, &params, name, bytes)
        .await
        .map_err(map_engine_err)
}

/// Async TRUEOSFS: read a file.
///
/// Returns `Ok(None)` if missing or fails integrity check.
pub async fn file_out_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<Option<Vec<u8>>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate_async(disk).await? else {
        return Ok(None);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    trueos_fs::read_file(&io, &params, name)
        .await
        .map_err(map_engine_err)
}

/// Async TRUEOSFS: delete a file.
pub async fn file_delete_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate_async(disk).await? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    trueos_fs::delete_file(&io, &params, name)
        .await
        .map_err(map_engine_err)
}

/// Async TRUEOSFS: check whether a file exists.
pub async fn file_exists_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate_async(disk).await? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    trueos_fs::file_exists(&io, &params, name)
        .await
        .map_err(map_engine_err)
}

/// Async TRUEOSFS: list the immediate children of a directory.
///
/// Returns `Ok(None)` if the disk does not contain TRUEOSFS.
pub async fn list_dir_async(
    disk: block::DeviceHandle,
    dir: &str,
) -> Result<Option<String>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate_async(disk).await? else {
        return Ok(None);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    let out = trueos_fs::list_dir(&io, &params, dir)
        .await
        .map_err(map_engine_err)?;
    Ok(Some(out))
}

/// Async TRUEOSFS: append bytes by performing a full new write.
pub async fn file_append_async(
    disk: block::DeviceHandle,
    name: &str,
    append_bytes: &[u8],
) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate_async(disk).await? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    trueos_fs::append_file(&io, &params, name, append_bytes)
        .await
        .map_err(map_engine_err)
}

/// TRUEOSFS: write/replace a file.
///
/// Semantics:
/// - Always checks space before writing.
/// - Returns `Ok(false)` if no space or invalid name.
/// - Returns `Err(_)` on I/O errors.
pub fn file_in(disk: block::DeviceHandle, name: &str, bytes: &[u8]) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let disk_id = disk.id();

    let had_index = root_mount_has_index(disk_id);
    let old_head_rel = if had_index { read_log_head_rel_blocks(&io, &params)? } else { 0 };

    let ok = crate::time::block_on(trueos_fs::write_file(&io, &params, name, bytes)).map_err(map_engine_err)?;
    if ok && had_index {
        let new_head_rel = read_log_head_rel_blocks(&io, &params)?;
        let applied = replay_tail_into_index(disk_id, &io, &params, old_head_rel, new_head_rel);
        if let Some(applied) = applied {
            if disk.supports_write() {
                root_index_note_writes_and_maybe_checkpoint(disk_id, &io, &params, new_head_rel, applied);
            }
        }
    }
    Ok(ok)
}

/// TRUEOSFS: read a file.
///
/// Returns `Ok(None)` if missing or fails integrity check.
pub fn file_out(disk: block::DeviceHandle, name: &str) -> Result<Option<Vec<u8>>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(None);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let disk_id = disk.id();

    if root_mount_has_index(disk_id) {
        let key = name.as_bytes().to_vec();
        if let Some(r) = root_index_lookup(disk_id, &key) {
            if r.kind == trueos_fs::LogKind::Delete {
                return Ok(None);
            }
            if let Some(bytes) = crate::time::block_on(trueos_fs::read_file_at_for_name(
                &io,
                &params,
                name,
                r.entry_lba,
            ))
            .map_err(map_engine_err)?
            {
                return Ok(Some(bytes));
            }
        }
    }

    crate::time::block_on(trueos_fs::read_file(&io, &params, name)).map_err(map_engine_err)
}

pub fn file_out_ok(disk: block::DeviceHandle, name: &str, out: &mut Vec<u8>) -> Result<bool, block::Error> {
    match file_out(disk, name)? {
        Some(v) => {
            out.clear();
            out.extend_from_slice(&v);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// TRUEOSFS: delete a file.
pub fn file_delete(disk: block::DeviceHandle, name: &str) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let disk_id = disk.id();

    let had_index = root_mount_has_index(disk_id);
    let old_head_rel = if had_index { read_log_head_rel_blocks(&io, &params)? } else { 0 };

    let ok = crate::time::block_on(trueos_fs::delete_file(&io, &params, name)).map_err(map_engine_err)?;
    if ok && had_index {
        let new_head_rel = read_log_head_rel_blocks(&io, &params)?;
        let applied = replay_tail_into_index(disk_id, &io, &params, old_head_rel, new_head_rel);
        if let Some(applied) = applied {
            if disk.supports_write() {
                root_index_note_writes_and_maybe_checkpoint(disk_id, &io, &params, new_head_rel, applied);
            }
        }
    }
    Ok(ok)
}

/// TRUEOSFS: validate a file by comparing stored SHA-256 with recomputation.
pub fn file_valid(disk: block::DeviceHandle, name: &str) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    crate::time::block_on(trueos_fs::file_valid(&io, &params, name)).map_err(map_engine_err)
}

/// TRUEOSFS: check whether a file exists.
pub fn file_exists(disk: block::DeviceHandle, name: &str) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    let disk_id = disk.id();
    if root_mount_has_index(disk_id) {
        let key = name.as_bytes().to_vec();
        if let Some(r) = root_index_lookup(disk_id, &key) {
            if r.kind != trueos_fs::LogKind::Put {
                return Ok(false);
            }
            // Validate against on-disk name so a stale index can't lie.
            let Some(kind) = crate::time::block_on(trueos_fs::read_entry_kind_at_named(
                &io,
                &params,
                r.entry_lba,
                key.as_slice(),
            ))
            .map_err(map_engine_err)?
            else {
                // Fall back to engine scan.
                return crate::time::block_on(trueos_fs::file_exists(&io, &params, name)).map_err(map_engine_err);
            };
            return Ok(kind == trueos_fs::LogKind::Put);
        }
    }

    crate::time::block_on(trueos_fs::file_exists(&io, &params, name)).map_err(map_engine_err)
}

/// TRUEOSFS: list the immediate children of a directory.
///
/// This treats stored file names as `/`-separated paths (e.g. `qjs/cdn/abc.mjs`).
/// Output matches the USBMS/FAT listing format: newline-separated entry names.
///
/// Returns `Ok(None)` if the disk does not contain TRUEOSFS.
pub fn list_dir(disk: block::DeviceHandle, dir: &str) -> Result<Option<String>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(None);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let disk_id = disk.id();

    if root_mount_has_index(disk_id) {
        if let Some(out) = list_dir_from_index(&io, &params, disk_id, dir) {
            return Ok(Some(out));
        }
    }

    let out = crate::time::block_on(trueos_fs::list_dir(&io, &params, dir)).map_err(map_engine_err)?;
    Ok(Some(out))
}

/// TRUEOSFS: append bytes by performing a full new write.
///
/// - If file missing: forwards to `file_in`.
/// - If `append_bytes` is empty: returns `Ok(true)`.
pub fn file_append(disk: block::DeviceHandle, name: &str, append_bytes: &[u8]) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let disk_id = disk.id();

    let had_index = root_mount_has_index(disk_id);
    let old_head_rel = if had_index { read_log_head_rel_blocks(&io, &params)? } else { 0 };

    let ok = crate::time::block_on(trueos_fs::append_file(&io, &params, name, append_bytes)).map_err(map_engine_err)?;
    if ok && had_index {
        let new_head_rel = read_log_head_rel_blocks(&io, &params)?;
        let applied = replay_tail_into_index(disk_id, &io, &params, old_head_rel, new_head_rel);
        if let Some(applied) = applied {
            if disk.supports_write() {
                root_index_note_writes_and_maybe_checkpoint(disk_id, &io, &params, new_head_rel, applied);
            }
        }
    }
    Ok(ok)
}

fn replay_tail_into_index(
    disk_id: block::DiscId,
    io: &KernelBlockIo,
    params: &trueos_fs::FsParams,
    start_rel: u64,
    end_rel: u64,
) -> Option<u32> {
    if start_rel >= end_rel {
        return Some(0);
    }

    let mut applied = 0u32;
    crate::time::block_on(trueos_fs::replay_log_range(
        io,
        params,
        start_rel,
        end_rel,
        |kind, name_bytes, entry_lba| {
            applied = applied.saturating_add(1);
            root_index_insert_if_mounted(
                disk_id,
                name_bytes,
                IndexRef { kind, entry_lba },
            );
        },
    ))
    .ok()?;

    Some(applied)
}

fn root_index_note_writes_and_maybe_checkpoint(
    disk_id: block::DiscId,
    io: &KernelBlockIo,
    params: &trueos_fs::FsParams,
    replay_from_rel_blocks: u64,
    newly_applied: u32,
) {
    if newly_applied == 0 {
        return;
    }

    // Update the counter and snapshot index entries under the lock.
    // Do not do I/O while holding the lock.
    let entries: Vec<(Vec<u8>, trueos_fs::LogKind, u64)> = {
        let mut roots = ROOTS.lock();
        let Some(m) = roots.iter_mut().find(|m| m.disk_id == disk_id) else {
            return;
        };
        let Some(idx) = m.index.as_deref() else {
            return;
        };

        m.writes_since_checkpoint = m.writes_since_checkpoint.saturating_add(newly_applied);
        if m.writes_since_checkpoint < TRUEOSFS_CHECKPOINT_EVERY {
            return;
        }

        idx.iter().map(|(k, v)| (k.clone(), v.kind, v.entry_lba)).collect()
    };

    // Attempt checkpoint write without holding ROOTS lock.
    let ok = crate::time::block_on(trueos_fs::write_index_checkpoint(
        io,
        params,
        replay_from_rel_blocks,
        entries.into_iter(),
    ))
    .ok();
    if ok == Some(true) {
        let mut roots = ROOTS.lock();
        if let Some(m) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
            m.writes_since_checkpoint = 0;
        }
    }
}

fn read_log_head_rel_blocks(
    io: &KernelBlockIo,
    params: &trueos_fs::FsParams,
) -> Result<u64, block::Error> {
    let sb_block = crate::time::block_on(io.read_blocks(params.super_lba, 1))?;
    let Some(sb) = trueos_fs::parse_superblock(&sb_block) else {
        return Err(block::Error::Corrupted);
    };
    Ok(sb.log_head_rel_blocks)
}

fn root_mount_has_index(disk_id: block::DiscId) -> bool {
    let roots = ROOTS.lock();
    roots
        .iter()
        .find(|m| m.disk_id == disk_id)
        .and_then(|m| m.index.as_deref())
        .is_some()
}

fn root_index_lookup(disk_id: block::DiscId, key: &Vec<u8>) -> Option<IndexRef> {
    let roots = ROOTS.lock();
    let idx = roots.iter().find(|m| m.disk_id == disk_id)?.index.as_deref()?;
    idx.get(key).copied()
}

fn root_index_insert_if_mounted(disk_id: block::DiscId, key: Vec<u8>, r: IndexRef) {
    let mut roots = ROOTS.lock();
    let Some(m) = roots.iter_mut().find(|m| m.disk_id == disk_id) else {
        return;
    };
    let idx = m.index.get_or_insert_with(|| Box::new(TrueosFsIndex::new()));
    let _ = idx.insert(key, r);
}

fn normalize_rel_no_parent(path: &str) -> Option<String> {
    if path.is_empty() {
        return Some(String::new());
    }
    let mut out = String::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }
    Some(out)
}

fn list_dir_from_index(
    io: &KernelBlockIo,
    params: &trueos_fs::FsParams,
    disk_id: block::DiscId,
    dir: &str,
) -> Option<String> {
    let Some(rel) = normalize_rel_no_parent(dir) else {
        return Some(String::new());
    };
    let prefix = if rel.is_empty() {
        Vec::new()
    } else {
        let mut p = rel.into_bytes();
        p.push(b'/');
        p
    };

    // Collect candidate full keys while holding the lock; do not do I/O under the lock.
    let candidates: Vec<(Vec<u8>, IndexRef)> = {
        let roots = ROOTS.lock();
        let idx = roots.iter().find(|m| m.disk_id == disk_id)?.index.as_deref()?;

        let mut out: Vec<(Vec<u8>, IndexRef)> = Vec::new();
        for (k, v) in idx.iter_from(&prefix) {
            if !k.as_slice().starts_with(prefix.as_slice()) {
                break;
            }
            if v.kind != trueos_fs::LogKind::Put {
                continue;
            }
            out.push((k.clone(), *v));
        }
        out
    };

    let mut live: alloc::collections::BTreeSet<String> = alloc::collections::BTreeSet::new();
    for (k, v) in candidates.into_iter() {
        // Validate against on-disk entry so a stale/corrupt index can't lie.
        let kind_opt = match crate::time::block_on(trueos_fs::read_entry_kind_at_named(
            io,
            params,
            v.entry_lba,
            k.as_slice(),
        )) {
            Ok(v) => v,
            Err(_) => return None,
        };
        let Some(kind) = kind_opt else {
            return None;
        };
        if kind != trueos_fs::LogKind::Put {
            // Index says Put but disk disagrees: fall back to engine scan for correctness.
            return None;
        }

        let rest = &k[prefix.len()..];
        if rest.is_empty() {
            continue;
        }
        let cut = rest.iter().position(|&b| b == b'/').unwrap_or(rest.len());
        let child = &rest[..cut];
        if child.is_empty() {
            continue;
        }
        if let Ok(s) = core::str::from_utf8(child) {
            live.insert(String::from(s));
        }
    }

    let mut out = String::new();
    for name in live.into_iter() {
        out.push_str(&name);
        out.push('\n');
    }
    Some(out)
}

fn build_root_index(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
) -> Result<(Option<Box<TrueosFsIndex>>, u32), block::Error> {
    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let sb_block = crate::time::block_on(io.read_blocks(params.super_lba, 1))?;
    let Some(sb) = trueos_fs::parse_superblock(&sb_block) else {
        return Err(block::Error::Corrupted);
    };

    let mut index = TrueosFsIndex::new();
    let mut replay_from_rel = 0u64;
    let mut replayed = 0u32;
    let mut post_replay_checkpoint_ok = false;

    match crate::time::block_on(trueos_fs::read_index_checkpoint(&io, &params)) {
        Ok(Some(ckpt)) => {
            replay_from_rel = ckpt.replay_from_rel_blocks;
            for (k, kind, entry_lba) in ckpt.entries.into_iter() {
                let _ = index.insert(
                    k,
                    IndexRef {
                        kind,
                        entry_lba,
                    },
                );
            }
        }
        Ok(None) => {}
        Err(e) => {
            // Best-effort: still mount without an index.
            crate::log!("trueosfs: checkpoint read error {:?}\n", e);
        }
    }

    let _ = crate::time::block_on(trueos_fs::replay_log_range(
        &io,
        &params,
        replay_from_rel,
        sb.log_head_rel_blocks,
        |kind, name_bytes, entry_lba| {
            replayed = replayed.saturating_add(1);
            let _ = index.insert(name_bytes, IndexRef { kind, entry_lba });
        },
    ));

    // Optional best-effort checkpoint after replay:
    // - If no checkpoint exists, write one.
    // - If we had to replay >=N records, write a new checkpoint after replay.
    // This does *not* seed the per-mount write counter; we always start at 0 after mount.
    let post_replay_checkpoint_attempted =
        disk.supports_write() && (sb.checkpoint_rel_blocks == 0 || replayed >= TRUEOSFS_CHECKPOINT_EVERY);
    if post_replay_checkpoint_attempted {
        post_replay_checkpoint_ok = crate::time::block_on(trueos_fs::write_index_checkpoint(
            &io,
            &params,
            sb.log_head_rel_blocks,
            index.iter().map(|(k, v)| (k.clone(), v.kind, v.entry_lba)),
        ))
        .is_ok();
    }

    let post_replay_checkpoint_status = if !post_replay_checkpoint_attempted {
        "skip"
    } else if post_replay_checkpoint_ok {
        "ok"
    } else {
        "err"
    };
    crate::log!(
        "trueosfs: mount replayed={} rel={}..{} ckpt_rel={} post_ckpt={} writes_since_ckpt=0\n",
        replayed,
        replay_from_rel,
        sb.log_head_rel_blocks,
        sb.checkpoint_rel_blocks,
        post_replay_checkpoint_status,
    );

    Ok((Some(Box::new(index)), 0))
}

pub fn roots_len() -> usize {
    ROOTS.lock().len()
}

/// Returns the most recently mounted TRUEOSFS root disk id (best-effort).
///
/// This is used by higher layers (shell, C ABI helpers) that want a sensible
/// default filesystem target without user-facing mount plumbing.
pub fn primary_root_id() -> Option<block::DiscId> {
    let roots = ROOTS.lock();
    roots.iter().max_by_key(|m| m.seq).map(|m| m.disk_id)
}

/// Returns a handle for the most recently mounted TRUEOSFS root disk.
pub fn primary_root_handle() -> Option<block::DeviceHandle> {
    primary_root_id().and_then(block::device_handle)
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
    crate::time::block_on(handle.read_blocks(lba, blocks))
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
        crate::time::poll();
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
            match crate::time::block_on(partition::read_gpt_partitions(handle)) {
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
                // If GPT parsing fails in a non-transient way, surface it so callers can log it.
                Err(e) => return Err(e),
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

async fn read_blocks_aligned_async(
    handle: block::DeviceHandle,
    lba: u64,
    blocks: usize,
) -> Result<Vec<u8>, block::Error> {
    handle.read_blocks(lba, blocks).await
}

async fn read_blocks_aligned_retry_async(
    handle: block::DeviceHandle,
    lba: u64,
    blocks: usize,
    attempts: u8,
) -> Result<Vec<u8>, block::Error> {
    let mut last: Option<block::Error> = None;
    let mut i = 0u8;
    while i < attempts {
        match read_blocks_aligned_async(handle, lba, blocks).await {
            Ok(v) => return Ok(v),
            Err(e) if is_transient_io(e) => {
                last = Some(e);
                // Give USB storage some time to become ready after heavy writes.
                Timer::after(EmbassyDuration::from_millis(10)).await;
            }
            Err(e) => return Err(e),
        }
        i = i.wrapping_add(1);
    }
    Err(last.unwrap_or(block::Error::Io))
}

/// Async variant of [`locate`].
///
/// This avoids `block_on` so it can be called from async installer jobs.
pub async fn locate_async(handle: block::DeviceHandle) -> Result<Option<TrueosFsPlacement>, block::Error> {
    if handle.parent().is_some() {
        return Ok(None);
    }

    // Prefer GPT-partitioned layouts (bootable-capable).
    {
        let mut tries = 0u8;
        while tries < 5 {
            match partition::read_gpt_partitions(handle).await {
                Ok(parts) => {
                    let mut has_esp = false;
                    for p in parts.iter() {
                        if p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES {
                            has_esp = true;
                        }

                        // Our superblock is at the start of the TRUEOS data partition.
                        if let Ok(p0) = read_blocks_aligned_retry_async(handle, p.range.first_lba(), 1, 3).await {
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
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                }
                Err(e) => return Err(e),
            }
            tries = tries.wrapping_add(1);
        }
    }

    // Fallback: superblock at LBA0 (data-only images/disks).
    let bs0 = read_blocks_aligned_retry_async(handle, 0, 1, 3).await?;
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
    if let Ok(parts) = crate::time::block_on(partition::read_gpt_partitions(handle)) {
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

/// Force-format the whole disk as data-only TRUEOSFS (superblock at LBA0).
///
/// This is intentionally destructive and is intended for interactive/debug use
/// (e.g. the shell `format` command) after explicit user confirmation.
///
/// Unlike [`format_blank`], this will *not* refuse to proceed when a GPT with an
/// ESP exists. It also wipes the primary/backup GPT headers best-effort so that
/// subsequent detection doesn't keep treating the disk as a GPT layout.
pub fn format_blank_force(handle: block::DeviceHandle) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    // Best-effort: wipe GPT headers (LBA1 and backup header at last LBA).
    // We do not try to wipe the whole partition array here.
    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    if info.block_count > 2 {
        let align = info.dma_alignment.max(1) as usize;
        let mut tmp = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
        let z = tmp.as_mut_slice();
        z.fill(0);

        // Primary GPT header.
        let _ = crate::time::block_on(handle.write_blocks(1, z));
        // Backup GPT header.
        let last_lba = info.block_count.saturating_sub(1);
        let _ = crate::time::block_on(handle.write_blocks(last_lba, z));
        let _ = crate::time::block_on(handle.flush());
    }

    format_blank_at(handle, 0)
}

/// Async variant of [`format_blank_force`].
///
/// This avoids `block_on` so it can be used from async contexts (e.g. the shell task)
/// without starving other services.
pub async fn format_blank_force_async(handle: block::DeviceHandle) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    // Best-effort: wipe GPT headers (LBA1 and backup header at last LBA).
    // We do not try to wipe the whole partition array here.
    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    if info.block_count > 2 {
        let align = info.dma_alignment.max(1) as usize;
        let mut tmp = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
        let z = tmp.as_mut_slice();
        z.fill(0);

        // Primary GPT header.
        let _ = handle.write_blocks(1, z).await;
        // Backup GPT header.
        let last_lba = info.block_count.saturating_sub(1);
        let _ = handle.write_blocks(last_lba, z).await;
        let _ = handle.flush().await;
    }

    format_blank_at_async(handle, 0).await
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

/// Async variant of [`format_blank_partition`].
pub async fn format_blank_partition_async(partition: block::DeviceHandle) -> Result<(), block::Error> {
    if partition.parent().is_none() {
        return Err(block::Error::InvalidParam);
    }
    if !partition.supports_write() {
        return Err(block::Error::NotSupported);
    }
    format_blank_at_async(partition, 0).await
}

async fn format_blank_at_async(handle: block::DeviceHandle, super_lba: u64) -> Result<(), block::Error> {
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

    handle.write_blocks(super_lba, buf).await?;
    handle.flush().await?;

    // Verify the superblock write actually stuck (important for flaky USBMS media).
    let verify0 = read_blocks_aligned_retry_async(handle, super_lba, 1, 10).await?;
    if !looks_like_trueos_superblock(&verify0) {
        return Err(block::Error::Corrupted);
    }

    // Best-effort end-to-end NVMe sanity check: write a tiny payload into the data region
    // and read it back.
    let info = handle.info();
    if info.kind == block::DeviceKind::Nvme {
        let data_lba = trueos_fs::data_lba_from_super(super_lba);
        if data_lba.saturating_add(2) <= info.block_count {
            let blocks_verify = core::cmp::min(2usize, max_blocks);
            let bytes_verify = bs.saturating_mul(blocks_verify);
            let mut tmp2 = AlignedBuf::new(bytes_verify, align).ok_or(block::Error::DmaUnavailable)?;
            let w = tmp2.as_mut_slice();
            w.fill(0);
            let tag = b"TRUEOSFS-NVME-VERIFY";
            let n = core::cmp::min(tag.len(), bs);
            w[..n].copy_from_slice(&tag[..n]);
            if blocks_verify > 1 {
                for (i, b) in w[bs..(2 * bs)].iter_mut().enumerate() {
                    *b = (i as u8).wrapping_mul(17).wrapping_add(0x5A);
                }
            }

            handle.write_blocks(data_lba, w).await?;
            handle.flush().await?;

            let r = handle.read_blocks(data_lba, blocks_verify).await?;
            if r.as_slice() != w {
                return Err(block::Error::Corrupted);
            }
        }
    }

    Ok(())
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

    crate::time::block_on(handle.write_blocks(super_lba, buf))?;
    crate::time::block_on(handle.flush())?;

    // Verify the superblock write actually stuck (important for flaky USBMS media).
    let verify0 = read_blocks_aligned_retry(handle, super_lba, 1, 10)?;
    if !looks_like_trueos_superblock(&verify0) {
        return Err(block::Error::Corrupted);
    }

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

            crate::time::block_on(handle.write_blocks(data_lba, w))?;
            crate::time::block_on(handle.flush())?;

            let r = crate::time::block_on(handle.read_blocks(data_lba, blocks_verify))?;
            if r.as_slice() != w {
                return Err(block::Error::Corrupted);
            }
        }
    }
    Ok(())
}

pub async fn bsp_smoke_test_once_async() {
    if BSP_SMOKE_DONE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    crate::log!("trueosfs: bsp smoke begin\n");

    let devices = block::devices();
    if devices.is_empty() {
        crate::log!("trueosfs: no block devices present\n");
        return;
    }

    // 1) Log present discs (whole devices).
    let discs: alloc::vec::Vec<_> = devices.iter().filter(|d| d.parent.is_none()).collect();
    crate::log!(
        "trueosfs: present devices={} discs={}\n",
        devices.len(),
        discs.len()
    );
    for info in discs.iter().copied() {
        crate::log!(
            "trueosfs: disc id={} kind={:?} bs={} blocks={} writable={} label={:?} pci={:?}\n",
            info.id,
            info.kind,
            info.block_size,
            info.block_count,
            info.writable,
            info.label,
            info.pci
        );
    }

    // 2) Detect classification for each disc.
    let mut trueos_disk: Option<block::DeviceHandle> = None;
    for h in block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }

        // Disks can briefly report transient I/O errors right after bring-up.
        // Retry a handful of times so BSP logs reflect the steady state.
        let mut last = (crate::v::disc::detect::DiscStatus::Unknown, None);
        let mut tries = 0u8;
        while tries < 10 {
            let r = crate::v::disc::detect::detect_physical_disk_detail(h).await;
            match r.1 {
                Some(e) if is_transient_io(e) => {
                    last = r;
                    Timer::after(EmbassyDuration::from_millis(25)).await;
                }
                _ => {
                    last = r;
                    break;
                }
            }
            tries = tries.wrapping_add(1);
        }

        let (status, err) = last;
        if let Some(e) = err {
            crate::log!(
                "trueosfs: detect {} => {} (err={:?})\n",
                h.id(),
                status.short(),
                e
            );
        } else {
            crate::log!("trueosfs: detect {} => {}\n", h.id(), status.short());
        }

        if trueos_disk.is_none() {
            if let crate::v::disc::detect::DiscStatus::Trueos { .. } = status {
                trueos_disk = Some(h);
            }
        }
    }

        // Debug convenience: allow the BSP smoke test to operate on a fresh, unformatted
        // `disk.img` by formatting a *completely blank* disk as data-only TRUEOSFS.
        //
        // Safety properties:
        // - Only in debug builds.
        // - Only if LBA0 is all-zero (strong signal of "empty" media).
        // - Only if the device is writable.
    #[cfg(debug_assertions)]
    if trueos_disk.is_none() {
        for h in block::device_handles().into_iter() {
            if h.parent().is_some() {
                continue;
            }
            if !h.supports_write() {
                continue;
            }

            let bs0 = match read_blocks_aligned_retry_async(h, 0, 1, 3).await {
                Ok(v) => v,
                Err(e) => {
                    crate::log!(
                        "trueosfs: smoke: blank check read failed dev={} err={:?}\n",
                        h.id(),
                        e
                    );
                    continue;
                }
            };

            if bs0.iter().any(|&b| b != 0) {
                continue;
            }

            crate::log!(
                "trueosfs: smoke: blank writable disk detected dev={} -> formatting TRUEOSFS\n",
                h.id()
            );
            match format_blank_async(h).await {
                Ok(()) => {
                    crate::log!("trueosfs: smoke: format ok dev={} -> mounting\n", h.id());
                }
                Err(e) => {
                    crate::log!(
                        "trueosfs: smoke: format failed dev={} err={:?}\n",
                        h.id(),
                        e
                    );
                    continue;
                }
            }

            match mount_root_async(h).await {
                Ok(Some(_)) => {
                    trueos_disk = Some(h);
                    break;
                }
                Ok(None) => {
                    crate::log!(
                        "trueosfs: smoke: format succeeded but mount_root_async returned none dev={}\n",
                        h.id()
                    );
                }
                Err(e) => {
                    crate::log!(
                        "trueosfs: smoke: mount_root_async failed after format dev={} err={:?}\n",
                        h.id(),
                        e
                    );
                }
            }
        }
    }

        // 3) If we have a TRUEOS partition, do a write/read smoke test.
    let Some(disk) = trueos_disk else {
        crate::log!("trueosfs: smoke: no partition\n");
        return;
    };

    let placement = match locate_async(disk).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            crate::log!("trueosfs: smoke: detect said trueos but locate_async returned none\n");
            return;
        }
        Err(e) => {
            crate::log!("trueosfs: smoke: locate_async failed: {:?}\n", e);
            return;
        }
    };
        crate::log!(
            "trueosfs: smoke: using {} bootable={} super_lba={} data_lba={} end={:?} writable={}\n",
            disk.id(),
            placement.bootable,
            placement.super_lba,
            placement.data_lba,
            placement.data_end_lba_exclusive,
            disk.supports_write()
        );

    // Async-safe verification: re-read the superblock and parse it.
    let info = disk.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        crate::log!("trueosfs: smoke: invalid block size\n");
        return;
    }
    match disk.read_blocks(placement.super_lba, 1).await {
        Ok(v) => {
            let b0 = &v[..core::cmp::min(bs, v.len())];
            if !looks_like_trueos_superblock(b0) {
                crate::log!("trueosfs: smoke: superblock readback does not look like TRUEOSFS\n");
                return;
            }
            if let Some(sb) = trueos_fs::parse_superblock(b0) {
                crate::log!(
                    "trueosfs: smoke: superblock ok log_head_rel={} ckpt_rel={}\n",
                    sb.log_head_rel_blocks,
                    sb.checkpoint_rel_blocks
                );
            }
        }
        Err(e) => {
            crate::log!("trueosfs: smoke: superblock read failed: {:?}\n", e);
            return;
        }
    }

    // Best-effort: ensure the root is registered for higher layers.
    let _ = mount_root_async(disk).await;

    crate::log!("trueosfs: bsp smoke ok\n");
}
