//! Host-owned QuickJS workbench VMs used by the `qjs.bp` Blueprint.
//!
//! The Blueprint owns terminal input and rendering.  This module deliberately
//! owns the interpreter so one QuickJS runtime survives every editor
//! submission and can use the same TRUEOS/Node module loader as other QJS VMs.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_void};
use core::ptr;

use spin::Mutex;
use trueos_qjs as qjs;

const OUTPUT_LINE_CAP: usize = 256;
const OUTPUT_BYTES_CAP: usize = 128 * 1024;

pub(crate) const MODE_AUTO: u32 = 0;
pub(crate) const MODE_SCRIPT: u32 = 1;
pub(crate) const MODE_MODULE: u32 = 2;

const RESULT_OK: u8 = 0;
const RESULT_JS_ERROR: u8 = 1;
const RESULT_HOST_ERROR: u8 = 2;
const RESULT_HEADER_LEN: usize = 10;

struct WorkbenchContextOpaque {
    output: VecDeque<String>,
    output_bytes: usize,
}

struct WorkbenchVm {
    vm_id: u8,
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    opaque: Box<WorkbenchContextOpaque>,
    eval_count: u64,
}

impl Drop for WorkbenchVm {
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

// Raw QuickJS pointers are only touched while WORKBENCHES is locked.
unsafe impl Send for WorkbenchVm {}

static WORKBENCHES: Mutex<Vec<Box<WorkbenchVm>>> = Mutex::new(Vec::new());

#[inline]
unsafe fn read_js_string(ctx: *mut qjs::JSContext, value: qjs::JSValueConst) -> Option<String> {
    let mut len = 0usize;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, value, 0);
    if cstr.is_null() {
        return None;
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let text = core::str::from_utf8(bytes).ok().map(String::from);
    qjs::JS_FreeCString(ctx, cstr);
    text
}

unsafe fn value_to_display_string(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValueConst,
) -> Option<String> {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return read_js_string(ctx, value);
    }
    let json = qjs::JS_GetPropertyStr(ctx, global, b"JSON\0".as_ptr() as *const c_char);
    qjs::js_free_value(ctx, global);
    if json.is_exception() {
        return read_js_string(ctx, value);
    }
    let stringify = qjs::JS_GetPropertyStr(ctx, json, b"stringify\0".as_ptr() as *const c_char);
    if stringify.is_exception() {
        qjs::js_free_value(ctx, json);
        return read_js_string(ctx, value);
    }

    let argument = qjs::js_dup_value(ctx, value);
    let rendered = qjs::JS_Call(ctx, stringify, json, 1, &argument as *const qjs::JSValueConst);
    qjs::js_free_value(ctx, argument);
    qjs::js_free_value(ctx, stringify);
    qjs::js_free_value(ctx, json);
    if rendered.is_exception() {
        let exception = qjs::JS_GetException(ctx);
        qjs::js_free_value(ctx, exception);
        return read_js_string(ctx, value);
    }
    if rendered.tag == qjs::JS_TAG_UNDEFINED {
        qjs::js_free_value(ctx, rendered);
        return read_js_string(ctx, value);
    }

    let text = read_js_string(ctx, rendered);
    qjs::js_free_value(ctx, rendered);
    text
}

unsafe fn exception_to_string(ctx: *mut qjs::JSContext) -> String {
    let exception = qjs::JS_GetException(ctx);
    let stack = qjs::JS_GetPropertyStr(ctx, exception, b"stack\0".as_ptr() as *const c_char);
    let message = if !stack.is_exception() && stack.tag != qjs::JS_TAG_UNDEFINED {
        read_js_string(ctx, stack)
    } else {
        None
    }
    .or_else(|| read_js_string(ctx, exception))
    .unwrap_or_else(|| String::from("<exception>"));
    qjs::js_free_value(ctx, stack);
    qjs::js_free_value(ctx, exception);
    message
}

fn push_output(opaque: &mut WorkbenchContextOpaque, line: String) {
    let line_bytes = line.len();
    while opaque.output.len() >= OUTPUT_LINE_CAP
        || opaque.output_bytes.saturating_add(line_bytes) > OUTPUT_BYTES_CAP
    {
        let Some(discarded) = opaque.output.pop_front() else {
            break;
        };
        opaque.output_bytes = opaque.output_bytes.saturating_sub(discarded.len());
    }
    if line_bytes <= OUTPUT_BYTES_CAP {
        opaque.output_bytes = opaque.output_bytes.saturating_add(line_bytes);
        opaque.output.push_back(line);
    }
}

unsafe extern "C" fn workbench_print(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let opaque = qjs::JS_GetContextOpaque(ctx) as *mut WorkbenchContextOpaque;
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
                    .or_else(|| read_js_string(ctx, *value))
                    .unwrap_or_else(|| String::from("<value>"))
                    .as_str(),
            );
        }
    }
    let len = line.len();
    push_output(&mut *opaque, line);
    qjs::JS_NewFloat64(ctx, len as f64)
}

unsafe fn install_workbench_globals(ctx: *mut qjs::JSContext) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let print = qjs::JS_NewCFunction2(
        ctx,
        Some(workbench_print),
        b"print\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, b"print\0".as_ptr() as *const c_char, print);

    let console = qjs::JS_GetPropertyStr(ctx, global, b"console\0".as_ptr() as *const c_char);
    if !console.is_exception() {
        for name in [
            b"log\0".as_slice(),
            b"info\0".as_slice(),
            b"warn\0".as_slice(),
            b"error\0".as_slice(),
        ] {
            let logger = qjs::JS_NewCFunction2(
                ctx,
                Some(workbench_print),
                name.as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ = qjs::JS_SetPropertyStr(ctx, console, name.as_ptr() as *const c_char, logger);
        }
    }
    qjs::js_free_value(ctx, console);
    qjs::js_free_value(ctx, global);
}

fn create_vm(vm_id: u8) -> Result<Box<WorkbenchVm>, &'static str> {
    let rt = unsafe { qjs::JS_NewRuntime() };
    if rt.is_null() {
        return Err("failed to create QuickJS runtime");
    }

    unsafe {
        qjs::qjs_diag::install_runtime(rt);
        qjs::node::install(rt);
    }
    let ctx = unsafe { qjs::JS_NewContext(rt) };
    if ctx.is_null() {
        unsafe { qjs::JS_FreeRuntime(rt) };
        return Err("failed to create QuickJS context");
    }
    unsafe {
        qjs::qjs_diag::install_context(ctx);
        qjs::node::install_globals_with_profile(ctx, qjs::node::RuntimeProfile::Shell);
    }

    let mut vm = Box::new(WorkbenchVm {
        vm_id,
        rt,
        ctx,
        opaque: Box::new(WorkbenchContextOpaque {
            output: VecDeque::new(),
            output_bytes: 0,
        }),
        eval_count: 0,
    });
    unsafe {
        qjs::JS_SetContextOpaque(vm.ctx, vm.opaque.as_mut() as *mut _ as *mut c_void);
        install_workbench_globals(vm.ctx);
    }
    Ok(vm)
}

fn ensure_vm(state: &mut Vec<Box<WorkbenchVm>>, vm_id: u8) -> Result<usize, &'static str> {
    if let Some(index) = state.iter().position(|vm| vm.vm_id == vm_id) {
        return Ok(index);
    }
    state.push(create_vm(vm_id)?);
    Ok(state.len() - 1)
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

fn source_uses_module_syntax(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut index = 0usize;
    let mut mode = ScanMode::Normal;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        match mode {
            ScanMode::Normal => {
                if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
                    mode = ScanMode::LineComment;
                    index += 2;
                    continue;
                }
                if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    mode = ScanMode::BlockComment;
                    index += 2;
                    continue;
                }
                match byte {
                    b'\'' => mode = ScanMode::SingleQuote,
                    b'"' => mode = ScanMode::DoubleQuote,
                    b'`' => mode = ScanMode::Backtick,
                    byte if byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic() => {
                        let start = index;
                        index += 1;
                        while bytes.get(index).is_some_and(|byte| {
                            *byte == b'_' || *byte == b'$' || byte.is_ascii_alphanumeric()
                        }) {
                            index += 1;
                        }
                        let token = &source[start..index];
                        if token == "export" {
                            return true;
                        }
                        if token == "import" {
                            let mut next = index;
                            while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                                next += 1;
                            }
                            if bytes.get(next) != Some(&b'(') {
                                return true;
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            ScanMode::SingleQuote | ScanMode::DoubleQuote | ScanMode::Backtick => {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if matches!(
                    (mode, byte),
                    (ScanMode::SingleQuote, b'\'')
                        | (ScanMode::DoubleQuote, b'"')
                        | (ScanMode::Backtick, b'`')
                ) {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::LineComment => {
                if byte == b'\n' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::BlockComment => {
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    mode = ScanMode::Normal;
                    index += 2;
                    continue;
                }
            }
        }
        index += 1;
    }
    false
}

fn encode_result(status: u8, mode: u8, eval_count: u64, text: &str) -> Vec<u8> {
    let mut response = Vec::with_capacity(RESULT_HEADER_LEN.saturating_add(text.len()));
    response.push(status);
    response.push(mode);
    response.extend_from_slice(&eval_count.to_le_bytes());
    response.extend_from_slice(text.as_bytes());
    response
}

pub(crate) fn eval(vm_id: u8, source: &str, requested_mode: u32) -> Vec<u8> {
    let mut state = WORKBENCHES.lock();
    let index = match ensure_vm(&mut state, vm_id) {
        Ok(index) => index,
        Err(error) => return encode_result(RESULT_HOST_ERROR, 0, 0, error),
    };
    let vm = state[index].as_mut();
    vm.eval_count = vm.eval_count.saturating_add(1);
    let mode = match requested_mode {
        MODE_SCRIPT => MODE_SCRIPT,
        MODE_MODULE => MODE_MODULE,
        MODE_AUTO if source_uses_module_syntax(source) => MODE_MODULE,
        _ => MODE_SCRIPT,
    };
    let filename = alloc::format!("<qjs-workbench-{:04}.mjs>\0", vm.eval_count);
    let value = unsafe {
        qjs::js_eval_bytes(
            vm.ctx,
            source.as_bytes(),
            filename.as_ptr() as *const c_char,
            if mode == MODE_MODULE {
                qjs::JS_EVAL_TYPE_MODULE
            } else {
                qjs::JS_EVAL_TYPE_GLOBAL
            },
        )
    };
    if value.is_exception() {
        let message = unsafe { exception_to_string(vm.ctx) };
        return encode_result(RESULT_JS_ERROR, mode as u8, vm.eval_count, message.as_str());
    }

    let rendered = unsafe { value_to_display_string(vm.ctx, value) };
    unsafe { qjs::js_free_value(vm.ctx, value) };
    let text = rendered.as_deref().unwrap_or("undefined");
    encode_result(RESULT_OK, mode as u8, vm.eval_count, text)
}

pub(crate) fn poll(vm_id: u8) -> Vec<u8> {
    let mut state = WORKBENCHES.lock();
    let Some(vm) = state.iter_mut().find(|vm| vm.vm_id == vm_id) else {
        return Vec::new();
    };
    if !unsafe { qjs::vm::pump_runtime_once(vm.rt, vm.ctx, "qjs-workbench") } {
        push_output(vm.opaque.as_mut(), String::from("runtime fault; reset the workbench VM"));
    }

    let mut output = String::new();
    while let Some(line) = vm.opaque.output.pop_front() {
        vm.opaque.output_bytes = vm.opaque.output_bytes.saturating_sub(line.len());
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(line.as_str());
    }
    output.into_bytes()
}

pub(crate) fn close(vm_id: u8) -> bool {
    let mut state = WORKBENCHES.lock();
    let Some(index) = state.iter().position(|vm| vm.vm_id == vm_id) else {
        return false;
    };
    let _ = state.swap_remove(index);
    true
}

#[cfg(test)]
mod tests {
    use super::source_uses_module_syntax;

    #[test]
    fn auto_mode_finds_native_module_syntax() {
        assert!(source_uses_module_syntax("import { readFile } from 'fs';"));
        assert!(source_uses_module_syntax("export const answer = 42;"));
        assert!(source_uses_module_syntax("import.meta.url"));
    }

    #[test]
    fn auto_mode_leaves_dynamic_import_and_strings_as_scripts() {
        assert!(!source_uses_module_syntax("import('fs').then(print)"));
        assert!(!source_uses_module_syntax("const text = 'export const nope = 1'"));
        assert!(!source_uses_module_syntax("// import x from 'y'\n1 + 1"));
    }
}
