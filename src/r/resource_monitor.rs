//! Encoded UI/GFX resource preservation.
//!
//! This service owns a private TRUEOSFS ramdisk and records encoded texture
//! assets at PNG/JPEG/SVG upload time, before decode.  The heavy decoded RGBA
//! path can later check this table instead of duplicating pixels.

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use embassy_executor::task;
use embassy_time_driver::{TICK_HZ, now};
use heapless::Deque;
use spin::Mutex;

use crate::disc::block;
use crate::wait::WaitQueue;

const RESOURCE_MONITOR_BLOCK_SIZE: u32 = 512;
const RESOURCE_MONITOR_RAMDISK_BYTES: u64 = 256 * 1024 * 1024;
const RESOURCE_MONITOR_QUEUE_CAP: usize = 64;
const RESOURCE_MONITOR_PROBE_PATH: &str = "resource/.probe";
const RESOURCE_MONITOR_PROBE_BYTES: &[u8] = b"trueos-resource-monitor-probe";
const RESOURCE_MONITOR_META_PREFIX: &str = "resource/meta-";
const RESOURCE_MONITOR_ASSET_PREFIX: &str = "resource/asset-";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodedKind {
    Png,
    Jpeg,
    Svg,
}

impl EncodedKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Svg => "svg",
        }
    }
}

#[derive(Clone)]
struct PreserveRequest {
    seq: u64,
    tex_id: u32,
    kind: EncodedKind,
    flags: u32,
    bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct EncodedAsset {
    pub tex_id: u32,
    pub kind: EncodedKind,
    pub flags: u32,
    pub bytes: Vec<u8>,
    pub seq: u64,
}

static RESOURCE_MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static RESOURCE_MONITOR_ONLINE: AtomicBool = AtomicBool::new(false);
static RESOURCE_MONITOR_DROPPED: AtomicU32 = AtomicU32::new(0);
static RESOURCE_MONITOR_PRESERVED: AtomicU32 = AtomicU32::new(0);
static RESOURCE_MONITOR_BYTES: AtomicU64 = AtomicU64::new(0);
static RESOURCE_MONITOR_SEQ: AtomicU64 = AtomicU64::new(1);
static RESOURCE_MONITOR_FLUSHED_SEQ: AtomicU64 = AtomicU64::new(0);
static RESOURCE_MONITOR_DISK: Mutex<Option<block::DeviceHandle>> = Mutex::new(None);
static RESOURCE_MONITOR_QUEUE: Mutex<Deque<PreserveRequest, RESOURCE_MONITOR_QUEUE_CAP>> =
    Mutex::new(Deque::new());
static RESOURCE_MONITOR_ENCODED: Mutex<Vec<EncodedAsset>> = Mutex::new(Vec::new());
static RESOURCE_MONITOR_WAIT: WaitQueue = WaitQueue::new();

#[inline]
pub fn online() -> bool {
    RESOURCE_MONITOR_ONLINE.load(Ordering::Acquire)
}

#[inline]
pub fn preserved_count() -> u32 {
    RESOURCE_MONITOR_PRESERVED.load(Ordering::Acquire)
}

#[inline]
pub fn preserved_bytes() -> u64 {
    RESOURCE_MONITOR_BYTES.load(Ordering::Acquire)
}

#[inline]
pub fn dropped_count() -> u32 {
    RESOURCE_MONITOR_DROPPED.load(Ordering::Acquire)
}

pub fn preserve_encoded_texture(
    tex_id: u32,
    kind: EncodedKind,
    flags: u32,
    encoded: &[u8],
) -> bool {
    if tex_id == 0 || encoded.is_empty() {
        return false;
    }

    let req = PreserveRequest {
        seq: RESOURCE_MONITOR_SEQ.fetch_add(1, Ordering::AcqRel).max(1),
        tex_id,
        kind,
        flags,
        bytes: encoded.to_vec(),
    };

    {
        let mut assets = RESOURCE_MONITOR_ENCODED.lock();
        if let Some(asset) = assets.iter_mut().find(|asset| asset.tex_id == tex_id) {
            asset.kind = kind;
            asset.flags = flags;
            asset.bytes.clear();
            asset.bytes.extend_from_slice(encoded);
            asset.seq = req.seq;
        } else {
            assets.push(EncodedAsset {
                tex_id,
                kind,
                flags,
                bytes: encoded.to_vec(),
                seq: req.seq,
            });
        }
    }

    let pushed = {
        let mut queue = RESOURCE_MONITOR_QUEUE.lock();
        queue.push_back(req).is_ok()
    };
    if pushed {
        RESOURCE_MONITOR_WAIT.notify_one();
        true
    } else {
        let dropped = RESOURCE_MONITOR_DROPPED.fetch_add(1, Ordering::AcqRel) + 1;
        if dropped <= 16 {
            crate::log!(
                "resource-monitor: queue full cap={} dropped={} tex={} kind={} bytes={}\n",
                RESOURCE_MONITOR_QUEUE_CAP,
                dropped,
                tex_id,
                kind.as_str(),
                encoded.len()
            );
        }
        false
    }
}

pub fn encoded_texture(tex_id: u32) -> Option<EncodedAsset> {
    RESOURCE_MONITOR_ENCODED
        .lock()
        .iter()
        .find(|asset| asset.tex_id == tex_id)
        .cloned()
}

pub fn encoded_assets_snapshot() -> Vec<EncodedAsset> {
    let mut assets = RESOURCE_MONITOR_ENCODED.lock().clone();
    assets.sort_by_key(|asset| asset.seq);
    assets
}

pub fn latest_encoded_seq() -> u64 {
    RESOURCE_MONITOR_ENCODED
        .lock()
        .iter()
        .map(|asset| asset.seq)
        .max()
        .unwrap_or(0)
}

pub async fn wait_until_flushed(target_seq: u64, timeout_ms: u64) -> bool {
    if target_seq == 0 {
        return true;
    }
    let ticks = if TICK_HZ == 0 || timeout_ms == 0 {
        0
    } else {
        timeout_ms.saturating_mul(TICK_HZ).div_ceil(1000).max(1)
    };
    let deadline = if ticks == 0 {
        0
    } else {
        now().saturating_add(ticks)
    };

    loop {
        if RESOURCE_MONITOR_FLUSHED_SEQ.load(Ordering::Acquire) >= target_seq {
            return true;
        }
        if deadline != 0 && now() >= deadline {
            return false;
        }
        let _ = RESOURCE_MONITOR_WAIT.wait_for_event_timeout(25).await;
    }
}

async fn ensure_disk() -> Result<block::DeviceHandle, block::Error> {
    if let Some(disk) = *RESOURCE_MONITOR_DISK.lock() {
        return Ok(disk);
    }

    crate::log!(
        "resource-monitor: ramdisk create begin bytes={}\n",
        RESOURCE_MONITOR_RAMDISK_BYTES
    );
    let disk = crate::r::disc::ramdisk::create_trueos_private(
        RESOURCE_MONITOR_RAMDISK_BYTES,
        RESOURCE_MONITOR_BLOCK_SIZE,
        "trueos-resource-monitor",
    )
    .await
    .map_err(|err| match err {
        crate::r::disc::ramdisk::TrueosPrivateError::Create(err) => err,
        crate::r::disc::ramdisk::TrueosPrivateError::Format(err)
        | crate::r::disc::ramdisk::TrueosPrivateError::Validate(err) => err,
    })?;

    let wrote = crate::r::fs::trueosfs::file_in_async(
        disk,
        RESOURCE_MONITOR_PROBE_PATH,
        RESOURCE_MONITOR_PROBE_BYTES,
    )
    .await?;
    if !wrote {
        return Err(block::Error::Io);
    }

    *RESOURCE_MONITOR_DISK.lock() = Some(disk);
    RESOURCE_MONITOR_ONLINE.store(true, Ordering::Release);
    crate::log!(
        "resource-monitor: ramdisk online disk={} bytes={}\n",
        disk.id().raw(),
        RESOURCE_MONITOR_RAMDISK_BYTES
    );
    Ok(disk)
}

fn object_path(prefix: &str, seq: u64) -> String {
    format!("{}{:020}", prefix, seq)
}

fn request_manifest(req: &PreserveRequest, asset_path: &str) -> String {
    format!(
        "seq={}\ntex_id={}\nkind={}\nflags=0x{:08X}\nbytes={}\nasset={}\n",
        req.seq,
        req.tex_id,
        req.kind.as_str(),
        req.flags,
        req.bytes.len(),
        asset_path
    )
}

async fn write_request(
    disk: block::DeviceHandle,
    req: PreserveRequest,
) -> Result<(), block::Error> {
    let asset_path = object_path(RESOURCE_MONITOR_ASSET_PREFIX, req.seq);
    let meta_path = object_path(RESOURCE_MONITOR_META_PREFIX, req.seq);
    let manifest = request_manifest(&req, asset_path.as_str());

    let wrote_asset =
        crate::r::fs::trueosfs::file_in_async(disk, asset_path.as_str(), req.bytes.as_slice())
            .await?;
    if !wrote_asset {
        return Err(block::Error::Io);
    }
    let wrote_meta =
        crate::r::fs::trueosfs::file_in_async(disk, meta_path.as_str(), manifest.as_bytes())
            .await?;
    if !wrote_meta {
        return Err(block::Error::Io);
    }

    RESOURCE_MONITOR_PRESERVED.fetch_add(1, Ordering::AcqRel);
    RESOURCE_MONITOR_BYTES.fetch_add(req.bytes.len() as u64, Ordering::AcqRel);
    RESOURCE_MONITOR_FLUSHED_SEQ.fetch_max(req.seq, Ordering::AcqRel);
    RESOURCE_MONITOR_WAIT.notify_all();
    Ok(())
}

#[task(pool_size = 1)]
pub async fn resource_monitor_task() {
    if RESOURCE_MONITOR_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::log!("resource-monitor: task start mode=private-ramdisk preserve=encoded\n");

    loop {
        let req = {
            let mut queue = RESOURCE_MONITOR_QUEUE.lock();
            queue.pop_front()
        };

        let Some(req) = req else {
            RESOURCE_MONITOR_WAIT.wait_for_event().await;
            continue;
        };

        match ensure_disk().await {
            Ok(disk) => {
                if let Err(err) = write_request(disk, req.clone()).await {
                    crate::log!(
                        "resource-monitor: preserve failed tex={} kind={} seq={} bytes={} err={:?}\n",
                        req.tex_id,
                        req.kind.as_str(),
                        req.seq,
                        req.bytes.len(),
                        err
                    );
                }
            }
            Err(err) => {
                crate::log!(
                    "resource-monitor: ramdisk unavailable tex={} kind={} seq={} bytes={} err={:?}\n",
                    req.tex_id,
                    req.kind.as_str(),
                    req.seq,
                    req.bytes.len(),
                    err
                );
            }
        }
    }
}
