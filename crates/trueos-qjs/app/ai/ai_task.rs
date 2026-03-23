#![cfg(feature = "trueos")]

use alloc::collections::VecDeque;
use alloc::string::String;
use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate as qjs;

static AI_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static AI_TASK_ACCEPTING_INPUT: AtomicBool = AtomicBool::new(false);
static PENDING_AI_INPUT: Mutex<VecDeque<AiInputEntry>> = Mutex::new(VecDeque::new());

#[derive(Clone)]
pub struct AiInputEntry {
    pub text: String,
    pub web_search: bool,
    pub file_search: bool,
    pub new_conversation: bool,
    pub computer_use: bool,
    pub shell_target_mask: u8,
}

pub fn queue_ai_input(next: AiInputEntry) -> bool {
    if !AI_TASK_ACCEPTING_INPUT.load(Ordering::SeqCst) {
        return false;
    }
    PENDING_AI_INPUT.lock().push_back(next);
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnsureStartedResult {
    Ready,
    BrowserNotReady,
    SpawnFailed,
}

pub fn ensure_started(spawner: &Spawner) -> EnsureStartedResult {
    if !qjs::truesurfer_qjs_vm_task::default_browser_started() {
        return EnsureStartedResult::BrowserNotReady;
    }

    if AI_TASK_ACCEPTING_INPUT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return EnsureStartedResult::Ready;
    }

    if spawner.spawn(run_once()).is_err() {
        AI_TASK_ACCEPTING_INPUT.store(false, Ordering::Release);
        return EnsureStartedResult::SpawnFailed;
    }
    EnsureStartedResult::Ready
}

#[inline]
unsafe fn read_js_string_arg(ctx: *mut qjs::JSContext, value: qjs::JSValueConst) -> Option<String> {
    let mut len = 0usize;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, value, 0);
    if cstr.is_null() {
        return None;
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let out = core::str::from_utf8(bytes).ok().map(String::from);
    qjs::JS_FreeCString(ctx, cstr);
    out
}

unsafe extern "C" fn qjs_ai_input_pop(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(next) = PENDING_AI_INPUT.lock().pop_front() else {
        return qjs::JSValue::undefined();
    };

    let payload = alloc::format!(
        "{{\"text\":{},\"webSearch\":{},\"fileSearch\":{},\"newConversation\":{},\"computerUse\":{},\"shellTargetMask\":{}}}",
        json_string(next.text.as_str()),
        if next.web_search { "true" } else { "false" },
        if next.file_search { "true" } else { "false" },
        if next.new_conversation {
            "true"
        } else {
            "false"
        },
        if next.computer_use { "true" } else { "false" },
        next.shell_target_mask,
    );
    qjs::JS_NewStringLen(ctx, payload.as_ptr() as *const c_char, payload.len())
}

unsafe extern "C" fn qjs_uart1_shell_write(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(text) = read_js_string_arg(ctx, args[0]) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let wrote = qjs::trueos_shims::uart1_shell_write(text.as_bytes());
    qjs::JS_NewFloat64(ctx, wrote as f64)
}

unsafe extern "C" fn qjs_shell2_print_line(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(text) = read_js_string_arg(ctx, args[0]) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let target_mask = if argc >= 2 {
        let mut target_mask_f = 0.0f64;
        if qjs::JS_ToFloat64(ctx, &mut target_mask_f as *mut f64, args[1]) == 0
            && target_mask_f.is_finite()
            && target_mask_f >= 0.0
        {
            target_mask_f as u32
        } else {
            0
        }
    } else {
        0
    };
    let wrote = qjs::trueos_shims::shell2_print_targeted_line(target_mask, text.as_bytes());
    qjs::JS_NewFloat64(ctx, wrote as f64)
}

unsafe extern "C" fn qjs_browser_rpc_start(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 2 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(method) = read_js_string_arg(ctx, args[0]) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let Some(args_json) = read_js_string_arg(ctx, args[1]) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let mut browser_window_id = 0u32;
    if argc >= 3 {
        let mut window_id_f = 0.0f64;
        if qjs::JS_ToFloat64(ctx, &mut window_id_f as *mut f64, args[2]) == 0
            && window_id_f.is_finite()
            && window_id_f >= 0.0
        {
            browser_window_id = window_id_f as u32;
        }
    }
    let id = qjs::truesurfer_qjs_vm_task::queue_browser_rpc(method, args_json, browser_window_id);
    qjs::JS_NewFloat64(ctx, id as f64)
}

unsafe extern "C" fn qjs_browser_rpc_poll(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0
        || !id_f.is_finite()
        || id_f <= 0.0
    {
        return qjs::JSValue::undefined();
    }
    let id = id_f as u32;
    let Some(payload) = qjs::truesurfer_qjs_vm_task::take_browser_rpc_result(id) else {
        return qjs::JSValue::undefined();
    };
    qjs::JS_NewStringLen(ctx, payload.as_ptr() as *const c_char, payload.len())
}

unsafe extern "C" fn qjs_input_write_cursor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 6 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut slot_id_f = 0.0f64;
    let mut x_f = 0.0f64;
    let mut y_f = 0.0f64;
    let mut buttons_f = 0.0f64;
    let mut wheel_f = 0.0f64;
    let mut flags_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut slot_id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut x_f as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut y_f as *mut f64, args[2]) != 0
        || qjs::JS_ToFloat64(ctx, &mut buttons_f as *mut f64, args[3]) != 0
        || qjs::JS_ToFloat64(ctx, &mut wheel_f as *mut f64, args[4]) != 0
        || qjs::JS_ToFloat64(ctx, &mut flags_f as *mut f64, args[5]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let slot_id = if slot_id_f.is_finite() && slot_id_f >= 1.0 {
        slot_id_f as u32
    } else {
        0
    };
    if slot_id == 0 || !x_f.is_finite() || !y_f.is_finite() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let rc = qjs::trueos_shims::trueos_cabi_input_write_cursor(
        slot_id,
        round_to_i32(x_f),
        round_to_i32(y_f),
        if buttons_f.is_finite() && buttons_f >= 0.0 {
            buttons_f as u32
        } else {
            0
        },
        if wheel_f.is_finite() {
            round_to_i32(wheel_f)
        } else {
            0
        },
        if flags_f.is_finite() && flags_f >= 0.0 {
            flags_f as u32
        } else {
            0
        },
    );
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_input_write_keyboard_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 2 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);

    let mut slot_id_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut slot_id_f as *mut f64, args[0]) != 0
        || !slot_id_f.is_finite()
        || slot_id_f < 1.0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let Some(text) = read_js_string_arg(ctx, args[1]) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };

    let mut flags = 0u32;
    if argc >= 3 {
        let mut flags_f = 0.0f64;
        if qjs::JS_ToFloat64(ctx, &mut flags_f as *mut f64, args[2]) == 0
            && flags_f.is_finite()
            && flags_f >= 0.0
        {
            flags = flags_f as u32;
        }
    }

    let wrote =
        qjs::trueos_shims::input_write_keyboard_text(slot_id_f as u32, text.as_bytes(), flags);
    qjs::JS_NewFloat64(ctx, wrote as f64)
}

unsafe extern "C" fn qjs_input_write_keyboard_key(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 5 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut slot_id_f = 0.0f64;
    let mut codepoint_f = 0.0f64;
    let mut key_code_f = 0.0f64;
    let mut modifiers_f = 0.0f64;
    let mut flags_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut slot_id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut codepoint_f as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut key_code_f as *mut f64, args[2]) != 0
        || qjs::JS_ToFloat64(ctx, &mut modifiers_f as *mut f64, args[3]) != 0
        || qjs::JS_ToFloat64(ctx, &mut flags_f as *mut f64, args[4]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let slot_id = if slot_id_f.is_finite() && slot_id_f >= 1.0 {
        slot_id_f as u32
    } else {
        0
    };
    if slot_id == 0 {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let rc = qjs::trueos_shims::input_write_keyboard_key(
        slot_id,
        if codepoint_f.is_finite() && codepoint_f >= 0.0 {
            codepoint_f as u32
        } else {
            0
        },
        if key_code_f.is_finite() && key_code_f >= 0.0 {
            key_code_f as u32
        } else {
            0
        },
        if modifiers_f.is_finite() && modifiers_f >= 0.0 {
            modifiers_f as u32
        } else {
            0
        },
        if flags_f.is_finite() && flags_f >= 0.0 {
            flags_f as u32
        } else {
            0
        },
    );
    qjs::JS_NewFloat64(ctx, if rc > 0 { 1.0 } else { 0.0 })
}

fn json_string(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 8);
    out.push('"');
    for ch in src.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[inline]
fn round_to_i32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
    }
}

unsafe fn install_ai_globals(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    let ai_input_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_ai_input_pop),
        b"__trueosAiInputPop\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosAiInputPop\0".as_ptr() as *const c_char,
        ai_input_fn,
    );

    let shell_write_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_uart1_shell_write),
        b"__trueosUart1ShellWrite\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosUart1ShellWrite\0".as_ptr() as *const c_char,
        shell_write_fn,
    );

    let shell2_print_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_shell2_print_line),
        b"__trueosShell2PrintLine\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosShell2PrintLine\0".as_ptr() as *const c_char,
        shell2_print_fn,
    );

    let browser_rpc_start_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_browser_rpc_start),
        b"__trueosBrowserRpcStart\0".as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosBrowserRpcStart\0".as_ptr() as *const c_char,
        browser_rpc_start_fn,
    );

    let browser_rpc_poll_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_browser_rpc_poll),
        b"__trueosBrowserRpcPoll\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosBrowserRpcPoll\0".as_ptr() as *const c_char,
        browser_rpc_poll_fn,
    );

    let input_write_cursor_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_input_write_cursor),
        b"__trueosInputWriteCursor\0".as_ptr() as *const c_char,
        6,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosInputWriteCursor\0".as_ptr() as *const c_char,
        input_write_cursor_fn,
    );

    let input_write_keyboard_text_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_input_write_keyboard_text),
        b"__trueosInputWriteKeyboardText\0".as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosInputWriteKeyboardText\0".as_ptr() as *const c_char,
        input_write_keyboard_text_fn,
    );

    let input_write_keyboard_key_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_input_write_keyboard_key),
        b"__trueosInputWriteKeyboardKey\0".as_ptr() as *const c_char,
        5,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosInputWriteKeyboardKey\0".as_ptr() as *const c_char,
        input_write_keyboard_key_fn,
    );

    qjs::js_free_value(ctx, global);
}

#[embassy_executor::task]
pub async fn run_once() {
    if AI_TASK_STARTED.swap(true, Ordering::SeqCst) {
        qjs::trueos_shims::log_info("ai-task: already running\n");
        return;
    }

    qjs::trueos_shims::log_info("ai-task: run_once begin\n");
    unsafe {
        qjs::trueos_shims::log_info("ai-task: creating JS runtime\n");
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                qjs::trueos_shims::log_error("ai-task: JS runtime init failed\n");
                AI_TASK_STARTED.store(false, Ordering::SeqCst);
                AI_TASK_ACCEPTING_INPUT.store(false, Ordering::SeqCst);
                return;
            }
        };
        let ctx = vm.ctx_ptr();
        let rt = vm.rt_ptr();

        qjs::trueos_shims::log_info("ai-task: installing node globals\n");
        qjs::node::install_globals(ctx);
        qjs::trueos_shims::log_info("ai-task: installing ai globals\n");
        install_ai_globals(ctx);

        let shim_filename = b"<ai-shims>\0";
        let shim_src = br#"
        var __g = (typeof globalThis !== 'undefined') ? globalThis : this;
        if (typeof __g.Blob !== 'function') {
            __g.Blob = function Blob(_parts, opts) {
                this.size = 0;
                this.type = (opts && opts.type) ? String(opts.type) : '';
            };
            __g.Blob.prototype.arrayBuffer = function () { return Promise.resolve(new ArrayBuffer(0)); };
            __g.Blob.prototype.text = function () { return Promise.resolve(''); };
            __g.Blob.prototype.slice = function () { return this; };
        }
        if (typeof __g.File !== 'function') {
            __g.File = function File(parts, name, opts) {
                __g.Blob.call(this, parts, opts);
                this.name = String(name || 'file');
                this.lastModified = (opts && opts.lastModified) ? opts.lastModified : 0;
            };
            __g.File.prototype = Object.create(__g.Blob.prototype);
            __g.File.prototype.constructor = __g.File;
        }
        if (typeof __g.btoa !== 'function') {
            __g.btoa = function (_s) { return ''; };
        }
        if (typeof __g.atob !== 'function') {
            __g.atob = function (_s) { return ''; };
        }
"#;
        let shim = qjs::js_eval_bytes(
            ctx,
            shim_src,
            shim_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        );
        if shim.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "ai-task shims");
            qjs::js_free_value(ctx, shim);
            AI_TASK_STARTED.store(false, Ordering::SeqCst);
            AI_TASK_ACCEPTING_INPUT.store(false, Ordering::SeqCst);
            return;
        }
        qjs::js_free_value(ctx, shim);
        qjs::trueos_shims::log_info("ai-task: shim bootstrap ok\n");

        let filename = b"<ai-init-module>\0";
        let src = b"import '/qjs/ai/ai_pc.mjs';";
        qjs::trueos_shims::log_info("ai-task: importing /qjs/ai/ai_pc.mjs\n");
        let boot = qjs::js_eval_bytes(
            ctx,
            src,
            filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
        );
        if boot.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "ai-task init");
            qjs::js_free_value(ctx, boot);
            AI_TASK_STARTED.store(false, Ordering::SeqCst);
            AI_TASK_ACCEPTING_INPUT.store(false, Ordering::SeqCst);
            return;
        }
        qjs::js_free_value(ctx, boot);
        qjs::trueos_shims::log_info("ai-task: ai_pc import submitted\n");

        loop {
            if !qjs::vm::pump_runtime_once(rt, ctx, "ai") {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }

        qjs::trueos_shims::log_info("ai-task: runtime loop ended\n");
        let _ = qjs::vm::teardown_main_context(rt, ctx, 2_000).await;
    }

    AI_TASK_STARTED.store(false, Ordering::SeqCst);
    AI_TASK_ACCEPTING_INPUT.store(false, Ordering::SeqCst);
    qjs::trueos_shims::log_info("ai-task: run_once end\n");
}
