extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

const ASSET_FETCH_IDLE_MS: u64 = 16;
const ASSET_FETCH_POLL_MS: u64 = 8;
const ASSET_FETCH_FIELD_MAX: usize = 512;
const ASSET_FETCH_QUEUE_CAP: usize = 256;
pub(crate) const ASSET_FETCH_WORKERS: usize = 4;

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
    pub bytes_len: usize,
}

#[derive(Default)]
struct AssetShack {
    queued: VecDeque<BrowserAssetRequest>,
    ready: VecDeque<BrowserAssetReady>,
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

fn pop_next_asset_request() -> Option<BrowserAssetRequest> {
    with_asset_shack(|shack| {
        while let Some(request) = shack.queued.pop_front() {
            if browser_alive(request.browser_instance_id, request.generation) {
                return Some(request);
            }
        }
        None
    })
}

fn store_ready_asset(request: BrowserAssetRequest, bytes_len: usize) -> usize {
    with_asset_shack(|shack| {
        shack.ready.push_back(BrowserAssetReady {
            browser_instance_id: request.browser_instance_id,
            generation: request.generation,
            tag: request.tag,
            url: request.url,
            kind: request.kind,
            bytes_len,
        });
        shack.ready.len()
    })
}

pub fn begin_browser_asset_refs(browser_instance_id: u32) -> i32 {
    let Some(slot) = generation_slot(browser_instance_id) else {
        return -1;
    };
    let next_generation = slot.fetch_add(1, Ordering::AcqRel).wrapping_add(1).max(1);
    let (dropped_queued, dropped_ready) = with_asset_shack(|shack| {
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
        (dropped_queued, dropped_ready)
    });
    crate::log!(
        "asset_shack: browser begin browser={} generation={} dropped_queued={} dropped_ready={} active_cancel_signal=1\n",
        browser_instance_id,
        next_generation,
        dropped_queued,
        dropped_ready
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
    with_asset_shack(|shack| {
        if shack.queued.len() >= ASSET_FETCH_QUEUE_CAP {
            return -4;
        }
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

        let ready_len = store_ready_asset(request.clone(), bytes.len());
        crate::log!(
            "asset_shack: fetch ready worker={} browser={} generation={} op_id={} tag={} kind={} bytes={} ready_queue={} url={}\n",
            worker_id,
            request.browser_instance_id,
            request.generation,
            op_id,
            request.tag,
            request.kind,
            bytes.len(),
            ready_len,
            request.url
        );
        return;
    }
}

#[embassy_executor::task(pool_size = 4)]
pub async fn asset_fetch_worker_task() {
    let worker_id = ASSET_FETCH_WORKER_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    crate::log!(
        "asset_shack: fetch worker started worker={} max_parallel={}\n",
        worker_id,
        ASSET_FETCH_WORKERS
    );
    loop {
        if let Some(request) = pop_next_asset_request() {
            fetch_asset_request(worker_id, request).await;
        } else {
            Timer::after(EmbassyDuration::from_millis(ASSET_FETCH_IDLE_MS)).await;
        }
    }
}
