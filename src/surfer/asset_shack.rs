extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

const ASSET_FETCH_IDLE_MS: u64 = 16;
const ASSET_FETCH_POLL_MS: u64 = 8;
const ASSET_BATCH_MONITOR_MS: u64 = 50;
const ASSET_BATCH_TIMEOUT_MS: u64 = 5_000;
const ASSET_FETCH_FIELD_MAX: usize = 512;
const ASSET_FETCH_QUEUE_CAP: usize = 256;
const ASSET_READY_CAP: usize = 256;
pub(crate) const ASSET_FETCH_WORKERS: usize = 4;
pub(crate) const ASSET_FETCH_POLICY_PCORE_IMAGE: u8 = 1;

#[derive(Clone, Debug)]
pub struct BrowserAssetRequest {
    pub browser_instance_id: u32,
    pub generation: u32,
    pub tag: String,
    pub url: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct BrowserAssetReady {
    pub browser_instance_id: u32,
    pub generation: u32,
    pub tag: String,
    pub url: String,
    pub kind: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
struct BrowserAssetBatch {
    browser_instance_id: u32,
    generation: u32,
    expected: usize,
    ready: usize,
    failed: usize,
    signaled: bool,
    deadline: Instant,
}

#[derive(Clone, Copy, Debug)]
struct AssetBatchSignal {
    browser_instance_id: u32,
    generation: u32,
    expected: usize,
    ready: usize,
    failed: usize,
    reason: &'static str,
}

#[derive(Default)]
struct AssetShack {
    queued: VecDeque<BrowserAssetRequest>,
    ready: VecDeque<BrowserAssetReady>,
    batches: Vec<BrowserAssetBatch>,
}

static ASSET_SHACK: Mutex<Option<AssetShack>> = Mutex::new(None);
static ASSET_GENERATIONS: [AtomicU32; crate::surfer::MAX_BROWSER_INSTANCE_ID as usize] =
    [const { AtomicU32::new(1) }; crate::surfer::MAX_BROWSER_INSTANCE_ID as usize];
static ASSET_FETCH_WORKER_SEQ: AtomicU32 = AtomicU32::new(0);

fn generation_slot(browser_instance_id: u32) -> Option<&'static AtomicU32> {
    if browser_instance_id == 0 || browser_instance_id > crate::surfer::MAX_BROWSER_INSTANCE_ID {
        return None;
    }
    ASSET_GENERATIONS.get(browser_instance_id.saturating_sub(1) as usize)
}

fn current_generation(browser_instance_id: u32) -> Option<u32> {
    Some(generation_slot(browser_instance_id)?.load(Ordering::Acquire))
}

fn browser_alive(browser_instance_id: u32, generation: u32) -> bool {
    current_generation(browser_instance_id) == Some(generation)
}

fn with_asset_shack<R>(f: impl FnOnce(&mut AssetShack) -> R) -> R {
    let mut guard = ASSET_SHACK.lock();
    let shack = guard.get_or_insert_with(AssetShack::default);
    f(shack)
}

fn bounded_utf8(ptr: *const u8, len: usize) -> Option<String> {
    if ptr.is_null() || len == 0 || len > ASSET_FETCH_FIELD_MAX {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    core::str::from_utf8(bytes).ok().map(String::from)
}

fn start_asset_fetch(url: &str) -> Result<u32, i32> {
    if url.starts_with('/') {
        trueos_qjs::async_fs::start_read_file(url.as_bytes())
    } else {
        trueos_qjs::async_fs::start_net_fetch_bytes(url.as_bytes())
    }
}

fn pop_next_asset_request(policy: u8) -> Option<BrowserAssetRequest> {
    with_asset_shack(|shack| {
        if policy == ASSET_FETCH_POLICY_PCORE_IMAGE {
            while let Some(index) = shack.queued.iter().position(|request| {
                !browser_alive(request.browser_instance_id, request.generation)
                    || request_prefers_perf_core(request)
            }) {
                let Some(request) = shack.queued.remove(index) else {
                    break;
                };
                if browser_alive(request.browser_instance_id, request.generation) {
                    return Some(request);
                }
            }
        }

        while let Some(request) = shack.queued.pop_front() {
            if browser_alive(request.browser_instance_id, request.generation) {
                return Some(request);
            }
        }
        None
    })
}

fn infer_decode_kind(kind: &str, url: &str, bytes: &[u8]) -> &'static str {
    let kind = kind.to_ascii_lowercase();
    let url = url.to_ascii_lowercase();
    if kind.contains("svg") || url.ends_with(".svg") || bytes.starts_with(b"<svg") {
        return "svg";
    }
    if kind.contains("png") || url.ends_with(".png") || bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return "png";
    }
    if kind.contains("jpg")
        || kind.contains("jpeg")
        || url.ends_with(".jpg")
        || url.ends_with(".jpeg")
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
    {
        return "jpeg";
    }
    "unknown"
}

fn request_prefers_perf_core(request: &BrowserAssetRequest) -> bool {
    matches!(infer_decode_kind(&request.kind, &request.url, &[]), "png" | "jpeg")
}

fn current_worker_residency() -> (usize, u8, &'static str) {
    let slot = crate::percpu::current_slot();
    let core_kind = crate::workers::core_kind_for_slot(slot as u32);
    let core_kind_name = match core_kind {
        crate::workers::CORE_KIND_PERF => "perf",
        crate::workers::CORE_KIND_EFF => "eff",
        _ => "unknown",
    };
    (slot, core_kind, core_kind_name)
}

fn decode_asset_rgba(kind: &str, url: &str, bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), i32> {
    match infer_decode_kind(kind, url, bytes) {
        "png" => crate::ui3::img::png_codec::decode_png_rgba(bytes)
            .map(|decoded| (decoded.width, decoded.height, decoded.rgba))
            .map_err(|err| err.code()),
        "jpeg" => crate::ui3::img::jpeg_codec::decode_jpeg_rgba(bytes)
            .map(|decoded| (decoded.width, decoded.height, decoded.rgba))
            .map_err(|err| err.code()),
        "svg" => crate::ui3::img::svg::render_svg_bytes_rgba(bytes)
            .map(|(info, rgba)| (info.width, info.height, rgba)),
        _ => Err(-8),
    }
}

fn browser_asset_batch_mut(
    batches: &mut Vec<BrowserAssetBatch>,
    browser_instance_id: u32,
    generation: u32,
) -> Option<&mut BrowserAssetBatch> {
    batches.iter_mut().find(|batch| {
        batch.browser_instance_id == browser_instance_id && batch.generation == generation
    })
}

fn note_asset_ref_queued(shack: &mut AssetShack, browser_instance_id: u32, generation: u32) {
    if let Some(batch) =
        browser_asset_batch_mut(&mut shack.batches, browser_instance_id, generation)
    {
        batch.expected = batch.expected.saturating_add(1);
        return;
    }
    shack.batches.push(BrowserAssetBatch {
        browser_instance_id,
        generation,
        expected: 1,
        ready: 0,
        failed: 0,
        signaled: false,
        deadline: Instant::now() + EmbassyDuration::from_millis(ASSET_BATCH_TIMEOUT_MS),
    });
}

fn note_asset_completed(browser_instance_id: u32, generation: u32, ready: bool) {
    with_asset_shack(|shack| {
        let Some(batch) =
            browser_asset_batch_mut(&mut shack.batches, browser_instance_id, generation)
        else {
            return;
        };
        if ready {
            batch.ready = batch.ready.saturating_add(1);
        } else {
            batch.failed = batch.failed.saturating_add(1);
        }
    });
}

fn store_ready_asset(
    request: BrowserAssetRequest,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
) -> usize {
    with_asset_shack(|shack| {
        while shack.ready.len() >= ASSET_READY_CAP {
            let _ = shack.ready.pop_front();
        }
        shack.ready.push_back(BrowserAssetReady {
            browser_instance_id: request.browser_instance_id,
            generation: request.generation,
            tag: request.tag,
            url: request.url,
            kind: request.kind,
            width,
            height,
            rgba,
        });
        shack.ready.len()
    })
}

pub(crate) fn ready_asset_for_tag(
    browser_instance_id: u32,
    tag: &str,
) -> Option<BrowserAssetReady> {
    let generation = current_generation(browser_instance_id)?;
    with_asset_shack(|shack| {
        shack
            .ready
            .iter()
            .rev()
            .find(|asset| {
                asset.browser_instance_id == browser_instance_id
                    && asset.generation == generation
                    && asset.tag == tag
            })
            .cloned()
    })
}

fn take_batch_signals() -> Vec<AssetBatchSignal> {
    let now = Instant::now();
    with_asset_shack(|shack| {
        let mut signals = Vec::new();
        for batch in &mut shack.batches {
            if batch.signaled || batch.expected == 0 {
                continue;
            }
            let done = batch.ready.saturating_add(batch.failed);
            let all_done = done >= batch.expected;
            let timed_out = now >= batch.deadline;
            if !all_done && !timed_out {
                continue;
            }
            batch.signaled = true;
            signals.push(AssetBatchSignal {
                browser_instance_id: batch.browser_instance_id,
                generation: batch.generation,
                expected: batch.expected,
                ready: batch.ready,
                failed: batch.failed,
                reason: if all_done { "all-ready" } else { "timeout" },
            });
        }
        signals
    })
}

pub fn begin_browser_asset_refs(browser_instance_id: u32) -> i32 {
    let Some(slot) = generation_slot(browser_instance_id) else {
        return -1;
    };
    let next_generation = slot.fetch_add(1, Ordering::AcqRel).wrapping_add(1).max(1);
    let (dropped_queued, dropped_ready, dropped_batches) = with_asset_shack(|shack| {
        let before = shack.queued.len();
        shack
            .queued
            .retain(|request| request.browser_instance_id != browser_instance_id);
        let dropped_queued = before.saturating_sub(shack.queued.len());

        let ready_before = shack.ready.len();
        shack
            .ready
            .retain(|asset| asset.browser_instance_id != browser_instance_id);
        let dropped_ready = ready_before.saturating_sub(shack.ready.len());

        let batches_before = shack.batches.len();
        shack
            .batches
            .retain(|batch| batch.browser_instance_id != browser_instance_id);
        let dropped_batches = batches_before.saturating_sub(shack.batches.len());
        (dropped_queued, dropped_ready, dropped_batches)
    });
    crate::log!(
        "asset_shack: browser begin browser={} generation={} dropped_queued={} dropped_ready={} dropped_batches={} active_cancel_signal=1\n",
        browser_instance_id,
        next_generation,
        dropped_queued,
        dropped_ready,
        dropped_batches
    );
    0
}

pub fn push_browser_asset_ref(
    browser_instance_id: u32,
    tag: String,
    url: String,
    kind: String,
) -> i32 {
    let Some(generation) = current_generation(browser_instance_id) else {
        return -1;
    };
    if crate::surfer::media_stream::is_stream_candidate(kind.as_str(), url.as_str()) {
        return crate::surfer::media_stream::push_candidate(
            crate::surfer::media_stream::BrowserMediaCandidate {
                browser_instance_id,
                generation,
                tag,
                url,
                kind,
            },
        ) as i32;
    }
    with_asset_shack(|shack| {
        if shack.queued.len() >= ASSET_FETCH_QUEUE_CAP {
            return -4;
        }
        note_asset_ref_queued(shack, browser_instance_id, generation);
        shack.queued.push_back(BrowserAssetRequest {
            browser_instance_id,
            generation,
            tag,
            url,
            kind,
        });
        shack.queued.len() as i32
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_browser_asset_refs_begin(browser_instance_id: u32) -> i32 {
    begin_browser_asset_refs(browser_instance_id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_browser_asset_ref_push(
    browser_instance_id: u32,
    tag_ptr: *const u8,
    tag_len: usize,
    url_ptr: *const u8,
    url_len: usize,
    kind_ptr: *const u8,
    kind_len: usize,
) -> i32 {
    let Some(tag) = bounded_utf8(tag_ptr, tag_len) else {
        return -2;
    };
    let Some(url) = bounded_utf8(url_ptr, url_len) else {
        return -3;
    };
    let kind = bounded_utf8(kind_ptr, kind_len).unwrap_or_else(|| String::from("asset"));
    push_browser_asset_ref(browser_instance_id, tag, url, kind)
}

async fn fetch_asset_request(worker_id: u32, request: BrowserAssetRequest) {
    let op_id = match start_asset_fetch(request.url.as_str()) {
        Ok(op_id) => op_id,
        Err(code) => {
            note_asset_completed(request.browser_instance_id, request.generation, false);
            crate::log!(
                "asset_shack: fetch start failed worker={} browser={} generation={} tag={} kind={} code={} url={}\n",
                worker_id,
                request.browser_instance_id,
                request.generation,
                request.tag,
                request.kind,
                code,
                request.url
            );
            return;
        }
    };

    crate::log!(
        "asset_shack: fetch begin worker={} browser={} generation={} op_id={} tag={} kind={} url={}\n",
        worker_id,
        request.browser_instance_id,
        request.generation,
        op_id,
        request.tag,
        request.kind,
        request.url
    );

    loop {
        if !browser_alive(request.browser_instance_id, request.generation) {
            let _ = trueos_qjs::async_fs::discard(op_id);
            crate::log!(
                "asset_shack: fetch canceled worker={} browser={} generation={} op_id={} tag={} url={}\n",
                worker_id,
                request.browser_instance_id,
                request.generation,
                op_id,
                request.tag,
                request.url
            );
            return;
        }

        let rc_or_done = trueos_qjs::async_fs::result_len(op_id);
        if rc_or_done == trueos_qjs::async_fs::FS_ERR_NOT_FOUND as isize {
            Timer::after(EmbassyDuration::from_millis(ASSET_FETCH_POLL_MS)).await;
            continue;
        }
        if rc_or_done < 0 {
            let _ = trueos_qjs::async_fs::discard(op_id);
            note_asset_completed(request.browser_instance_id, request.generation, false);
            crate::log!(
                "asset_shack: fetch failed worker={} browser={} generation={} op_id={} tag={} code={} url={}\n",
                worker_id,
                request.browser_instance_id,
                request.generation,
                op_id,
                request.tag,
                rc_or_done,
                request.url
            );
            return;
        }

        let len = rc_or_done as usize;
        let mut bytes: Vec<u8> = Vec::with_capacity(len);
        bytes.resize(len, 0);
        let got = trueos_qjs::async_fs::read_result(op_id, bytes.as_mut_ptr(), bytes.len());
        if got < 0 {
            let _ = trueos_qjs::async_fs::discard(op_id);
            note_asset_completed(request.browser_instance_id, request.generation, false);
            crate::log!(
                "asset_shack: fetch read failed worker={} browser={} generation={} op_id={} tag={} code={} url={}\n",
                worker_id,
                request.browser_instance_id,
                request.generation,
                op_id,
                request.tag,
                got,
                request.url
            );
            return;
        }
        bytes.truncate(got as usize);

        if !browser_alive(request.browser_instance_id, request.generation) {
            crate::log!(
                "asset_shack: fetch late-discard worker={} browser={} generation={} op_id={} tag={} bytes={} url={}\n",
                worker_id,
                request.browser_instance_id,
                request.generation,
                op_id,
                request.tag,
                bytes.len(),
                request.url
            );
            return;
        }

        let (width, height, rgba) = match decode_asset_rgba(&request.kind, &request.url, &bytes) {
            Ok(decoded) => decoded,
            Err(code) => {
                note_asset_completed(request.browser_instance_id, request.generation, false);
                let (slot, core_kind, core_kind_name) = current_worker_residency();
                crate::log!(
                    "asset_shack: decode failed worker={} slot={} core_kind={} core={} browser={} generation={} op_id={} tag={} kind={} code={} bytes={} url={}\n",
                    worker_id,
                    slot,
                    core_kind,
                    core_kind_name,
                    request.browser_instance_id,
                    request.generation,
                    op_id,
                    request.tag,
                    request.kind,
                    code,
                    bytes.len(),
                    request.url
                );
                return;
            }
        };

        let ready_len = store_ready_asset(request.clone(), width, height, rgba);
        note_asset_completed(request.browser_instance_id, request.generation, true);
        let (slot, core_kind, core_kind_name) = current_worker_residency();
        crate::log!(
            "asset_shack: fetch ready worker={} slot={} core_kind={} core={} browser={} generation={} op_id={} tag={} kind={} bytes={} image={}x{} ready_queue={} url={}\n",
            worker_id,
            slot,
            core_kind,
            core_kind_name,
            request.browser_instance_id,
            request.generation,
            op_id,
            request.tag,
            request.kind,
            bytes.len(),
            width,
            height,
            ready_len,
            request.url
        );
        return;
    }
}

#[embassy_executor::task]
pub async fn asset_batch_monitor_task() {
    crate::log!(
        "asset_shack: batch monitor started timeout_ms={} poll_ms={}\n",
        ASSET_BATCH_TIMEOUT_MS,
        ASSET_BATCH_MONITOR_MS
    );
    loop {
        for signal in take_batch_signals() {
            crate::log!(
                "asset_shack: batch ready browser={} generation={} reason={} expected={} ready={} failed={} timeout_ms={}\n",
                signal.browser_instance_id,
                signal.generation,
                signal.reason,
                signal.expected,
                signal.ready,
                signal.failed,
                ASSET_BATCH_TIMEOUT_MS
            );
            crate::surfer::signal_ui3_asset_batch_ready(signal.browser_instance_id);
        }
        Timer::after(EmbassyDuration::from_millis(ASSET_BATCH_MONITOR_MS)).await;
    }
}

#[embassy_executor::task(pool_size = 4)]
pub async fn asset_fetch_worker_task(policy: u8) {
    let worker_id = ASSET_FETCH_WORKER_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let policy_name = match policy {
        ASSET_FETCH_POLICY_PCORE_IMAGE => "pcore-image-png-jpeg",
        _ => "any",
    };
    let (slot, core_kind, core_kind_name) = current_worker_residency();
    crate::log!(
        "asset_shack: fetch worker started worker={} slot={} core_kind={} core={} max_parallel={} policy={}\n",
        worker_id,
        slot,
        core_kind,
        core_kind_name,
        ASSET_FETCH_WORKERS,
        policy_name
    );
    loop {
        if let Some(request) = pop_next_asset_request(policy) {
            fetch_asset_request(worker_id, request).await;
        } else {
            Timer::after(EmbassyDuration::from_millis(ASSET_FETCH_IDLE_MS)).await;
        }
    }
}
