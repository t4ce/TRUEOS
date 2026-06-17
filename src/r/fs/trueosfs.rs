#![allow(dead_code)]

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};

use crate::disc::block;
use crate::r::disc::partition;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub use trueos_fs::FileInfo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IndexRef {
    kind: trueos_fs::LogKind,
    entry_lba: u64,
}

// Switch to alloc::collections::BTreeMap for full delete support
// type TrueosFsIndex = BPlusTree<Vec<u8>, IndexRef, TRUEOSFS_INDEX_M>;
type TrueosFsIndex = BTreeMap<Vec<u8>, IndexRef>;

const FILE_RECORD_CACHE_CAP: usize = 64;
const TRUEOSFS_CHECKPOINT_MIN_TAIL_BLOCKS: u64 = 4096;

struct BuiltIndex {
    tree: Box<TrueosFsIndex>,
    replay_from_rel_blocks: u64,
    end_rel_blocks: u64,
    had_checkpoint: bool,
}

struct FileRecordCacheEntry {
    disk_id: block::DiscId,
    path: String,
    record: trueos_fs::FileRecordRef,
    cache_gen: u32,
    last_use: u64,
}

// Standard EFI System Partition type GUID.
// C12A7328-F81F-11D2-BA4B-00A0C93EC93B
const GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];
const TRUEOSFS_MIN_TOTAL_BLOCKS: u64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrueosFsPlacement {
    pub bootable: bool,
    pub super_lba: u64,
    pub data_lba: u64,
    pub data_end_lba_exclusive: Option<u64>,
}

struct RootMount {
    disk_id: block::DiscId,
    seq: u32,
    index: Option<Box<TrueosFsIndex>>,
    building_index: bool,
    writes_since_checkpoint: u32,
    cache_gen: u32,
}

static ROOT_SEQ: AtomicU32 = AtomicU32::new(0);
static ROOTS: Mutex<Vec<RootMount>> = Mutex::new(Vec::new());
static PRIMARY_ROOT_RAW: AtomicU32 = AtomicU32::new(0);
static PRIMARY_ROOT_HANDLE_RAW: AtomicUsize = AtomicUsize::new(0);

static FILE_RECORD_CACHE_SEQ: AtomicU64 = AtomicU64::new(1);
static FILE_RECORD_CACHE: Mutex<Vec<FileRecordCacheEntry>> = Mutex::new(Vec::new());

static MOUNT_REQUESTED: AtomicBool = AtomicBool::new(false);
static MOUNT_QUEUE: Mutex<heapless::Vec<block::DeviceHandle, 8>> = Mutex::new(heapless::Vec::new());
static INDEX_REQUESTED: AtomicBool = AtomicBool::new(false);
static INDEX_QUEUE: Mutex<heapless::Vec<block::DeviceHandle, 8>> = Mutex::new(heapless::Vec::new());

struct FileWriteStream {
    disk: block::DeviceHandle,
    path: String,
    params: trueos_fs::FsParams,
    stream: trueos_fs::PutWriteStream,
}

static FILE_WRITE_STREAM_SEQ: AtomicU32 = AtomicU32::new(1);
static FILE_WRITE_STREAMS: Mutex<BTreeMap<u32, FileWriteStream>> = Mutex::new(BTreeMap::new());

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

fn request_mount_existing_visible_roots() {
    for disk in block::device_handles().into_iter() {
        let info = disk.info();
        if info.parent.is_none() && info.user_visible {
            request_mount_root(disk);
        }
    }
}

/// Background task that performs deferred TRUEOSFS probing/mounting.
/// Eagerly build the in-memory index for `disk` right after mounting, so later
/// callers (e.g. vhttps-cache) don't pay the log-replay cost on their first access.
async fn warm_index_async(disk: block::DeviceHandle) {
    let placement = match locate_async(disk).await {
        Ok(Some(p)) => p,
        _ => return,
    };
    if let Err(e) = ensure_index_async(disk, &placement).await {
        crate::log!("trueosfs: warm_index error {:?}\n", e);
    }
}

#[embassy_executor::task]
pub async fn mount_service_task() {
    async move {
        request_mount_existing_visible_roots();
        loop {
            if MOUNT_REQUESTED.swap(false, Ordering::AcqRel) {
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
                            request_warm_index(disk_id);
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
    .await;
}

#[embassy_executor::task]
pub async fn index_service_task() {
    async move {
        loop {
            if INDEX_REQUESTED.swap(false, Ordering::AcqRel) {
                let mut local: heapless::Vec<block::DeviceHandle, 8> = heapless::Vec::new();
                {
                    let mut q = INDEX_QUEUE.lock();
                    while let Some(d) = q.pop() {
                        let _ = local.push(d);
                    }
                }

                for disk in local.iter().copied() {
                    warm_index_async(disk).await;
                    Timer::after(EmbassyDuration::from_millis(1)).await;
                }
            }

            Timer::after(EmbassyDuration::from_millis(25)).await;
        }
    }
    .await;
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

fn trueosfs_trace_now_ms() -> u64 {
    let ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
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
        let total_bytes = bs.saturating_mul(blocks);
        let trace = crate::logflag::STORAGE_TRACE_LOGS && total_bytes >= 128 * 1024;
        let start_ms = trueosfs_trace_now_ms();
        let mut last_log_ms = start_ms;
        let mut last_log_bytes = 0usize;

        if trace {
            crate::log!(
                "trueosfs: block-read start disk={} lba={} blocks={} bytes={} bs={} max_blocks={} max_xfer={}\n",
                self.handle.id().raw(),
                lba,
                blocks,
                total_bytes,
                bs,
                max_blocks,
                info.max_transfer_bytes
            );
        }

        if blocks <= max_blocks {
            let out = self.handle.read_blocks(lba, blocks).await?;
            if trace {
                let now_ms = trueosfs_trace_now_ms();
                crate::log!(
                    "trueosfs: block-read progress disk={} lba={} done_blocks={} total_blocks={} done={} total={} elapsed_ms={}\n",
                    self.handle.id().raw(),
                    lba,
                    blocks,
                    blocks,
                    total_bytes,
                    total_bytes,
                    now_ms.saturating_sub(start_ms)
                );
                crate::log!(
                    "trueosfs: block-read done disk={} lba={} blocks={} bytes={} elapsed_ms={}\n",
                    self.handle.id().raw(),
                    lba,
                    blocks,
                    total_bytes,
                    now_ms.saturating_sub(start_ms)
                );
            }
            return Ok(out);
        }

        let mut out = Vec::with_capacity(bs.saturating_mul(blocks));
        let mut cur_lba = lba;
        let mut remaining = blocks;
        let mut done_blocks = 0usize;
        while remaining > 0 {
            let blocks_here = core::cmp::min(remaining, max_blocks);
            let tmp = self.handle.read_blocks(cur_lba, blocks_here).await?;
            out.extend_from_slice(&tmp);
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            remaining = remaining.saturating_sub(blocks_here);
            done_blocks = done_blocks.saturating_add(blocks_here);

            if trace {
                let done_bytes = done_blocks.saturating_mul(bs);
                let now_ms = trueosfs_trace_now_ms();
                if remaining == 0
                    || done_bytes.saturating_sub(last_log_bytes) >= 512 * 1024
                    || now_ms.saturating_sub(last_log_ms) >= 1000
                {
                    crate::log!(
                        "trueosfs: block-read progress disk={} lba={} done_blocks={} total_blocks={} done={} total={} elapsed_ms={}\n",
                        self.handle.id().raw(),
                        lba,
                        done_blocks,
                        blocks,
                        done_bytes,
                        total_bytes,
                        now_ms.saturating_sub(start_ms)
                    );
                    last_log_ms = now_ms;
                    last_log_bytes = done_bytes;
                }
            }
        }

        if trace {
            crate::log!(
                "trueosfs: block-read done disk={} lba={} blocks={} bytes={} elapsed_ms={}\n",
                self.handle.id().raw(),
                lba,
                blocks,
                total_bytes,
                trueosfs_trace_now_ms().saturating_sub(start_ms)
            );
        }

        Ok(out)
    }

    async fn read_blocks_into(
        &self,
        lba: u64,
        blocks: usize,
        dst: &mut [u8],
    ) -> Result<(), block::Error> {
        if blocks == 0 {
            return if dst.is_empty() {
                Ok(())
            } else {
                Err(block::Error::InvalidParam)
            };
        }

        let info = self.handle.info();
        let bs = info.block_size as usize;
        if bs == 0 {
            return Err(block::Error::InvalidParam);
        }

        let total_bytes = bs.checked_mul(blocks).ok_or(block::Error::InvalidParam)?;
        if dst.len() != total_bytes {
            return Err(block::Error::InvalidParam);
        }

        let max_blocks = if info.max_transfer_bytes > 0 {
            (info.max_transfer_bytes as usize / bs).max(1)
        } else {
            1
        };
        let trace = crate::logflag::STORAGE_TRACE_LOGS && total_bytes >= 128 * 1024;
        let start_ms = trueosfs_trace_now_ms();
        let mut last_log_ms = start_ms;
        let mut last_log_bytes = 0usize;

        if trace {
            crate::log!(
                "trueosfs: block-read start disk={} lba={} blocks={} bytes={} bs={} max_blocks={} max_xfer={}\n",
                self.handle.id().raw(),
                lba,
                blocks,
                total_bytes,
                bs,
                max_blocks,
                info.max_transfer_bytes
            );
        }

        if blocks <= max_blocks {
            self.handle.read_blocks_into(lba, blocks, dst).await?;
            if trace {
                let now_ms = trueosfs_trace_now_ms();
                crate::log!(
                    "trueosfs: block-read progress disk={} lba={} done_blocks={} total_blocks={} done={} total={} elapsed_ms={}\n",
                    self.handle.id().raw(),
                    lba,
                    blocks,
                    blocks,
                    total_bytes,
                    total_bytes,
                    now_ms.saturating_sub(start_ms)
                );
                crate::log!(
                    "trueosfs: block-read done disk={} lba={} blocks={} bytes={} elapsed_ms={}\n",
                    self.handle.id().raw(),
                    lba,
                    blocks,
                    total_bytes,
                    now_ms.saturating_sub(start_ms)
                );
            }
            return Ok(());
        }

        let mut cur_lba = lba;
        let mut remaining_blocks = blocks;
        let mut off = 0usize;
        let mut done_blocks = 0usize;
        while remaining_blocks > 0 {
            let blocks_here = core::cmp::min(remaining_blocks, max_blocks);
            let bytes_here = blocks_here * bs;
            self.handle
                .read_blocks_into(cur_lba, blocks_here, &mut dst[off..off + bytes_here])
                .await?;
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            remaining_blocks = remaining_blocks.saturating_sub(blocks_here);
            off = off.saturating_add(bytes_here);
            done_blocks = done_blocks.saturating_add(blocks_here);

            if trace {
                let done_bytes = done_blocks.saturating_mul(bs);
                let now_ms = trueosfs_trace_now_ms();
                if remaining_blocks == 0
                    || done_bytes.saturating_sub(last_log_bytes) >= 512 * 1024
                    || now_ms.saturating_sub(last_log_ms) >= 1000
                {
                    crate::log!(
                        "trueosfs: block-read progress disk={} lba={} done_blocks={} total_blocks={} done={} total={} elapsed_ms={}\n",
                        self.handle.id().raw(),
                        lba,
                        done_blocks,
                        blocks,
                        done_bytes,
                        total_bytes,
                        now_ms.saturating_sub(start_ms)
                    );
                    last_log_ms = now_ms;
                    last_log_bytes = done_bytes;
                }
            }
        }

        if trace {
            crate::log!(
                "trueosfs: block-read done disk={} lba={} blocks={} bytes={} elapsed_ms={}\n",
                self.handle.id().raw(),
                lba,
                blocks,
                total_bytes,
                trueosfs_trace_now_ms().saturating_sub(start_ms)
            );
        }

        Ok(())
    }

    async fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), block::Error> {
        if buf.is_empty() {
            return Ok(());
        }
        let info = self.handle.info();
        let bs = info.block_size as usize;
        if bs == 0 || !buf.len().is_multiple_of(bs) {
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
// NOTE: the synchronous `mount_root` wrapper was removed.
// Use `mount_root_async` (and call it from an async task/service).

/// Async variant of [`mount_root`].
///
/// Use this from async contexts to avoid `block_on` (which can starve other tasks such as USB polling).
pub async fn mount_root_async(
    disk: block::DeviceHandle,
) -> Result<Option<block::DiscId>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let Some(_placement) = locate_async(disk).await? else {
        return Ok(None);
    };

    let disk_id = disk.id();

    {
        let roots = ROOTS.lock();
        if roots.iter().any(|m| m.disk_id == disk_id) {
            return Ok(Some(disk_id));
        }
    }

    register_root_mount(disk, false);
    Ok(Some(disk_id))
}

/// Async remount path used after destructive operations that replace the on-disk
/// TRUEOSFS contents on an already-mounted disk.
///
/// Unlike [`mount_root_async`], this always refreshes the in-memory root mount
/// state for `disk` when TRUEOSFS is present so stale indexes and file-record
/// caches do not survive a format/install/update.
pub async fn remount_root_async(
    disk: block::DeviceHandle,
) -> Result<Option<block::DiscId>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let Some(_placement) = locate_async(disk).await? else {
        unregister_root_mount(disk.id());
        return Ok(None);
    };

    register_root_mount(disk, true);
    Ok(Some(disk.id()))
}

fn register_root_mount(disk: block::DeviceHandle, replace_existing: bool) {
    let disk_id = disk.id();
    let seq = ROOT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    let cache_gen = if replace_existing {
        let roots = ROOTS.lock();
        roots
            .iter()
            .find(|m| m.disk_id == disk_id)
            .map(|m| m.cache_gen.wrapping_add(1))
            .unwrap_or(0)
    } else {
        0
    };

    {
        let mut roots = ROOTS.lock();
        if let Some(existing) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
            if !replace_existing {
                return;
            }
            existing.seq = seq;
            existing.index = None;
            existing.building_index = false;
            existing.writes_since_checkpoint = 0;
            existing.cache_gen = cache_gen;
        } else {
            roots.push(RootMount {
                building_index: false,
                disk_id,
                seq,
                index: None,
                writes_since_checkpoint: 0,
                cache_gen,
            });
        }
    }

    PRIMARY_ROOT_RAW.store(disk_id.raw(), Ordering::Release);
    PRIMARY_ROOT_HANDLE_RAW.store(disk.into_raw(), Ordering::Release);

    file_record_cache_invalidate_disk(disk_id);

    crate::r::readiness::set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED);
}

fn unregister_root_mount(disk_id: block::DiscId) {
    let mut roots = ROOTS.lock();
    let before = roots.len();
    roots.retain(|m| m.disk_id != disk_id);
    if roots.len() == before {
        return;
    }
    drop(roots);

    file_record_cache_invalidate_disk(disk_id);

    let primary_raw = PRIMARY_ROOT_RAW.load(Ordering::Acquire);
    if primary_raw == disk_id.raw() {
        PRIMARY_ROOT_RAW.store(0, Ordering::Release);
        PRIMARY_ROOT_HANDLE_RAW.store(0, Ordering::Release);
    }
}

fn root_cache_gen(disk_id: block::DiscId) -> u32 {
    let roots = ROOTS.lock();
    roots
        .iter()
        .find(|m| m.disk_id == disk_id)
        .map(|m| m.cache_gen)
        .unwrap_or(0)
}

fn bump_root_cache_gen(disk_id: block::DiscId) {
    let mut roots = ROOTS.lock();
    if let Some(m) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
        m.cache_gen = m.cache_gen.wrapping_add(1);
    }
}

fn file_record_cache_lookup(
    disk_id: block::DiscId,
    path: &str,
) -> Option<trueos_fs::FileRecordRef> {
    let cache_gen = root_cache_gen(disk_id);
    let mut cache = FILE_RECORD_CACHE.lock();
    let mut idx = None;
    for (i, entry) in cache.iter().enumerate() {
        if entry.disk_id == disk_id && entry.path == path {
            idx = Some(i);
            break;
        }
    }

    let Some(i) = idx else {
        return None;
    };

    if cache[i].cache_gen != cache_gen {
        cache.remove(i);
        return None;
    }

    let seq = FILE_RECORD_CACHE_SEQ.fetch_add(1, Ordering::Relaxed);
    cache[i].last_use = seq;
    Some(cache[i].record)
}

fn file_record_cache_insert(disk_id: block::DiscId, path: &str, record: trueos_fs::FileRecordRef) {
    let cache_gen = root_cache_gen(disk_id);
    let seq = FILE_RECORD_CACHE_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut cache = FILE_RECORD_CACHE.lock();

    if let Some(pos) = cache
        .iter()
        .position(|entry| entry.disk_id == disk_id && entry.path == path)
    {
        cache.remove(pos);
    }

    if cache.len() >= FILE_RECORD_CACHE_CAP
        && let Some((evict_idx, _)) = cache
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.last_use)
    {
        cache.remove(evict_idx);
    }

    cache.push(FileRecordCacheEntry {
        disk_id,
        path: path.into(),
        record,
        cache_gen,
        last_use: seq,
    });
}

fn file_record_cache_invalidate_path(disk_id: block::DiscId, path: &str) {
    let mut cache = FILE_RECORD_CACHE.lock();
    cache.retain(|entry| !(entry.disk_id == disk_id && entry.path == path));
}

fn file_record_cache_invalidate_disk(disk_id: block::DiscId) {
    let mut cache = FILE_RECORD_CACHE.lock();
    cache.retain(|entry| entry.disk_id != disk_id);
}

fn invalidate_root_index(disk_id: block::DiscId) {
    let should_request_warm = {
        let mut roots = ROOTS.lock();
        if let Some(m) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
            m.index = None;
            !m.building_index
        } else {
            false
        }
    };

    if should_request_warm {
        request_warm_index(disk_id);
    }
}

fn update_root_index_put(
    disk_id: block::DiscId,
    path: &str,
    record: trueos_fs::FileRecordRef,
) -> bool {
    let mut roots = ROOTS.lock();
    let Some(mount) = roots.iter_mut().find(|m| m.disk_id == disk_id) else {
        return false;
    };
    let Some(index) = mount.index.as_mut() else {
        return false;
    };

    index.insert(
        path.as_bytes().to_vec(),
        IndexRef {
            kind: trueos_fs::LogKind::Put,
            entry_lba: record.entry_lba,
        },
    );
    mount.writes_since_checkpoint = mount.writes_since_checkpoint.saturating_add(1);
    true
}

fn update_root_index_delete(disk_id: block::DiscId, path: &str) -> bool {
    let mut roots = ROOTS.lock();
    let Some(mount) = roots.iter_mut().find(|m| m.disk_id == disk_id) else {
        return false;
    };
    let Some(index) = mount.index.as_mut() else {
        return false;
    };

    index.remove(path.as_bytes());
    mount.writes_since_checkpoint = mount.writes_since_checkpoint.saturating_add(1);
    true
}

fn snapshot_index_for_checkpoint(
    disk_id: block::DiscId,
) -> Option<Vec<(Vec<u8>, trueos_fs::LogKind, u64)>> {
    let roots = ROOTS.lock();
    let mount = roots.iter().find(|m| m.disk_id == disk_id)?;
    let index = mount.index.as_ref()?;
    let mut entries = Vec::with_capacity(index.len());
    for (key, index_ref) in index.iter() {
        entries.push((key.clone(), index_ref.kind, index_ref.entry_lba));
    }
    Some(entries)
}

fn note_checkpoint_written(disk_id: block::DiscId) {
    let mut roots = ROOTS.lock();
    if let Some(mount) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
        mount.writes_since_checkpoint = 0;
    }
}

async fn write_index_checkpoint_async(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
    replay_from_rel_blocks: u64,
) -> Result<bool, block::Error> {
    let disk_id = disk.id();
    let Some(entries) = snapshot_index_for_checkpoint(disk_id) else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let ok = trueos_fs::write_index_checkpoint(
        &io,
        &params,
        replay_from_rel_blocks,
        entries.into_iter(),
    )
    .await
    .map_err(map_engine_err)?;
    if ok {
        note_checkpoint_written(disk_id);
    }
    Ok(ok)
}

async fn maybe_checkpoint_built_index_async(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
    replay_from_rel_blocks: u64,
    end_rel_blocks: u64,
    had_checkpoint: bool,
    entry_count: usize,
) {
    let tail_blocks = end_rel_blocks.saturating_sub(replay_from_rel_blocks);
    if had_checkpoint && tail_blocks < TRUEOSFS_CHECKPOINT_MIN_TAIL_BLOCKS {
        return;
    }

    match write_index_checkpoint_async(disk, placement, end_rel_blocks).await {
        Ok(true) => {
            crate::log!(
                "trueosfs: index checkpoint written disk_id={} replay_from={} entries={}\n",
                disk.id().raw(),
                end_rel_blocks,
                entry_count
            );
        }
        Ok(false) => {
            crate::log!(
                "trueosfs: index checkpoint skipped disk_id={} reason=no-space-or-no-index\n",
                disk.id().raw()
            );
        }
        Err(e) => {
            crate::log!(
                "trueosfs: index checkpoint error disk_id={} err={:?}\n",
                disk.id().raw(),
                e
            );
        }
    }
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
    let Some(mut stream) =
        trueos_fs::begin_write_file_stream(&io, &params, name, bytes.len() as u64)
            .await
            .map_err(map_engine_err)?
    else {
        return Ok(false);
    };
    trueos_fs::write_file_stream_chunk(&io, &mut stream, bytes)
        .await
        .map_err(map_engine_err)?;
    let record = trueos_fs::write_stream_record_ref(&stream);
    trueos_fs::finish_write_file_stream(&io, &params, stream)
        .await
        .map_err(map_engine_err)?;

    let disk_id = disk.id();
    bump_root_cache_gen(disk_id);
    file_record_cache_invalidate_path(disk_id, name);
    file_record_cache_insert(disk_id, name, record);
    if !update_root_index_put(disk_id, name, record) {
        invalidate_root_index(disk_id);
    }
    Ok(true)
}

/// Async TRUEOSFS: begin a streamed write for `name` with known final byte length.
///
/// Returns:
/// - `Ok(Some(handle))` when the stream is created.
/// - `Ok(None)` when there is no space or no filesystem placement.
pub async fn file_write_begin_async(
    disk: block::DeviceHandle,
    name: &str,
    total_len: u64,
) -> Result<Option<u32>, block::Error> {
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
    let Some(stream) = trueos_fs::begin_write_file_stream(&io, &params, name, total_len)
        .await
        .map_err(map_engine_err)?
    else {
        return Ok(None);
    };

    let handle = FILE_WRITE_STREAM_SEQ.fetch_add(1, Ordering::Relaxed).max(1);
    let entry = FileWriteStream {
        disk,
        path: name.into(),
        params,
        stream,
    };
    FILE_WRITE_STREAMS.lock().insert(handle, entry);
    Ok(Some(handle))
}

/// Async TRUEOSFS: write a chunk into an open stream handle.
pub async fn file_write_chunk_async(stream_handle: u32, bytes: &[u8]) -> Result<(), block::Error> {
    let mut entry = {
        let mut streams = FILE_WRITE_STREAMS.lock();
        streams
            .remove(&stream_handle)
            .ok_or(block::Error::InvalidParam)?
    };

    let io = KernelBlockIo::new(entry.disk);
    let res = trueos_fs::write_file_stream_chunk(&io, &mut entry.stream, bytes)
        .await
        .map_err(map_engine_err);

    match res {
        Ok(()) => {
            FILE_WRITE_STREAMS.lock().insert(stream_handle, entry);
            Ok(())
        }
        Err(e) => {
            // On any chunk failure we abort by dropping stream state.
            Err(e)
        }
    }
}

/// Async TRUEOSFS: finish an open stream and publish the file atomically.
pub async fn file_write_finish_async(stream_handle: u32) -> Result<(), block::Error> {
    let entry = {
        let mut streams = FILE_WRITE_STREAMS.lock();
        streams
            .remove(&stream_handle)
            .ok_or(block::Error::InvalidParam)?
    };

    let io = KernelBlockIo::new(entry.disk);
    let record = trueos_fs::write_stream_record_ref(&entry.stream);
    trueos_fs::finish_write_file_stream(&io, &entry.params, entry.stream)
        .await
        .map_err(map_engine_err)?;

    let disk_id = entry.disk.id();
    bump_root_cache_gen(disk_id);
    file_record_cache_invalidate_path(disk_id, entry.path.as_str());
    file_record_cache_insert(disk_id, entry.path.as_str(), record);
    if !update_root_index_put(disk_id, entry.path.as_str(), record) {
        invalidate_root_index(disk_id);
    }
    Ok(())
}

/// Async TRUEOSFS: abort an open stream handle.
pub async fn file_write_abort_async(stream_handle: u32) -> Result<(), block::Error> {
    let removed = FILE_WRITE_STREAMS.lock().remove(&stream_handle);
    if removed.is_some() {
        Ok(())
    } else {
        Err(block::Error::InvalidParam)
    }
}

async fn lookup_via_index_async(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
    name: &str,
) -> Result<Option<trueos_fs::FileRecordRef>, block::Error> {
    ensure_index_async(disk, placement).await?;
    let disk_id = disk.id();

    let entry_lba = {
        let roots = ROOTS.lock();
        let Some(mount) = roots.iter().find(|m| m.disk_id == disk_id) else {
            return Ok(None);
        };
        let Some(index) = &mount.index else {
            return Ok(None);
        };
        match index.get(name.as_bytes()) {
            Some(entry) => {
                if entry.kind != trueos_fs::LogKind::Put {
                    return Ok(None);
                }
                entry.entry_lba
            }
            None => return Ok(None),
        }
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    trueos_fs::get_file_record_at(&io, &params, entry_lba, name)
        .await
        .map_err(map_engine_err)
}

/// Async TRUEOSFS: read a file.
///
/// Returns `Ok(None)` if missing.
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

    let disk_id = disk.id();
    let record = file_record_cache_lookup(disk_id, name);
    let record = match record {
        Some(v) => Some(v),
        None => {
            let rec = lookup_via_index_async(disk, &placement, name).await?;
            if let Some(r) = rec {
                file_record_cache_insert(disk_id, name, r);
                Some(r)
            } else {
                None
            }
        }
    };

    if let Some(rec) = record {
        return trueos_fs::read_file_at_record(&io, &params, &rec)
            .await
            .map_err(map_engine_err);
    }

    Ok(None)
}

/// Async TRUEOSFS: read a file only if the root index is already ready.
///
/// Unlike `file_out_async`, this will not trigger index construction on a cold root.
/// It is intended for opportunistic cache reads from latency-sensitive paths.
pub async fn file_out_if_index_ready_async(
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

    let disk_id = disk.id();
    let record = file_record_cache_lookup(disk_id, name);
    let record = match record {
        Some(v) => Some(v),
        None => {
            let mut should_warm = false;
            let entry_lba = {
                let roots = ROOTS.lock();
                match roots.iter().find(|m| m.disk_id == disk_id) {
                    Some(mount) => match &mount.index {
                        Some(index) => index.get(name.as_bytes()).map(|entry| entry.entry_lba),
                        None => {
                            should_warm = !mount.building_index;
                            None
                        }
                    },
                    None => return Ok(None),
                }
            };

            if let Some(entry_lba) = entry_lba {
                let rec = trueos_fs::get_file_record_at(&io, &params, entry_lba, name)
                    .await
                    .map_err(map_engine_err)?;
                if let Some(r) = rec {
                    file_record_cache_insert(disk_id, name, r);
                    Some(r)
                } else {
                    None
                }
            } else {
                if should_warm {
                    request_warm_index(disk_id);
                }
                None
            }
        }
    };

    if let Some(rec) = record {
        return trueos_fs::read_file_at_record(&io, &params, &rec)
            .await
            .map_err(map_engine_err);
    }

    Ok(None)
}

/// Async TRUEOSFS: read file metadata.
pub async fn file_info_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<Option<FileInfo>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate_async(disk).await? else {
        return Ok(None);
    };

    let disk_id = disk.id();
    let record = file_record_cache_lookup(disk_id, name);
    if let Some(rec) = record {
        return Ok(Some(trueos_fs::file_info_from_record(&rec)));
    }

    let rec = lookup_via_index_async(disk, &placement, name).await?;
    if let Some(r) = rec {
        file_record_cache_insert(disk_id, name, r);
        return Ok(Some(trueos_fs::file_info_from_record(&r)));
    }

    Ok(None)
}

/// Async TRUEOSFS: read a file range into a caller-provided buffer.
pub async fn file_read_range_async(
    disk: block::DeviceHandle,
    name: &str,
    offset: u64,
    out: &mut [u8],
) -> Result<Option<usize>, block::Error> {
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

    let disk_id = disk.id();
    let record = file_record_cache_lookup(disk_id, name);
    let record = match record {
        Some(v) => Some(v),
        None => {
            let rec = lookup_via_index_async(disk, &placement, name).await?;
            if let Some(r) = rec {
                file_record_cache_insert(disk_id, name, r);
                Some(r)
            } else {
                None
            }
        }
    };

    if let Some(rec) = record {
        return trueos_fs::read_file_range_at(&io, &params, &rec, offset, out)
            .await
            .map_err(map_engine_err);
    }

    Ok(None)
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

    let ok = trueos_fs::delete_file(&io, &params, name)
        .await
        .map_err(map_engine_err)?;
    if ok {
        let disk_id = disk.id();
        bump_root_cache_gen(disk_id);
        file_record_cache_invalidate_path(disk_id, name);
        if !update_root_index_delete(disk_id, name) {
            invalidate_root_index(disk_id);
        }
    }
    Ok(ok)
}

/// Async TRUEOSFS: best-effort rename (copy + delete).
///
/// Returns:
/// - `Ok(true)` if `src` was copied to `dst` (and `src` was best-effort deleted)
/// - `Ok(false)` if `src` is missing, `dst` already exists, or the filesystem is unavailable
pub async fn file_rename_async(
    disk: block::DeviceHandle,
    src: &str,
    dst: &str,
) -> Result<bool, block::Error> {
    if src == dst {
        return Ok(true);
    }

    // Disallow nested/partition handles.
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    // Conservative: never overwrite an existing destination.
    if file_exists_async(disk, dst).await? {
        return Ok(false);
    }

    let Some(bytes) = file_out_async(disk, src).await? else {
        return Ok(false);
    };

    let ok = file_in_async(disk, dst, bytes.as_slice()).await?;
    if !ok {
        return Ok(false);
    }

    // Best-effort cleanup; ignore failure.
    let _ = file_delete_async(disk, src).await;
    Ok(true)
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

    let disk_id = disk.id();

    // Check cache first for fast path
    if file_record_cache_lookup(disk_id, name).is_some() {
        return Ok(true);
    }

    // Check index (metadata lookup only)
    if lookup_via_index_async(disk, &placement, name)
        .await?
        .is_some()
    {
        return Ok(true);
    }

    Ok(false)
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

    let disk_id = disk.id();
    ensure_index_async(disk, &placement).await?;

    let roots = ROOTS.lock();
    let Some(mount) = roots.iter().find(|m| m.disk_id == disk_id) else {
        // Fallback if not mounted (should not happen if ensure_index succeeded)
        let params = trueos_fs::FsParams {
            super_lba: placement.super_lba,
            data_lba: placement.data_lba,
            data_end_lba_exclusive: placement.data_end_lba_exclusive,
        };
        let io = KernelBlockIo::new(disk);
        let out = trueos_fs::list_dir(&io, &params, dir)
            .await
            .map_err(map_engine_err)?;
        return Ok(Some(out));
    };

    let Some(index) = &mount.index else {
        return Err(block::Error::Corrupted);
    };

    let prefix = normalized_dir_prefix(dir);
    let prefix_bytes = prefix.as_bytes();

    let mut children: alloc::collections::BTreeSet<String> = alloc::collections::BTreeSet::new();

    if prefix.is_empty() {
        for key in index.keys() {
            if let Ok(name) = core::str::from_utf8(key) {
                let seg = name.split('/').next().unwrap_or("");
                if !seg.is_empty() {
                    children.insert(String::from(seg));
                }
            }
        }
    } else {
        for (key, _) in index.range(prefix_bytes.to_vec()..) {
            if !key.starts_with(prefix_bytes) {
                break;
            }
            if key.len() <= prefix_bytes.len() {
                continue;
            }
            if let Ok(rest_str) = core::str::from_utf8(&key[prefix_bytes.len()..]) {
                let seg = rest_str.split('/').next().unwrap_or("");
                if !seg.is_empty() {
                    children.insert(String::from(seg));
                }
            }
        }
    }

    const MAX_LISTING_BYTES: usize = 64 * 1024;
    let mut out = String::new();
    for entry in children.iter() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(entry);
        if out.len() > MAX_LISTING_BYTES {
            break;
        }
    }

    Ok(Some(out))
}

/// Async TRUEOSFS: report whether `dir` is represented by any indexed child path.
///
/// TRUEOSFS does not currently store directory records. Directories are therefore
/// meaningful only as path prefixes, plus empty directories represented by their
/// `.keep` marker at higher layers.
pub async fn dir_has_children_async(
    disk: block::DeviceHandle,
    dir: &str,
) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate_async(disk).await? else {
        return Ok(false);
    };

    let disk_id = disk.id();
    ensure_index_async(disk, &placement).await?;

    let roots = ROOTS.lock();
    let Some(mount) = roots.iter().find(|m| m.disk_id == disk_id) else {
        return Err(block::Error::NotReady);
    };
    let Some(index) = &mount.index else {
        return Err(block::Error::Corrupted);
    };

    let prefix = normalized_dir_prefix(dir);
    if prefix.is_empty() {
        return Ok(!index.is_empty());
    }

    let prefix_bytes = prefix.as_bytes();
    for (key, _) in index.range(prefix_bytes.to_vec()..) {
        if !key.starts_with(prefix_bytes) {
            break;
        }
        if key.len() > prefix_bytes.len() {
            return Ok(true);
        }
    }

    Ok(false)
}

fn normalized_dir_prefix(dir: &str) -> String {
    if dir.is_empty() || dir == "/" {
        return String::new();
    }
    let mut s = String::from(dir);
    if s.starts_with('/') {
        s.remove(0);
    }
    if s.ends_with('/') {
        s.pop();
    }
    if !s.is_empty() {
        s.push('/');
    }
    s
}

async fn ensure_index_async(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
) -> Result<(), block::Error> {
    let disk_id = disk.id();
    let start_cache_gen;

    // Claim a single builder slot; all others wait until index becomes available.
    loop {
        let mut roots = ROOTS.lock();
        match roots.iter_mut().find(|m| m.disk_id == disk_id) {
            None => return Err(block::Error::NotReady),
            Some(m) if m.index.is_some() => return Ok(()),
            Some(m) if m.building_index => {
                drop(roots);
                Timer::after(EmbassyDuration::from_millis(5)).await;
            }
            Some(m) => {
                m.building_index = true;
                start_cache_gen = m.cache_gen;
                break;
            }
        }
    }

    // Build outside lock.
    let build_result: Result<BuiltIndex, block::Error> =
        async {
            let params = trueos_fs::FsParams {
                super_lba: placement.super_lba,
                data_lba: placement.data_lba,
                data_end_lba_exclusive: placement.data_end_lba_exclusive,
            };
            let io = KernelBlockIo::new(disk);

            let mut tree = Box::new(BTreeMap::new());

            // Replay log.
            let sb_blk = read_blocks_aligned_async(disk, params.super_lba, 1).await?;
            let sb = trueos_fs::parse_superblock(&sb_blk).ok_or(block::Error::Corrupted)?;

            let mut replay_from = 0u64;
            let mut had_checkpoint = false;

            if let Ok(Some(ckpt)) = trueos_fs::read_index_checkpoint(&io, &params)
                .await
                .map_err(map_engine_err)
            {
                had_checkpoint = true;
                replay_from = ckpt.replay_from_rel_blocks;
                for (key, kind, lba) in ckpt.entries {
                    match kind {
                        trueos_fs::LogKind::Put => {
                            tree.insert(
                                key,
                                IndexRef {
                                    kind,
                                    entry_lba: lba,
                                },
                            );
                        }
                        trueos_fs::LogKind::Delete => {
                            tree.remove(&key);
                        }
                        _ => {}
                    }
                }
            }

            let end_rel = sb.log_head_rel_blocks;

            trueos_fs::replay_log_range(&io, &params, replay_from, end_rel, |kind, name, lba| {
                match kind {
                    trueos_fs::LogKind::Put => {
                        tree.insert(
                            name,
                            IndexRef {
                                kind,
                                entry_lba: lba,
                            },
                        );
                    }
                    trueos_fs::LogKind::Delete => {
                        tree.remove(&name);
                    }
                    _ => {}
                }
            })
            .await
            .map_err(map_engine_err)?;

            Ok(BuiltIndex {
                tree,
                replay_from_rel_blocks: replay_from,
                end_rel_blocks: end_rel,
                had_checkpoint,
            })
        }
        .await;

    // Always clear the build flag; publish the index only when no writer raced the build.
    let mut needs_rebuild = false;
    let mut checkpoint_after_publish = None;
    let result = match build_result {
        Ok(built) => {
            let BuiltIndex {
                tree,
                replay_from_rel_blocks,
                end_rel_blocks,
                had_checkpoint,
            } = built;
            let entry_count = tree.len();
            let mut roots = ROOTS.lock();
            if let Some(m) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
                if m.cache_gen == start_cache_gen {
                    m.index = Some(tree);
                    crate::r::readiness::set(crate::r::readiness::TRUEOSFS_INDEX_READY);
                    checkpoint_after_publish =
                        Some((replay_from_rel_blocks, end_rel_blocks, had_checkpoint, entry_count));
                } else {
                    needs_rebuild = true;
                }
                m.building_index = false;
            }
            Ok(())
        }
        Err(e) => {
            let mut roots = ROOTS.lock();
            if let Some(m) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
                m.building_index = false;
            }
            Err(e)
        }
    };

    if needs_rebuild {
        request_warm_index(disk_id);
        return Err(block::Error::NotReady);
    }

    if result.is_ok()
        && let Some((replay_from_rel_blocks, end_rel_blocks, had_checkpoint, entry_count)) =
            checkpoint_after_publish
    {
        maybe_checkpoint_built_index_async(
            disk,
            placement,
            replay_from_rel_blocks,
            end_rel_blocks,
            had_checkpoint,
            entry_count,
        )
        .await;
    }

    result
}

/// Best-effort: build an HTML `<ul>/<li>` tree of the TRUEOSFS directory structure.
///
/// Returns `Ok(None)` if the disk does not contain TRUEOSFS.
///
/// Notes:
/// - Traversal is capped (`max_entries`) to keep this usable for tiny HTTP responses.
/// - Uses the same HTML escaping guarantees as `trueos_math::Tree::html_tree_string`.
pub async fn html_tree_async(
    disk: block::DeviceHandle,
    max_entries: usize,
) -> Result<Option<String>, block::Error> {
    use alloc::string::String as AString;
    use alloc::{collections::BTreeMap, vec::Vec};
    use trueos_math::{NodeId, Tree};

    if max_entries == 0 {
        return Ok(Some(String::from("<ul></ul>")));
    }
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let Some(placement) = locate_async(disk).await? else {
        return Ok(None);
    };

    ensure_index_async(disk, &placement).await?;
    let disk_id = disk.id();

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum FsKind {
        Root,
        Dir,
        File,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FsEntry {
        kind: FsKind,
        name: AString,
    }

    // Keep memory bounded: CAP is the allocation for nodes/edges, while max_entries
    // constrains traversal and output size.
    const CAP: usize = 1024;
    let cap_limit = core::cmp::min(max_entries.saturating_add(1), CAP);

    let mut tree: Tree<FsEntry, CAP> = Tree::new();
    let Some(root) = tree.add_root(FsEntry {
        kind: FsKind::Root,
        name: AString::from("/"),
    }) else {
        return Ok(Some(String::from("<ul><li>alloc failed</li></ul>")));
    };

    let mut dir_nodes: BTreeMap<Vec<u8>, NodeId> = BTreeMap::new();
    dir_nodes.insert(Vec::new(), root);

    {
        let roots = ROOTS.lock();
        let Some(mount) = roots.iter().find(|m| m.disk_id == disk_id) else {
            return Err(block::Error::NotReady);
        };
        let Some(index) = &mount.index else {
            return Err(block::Error::Corrupted);
        };

        'files: for key in index.keys() {
            let Ok(path) = core::str::from_utf8(key.as_slice()) else {
                continue;
            };
            if path.is_empty() {
                continue;
            }

            let mut parent_node = root;
            let mut dir_path: Vec<u8> = Vec::new();
            let mut parts = path.split('/').filter(|seg| !seg.is_empty()).peekable();
            while let Some(seg) = parts.next() {
                let is_last = parts.peek().is_none();
                if is_last {
                    if tree.len() >= cap_limit {
                        break 'files;
                    }
                    if tree
                        .add_child(
                            parent_node,
                            FsEntry {
                                kind: FsKind::File,
                                name: AString::from(seg),
                            },
                        )
                        .is_none()
                    {
                        break 'files;
                    }
                    continue;
                }

                if !dir_path.is_empty() {
                    dir_path.push(b'/');
                }
                dir_path.extend_from_slice(seg.as_bytes());

                if let Some(existing) = dir_nodes.get(&dir_path).copied() {
                    parent_node = existing;
                    continue;
                }

                if tree.len() >= cap_limit {
                    break 'files;
                }
                let Some(node) = tree.add_child(
                    parent_node,
                    FsEntry {
                        kind: FsKind::Dir,
                        name: AString::from(seg),
                    },
                ) else {
                    break 'files;
                };
                dir_nodes.insert(dir_path.clone(), node);
                parent_node = node;
            }
        }
    }

    Ok(Some(tree.html_tree_string(root, |e, out| match e.kind {
        FsKind::Root => out.push('/'),
        FsKind::Dir => {
            out.push_str(e.name.as_str());
            out.push('/');
        }
        FsKind::File => out.push_str(e.name.as_str()),
    })))
}

fn push_json_string_escaped(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{1f}' => {
                let code = c as u32;
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.push_str("\\u00");
                out.push(HEX[((code >> 4) & 0x0f) as usize] as char);
                out.push(HEX[(code & 0x0f) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Async TRUEOSFS: return a compact broad-first JSON listing of the primary tree.
///
/// Returns `Ok(None)` if the disk does not contain TRUEOSFS.
pub async fn json_all_async(
    disk: block::DeviceHandle,
    max_entries: usize,
) -> Result<Option<String>, block::Error> {
    use alloc::collections::{BTreeMap, BTreeSet};
    use alloc::vec::Vec;

    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let Some(placement) = locate_async(disk).await? else {
        return Ok(None);
    };

    ensure_index_async(disk, &placement).await?;
    let disk_id = disk.id();
    let effective_limit = if max_entries == 0 {
        4096usize
    } else {
        max_entries
    };

    #[derive(Clone)]
    struct JsonEntry {
        depth: usize,
        path: String,
        kind: &'static str,
        name: String,
        id: u64,
    }

    let mut by_depth: BTreeMap<usize, Vec<JsonEntry>> = BTreeMap::new();
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();

    {
        let roots = ROOTS.lock();
        let Some(mount) = roots.iter().find(|m| m.disk_id == disk_id) else {
            return Err(block::Error::NotReady);
        };
        let Some(index) = &mount.index else {
            return Err(block::Error::Corrupted);
        };

        'scan: for (key, index_ref) in index.iter() {
            let Ok(path) = core::str::from_utf8(key.as_slice()) else {
                continue;
            };
            if path.is_empty() {
                continue;
            }

            let segments: Vec<&str> = path.split('/').filter(|seg| !seg.is_empty()).collect();
            if segments.is_empty() {
                continue;
            }

            for depth in 0..segments.len() {
                let rel_path = segments[..=depth].join("/");
                let rel_path_bytes = rel_path.as_bytes().to_vec();
                if !seen.insert(rel_path_bytes) {
                    continue;
                }

                by_depth.entry(depth).or_default().push(JsonEntry {
                    depth,
                    id: if depth + 1 == segments.len() {
                        index_ref.entry_lba
                    } else {
                        let marker = alloc::format!("{}/.keep", rel_path);
                        index
                            .get(marker.as_bytes())
                            .map(|entry| entry.entry_lba)
                            .unwrap_or(index_ref.entry_lba)
                    },
                    path: rel_path,
                    name: String::from(segments[depth]),
                    kind: if depth + 1 == segments.len() {
                        "file"
                    } else {
                        "dir"
                    },
                });

                let count = by_depth.values().map(|items| items.len()).sum::<usize>();
                if count >= effective_limit {
                    break 'scan;
                }
            }
        }
    }

    let total = by_depth.values().map(|items| items.len()).sum::<usize>();
    let truncated = total >= effective_limit;
    let mut written = 0usize;
    let mut out = String::new();
    out.push_str("{\"version\":1,\"root\":\"/\",\"max_entries\":");
    out.push_str(alloc::format!("{}", effective_limit).as_str());
    out.push_str(",\"truncated\":");
    out.push_str(if truncated { "true" } else { "false" });
    out.push_str(",\"entries\":[");

    let mut first = true;
    'write: for entries in by_depth.values() {
        for entry in entries.iter() {
            if written >= effective_limit {
                break 'write;
            }
            if !first {
                out.push(',');
            }
            first = false;
            out.push('{');
            out.push_str("\"path\":");
            push_json_string_escaped(&mut out, entry.path.as_str());
            out.push_str(",\"name\":");
            push_json_string_escaped(&mut out, entry.name.as_str());
            out.push_str(",\"kind\":");
            push_json_string_escaped(&mut out, entry.kind);
            out.push_str(",\"depth\":");
            out.push_str(alloc::format!("{}", entry.depth).as_str());
            out.push_str(",\"id\":");
            out.push_str(alloc::format!("{}", entry.id).as_str());
            out.push('}');
            written += 1;
        }
    }

    out.push_str("]}");
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

    if append_bytes.is_empty() {
        return Ok(true);
    }

    let mut bytes = match trueos_fs::read_file(&io, &params, name)
        .await
        .map_err(map_engine_err)?
    {
        Some(existing) => existing,
        None => Vec::new(),
    };
    bytes.extend_from_slice(append_bytes);

    let Some(mut stream) =
        trueos_fs::begin_write_file_stream(&io, &params, name, bytes.len() as u64)
            .await
            .map_err(map_engine_err)?
    else {
        return Ok(false);
    };
    trueos_fs::write_file_stream_chunk(&io, &mut stream, bytes.as_slice())
        .await
        .map_err(map_engine_err)?;
    let record = trueos_fs::write_stream_record_ref(&stream);
    trueos_fs::finish_write_file_stream(&io, &params, stream)
        .await
        .map_err(map_engine_err)?;

    let disk_id = disk.id();
    bump_root_cache_gen(disk_id);
    file_record_cache_invalidate_path(disk_id, name);
    file_record_cache_insert(disk_id, name, record);
    if !update_root_index_put(disk_id, name, record) {
        invalidate_root_index(disk_id);
    }
    Ok(true)
}

// NOTE: synchronous TRUEOSFS file operations (`file_in`, `file_out`, etc.) were removed.
// Use the async entrypoints above.

// NOTE: Root index construction/checkpointing was part of the old synchronous TRUEOSFS path.
// The async mount path intentionally avoids this (it would require blocking I/O).

pub fn roots_len() -> usize {
    ROOTS.lock().len()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootInfo {
    pub disk_id: block::DiscId,
    pub seq: u32,
    pub index_ready: bool,
    pub index_building: bool,
}

/// Returns a snapshot list of mounted TRUEOSFS roots.
///
/// Sorted by descending mount sequence (newest first).
pub fn list_roots() -> Vec<RootInfo> {
    use core::cmp::Reverse;

    let roots = ROOTS.lock();
    let index_queue = INDEX_QUEUE.lock();
    let mut out: Vec<RootInfo> = Vec::with_capacity(roots.len());
    for m in roots.iter() {
        out.push(RootInfo {
            disk_id: m.disk_id,
            seq: m.seq,
            index_ready: m.index.is_some(),
            index_building: m.building_index || index_queue.iter().any(|d| d.id() == m.disk_id),
        });
    }
    out.sort_by_key(|r| Reverse(r.seq));
    out
}

pub fn root_index_paths(disk_id: block::DiscId, max_paths: usize) -> Option<Vec<String>> {
    let roots = ROOTS.lock();
    let mount = roots.iter().find(|m| m.disk_id == disk_id)?;
    let index = mount.index.as_ref()?;

    let mut out = Vec::new();
    for key in index.keys() {
        if out.len() >= max_paths {
            break;
        }
        if let Ok(path) = core::str::from_utf8(key.as_slice())
            && !path.is_empty()
        {
            out.push(String::from(path));
        }
    }
    Some(out)
}

pub async fn raw_log_scan_async(
    disk: block::DeviceHandle,
    max_records: usize,
) -> Result<Option<trueos_fs::RawLogScan>, block::Error> {
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
    trueos_fs::scan_raw_log(&io, &params, max_records)
        .await
        .map(Some)
        .map_err(map_engine_err)
}

pub fn request_warm_index(disk_id: block::DiscId) {
    let Some(disk) = block::device_handle(disk_id) else {
        return;
    };

    {
        let roots = ROOTS.lock();
        if roots
            .iter()
            .find(|m| m.disk_id == disk_id)
            .is_some_and(|m| m.index.is_some() || m.building_index)
        {
            return;
        }
    }

    {
        let mut q = INDEX_QUEUE.lock();
        if q.iter().any(|d| d.id() == disk_id) {
            return;
        }
        if q.push(disk).is_err() {
            crate::log!("trueosfs: index queue full disk_id={}\n", disk_id.raw());
            return;
        }
    }

    INDEX_REQUESTED.store(true, Ordering::Release);
}

/// Returns the most recently mounted TRUEOSFS root disk id (best-effort).
///
/// This is used by higher layers (shell, C ABI helpers) that want a sensible
/// default filesystem target without user-facing mount plumbing.
pub fn primary_root_id() -> Option<block::DiscId> {
    let cached_handle = PRIMARY_ROOT_HANDLE_RAW.load(Ordering::Acquire);
    if cached_handle != 0 {
        let disk = unsafe { block::DeviceHandle::from_raw(cached_handle) };
        let disk_id = disk.id();
        PRIMARY_ROOT_RAW.store(disk_id.raw(), Ordering::Release);
        return Some(disk_id);
    }

    let cached = PRIMARY_ROOT_RAW.load(Ordering::Acquire);
    if cached != 0 {
        let disk_id = block::DiscId::from_raw(cached);
        if block::device_handle(disk_id).is_some() {
            return Some(disk_id);
        }
        PRIMARY_ROOT_RAW.store(0, Ordering::Release);
        PRIMARY_ROOT_HANDLE_RAW.store(0, Ordering::Release);
    }

    let roots = ROOTS.lock();
    let picked = roots.iter().max_by_key(|m| m.seq).map(|m| m.disk_id);
    if let Some(disk_id) = picked {
        PRIMARY_ROOT_RAW.store(disk_id.raw(), Ordering::Release);
        if let Some(disk) = block::device_handle(disk_id) {
            PRIMARY_ROOT_HANDLE_RAW.store(disk.into_raw(), Ordering::Release);
        }
    }
    picked
}

/// Returns a handle for the most recently mounted TRUEOSFS root disk.
pub fn primary_root_handle() -> Option<block::DeviceHandle> {
    let cached = PRIMARY_ROOT_HANDLE_RAW.load(Ordering::Acquire);
    if cached != 0 {
        return Some(unsafe { block::DeviceHandle::from_raw(cached) });
    }

    let disk = primary_root_id().and_then(block::device_handle);
    if let Some(handle) = disk {
        PRIMARY_ROOT_HANDLE_RAW.store(handle.into_raw(), Ordering::Release);
    }
    disk
}

/// Returns read-only state for the current primary TRUEOSFS root disk.
pub fn primary_root_is_read_only() -> Option<bool> {
    primary_root_handle().map(|h| h.info().is_read_only())
}

pub fn root_seq(disk_id: block::DiscId) -> Option<u32> {
    let roots = ROOTS.lock();
    roots.iter().find(|m| m.disk_id == disk_id).map(|m| m.seq)
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

fn looks_like_trueos_superblock(block0: &[u8]) -> bool {
    block0.len() >= 8 && block0[0..8] == trueos_fs::MAGIC
}

fn is_transient_io(e: block::Error) -> bool {
    matches!(e, block::Error::NotReady | block::Error::Timeout | block::Error::Io)
}

#[inline]
fn is_nvme_handle(handle: block::DeviceHandle) -> bool {
    handle.info().kind == block::DeviceKind::Nvme
}

// NOTE: the synchronous `locate` helper was removed.
// Use `locate_async`.

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
    let attempts = if is_nvme_handle(handle) {
        // On a wedged NVMe IO queue, repeated retries just enqueue more doomed
        // commands and amplify timeout storms. Fail fast for probe-time reads.
        attempts.min(1)
    } else {
        attempts
    };

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
    let err = last.unwrap_or(block::Error::Io);
    if is_nvme_handle(handle) {
        crate::log!(
            "trueosfs: read-retry failed dev={} lba={} blocks={} attempts={} err={:?}\n",
            handle.id(),
            lba,
            blocks,
            attempts,
            err
        );
    }
    Err(err)
}

/// Find where TRUEOSFS lives on a whole disk.
///
/// This avoids `block_on` so it can be called from async contexts (e.g. installer jobs).
pub async fn locate_async(
    handle: block::DeviceHandle,
) -> Result<Option<TrueosFsPlacement>, block::Error> {
    if handle.parent().is_some() {
        let bs0 = read_blocks_aligned_retry_async(handle, 0, 1, 3).await?;
        if looks_like_trueos_superblock(&bs0) {
            return Ok(Some(TrueosFsPlacement {
                bootable: false,
                super_lba: 0,
                data_lba: trueos_fs::data_lba_from_super(0),
                data_end_lba_exclusive: Some(handle.info().block_count),
            }));
        }
        return Ok(None);
    }

    // Prefer GPT-partitioned layouts (bootable-capable).
    {
        let max_gpt_tries = if is_nvme_handle(handle) { 1 } else { 5 };
        let mut tries = 0u8;
        while tries < max_gpt_tries {
            match partition::read_gpt_partitions(handle).await {
                Ok(parts) => {
                    let mut has_esp = false;
                    for p in parts.iter() {
                        if p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES {
                            has_esp = true;
                        }

                        // Our superblock is at the start of the TRUEOS data partition.
                        if let Ok(p0) =
                            read_blocks_aligned_retry_async(handle, p.range.first_lba(), 1, 3).await
                            && looks_like_trueos_superblock(&p0)
                        {
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
                    break;
                }
                Err(e) if is_transient_io(e) => {
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                }
                Err(e) => {
                    if is_nvme_handle(handle) {
                        crate::log!(
                            "trueosfs: locate stage=read_gpt_partitions dev={} err={:?}\n",
                            handle.id(),
                            e
                        );
                    }
                    return Err(e);
                }
            }
            tries = tries.wrapping_add(1);
        }
    }

    // Fallback: superblock at LBA0 (data-only images/disks).
    let bs0 = match read_blocks_aligned_retry_async(handle, 0, 1, 3).await {
        Ok(v) => v,
        Err(e) => {
            if is_nvme_handle(handle) {
                crate::log!(
                    "trueosfs: locate stage=read_lba0_super dev={} err={:?}\n",
                    handle.id(),
                    e
                );
            }
            return Err(e);
        }
    };
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

// NOTE: the synchronous `format_blank` wrapper was removed.
// Use `format_blank_async`.

/// Force-format the whole disk as data-only TRUEOSFS (superblock at LBA0).
///
/// This is intentionally destructive and is intended for interactive/debug use
/// (e.g. the shell `format` command) after explicit user confirmation.
///
/// Unlike [`format_blank`], this will *not* refuse to proceed when a GPT with an
/// ESP exists. It also wipes the primary/backup GPT headers best-effort so that
/// subsequent detection doesn't keep treating the disk as a GPT layout.
// NOTE: the synchronous `format_blank_force` wrapper was removed.
// Use `format_blank_force_async`.

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

    format_blank_at_async(handle, 0).await?;
    if handle.info().user_visible {
        request_mount_root(handle);
    }
    Ok(())
}

fn validate_blank_format_args(
    handle: block::DeviceHandle,
    super_lba: u64,
    allow_partition: bool,
) -> Result<(block::DeviceInfo, usize, usize, usize), block::Error> {
    if !allow_partition && handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    if info.block_count < TRUEOSFS_MIN_TOTAL_BLOCKS {
        return Err(block::Error::InvalidParam);
    }
    if super_lba >= info.block_count {
        return Err(block::Error::OutOfBounds);
    }

    let data_lba = trueos_fs::data_lba_from_super(super_lba);
    if data_lba >= info.block_count {
        return Err(block::Error::OutOfBounds);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        (info.max_transfer_bytes as usize / bs).max(1)
    } else {
        1
    };
    let align = info.dma_alignment.max(1) as usize;
    Ok((info, bs, max_blocks, align))
}

pub async fn validate_private_medium_async(
    handle: block::DeviceHandle,
    expect_super_lba: u64,
) -> Result<TrueosFsPlacement, block::Error> {
    let Some(placement) = locate_async(handle).await? else {
        return Err(block::Error::Corrupted);
    };
    if placement.super_lba != expect_super_lba {
        return Err(block::Error::Corrupted);
    }
    if placement.data_lba != trueos_fs::data_lba_from_super(expect_super_lba) {
        return Err(block::Error::Corrupted);
    }
    Ok(placement)
}

pub async fn validate_public_medium_async(
    handle: block::DeviceHandle,
    expect_super_lba: u64,
) -> Result<TrueosFsPlacement, block::Error> {
    let Some(placement) = locate_async(handle).await? else {
        return Err(block::Error::Corrupted);
    };
    if placement.super_lba != expect_super_lba {
        return Err(block::Error::Corrupted);
    }
    if placement.data_lba != trueos_fs::data_lba_from_super(expect_super_lba) {
        return Err(block::Error::Corrupted);
    }
    Ok(placement)
}

/// Format TRUEOSFS at the start of an already-created partition.
///
/// This is intended for installer code that first creates a GPT layout and then
/// formats the TRUEOS data partition without clobbering LBA0 of the whole disk.
// NOTE: the synchronous `format_blank_partition` wrapper was removed.
// Use `format_blank_partition_async`.

/// Async variant of [`format_blank_partition`].
pub async fn format_blank_partition_async(
    partition: block::DeviceHandle,
) -> Result<(), block::Error> {
    if partition.parent().is_none() {
        return Err(block::Error::InvalidParam);
    }
    if !partition.supports_write() {
        return Err(block::Error::NotSupported);
    }
    format_blank_at_async(partition, 0).await
}

pub(crate) async fn format_blank_at_async(
    handle: block::DeviceHandle,
    super_lba: u64,
) -> Result<(), block::Error> {
    let (info, bs, max_blocks, align) =
        validate_blank_format_args(handle, super_lba, handle.parent().is_some())?;
    let blocks = core::cmp::min(8usize, max_blocks);
    let bytes = bs.saturating_mul(blocks);

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
    let placement = validate_private_medium_async(handle, super_lba).await?;
    if placement.super_lba != super_lba {
        return Err(block::Error::Corrupted);
    }

    // Best-effort end-to-end NVMe sanity check: write a tiny payload into the data region
    // and read it back.
    if info.kind == block::DeviceKind::Nvme {
        let data_lba = trueos_fs::data_lba_from_super(super_lba);
        if data_lba.saturating_add(2) <= info.block_count {
            let blocks_verify = core::cmp::min(2usize, max_blocks);
            let bytes_verify = bs.saturating_mul(blocks_verify);
            let mut tmp2 =
                AlignedBuf::new(bytes_verify, align).ok_or(block::Error::DmaUnavailable)?;
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
