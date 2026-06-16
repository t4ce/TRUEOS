extern crate alloc;

use super::dns::{self, DnsConfig};
use super::http::{self, HttpFetchError};
use super::hyper_io::{HyperBytesBody, HyperTokioIo};
use super::tls_stream::{self, TlsStreamError};
use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::TlsTimeouts;
use crate::r::io::cabi::{
    FS_ERR_BAD_PARAM, FS_ERR_BAD_PATH, FS_ERR_IO, FS_ERR_NO_SPACE, FS_ERR_NOT_FOUND,
    FS_ERR_TIMEOUT, FS_ERR_TOO_LARGE, NET_ERR_BAD_URL, NET_ERR_HTTP, NET_ERR_TIMEOUT,
    NET_ERR_TIMEOUT_BODY, NET_ERR_TIMEOUT_CONNECT, NET_ERR_TIMEOUT_DNS, NET_ERR_TIMEOUT_TLS,
    NET_ERR_TLS,
};
use crate::r::net::NetProfile;
use crate::wait::WaitQueue;
use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    pin::Pin,
    sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering},
};
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use hyper::body::Body;
use spin::Mutex;
use tokio::io::DuplexStream;
use v::vnet;

static CABI_NET_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);
static CABI_NET_FETCH_RESULTS: Mutex<BTreeMap<u32, Option<i32>>> = Mutex::new(BTreeMap::new());
struct CabiNetFetchBytesResult {
    rc: Option<i32>,
    body: Vec<u8>,
}

impl Default for CabiNetFetchBytesResult {
    fn default() -> Self {
        Self {
            rc: None,
            body: Vec::new(),
        }
    }
}

static CABI_NET_FETCH_BYTES_RESULTS: Mutex<BTreeMap<u32, CabiNetFetchBytesResult>> =
    Mutex::new(BTreeMap::new());
static CABI_NET_FETCH_WAIT: WaitQueue = WaitQueue::new();
static CABI_NET_FETCH_WAIT_MODE_LOGGED: AtomicU8 = AtomicU8::new(0);
const CABI_NET_FETCH_TASK_POOL_SIZE: usize = crate::allcaps::net::CABI_NET_FETCH_TASK_POOL_SIZE;
const NET_FETCH_MAX_CONCURRENCY: usize = crate::allcaps::net::CABI_NET_FETCH_MAX_CONCURRENCY;
static NET_FETCH_ACTIVE: AtomicUsize = AtomicUsize::new(0);
static NET_FETCH_CONCURRENCY_CAP_LOG_COUNT: AtomicU32 = AtomicU32::new(0);
static CABI_NET_FETCH_TASK_POOL_CAP_LOG_COUNT: AtomicU32 = AtomicU32::new(0);

#[inline]
fn wait_on_net_fetch_queue_blocking(timeout_ms: u64) -> bool {
    let ready = crate::r::readiness::is_set(
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
    );
    if ready {
        if CABI_NET_FETCH_WAIT_MODE_LOGGED
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            crate::log!("net-fetch-wait: mode=spin-ready\n");
        }
        return CABI_NET_FETCH_WAIT.wait_for_event_blocking(timeout_ms);
    }

    // Early boot and degraded bring-up still fall back to the conservative polling wait.
    if CABI_NET_FETCH_WAIT_MODE_LOGGED
        .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!("net-fetch-wait: mode=spin\n");
    }
    CABI_NET_FETCH_WAIT.wait_for_event_blocking(timeout_ms)
}
// Net-fetch scheduler (used by QJS URL module cache):
// - coalesces concurrent requests for the same cache key
// - caps concurrency to avoid TLS-handshake storms starving the executor

#[derive(Debug, Default)]
struct InflightFetch {
    owner_op_id: u32,
    followers: Vec<u32>,
}

static CABI_NET_FETCH_INFLIGHT: Mutex<BTreeMap<String, InflightFetch>> =
    Mutex::new(BTreeMap::new());

#[inline]
fn should_log_net_fetch_cap(count: u32) -> bool {
    count <= 8 || count.is_multiple_of(64)
}

fn log_net_fetch_concurrency_cap(op_kind: &'static str, op_id: u32, active: usize) {
    let count = NET_FETCH_CONCURRENCY_CAP_LOG_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if should_log_net_fetch_cap(count) {
        crate::log!(
            "WARNING net-fetch: concurrency cap reached kind={} op_id={} active={} cap={} count={}\n",
            op_kind,
            op_id,
            active,
            NET_FETCH_MAX_CONCURRENCY,
            count
        );
    }
}

fn log_net_fetch_task_pool_cap(op_kind: &'static str, op_id: u32, caller_slot: u32) {
    let count = CABI_NET_FETCH_TASK_POOL_CAP_LOG_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if should_log_net_fetch_cap(count) {
        crate::log!(
            "WARNING net-fetch: task pool cap reached kind={} op_id={} caller_slot={} pool_cap={} count={}\n",
            op_kind,
            op_id,
            caller_slot,
            CABI_NET_FETCH_TASK_POOL_SIZE,
            count
        );
    }
}

async fn net_fetch_acquire_slot() {
    loop {
        let cur = NET_FETCH_ACTIVE.load(Ordering::Relaxed);
        if cur < NET_FETCH_MAX_CONCURRENCY
            && NET_FETCH_ACTIVE
                .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return;
        }
        log_net_fetch_concurrency_cap("legacy", 0, cur);
        // Cooperative backoff.
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

async fn net_fetch_acquire_slot_while<F>(op_kind: &'static str, op_id: u32, is_needed: F) -> bool
where
    F: Fn() -> bool,
{
    loop {
        if !is_needed() {
            return false;
        }

        let cur = NET_FETCH_ACTIVE.load(Ordering::Relaxed);
        if cur < NET_FETCH_MAX_CONCURRENCY
            && NET_FETCH_ACTIVE
                .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            if !is_needed() {
                net_fetch_release_slot();
                return false;
            }
            return true;
        }

        log_net_fetch_concurrency_cap(op_kind, op_id, cur);
        // Cooperative backoff.
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

fn net_fetch_release_slot() {
    NET_FETCH_ACTIVE.fetch_sub(1, Ordering::AcqRel);
}

fn inflight_fetch_has_live_interest(
    owner_op_id: u32,
    followers: &[u32],
    results: &BTreeMap<u32, Option<i32>>,
) -> bool {
    results.contains_key(&owner_op_id) || followers.iter().any(|id| results.contains_key(id))
}

fn net_fetch_file_task_has_interest(op_id: u32, key: &str) -> bool {
    let (owner_op_id, followers) = {
        let inflight = CABI_NET_FETCH_INFLIGHT.lock();
        let Some(entry) = inflight.get(key) else {
            return false;
        };
        if entry.owner_op_id != op_id {
            return false;
        }
        (entry.owner_op_id, entry.followers.clone())
    };

    let results = CABI_NET_FETCH_RESULTS.lock();
    inflight_fetch_has_live_interest(owner_op_id, followers.as_slice(), &results)
}

fn net_fetch_bytes_op_is_live(op_id: u32) -> bool {
    CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&op_id)
}

async fn cabi_net_fetch_task_inner(
    op_id: u32,
    key: String,
    url: String,
    path: String,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let _host_alloc_domain = crate::allocators::enter_host_alloc_domain_current_cpu();
    let t0 = Instant::now();
    if !net_fetch_acquire_slot_while("file", op_id, || {
        net_fetch_file_task_has_interest(op_id, key.as_str())
    })
    .await
    {
        crate::log!("net-fetch: skipped key={} reason=no_interest_before_slot\n", key);
        return;
    }
    let t_fetch_start = Instant::now();
    if !net_fetch_file_task_has_interest(op_id, key.as_str()) {
        net_fetch_release_slot();
        crate::log!("net-fetch: skipped key={} reason=no_interest_after_slot\n", key);
        return;
    }
    let rc =
        match fetch_https_to_file_hyper_async(url.as_str(), path.as_str(), timeout_ms, max_bytes)
            .await
        {
            Ok(()) => 0,
            Err(code) => code,
        };
    net_fetch_release_slot();
    let total_ms = t0.elapsed().as_millis();
    let wait_ms = t_fetch_start.saturating_duration_since(t0).as_millis();
    let fetch_ms = total_ms.saturating_sub(wait_ms);

    let followers = {
        let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
        inflight
            .remove(&key)
            .map(|e| e.followers)
            .unwrap_or_default()
    };

    let mut map = CABI_NET_FETCH_RESULTS.lock();
    if let Some(slot) = map.get_mut(&op_id) {
        *slot = Some(rc);
    }
    for fid in &followers {
        if let Some(slot) = map.get_mut(fid) {
            *slot = Some(rc);
        }
    }

    crate::log!(
        "net-fetch: done key={} rc={} ms={} wait_ms={} fetch_ms={} followers={}\n",
        key,
        rc,
        total_ms,
        wait_ms,
        fetch_ms,
        followers.len()
    );

    CABI_NET_FETCH_WAIT.notify_all();
}

#[embassy_executor::task(pool_size = CABI_NET_FETCH_TASK_POOL_SIZE)]
async fn cabi_net_fetch_task(
    op_id: u32,
    key: String,
    url: String,
    path: String,
    timeout_ms: u32,
    max_bytes: usize,
) {
    cabi_net_fetch_task_inner(op_id, key, url, path, timeout_ms, max_bytes).await;
}

fn spawn_cabi_net_fetch(
    op_id: u32,
    key: String,
    url: String,
    path: String,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let caller_slot = crate::percpu::current_slot() as u32;
    let picked_spawner = pick_net_fetch_background_spawner(caller_slot);

    if let Some((_cpu_slot, _core_kind, spawner)) = picked_spawner {
        if let Ok(token) = cabi_net_fetch_task(
            op_id,
            key.clone(),
            url.clone(),
            path.clone(),
            timeout_ms,
            max_bytes,
        ) {
            spawner.spawn(token);
            return;
        }
        log_net_fetch_task_pool_cap("file", op_id, caller_slot);
    }

    if vmx_guest_cabi_context() {
        crate::log!(
            "net-fetch: spawn op_id={} refused local fallback caller_slot={} reason=vmx_guest_context\n",
            op_id,
            caller_slot
        );
        if let Some(slot) = CABI_NET_FETCH_RESULTS.lock().get_mut(&op_id) {
            *slot = Some(FS_ERR_IO);
        }
        {
            let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
            if inflight
                .get(&key)
                .map(|entry| entry.owner_op_id == op_id)
                .unwrap_or(false)
            {
                inflight.remove(&key);
            }
        }
        CABI_NET_FETCH_WAIT.notify_all();
        return;
    }

    crate::wait::spawn_local_detached(async move {
        cabi_net_fetch_task_inner(op_id, key, url, path, timeout_ms, max_bytes).await;
    });
}

async fn cabi_net_fetch_bytes_task_inner(
    op_id: u32,
    url: String,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let _host_alloc_domain = crate::allocators::enter_host_alloc_domain_current_cpu();
    let cpu_slot = crate::percpu::current_slot();
    let lapic_id = crate::percpu::current_lapic_id_via_cpuid();
    let t0 = Instant::now();
    crate::log!(
        "net-fetch-bytes: enter op_id={} cpu_slot={} lapic={} timeout_ms={} max_bytes={}\n",
        op_id,
        cpu_slot,
        lapic_id,
        timeout_ms,
        max_bytes
    );
    if !net_fetch_acquire_slot_while("bytes", op_id, || net_fetch_bytes_op_is_live(op_id)).await {
        crate::log!("net-fetch-bytes: skipped op_id={} reason=no_interest_before_slot\n", op_id);
        return;
    }
    let t_fetch_start = Instant::now();
    crate::log!(
        "net-fetch-bytes: acquired op_id={} wait_ms={} cpu_slot={} lapic={}\n",
        op_id,
        t_fetch_start.saturating_duration_since(t0).as_millis(),
        cpu_slot,
        lapic_id
    );
    if !net_fetch_bytes_op_is_live(op_id) {
        net_fetch_release_slot();
        crate::log!("net-fetch-bytes: skipped op_id={} reason=no_interest_after_slot\n", op_id);
        return;
    }
    let (rc, body) = match get_bytes_shared(url.clone(), timeout_ms, max_bytes).await {
        Ok(body) => (0, body),
        Err(SharedFetchError::Fetch(err)) => (fetch_error_to_code(err), Vec::new()),
        Err(SharedFetchError::Runtime) => {
            crate::log!(
                "net-fetch-bytes: shared runtime unavailable op_id={} url={}\n",
                op_id,
                url
            );
            (FS_ERR_IO, Vec::new())
        }
    };
    net_fetch_release_slot();
    let total_ms = t0.elapsed().as_millis();
    let wait_ms = t_fetch_start.saturating_duration_since(t0).as_millis();
    let fetch_ms = total_ms.saturating_sub(wait_ms);

    if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&op_id) {
        slot.rc = Some(rc);
        slot.body = body;
    }

    crate::log!(
        "net-fetch-bytes: done transport=hyper op_id={} rc={} ms={} wait_ms={} fetch_ms={} len={}\n",
        op_id,
        rc,
        total_ms,
        wait_ms,
        fetch_ms,
        CABI_NET_FETCH_BYTES_RESULTS
            .lock()
            .get(&op_id)
            .map(|v| v.body.len())
            .unwrap_or(0)
    );

    CABI_NET_FETCH_WAIT.notify_all();
}

#[embassy_executor::task(pool_size = CABI_NET_FETCH_TASK_POOL_SIZE)]
async fn cabi_net_fetch_bytes_task(op_id: u32, url: String, timeout_ms: u32, max_bytes: usize) {
    cabi_net_fetch_bytes_task_inner(op_id, url, timeout_ms, max_bytes).await;
}

fn spawn_cabi_net_fetch_bytes(op_id: u32, url: String, timeout_ms: u32, max_bytes: usize) {
    let caller_slot = crate::percpu::current_slot() as u32;
    let picked_spawner = pick_net_fetch_background_spawner(caller_slot);

    if let Some((cpu_slot, core_kind, spawner)) = picked_spawner {
        if let Ok(token) = cabi_net_fetch_bytes_task(op_id, url.clone(), timeout_ms, max_bytes) {
            crate::log!(
                "net-fetch-bytes: spawn op_id={} lane=background caller_slot={} cpu_slot={} core_kind={} timeout_ms={} max_bytes={} url_len={}\n",
                op_id,
                caller_slot,
                cpu_slot,
                core_kind,
                timeout_ms,
                max_bytes,
                url.len()
            );
            spawner.spawn(token);
            return;
        }
        log_net_fetch_task_pool_cap("bytes", op_id, caller_slot);
    }

    if vmx_guest_cabi_context() {
        crate::log!(
            "net-fetch-bytes: spawn op_id={} refused local fallback caller_slot={} reason=vmx_guest_context\n",
            op_id,
            caller_slot
        );
        if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&op_id) {
            slot.rc = Some(FS_ERR_IO);
            slot.body.clear();
        }
        CABI_NET_FETCH_WAIT.notify_all();
        return;
    }
    crate::log!(
        "net-fetch-bytes: spawn op_id={} lane=local caller_slot={} timeout_ms={} max_bytes={} url_len={}\n",
        op_id,
        caller_slot,
        timeout_ms,
        max_bytes,
        url.len()
    );
    crate::wait::spawn_local_detached(async move {
        cabi_net_fetch_bytes_task_inner(op_id, url, timeout_ms, max_bytes).await;
    });
}

fn pick_net_fetch_background_spawner(
    caller_slot: u32,
) -> Option<(u32, u8, crate::workers::WorkerSpawner)> {
    crate::workers::pick_background_spawner_where(|slot| {
        (!crate::workers::is_background_worker_slot(caller_slot) || slot != caller_slot)
            && crate::hv::lane::is_carrier_lane_free(slot)
    })
}

#[inline]
fn vmx_guest_cabi_context() -> bool {
    crate::hv::current_hull_guest_context_vm_id().is_some()
}

fn guest_fetch_bytes_start(url: &[u8]) -> u32 {
    if url.is_empty() || url.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return 0;
    }
    let mut out = [0u8; 1];
    let (status, op_id) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_FETCH_BYTES_START,
        0,
        0,
        url,
        &mut out,
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        op_id as u32
    } else {
        0
    }
}

fn guest_fetch_bytes_result_len(op_id: u32) -> isize {
    let (status, value) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_FETCH_BYTES_RESULT_LEN, op_id as u64, 0);
    if status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as isize
    } else {
        FS_ERR_BAD_PARAM as isize
    }
}

fn guest_fetch_bytes_read(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize {
    let len = guest_fetch_bytes_result_len(op_id);
    if len < 0 {
        return len;
    }
    let len = len as usize;
    if out_ptr.is_null() || out_cap == 0 {
        return len as isize;
    }
    if len > out_cap {
        return FS_ERR_NO_SPACE as isize;
    }

    let mut copied = 0usize;
    while copied < len {
        let mut chunk = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, value) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_FETCH_BYTES_READ,
            op_id as u64,
            copied as u64,
            &[],
            &mut chunk,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return (value as i64) as isize;
        }
        let got = value as usize;
        if got == 0 {
            return FS_ERR_IO as isize;
        }
        let n = core::cmp::min(got, len.saturating_sub(copied));
        unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), out_ptr.add(copied), n) };
        copied += n;
    }

    if len == 0 {
        let mut out = [0u8; 1];
        let _ = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_FETCH_BYTES_READ,
            op_id as u64,
            0,
            &[],
            &mut out,
        );
    }

    copied as isize
}

fn guest_fetch_bytes_discard(op_id: u32) -> i32 {
    let (status, value) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_FETCH_BYTES_DISCARD, op_id as u64, 0);
    if status == trueos_vm::vmcall::STATUS_OK {
        value as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

fn guest_fetch_file_start(url: &[u8], path: &[u8]) -> u32 {
    if url.is_empty() || path.is_empty() {
        return 0;
    }
    let Some(payload_len) = url.len().checked_add(path.len()) else {
        return 0;
    };
    if payload_len > trueos_vm::vmcall::PAYLOAD_CAP {
        return 0;
    }

    let mut payload = Vec::with_capacity(payload_len);
    payload.extend_from_slice(url);
    payload.extend_from_slice(path);

    let mut out = [0u8; 1];
    let (status, op_id) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_FETCH_FILE_START,
        url.len() as u64,
        0,
        payload.as_slice(),
        &mut out,
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        op_id as u32
    } else {
        0
    }
}

fn guest_fetch_file_result(op_id: u32) -> i32 {
    let (status, value) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_FETCH_FILE_RESULT, op_id as u64, 0);
    if status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

fn guest_fetch_file_discard(op_id: u32) -> i32 {
    let (status, value) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_FETCH_FILE_DISCARD, op_id as u64, 0);
    if status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        FS_ERR_BAD_PARAM
    }
}

fn wait_on_net_fetch_or_guest_sleep(step: EmbassyDuration) {
    let ms = step.as_millis() as u64;
    if vmx_guest_cabi_context() {
        trueos_vm::vmcall::sleep_ms(ms.max(1));
    } else {
        let _ = wait_on_net_fetch_queue_blocking(ms);
    }
}

pub(crate) fn cabi_net_fetch_start_host(
    url_s: &str,
    path_s: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> u32 {
    let key = match normalize_rel(path_s, false) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    let url = String::from(url_s);
    let path = String::from(path_s);
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);

    {
        let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
        if let Some(entry) = inflight.get_mut(&key) {
            entry.followers.push(op_id);
            return op_id;
        }
        inflight.insert(
            key.clone(),
            InflightFetch {
                owner_op_id: op_id,
                followers: Vec::new(),
            },
        );
    }

    spawn_cabi_net_fetch(op_id, key, url, path, timeout_ms, max_bytes);
    op_id
}

pub(crate) fn cabi_net_fetch_result_host(op_id: u32) -> i32 {
    let map = CABI_NET_FETCH_RESULTS.lock();
    match map.get(&op_id) {
        Some(Some(rc)) => *rc,
        Some(None) => FS_ERR_NOT_FOUND,
        None => FS_ERR_NOT_FOUND,
    }
}

pub(crate) fn cabi_net_fetch_discard_host(op_id: u32) -> i32 {
    let mut map = CABI_NET_FETCH_RESULTS.lock();
    map.remove(&op_id);

    {
        let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
        let mut dead_keys: Vec<String> = Vec::new();
        for (k, v) in inflight.iter_mut() {
            v.followers.retain(|&id| id != op_id);
            if !inflight_fetch_has_live_interest(v.owner_op_id, v.followers.as_slice(), &map) {
                dead_keys.push(k.clone());
            }
        }
        for key in dead_keys {
            inflight.remove(&key);
        }
    }
    0
}

pub(crate) fn cabi_net_fetch_bytes_start_host(
    url_s: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> u32 {
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(op_id, CabiNetFetchBytesResult::default());
    crate::log!(
        "net-fetch-bytes: start op_id={} timeout_ms={} max_bytes={} url={}\n",
        op_id,
        timeout_ms,
        max_bytes,
        url_s
    );
    spawn_cabi_net_fetch_bytes(op_id, String::from(url_s), timeout_ms, max_bytes);
    op_id
}

pub(crate) fn cabi_net_fetch_bytes_result_len_host(op_id: u32) -> isize {
    let map = CABI_NET_FETCH_BYTES_RESULTS.lock();
    match map.get(&op_id) {
        Some(v) => match v.rc {
            Some(0) => v.body.len() as isize,
            Some(rc) => rc as isize,
            None => FS_ERR_NOT_FOUND as isize,
        },
        None => FS_ERR_NOT_FOUND as isize,
    }
}

pub(crate) fn cabi_net_fetch_bytes_read_chunk_host(
    op_id: u32,
    offset: usize,
    out: &mut [u8],
) -> isize {
    let mut map = CABI_NET_FETCH_BYTES_RESULTS.lock();
    let Some(entry) = map.get(&op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    let Some(rc) = entry.rc else {
        return FS_ERR_NOT_FOUND as isize;
    };
    if rc != 0 {
        map.remove(&op_id);
        return rc as isize;
    }
    let len = entry.body.len();
    if offset > len {
        return FS_ERR_BAD_PARAM as isize;
    }
    let n = core::cmp::min(out.len(), len.saturating_sub(offset));
    if n != 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(entry.body.as_ptr().add(offset), out.as_mut_ptr(), n)
        };
    }
    if offset.saturating_add(n) >= len {
        map.remove(&op_id);
    }
    n as isize
}

pub(crate) fn cabi_net_fetch_bytes_discard_host(op_id: u32) -> i32 {
    CABI_NET_FETCH_BYTES_RESULTS.lock().remove(&op_id);
    0
}

async fn cabi_net_fetch_post_json_task_inner(
    op_id: u32,
    key: String,
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let _host_alloc_domain = crate::allocators::enter_host_alloc_domain_current_cpu();
    let t0 = Instant::now();
    net_fetch_acquire_slot().await;

    let rc = match post_json_body_async(
        url.as_str(),
        body_json,
        bearer.as_deref(),
        timeout_ms,
        max_bytes,
    )
    .await
    {
        Ok(bytes) => {
            crate::log!("net-fetch-post: response_body_len={}\n", bytes.len());
            if let Ok(s) = core::str::from_utf8(bytes.as_slice()) {
                if let Some(summary) = crate::r::net::json::summarize_openai_response_json(s) {
                    crate::log!("net-fetch-post: summary {}\n", summary);
                }
                log_utf8_chunks("net-fetch-post: response_json: ", s);
            } else {
                crate::log!("net-fetch-post: response_json: [non-utf8]\n");
            }
            if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
                match crate::r::fs::trueosfs::file_in_async(disk, key.as_str(), bytes.as_slice())
                    .await
                {
                    Ok(true) => 0,
                    Ok(false) => FS_ERR_IO,
                    Err(e) => block_error_to_code(e),
                }
            } else {
                FS_ERR_NOT_FOUND
            }
        }
        Err(rc) => rc,
    };

    net_fetch_release_slot();

    let elapsed_ms = t0.elapsed().as_millis();
    if let Some(slot) = CABI_NET_FETCH_RESULTS.lock().get_mut(&op_id) {
        *slot = Some(rc);
    }

    crate::log!("net-fetch-post: done key={} rc={} ms={}\n", key, rc, elapsed_ms);

    CABI_NET_FETCH_WAIT.notify_all();
}

#[embassy_executor::task]
async fn cabi_net_fetch_post_json_task(
    op_id: u32,
    key: String,
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    cabi_net_fetch_post_json_task_inner(op_id, key, url, body_json, bearer, timeout_ms, max_bytes)
        .await;
}

fn spawn_cabi_net_fetch_post_json(
    op_id: u32,
    key: String,
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let caller_slot = crate::percpu::current_slot() as u32;
    if let Some((_cpu_slot, _core_kind, spawner)) = pick_net_fetch_background_spawner(caller_slot)
        && let Ok(token) = cabi_net_fetch_post_json_task(
            op_id,
            key.clone(),
            url.clone(),
            body_json.clone(),
            bearer.clone(),
            timeout_ms,
            max_bytes,
        )
    {
        spawner.spawn(token);
        return;
    }

    crate::wait::spawn_local_detached(async move {
        cabi_net_fetch_post_json_task_inner(
            op_id, key, url, body_json, bearer, timeout_ms, max_bytes,
        )
        .await;
    });
}

async fn cabi_net_fetch_post_json_bytes_task_inner(
    request_id: u32,
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let _host_alloc_domain = crate::allocators::enter_host_alloc_domain_current_cpu();
    let t0 = Instant::now();
    net_fetch_acquire_slot().await;

    let (rc, bytes) = match post_json_body_async(
        url.as_str(),
        body_json,
        bearer.as_deref(),
        timeout_ms,
        max_bytes,
    )
    .await
    {
        Ok(bytes) => {
            crate::log!("net-fetch-post: response_body_len={}\n", bytes.len());
            if let Ok(s) = core::str::from_utf8(bytes.as_slice()) {
                if let Some(summary) = crate::r::net::json::summarize_openai_response_json(s) {
                    crate::log!("net-fetch-post: summary {}\n", summary);
                }
                log_utf8_chunks("net-fetch-post: response_json: ", s);
            } else {
                crate::log!("net-fetch-post: response_json: [non-utf8]\n");
            }
            (0, bytes)
        }
        Err(rc) => (rc, Vec::new()),
    };

    net_fetch_release_slot();

    if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&request_id) {
        slot.rc = Some(rc);
        slot.body = bytes;
    }

    let elapsed_ms = t0.elapsed().as_millis();
    crate::log!(
        "net-fetch-post: done request_id={} rc={} ms={} len={}\n",
        request_id,
        rc,
        elapsed_ms,
        CABI_NET_FETCH_BYTES_RESULTS
            .lock()
            .get(&request_id)
            .map(|v| v.body.len())
            .unwrap_or(0)
    );

    CABI_NET_FETCH_WAIT.notify_all();
}

#[embassy_executor::task]
async fn cabi_net_fetch_post_json_bytes_task(
    request_id: u32,
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    cabi_net_fetch_post_json_bytes_task_inner(
        request_id, url, body_json, bearer, timeout_ms, max_bytes,
    )
    .await;
}

fn spawn_cabi_net_fetch_post_json_bytes(
    request_id: u32,
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    let caller_slot = crate::percpu::current_slot() as u32;
    if let Some((_cpu_slot, _core_kind, spawner)) = pick_net_fetch_background_spawner(caller_slot)
        && let Ok(token) = cabi_net_fetch_post_json_bytes_task(
            request_id,
            url.clone(),
            body_json.clone(),
            bearer.clone(),
            timeout_ms,
            max_bytes,
        )
    {
        spawner.spawn(token);
        return;
    }

    crate::wait::spawn_local_detached(async move {
        cabi_net_fetch_post_json_bytes_task_inner(
            request_id, url, body_json, bearer, timeout_ms, max_bytes,
        )
        .await;
    });
}

async fn cabi_net_prewarm_url_task_inner(url: String) {
    let _host_alloc_domain = crate::allocators::enter_host_alloc_domain_current_cpu();
    let Some(parsed) = parse_https_url(url.as_str()) else {
        return;
    };
    let profile = NetProfile::default();
    let Some(dev_idx) = profile.resolve_device_index() else {
        return;
    };
    let _ = dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_profile(profile),
    )
    .await;
}

#[embassy_executor::task]
async fn cabi_net_prewarm_url_task(url: String) {
    cabi_net_prewarm_url_task_inner(url).await;
}

fn spawn_cabi_net_prewarm_url(url: String) {
    let caller_slot = crate::percpu::current_slot() as u32;
    if let Some((_cpu_slot, _core_kind, spawner)) = pick_net_fetch_background_spawner(caller_slot)
        && let Ok(token) = cabi_net_prewarm_url_task(url.clone())
    {
        spawner.spawn(token);
        return;
    }

    crate::wait::spawn_local_detached(async move {
        cabi_net_prewarm_url_task_inner(url).await;
    });
}

/// Errors returned by hyper HTTPS fetch helpers.
#[derive(Clone, Debug)]
pub enum FetchError {
    NoNic,
    BadUrl,
    DnsFailed,
    DnsTimeout,
    ConnectTimeout,
    TlsTimeout,
    BodyTimeout,
    Tls,
    Http(u16),
    Redirect { status: u16, url: String },
    ResponseTooLarge,
}

#[inline]
fn fetch_device_index(profile: NetProfile) -> Result<usize, FetchError> {
    profile.resolve_device_index().ok_or(FetchError::NoNic)
}
#[inline]
fn block_error_to_code(err: crate::disc::block::Error) -> i32 {
    use crate::disc::block::Error;
    match err {
        Error::InvalidParam | Error::OutOfBounds => FS_ERR_BAD_PARAM,
        Error::NotReady => FS_ERR_NOT_FOUND,
        Error::Corrupted
        | Error::Io
        | Error::Timeout
        | Error::NotSupported
        | Error::DmaUnavailable
        | Error::MmioMapFailed => FS_ERR_IO,
    }
}

#[inline]
fn fetch_error_to_code(err: FetchError) -> i32 {
    match err {
        FetchError::NoNic => NET_ERR_TIMEOUT,
        FetchError::BadUrl => NET_ERR_BAD_URL,
        FetchError::DnsFailed | FetchError::DnsTimeout => NET_ERR_TIMEOUT_DNS,
        FetchError::ConnectTimeout => NET_ERR_TIMEOUT_CONNECT,
        FetchError::TlsTimeout => NET_ERR_TIMEOUT_TLS,
        FetchError::BodyTimeout => NET_ERR_TIMEOUT_BODY,
        FetchError::Tls => NET_ERR_TLS,
        FetchError::Http(status) => {
            let _status = status;
            NET_ERR_HTTP
        }
        FetchError::Redirect { .. } => NET_ERR_HTTP,
        FetchError::ResponseTooLarge => FS_ERR_TOO_LARGE,
    }
}

#[inline]
fn http_fetch_error_to_code(err: HttpFetchError) -> i32 {
    match err {
        HttpFetchError::BadUrl => NET_ERR_BAD_URL,
        HttpFetchError::TimedOut => NET_ERR_TIMEOUT,
        HttpFetchError::DnsFailed => NET_ERR_TIMEOUT_DNS,
        HttpFetchError::HttpStatus(status) => {
            let _status = status;
            NET_ERR_HTTP
        }
        HttpFetchError::Redirect(_) => NET_ERR_HTTP,
        HttpFetchError::ResponseTooLarge => FS_ERR_TOO_LARGE,
        HttpFetchError::NoSpace => FS_ERR_NO_SPACE,
        HttpFetchError::Truncated => NET_ERR_TIMEOUT_BODY,
    }
}

async fn post_json_body_async(
    url: &str,
    body_json: String,
    bearer: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, i32> {
    if url.starts_with("http://") {
        let auth_header = bearer.map(|token| alloc::format!("Bearer {}", token));
        let headers_with_auth = [
            ("Accept", "application/json"),
            ("Authorization", auth_header.as_deref().unwrap_or_default()),
        ];
        let headers_without_auth = [("Accept", "application/json")];
        let headers = if auth_header.is_some() {
            &headers_with_auth[..]
        } else {
            &headers_without_auth[..]
        };
        return http::post_http_body_hyper_with_headers(
            url,
            "application/json",
            headers,
            body_json.as_bytes(),
            timeout_ms,
            max_bytes,
        )
        .await
        .map_err(http_fetch_error_to_code);
    }

    post_https_json_hyper_async(url, body_json, bearer, timeout_ms, max_bytes)
        .await
        .map_err(fetch_error_to_code)
}

#[inline]
fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn normalize_rel(path: &str, allow_empty: bool) -> Result<String, i32> {
    let mut out = String::new();
    let t = path.trim();
    if t.is_empty() {
        return if allow_empty {
            Ok(out)
        } else {
            Err(FS_ERR_BAD_PATH)
        };
    }

    for part in t.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(FS_ERR_BAD_PATH);
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }

    if out.is_empty() && !allow_empty {
        return Err(FS_ERR_BAD_PATH);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
struct ParsedHttpsUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_https_url(url: &str) -> Option<ParsedHttpsUrl> {
    let url = url.strip_prefix("https://")?;

    // Split authority and path.
    let (authority, path) = match url.split_once('/') {
        Some((a, p)) => (a, format!("/{}", p)),
        None => (url, String::from("/")),
    };

    if authority.is_empty() {
        return None;
    }

    // Parse optional ":port" in authority.
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        // Only treat as port if digits.
        if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            let port = p.parse::<u16>().ok()?;
            (String::from(h), port)
        } else {
            (String::from(authority), 443)
        }
    } else {
        (String::from(authority), 443)
    };

    if host.is_empty() {
        return None;
    }

    Some(ParsedHttpsUrl { host, port, path })
}

fn log_utf8_chunks(prefix: &str, s: &str) {
    // Avoid log-line truncation by splitting into multiple lines.
    // UTF-8 safe: ensure chunk boundaries are on char boundaries.
    const CHUNK: usize = 768;
    let mut i = 0usize;
    while i < s.len() {
        let mut end = (i + CHUNK).min(s.len());
        while end < s.len() && !s.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        if end == i {
            // Avoid infinite loop on unexpected boundary issues.
            end = (i + 1).min(s.len());
            while end < s.len() && !s.is_char_boundary(end) {
                end += 1;
            }
        }
        crate::log!("{}{}\n", prefix, &s[i..end]);
        i = end;
    }
}
async fn connect_hyper_tls_stream(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
) -> Result<DuplexStream, FetchError> {
    crate::log!("vhttps-hyper: dns begin host={} dev={}\n", parsed.host, dev_idx);
    let ip = match dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_device(dev_idx),
    )
    .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => {
            crate::log!(
                "vhttps-hyper: dns failed host={} dev={} err=timeout\n",
                parsed.host,
                dev_idx
            );
            return Err(FetchError::DnsTimeout);
        }
        Err(err) => {
            crate::log!(
                "vhttps-hyper: dns failed host={} dev={} err={:?}\n",
                parsed.host,
                dev_idx,
                err
            );
            return Err(FetchError::DnsFailed);
        }
    };
    crate::log!(
        "vhttps-hyper: dns ok host={} dev={} ip={}.{}.{}.{}\n",
        parsed.host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    crate::log!(
        "vhttps-hyper: connect host={} dev={} port={}\n",
        parsed.host,
        dev_idx,
        parsed.port
    );
    tls_stream::connect_tls_v4_stream(
        dev_idx,
        vnet::EndpointV4 {
            addr: ip,
            port: parsed.port,
        },
        parsed.host.clone(),
        cfg,
        roots,
        TlsTimeouts {
            connect_ms: (timeout_ms / 4).max(5_000),
            tls_ms: (timeout_ms / 4).max(5_000),
            idle_ms: timeout_ms,
        },
        timeout_ms,
        64 * 1024,
        "vhttps-hyper",
    )
    .await
    .map_err(|err| {
        crate::log!(
            "vhttps-hyper: connect failed host={} dev={} stage={}\n",
            parsed.host,
            dev_idx,
            err.as_stage()
        );
        match err {
            TlsStreamError::TlsTimedOut => FetchError::TlsTimeout,
            TlsStreamError::BridgeRead
            | TlsStreamError::BridgeWrite
            | TlsStreamError::QueueFull => FetchError::BodyTimeout,
            TlsStreamError::Tls => FetchError::Tls,
            TlsStreamError::OpenTimedOut | TlsStreamError::ConnectTimedOut => {
                FetchError::ConnectTimeout
            }
        }
    })
}

fn hyper_redirect_url_from_location(
    current: &ParsedHttpsUrl,
    headers: &hyper::HeaderMap,
) -> Option<String> {
    let loc = headers.get(hyper::header::LOCATION)?.to_str().ok()?.trim();
    if loc.is_empty() {
        return None;
    }
    if loc.starts_with("https://") {
        return Some(String::from(loc));
    }
    if loc.starts_with("http://") {
        return None;
    }
    if loc.starts_with('/') {
        if current.port == 443 {
            return Some(format!("https://{}{}", current.host, loc));
        }
        return Some(format!("https://{}:{}{}", current.host, current.port, loc));
    }
    None
}

async fn fetch_on_device_hyper(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    request_on_device_hyper(
        parsed,
        dev_idx,
        hyper::Method::GET,
        "*/*",
        None,
        &[],
        None,
        timeout_ms,
        max_bytes,
    )
    .await
}

async fn request_on_device_hyper(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    method: hyper::Method,
    accept: &str,
    content_type: Option<&str>,
    body_bytes: &[u8],
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let stream = connect_hyper_tls_stream(parsed, dev_idx, timeout_ms).await?;
    crate::log!("vhttps-hyper: handshake begin host={}\n", parsed.host);
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, HyperBytesBody>(HyperTokioIo::new(stream))
            .await
            .map_err(|_| FetchError::Tls)?;
    crate::log!("vhttps-hyper: handshake ok host={}\n", parsed.host);
    let connection = tokio::spawn(async move { connection.await });

    crate::log!("vhttps-hyper: sender ready begin host={}\n", parsed.host);
    sender.ready().await.map_err(|_| FetchError::BodyTimeout)?;
    crate::log!(
        "vhttps-hyper: request method={} host={} path={}\n",
        method.as_str(),
        parsed.host,
        parsed.path
    );
    let mut builder = hyper::Request::builder()
        .method(method)
        .uri(parsed.path.as_str())
        .header(hyper::header::HOST, parsed.host.as_str())
        .header(hyper::header::USER_AGENT, "TRUEOS hyper")
        .header(hyper::header::ACCEPT, accept)
        .header(hyper::header::ACCEPT_ENCODING, "identity")
        .header(hyper::header::CONNECTION, "close");
    if !body_bytes.is_empty() {
        builder = builder.header(hyper::header::CONTENT_LENGTH, body_bytes.len().to_string());
        if let Some(content_type) = content_type {
            builder = builder.header(hyper::header::CONTENT_TYPE, content_type);
        }
    }
    if let Some(token) = auth_token {
        builder = builder.header(hyper::header::AUTHORIZATION, format!("Bearer {}", token));
    }
    let request = builder
        .body(HyperBytesBody::new(body_bytes))
        .map_err(|_| FetchError::BadUrl)?;
    let response = tokio::time::timeout(
        core::time::Duration::from_millis(timeout_ms as u64),
        sender.send_request(request),
    )
    .await
    .map_err(|_| FetchError::BodyTimeout)?
    .map_err(|_| FetchError::BodyTimeout)?;

    let status = response.status().as_u16();
    if is_redirect_status(status) {
        if let Some(url) = hyper_redirect_url_from_location(parsed, response.headers()) {
            return Err(FetchError::Redirect { status, url });
        }
    }
    if status != 200 {
        return Err(FetchError::Http(status));
    }

    let mut body = response.into_body();
    let mut out = Vec::new();
    loop {
        let next = tokio::time::timeout(
            core::time::Duration::from_millis(timeout_ms as u64),
            core::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)),
        )
        .await
        .map_err(|_| FetchError::BodyTimeout)?;
        let Some(frame) = next else {
            break;
        };
        let frame = frame.map_err(|_| FetchError::BodyTimeout)?;
        if let Ok(data) = frame.into_data() {
            if out.len().saturating_add(data.len()) > max_bytes {
                return Err(FetchError::ResponseTooLarge);
            }
            out.extend_from_slice(&data);
        }
    }

    drop(sender);
    let _ = tokio::time::timeout(core::time::Duration::from_millis(250), connection).await;
    crate::log!("vhttps-hyper: body-complete host={} bytes={}\n", parsed.host, out.len());
    Ok(out)
}

pub async fn fetch_https_body_hyper_async(
    url: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    fetch_https_body_hyper_with_profile_async(url, NetProfile::default(), timeout_ms, max_bytes)
        .await
}

#[derive(Debug)]
pub enum SharedFetchError {
    Runtime,
    Fetch(FetchError),
}

pub async fn get_bytes_shared(
    url: String,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, SharedFetchError> {
    crate::t::run_on_shared_tokio({
        let url = url.clone();
        move || async move {
            fetch_https_body_hyper_async(url.as_str(), timeout_ms, max_bytes).await
        }
    })
    .await
    .map_err(|_| SharedFetchError::Runtime)?
    .map_err(SharedFetchError::Fetch)
}

pub async fn fetch_https_body_hyper_with_profile_async(
    url: &str,
    profile: NetProfile,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let dev_idx = fetch_device_index(profile)?;

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        match fetch_on_device_hyper(&parsed, dev_idx, timeout_ms, max_bytes).await {
            Ok(v) => return Ok(v),
            Err(FetchError::Redirect { status, url }) => {
                if hop >= MAX_REDIRECTS {
                    return Err(FetchError::Http(status));
                }
                current_url = url;
            }
            Err(e) => return Err(e),
        }
    }

    Err(FetchError::Http(0))
}

pub async fn post_https_json_hyper_async(
    url: &str,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    post_https_json_hyper_with_profile_async(
        url,
        NetProfile::default(),
        body_json,
        auth_token,
        timeout_ms,
        max_bytes,
    )
    .await
}

pub async fn post_https_json_hyper_with_profile_async(
    url: &str,
    profile: NetProfile,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let dev_idx = fetch_device_index(profile)?;

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        let res = request_on_device_hyper(
            &parsed,
            dev_idx,
            hyper::Method::POST,
            "application/json",
            Some("application/json"),
            body_json.as_bytes(),
            auth_token,
            timeout_ms,
            max_bytes,
        )
        .await;

        match res {
            Ok(v) => return Ok(v),
            Err(FetchError::Redirect { status, url }) => {
                if hop >= MAX_REDIRECTS {
                    return Err(FetchError::Http(status));
                }
                current_url = url;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(FetchError::Http(0))
}

pub async fn fetch_https_to_file_hyper_async(
    url: &str,
    path: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<(), i32> {
    fetch_https_to_file_hyper_with_profile_async(
        url,
        NetProfile::default(),
        path,
        timeout_ms,
        max_bytes,
    )
    .await
}

pub async fn fetch_https_to_file_hyper_with_profile_async(
    url: &str,
    profile: NetProfile,
    path: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<(), i32> {
    let t0 = Instant::now();
    let key = normalize_rel(path, false)?;
    let dev_idx = fetch_device_index(profile).map_err(fetch_error_to_code)?;

    crate::log!("vhttps-hyper-file: start key={} url={}\n", key, url);

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);
    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str())
            .ok_or(FetchError::BadUrl)
            .map_err(fetch_error_to_code)?;
        match fetch_on_device_hyper(&parsed, dev_idx, timeout_ms, max_bytes).await {
            Ok(body) => {
                if body.len() > max_bytes {
                    return Err(fetch_error_to_code(FetchError::ResponseTooLarge));
                }
                let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
                    crate::log!("vhttps-hyper-file: publish failed key={} reason=no-root\n", key);
                    return Err(FS_ERR_NOT_FOUND);
                };
                let disk_info = disk.info();
                crate::log!(
                    "vhttps-hyper-file: publish begin key={} disk={} kind={:?} writable={} blocks={} bs={}\n",
                    key,
                    disk_info.id.raw(),
                    disk_info.kind,
                    disk_info.writable,
                    disk_info.block_count,
                    disk_info.block_size
                );
                match crate::r::fs::trueosfs::file_in_async(disk, key.as_str(), body.as_slice())
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        crate::log!(
                            "vhttps-hyper-file: publish failed key={} reason=no-space\n",
                            key
                        );
                        return Err(FS_ERR_NO_SPACE);
                    }
                    Err(err) => {
                        crate::log!(
                            "vhttps-hyper-file: publish failed key={} err={:?}\n",
                            key,
                            err
                        );
                        return Err(block_error_to_code(err));
                    }
                }
                match crate::r::fs::trueosfs::file_info_async(disk, key.as_str()).await {
                    Ok(Some(info)) if info.data_len == body.len() as u64 => {
                        crate::log!(
                            "vhttps-hyper-file: publish verified key={} bytes={}\n",
                            key,
                            info.data_len
                        );
                    }
                    Ok(Some(info)) => {
                        crate::log!(
                            "vhttps-hyper-file: publish verify mismatch key={} expected={} actual={}\n",
                            key,
                            body.len(),
                            info.data_len
                        );
                        return Err(FS_ERR_IO);
                    }
                    Ok(None) => {
                        crate::log!("vhttps-hyper-file: publish verify missing key={}\n", key);
                        return Err(FS_ERR_NOT_FOUND);
                    }
                    Err(err) => {
                        crate::log!(
                            "vhttps-hyper-file: publish verify failed key={} err={:?}\n",
                            key,
                            err
                        );
                        return Err(block_error_to_code(err));
                    }
                }
                let total_ms = Instant::now().saturating_duration_since(t0).as_millis();
                crate::log!(
                    "vhttps-hyper-file: done key={} bytes={} ms_total={}\n",
                    key,
                    body.len(),
                    total_ms
                );
                return Ok(());
            }
            Err(FetchError::Redirect { status, url }) => {
                if hop >= MAX_REDIRECTS {
                    return Err(fetch_error_to_code(FetchError::Http(status)));
                }
                current_url = url;
            }
            Err(err) => {
                return Err(fetch_error_to_code(err));
            }
        }
    }

    Err(fetch_error_to_code(FetchError::Http(0)))
}

/// TRUEOS C ABI: start async HTTPS fetch to cache file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_start(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
) -> u32 {
    if url_ptr.is_null() || url_len == 0 || path_ptr.is_null() || path_len == 0 {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
    if vmx_guest_cabi_context() {
        return guest_fetch_file_start(url_bytes, path_bytes);
    }
    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };
    let Ok(path_s) = core::str::from_utf8(path_bytes) else {
        return 0;
    };

    // Fixed fetch limits for loader cache path.
    //
    // This powers the QJS URL-module cache (esm.sh / CDN imports). Some responses are
    // large and/or slow enough that a ~2.5s global deadline causes spurious
    // `NET_ERR_TIMEOUT_BODY` failures even when connectivity is fine.
    const TIMEOUT_MS: u32 = 45_000;
    const MAX_BYTES: usize = 8 * 1024 * 1024;

    cabi_net_fetch_start_host(url_s, path_s, TIMEOUT_MS, MAX_BYTES)
}

/// TRUEOS C ABI: start async HTTPS fetch to in-memory bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_bytes_start(
    url_ptr: *const u8,
    url_len: usize,
) -> u32 {
    if url_ptr.is_null() || url_len == 0 {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    if vmx_guest_cabi_context() {
        return guest_fetch_bytes_start(url_bytes);
    }
    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };

    const TIMEOUT_MS: u32 = 45_000;
    const MAX_BYTES: usize = 8 * 1024 * 1024;

    cabi_net_fetch_bytes_start_host(url_s, TIMEOUT_MS, MAX_BYTES)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_prewarm_url_start(
    url_ptr: *const u8,
    url_len: usize,
) -> i32 {
    if url_ptr.is_null() || url_len == 0 {
        return -1;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return -2;
    };
    if parse_https_url(url_s).is_none() {
        return -3;
    }
    if vmx_guest_cabi_context() {
        return 0;
    }

    spawn_cabi_net_prewarm_url(String::from(url_s));
    0
}

/// TRUEOS C ABI: start async HTTP(S) POST(JSON) to file.
///
/// `bearer_ptr/bearer_len` are optional (pass null/0 for no Authorization header).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_start(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
) -> u32 {
    const TIMEOUT_MS: u32 = 15_000;
    trueos_cabi_net_fetch_post_json_start_with_timeout(
        url_ptr, url_len, path_ptr, path_len, body_ptr, body_len, bearer_ptr, bearer_len,
        TIMEOUT_MS,
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_start_with_timeout(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
    timeout_ms: u32,
) -> u32 {
    const MAX_BYTES: usize = 4 * 1024 * 1024;

    if url_ptr.is_null()
        || url_len == 0
        || path_ptr.is_null()
        || path_len == 0
        || body_ptr.is_null()
        || body_len == 0
    {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
    let body_bytes = core::slice::from_raw_parts(body_ptr, body_len);

    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };
    let Ok(path_s) = core::str::from_utf8(path_bytes) else {
        return 0;
    };
    let Ok(body_s) = core::str::from_utf8(body_bytes) else {
        return 0;
    };

    let bearer = if bearer_ptr.is_null() || bearer_len == 0 {
        None
    } else {
        let bearer_bytes = core::slice::from_raw_parts(bearer_ptr, bearer_len);
        let Ok(v) = core::str::from_utf8(bearer_bytes) else {
            return 0;
        };
        Some(String::from(v))
    };

    let key = match normalize_rel(path_s, false) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    let url = String::from(url_s);
    let body_json = String::from(body_s);
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);

    spawn_cabi_net_fetch_post_json(
        op_id,
        key,
        url,
        body_json,
        bearer,
        timeout_ms.max(1),
        MAX_BYTES,
    );

    op_id
}

/// TRUEOS C ABI: start async HTTP(S) POST(JSON) to in-memory bytes.
///
/// `bearer_ptr/bearer_len` are optional (pass null/0 for no Authorization header).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_bytes_start(
    url_ptr: *const u8,
    url_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
) -> u32 {
    const TIMEOUT_MS: u32 = 15_000;
    trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
        url_ptr, url_len, body_ptr, body_len, bearer_ptr, bearer_len, TIMEOUT_MS,
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
    url_ptr: *const u8,
    url_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
    timeout_ms: u32,
) -> u32 {
    const MAX_BYTES: usize = 4 * 1024 * 1024;

    if url_ptr.is_null() || url_len == 0 || body_ptr.is_null() || body_len == 0 {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let body_bytes = core::slice::from_raw_parts(body_ptr, body_len);

    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };
    let Ok(body_s) = core::str::from_utf8(body_bytes) else {
        return 0;
    };

    let bearer = if bearer_ptr.is_null() || bearer_len == 0 {
        None
    } else {
        let bearer_bytes = core::slice::from_raw_parts(bearer_ptr, bearer_len);
        let Ok(v) = core::str::from_utf8(bearer_bytes) else {
            return 0;
        };
        Some(String::from(v))
    };

    let url = String::from(url_s);
    let body_json = String::from(body_s);
    let request_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(request_id, CabiNetFetchBytesResult::default());

    spawn_cabi_net_fetch_post_json_bytes(
        request_id,
        url,
        body_json,
        bearer,
        timeout_ms.max(1),
        MAX_BYTES,
    );

    request_id
}

/// TRUEOS C ABI: query async HTTPS fetch result.
///
/// Returns:
/// - `FS_ERR_NOT_FOUND` while operation is pending/unknown
/// - `0` on success
/// - negative error code on completion failure
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_result(op_id: u32) -> i32 {
    if vmx_guest_cabi_context() {
        return guest_fetch_file_result(op_id);
    }
    cabi_net_fetch_result_host(op_id)
}

/// TRUEOS C ABI: discard async HTTPS fetch state.
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32 {
    if vmx_guest_cabi_context() {
        return guest_fetch_file_discard(op_id);
    }
    cabi_net_fetch_discard_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_result_len(op_id: u32) -> isize {
    if vmx_guest_cabi_context() {
        return guest_fetch_bytes_result_len(op_id);
    }
    cabi_net_fetch_bytes_result_len_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_read(
    op_id: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if vmx_guest_cabi_context() {
        return guest_fetch_bytes_read(op_id, out_ptr, out_cap);
    }
    let mut map = CABI_NET_FETCH_BYTES_RESULTS.lock();
    let Some(entry) = map.get(&op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    let Some(rc) = entry.rc else {
        return FS_ERR_NOT_FOUND as isize;
    };
    if rc != 0 {
        map.remove(&op_id);
        return rc as isize;
    }
    let len = entry.body.len();
    if out_ptr.is_null() || out_cap == 0 {
        return len as isize;
    }
    if len > out_cap {
        return FS_ERR_NO_SPACE as isize;
    }
    let entry = map.remove(&op_id).expect("entry present");
    unsafe { core::ptr::copy_nonoverlapping(entry.body.as_ptr(), out_ptr, entry.body.len()) };
    len as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32 {
    if vmx_guest_cabi_context() {
        return guest_fetch_bytes_discard(op_id);
    }
    cabi_net_fetch_bytes_discard_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }

    if timeout_ms == 0 {
        let rc = trueos_cabi_net_fetch_bytes_result_len(op_id);
        return if rc == FS_ERR_NOT_FOUND as isize {
            FS_ERR_NOT_FOUND
        } else if rc < 0 {
            rc as i32
        } else {
            0
        };
    }

    let start = embassy_time::Instant::now();
    let timeout = EmbassyDuration::from_millis(timeout_ms);
    loop {
        let rc = trueos_cabi_net_fetch_bytes_result_len(op_id);
        if rc != FS_ERR_NOT_FOUND as isize {
            return if rc < 0 { rc as i32 } else { 0 };
        }

        let elapsed = embassy_time::Instant::now().saturating_duration_since(start);
        if elapsed >= timeout {
            return FS_ERR_TIMEOUT;
        }
        let remain = timeout - elapsed;
        let step = core::cmp::min(remain, EmbassyDuration::from_millis(100));
        wait_on_net_fetch_or_guest_sleep(step);
    }
}

/// TRUEOS C ABI: wait for a net-fetch operation to complete.
///
/// Returns:
/// - `FS_ERR_NOT_FOUND` while pending (only when timeout_ms == 0)
/// - `FS_ERR_TIMEOUT` when deadline expires
/// - `0` on success
/// - negative error code on completion failure
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }

    if timeout_ms == 0 {
        return trueos_cabi_net_fetch_result(op_id);
    }

    let start = embassy_time::Instant::now();
    let timeout = EmbassyDuration::from_millis(timeout_ms);
    loop {
        let rc = trueos_cabi_net_fetch_result(op_id);
        if rc != FS_ERR_NOT_FOUND {
            return rc;
        }

        let elapsed = embassy_time::Instant::now().saturating_duration_since(start);
        if elapsed >= timeout {
            return FS_ERR_TIMEOUT;
        }
        let remain = timeout - elapsed;
        let step = core::cmp::min(remain, EmbassyDuration::from_millis(100));
        wait_on_net_fetch_or_guest_sleep(step);
    }
}
