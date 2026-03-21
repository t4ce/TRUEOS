use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_void};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
use trueos_qjs as qjs;

use super::{MatrixTarget, ShellBackend2, matrix};

struct ShellQjsContextOpaque {
    slot_id: matrix::MatrixSlotId,
}

struct ShellQjsVmSlot {
    slot_id: matrix::MatrixSlotId,
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    opaque: Box<ShellQjsContextOpaque>,
}

impl Drop for ShellQjsVmSlot {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                qjs::JS_SetContextOpaque(self.ctx, ptr::null_mut());
                qjs::workers::terminate_all_for_context(self.ctx);
                qjs::async_ops::drain_all_for_context(self.ctx);
                qjs::workers::drain_all_for_context(self.ctx);
                qjs::timers::drain_all_for_context(self.ctx);
                qjs::JS_FreeContext(self.ctx);
                self.ctx = ptr::null_mut();
            }
            if !self.rt.is_null() {
                qjs::JS_FreeRuntime(self.rt);
                self.rt = ptr::null_mut();
            }
        }
    }
}

// The shell owns these raw QuickJS pointers behind a single mutex and only drives
// them from the shell executor plus its dedicated repl drainer task.
unsafe impl Send for ShellQjsVmSlot {}

struct ShellQjsState {
    repl_slots: Vec<Box<ShellQjsVmSlot>>,
}

impl ShellQjsState {
    const fn new() -> Self {
        Self { repl_slots: Vec::new() }
    }
}

static SHELL_QJS_STATE: Mutex<ShellQjsState> = Mutex::new(ShellQjsState::new());
static SHELL_QJS_REPL_DRAINER_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QjsPromptMode {
    Repl,
    Eval,
}

impl QjsPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Repl => Self::Eval,
            Self::Eval => Self::Repl,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Repl => "repl",
            Self::Eval => "eval",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    Normal,
    SingleQuote,
    DoubleQuote,
    Backtick,
    LineComment,
    BlockComment,
}

pub(crate) fn is_likely_valid(source: &str) -> bool {
    let src = source.trim();
    if src.is_empty() {
        return false;
    }

    let mut mode = ScanMode::Normal;
    let mut escaped = false;
    let mut stack: heapless::Vec<char, 64> = heapless::Vec::new();
    let mut prev = '\0';

    for ch in src.chars() {
        match mode {
            ScanMode::Normal => {
                if prev == '/' && ch == '/' {
                    mode = ScanMode::LineComment;
                    prev = '\0';
                    continue;
                }
                if prev == '/' && ch == '*' {
                    mode = ScanMode::BlockComment;
                    prev = '\0';
                    continue;
                }

                match ch {
                    '\'' => mode = ScanMode::SingleQuote,
                    '"' => mode = ScanMode::DoubleQuote,
                    '`' => mode = ScanMode::Backtick,
                    '(' | '[' | '{' => {
                        let _ = stack.push(ch);
                    }
                    ')' => {
                        if stack.pop() != Some('(') {
                            return false;
                        }
                    }
                    ']' => {
                        if stack.pop() != Some('[') {
                            return false;
                        }
                    }
                    '}' => {
                        if stack.pop() != Some('{') {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            ScanMode::SingleQuote => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '\'' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::DoubleQuote => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::Backtick => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '`' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::LineComment => {
                if ch == '\n' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::BlockComment => {
                if prev == '*' && ch == '/' {
                    mode = ScanMode::Normal;
                    prev = '\0';
                    continue;
                }
            }
        }

        prev = ch;
    }

    mode == ScanMode::Normal && stack.is_empty()
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
    let opaque = qjs::JS_GetContextOpaque(ctx) as *mut ShellQjsContextOpaque;
    if opaque.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    print_slot_line(&(*opaque).slot_id, text.as_str());
    qjs::JS_NewFloat64(ctx, text.len() as f64)
}

unsafe fn install_shell_globals(ctx: *mut qjs::JSContext) {
    let global = qjs::JS_GetGlobalObject(ctx);
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
    qjs::js_free_value(ctx, global);
}

fn normalize_slot_id(requested: &str) -> matrix::MatrixSlotId {
    let trimmed = requested.trim();
    let trimmed = trimmed.strip_prefix('§').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('§').unwrap_or(trimmed);
    if trimmed.is_empty() {
        return matrix::MatrixSlotId::new();
    }

    let mut id = matrix::MatrixSlotId::new();
    for ch in trimmed.chars() {
        if id.push(ch).is_err() {
            break;
        }
    }
    id
}

fn print_slot_line(slot_id: &matrix::MatrixSlotId, text: &str) {
    matrix::record_line_in_slot(slot_id, super::LineSource::System, text);
}

fn print_target_line(target: &MatrixTarget, text: &str) {
    print_slot_line(&target.slot_id, text);
}

fn create_shell_vm(slot_id: &matrix::MatrixSlotId) -> Result<Box<ShellQjsVmSlot>, &'static str> {
    let rt = unsafe { qjs::JS_NewRuntime() };
    if rt.is_null() {
        return Err("qjs: failed to create runtime");
    }

    unsafe { qjs::qjs_diag::install_runtime(rt) };
    unsafe { qjs::node::install(rt) };

    let ctx = unsafe { qjs::JS_NewContext(rt) };
    if ctx.is_null() {
        unsafe { qjs::JS_FreeRuntime(rt) };
        return Err("qjs: failed to create context");
    }

    unsafe { qjs::qjs_diag::install_context(ctx) };
    unsafe { qjs::node::install_globals(ctx) };

    let mut slot = Box::new(ShellQjsVmSlot {
        slot_id: slot_id.clone(),
        rt,
        ctx,
        opaque: Box::new(ShellQjsContextOpaque {
            slot_id: slot_id.clone(),
        }),
    });
    unsafe {
        qjs::JS_SetContextOpaque(slot.ctx, slot.opaque.as_mut() as *mut _ as *mut c_void);
    }
    unsafe { install_shell_globals(ctx) };

    Ok(slot)
}

fn ensure_repl_vm<'a>(
    state: &'a mut ShellQjsState,
    slot_id: &matrix::MatrixSlotId,
) -> Result<&'a mut ShellQjsVmSlot, &'static str> {
    if let Some(idx) = state.repl_slots.iter().position(|slot| slot.slot_id == *slot_id) {
        return Ok(state.repl_slots[idx].as_mut());
    }

    let slot = create_shell_vm(slot_id)?;
    state.repl_slots.push(slot);
    let idx = state.repl_slots.len().saturating_sub(1);
    Ok(state.repl_slots[idx].as_mut())
}

fn drain_shell_vm(rt: *mut qjs::JSRuntime, ctx: *mut qjs::JSContext, label: &str) -> bool {
    for _ in 0..64 {
        if !unsafe { qjs::vm::pump_runtime_once(rt, ctx, label) } {
            return false;
        }
        let pending = unsafe { qjs::JS_IsJobPending(rt) > 0 }
            || unsafe { qjs::async_ops::has_pending(ctx) }
            || qjs::timers::has_pending(ctx)
            || qjs::workers::has_pending_for_ctx(ctx);
        if !pending {
            return true;
        }
    }
    true
}

unsafe fn js_value_to_string(ctx: *mut qjs::JSContext, value: qjs::JSValueConst) -> Option<String> {
    read_js_string_arg(ctx, value)
}

unsafe fn js_exception_to_string(ctx: *mut qjs::JSContext) -> String {
    let exc = qjs::JS_GetException(ctx);
    let stack = qjs::JS_GetPropertyStr(ctx, exc, b"stack\0".as_ptr() as *const c_char);
    let message = if !stack.is_exception() && stack.tag != qjs::JS_TAG_UNDEFINED {
        js_value_to_string(ctx, stack)
    } else {
        None
    }
    .or_else(|| js_value_to_string(ctx, exc))
    .unwrap_or_else(|| String::from("<exception>"));
    qjs::js_free_value(ctx, stack);
    qjs::js_free_value(ctx, exc);
    message
}

fn ensure_repl_drainer_started(spawner: &Spawner) -> bool {
    if SHELL_QJS_REPL_DRAINER_STARTED.load(Ordering::Acquire) {
        return true;
    }

    if SHELL_QJS_REPL_DRAINER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return true;
    }

    if spawner.spawn(shell_qjs_repl_slots_drainer()).is_err() {
        SHELL_QJS_REPL_DRAINER_STARTED.store(false, Ordering::Release);
        return false;
    }

    true
}

fn submit_repl(spawner: &Spawner, target: &MatrixTarget, source: &str) {
    if !ensure_repl_drainer_started(spawner) {
        print_target_line(target, "qjs repl error: failed to start repl drainer");
        return;
    }

    let mut state = SHELL_QJS_STATE.lock();
    let Ok(slot) = ensure_repl_vm(&mut state, &target.slot_id) else {
        print_target_line(target, "qjs repl error: failed to initialize slot runtime");
        return;
    };

    let value = unsafe {
        qjs::js_eval_bytes(
            slot.ctx,
            source.as_bytes(),
            b"<shell-qjs-repl>\0".as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        )
    };

    if value.is_exception() {
        let msg = unsafe { js_exception_to_string(slot.ctx) };
        let line = alloc::format!("qjs repl error: {}", msg);
        print_target_line(target, line.as_str());
        return;
    }

    let text = unsafe { js_value_to_string(slot.ctx, value) };
    unsafe { qjs::js_free_value(slot.ctx, value) };

    if let Some(text) = text {
        if !text.is_empty() && text != "undefined" {
            let line = alloc::format!("qjs repl => {}", text);
            print_target_line(target, line.as_str());
            return;
        }
    }

    print_target_line(target, "qjs repl ok");
}

fn submit_eval(target: &MatrixTarget, source: &str) {
    let Ok(vm) = create_shell_vm(&target.slot_id) else {
        print_target_line(target, "qjs eval error: failed to initialize eval runtime");
        return;
    };

    let rt = vm.rt;
    let ctx = vm.ctx;
    let value = unsafe {
        qjs::js_eval_bytes(
            ctx,
            source.as_bytes(),
            b"<shell-qjs-eval>\0".as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        )
    };

    if value.is_exception() {
        let msg = unsafe { js_exception_to_string(ctx) };
        let line = alloc::format!("qjs eval error: {}", msg);
        print_target_line(target, line.as_str());
        return;
    }

    let _ = drain_shell_vm(rt, ctx, "qjs-eval");
    let text = unsafe { js_value_to_string(ctx, value) };
    unsafe { qjs::js_free_value(ctx, value) };

    if let Some(text) = text {
        if !text.is_empty() && text != "undefined" {
            let line = alloc::format!("qjs eval => {}", text);
            print_target_line(target, line.as_str());
            return;
        }
    }

    print_target_line(target, "qjs eval ok");
}

pub(crate) fn free_slot(requested: &str) {
    let slot_id = normalize_slot_id(requested);
    let mut state = SHELL_QJS_STATE.lock();
    if let Some(idx) = state.repl_slots.iter().position(|slot| slot.slot_id == slot_id) {
        let _ = state.repl_slots.swap_remove(idx);
    }
}

pub(crate) fn submit(
    spawner: &Spawner,
    _io: &'static dyn ShellBackend2,
    target: &MatrixTarget,
    mode: QjsPromptMode,
    submitted: &str,
) {
    let source = submitted.trim();
    if source.is_empty() {
        print_target_line(target, "qjs: empty input");
        return;
    }

    if !is_likely_valid(source) {
        print_target_line(target, "qjs: input looks incomplete");
        return;
    }

    if mode == QjsPromptMode::Repl {
        submit_repl(spawner, target, source);
    } else {
        submit_eval(target, source);
    }
}

#[embassy_executor::task]
async fn shell_qjs_repl_slots_drainer() {
    loop {
        let sleep_ms = {
            let mut state = SHELL_QJS_STATE.lock();
            if state.repl_slots.is_empty() {
                50
            } else {
                let mut failed_indexes = Vec::new();
                for (idx, slot) in state.repl_slots.iter_mut().enumerate() {
                    if unsafe { qjs::vm::pump_runtime_once(slot.rt, slot.ctx, "shell-qjs-repl") } {
                        continue;
                    }

                    let line = alloc::format!(
                        "qjs repl slot §{}§ runtime fault; world reset",
                        slot.slot_id.as_str()
                    );
                    print_slot_line(&slot.slot_id, line.as_str());
                    failed_indexes.push(idx);
                }

                for idx in failed_indexes.into_iter().rev() {
                    let _ = state.repl_slots.swap_remove(idx);
                }

                5
            }
        };

        Timer::after(EmbassyDuration::from_millis(sleep_ms)).await;
    }
}
