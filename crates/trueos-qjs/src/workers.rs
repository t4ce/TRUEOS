#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char, c_int};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(2, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

static WORKER_SEQ: AtomicU32 = AtomicU32::new(1);

pub const CORE_KIND_UNKNOWN: u8 = 0;
pub const CORE_KIND_PERF: u8 = 1;
pub const CORE_KIND_EFF: u8 = 2;

static CORE_SPAWNERS: Mutex<BTreeMap<u32, embassy_executor::SendSpawner>> =
    Mutex::new(BTreeMap::new());
static CORE_KINDS: Mutex<BTreeMap<u32, u8>> = Mutex::new(BTreeMap::new());
static SPAWN_RR: AtomicU32 = AtomicU32::new(0);
static LOGGED_WORKER_API_USE: AtomicBool = AtomicBool::new(false);

// Slots 0 and 1 are reserved by the kernel as critical lanes (BSP + UI2/cursor/HID AP).
const FIRST_DISPOSABLE_SLOT: u32 = 2;
const WORKER_TASK_POOL: usize = 32;
const WORKER_TEARDOWN_WAIT_MS: u64 = 50;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SpawnPolicy {
    AnyNonCritical,
}

static WORKERS: Mutex<BTreeMap<u32, WorkerState>> = Mutex::new(BTreeMap::new());

// Context -> worker_id mapping (only populated for worker contexts).
static CTX_WORKER_ID: Mutex<BTreeMap<usize, u32>> = Mutex::new(BTreeMap::new());

// (ctx, worker_id) -> message callbacks
struct JsCallback {
    val: qjs::JSValue,
}

// QuickJS values are not thread-safe, but we only ever access callbacks from the owning VM/task.
// This matches the same "single owning task" assumption used elsewhere in TRUEOS QJS glue.
unsafe impl Send for JsCallback {}

static MAIN_ON_MESSAGE: Mutex<BTreeMap<(usize, u32), JsCallback>> = Mutex::new(BTreeMap::new());
static WORKER_ON_MESSAGE: Mutex<BTreeMap<(usize, u32), JsCallback>> = Mutex::new(BTreeMap::new());
// Main-context ownership of workers created from that context.
static MAIN_CTX_WORKERS: Mutex<BTreeMap<usize, Vec<u32>>> = Mutex::new(BTreeMap::new());

struct WorkerState {
    startup: Option<Vec<u8>>,
    to_worker: VecDeque<Vec<u8>>,
    to_parent: VecDeque<Vec<u8>>,
    terminated: AtomicBool,
    exited: AtomicBool,
    warned_no_worker_on_message: AtomicBool,
}

impl WorkerState {
    fn new(startup: Vec<u8>) -> Self {
        Self {
            startup: Some(startup),
            to_worker: VecDeque::new(),
            to_parent: VecDeque::new(),
            terminated: AtomicBool::new(false),
            exited: AtomicBool::new(false),
            warned_no_worker_on_message: AtomicBool::new(false),
        }
    }
}

pub fn ensure_service_started(spawner: &Spawner) -> bool {
    // Back-compat: treat this as "register BSP as unknown".
    register_core_spawner(0, CORE_KIND_UNKNOWN, *spawner);
    true
}

/// Register a core's SendSpawner along with a best-effort core-kind hint.
///
/// `core_kind` is typically derived from Intel hybrid CPUID leaf 0x1A:
/// - `CORE_KIND_PERF`: performance core
/// - `CORE_KIND_EFF`: efficient core
/// - `CORE_KIND_UNKNOWN`: fallback
pub fn register_core_spawner(cpu_slot: u32, core_kind: u8, spawner: Spawner) {
    CORE_SPAWNERS.lock().insert(cpu_slot, spawner.make_send());
    CORE_KINDS.lock().insert(cpu_slot, core_kind);
}

#[inline]
fn core_kind_name(kind: u8) -> &'static str {
    match kind {
        CORE_KIND_PERF => "perf",
        CORE_KIND_EFF => "eff",
        _ => "unknown",
    }
}

#[inline]
fn pick_from_pool(
    pool: &[(u32, u8, embassy_executor::SendSpawner)],
) -> Option<(u32, u8, embassy_executor::SendSpawner)> {
    if pool.is_empty() {
        return None;
    }
    let idx = SPAWN_RR.fetch_add(1, Ordering::Relaxed) as usize;
    Some(pool[idx % pool.len()].clone())
}

fn pick_spawner_with_policy(
    policy: SpawnPolicy,
) -> Option<(u32, u8, embassy_executor::SendSpawner)> {
    // Slots 0 and 1 remain reserved for critical kernel work and are excluded
    // from QJS scheduling. Only disposable worker lanes are eligible.
    let map = CORE_SPAWNERS.lock();
    if map.is_empty() {
        return None;
    }

    let kinds = CORE_KINDS.lock();

    let mut eligible: Vec<(u32, u8, embassy_executor::SendSpawner)> = Vec::new();
    for (slot, sp) in map.iter() {
        let kind = kinds.get(slot).copied().unwrap_or(CORE_KIND_UNKNOWN);
        if *slot >= FIRST_DISPOSABLE_SLOT {
            eligible.push((*slot, kind, sp.clone()));
        }
    }

    match policy {
        SpawnPolicy::AnyNonCritical => pick_from_pool(&eligible),
    }
}

pub fn pick_background_spawner() -> Option<embassy_executor::SendSpawner> {
    pick_spawner_with_policy(SpawnPolicy::AnyNonCritical).map(|(_, _, spawner)| spawner)
}

pub fn spawner_for_slot(cpu_slot: u32) -> Option<embassy_executor::SendSpawner> {
    CORE_SPAWNERS.lock().get(&cpu_slot).cloned()
}

pub fn background_worker_slots() -> Vec<u32> {
    let map = CORE_SPAWNERS.lock();
    let mut out: Vec<u32> = map
        .keys()
        .copied()
        .filter(|slot| *slot >= FIRST_DISPOSABLE_SLOT)
        .collect();
    out.sort_unstable();
    out
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CtxRole {
    Main,
    Worker(u32),
}

pub fn ctx_role(ctx: *mut qjs::JSContext) -> CtxRole {
    if ctx.is_null() {
        return CtxRole::Main;
    }
    let key = ctx as usize;
    if let Some(id) = CTX_WORKER_ID.lock().get(&key).copied() {
        return CtxRole::Worker(id);
    }
    CtxRole::Main
}

fn worker_state_mut<F, R>(worker_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&mut WorkerState) -> R,
{
    let mut map = WORKERS.lock();
    let st = map.get_mut(&worker_id)?;
    Some(f(st))
}

pub fn spawn_eval(code_utf8: &[u8]) -> Result<u32, i32> {
    spawn_eval_on_slot_inner(None, code_utf8)
}

pub fn spawn_eval_on_slot(cpu_slot: u32, code_utf8: &[u8]) -> Result<u32, i32> {
    spawn_eval_on_slot_inner(Some(cpu_slot), code_utf8)
}

fn spawn_eval_on_slot_inner(cpu_slot: Option<u32>, code_utf8: &[u8]) -> Result<u32, i32> {
    if code_utf8.is_empty() {
        return Err(-2);
    }

    let worker_id = WORKER_SEQ.fetch_add(1, Ordering::Relaxed);
    WORKERS
        .lock()
        .insert(worker_id, WorkerState::new(code_utf8.to_vec()));

    if !LOGGED_WORKER_API_USE.swap(true, Ordering::AcqRel) {
        log_str("qjs-worker: Worker API path used; worker spawn requested\n");
    }

    let (slot, kind, spawner) = if let Some(slot) = cpu_slot {
        if slot < FIRST_DISPOSABLE_SLOT {
            return Err(-2);
        }
        let spawner = CORE_SPAWNERS.lock().get(&slot).cloned().ok_or(-2)?;
        let kind = CORE_KINDS
            .lock()
            .get(&slot)
            .copied()
            .unwrap_or(CORE_KIND_UNKNOWN);
        (slot, kind, spawner)
    } else {
        pick_spawner_with_policy(SpawnPolicy::AnyNonCritical).ok_or(-2)?
    };
    log_str(&format!(
        "qjs-worker: worker#{} created; scheduled slot={} kind={}\n",
        worker_id,
        slot,
        core_kind_name(kind)
    ));
    spawner
        .spawn(worker_task(worker_id, slot, kind))
        .map_err(|_| -2)?;
    Ok(worker_id)
}

pub fn terminate(worker_id: u32) {
    let _ = worker_state_mut(worker_id, |st| {
        st.terminated.store(true, Ordering::Release);
    });
}

pub fn terminate_all_for_context(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let key = ctx as usize;
    let ids = MAIN_CTX_WORKERS.lock().remove(&key).unwrap_or_default();
    for worker_id in ids {
        terminate(worker_id);
    }
}

pub fn post_to_worker(worker_id: u32, msg: &[u8]) -> Result<(), i32> {
    worker_state_mut(worker_id, |st| {
        st.to_worker.push_back(msg.to_vec());
    })
    .ok_or(-2)?;
    Ok(())
}

pub fn post_to_parent(worker_id: u32, msg: &[u8]) -> Result<(), i32> {
    worker_state_mut(worker_id, |st| {
        st.to_parent.push_back(msg.to_vec());
    })
    .ok_or(-2)?;
    Ok(())
}

pub fn take_parent_message(worker_id: u32) -> Option<Vec<u8>> {
    worker_state_mut(worker_id, |st| st.to_parent.pop_front()).flatten()
}

pub fn worker_exited(worker_id: u32) -> bool {
    let map = WORKERS.lock();
    map.get(&worker_id)
        .is_some_and(|st| st.exited.load(Ordering::Acquire))
}

pub fn has_pending_for_ctx(ctx: *mut qjs::JSContext) -> bool {
    match ctx_role(ctx) {
        CtxRole::Main => {
            let map = WORKERS.lock();
            map.values().any(|st| {
                let exited = st.exited.load(Ordering::Acquire);
                !st.to_parent.is_empty() || (!exited && !st.to_worker.is_empty())
            })
        }
        CtxRole::Worker(id) => {
            let map = WORKERS.lock();
            map.get(&id)
                .is_some_and(|st| !st.exited.load(Ordering::Acquire) && !st.to_worker.is_empty())
        }
    }
}

unsafe fn js_string(ctx: *mut qjs::JSContext, bytes: &[u8]) -> qjs::JSValue {
    qjs::JS_NewStringLen(ctx, bytes.as_ptr() as *const c_char, bytes.len())
}

unsafe fn call_on_message(ctx: *mut qjs::JSContext, cb: qjs::JSValue, msg_bytes: &[u8]) {
    let arg = unsafe { js_string(ctx, msg_bytes) };
    let _ = unsafe {
        qjs::JS_Call(
            ctx,
            cb,
            qjs::JSValue::undefined(),
            1,
            &arg as *const qjs::JSValue,
        )
    };
    unsafe { qjs::js_free_value(ctx, arg) };
}

pub unsafe fn pump(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }
    let mut progress = false;

    match ctx_role(ctx) {
        CtxRole::Main => {
            // Drain all outbound worker->parent messages and deliver to callbacks registered
            // in the *current* main context.
            let key_ctx = ctx as usize;

            // Snapshot which workers have pending messages first to avoid holding the WORKERS lock
            // while calling into JS.
            let pending_ids: Vec<u32> = {
                let map = WORKERS.lock();
                map.iter()
                    .filter(|(_, st)| !st.to_parent.is_empty())
                    .map(|(id, _)| *id)
                    .collect()
            };

            for worker_id in pending_ids {
                loop {
                    let msg = worker_state_mut(worker_id, |st| st.to_parent.pop_front()).flatten();
                    let Some(msg) = msg else { break };
                    progress = true;

                    let cb = MAIN_ON_MESSAGE
                        .lock()
                        .get(&(key_ctx, worker_id))
                        .map(|c| c.val);
                    if let Some(cb) = cb {
                        unsafe { call_on_message(ctx, cb, msg.as_slice()) };
                    }
                }
            }
        }
        CtxRole::Worker(worker_id) => {
            let key_ctx = ctx as usize;
            loop {
                let msg = worker_state_mut(worker_id, |st| st.to_worker.pop_front()).flatten();
                let Some(msg) = msg else { break };
                progress = true;

                let cb = WORKER_ON_MESSAGE
                    .lock()
                    .get(&(key_ctx, worker_id))
                    .map(|c| c.val);
                if let Some(cb) = cb {
                    unsafe { call_on_message(ctx, cb, msg.as_slice()) };
                } else {
                    let should_warn = worker_state_mut(worker_id, |st| {
                        !st.warned_no_worker_on_message.swap(true, Ordering::AcqRel)
                    })
                    .unwrap_or(false);
                    if should_warn {
                        let _ = post_to_parent(
                            worker_id,
                            b"{\"ok\":0,\"dbg\":\"worker-no-onmessage-handler\"}",
                        );
                    }
                }
            }
        }
    }

    progress
}

pub unsafe fn set_on_message(ctx: *mut qjs::JSContext, worker_id: u32, cb: qjs::JSValue) {
    if ctx.is_null() {
        return;
    }
    let key_ctx = ctx as usize;
    let map = match ctx_role(ctx) {
        CtxRole::Main => &MAIN_ON_MESSAGE,
        CtxRole::Worker(_) => &WORKER_ON_MESSAGE,
    };

    let mut m = map.lock();
    if let Some(prev) = m.remove(&(key_ctx, worker_id)) {
        unsafe { qjs::js_free_value(ctx, prev.val) };
    }
    m.insert(
        (key_ctx, worker_id),
        JsCallback {
            val: unsafe { qjs::js_dup_value(ctx, cb) },
        },
    );
}

pub unsafe fn drain_all_for_context(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let key_ctx = ctx as usize;

    let mut main = MAIN_ON_MESSAGE.lock();
    let main_keys: Vec<(usize, u32)> = main
        .keys()
        .copied()
        .filter(|(c, _)| *c == key_ctx)
        .collect();
    for k in main_keys {
        if let Some(v) = main.remove(&k) {
            qjs::js_free_value(ctx, v.val);
        }
    }

    let mut wk = WORKER_ON_MESSAGE.lock();
    let wk_keys: Vec<(usize, u32)> = wk.keys().copied().filter(|(c, _)| *c == key_ctx).collect();
    for k in wk_keys {
        if let Some(v) = wk.remove(&k) {
            qjs::js_free_value(ctx, v.val);
        }
    }
}

fn take_startup(worker_id: u32) -> Option<Vec<u8>> {
    worker_state_mut(worker_id, |st| st.startup.take()).flatten()
}

fn is_terminated(worker_id: u32) -> bool {
    let map = WORKERS.lock();
    map.get(&worker_id)
        .is_some_and(|st| st.terminated.load(Ordering::Acquire))
}

fn mark_exited(worker_id: u32) {
    let _ = worker_state_mut(worker_id, |st| {
        st.exited.store(true, Ordering::Release);
        // Drop undeliverable parent->worker messages so dead workers don't keep
        // the main pump in a perpetual "workers pending" state.
        st.to_worker.clear();
    });
}

#[embassy_executor::task(pool_size = WORKER_TASK_POOL)]
async fn worker_task(worker_id: u32, scheduled_slot: u32, scheduled_kind: u8) {
    log_str(&format!(
        "qjs-worker: worker#{} executor start slot={} kind={}\n",
        worker_id,
        scheduled_slot,
        core_kind_name(scheduled_kind)
    ));
    let _ = post_to_parent(worker_id, b"{\"ok\":1,\"dbg\":\"worker-rust-task-start\"}");

    // Each worker owns its own QuickJS VM.
    let Some(vm) = (unsafe { qjs::vm::QjsVm::new_node() }) else {
        log_str("qjs-worker: failed to create VM\n");
        let _ = post_to_parent(worker_id, b"{\"ok\":0,\"dbg\":\"worker-vm-create-failed\"}");
        mark_exited(worker_id);
        return;
    };
    log_str(&format!(
        "qjs-worker: worker#{} vm-created slot={}\n",
        worker_id, scheduled_slot
    ));
    let rt = vm.rt_ptr();
    let ctx = vm.ctx_ptr();

    CTX_WORKER_ID.lock().insert(ctx as usize, worker_id);

    // Install per-context Node-ish globals.
    unsafe { qjs::node::install_globals(ctx) };
    log_str(&format!(
        "qjs-worker: worker#{} globals-installed slot={}\n",
        worker_id, scheduled_slot
    ));
    let _ = post_to_parent(worker_id, b"{\"ok\":1,\"dbg\":\"worker-globals-installed\"}");

    // Evaluate the startup code as a module (MVP: eval string only).
    let startup = take_startup(worker_id).unwrap_or_else(|| b"".to_vec());
    log_str(&format!(
        "qjs-worker: worker#{} startup-bytes={} slot={}\n",
        worker_id,
        startup.len(),
        scheduled_slot
    ));
    let _ = post_to_parent(worker_id, b"{\"ok\":1,\"dbg\":\"worker-startup-begin\"}");
    if !startup.is_empty() {
        let filename = b"<worker>\0";
        log_str(&format!(
            "qjs-worker: worker#{} startup-eval-begin slot={}\n",
            worker_id, scheduled_slot
        ));
        let v = unsafe {
            qjs::js_eval_bytes(
                ctx,
                &startup,
                filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_MODULE,
            )
        };
        if v.is_exception() {
            log_str("qjs-worker: startup eval exception\n");
            unsafe { qjs::qjs_diag::dump_last_exception(ctx, "worker-startup-eval") };
            let _ = post_to_parent(
                worker_id,
                b"{\"ok\":0,\"dbg\":\"worker-startup-eval-exception\"}",
            );
            unsafe { qjs::js_free_value(ctx, v) };
            let drained =
                unsafe { qjs::vm::teardown_worker_context(rt, ctx, WORKER_TEARDOWN_WAIT_MS).await };
            if !drained {
                log_str("qjs-worker: teardown drain incomplete\n");
            }
            CTX_WORKER_ID.lock().remove(&(ctx as usize));
            drop(vm);
            mark_exited(worker_id);
            return;
        } else {
            log_str(&format!(
                "qjs-worker: worker#{} startup-eval-ok slot={}\n",
                worker_id, scheduled_slot
            ));
            let _ = post_to_parent(worker_id, b"{\"ok\":1,\"dbg\":\"worker-startup-eval-ok\"}");
        }
        unsafe { qjs::js_free_value(ctx, v) };
    }

    // Worker loop: pump message inbox + async completions + microtasks.
    loop {
        if is_terminated(worker_id) {
            break;
        }

        let mut progress = false;
        progress |= unsafe { pump(ctx) };
        progress |= unsafe { qjs::async_ops::pump(ctx) };
        progress |= unsafe { qjs::timers::pump(ctx) };

        // Drain microtasks.
        loop {
            let mut job_ctx: *mut qjs::JSContext = core::ptr::null_mut();
            let rc =
                unsafe { qjs::JS_ExecutePendingJob(rt, &mut job_ctx as *mut *mut qjs::JSContext) };
            if rc > 0 {
                progress = true;
                continue;
            }
            if rc < 0 {
                let ectx = if !job_ctx.is_null() { job_ctx } else { ctx };
                log_str("qjs-worker: pending-job exception\n");
                unsafe { qjs::qjs_diag::dump_last_exception(ectx, "worker-pending-job") };
            }
            break;
        }

        let pending = unsafe { qjs::JS_IsJobPending(rt) > 0 }
            || unsafe { qjs::async_ops::has_pending(ctx) }
            || qjs::timers::has_pending(ctx)
            || qjs::workers::has_pending_for_ctx(ctx);
        if !progress && !pending {
            break;
        }

        if !progress {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }

    let drained = unsafe { qjs::vm::teardown_worker_context(rt, ctx, WORKER_TEARDOWN_WAIT_MS).await };
    if !drained {
        log_str("qjs-worker: teardown drain incomplete\n");
    }
    CTX_WORKER_ID.lock().remove(&(ctx as usize));
    drop(vm);
    mark_exited(worker_id);
}

// --- JS bindings for node:worker_threads ---

const WORKER_ID_PROP: &[u8] = b"__wid\0";

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

#[inline]
fn js_bool(v: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion {
            int32: if v { 1 } else { 0 },
        },
        tag: qjs::JS_TAG_BOOL,
    }
}

#[inline]
fn js_null() -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: 0 },
        tag: qjs::JS_TAG_NULL,
    }
}

unsafe fn arg_to_bytes(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<Vec<u8>> {
    let mut len: usize = 0;
    let cstr = unsafe { qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, v, 0) };
    if cstr.is_null() {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(cstr as *const u8, len) }.to_vec();
    unsafe { qjs::JS_FreeCString(ctx, cstr) };
    Some(bytes)
}

unsafe fn worker_id_from_this(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
) -> Option<u32> {
    let v =
        unsafe { qjs::JS_GetPropertyStr(ctx, this_val, WORKER_ID_PROP.as_ptr() as *const c_char) };
    if v.is_exception() {
        return None;
    }
    let id = if v.tag == qjs::JS_TAG_INT {
        (unsafe { v.u.int32 }) as i64
    } else {
        -1
    };
    unsafe { qjs::js_free_value(ctx, v) };
    if id <= 0 { None } else { Some(id as u32) }
}

#[inline]
unsafe fn event_name_is_message(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> bool {
    let Some(name) = (unsafe { arg_to_bytes(ctx, v) }) else {
        return false;
    };
    name.as_slice() == b"message"
}

/// Worker constructor: `new Worker(<code_string>)` (MVP: eval string only).
pub unsafe extern "C" fn js_worker_ctor(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::exception();
    }
    if argv.is_null() || argc < 1 {
        return qjs::JS_NewObject(ctx);
    }

    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let Some(code) = (unsafe { arg_to_bytes(ctx, args[0]) }) else {
        return qjs::JSValue::exception();
    };

    let worker_id = match spawn_eval(code.as_slice()) {
        Ok(id) => id,
        Err(_) => return qjs::JSValue::exception(),
    };
    MAIN_CTX_WORKERS
        .lock()
        .entry(ctx as usize)
        .or_default()
        .push(worker_id);

    // Constructor mode (`new Worker(...)`) passes a pre-created `this` with Worker.prototype.
    // Keep that object so class inheritance/prototype checks work for browser-style code.
    let obj = if this_val.tag == qjs::JS_TAG_OBJECT {
        qjs::js_dup_value(ctx, this_val)
    } else {
        qjs::JS_NewObject(ctx)
    };
    if obj.is_exception() {
        return obj;
    }

    // __wid (internal)
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        WORKER_ID_PROP.as_ptr() as *const c_char,
        js_int32(worker_id as i32),
    );

    // threadId
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"threadId\0".as_ptr() as *const c_char,
        js_int32(worker_id as i32),
    );

    // postMessage(message)
    let pm = qjs::JS_NewCFunction2(
        ctx,
        Some(js_worker_post_message),
        b"postMessage\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"postMessage\0".as_ptr() as *const c_char, pm);

    // terminate()
    let term = qjs::JS_NewCFunction2(
        ctx,
        Some(js_worker_terminate),
        b"terminate\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"terminate\0".as_ptr() as *const c_char, term);

    // onmessage(cb)
    let onm = qjs::JS_NewCFunction2(
        ctx,
        Some(js_worker_on_message),
        b"onMessage\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"onMessage\0".as_ptr() as *const c_char, onm);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"onmessage\0".as_ptr() as *const c_char,
        qjs::JSValue::undefined(),
    );

    obj
}

pub unsafe extern "C" fn js_worker_add_event_listener(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    let Some(worker_id) = (unsafe { worker_id_from_this(ctx, this_val) }) else {
        return qjs::JSValue::undefined();
    };
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    if unsafe { event_name_is_message(ctx, args[0]) } {
        unsafe { set_on_message(ctx, worker_id, args[1]) };
    }
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_worker_remove_event_listener(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // No-op MVP. We keep this for browser API shape compatibility.
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_worker_post_message(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    let Some(worker_id) = (unsafe { worker_id_from_this(ctx, this_val) }) else {
        return qjs::JSValue::undefined();
    };
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let Some(msg) = (unsafe { arg_to_bytes(ctx, args[0]) }) else {
        return qjs::JSValue::exception();
    };
    let _ = post_to_worker(worker_id, msg.as_slice());
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_worker_terminate(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    if let Some(worker_id) = unsafe { worker_id_from_this(ctx, this_val) } {
        terminate(worker_id);
    }
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_worker_on_message(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    let Some(worker_id) = (unsafe { worker_id_from_this(ctx, this_val) }) else {
        return qjs::JSValue::undefined();
    };
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    unsafe { set_on_message(ctx, worker_id, args[0]) };
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_parent_port_post_message(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let Some(msg) = (unsafe { arg_to_bytes(ctx, args[0]) }) else {
        return qjs::JSValue::exception();
    };

    match ctx_role(ctx) {
        CtxRole::Worker(id) => {
            let _ = post_to_parent(id, msg.as_slice());
        }
        CtxRole::Main => {
            // No parentPort on main.
        }
    }
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_parent_port_on_message(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let cb = args[0];
    match ctx_role(ctx) {
        CtxRole::Worker(id) => unsafe { set_on_message(ctx, id, cb) },
        CtxRole::Main => {
            // No-op.
        }
    }
    qjs::JSValue::undefined()
}

pub unsafe fn install_worker_threads_exports(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
    if ctx.is_null() || m.is_null() {
        return -1;
    }

    match ctx_role(ctx) {
        CtxRole::Main => {
            let worker_fn = qjs::JS_NewCFunction2(
                ctx,
                Some(js_worker_ctor),
                b"Worker\0".as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_CONSTRUCTOR,
                0,
            );
            let _ =
                qjs::JS_SetModuleExport(ctx, m, b"Worker\0".as_ptr() as *const c_char, worker_fn);
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"isMainThread\0".as_ptr() as *const c_char,
                js_bool(true),
            );
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"parentPort\0".as_ptr() as *const c_char,
                js_null(),
            );
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"threadId\0".as_ptr() as *const c_char,
                js_int32(0),
            );
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"workerData\0".as_ptr() as *const c_char,
                qjs::JSValue::undefined(),
            );
        }
        CtxRole::Worker(id) => {
            let port = qjs::JS_NewObject(ctx);
            let pm = qjs::JS_NewCFunction2(
                ctx,
                Some(js_parent_port_post_message),
                b"postMessage\0".as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let onm = qjs::JS_NewCFunction2(
                ctx,
                Some(js_parent_port_on_message),
                b"onMessage\0".as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ =
                qjs::JS_SetPropertyStr(ctx, port, b"postMessage\0".as_ptr() as *const c_char, pm);
            let _ =
                qjs::JS_SetPropertyStr(ctx, port, b"onMessage\0".as_ptr() as *const c_char, onm);

            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"isMainThread\0".as_ptr() as *const c_char,
                js_bool(false),
            );
            let _ =
                qjs::JS_SetModuleExport(ctx, m, b"parentPort\0".as_ptr() as *const c_char, port);
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"threadId\0".as_ptr() as *const c_char,
                js_int32(id as i32),
            );
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"workerData\0".as_ptr() as *const c_char,
                qjs::JSValue::undefined(),
            );
        }
    }

    0
}

#[inline]
pub(crate) unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }

    let name = unsafe { CStr::from_ptr(module_name).to_bytes() };
    if name != b"worker_threads" && name != b"node:worker_threads" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn worker_threads_module_init(
        ctx: *mut qjs::JSContext,
        m: *mut qjs::JSModuleDef,
    ) -> c_int {
        unsafe { install_worker_threads_exports(ctx, m) }
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(worker_threads_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    macro_rules! add_export {
        ($name:literal) => {{
            let k = concat!($name, "\0");
            let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char) };
        }};
    }

    add_export!("Worker");
    add_export!("isMainThread");
    add_export!("parentPort");
    add_export!("threadId");
    add_export!("workerData");

    m
}
