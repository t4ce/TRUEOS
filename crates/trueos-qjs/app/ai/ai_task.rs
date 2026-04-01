#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, Ordering};

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
}

const AI_IMPORT_FILENAME: &[u8] = b"<ai-task-import>\0";
const AI_IMPORT_SOURCE: &[u8] = br#"
globalThis.__trueosAiTaskDone = 0;
globalThis.__trueosAiTaskError = '';
globalThis.__trueosAiTaskPromise = Promise.resolve(
  globalThis.importModule('/qjs/ai/ai_shell_normal.mjs'),
)
  .then((entry) => {
    if (!entry || typeof entry.runNormalPrompt !== 'function') {
      throw new Error('ai_shell_normal.mjs does not export runNormalPrompt()');
    }
    return entry.runNormalPrompt(String(globalThis.__trueosAiTaskPrompt || ''));
  })
  .then(
    () => {
      globalThis.__trueosAiTaskDone = 1;
    },
    (error) => {
      const message = error && error.stack ? String(error.stack) : String(error || 'unknown ai task error');
      globalThis.__trueosAiTaskError = message;
      globalThis.__trueosAiTaskDone = -1;
    },
  );
"#;
const AI_DONE_PROP: &[u8] = b"__trueosAiTaskDone\0";
const AI_ERROR_PROP: &[u8] = b"__trueosAiTaskError\0";
const AI_PROMPT_PROP: &[u8] = b"__trueosAiTaskPrompt\0";
const AI_PRINT_FN_PROP: &[u8] = b"__trueosAiPrintLine\0";
const AI_PROMPT_TIMEOUT_MS: u64 = 90_000;
const AI_MODE_NOT_IMPLEMENTED: &str =
    "ai: mode not implemented yet; use normal mode for now";

static AI_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static AI_TASK_QUEUE: Mutex<VecDeque<AiInputEntry>> = Mutex::new(VecDeque::new());
static AI_TASK_SIGNAL: Signal<SpinRawMutex, ()> = Signal::new();

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

fn prompt_mode_supported(entry: &AiInputEntry) -> bool {
    !entry.web_search && !entry.file_search && !entry.new_conversation && !entry.computer_use
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

    print_targeted_multiline((*opaque).target_mask as u8, text.as_str());
    qjs::JS_NewFloat64(ctx, text.len() as f64)
}

unsafe fn install_ai_globals(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    let global = qjs::JS_GetGlobalObject(ctx);
    let print_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_ai_print_line),
        AI_PRINT_FN_PROP.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, AI_PRINT_FN_PROP.as_ptr() as *const c_char, print_fn);
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
    let mut opaque = AiTaskJsContextOpaque {
        target_mask: entry.shell_target_mask as u32,
    };
    qjs::JS_SetContextOpaque(ctx, (&mut opaque as *mut AiTaskJsContextOpaque).cast::<c_void>());
    install_ai_globals(ctx);

    let global = qjs::JS_GetGlobalObject(ctx);
    let _ = qjs::jsbind::set_str_prop(ctx, global, AI_PROMPT_PROP, entry.text.as_str());
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
        print_targeted_line(entry.shell_target_mask, "ai: failed to start js prompt runner");
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
            print_targeted_line(entry.shell_target_mask, "ai: runtime pump failed");
            break;
        }

        let global = qjs::JS_GetGlobalObject(ctx);
        let done = read_f64_prop(ctx, global, AI_DONE_PROP).unwrap_or(0.0) as i32;
        if done > 0 {
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
            print_targeted_multiline(entry.shell_target_mask, alloc::format!("ai: {}", err).as_str());
            let drained = qjs::vm::teardown_main_context(rt, ctx, 2_000).await;
            if !drained {
                qjs::trueos_shims::log_error("ai-task: teardown drain incomplete after js error\n");
            }
            drop(vm);
            return false;
        }
        qjs::js_free_value(ctx, global);

        if elapsed_ms >= AI_PROMPT_TIMEOUT_MS {
            print_targeted_line(entry.shell_target_mask, "ai: prompt timed out");
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

pub fn queue_ai_input(next: AiInputEntry) -> bool {
    AI_TASK_QUEUE.lock().push_back(next);
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
