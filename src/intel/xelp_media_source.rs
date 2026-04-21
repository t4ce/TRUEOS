extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

const MEDIA_HTTP_LOCAL_DEMO_URLS: [&str; 2] = [
    "http://192.168.178.112:8080/tools/vid/demo_yelly.mp4",
    "http://pcjb:8080/tools/vid/demo_yelly.mp4",
];
const MEDIA_HTTP_LOCAL_DEMO_TIMEOUT_MS: u32 = 180_000;
const MEDIA_HTTP_LOCAL_DEMO_MAX_BYTES: usize = 1024 * 1024 * 1024;
const MEDIA_DECODE_CACHE_PATH: &str = "media/demo_yelly.mp4";
const MEDIA_CACHE_WRITE_PROGRESS_BYTES: u64 = 8 * 1024 * 1024;
const MEDIA_CACHE_WRITE_YIELD_EVERY_BYTES: u64 = 512 * 1024;
const MEDIA_CACHE_WRITE_FALLBACK_CHUNK_BYTES: usize = 256 * 1024;
const MEDIA_CACHE_WRITE_MAX_CHUNK_BYTES: usize = 1024 * 1024;
const MEDIA_SOURCE_FETCH_WAIT_MS: u64 = 50;
const MEDIA_SOURCE_FETCH_LOG_EVERY_POLLS: u32 = 20;

static MEDIA_SOURCE_WARM_RAN: AtomicBool = AtomicBool::new(false);
static MEDIA_DECODE_REQUESTED: AtomicBool = AtomicBool::new(false);
static MEDIA_SOURCE_FETCH_ACTIVE: AtomicBool = AtomicBool::new(false);
static MEDIA_SOURCE_STASH: Mutex<Option<MediaSourceStash>> = Mutex::new(None);

pub(crate) enum MediaSource {
    Memory {
        source: &'static str,
        body: Vec<u8>,
    },
    CacheFile {
        source: &'static str,
        disk: crate::disc::block::DeviceHandle,
        path: &'static str,
        len: u64,
    },
}

impl MediaSource {
    pub(crate) fn source_name(&self) -> &'static str {
        match self {
            Self::Memory { source, .. } | Self::CacheFile { source, .. } => source,
        }
    }

    pub(crate) fn total_len(&self) -> u64 {
        match self {
            Self::Memory { body, .. } => body.len() as u64,
            Self::CacheFile { len, .. } => *len,
        }
    }

    pub(crate) fn body(&self) -> Option<&[u8]> {
        match self {
            Self::Memory { body, .. } => Some(body.as_slice()),
            Self::CacheFile { .. } => None,
        }
    }

    pub(crate) async fn read_range_into(
        &self,
        offset: u64,
        dst: &mut [u8],
    ) -> Result<bool, crate::disc::block::Error> {
        match self {
            Self::Memory { body, .. } => {
                let start = usize::try_from(offset).map_err(|_| crate::disc::block::Error::OutOfBounds)?;
                let end = start
                    .checked_add(dst.len())
                    .ok_or(crate::disc::block::Error::OutOfBounds)?;
                let Some(slice) = body.get(start..end) else {
                    return Err(crate::disc::block::Error::OutOfBounds);
                };
                dst.copy_from_slice(slice);
                Ok(true)
            }
            Self::CacheFile {
                disk, path, len: total, ..
            } => {
                let end = offset
                    .checked_add(dst.len() as u64)
                    .ok_or(crate::disc::block::Error::OutOfBounds)?;
                if end > *total {
                    return Err(crate::disc::block::Error::OutOfBounds);
                }
                crate::r::stream::read_trueosfs_file_range_into_async(*disk, path, offset, dst)
                    .await
            }
        }
    }
}

struct MediaSourceStash {
    source: &'static str,
    body: Vec<u8>,
}

struct MediaSourceFetchGuard;

impl Drop for MediaSourceFetchGuard {
    fn drop(&mut self) {
        MEDIA_SOURCE_FETCH_ACTIVE.store(false, Ordering::Release);
    }
}

pub(crate) fn demo_urls() -> &'static [&'static str; 2] {
    &MEDIA_HTTP_LOCAL_DEMO_URLS
}

pub(crate) fn mark_decode_requested() {
    MEDIA_DECODE_REQUESTED.store(true, Ordering::Release);
}

fn take_media_source_stash(owner: &'static str) -> Option<MediaSource> {
    let stash = MEDIA_SOURCE_STASH.lock().take()?;
    crate::log!(
        "intel/media: source stash owner={} source={} bytes={}\n",
        owner,
        stash.source,
        stash.body.len(),
    );
    Some(MediaSource::Memory {
        source: stash.source,
        body: stash.body,
    })
}

fn store_media_source_stash(source: &'static str, body: Vec<u8>) {
    crate::log!("intel/media: source stash store source={} bytes={}\n", source, body.len(),);
    *MEDIA_SOURCE_STASH.lock() = Some(MediaSourceStash { source, body });
}

async fn fetch_media_source_inner_async(_owner: &'static str) -> Option<MediaSource> {
    // 1) Wait for FS, try cache.
    if crate::logflag::INTEL_MEDIA_FS_CACHE_ENABLED {
        if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
            let info = disk.info();
            crate::log!(
                "intel/media: cache probe path={} disk_id={} readonly={} block={} max_transfer={}\n",
                MEDIA_DECODE_CACHE_PATH,
                info.id.raw(),
                info.is_read_only() as u8,
                info.block_size,
                info.max_transfer_bytes,
            );
            match crate::r::fs::trueosfs::file_info_async(disk, MEDIA_DECODE_CACHE_PATH).await {
                Ok(Some(info)) if info.data_len != 0 => {
                    crate::log!(
                        "intel/media: cache hit path={} bytes={} source=file\n",
                        MEDIA_DECODE_CACHE_PATH,
                        info.data_len
                    );
                    return Some(MediaSource::CacheFile {
                        source: "cache",
                        disk,
                        path: MEDIA_DECODE_CACHE_PATH,
                        len: info.data_len,
                    });
                }
                Ok(_) => {
                    crate::log!("intel/media: cache miss path={}\n", MEDIA_DECODE_CACHE_PATH,);
                }
                Err(err) => {
                    crate::log!(
                        "intel/media: cache probe failed path={} err={:?}\n",
                        MEDIA_DECODE_CACHE_PATH,
                        err
                    );
                }
            }
        }
    }

    // 2) Fetch over HTTP.
    for url in MEDIA_HTTP_LOCAL_DEMO_URLS {
        crate::log!(
            "intel/media: try local url={} timeout_ms={} max_bytes={}\n",
            url,
            MEDIA_HTTP_LOCAL_DEMO_TIMEOUT_MS,
            MEDIA_HTTP_LOCAL_DEMO_MAX_BYTES,
        );
        match crate::r::net::http::fetch_http_body(
            url,
            MEDIA_HTTP_LOCAL_DEMO_TIMEOUT_MS,
            MEDIA_HTTP_LOCAL_DEMO_MAX_BYTES,
        )
        .await
        {
            Ok(body) => {
                // 3) Persist to cache.
                if crate::logflag::INTEL_MEDIA_FS_CACHE_ENABLED {
                    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
                        match persist_media_cache_async(
                            disk,
                            MEDIA_DECODE_CACHE_PATH,
                            body.as_slice(),
                        )
                        .await
                        {
                            Ok(true) => {
                                crate::log!(
                                    "intel/media: cached path={} bytes={}\n",
                                    MEDIA_DECODE_CACHE_PATH,
                                    body.len()
                                );
                            }
                            Ok(false) => {
                                crate::log!(
                                    "intel/media: cache write skipped path={}\n",
                                    MEDIA_DECODE_CACHE_PATH
                                );
                            }
                            Err(err) => {
                                crate::log!(
                                    "intel/media: cache write failed path={} err={:?}\n",
                                    MEDIA_DECODE_CACHE_PATH,
                                    err
                                );
                            }
                        }
                    }
                }
                return Some(MediaSource::Memory { source: url, body });
            }
            Err(err) => {
                crate::log!("intel/media: local fetch failed url={} err={:?}\n", url, err);
            }
        }
    }

    None
}

async fn acquire_media_source_fetch_guard_async(
    owner: &'static str,
) -> Option<MediaSourceFetchGuard> {
    let mut wait_polls = 0u32;
    loop {
        if MEDIA_SOURCE_FETCH_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            if wait_polls != 0 {
                crate::log!(
                    "intel/media: source fetch owner={} acquired after_wait_ms={}\n",
                    owner,
                    u64::from(wait_polls) * MEDIA_SOURCE_FETCH_WAIT_MS,
                );
            }
            return Some(MediaSourceFetchGuard);
        }

        if owner == "warmup" && MEDIA_DECODE_REQUESTED.load(Ordering::Acquire) {
            crate::log!(
                "intel/media: source fetch owner=warmup skipped reason=decode-requested while_waiting=1\n"
            );
            return None;
        }

        wait_polls = wait_polls.saturating_add(1);
        if wait_polls == 1 || wait_polls % MEDIA_SOURCE_FETCH_LOG_EVERY_POLLS == 0 {
            crate::log!(
                "intel/media: source fetch owner={} waiting active=1 waited_ms={}\n",
                owner,
                u64::from(wait_polls) * MEDIA_SOURCE_FETCH_WAIT_MS,
            );
        }
        Timer::after(EmbassyDuration::from_millis(MEDIA_SOURCE_FETCH_WAIT_MS)).await;
    }
}

pub(crate) async fn fetch_media_source_async(
    owner: &'static str,
) -> Option<MediaSource> {
    if let Some(stashed) = take_media_source_stash(owner) {
        return Some(stashed);
    }

    let _guard = acquire_media_source_fetch_guard_async(owner).await?;

    if let Some(stashed) = take_media_source_stash(owner) {
        return Some(stashed);
    }

    fetch_media_source_inner_async(owner).await
}

async fn warm_media_source_async() -> Option<(&'static str, usize)> {
    let _guard = acquire_media_source_fetch_guard_async("warmup").await?;
    let fetched = fetch_media_source_inner_async("warmup").await?;
    let source = fetched.source_name();
    let bytes = usize::try_from(fetched.total_len()).ok()?;
    if let MediaSource::Memory { source, body } = fetched {
        store_media_source_stash(source, body);
    }
    Some((source, bytes))
}

pub(crate) async fn run_media_source_warmup_async() {
    if MEDIA_SOURCE_WARM_RAN.swap(true, Ordering::AcqRel) {
        crate::log!("intel/media: source warmup skipped reason=already-ran\n");
        return;
    }

    if !crate::logflag::INTEL_MEDIA_FS_CACHE_ENABLED {
        crate::log!("intel/media: source warmup skipped reason=fs-cache-disabled\n");
        return;
    }

    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!("intel/media: source warmup skipped reason=no-root-disk\n");
        return;
    };

    let info = disk.info();
    crate::log!(
        "intel/media: source warmup root disk_id={} readonly={} block={} max_transfer={}\n",
        info.id.raw(),
        info.is_read_only() as u8,
        info.block_size,
        info.max_transfer_bytes,
    );

    if MEDIA_DECODE_REQUESTED.load(Ordering::Acquire) {
        crate::log!("intel/media: source warmup skipped reason=decode-requested\n");
        return;
    }

    match warm_media_source_async().await {
        Some((source, bytes)) => {
            crate::log!("intel/media: source warmup ready source={} bytes={}\n", source, bytes,);
        }
        None => {
            crate::log!("intel/media: source warmup failed reason=fetch-exhausted\n");
        }
    }
}

fn media_cache_chunk_bytes(info: &crate::disc::block::DeviceInfo) -> usize {
    let block_size = usize::max(info.block_size as usize, 1);
    let raw = if info.max_transfer_bytes > 0 {
        usize::min(info.max_transfer_bytes as usize, MEDIA_CACHE_WRITE_MAX_CHUNK_BYTES)
    } else {
        MEDIA_CACHE_WRITE_FALLBACK_CHUNK_BYTES
    };
    let aligned = raw - (raw % block_size);
    usize::max(aligned, block_size)
}

fn media_cache_bps(written: u64, started: Instant) -> u64 {
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms == 0 {
        0
    } else {
        ((written as u128).saturating_mul(1000) / u128::from(elapsed_ms)) as u64
    }
}

async fn persist_media_cache_async(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    bytes: &[u8],
) -> Result<bool, crate::disc::block::Error> {
    let info = disk.info();
    let chunk_bytes = media_cache_chunk_bytes(&info);
    crate::log!(
        "intel/media: cache write start path={} bytes={} disk_id={} kind={:?} block={} max_transfer={} chunk={} label={}\n",
        path,
        bytes.len(),
        info.id.raw(),
        info.kind,
        info.block_size,
        info.max_transfer_bytes,
        chunk_bytes,
        info.label.as_deref().unwrap_or("-"),
    );

    let Some(handle) =
        crate::r::fs::trueosfs::file_write_begin_async(disk, path, bytes.len() as u64).await?
    else {
        return Ok(false);
    };

    let started = Instant::now();
    let mut written = 0u64;
    let mut next_progress = MEDIA_CACHE_WRITE_PROGRESS_BYTES;
    let mut next_yield = MEDIA_CACHE_WRITE_YIELD_EVERY_BYTES;

    for chunk in bytes.chunks(chunk_bytes) {
        if let Err(err) = crate::r::fs::trueosfs::file_write_chunk_async(handle, chunk).await {
            let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
            crate::log!(
                "intel/media: cache write chunk failed path={} offset={} chunk={} err={:?}\n",
                path,
                written,
                chunk.len(),
                err,
            );
            return Err(err);
        }
        written = written.saturating_add(chunk.len() as u64);
        if written >= next_progress || written == bytes.len() as u64 {
            crate::log!(
                "intel/media: cache write progress path={} written={} total={} bps={} elapsed_ms={}\n",
                path,
                written,
                bytes.len(),
                media_cache_bps(written, started),
                started.elapsed().as_millis(),
            );
            next_progress = next_progress.saturating_add(MEDIA_CACHE_WRITE_PROGRESS_BYTES);
        }

        if written >= next_yield && written != bytes.len() as u64 {
            Timer::after(EmbassyDuration::from_millis(1)).await;
            next_yield = next_yield.saturating_add(MEDIA_CACHE_WRITE_YIELD_EVERY_BYTES);
        }
    }

    crate::log!(
        "intel/media: cache write flush path={} written={} elapsed_ms={}\n",
        path,
        written,
        started.elapsed().as_millis(),
    );
    if let Err(err) = crate::r::fs::trueosfs::file_write_finish_async(handle).await {
        crate::log!(
            "intel/media: cache write finish failed path={} written={} err={:?}\n",
            path,
            written,
            err,
        );
        return Err(err);
    }

    crate::log!(
        "intel/media: cache write committed path={} bytes={} bps={} elapsed_ms={}\n",
        path,
        written,
        media_cache_bps(written, started),
        started.elapsed().as_millis(),
    );
    Ok(true)
}
