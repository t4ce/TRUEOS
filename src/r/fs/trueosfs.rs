use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};

use crate::disc::block;
use crate::r::disc::partition;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;
use trueos_time::{Duration as EmbassyDuration, Timer};

pub use trueos_fs::{ContentTypeId, DirEntry, FileInfo, NodeInfo, NodeKind, RecordKey};

/// A bounded, sorted directory listing.  `truncated` is out-of-band so an
/// on-disk filename can never be mistaken for a pagination sentinel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirListing {
    pub entries: Vec<DirEntry>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IndexRef {
    kind: trueos_fs::LogKind,
    entry_lba: u64,
    content_type: ContentTypeId,
}

// Switch to alloc::collections::BTreeMap for full delete support
// type TrueosFsIndex = BPlusTree<Vec<u8>, IndexRef, TRUEOSFS_INDEX_M>;
type TrueosFsIndex = BTreeMap<Vec<u8>, IndexRef>;

const FILE_RECORD_CACHE_CAP: usize = 64;
const TRUEOSFS_CHECKPOINT_MIN_TAIL_BLOCKS: u64 = 4096;
pub const TRUEOSFS_LIST_SOFT_CAP: usize = 1024;

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
    placement: TrueosFsPlacement,
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

static CONTENT_TYPED_COMMITS: AtomicU64 = AtomicU64::new(0);
static CONTENT_LEGACY_BLOB_COMMITS: AtomicU64 = AtomicU64::new(0);
static CONTENT_EXPLICIT_BLOB_IMPORTS: AtomicU64 = AtomicU64::new(0);
static CONTENT_REJECTS: AtomicU64 = AtomicU64::new(0);
static CONTENT_REJECT_UNSUPPORTED_INGRESS: AtomicU64 = AtomicU64::new(0);
static CONTENT_REJECT_EVIDENCE_MISMATCH: AtomicU64 = AtomicU64::new(0);
static CONTENT_REJECT_TYPE_REQUIRED: AtomicU64 = AtomicU64::new(0);
static CONTENT_REJECT_UNREGISTERED: AtomicU64 = AtomicU64::new(0);
static CONTENT_REJECT_LEGACY_DOWNGRADE: AtomicU64 = AtomicU64::new(0);
static CONTENT_REJECT_TYPE_MISMATCH: AtomicU64 = AtomicU64::new(0);
static CONTENT_REJECT_OTHER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentIdentityRejectReason {
    UnsupportedIngress,
    EvidenceMismatch,
    TypeRequired,
    UnregisteredType,
    LegacyDowngrade,
    TypeMismatch,
    Other,
}

impl ContentIdentityRejectReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedIngress => "unsupported-ingress",
            Self::EvidenceMismatch => "evidence-mismatch",
            Self::TypeRequired => "type-required",
            Self::UnregisteredType => "unregistered-type",
            Self::LegacyDowngrade => "legacy-downgrade",
            Self::TypeMismatch => "type-mismatch",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentIdentityDecisionSnapshot {
    pub typed_commits: u64,
    pub legacy_blob_commits: u64,
    pub explicit_blob_imports: u64,
    pub rejects: u64,
    pub reject_unsupported_ingress: u64,
    pub reject_evidence_mismatch: u64,
    pub reject_type_required: u64,
    pub reject_unregistered_type: u64,
    pub reject_legacy_downgrade: u64,
    pub reject_type_mismatch: u64,
    pub reject_other: u64,
}

pub fn content_identity_decisions() -> ContentIdentityDecisionSnapshot {
    ContentIdentityDecisionSnapshot {
        typed_commits: CONTENT_TYPED_COMMITS.load(Ordering::Relaxed),
        legacy_blob_commits: CONTENT_LEGACY_BLOB_COMMITS.load(Ordering::Relaxed),
        explicit_blob_imports: CONTENT_EXPLICIT_BLOB_IMPORTS.load(Ordering::Relaxed),
        rejects: CONTENT_REJECTS.load(Ordering::Relaxed),
        reject_unsupported_ingress: CONTENT_REJECT_UNSUPPORTED_INGRESS.load(Ordering::Relaxed),
        reject_evidence_mismatch: CONTENT_REJECT_EVIDENCE_MISMATCH.load(Ordering::Relaxed),
        reject_type_required: CONTENT_REJECT_TYPE_REQUIRED.load(Ordering::Relaxed),
        reject_unregistered_type: CONTENT_REJECT_UNREGISTERED.load(Ordering::Relaxed),
        reject_legacy_downgrade: CONTENT_REJECT_LEGACY_DOWNGRADE.load(Ordering::Relaxed),
        reject_type_mismatch: CONTENT_REJECT_TYPE_MISMATCH.load(Ordering::Relaxed),
        reject_other: CONTENT_REJECT_OTHER.load(Ordering::Relaxed),
    }
}

pub fn record_explicit_blob_import() {
    CONTENT_EXPLICIT_BLOB_IMPORTS.fetch_add(1, Ordering::Relaxed);
    crate::log_info!(target: "storage";
        "trueosfs: content import type={} explicit_blob=true decision=accepted\n",
        ContentTypeId::BLOB.raw(),
    );
}

pub fn record_type_reject(reason: ContentIdentityRejectReason) {
    CONTENT_REJECTS.fetch_add(1, Ordering::Relaxed);
    match reason {
        ContentIdentityRejectReason::UnsupportedIngress => {
            CONTENT_REJECT_UNSUPPORTED_INGRESS.fetch_add(1, Ordering::Relaxed);
        }
        ContentIdentityRejectReason::EvidenceMismatch => {
            CONTENT_REJECT_EVIDENCE_MISMATCH.fetch_add(1, Ordering::Relaxed);
        }
        ContentIdentityRejectReason::TypeRequired => {
            CONTENT_REJECT_TYPE_REQUIRED.fetch_add(1, Ordering::Relaxed);
        }
        ContentIdentityRejectReason::UnregisteredType => {
            CONTENT_REJECT_UNREGISTERED.fetch_add(1, Ordering::Relaxed);
        }
        ContentIdentityRejectReason::LegacyDowngrade => {
            CONTENT_REJECT_LEGACY_DOWNGRADE.fetch_add(1, Ordering::Relaxed);
        }
        ContentIdentityRejectReason::TypeMismatch => {
            CONTENT_REJECT_TYPE_MISMATCH.fetch_add(1, Ordering::Relaxed);
        }
        ContentIdentityRejectReason::Other => {
            CONTENT_REJECT_OTHER.fetch_add(1, Ordering::Relaxed);
        }
    }
    crate::log_important!(target: "storage";
        "trueosfs: content-type reject reason={} decision=rejected\n",
        reason.as_str(),
    );
}

fn record_successful_content_commit(content_type: ContentTypeId, legacy: bool) {
    if legacy {
        CONTENT_LEGACY_BLOB_COMMITS.fetch_add(1, Ordering::Relaxed);
    } else {
        CONTENT_TYPED_COMMITS.fetch_add(1, Ordering::Relaxed);
    }
    crate::log_info!(target: "storage";
        "trueosfs: content commit type={} legacy={} decision=accepted\n",
        content_type.raw(), legacy,
    );
}

static MOUNT_REQUESTED: AtomicBool = AtomicBool::new(false);
static MOUNT_QUEUE: Mutex<heapless::Vec<block::DeviceHandle, 8>> = Mutex::new(heapless::Vec::new());
static INDEX_REQUESTED: AtomicBool = AtomicBool::new(false);
static INDEX_QUEUE: Mutex<heapless::Vec<block::DeviceHandle, 8>> = Mutex::new(heapless::Vec::new());

struct FileWriteStream {
    disk: block::DeviceHandle,
    path: String,
    params: trueos_fs::FsParams,
    stream: trueos_fs::PutWriteStream,
    legacy_blob: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct FileReadHandle {
    disk: block::DeviceHandle,
    params: trueos_fs::FsParams,
    record: trueos_fs::FileRecordRef,
}

impl FileReadHandle {
    #[inline]
    pub const fn data_len(&self) -> u64 {
        self.record.data_len
    }

    #[inline]
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn data_lba(&self) -> u64 {
        self.record.data_lba
    }
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

    crate::log_info!(target: "trueosfs";
        "trueosfs: diag phase=mount-request disk={} blocks={} block_size={} user_visible={}\n",
        disk.id().raw(),
        disk.info().block_count,
        disk.info().block_size,
        disk.info().user_visible,
    );
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

/// Eagerly build the in-memory index for `disk` right after mounting, so later
/// callers (e.g. vhttps-cache) don't pay the log-replay cost on their first access.
async fn warm_index_async(disk: block::DeviceHandle) {
    let placement = match placement_for_io_async(disk).await {
        Ok(Some(p)) => p,
        _ => return,
    };
    if let Err(e) = ensure_index_async(disk, &placement).await {
        crate::log_info!(target: "trueosfs";
            "trueosfs: diag phase=index-warm-error disk={} err={:?}\n",
            disk.id().raw(), e
        );
    }
}

/// Background task that performs deferred TRUEOSFS probing and mounting.
#[trueos_executor::task]
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
                    crate::log_info!(target: "trueosfs";
                        "trueosfs: diag phase=mount-dequeue disk={} batch={}\n",
                        disk.id().raw(), local.len()
                    );
                    // Best-effort: only log when we actually mount or error.
                    match mount_root_async(disk).await {
                        Ok(Some(disk_id)) => {
                            crate::log_info!(target: "trueosfs";
                                "trueosfs: diag phase=mount-complete disk={}\n", disk_id.raw()
                            );
                            request_warm_index(disk_id);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            crate::log_info!(target: "trueosfs";
                                "trueosfs: diag phase=mount-error disk={} err={:?}\n",
                                disk.id().raw(), e
                            );
                        }
                    }
                    trueosfs_boot_work_yield().await;
                }
            }

            Timer::after(EmbassyDuration::from_millis(50)).await;
        }
    }
    .await;
}

#[trueos_executor::task]
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
                    crate::log_info!(target: "trueosfs";
                        "trueosfs: diag phase=index-dequeue disk={} batch={}\n",
                        disk.id().raw(), local.len()
                    );
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    fair_boot_work: bool,
}

impl KernelBlockIo {
    #[inline]
    fn new(handle: block::DeviceHandle) -> Self {
        Self {
            handle,
            fair_boot_work: false,
        }
    }

    #[inline]
    fn new_fair_boot_work(handle: block::DeviceHandle) -> Self {
        Self {
            handle,
            fair_boot_work: true,
        }
    }

    async fn fair_yield(&self) {
        if self.fair_boot_work {
            trueosfs_boot_work_yield().await;
        }
    }
}

async fn trueosfs_boot_work_yield() {
    Timer::after(EmbassyDuration::from_millis(
        crate::allcaps::storage::TRUEOSFS_BOOT_WORK_YIELD_MS,
    ))
    .await;
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
        let trace = crate::log_os::flags::STORAGE_TRACE_LOGS && total_bytes >= 128 * 1024;
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
            self.fair_yield().await;
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
            self.fair_yield().await;
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
        let trace = crate::log_os::flags::STORAGE_TRACE_LOGS && total_bytes >= 128 * 1024;
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
            self.fair_yield().await;
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
            self.fair_yield().await;
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

        let start_ms = trueosfs_trace_now_ms();
        crate::log_trace!(target: "trueosfs";
            "trueosfs: block-write start disk={} lba={} bytes={} bs={} max_blocks={} max_xfer={}\n",
            self.handle.id().raw(),
            lba,
            buf.len(),
            bs,
            max_blocks,
            info.max_transfer_bytes
        );

        let mut cur_lba = lba;
        let mut off = 0usize;
        while off < buf.len() {
            let remaining = buf.len() - off;
            let blocks_here = core::cmp::min(max_blocks, remaining / bs);
            let bytes_here = blocks_here * bs;
            crate::log_trace!(target: "trueosfs";
                "trueosfs: block-write chunk disk={} lba={} blocks={} bytes={} off={}\n",
                self.handle.id().raw(),
                cur_lba,
                blocks_here,
                bytes_here,
                off
            );
            self.handle
                .write_blocks(cur_lba, &buf[off..off + bytes_here])
                .await?;
            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            off = off.saturating_add(bytes_here);
        }

        crate::log_trace!(target: "trueosfs";
            "trueosfs: block-write done disk={} lba={} bytes={} elapsed_ms={}\n",
            self.handle.id().raw(),
            lba,
            buf.len(),
            trueosfs_trace_now_ms().saturating_sub(start_ms)
        );
        Ok(())
    }

    #[inline]
    async fn flush(&self) -> Result<(), block::Error> {
        let start_ms = trueosfs_trace_now_ms();
        crate::log_trace!(target: "trueosfs"; "trueosfs: block-flush start disk={}\n", self.handle.id().raw());
        let out = self.handle.flush().await;
        crate::log_trace!(target: "trueosfs";
            "trueosfs: block-flush done disk={} result={:?} elapsed_ms={}\n",
            self.handle.id().raw(),
            out,
            trueosfs_trace_now_ms().saturating_sub(start_ms)
        );
        out
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

/// Asynchronously ensure a single TRUEOSFS root exists for this *whole disk*.
///
/// Returns:
/// - `Ok(Some(disk_id))` if the disk contains TRUEOSFS and is now registered
/// - `Ok(None)` if the disk does not contain TRUEOSFS
/// - `Err(_)` on I/O or invalid param
///
/// Driver attach paths should enqueue [`request_mount_root`] instead of waiting
/// here, so the executor remains available to service the underlying device.
pub async fn mount_root_async(
    disk: block::DeviceHandle,
) -> Result<Option<block::DiscId>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let locate_started_ms = trueosfs_trace_now_ms();
    crate::log_info!(target: "trueosfs";
        "trueosfs: diag phase=locate-start disk={}\n", disk.id().raw()
    );
    let located = locate_async(disk).await;
    let locate_elapsed_ms = trueosfs_trace_now_ms().saturating_sub(locate_started_ms);
    let located = match located {
        Ok(located) => located,
        Err(e) => {
            crate::log_info!(target: "trueosfs";
                "trueosfs: diag phase=locate-error disk={} elapsed_ms={} err={:?}\n",
                disk.id().raw(), locate_elapsed_ms, e
            );
            return Err(e);
        }
    };
    let Some(placement) = located else {
        crate::log_info!(target: "trueosfs";
            "trueosfs: diag phase=locate-miss disk={} elapsed_ms={}\n",
            disk.id().raw(), locate_elapsed_ms
        );
        return Ok(None);
    };

    crate::log_info!(target: "trueosfs";
        "trueosfs: diag phase=locate-found disk={} elapsed_ms={} super_lba={} data_lba={} data_end={:?} bootable={}\n",
        disk.id().raw(), locate_elapsed_ms, placement.super_lba, placement.data_lba,
        placement.data_end_lba_exclusive, placement.bootable
    );

    let disk_id = disk.id();

    {
        let roots = ROOTS.lock();
        if roots.iter().any(|m| m.disk_id == disk_id) {
            return Ok(Some(disk_id));
        }
    }

    register_root_mount(disk, placement, false);
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

    let Some(placement) = locate_async(disk).await? else {
        unregister_root_mount(disk.id());
        return Ok(None);
    };

    register_root_mount(disk, placement, true);
    Ok(Some(disk.id()))
}

fn register_root_mount(
    disk: block::DeviceHandle,
    placement: TrueosFsPlacement,
    replace_existing: bool,
) {
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
            } else {
                existing.seq = seq;
                existing.placement = placement;
                existing.index = None;
                existing.building_index = false;
                existing.writes_since_checkpoint = 0;
                existing.cache_gen = cache_gen;
            }
        } else {
            roots.push(RootMount {
                building_index: false,
                disk_id,
                placement,
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
    crate::log_info!(target: "trueosfs";
        "trueosfs: diag phase=root-mounted-published disk={} seq={} cache_gen={} super_lba={} data_lba={}\n",
        disk_id.raw(), seq, cache_gen, placement.super_lba, placement.data_lba
    );
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

fn root_placement(disk_id: block::DiscId) -> Option<TrueosFsPlacement> {
    let roots = ROOTS.lock();
    roots
        .iter()
        .find(|m| m.disk_id == disk_id)
        .map(|m| m.placement)
}

async fn placement_for_io_async(
    disk: block::DeviceHandle,
) -> Result<Option<TrueosFsPlacement>, block::Error> {
    if let Some(placement) = root_placement(disk.id()) {
        return Ok(Some(placement));
    }
    locate_async(disk).await
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

fn file_record_cache_invalidate_prefix(disk_id: block::DiscId, prefix: &str) {
    let mut cache = FILE_RECORD_CACHE.lock();
    cache.retain(|entry| !(entry.disk_id == disk_id && entry.path.starts_with(prefix)));
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
            content_type: record.content_type,
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

fn update_root_index_rename_tree(
    disk_id: block::DiscId,
    moves: &[(String, String, IndexRef)],
) -> bool {
    let mut roots = ROOTS.lock();
    let Some(mount) = roots.iter_mut().find(|m| m.disk_id == disk_id) else {
        return false;
    };
    let Some(index) = mount.index.as_mut() else {
        return false;
    };

    for (src, _, _) in moves {
        index.remove(src.as_bytes());
    }
    for (_, dst, index_ref) in moves {
        index.insert(dst.as_bytes().to_vec(), *index_ref);
    }
    mount.writes_since_checkpoint = mount.writes_since_checkpoint.saturating_add(1);
    true
}

fn apply_index_rename_tree(index: &mut TrueosFsIndex, src_dir: &str, dst_dir: &str) {
    let src_prefix = normalized_dir_prefix(src_dir);
    let dst_prefix = normalized_dir_prefix(dst_dir);
    if src_prefix.is_empty() || dst_prefix.is_empty() || dst_prefix.starts_with(src_prefix.as_str())
    {
        return;
    }

    let mut moves: Vec<(Vec<u8>, Vec<u8>, IndexRef)> = Vec::new();
    if let Some(index_ref) = index.get(src_dir.as_bytes()) {
        moves.push((src_dir.as_bytes().to_vec(), dst_dir.as_bytes().to_vec(), *index_ref));
    }
    for (key, index_ref) in index.range(src_prefix.as_bytes().to_vec()..) {
        if !key.starts_with(src_prefix.as_bytes()) {
            break;
        }
        let suffix = &key[src_prefix.len()..];
        if suffix.is_empty() {
            continue;
        }
        let mut dst = dst_prefix.as_bytes().to_vec();
        dst.extend_from_slice(suffix);
        moves.push((key.clone(), dst, *index_ref));
    }

    for (src, _, _) in moves.iter() {
        index.remove(src);
    }
    for (_, dst, index_ref) in moves {
        index.insert(dst, index_ref);
    }
}

fn snapshot_index_for_checkpoint(
    disk_id: block::DiscId,
) -> Option<Vec<(Vec<u8>, trueos_fs::LogKind, u64, ContentTypeId)>> {
    let roots = ROOTS.lock();
    let mount = roots.iter().find(|m| m.disk_id == disk_id)?;
    let index = mount.index.as_ref()?;
    let mut entries = Vec::with_capacity(index.len());
    for (key, index_ref) in index.iter() {
        entries.push((key.clone(), index_ref.kind, index_ref.entry_lba, index_ref.content_type));
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
        crate::log_info!(target: "trueosfs";
            "trueosfs: diag phase=post-publish-checkpoint disk={} action=skip reason=tail-below-threshold tail_blocks={} threshold_blocks={} entries={}\n",
            disk.id().raw(), tail_blocks, TRUEOSFS_CHECKPOINT_MIN_TAIL_BLOCKS, entry_count
        );
        return;
    }

    crate::log_info!(target: "trueosfs";
        "trueosfs: diag phase=post-publish-checkpoint disk={} action=write replay_from={} tail_blocks={} had_checkpoint={} entries={}\n",
        disk.id().raw(), end_rel_blocks, tail_blocks, had_checkpoint, entry_count
    );

    match write_index_checkpoint_async(disk, placement, end_rel_blocks).await {
        Ok(true) => {
            crate::log_info!(target: "trueosfs";
                "trueosfs: diag phase=post-publish-checkpoint disk={} action=written replay_from={} entries={}\n",
                disk.id().raw(),
                end_rel_blocks,
                entry_count
            );
        }
        Ok(false) => {
            crate::log_info!(target: "trueosfs";
                "trueosfs: diag phase=post-publish-checkpoint disk={} action=skip reason=no-space-or-no-index\n",
                disk.id().raw()
            );
        }
        Err(e) => {
            crate::log_info!(target: "trueosfs";
                "trueosfs: diag phase=post-publish-checkpoint disk={} action=error err={:?}\n",
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
    let record_key = file_info_async(disk, name)
        .await?
        .map(|info| info.record_key)
        .unwrap_or(RecordKey::Ffa);
    file_in_with_metadata_async(disk, name, bytes, ContentTypeId::BLOB, record_key, true).await
}

pub async fn file_in_typed_async(
    disk: block::DeviceHandle,
    name: &str,
    bytes: &[u8],
    content_type: ContentTypeId,
) -> Result<bool, block::Error> {
    file_in_with_metadata_async(
        disk,
        name,
        bytes,
        content_type,
        file_info_async(disk, name)
            .await?
            .map(|i| i.record_key)
            .unwrap_or(RecordKey::Ffa),
        false,
    )
    .await
}

async fn file_in_with_metadata_async(
    disk: block::DeviceHandle,
    name: &str,
    bytes: &[u8],
    content_type: ContentTypeId,
    record_key: RecordKey,
    legacy_blob: bool,
) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if content_type == ContentTypeId::NONE {
        record_type_reject(ContentIdentityRejectReason::TypeRequired);
        return Err(block::Error::InvalidParam);
    }
    if !content_type.is_registered() {
        record_type_reject(ContentIdentityRejectReason::UnregisteredType);
        return Err(block::Error::InvalidParam);
    }
    if legacy_blob
        && file_info_async(disk, name)
            .await?
            .is_some_and(|info| info.content_type != ContentTypeId::BLOB)
    {
        record_type_reject(ContentIdentityRejectReason::LegacyDowngrade);
        return Err(block::Error::InvalidParam);
    }
    prepare_file_target_async(disk, name).await?;
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let Some(mut stream) = trueos_fs::begin_write_file_stream_with_metadata(
        &io,
        &params,
        name,
        bytes.len() as u64,
        trueos_fs::FileWriteMetadata {
            content_type,
            record_key,
        },
    )
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
    record_successful_content_commit(record.content_type, legacy_blob);

    let disk_id = disk.id();
    bump_root_cache_gen(disk_id);
    file_record_cache_invalidate_path(disk_id, name);
    file_record_cache_insert(disk_id, name, record);
    if !update_root_index_put(disk_id, name, record) {
        invalidate_root_index(disk_id);
    }
    Ok(true)
}

#[deprecated(note = "use file_in_typed_async")]
pub async fn file_in_with_key_async(
    disk: block::DeviceHandle,
    name: &str,
    bytes: &[u8],
    record_key: RecordKey,
) -> Result<bool, block::Error> {
    file_in_with_metadata_async(disk, name, bytes, ContentTypeId::BLOB, record_key, true).await
}

/// Asynchronously materialize every directory prefix using TRUEOSFS marker files.
///
/// Returns `Ok(false)` if the filesystem cannot allocate a required marker.
pub async fn dir_create_all_async(
    disk: block::DeviceHandle,
    path: &str,
) -> Result<bool, block::Error> {
    let mut prefix = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);

        match node_info_async(disk, prefix.as_str()).await? {
            Some(info) if info.kind == NodeKind::Directory => continue,
            Some(_) => return Err(block::Error::InvalidParam),
            None => {}
        }
        if !create_directory_async(disk, prefix.as_str()).await? {
            return Ok(false);
        }
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
    let record_key = file_info_async(disk, name)
        .await?
        .map(|info| info.record_key)
        .unwrap_or(RecordKey::Ffa);
    file_write_begin_with_metadata_async(
        disk,
        name,
        total_len,
        ContentTypeId::BLOB,
        record_key,
        true,
    )
    .await
}

pub async fn file_write_begin_typed_async(
    disk: block::DeviceHandle,
    name: &str,
    total_len: u64,
    content_type: ContentTypeId,
) -> Result<Option<u32>, block::Error> {
    let record_key = file_info_async(disk, name)
        .await?
        .map(|i| i.record_key)
        .unwrap_or(RecordKey::Ffa);
    file_write_begin_with_metadata_async(disk, name, total_len, content_type, record_key, false).await
}

/// Begin a streamed file write with an explicit native record key.
pub async fn file_write_begin_with_key_async(
    disk: block::DeviceHandle,
    name: &str,
    total_len: u64,
    record_key: RecordKey,
) -> Result<Option<u32>, block::Error> {
    file_write_begin_with_metadata_async(disk, name, total_len, ContentTypeId::BLOB, record_key, true)
        .await
}

async fn file_write_begin_with_metadata_async(
    disk: block::DeviceHandle,
    name: &str,
    total_len: u64,
    content_type: ContentTypeId,
    record_key: RecordKey,
    legacy_blob: bool,
) -> Result<Option<u32>, block::Error> {
    crate::log!(
        "trueosfs: file-write-begin stage=start disk={} path={} bytes={}\n",
        disk.id().raw(),
        name,
        total_len
    );
    if disk.parent().is_some() {
        crate::log!(
            "trueosfs: file-write-begin failed stage=start disk={} err=InvalidParamParent\n",
            disk.id().raw()
        );
        return Err(block::Error::InvalidParam);
    }
    if content_type == ContentTypeId::NONE {
        record_type_reject(ContentIdentityRejectReason::TypeRequired);
        return Err(block::Error::InvalidParam);
    }
    if !content_type.is_registered() {
        record_type_reject(ContentIdentityRejectReason::UnregisteredType);
        return Err(block::Error::InvalidParam);
    }
    if legacy_blob
        && file_info_async(disk, name)
            .await?
            .is_some_and(|info| info.content_type != ContentTypeId::BLOB)
    {
        record_type_reject(ContentIdentityRejectReason::LegacyDowngrade);
        return Err(block::Error::InvalidParam);
    }
    prepare_file_target_async(disk, name).await?;
    crate::log!("trueosfs: file-write-begin stage=locate disk={}\n", disk.id().raw());
    let Some(placement) = placement_for_io_async(disk).await? else {
        crate::log!(
            "trueosfs: file-write-begin failed stage=locate disk={} err=no-placement\n",
            disk.id().raw()
        );
        return Ok(None);
    };
    crate::log!(
        "trueosfs: file-write-begin stage=engine disk={} super_lba={} data_lba={} data_end={:?}\n",
        disk.id().raw(),
        placement.super_lba,
        placement.data_lba,
        placement.data_end_lba_exclusive
    );

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let Some(stream) = trueos_fs::begin_write_file_stream_with_metadata(
        &io,
        &params,
        name,
        total_len,
        trueos_fs::FileWriteMetadata {
            content_type,
            record_key,
        },
    )
    .await
    .map_err(map_engine_err)?
    else {
        crate::log!(
            "trueosfs: file-write-begin failed stage=engine disk={} err=no-space\n",
            disk.id().raw()
        );
        return Ok(None);
    };

    let handle = FILE_WRITE_STREAM_SEQ.fetch_add(1, Ordering::Relaxed).max(1);
    let entry = FileWriteStream {
        disk,
        path: name.into(),
        params,
        stream,
        legacy_blob,
    };
    FILE_WRITE_STREAMS.lock().insert(handle, entry);
    crate::log!(
        "trueosfs: file-write-begin success disk={} handle={} path={} bytes={}\n",
        disk.id().raw(),
        handle,
        name,
        total_len
    );
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
    record_successful_content_commit(record.content_type, entry.legacy_blob);

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

/// Asynchronously write a complete file from a BSP-owned executor task.
///
/// Filesystem consumers that already run in async kernel context must use this
/// native path. The synchronous `kfs` facade is a compatibility bridge for AP
/// blocking lanes and must never be entered from the BSP executor.
///
/// Returns `Ok(false)` when the filesystem cannot allocate the file.
pub async fn file_write_all_async(
    disk: block::DeviceHandle,
    name: &str,
    bytes: &[u8],
) -> Result<bool, block::Error> {
    let Some(handle) = file_write_begin_async(disk, name, bytes.len() as u64).await? else {
        return Ok(false);
    };

    for chunk in bytes.chunks(64 * 1024) {
        if let Err(error) = file_write_chunk_async(handle, chunk).await {
            // A failed chunk is removed by `file_write_chunk_async`; abort is
            // still useful if that implementation later retains failed state.
            let _ = file_write_abort_async(handle).await;
            return Err(error);
        }
    }

    file_write_finish_async(handle).await?;
    Ok(true)
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

    Ok(trueos_fs::get_node_record_by_lba(&io, &params, entry_lba)
        .await
        .map_err(map_engine_err)?
        .and_then(|record| {
            (record.kind == NodeKind::File).then_some(trueos_fs::FileRecordRef {
                entry_lba: record.entry_lba,
                data_lba: record.data_lba,
                data_len: record.data_len,
                content_type: record.content_type,
                record_key: record.record_key,
            })
        }))
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
    let Some(placement) = placement_for_io_async(disk).await? else {
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub async fn file_out_if_index_ready_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<Option<Vec<u8>>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
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
                let rec = trueos_fs::get_node_record_by_lba(&io, &params, entry_lba)
                    .await
                    .map_err(map_engine_err)?
                    .and_then(|record| {
                        (record.kind == NodeKind::File).then_some(trueos_fs::FileRecordRef {
                            entry_lba: record.entry_lba,
                            data_lba: record.data_lba,
                            data_len: record.data_len,
                            content_type: record.content_type,
                            record_key: record.record_key,
                        })
                    });
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
pub async fn node_info_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<Option<NodeInfo>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if name.is_empty() || name == "/" {
        return Ok(Some(NodeInfo {
            kind: NodeKind::Directory,
            data_len: 0,
            content_type: ContentTypeId::NONE,
            record_key: RecordKey::Ffa,
        }));
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(None);
    };
    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    trueos_fs::read_node_info(&KernelBlockIo::new(disk), &params, name)
        .await
        .map_err(map_engine_err)
}

pub async fn dir_exists_async(disk: block::DeviceHandle, path: &str) -> Result<bool, block::Error> {
    Ok(node_info_async(disk, path)
        .await?
        .is_some_and(|info| info.kind == NodeKind::Directory))
}

pub async fn create_directory_async(
    disk: block::DeviceHandle,
    path: &str,
) -> Result<bool, block::Error> {
    if path.is_empty() || path == "/" {
        return Ok(true);
    }
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(false);
    };
    if let Some((parent, _)) = path.rsplit_once('/')
        && !parent.is_empty()
        && !dir_exists_async(disk, parent).await?
    {
        return Err(block::Error::InvalidParam);
    }
    match node_info_async(disk, path).await? {
        Some(info) if info.kind == NodeKind::Directory => return Ok(true),
        Some(_) => return Err(block::Error::InvalidParam),
        None => {}
    }
    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let ok = trueos_fs::create_directory(&KernelBlockIo::new(disk), &params, path)
        .await
        .map_err(map_engine_err)?;
    if ok {
        bump_root_cache_gen(disk.id());
        invalidate_root_index(disk.id());
    }
    Ok(ok)
}

async fn prepare_file_target_async(
    disk: block::DeviceHandle,
    path: &str,
) -> Result<(), block::Error> {
    if let Some((parent, _)) = path.rsplit_once('/')
        && !parent.is_empty()
        && !dir_create_all_async(disk, parent).await?
    {
        return Err(block::Error::Io);
    }
    if node_info_async(disk, path)
        .await?
        .is_some_and(|info| info.kind == NodeKind::Directory)
    {
        return Err(block::Error::InvalidParam);
    }
    Ok(())
}

pub async fn file_info_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<Option<FileInfo>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(None);
    };

    if node_info_async(disk, name)
        .await?
        .is_some_and(|info| info.kind != NodeKind::File)
    {
        return Ok(None);
    }

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

/// Async TRUEOSFS: open a file for repeated range reads.
///
/// This pins the current placement and file record, avoiding repeated root
/// location and index/cache lookups for seek-heavy readers.
pub async fn file_read_open_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<Option<FileReadHandle>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(None);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };

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

    Ok(record.map(|record| FileReadHandle {
        disk,
        params,
        record,
    }))
}

/// Async TRUEOSFS: read a file range through an open read handle.
pub async fn file_read_handle_range_async(
    handle: FileReadHandle,
    offset: u64,
    out: &mut [u8],
) -> Result<Option<usize>, block::Error> {
    let io = KernelBlockIo::new(handle.disk);
    trueos_fs::read_file_range_at(&io, &handle.params, &handle.record, offset, out)
        .await
        .map_err(map_engine_err)
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
    let Some(placement) = placement_for_io_async(disk).await? else {
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
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    let disk_id = disk.id();
    let record = match file_record_cache_lookup(disk_id, name) {
        Some(record) => Some(record),
        None => {
            let record = lookup_via_index_async(disk, &placement, name).await?;
            if let Some(record) = record {
                file_record_cache_insert(disk_id, name, record);
            }
            record
        }
    };
    let Some(record) = record else {
        return Ok(false);
    };

    let ok = trueos_fs::delete_file_at_record(&io, &params, name, &record)
        .await
        .map_err(map_engine_err)?;
    if ok {
        bump_root_cache_gen(disk_id);
        file_record_cache_invalidate_path(disk_id, name);
        if !update_root_index_delete(disk_id, name) {
            invalidate_root_index(disk_id);
        }
    }
    Ok(ok)
}

/// Remove a file or directory, recursively for directories.
pub async fn remove_recursive_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<bool, block::Error> {
    if name.is_empty() || name == "/" || disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(false);
    };
    let Some(record) = trueos_fs::lookup_node_record(
        &KernelBlockIo::new(disk),
        &trueos_fs::FsParams {
            super_lba: placement.super_lba,
            data_lba: placement.data_lba,
            data_end_lba_exclusive: placement.data_end_lba_exclusive,
        },
        name,
    )
    .await
    .map_err(map_engine_err)?
    else {
        return Ok(false);
    };
    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let ok = match record.kind {
        NodeKind::File => trueos_fs::delete_node_at_record(&io, &params, name, &record).await,
        NodeKind::Directory => trueos_fs::delete_tree(&io, &params, name).await,
    }
    .map_err(map_engine_err)?;
    if ok {
        bump_root_cache_gen(disk.id());
        file_record_cache_invalidate_prefix(disk.id(), normalized_dir_prefix(name).as_str());
        file_record_cache_invalidate_path(disk.id(), name);
        invalidate_root_index(disk.id());
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
    if node_info_async(disk, dst).await?.is_some() {
        return Ok(false);
    }

    let Some(source_record) = lookup_file_record_async(disk, src).await? else {
        return Ok(false);
    };
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(false);
    };
    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let ok = trueos_fs::copy_file_from_record(&io, &params, src, &source_record, dst)
        .await
        .map_err(map_engine_err)?;
    if !ok {
        return Ok(false);
    }

    // The engine copy is a direct physical operation, so publish the new
    // record into every kernel-side view before removing the old logical name.
    let Some(destination_record) = trueos_fs::lookup_file_record(&io, &params, dst)
        .await
        .map_err(map_engine_err)?
    else {
        invalidate_root_index(disk.id());
        return Err(block::Error::Corrupted);
    };
    record_successful_content_commit(destination_record.content_type, false);
    bump_root_cache_gen(disk.id());
    file_record_cache_invalidate_path(disk.id(), dst);
    file_record_cache_insert(disk.id(), dst, destination_record);
    if !update_root_index_put(disk.id(), dst, destination_record) {
        invalidate_root_index(disk.id());
    }

    // Best-effort cleanup; ignore failure.
    let _ = file_delete_async(disk, src).await;
    Ok(true)
}

async fn lookup_file_record_async(
    disk: block::DeviceHandle,
    name: &str,
) -> Result<Option<trueos_fs::FileRecordRef>, block::Error> {
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(None);
    };
    lookup_via_index_async(disk, &placement, name).await
}

fn normalize_dir_name(path: &str) -> String {
    let prefix = normalized_dir_prefix(path);
    prefix.trim_end_matches('/').into()
}

fn collect_index_tree_moves(
    disk_id: block::DiscId,
    src_dir: &str,
    dst_dir: &str,
) -> Option<Vec<(String, String, IndexRef)>> {
    let src_prefix = normalized_dir_prefix(src_dir);
    if src_prefix.is_empty() {
        return None;
    }
    let dst_prefix = normalized_dir_prefix(dst_dir);
    if dst_prefix.is_empty() {
        return None;
    }
    if dst_prefix.starts_with(src_prefix.as_str()) {
        return None;
    }

    let roots = ROOTS.lock();
    let mount = roots.iter().find(|m| m.disk_id == disk_id)?;
    let index = mount.index.as_ref()?;

    let mut moves = Vec::new();
    if let Some(index_ref) = index.get(src_dir.as_bytes()) {
        moves.push((String::from(src_dir), String::from(dst_dir), *index_ref));
    }
    for (key, index_ref) in index.range(src_prefix.as_bytes().to_vec()..) {
        if !key.starts_with(src_prefix.as_bytes()) {
            break;
        }
        let Ok(src_path) = core::str::from_utf8(key) else {
            return None;
        };
        let suffix = &src_path[src_prefix.len()..];
        if suffix.is_empty() {
            continue;
        }
        moves.push((String::from(src_path), alloc::format!("{dst_prefix}{suffix}"), *index_ref));
    }
    for (_, dst, _) in moves.iter() {
        if index.contains_key(dst.as_bytes()) {
            let occupied_by_source = moves.iter().any(|(src, _, _)| src == dst);
            if !occupied_by_source {
                return None;
            }
        }
    }

    Some(moves)
}

/// Async TRUEOSFS: move a whole directory tree by appending one metadata record.
///
/// File payload blocks are not copied. The live index remaps the directory
/// record itself and every descendant under `src_dir`.
pub async fn dir_rename_async(
    disk: block::DeviceHandle,
    src_dir: &str,
    dst_dir: &str,
) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let src = normalize_dir_name(src_dir);
    let dst = normalize_dir_name(dst_dir);
    if src.is_empty() || dst.is_empty() || src == dst {
        return Ok(false);
    }
    if let Some((parent, _)) = dst.rsplit_once('/')
        && !parent.is_empty()
        && !dir_exists_async(disk, parent).await?
    {
        return Ok(false);
    }

    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(false);
    };
    ensure_index_async(disk, &placement).await?;

    let disk_id = disk.id();
    let Some(moves) = collect_index_tree_moves(disk_id, src.as_str(), dst.as_str()) else {
        return Ok(false);
    };
    if moves.is_empty()
        || !moves
            .iter()
            .any(|(source, _, entry)| source == &src && entry.kind == trueos_fs::LogKind::Directory)
    {
        return Ok(false);
    }

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    let ok = trueos_fs::rename_tree(&io, &params, src.as_str(), dst.as_str())
        .await
        .map_err(map_engine_err)?;
    if !ok {
        return Ok(false);
    }

    bump_root_cache_gen(disk_id);
    file_record_cache_invalidate_prefix(disk_id, normalized_dir_prefix(src.as_str()).as_str());
    file_record_cache_invalidate_prefix(disk_id, normalized_dir_prefix(dst.as_str()).as_str());
    if !update_root_index_rename_tree(disk_id, moves.as_slice()) {
        invalidate_root_index(disk_id);
    }
    Ok(true)
}

/// Async TRUEOSFS: list the immediate children of a directory.
///
/// Returns `Ok(None)` if the disk does not contain TRUEOSFS.
pub async fn list_dir_async(
    disk: block::DeviceHandle,
    dir: &str,
) -> Result<Option<DirListing>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
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
        let io = KernelBlockIo::new_fair_boot_work(disk);
        let out = trueos_fs::list_dir(&io, &params, dir)
            .await
            .map_err(map_engine_err)?;
        let truncated = out.len() > TRUEOSFS_LIST_SOFT_CAP;
        let mut entries = out;
        entries.truncate(TRUEOSFS_LIST_SOFT_CAP);
        return Ok(Some(DirListing { entries, truncated }));
    };

    let Some(index) = &mount.index else {
        return Err(block::Error::Corrupted);
    };

    let prefix = normalized_dir_prefix(dir);
    let prefix_bytes = prefix.as_bytes();

    if !prefix.is_empty()
        && !index
            .get(prefix.trim_end_matches('/').as_bytes())
            .is_some_and(|entry| entry.kind == trueos_fs::LogKind::Directory)
    {
        return Err(block::Error::InvalidParam);
    }

    let mut children: BTreeMap<String, (NodeKind, ContentTypeId)> = BTreeMap::new();

    if prefix.is_empty() {
        for (key, index_ref) in index.iter() {
            if let Ok(name) = core::str::from_utf8(key) {
                if name.is_empty() || name.contains('/') {
                    continue;
                }
                let kind = match index_ref.kind {
                    trueos_fs::LogKind::Put => NodeKind::File,
                    trueos_fs::LogKind::Directory => NodeKind::Directory,
                    _ => continue,
                };
                children.insert(String::from(name), (kind, index_ref.content_type));
            }
        }
    } else {
        for (key, index_ref) in index.range(prefix_bytes.to_vec()..) {
            if !key.starts_with(prefix_bytes) {
                break;
            }
            if key.len() <= prefix_bytes.len() {
                continue;
            }
            if let Ok(rest_str) = core::str::from_utf8(&key[prefix_bytes.len()..]) {
                if rest_str.is_empty() || rest_str.contains('/') {
                    continue;
                }
                let kind = match index_ref.kind {
                    trueos_fs::LogKind::Put => NodeKind::File,
                    trueos_fs::LogKind::Directory => NodeKind::Directory,
                    _ => continue,
                };
                children.insert(String::from(rest_str), (kind, index_ref.content_type));
            }
        }
    }

    let mut entries = Vec::new();
    let mut truncated = children.len() > TRUEOSFS_LIST_SOFT_CAP;
    for (name, (kind, content_type)) in children {
        if entries.len() >= TRUEOSFS_LIST_SOFT_CAP {
            truncated = true;
            break;
        }
        entries.push(DirEntry { name, kind, content_type });
    }
    if truncated {
        crate::log_warn!(target: "filesystem";
            "trueosfs: file listing soft cap reached operation=list_dir cap={}\n",
            TRUEOSFS_LIST_SOFT_CAP
        );
    }

    Ok(Some(DirListing { entries, truncated }))
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
    let build_started_ms = trueosfs_trace_now_ms();

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
                crate::log_info!(target: "trueosfs";
                    "trueosfs: diag phase=index-build-claim disk={} cache_gen={}\n",
                    disk_id.raw(), start_cache_gen
                );
                break;
            }
        }
    }

    // Build outside lock.
    let build_result: Result<BuiltIndex, block::Error> = async {
        let params = trueos_fs::FsParams {
            super_lba: placement.super_lba,
            data_lba: placement.data_lba,
            data_end_lba_exclusive: placement.data_end_lba_exclusive,
        };
        let io = KernelBlockIo::new(disk);

        let mut tree = Box::new(BTreeMap::new());

        // Replay log.
        let superblock_started_ms = trueosfs_trace_now_ms();
        let sb_blk = read_blocks_aligned_async(disk, params.super_lba, 1).await?;
        let sb = trueos_fs::parse_superblock(&sb_blk).ok_or(block::Error::Corrupted)?;
        crate::log_info!(target: "trueosfs";
            "trueosfs: diag phase=index-superblock disk={} elapsed_ms={} log_head_rel_blocks={} checkpoint_rel_blocks={} data_lba={}\n",
            disk_id.raw(), trueosfs_trace_now_ms().saturating_sub(superblock_started_ms),
            sb.log_head_rel_blocks, sb.checkpoint_rel_blocks, params.data_lba
        );

        let mut replay_from = 0u64;
        let mut had_checkpoint = false;

        let checkpoint_started_ms = trueosfs_trace_now_ms();
        let checkpoint_status = match trueos_fs::read_index_checkpoint_with_status(&io, &params)
            .await
            .map_err(map_engine_err)
        {
            Ok(status) => status,
            Err(e) => {
                crate::log_info!(target: "trueosfs";
                    "trueosfs: diag phase=index-checkpoint disk={} validity=read-error pointer={} elapsed_ms={} err={:?}\n",
                    disk_id.raw(), sb.checkpoint_rel_blocks,
                    trueosfs_trace_now_ms().saturating_sub(checkpoint_started_ms), e
                );
                return Err(e);
            }
        };
        match checkpoint_status {
            trueos_fs::IndexCheckpointRead::Absent => {
                crate::log_info!(target: "trueosfs";
                    "trueosfs: diag phase=index-checkpoint disk={} validity=absent pointer={} elapsed_ms={}\n",
                    disk_id.raw(), sb.checkpoint_rel_blocks,
                    trueosfs_trace_now_ms().saturating_sub(checkpoint_started_ms)
                );
            }
            trueos_fs::IndexCheckpointRead::Invalid => {
                crate::log_info!(target: "trueosfs";
                    "trueosfs: diag phase=index-checkpoint disk={} validity=invalid pointer={} elapsed_ms={}\n",
                    disk_id.raw(), sb.checkpoint_rel_blocks,
                    trueosfs_trace_now_ms().saturating_sub(checkpoint_started_ms)
                );
            }
            trueos_fs::IndexCheckpointRead::Valid(ckpt) => {
                had_checkpoint = true;
                replay_from = ckpt.replay_from_rel_blocks;
                let checkpoint_entry_count = ckpt.entries.len();
                let mut checkpoint_entries_since_yield = 0usize;
                for (key, kind, lba, content_type) in ckpt.entries {
                    match kind {
                        trueos_fs::LogKind::Put | trueos_fs::LogKind::Directory => {
                            tree.insert(key, IndexRef { kind, entry_lba: lba, content_type });
                        }
                        trueos_fs::LogKind::Delete => {
                            tree.remove(&key);
                        }
                        _ => {}
                    }
                    checkpoint_entries_since_yield += 1;
                    if checkpoint_entries_since_yield
                        >= crate::allcaps::storage::TRUEOSFS_INDEX_CHECKPOINT_ENTRIES_PER_YIELD
                    {
                        checkpoint_entries_since_yield = 0;
                        trueosfs_boot_work_yield().await;
                    }
                }
                crate::log_info!(target: "trueosfs";
                    "trueosfs: diag phase=index-checkpoint disk={} validity=valid pointer={} entries={} replay_from={} elapsed_ms={}\n",
                    disk_id.raw(), sb.checkpoint_rel_blocks, checkpoint_entry_count, replay_from,
                    trueosfs_trace_now_ms().saturating_sub(checkpoint_started_ms)
                );
            }
        }

        let end_rel = sb.log_head_rel_blocks;
        let tail_blocks = end_rel.saturating_sub(replay_from);
        crate::log_info!(target: "trueosfs";
            "trueosfs: diag phase=index-replay-start disk={} replay_from={} end_rel={} tail_blocks={}\n",
            disk_id.raw(), replay_from, end_rel, tail_blocks
        );
        let replay_started_ms = trueosfs_trace_now_ms();
        let mut replay_records = 0u64;
        let mut last_progress_ms = replay_started_ms;

        let replay_stats = trueos_fs::replay_log_range_with_stats(&io, &params, replay_from, end_rel, |kind, name, data, lba, content_type| {
            replay_records = replay_records.saturating_add(1);
            let now_ms = trueosfs_trace_now_ms();
            if now_ms.saturating_sub(last_progress_ms) >= 1_000 {
                crate::log_info!(target: "trueosfs";
                    "trueosfs: diag phase=index-replay-progress disk={} records={} rel_blocks={} tail_blocks={} elapsed_ms={}\n",
                    disk_id.raw(), replay_records, lba.saturating_sub(params.data_lba), tail_blocks,
                    now_ms.saturating_sub(replay_started_ms)
                );
                last_progress_ms = now_ms;
            }
            match kind {
                trueos_fs::LogKind::Put | trueos_fs::LogKind::Directory => {
                    tree.insert(
                        name,
                        IndexRef {
                            kind,
                            entry_lba: lba,
                            content_type,
                        },
                    );
                }
                trueos_fs::LogKind::Delete => {
                    tree.remove(&name);
                }
                trueos_fs::LogKind::RenameTree => {
                    let Ok(src) = core::str::from_utf8(name.as_slice()) else {
                        return;
                    };
                    let Ok(dst) = core::str::from_utf8(data.as_slice()) else {
                        return;
                    };
                    apply_index_rename_tree(&mut tree, src, dst);
                }
                trueos_fs::LogKind::DeleteTree => {
                    let Ok(path) = core::str::from_utf8(name.as_slice()) else {
                        return;
                    };
                    let prefix = normalized_dir_prefix(path);
                    tree.remove(path.as_bytes());
                    tree.retain(|key, _| !key.starts_with(prefix.as_bytes()));
                }
                _ => {}
            }
        })
        .await
        .map_err(map_engine_err)?;

        crate::log_info!(target: "trueosfs";
            "trueosfs: diag phase=index-replay-done disk={} elapsed_ms={} logical_records={} checkpoint_records={} physical_blocks={} reached_end={} stop_rel={} tail_blocks={}\n",
            disk_id.raw(), trueosfs_trace_now_ms().saturating_sub(replay_started_ms),
            replay_stats.logical_records, replay_stats.checkpoint_records,
            replay_stats.physical_blocks, replay_stats.reached_end,
            replay_stats.stop_rel_blocks, tail_blocks
        );

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
                    crate::log_info!(target: "trueosfs";
                        "trueosfs: diag phase=index-published disk={} cache_gen={} entries={} elapsed_ms={}\n",
                        disk_id.raw(), start_cache_gen, entry_count,
                        trueosfs_trace_now_ms().saturating_sub(build_started_ms)
                    );
                    checkpoint_after_publish =
                        Some((replay_from_rel_blocks, end_rel_blocks, had_checkpoint, entry_count));
                } else {
                    needs_rebuild = true;
                    crate::log_info!(target: "trueosfs";
                        "trueosfs: diag phase=index-raced disk={} start_cache_gen={} current_cache_gen={} action=rebuild\n",
                        disk_id.raw(), start_cache_gen, m.cache_gen
                    );
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
            crate::log_info!(target: "trueosfs";
                "trueosfs: diag phase=index-build-error disk={} cache_gen={} elapsed_ms={} err={:?}\n",
                disk_id.raw(), start_cache_gen,
                trueosfs_trace_now_ms().saturating_sub(build_started_ms), e
            );
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

fn push_hex_bytes(out: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes.iter().copied() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

fn push_record_key_json(out: &mut String, record_key: RecordKey) {
    out.push_str(",\"key\":{");
    match record_key {
        RecordKey::Ffa => out.push_str("\"kind\":\"ffa\""),
        RecordKey::Key(key) => {
            out.push_str("\"kind\":\"key\",\"provider\":\"");
            push_hex_bytes(out, key.provider.as_bytes());
            out.push_str("\",\"handle\":\"");
            push_hex_bytes(out, key.handle.as_bytes());
            out.push('"');
        }
    }
    out.push('}');
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

    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(None);
    };

    ensure_index_async(disk, &placement).await?;
    let disk_id = disk.id();
    let effective_limit = if max_entries == 0 {
        TRUEOSFS_LIST_SOFT_CAP
    } else {
        core::cmp::min(max_entries, TRUEOSFS_LIST_SOFT_CAP)
    };

    #[derive(Clone)]
    struct JsonEntry {
        depth: usize,
        path: String,
        kind: &'static str,
        name: String,
        id: u64,
        record_lba: Option<u64>,
        record_key: RecordKey,
    }

    let mut by_depth: BTreeMap<usize, Vec<JsonEntry>> = BTreeMap::new();
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut truncated = false;

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
            if !seen.insert(key.clone()) {
                continue;
            }
            let kind = match index_ref.kind {
                trueos_fs::LogKind::Put => "file",
                trueos_fs::LogKind::Directory => "dir",
                _ => continue,
            };
            let depth = segments.len().saturating_sub(1);
            by_depth.entry(depth).or_default().push(JsonEntry {
                depth,
                id: index_ref.entry_lba,
                path: String::from(path),
                name: String::from(*segments.last().unwrap_or(&"")),
                kind,
                record_lba: Some(index_ref.entry_lba),
                record_key: RecordKey::Ffa,
            });
            let count = by_depth.values().map(|items| items.len()).sum::<usize>();
            if count > effective_limit {
                truncated = true;
                break 'scan;
            }
        }
    }

    if truncated {
        crate::log_warn!(target: "filesystem";
            "trueosfs: file listing soft cap reached operation=json_all cap={} requested={}\n",
            TRUEOSFS_LIST_SOFT_CAP,
            max_entries
        );
    }

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);
    for entries in by_depth.values_mut() {
        for entry in entries.iter_mut() {
            if let Some(entry_lba) = entry.record_lba {
                entry.record_key = trueos_fs::read_record_key_at(&io, &params, entry_lba)
                    .await
                    .map_err(map_engine_err)?
                    .ok_or(block::Error::Corrupted)?;
            }
        }
    }

    let mut written = 0usize;
    let mut out = String::new();
    out.push_str("{\"version\":2,\"root\":\"/\",\"max_entries\":");
    out.push_str(alloc::format!("{}", effective_limit).as_str());
    out.push_str(",\"truncated\":");
    out.push_str(if truncated { "true" } else { "false" });
    out.push_str(",\"entries\":[");

    let mut first = true;
    let visible_limit = if truncated {
        effective_limit.saturating_sub(1)
    } else {
        effective_limit
    };
    'write: for entries in by_depth.values() {
        for entry in entries.iter() {
            if written >= visible_limit {
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
            push_record_key_json(&mut out, entry.record_key);
            out.push('}');
            written += 1;
        }
    }

    if truncated {
        if !first {
            out.push(',');
        }
        out.push_str("{\"path\":\"...\",\"name\":\"...\",\"kind\":\"more\",\"depth\":0,\"id\":0,\"key\":{\"kind\":\"ffa\"}}");
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
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(false);
    };

    let params = trueos_fs::FsParams {
        super_lba: placement.super_lba,
        data_lba: placement.data_lba,
        data_end_lba_exclusive: placement.data_end_lba_exclusive,
    };
    let io = KernelBlockIo::new(disk);

    let disk_id = disk.id();
    let record = match file_record_cache_lookup(disk_id, name) {
        Some(record) => Some(record),
        None => lookup_via_index_async(disk, &placement, name).await?,
    };
    if record.is_some_and(|record| record.content_type != ContentTypeId::BLOB) {
        record_type_reject(ContentIdentityRejectReason::LegacyDowngrade);
        return Err(block::Error::InvalidParam);
    }
    if append_bytes.is_empty() {
        return Ok(true);
    }
    let (mut bytes, record_key) = match record {
        Some(record) => {
            let Some(existing) = trueos_fs::read_file_at_record(&io, &params, &record)
                .await
                .map_err(map_engine_err)?
            else {
                return Ok(false);
            };
            (existing, record.record_key)
        }
        None => (Vec::new(), RecordKey::Ffa),
    };
    bytes.extend_from_slice(append_bytes);

    let Some(mut stream) = trueos_fs::begin_write_file_stream_with_key(
        &io,
        &params,
        name,
        bytes.len() as u64,
        record_key,
    )
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
    record_successful_content_commit(record.content_type, true);

    bump_root_cache_gen(disk_id);
    file_record_cache_invalidate_path(disk_id, name);
    file_record_cache_insert(disk_id, name, record);
    if !update_root_index_put(disk_id, name, record) {
        invalidate_root_index(disk_id);
    }
    Ok(true)
}

pub async fn file_append_typed_async(
    disk: block::DeviceHandle,
    name: &str,
    append_bytes: &[u8],
    content_type: ContentTypeId,
) -> Result<bool, block::Error> {
    if content_type == ContentTypeId::NONE {
        record_type_reject(ContentIdentityRejectReason::TypeRequired);
        return Err(block::Error::InvalidParam);
    }
    if !content_type.is_registered() {
        record_type_reject(ContentIdentityRejectReason::UnregisteredType);
        return Err(block::Error::InvalidParam);
    }
    let Some(info) = file_info_async(disk, name).await? else {
        return file_in_typed_async(disk, name, append_bytes, content_type).await;
    };
    if info.content_type != content_type {
        record_type_reject(ContentIdentityRejectReason::TypeMismatch);
        return Err(block::Error::InvalidParam);
    }
    let Some(mut bytes) = file_out_async(disk, name).await? else {
        return Ok(false);
    };
    bytes.extend_from_slice(append_bytes);
    file_in_typed_async(disk, name, &bytes, content_type).await
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

#[derive(Clone, Debug)]
pub(super) struct IndexNode {
    pub path: String,
    pub kind: NodeKind,
}

pub(super) async fn index_path_snapshot_async(
    disk: block::DeviceHandle,
) -> Result<Option<Vec<IndexNode>>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
        return Ok(None);
    };

    ensure_index_async(disk, &placement).await?;
    let disk_id = disk.id();

    let roots = ROOTS.lock();
    let Some(mount) = roots.iter().find(|m| m.disk_id == disk_id) else {
        return Err(block::Error::NotReady);
    };
    let Some(index) = mount.index.as_ref() else {
        return Err(block::Error::Corrupted);
    };

    let mut out = Vec::with_capacity(index.len());
    for (key, index_ref) in index.iter() {
        if let Ok(path) = core::str::from_utf8(key.as_slice())
            && !path.is_empty()
        {
            let kind = match index_ref.kind {
                trueos_fs::LogKind::Put => NodeKind::File,
                trueos_fs::LogKind::Directory => NodeKind::Directory,
                _ => continue,
            };
            out.push(IndexNode {
                path: String::from(path),
                kind,
            });
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
pub(crate) async fn raw_log_scan_async(
    disk: block::DeviceHandle,
    max_records: usize,
) -> Result<Option<trueos_fs::RawLogScan>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = placement_for_io_async(disk).await? else {
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

    crate::log_info!(target: "trueosfs";
        "trueosfs: diag phase=index-request disk={}\n", disk_id.raw()
    );
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn primary_root_is_read_only() -> Option<bool> {
    primary_root_handle().map(|h| h.info().is_read_only())
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuperblockProbe {
    Other,
    Current,
    UnsupportedTrueosFs,
}

fn probe_trueos_superblock(block0: &[u8]) -> SuperblockProbe {
    if block0.len() < trueos_fs::MAGIC.len() || block0[..trueos_fs::MAGIC.len()] != trueos_fs::MAGIC
    {
        return SuperblockProbe::Other;
    }
    if trueos_fs::parse_superblock(block0).is_some() {
        SuperblockProbe::Current
    } else {
        SuperblockProbe::UnsupportedTrueosFs
    }
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
        return match probe_trueos_superblock(&bs0) {
            SuperblockProbe::Current => Ok(Some(TrueosFsPlacement {
                bootable: false,
                super_lba: 0,
                data_lba: trueos_fs::data_lba_from_super(0),
                data_end_lba_exclusive: Some(handle.info().block_count),
            })),
            SuperblockProbe::UnsupportedTrueosFs => Err(block::Error::NotSupported),
            SuperblockProbe::Other => Ok(None),
        };
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
                        {
                            match probe_trueos_superblock(&p0) {
                                SuperblockProbe::Current => {
                                    let super_lba = p.range.first_lba();
                                    let end_lba_exclusive = p.range.last_lba().saturating_add(1);
                                    return Ok(Some(TrueosFsPlacement {
                                        bootable: has_esp,
                                        super_lba,
                                        data_lba: trueos_fs::data_lba_from_super(super_lba),
                                        data_end_lba_exclusive: Some(end_lba_exclusive),
                                    }));
                                }
                                SuperblockProbe::UnsupportedTrueosFs => {
                                    crate::log_important!(target: "storage";
                                        "trueosfs: unsupported format disk={} super_lba={} expected_version={} decision=reformat-required\n",
                                        handle.id().raw(),
                                        p.range.first_lba(),
                                        trueos_fs::FORMAT_VERSION,
                                    );
                                    return Err(block::Error::NotSupported);
                                }
                                SuperblockProbe::Other => {}
                            }
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
    match probe_trueos_superblock(&bs0) {
        SuperblockProbe::Current => Ok(Some(TrueosFsPlacement {
            bootable: false,
            super_lba: 0,
            data_lba: trueos_fs::data_lba_from_super(0),
            data_end_lba_exclusive: None,
        })),
        SuperblockProbe::UnsupportedTrueosFs => {
            crate::log_important!(target: "storage";
                "trueosfs: unsupported format disk={} super_lba=0 expected_version={} decision=reformat-required\n",
                handle.id().raw(),
                trueos_fs::FORMAT_VERSION,
            );
            Err(block::Error::NotSupported)
        }
        SuperblockProbe::Other => Ok(None),
    }
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
        unregister_root_mount(handle.id());
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
    if probe_trueos_superblock(&verify0) != SuperblockProbe::Current {
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
