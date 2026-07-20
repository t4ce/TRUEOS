use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ffi::{c_char, c_void};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
use trueos_qjs as qjs;

use super::shell2_cmd::CommandSessionKind;
use super::{CommandSessionInputResult, LineSource, MatrixTarget, matrix};

const SOURCE_HISTORY_CAP: usize = 32;
const SOURCE_HISTORY_VIEW: usize = 10;
const PENDING_SOURCE_MAX: usize = 16 * 1024;

struct ShellQjsContextOpaque {
    slot_id: matrix::MatrixSlotId,
}

struct ShellQjsVmSlot {
    slot_id: matrix::MatrixSlotId,
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    opaque: Box<ShellQjsContextOpaque>,
    eval_count: u64,
    pending_source: String,
    history: VecDeque<String>,
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

// The shell drives these raw QuickJS pointers behind one mutex from its executor
// and from the dedicated pending-job drainer.
unsafe impl Send for ShellQjsVmSlot {}

struct ShellQjsState {
    sessions: Vec<Box<ShellQjsVmSlot>>,
}

impl ShellQjsState {
    const fn new() -> Self {
        Self {
            sessions: Vec::new(),
        }
    }
}

static SHELL_QJS_STATE: Mutex<ShellQjsState> = Mutex::new(ShellQjsState::new());
static SHELL_QJS_DRAINER_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    Normal,
    SingleQuote,
    DoubleQuote,
    Backtick,
    LineComment,
    BlockComment,
}

fn is_likely_complete(source: &str) -> bool {
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
                            return true;
                        }
                    }
                    ']' => {
                        if stack.pop() != Some('[') {
                            return true;
                        }
                    }
                    '}' => {
                        if stack.pop() != Some('{') {
                            return true;
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

    matches!(mode, ScanMode::Normal | ScanMode::LineComment) && stack.is_empty()
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

unsafe fn value_to_display_string(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValueConst,
) -> Option<String> {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return read_js_string_arg(ctx, value);
    }
    let json = qjs::JS_GetPropertyStr(ctx, global, b"JSON\0".as_ptr() as *const c_char);
    qjs::js_free_value(ctx, global);
    if json.is_exception() {
        return read_js_string_arg(ctx, value);
    }
    let stringify = qjs::JS_GetPropertyStr(
        ctx,
        json,
        b"stringify\0".as_ptr() as *const c_char,
    );
    if stringify.is_exception() {
        qjs::js_free_value(ctx, json);
        return read_js_string_arg(ctx, value);
    }

    let arg = qjs::js_dup_value(ctx, value);
    let rendered = qjs::JS_Call(ctx, stringify, json, 1, &arg as *const qjs::JSValueConst);
    qjs::js_free_value(ctx, arg);
    qjs::js_free_value(ctx, stringify);
    qjs::js_free_value(ctx, json);
    if rendered.is_exception() {
        let exception = qjs::JS_GetException(ctx);
        qjs::js_free_value(ctx, exception);
        return read_js_string_arg(ctx, value);
    }
    if rendered.tag == qjs::JS_TAG_UNDEFINED {
        qjs::js_free_value(ctx, rendered);
        return read_js_string_arg(ctx, value);
    }

    let out = read_js_string_arg(ctx, rendered);
    qjs::js_free_value(ctx, rendered);
    out
}

unsafe extern "C" fn qjs_tui_print(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let opaque = qjs::JS_GetContextOpaque(ctx) as *mut ShellQjsContextOpaque;
    if opaque.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let mut line = String::new();
    if argc > 0 && !argv.is_null() {
        for value in core::slice::from_raw_parts(argv, argc as usize) {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(
                value_to_display_string(ctx, *value)
                    .or_else(|| read_js_string_arg(ctx, *value))
                    .unwrap_or_else(|| String::from("<value>"))
                    .as_str(),
            );
        }
    }
    print_slot_line(&(*opaque).slot_id, line.as_str());
    qjs::JS_NewFloat64(ctx, line.len() as f64)
}

unsafe extern "C" fn qjs_shell2_slot_array_buffer(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let slot_id = if argc >= 1 && !argv.is_null() {
        let args = core::slice::from_raw_parts(argv, argc as usize);
        read_js_string_arg(ctx, args[0])
            .map(|requested| normalize_slot_id(requested.as_str()))
            .unwrap_or_else(matrix::MatrixSlotId::new)
    } else {
        let opaque = qjs::JS_GetContextOpaque(ctx) as *mut ShellQjsContextOpaque;
        if opaque.is_null() {
            matrix::MatrixSlotId::new()
        } else {
            (*opaque).slot_id.clone()
        }
    };

    let text = matrix::slot_transcript_text(&slot_id);
    qjs::JS_NewArrayBufferCopy(ctx, text.as_bytes().as_ptr(), text.len())
}

unsafe fn install_tui_globals(ctx: *mut qjs::JSContext) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let print_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_tui_print),
        b"print\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, b"print\0".as_ptr() as *const c_char, print_fn);

    for name in [b"abuffer\0".as_slice(), "§\0".as_bytes()] {
        let slot_fn = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_shell2_slot_array_buffer),
            name.as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let _ = qjs::JS_SetPropertyStr(ctx, global, name.as_ptr() as *const c_char, slot_fn);
    }
    qjs::js_free_value(ctx, global);
}

fn normalize_slot_id(requested: &str) -> matrix::MatrixSlotId {
    let trimmed = requested.trim();
    let trimmed = trimmed.strip_prefix('§').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('§').unwrap_or(trimmed);
    let mut id = matrix::MatrixSlotId::new();
    for ch in trimmed.chars() {
        if id.push(ch).is_err() {
            break;
        }
    }
    id
}

fn print_slot_line(slot_id: &matrix::MatrixSlotId, text: &str) {
    matrix::record_line_in_slot(slot_id, LineSource::System, text);
}

fn print_target_line(target: &MatrixTarget, text: &str) {
    print_slot_line(&target.slot_id, text);
}

fn create_vm(slot_id: &matrix::MatrixSlotId) -> Result<Box<ShellQjsVmSlot>, &'static str> {
    let rt = unsafe { qjs::JS_NewRuntime() };
    if rt.is_null() {
        return Err("failed to create QuickJS runtime");
    }

    unsafe { qjs::qjs_diag::install_runtime(rt) };
    // This is the shared TRUEOS + Node-style loader used by the other QJS VMs.
    unsafe { qjs::node::install(rt) };

    let ctx = unsafe { qjs::JS_NewContext(rt) };
    if ctx.is_null() {
        unsafe { qjs::JS_FreeRuntime(rt) };
        return Err("failed to create QuickJS context");
    }

    unsafe { qjs::qjs_diag::install_context(ctx) };
    unsafe { qjs::node::install_globals_with_profile(ctx, qjs::node::RuntimeProfile::Shell) };

    let mut slot = Box::new(ShellQjsVmSlot {
        slot_id: slot_id.clone(),
        rt,
        ctx,
        opaque: Box::new(ShellQjsContextOpaque {
            slot_id: slot_id.clone(),
        }),
        eval_count: 0,
        pending_source: String::new(),
        history: VecDeque::new(),
    });
    unsafe {
        qjs::JS_SetContextOpaque(slot.ctx, slot.opaque.as_mut() as *mut _ as *mut c_void);
        install_tui_globals(slot.ctx);
    }
    Ok(slot)
}

fn ensure_vm<'a>(
    state: &'a mut ShellQjsState,
    slot_id: &matrix::MatrixSlotId,
) -> Result<&'a mut ShellQjsVmSlot, &'static str> {
    if let Some(index) = state
        .sessions
        .iter()
        .position(|session| session.slot_id == *slot_id)
    {
        return Ok(state.sessions[index].as_mut());
    }
    state.sessions.push(create_vm(slot_id)?);
    Ok(state
        .sessions
        .last_mut()
        .expect("QJS session was just inserted")
        .as_mut())
}

unsafe fn exception_to_string(ctx: *mut qjs::JSContext) -> String {
    let exception = qjs::JS_GetException(ctx);
    let stack = qjs::JS_GetPropertyStr(ctx, exception, b"stack\0".as_ptr() as *const c_char);
    let message = if !stack.is_exception() && stack.tag != qjs::JS_TAG_UNDEFINED {
        read_js_string_arg(ctx, stack)
    } else {
        None
    }
    .or_else(|| read_js_string_arg(ctx, exception))
    .unwrap_or_else(|| String::from("<exception>"));
    qjs::js_free_value(ctx, stack);
    qjs::js_free_value(ctx, exception);
    message
}

fn ensure_drainer_started(spawner: &Spawner) -> bool {
    if SHELL_QJS_DRAINER_STARTED.load(Ordering::Acquire) {
        return true;
    }
    if SHELL_QJS_DRAINER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return true;
    }

    match shell_qjs_sessions_drainer() {
        Ok(token) => {
            spawner.spawn(token);
            true
        }
        Err(_) => {
            SHELL_QJS_DRAINER_STARTED.store(false, Ordering::Release);
            false
        }
    }
}

fn dashboard_lines() -> [&'static str; 7] {
    [
        "╭─ QuickJS scripting workbench ─────────────────────────────────────────────╮",
        "│ VM       persistent for this TUI session                                   │",
        "│ Runtime  shell profile · timers · workers · fetch                          │",
        "│ Modules  TRUEOS/Node loader · :import SPECIFIER → $module                  │",
        "├─ Editor / REPL ────────────────────────────────────────────────────────────┤",
        "│ Enter JavaScript · ↑/↓ history · :help · :reset · :clear · :quit · ESC     │",
        "╰────────────────────────────────────────────────────────────────────────────╯",
    ]
}

fn print_dashboard(target: &MatrixTarget) {
    // Matrix transcripts are newest-first, so insert the panel bottom-to-top.
    for line in dashboard_lines().iter().rev() {
        print_target_line(target, line);
    }
}

pub(crate) fn begin_session(spawner: &Spawner, target: &MatrixTarget) -> Result<(), &'static str> {
    if !ensure_drainer_started(spawner) {
        return Err("background job drainer is unavailable");
    }
    {
        let mut state = SHELL_QJS_STATE.lock();
        let _ = ensure_vm(&mut state, &target.slot_id)?;
    }
    print_dashboard(target);
    Ok(())
}

pub(crate) fn is_session_active(slot_id: &matrix::MatrixSlotId) -> bool {
    SHELL_QJS_STATE
        .lock()
        .sessions
        .iter()
        .any(|session| session.slot_id == *slot_id)
}

pub(crate) fn free_slot(requested: &str) {
    let slot_id = normalize_slot_id(requested);
    let mut state = SHELL_QJS_STATE.lock();
    if let Some(index) = state
        .sessions
        .iter()
        .position(|session| session.slot_id == slot_id)
    {
        let _ = state.sessions.swap_remove(index);
    }
}

pub(crate) fn end_session(target: &MatrixTarget, reason: &str) {
    {
        let mut state = SHELL_QJS_STATE.lock();
        if let Some(index) = state
            .sessions
            .iter()
            .position(|session| session.slot_id == target.slot_id)
        {
            let _ = state.sessions.swap_remove(index);
        }
    }
    print_target_line(target, alloc::format!("qjs: workbench closed ({reason})").as_str());
}

fn reset_session(target: &MatrixTarget) {
    let result = {
        let mut state = SHELL_QJS_STATE.lock();
        if let Some(index) = state
            .sessions
            .iter()
            .position(|session| session.slot_id == target.slot_id)
        {
            let _ = state.sessions.swap_remove(index);
        }
        create_vm(&target.slot_id).map(|vm| state.sessions.push(vm))
    };
    match result {
        Ok(()) => print_target_line(target, "qjs: VM reset; globals and pending jobs were discarded"),
        Err(error) => print_target_line(target, alloc::format!("qjs: reset failed: {error}").as_str()),
    }
}

fn print_help(target: &MatrixTarget) {
    for line in [
        "qjs controls:",
        "  :help                 show this help",
        "  :clear                clear the workbench transcript",
        "  :reset                replace the persistent VM",
        "  :history              show recent evaluated sources",
        "  :cancel               discard a multiline continuation",
        "  :import SPECIFIER     load through the shared module loader into $module",
        "  :modules              show supported module specifiers",
        "  :quit                 close the workbench (ESC also closes it)",
    ]
    .iter()
    .rev()
    {
        print_target_line(target, line);
    }
}

fn print_modules_help(target: &MatrixTarget) {
    for line in [
        "qjs module loader:",
        "  embedded/native:  :import fs  ·  :import complex  ·  :import node:events",
        "  TRUEOSFS:         :import /path/to/module.mjs",
        "  URL/cache:        :import https://example.test/module.mjs",
        "  namespace:        the last successful import is stored in globalThis.$module",
    ]
    .iter()
    .rev()
    {
        print_target_line(target, line);
    }
}

fn push_js_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                use core::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn queue_module_import(target: &MatrixTarget, specifier: &str) {
    if specifier.is_empty() {
        print_target_line(target, "qjs: usage `:import SPECIFIER`");
        return;
    }
    let mut quoted = String::new();
    push_js_string(&mut quoted, specifier);
    let source = alloc::format!(
        "importModule({quoted}).then(function (module) {{ globalThis.$module = module; print('module loaded as $module:', {quoted}); return module; }})"
    );
    evaluate_source(target, source.as_str(), Some("module import"));
}

fn print_history(target: &MatrixTarget) {
    let history = {
        let state = SHELL_QJS_STATE.lock();
        state
            .sessions
            .iter()
            .find(|session| session.slot_id == target.slot_id)
            .map(|session| session.history.clone())
            .unwrap_or_default()
    };
    if history.is_empty() {
        print_target_line(target, "qjs: source history is empty");
        return;
    }
    for (index, source) in history.iter().rev().take(SOURCE_HISTORY_VIEW).enumerate().rev() {
        let compact = source.replace('\n', " ↵ ");
        print_target_line(
            target,
            alloc::format!("  -{}  {}", index + 1, compact).as_str(),
        );
    }
    print_target_line(target, "qjs source history (newest first):");
}

fn evaluate_source(target: &MatrixTarget, source: &str, label: Option<&str>) {
    let mut state = SHELL_QJS_STATE.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.slot_id == target.slot_id)
    else {
        print_target_line(target, "qjs: VM is unavailable; close and reopen the workbench");
        return;
    };

    session.eval_count = session.eval_count.saturating_add(1);
    let eval_number = session.eval_count;
    if session.history.len() >= SOURCE_HISTORY_CAP {
        let _ = session.history.pop_front();
    }
    session.history.push_back(source.to_string());

    let value = unsafe {
        qjs::js_eval_bytes(
            session.ctx,
            source.as_bytes(),
            b"<shell-qjs-tui>\0".as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        )
    };
    if value.is_exception() {
        let message = unsafe { exception_to_string(session.ctx) };
        print_target_line(
            target,
            alloc::format!("qjs #{eval_number:04} error: {message}").as_str(),
        );
        return;
    }

    let rendered = unsafe { value_to_display_string(session.ctx, value) };
    unsafe { qjs::js_free_value(session.ctx, value) };
    match rendered.as_deref() {
        Some("undefined") | None => print_target_line(
            target,
            alloc::format!("qjs #{eval_number:04} {}ok", label.unwrap_or("")).as_str(),
        ),
        Some(text) => print_target_line(
            target,
            alloc::format!("qjs #{eval_number:04} ⇒ {text}").as_str(),
        ),
    }
}

fn take_complete_source(target: &MatrixTarget, submitted: &str) -> Option<String> {
    let mut state = SHELL_QJS_STATE.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.slot_id == target.slot_id)
    else {
        return Some(submitted.to_string());
    };

    let mut source = core::mem::take(&mut session.pending_source);
    if !source.is_empty() {
        source.push('\n');
    }
    source.push_str(submitted);
    if is_likely_complete(source.as_str()) {
        return Some(source);
    }
    if source.len() > PENDING_SOURCE_MAX {
        print_target_line(target, "qjs: multiline source exceeded 16 KiB and was discarded");
        return None;
    }
    session.pending_source = source;
    print_target_line(target, "qjs … multiline continuation (:cancel to discard)");
    None
}

pub(crate) fn handle_session_input(
    _spawner: &Spawner,
    target: &MatrixTarget,
    submitted: &str,
) -> CommandSessionInputResult {
    let trimmed = submitted.trim();
    if matches!(trimmed, ":quit" | ".quit" | ":q" | "Quit") {
        end_session(target, ":quit");
        return CommandSessionInputResult::CompleteIdle;
    }
    match trimmed {
        ":help" | ".help" => {
            print_help(target);
            return CommandSessionInputResult::KeepRunning;
        }
        ":clear" | ".clear" => {
            matrix::clear_active_lines(target.output_mask);
            print_dashboard(target);
            return CommandSessionInputResult::KeepRunning;
        }
        ":reset" | ".reset" => {
            reset_session(target);
            return CommandSessionInputResult::KeepRunning;
        }
        ":history" | ".history" => {
            print_history(target);
            return CommandSessionInputResult::KeepRunning;
        }
        ":cancel" | ".cancel" => {
            let mut state = SHELL_QJS_STATE.lock();
            if let Some(session) = state
                .sessions
                .iter_mut()
                .find(|session| session.slot_id == target.slot_id)
            {
                session.pending_source.clear();
            }
            print_target_line(target, "qjs: multiline continuation discarded");
            return CommandSessionInputResult::KeepRunning;
        }
        ":modules" | ".modules" => {
            print_modules_help(target);
            return CommandSessionInputResult::KeepRunning;
        }
        _ => {}
    }

    if let Some(specifier) = trimmed
        .strip_prefix(":import ")
        .or_else(|| trimmed.strip_prefix(":load "))
        .map(str::trim)
    {
        queue_module_import(target, specifier);
        return CommandSessionInputResult::KeepRunning;
    }
    if trimmed.starts_with(':') || trimmed.starts_with('.') && !trimmed.starts_with("..") {
        print_target_line(target, "qjs: unknown workbench command; use :help");
        return CommandSessionInputResult::KeepRunning;
    }
    if trimmed.is_empty() {
        return CommandSessionInputResult::KeepRunning;
    }

    if let Some(source) = take_complete_source(target, submitted) {
        evaluate_source(target, source.as_str(), None);
    }
    CommandSessionInputResult::KeepRunning
}

pub(crate) fn session_kind() -> CommandSessionKind {
    CommandSessionKind::Qjs
}

#[embassy_executor::task]
async fn shell_qjs_sessions_drainer() {
    loop {
        let sleep_ms = {
            let mut state = SHELL_QJS_STATE.lock();
            if state.sessions.is_empty() {
                50
            } else {
                let mut failed = Vec::new();
                for (index, session) in state.sessions.iter_mut().enumerate() {
                    if unsafe {
                        qjs::vm::pump_runtime_once(session.rt, session.ctx, "shell-qjs-tui")
                    } {
                        continue;
                    }
                    print_slot_line(
                        &session.slot_id,
                        "qjs: runtime fault; close and reopen the workbench",
                    );
                    failed.push(index);
                }
                for index in failed.into_iter().rev() {
                    let _ = state.sessions.swap_remove(index);
                }
                5
            }
        };
        Timer::after(EmbassyDuration::from_millis(sleep_ms)).await;
    }
}
