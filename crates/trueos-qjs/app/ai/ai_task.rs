#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate as qjs;

#[derive(Clone)]
pub struct AiInputEntry {
    pub text: String,
    pub web_search: bool,
    pub file_search: bool,
    pub new_conversation: bool,
    pub computer_use: bool,
    pub shell_target_mask: u8,
    pub request_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnsureStartedResult {
    Ready,
    BrowserNotReady,
    SpawnFailed,
}

struct SpinRawMutex(Mutex<()>);

unsafe impl RawMutex for SpinRawMutex {
    const INIT: Self = Self(Mutex::new(()));

    fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.lock();
        f()
    }
}

struct AiTaskJsContextOpaque {
    target_mask: u32,
    request_id: u64,
    next_conversation_id: Option<String>,
}

#[derive(Clone, Copy)]
struct AiUsageTotals {
    input_tokens: u64,
    output_tokens: u64,
}

const AI_IMPORT_FILENAME: &[u8] = b"<ai-task-import>\0";
const AI_IMPORT_SOURCE: &[u8] = br#"
globalThis.__trueosAiTaskDone = 0;
globalThis.__trueosAiTaskError = '';
globalThis.__trueosAiTaskStage = 'loading-entry';
const __trueosAiTaskEntryModule =
  Number(globalThis.__trueosAiTaskComputerUse || 0) > 0
    ? '/qjs/ai/ai_pc_runner.mjs'
    : '/qjs/ai/ai_shell_normal.mjs';
if (typeof globalThis.importModule !== 'function') {
  globalThis.__trueosAiTaskError = 'importModule is not available';
  globalThis.__trueosAiTaskDone = -1;
  throw new Error('importModule is not available');
}
const __trueosAiImportTimeoutMs = Math.max(
  1,
  Number(globalThis.__trueosAiTaskImportTimeoutMs || 0) || 8000,
);
const __trueosAiImportTimeoutPromise = new Promise((_, reject) => {
  setTimeout(() => {
    reject(new Error(`ai module import timed out after ${__trueosAiImportTimeoutMs}ms`));
  }, __trueosAiImportTimeoutMs);
});
globalThis.__trueosAiTaskPromise = Promise.race([
  Promise.resolve(globalThis.importModule(__trueosAiTaskEntryModule)),
  __trueosAiImportTimeoutPromise,
])
  .then((entry) => {
        globalThis.__trueosAiTaskStage = 'starting-prompt';
        if (!entry || typeof entry.runShellPrompt !== 'function') {
            throw new Error(`${__trueosAiTaskEntryModule} does not export runShellPrompt()`);
    }
        globalThis.__trueosAiTaskStage = 'running-prompt';
        return entry.runShellPrompt({
            prompt: String(globalThis.__trueosAiTaskPrompt || ''),
            webSearch: Number(globalThis.__trueosAiTaskWebSearch || 0) > 0,
            fileSearch: Number(globalThis.__trueosAiTaskFileSearch || 0) > 0,
            conversationId: String(globalThis.__trueosAiTaskConversationId || ''),
            computerUse: Number(globalThis.__trueosAiTaskComputerUse || 0) > 0,
            targetMask: Number(globalThis.__trueosAiTaskTargetMask || 0) || 0,
        });
  })
  .then(
    () => {
      globalThis.__trueosAiTaskStage = 'done';
      globalThis.__trueosAiTaskDone = 1;
    },
    (error) => {
      const stage = String(globalThis.__trueosAiTaskStage || 'unknown');
      const message = error && error.stack ? String(error.stack) : String(error || 'unknown ai task error');
      globalThis.__trueosAiTaskError = `[stage=${stage}] ${message}`;
      globalThis.__trueosAiTaskStage = 'error';
      globalThis.__trueosAiTaskDone = -1;
    },
  );
"#;
const AI_DONE_PROP: &[u8] = b"__trueosAiTaskDone\0";
const AI_ERROR_PROP: &[u8] = b"__trueosAiTaskError\0";
const AI_STAGE_PROP: &[u8] = b"__trueosAiTaskStage\0";
const AI_PROMPT_PROP: &[u8] = b"__trueosAiTaskPrompt\0";
const AI_WEB_SEARCH_PROP: &[u8] = b"__trueosAiTaskWebSearch\0";
const AI_FILE_SEARCH_PROP: &[u8] = b"__trueosAiTaskFileSearch\0";
const AI_CONVERSATION_ID_PROP: &[u8] = b"__trueosAiTaskConversationId\0";
const AI_COMPUTER_USE_PROP: &[u8] = b"__trueosAiTaskComputerUse\0";
const AI_TARGET_MASK_PROP: &[u8] = b"__trueosAiTaskTargetMask\0";
const AI_IMPORT_TIMEOUT_PROP: &[u8] = b"__trueosAiTaskImportTimeoutMs\0";
const AI_PRINT_FN_PROP: &[u8] = b"__trueosAiPrintLine\0";
const AI_SET_CONVERSATION_FN_PROP: &[u8] = b"__trueosAiSetConversationId\0";
const AI_ADD_USAGE_TOTALS_FN_PROP: &[u8] = b"__trueosAiAddUsageTotals\0";
const AI_READ_PRIMARY_FS_TREE_FN_PROP: &[u8] = b"__trueosAiReadPrimaryFsTreeJsonAll\0";
const AI_PROMPT_TIMEOUT_MS: u64 = 90_000;
const AI_IMPORT_TIMEOUT_MS: u64 = 8_000;
const AI_MODE_NOT_IMPLEMENTED: &str = "ai: mode not implemented yet; use normal, web, file, or newchat";
const AI_PRIMARY_FS_TREE_MAX_ENTRIES: u32 = 96;

static AI_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static AI_TASK_QUEUE: Mutex<VecDeque<AiInputEntry>> = Mutex::new(VecDeque::new());
static AI_TASK_CONVERSATIONS: Mutex<Vec<(u8, String)>> = Mutex::new(Vec::new());
static AI_TASK_USAGE_TOTALS: Mutex<Vec<(u8, AiUsageTotals)>> = Mutex::new(Vec::new());
static AI_TASK_LATEST_REQUESTS: Mutex<Vec<(u8, u64)>> = Mutex::new(Vec::new());
static AI_TASK_SIGNAL: Signal<SpinRawMutex, ()> = Signal::new();
static AI_TASK_REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

fn print_targeted_line(target_mask: u8, text: &str) {
    let _ = qjs::trueos_shims::shell2_print_targeted_line(target_mask as u32, text.as_bytes());
}

fn print_targeted_multiline(target_mask: u8, text: &str) {
    let mut printed_any = false;
    for line in text.lines() {
        let trimmed = line.trim_end_matches('\r');
        print_targeted_line(target_mask, trimmed);
        printed_any = true;
    }
    if !printed_any {
        print_targeted_line(target_mask, text);
    }
}

fn current_request_id(target_mask: u8) -> u64 {
    AI_TASK_LATEST_REQUESTS
        .lock()
        .iter()
        .find_map(|(mask, request_id)| (*mask == target_mask).then_some(*request_id))
        .unwrap_or(0)
}

fn mark_latest_request(target_mask: u8, request_id: u64) {
    let mut state = AI_TASK_LATEST_REQUESTS.lock();
    if let Some((_, current)) = state.iter_mut().find(|(mask, _)| *mask == target_mask) {
        *current = request_id;
        return;
    }
    state.push((target_mask, request_id));
}

fn is_latest_request(target_mask: u8, request_id: u64) -> bool {
    current_request_id(target_mask) == request_id
}

fn print_targeted_line_if_current(target_mask: u8, request_id: u64, text: &str) {
    if is_latest_request(target_mask, request_id) {
        print_targeted_line(target_mask, text);
    }
}

fn print_targeted_multiline_if_current(target_mask: u8, request_id: u64, text: &str) {
    if is_latest_request(target_mask, request_id) {
        print_targeted_multiline(target_mask, text);
    }
}

fn prompt_mode_supported(_entry: &AiInputEntry) -> bool { true }

fn read_primary_fs_tree_json_all(max_entries: u32) -> Option<String> {
    let bytes = v::vfs::trueosfs_json_all(max_entries).ok()?;
    String::from_utf8(bytes).ok()
}

fn get_conversation_id(target_mask: u8) -> Option<String> {
    AI_TASK_CONVERSATIONS
        .lock()
        .iter()
        .find_map(|(mask, conversation_id)| (*mask == target_mask).then(|| conversation_id.clone()))
}

fn set_conversation_id(target_mask: u8, conversation_id: String) {
    let mut state = AI_TASK_CONVERSATIONS.lock();
    if let Some((_, current)) = state.iter_mut().find(|(mask, _)| *mask == target_mask) {
        *current = conversation_id;
        return;
    }
    state.push((target_mask, conversation_id));
}

fn clear_conversation_id(target_mask: u8) {
    let mut state = AI_TASK_CONVERSATIONS.lock();
    if let Some(index) = state.iter().position(|(mask, _)| *mask == target_mask) {
        state.swap_remove(index);
    }
}

pub fn forget_conversation(target_mask: u8) {
    clear_conversation_id(target_mask);
    clear_usage_totals(target_mask);
}

fn clear_usage_totals(target_mask: u8) {
    let mut state = AI_TASK_USAGE_TOTALS.lock();
    if let Some(index) = state.iter().position(|(mask, _)| *mask == target_mask) {
        state.swap_remove(index);
    }
}

fn add_usage_totals(target_mask: u8, input_tokens: u64, output_tokens: u64) -> AiUsageTotals {
    let mut state = AI_TASK_USAGE_TOTALS.lock();
    if let Some((_, totals)) = state.iter_mut().find(|(mask, _)| *mask == target_mask) {
        totals.input_tokens = totals.input_tokens.saturating_add(input_tokens);
        totals.output_tokens = totals.output_tokens.saturating_add(output_tokens);
        return *totals;
    }

    let totals = AiUsageTotals {
        input_tokens,
        output_tokens,
    };
    state.push((target_mask, totals));
    totals
}

unsafe extern "C" fn qjs_ai_print_line(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(text) = qjs::jsbind::to_string(ctx, args[0]) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };

    let opaque = qjs::JS_GetContextOpaque(ctx) as *mut AiTaskJsContextOpaque;
    if opaque.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    print_targeted_multiline_if_current(
        (*opaque).target_mask as u8,
        (*opaque).request_id,
        text.as_str(),
    );
    qjs::JS_NewFloat64(ctx, text.len() as f64)
}

unsafe extern "C" fn qjs_ai_set_conversation_id(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(conversation_id) = qjs::jsbind::to_string(ctx, args[0]) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };

    let opaque = qjs::JS_GetContextOpaque(ctx) as *mut AiTaskJsContextOpaque;
    if opaque.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    if !is_latest_request((*opaque).target_mask as u8, (*opaque).request_id) {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    (*opaque).next_conversation_id = (!conversation_id.trim().is_empty()).then_some(conversation_id);
    qjs::JS_NewFloat64(ctx, 1.0)
}

unsafe extern "C" fn qjs_ai_add_usage_totals(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 2 || argv.is_null() {
        return qjs::JSValue::undefined();
    }

    let opaque = qjs::JS_GetContextOpaque(ctx) as *mut AiTaskJsContextOpaque;
    if opaque.is_null() {
        return qjs::JSValue::undefined();
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut input_tokens_f64 = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut input_tokens_f64 as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    let mut output_tokens_f64 = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut output_tokens_f64 as *mut f64, args[1]) != 0 {
        return qjs::JSValue::undefined();
    }

    let input_tokens = if input_tokens_f64.is_sign_negative() {
        0
    } else {
        input_tokens_f64 as u64
    };
    let output_tokens = if output_tokens_f64.is_sign_negative() {
        0
    } else {
        output_tokens_f64 as u64
    };

    if !is_latest_request((*opaque).target_mask as u8, (*opaque).request_id) {
        return qjs::JSValue::undefined();
    }

    let totals = add_usage_totals((*opaque).target_mask as u8, input_tokens, output_tokens);
    let summary = alloc::format!(
        "sum_in={} sum_out={}",
        totals.input_tokens,
        totals.output_tokens
    );
    qjs::jsbind::new_string(ctx, summary.as_bytes())
}

unsafe extern "C" fn qjs_ai_read_primary_fs_tree_json_all(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let mut max_entries = AI_PRIMARY_FS_TREE_MAX_ENTRIES;
    if argc >= 1 && !argv.is_null() {
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut next = 0.0f64;
        if qjs::JS_ToFloat64(ctx, &mut next as *mut f64, args[0]) == 0 {
            let next = next as i32;
            if next > 0 {
                max_entries = next as u32;
            }
        }
    }

    let Some(json) = read_primary_fs_tree_json_all(max_entries) else {
        return qjs::JSValue::undefined();
    };

    qjs::jsbind::new_string(ctx, json.as_bytes())
}

unsafe fn install_ai_globals(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    let global = qjs::JS_GetGlobalObject(ctx);
    let _ = qjs::jsbind::install_fn(ctx, global, AI_PRINT_FN_PROP, 1, Some(qjs_ai_print_line));
    let _ = qjs::jsbind::install_fn(
        ctx,
        global,
        AI_SET_CONVERSATION_FN_PROP,
        1,
        Some(qjs_ai_set_conversation_id),
    );
    let _ = qjs::jsbind::install_fn(
        ctx,
        global,
        AI_ADD_USAGE_TOTALS_FN_PROP,
        2,
        Some(qjs_ai_add_usage_totals),
    );
    let _ = qjs::jsbind::install_fn(
        ctx,
        global,
        AI_READ_PRIMARY_FS_TREE_FN_PROP,
        1,
        Some(qjs_ai_read_primary_fs_tree_json_all),
    );
    qjs::js_free_value(ctx, global);
}

unsafe fn read_f64_prop(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
) -> Option<f64> {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    if value.is_exception() {
        return None;
    }

    let mut out = 0.0f64;
    let ok = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0;
    qjs::js_free_value(ctx, value);
    ok.then_some(out)
}

unsafe fn read_string_prop(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
) -> Option<String> {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    if value.is_exception() {
        return None;
    }
    let out = qjs::jsbind::to_string(ctx, value);
    qjs::js_free_value(ctx, value);
    out
}

async unsafe fn run_prompt_in_vm(entry: &AiInputEntry) -> bool {
    let Some(vm) = qjs::vm::QjsVm::new_node_with_profile(qjs::node::RuntimeProfile::Ai) else {
        print_targeted_line(entry.shell_target_mask, "ai: failed to create qjs vm");
        return false;
    };

    let rt = vm.rt_ptr();
    let ctx = vm.ctx_ptr();
    let existing_conversation_id = get_conversation_id(entry.shell_target_mask);
    let mut opaque = AiTaskJsContextOpaque {
        target_mask: entry.shell_target_mask as u32,
        request_id: entry.request_id,
        next_conversation_id: existing_conversation_id.clone(),
    };
    qjs::JS_SetContextOpaque(ctx, (&mut opaque as *mut AiTaskJsContextOpaque).cast::<c_void>());
    install_ai_globals(ctx);

    let global = qjs::JS_GetGlobalObject(ctx);
    let _ = qjs::jsbind::set_str_prop(ctx, global, AI_PROMPT_PROP, entry.text.as_str());
    let _ = qjs::jsbind::set_prop(
        ctx,
        global,
        AI_WEB_SEARCH_PROP,
        qjs::JS_NewFloat64(ctx, if entry.web_search { 1.0 } else { 0.0 }),
    );
    let _ = qjs::jsbind::set_prop(
        ctx,
        global,
        AI_FILE_SEARCH_PROP,
        qjs::JS_NewFloat64(ctx, if entry.file_search { 1.0 } else { 0.0 }),
    );
    let _ = qjs::jsbind::set_str_prop(
        ctx,
        global,
        AI_CONVERSATION_ID_PROP,
        existing_conversation_id.as_deref().unwrap_or(""),
    );
    let _ = qjs::jsbind::set_prop(
        ctx,
        global,
        AI_COMPUTER_USE_PROP,
        qjs::JS_NewFloat64(ctx, if entry.computer_use { 1.0 } else { 0.0 }),
    );
    let _ = qjs::jsbind::set_prop(
        ctx,
        global,
        AI_TARGET_MASK_PROP,
        qjs::JS_NewFloat64(ctx, entry.shell_target_mask as f64),
    );
    let _ = qjs::jsbind::set_prop(
        ctx,
        global,
        AI_IMPORT_TIMEOUT_PROP,
        qjs::JS_NewFloat64(ctx, AI_IMPORT_TIMEOUT_MS as f64),
    );
    qjs::js_free_value(ctx, global);

    let import = qjs::js_eval_bytes(
        ctx,
        AI_IMPORT_SOURCE,
        AI_IMPORT_FILENAME.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );
    if import.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "ai-task-import");
        qjs::js_free_value(ctx, import);
        print_targeted_line_if_current(
            entry.shell_target_mask,
            entry.request_id,
            "ai: failed to start js prompt runner",
        );
        let drained = qjs::vm::teardown_main_context(rt, ctx, 500).await;
        if !drained {
            qjs::trueos_shims::log_error("ai-task: teardown drain incomplete after import failure\n");
        }
        drop(vm);
        return false;
    }
    qjs::js_free_value(ctx, import);

    let mut elapsed_ms = 0u64;
    loop {
        if !qjs::vm::pump_runtime_once(rt, ctx, "ai-task") {
            print_targeted_line_if_current(
                entry.shell_target_mask,
                entry.request_id,
                "ai: runtime pump failed",
            );
            break;
        }

        let global = qjs::JS_GetGlobalObject(ctx);
        let done = read_f64_prop(ctx, global, AI_DONE_PROP).unwrap_or(0.0) as i32;
        if done > 0 {
            if is_latest_request(entry.shell_target_mask, entry.request_id) {
                if let Some(conversation_id) = opaque.next_conversation_id.take() {
                    set_conversation_id(entry.shell_target_mask, conversation_id);
                } else {
                    clear_conversation_id(entry.shell_target_mask);
                }
            } else {
                let _ = opaque.next_conversation_id.take();
            }
            qjs::js_free_value(ctx, global);
            let drained = qjs::vm::teardown_main_context(rt, ctx, 2_000).await;
            if !drained {
                qjs::trueos_shims::log_error("ai-task: teardown drain incomplete after success\n");
            }
            drop(vm);
            return true;
        }
        if done < 0 {
            let err = read_string_prop(ctx, global, AI_ERROR_PROP)
                .unwrap_or_else(|| String::from("unknown ai task error"));
            qjs::js_free_value(ctx, global);
            print_targeted_multiline_if_current(
                entry.shell_target_mask,
                entry.request_id,
                alloc::format!("ai: {}", err).as_str(),
            );
            let drained = qjs::vm::teardown_main_context(rt, ctx, 2_000).await;
            if !drained {
                qjs::trueos_shims::log_error("ai-task: teardown drain incomplete after js error\n");
            }
            drop(vm);
            return false;
        }
        qjs::js_free_value(ctx, global);

        if elapsed_ms >= AI_PROMPT_TIMEOUT_MS {
            let global = qjs::JS_GetGlobalObject(ctx);
            let stage = read_string_prop(ctx, global, AI_STAGE_PROP)
                .unwrap_or_else(|| String::from("unknown"));
            qjs::js_free_value(ctx, global);
            print_targeted_line_if_current(
                entry.shell_target_mask,
                entry.request_id,
                alloc::format!("ai: prompt timed out [stage={}]", stage).as_str(),
            );
            break;
        }

        Timer::after(EmbassyDuration::from_millis(1)).await;
        elapsed_ms = elapsed_ms.saturating_add(1);
    }

    let drained = qjs::vm::teardown_main_context(rt, ctx, 2_000).await;
    if !drained {
        qjs::trueos_shims::log_error("ai-task: teardown drain incomplete after timeout\n");
    }
    drop(vm);
    false
}

pub fn queue_ai_input(mut next: AiInputEntry) -> bool {
    if next.request_id == 0 {
        next.request_id = AI_TASK_REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
    }
    mark_latest_request(next.shell_target_mask, next.request_id);
    let mut queue = AI_TASK_QUEUE.lock();
    queue.retain(|entry| {
        entry.shell_target_mask != next.shell_target_mask || entry.request_id >= next.request_id
    });
    queue.push_back(next);
    AI_TASK_SIGNAL.signal(());
    true
}

pub fn ensure_started(spawner: &Spawner) -> EnsureStartedResult {
    if AI_TASK_STARTED.load(Ordering::Acquire) {
        return EnsureStartedResult::Ready;
    }

    if AI_TASK_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return EnsureStartedResult::Ready;
    }

    if spawner.spawn(run_once()).is_err() {
        AI_TASK_STARTED.store(false, Ordering::Release);
        qjs::trueos_shims::log_error("ai-task: spawn failed\n");
        return EnsureStartedResult::SpawnFailed;
    }

    qjs::trueos_shims::log_info("ai-task: started\n");
    EnsureStartedResult::Ready
}

#[embassy_executor::task]
pub async fn run_once() {
    qjs::trueos_shims::log_info("ai-task: worker loop online\n");

    loop {
        let next = loop {
            if let Some(entry) = AI_TASK_QUEUE.lock().pop_front() {
                break entry;
            }
            AI_TASK_SIGNAL.wait().await;
        };

        if next.new_conversation {
            clear_conversation_id(next.shell_target_mask);
            clear_usage_totals(next.shell_target_mask);
        }

        if next.text.trim().is_empty() {
            print_targeted_line(next.shell_target_mask, "ai: empty prompt");
            continue;
        }

        if !prompt_mode_supported(&next) {
            print_targeted_line(next.shell_target_mask, AI_MODE_NOT_IMPLEMENTED);
            continue;
        }

        let _ = unsafe { run_prompt_in_vm(&next).await };
    }
}
