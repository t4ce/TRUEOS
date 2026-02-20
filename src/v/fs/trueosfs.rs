use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};

use crate::disc::block;
use crate::v::disc::partition;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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
    writes_since_checkpoint: u32,
    cache_gen: u32,
}

static ROOT_SEQ: AtomicU32 = AtomicU32::new(0);
static ROOTS: Mutex<Vec<RootMount>> = Mutex::new(Vec::new());

static FILE_RECORD_CACHE_SEQ: AtomicU64 = AtomicU64::new(1);
static FILE_RECORD_CACHE: Mutex<Vec<FileRecordCacheEntry>> = Mutex::new(Vec::new());

static MOUNT_REQUESTED: AtomicBool = AtomicBool::new(false);
static MOUNT_QUEUE: Mutex<heapless::Vec<block::DeviceHandle, 8>> = Mutex::new(heapless::Vec::new());

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

/// Background task that performs deferred TRUEOSFS probing/mounting.
#[embassy_executor::task]
pub async fn mount_service_task() {
    async move {
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

    // IMPORTANT: do not call `build_root_index()` here.
    // `build_root_index()` uses the synchronous TRUEOSFS engine which calls into
    // `KernelBlockIo` and ultimately a synchronous wait wrapper. Doing that from
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
        seq: ROOT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1),
        index,
        writes_since_checkpoint,
        cache_gen: 0,
    });

    file_record_cache_invalidate_disk(disk_id);

    crate::v::readiness::set(crate::v::readiness::TRUEOSFS_ROOT_MOUNTED);

    Ok(Some(disk_id))
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
    let mut roots = ROOTS.lock();
    if let Some(m) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
        m.index = None;
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

    let ok = trueos_fs::write_file(&io, &params, name, bytes)
        .await
        .map_err(map_engine_err)?;
    if ok {
        let disk_id = disk.id();
        bump_root_cache_gen(disk_id);
        file_record_cache_invalidate_path(disk_id, name);
        invalidate_root_index(disk_id);
    }
    Ok(ok)
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
    trueos_fs::finish_write_file_stream(&io, &entry.params, entry.stream)
        .await
        .map_err(map_engine_err)?;

    let disk_id = entry.disk.id();
    bump_root_cache_gen(disk_id);
    file_record_cache_invalidate_path(disk_id, entry.path.as_str());
    invalidate_root_index(disk_id);
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
        invalidate_root_index(disk_id);
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
    invalidate_root_index(disk.id());
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
    if let Some(_) = lookup_via_index_async(disk, &placement, name).await? {
        // We could cache this here, but lookup_via_index_async returns FsRecordRef,
        // and if we cache it, we avoid re-reading the header later.
        // But here we return bool. Since lookup_via_index_async does IO (reads header),
        // we probably should cache it if we could?
        // lookup_via_index_async doesn't currently cache.
        // Let's stick to true/false.
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

    // Normalize dir path
    let prefix = if dir.is_empty() || dir == "/" {
        String::new()
    } else {
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
    };
    let prefix_bytes = prefix.as_bytes();

    let mut children: alloc::collections::BTreeSet<String> = alloc::collections::BTreeSet::new();

    // Iterate BTreeMap
    // BTreeMap supports range, but keys are Vec<u8> paths.
    // Iterating everything is still faster than disk I/O, but we can optimize with range().

    // Range start: prefix (e.g. "foo/")
    // Range end: prefix + next char
    // Simple iteration for now is robust.
    for (key, _) in index.iter() {
        if !prefix.is_empty() {
            if !key.starts_with(prefix_bytes) {
                continue;
            }
            if key.len() <= prefix_bytes.len() {
                continue;
            }
            let rest = &key[prefix_bytes.len()..];
            // The key is a Vec<u8>, assuming utf-8 valid paths
            if let Ok(rest_str) = core::str::from_utf8(rest) {
                let seg = rest_str.split('/').next().unwrap_or("");
                if !seg.is_empty() {
                    children.insert(String::from(seg));
                }
            }
        } else {
            if let Ok(name) = core::str::from_utf8(key) {
                let seg = name.split('/').next().unwrap_or("");
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

async fn ensure_index_async(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
) -> Result<(), block::Error> {
    let disk_id = disk.id();

    // Check if we need to build
    {
        let roots = ROOTS.lock();
        if let Some(mount) = roots.iter().find(|m| m.disk_id == disk_id) {
            if mount.index.is_some() {
                return Ok(());
            }
        } else {
            // Not mounted?
            return Err(block::Error::NotReady);
        }
    }

    // Build outside lock
    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    let mut tree = Box::new(BTreeMap::new());

    // Replay log
    let sb_blk = read_blocks_aligned_async(disk, params.super_lba, 1).await?;
    let sb = trueos_fs::parse_superblock(&sb_blk).ok_or(block::Error::Corrupted)?;

    let mut replay_from = 0u64;

    if let Ok(Some(ckpt)) = trueos_fs::read_index_checkpoint(&io, &params)
        .await
        .map_err(map_engine_err)
    {
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

    trueos_fs::replay_log_range(
        &io,
        &params,
        replay_from,
        end_rel,
        |kind, name, lba| match kind {
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
        },
    )
    .await
    .map_err(map_engine_err)?;

    // Store in mount
    let mut roots = ROOTS.lock();
    if let Some(mount) = roots.iter_mut().find(|m| m.disk_id == disk_id) {
        mount.index = Some(tree);
    }

    Ok(())
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
    use alloc::vec::Vec;
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

    let _params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let _io = KernelBlockIo::new(disk);

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

    // BFS-ish: keeps early siblings visible when we hit the cap.
    let mut queue: Vec<(NodeId, AString)> = Vec::new();
    queue.push((root, AString::new()));

    while let Some((parent, path)) = queue.pop() {
        if tree.len() >= cap_limit {
            break;
        }

        let listing = list_dir_async(disk, path.as_str())
            .await?
            .unwrap_or_default();

        for name in listing.lines() {
            if tree.len() >= cap_limit {
                break;
            }

            let name = name.trim();
            if name.is_empty() {
                continue;
            }

            let child_path = if path.is_empty() {
                AString::from(name)
            } else {
                let mut p = path.clone();
                p.push('/');
                p.push_str(name);
                p
            };

            let is_file = file_exists_async(disk, child_path.as_str()).await?;
            let kind = if is_file { FsKind::File } else { FsKind::Dir };

            let Some(node) = tree.add_child(
                parent,
                FsEntry {
                    kind: kind.clone(),
                    name: AString::from(name),
                },
            ) else {
                break;
            };

            if matches!(kind, FsKind::Dir) {
                queue.insert(0, (node, child_path));
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

    let ok = trueos_fs::append_file(&io, &params, name, append_bytes)
        .await
        .map_err(map_engine_err)?;
    if ok {
        let disk_id = disk.id();
        bump_root_cache_gen(disk_id);
        file_record_cache_invalidate_path(disk_id, name);
        invalidate_root_index(disk_id);
    }
    Ok(ok)
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
}

/// Returns a snapshot list of mounted TRUEOSFS roots.
///
/// Sorted by descending mount sequence (newest first).
pub fn list_roots() -> Vec<RootInfo> {
    use core::cmp::Reverse;

    let roots = ROOTS.lock();
    let mut out: Vec<RootInfo> = Vec::with_capacity(roots.len());
    for m in roots.iter() {
        out.push(RootInfo {
            disk_id: m.disk_id,
            seq: m.seq,
        });
    }
    out.sort_by_key(|r| Reverse(r.seq));
    out
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
    matches!(
        e,
        block::Error::NotReady | block::Error::Timeout | block::Error::Io
    )
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
                            && looks_like_trueos_superblock(&p0) {
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

    format_blank_at_async(handle, 0).await
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
