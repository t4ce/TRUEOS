#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

use spin::Mutex;

use crate as qjs;
use crate::trueos_shims;

#[derive(Clone, Default)]
struct ContextMenuState {
    open: bool,
    x: f64,
    y: f64,
    target: Option<String>,
}

#[derive(Clone, Default)]
struct CursorInputState {
    id: u32,
    x: f64,
    y: f64,
    focused: Option<String>,
    hovered: Option<String>,
    clipboard: String,
    menu: ContextMenuState,
    pointer_down_seq: u32,
    pointer_down_button: i32,
}

#[derive(Default)]
struct BrowserContextState {
    active_cursor: u32,
    cursors: Vec<CursorInputState>,
}

static BROWSER_CONTEXT_STATE: Mutex<BrowserContextState> = Mutex::new(BrowserContextState {
    active_cursor: 1,
    cursors: Vec::new(),
});

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

#[inline]
unsafe fn js_get_f64(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<f64> {
    let mut out = 0.0f64;
    let rc = unsafe { qjs::JS_ToFloat64(ctx, &mut out as *mut f64, v) };
    if rc == 0 && out.is_finite() {
        Some(out)
    } else {
        None
    }
}

#[inline]
unsafe fn js_get_u32_or(ctx: *mut qjs::JSContext, v: qjs::JSValueConst, default: u32) -> u32 {
    match unsafe { js_get_f64(ctx, v) } {
        Some(n) if n >= 0.0 => n.min(u32::MAX as f64) as u32,
        _ => default,
    }
}

#[inline]
unsafe fn js_get_f64_or(ctx: *mut qjs::JSContext, v: qjs::JSValueConst, default: f64) -> f64 {
    unsafe { js_get_f64(ctx, v) }.unwrap_or(default)
}

#[inline]
unsafe fn js_get_optional_string(
    ctx: *mut qjs::JSContext,
    v: qjs::JSValueConst,
) -> Option<String> {
    if v.tag == qjs::JS_TAG_NULL || v.tag == qjs::JS_TAG_UNDEFINED {
        return None;
    }
    let ptr = unsafe { qjs::js_to_cstring(ctx, v) };
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(ptr).to_bytes() };
    let s = String::from_utf8_lossy(bytes).into_owned();
    unsafe { qjs::JS_FreeCString(ctx, ptr) };
    Some(s)
}

#[inline]
unsafe fn js_new_string(ctx: *mut qjs::JSContext, s: &str) -> qjs::JSValue {
    unsafe { qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len()) }
}

#[inline]
fn get_or_create_cursor(
    state: &mut BrowserContextState,
    cursor_id: u32,
) -> &mut CursorInputState {
    if let Some(i) = state.cursors.iter().position(|c| c.id == cursor_id) {
        return &mut state.cursors[i];
    }
    state.cursors.push(CursorInputState {
        id: cursor_id,
        ..CursorInputState::default()
    });
    let idx = state.cursors.len() - 1;
    &mut state.cursors[idx]
}

#[inline]
fn find_cursor(state: &BrowserContextState, cursor_id: u32) -> Option<&CursorInputState> {
    state.cursors.iter().find(|c| c.id == cursor_id)
}

#[inline]
fn live_cursor_pos(cursor_id: u32) -> Option<(f64, f64)> {
    if cursor_id == 0 {
        return None;
    }
    let mut x = 0i32;
    let mut y = 0i32;
    let rc = unsafe { trueos_shims::trueos_cabi_input_cursor_pos(cursor_id, &mut x, &mut y) };
    if rc == 0 {
        Some((x as f64, y as f64))
    } else {
        None
    }
}

unsafe extern "C" fn qjs_browser_context_set_active_cursor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor = if args.is_empty() {
        1
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let mut st = BROWSER_CONTEXT_STATE.lock();
    st.active_cursor = cursor;
    let _ = get_or_create_cursor(&mut st, cursor);
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_browser_context_get_active_cursor(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let st = BROWSER_CONTEXT_STATE.lock();
    js_int32(st.active_cursor as i32)
}

unsafe extern "C" fn qjs_browser_context_route_pointer_move(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 4 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let cursor_id = unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1);
    let x = unsafe { js_get_f64_or(ctx, args[1], 0.0) };
    let y = unsafe { js_get_f64_or(ctx, args[2], 0.0) };
    let target = unsafe { js_get_optional_string(ctx, args[3]) };

    let mut st = BROWSER_CONTEXT_STATE.lock();
    st.active_cursor = cursor_id;
    let c = get_or_create_cursor(&mut st, cursor_id);
    c.x = x;
    c.y = y;
    c.hovered = target;
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_browser_context_route_pointer_down(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 4 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let cursor_id = unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1);
    let x = unsafe { js_get_f64_or(ctx, args[1], 0.0) };
    let y = unsafe { js_get_f64_or(ctx, args[2], 0.0) };
    let target = unsafe { js_get_optional_string(ctx, args[3]) };
    let button = if argc > 4 {
        unsafe { js_get_f64_or(ctx, args[4], 0.0) as i32 }
    } else {
        0
    };

    let mut st = BROWSER_CONTEXT_STATE.lock();
    st.active_cursor = cursor_id;
    let c = get_or_create_cursor(&mut st, cursor_id);
    c.x = x;
    c.y = y;
    c.hovered = target.clone();
    c.focused = target;
    c.pointer_down_seq = c.pointer_down_seq.wrapping_add(1);
    c.pointer_down_button = button;
    c.menu.open = false;
    c.menu.target = None;
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_browser_context_get_pointer_down_seq(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let seq = find_cursor(&st, cursor_id)
        .map(|c| c.pointer_down_seq as i32)
        .unwrap_or(0);
    js_int32(seq)
}

unsafe extern "C" fn qjs_browser_context_get_pointer_down_button(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let button = find_cursor(&st, cursor_id)
        .map(|c| c.pointer_down_button)
        .unwrap_or(0);
    js_int32(button)
}

unsafe extern "C" fn qjs_browser_context_route_pointer_up(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 4 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let cursor_id = unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1);
    let x = unsafe { js_get_f64_or(ctx, args[1], 0.0) };
    let y = unsafe { js_get_f64_or(ctx, args[2], 0.0) };
    let target = unsafe { js_get_optional_string(ctx, args[3]) };

    let mut st = BROWSER_CONTEXT_STATE.lock();
    st.active_cursor = cursor_id;
    let c = get_or_create_cursor(&mut st, cursor_id);
    c.x = x;
    c.y = y;
    c.hovered = target;
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_browser_context_open_context_menu(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 4 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let cursor_id = unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1);
    let x = unsafe { js_get_f64_or(ctx, args[1], 0.0) };
    let y = unsafe { js_get_f64_or(ctx, args[2], 0.0) };
    let target = unsafe { js_get_optional_string(ctx, args[3]) };

    let mut st = BROWSER_CONTEXT_STATE.lock();
    st.active_cursor = cursor_id;
    let c = get_or_create_cursor(&mut st, cursor_id);
    c.x = x;
    c.y = y;
    c.menu.open = true;
    c.menu.x = x;
    c.menu.y = y;
    c.menu.target = target.clone();
    if target.is_some() {
        c.focused = target;
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_browser_context_close_context_menu(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };

    let mut st = BROWSER_CONTEXT_STATE.lock();
    let c = get_or_create_cursor(&mut st, cursor_id);
    c.menu.open = false;
    c.menu.target = None;
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_browser_context_is_context_menu_open(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let open = find_cursor(&st, cursor_id).map(|c| c.menu.open).unwrap_or(false);
    js_bool(open)
}

unsafe extern "C" fn qjs_browser_context_get_context_menu_x(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let x = find_cursor(&st, cursor_id).map(|c| c.menu.x).unwrap_or(0.0);
    js_int32(x as i32)
}

unsafe extern "C" fn qjs_browser_context_get_context_menu_y(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let y = find_cursor(&st, cursor_id).map(|c| c.menu.y).unwrap_or(0.0);
    js_int32(y as i32)
}

unsafe extern "C" fn qjs_browser_context_get_context_menu_target(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let Some(target) = find_cursor(&st, cursor_id).and_then(|c| c.menu.target.as_ref()) else {
        return js_null();
    };
    unsafe { js_new_string(ctx, target) }
}

unsafe extern "C" fn qjs_browser_context_get_focused_target(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let Some(target) = find_cursor(&st, cursor_id).and_then(|c| c.focused.as_ref()) else {
        return js_null();
    };
    unsafe { js_new_string(ctx, target) }
}

unsafe extern "C" fn qjs_browser_context_get_hovered_target(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let Some(target) = find_cursor(&st, cursor_id).and_then(|c| c.hovered.as_ref()) else {
        return js_null();
    };
    unsafe { js_new_string(ctx, target) }
}

unsafe extern "C" fn qjs_browser_context_set_clipboard_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let cursor_id = unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1);
    let text = unsafe { js_get_optional_string(ctx, args[1]) }.unwrap_or_default();
    let mut st = BROWSER_CONTEXT_STATE.lock();
    let c = get_or_create_cursor(&mut st, cursor_id);
    c.clipboard = text;
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_browser_context_get_clipboard_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let st = BROWSER_CONTEXT_STATE.lock();
    let Some(c) = find_cursor(&st, cursor_id) else {
        return unsafe { js_new_string(ctx, "") };
    };
    unsafe { js_new_string(ctx, c.clipboard.as_str()) }
}

unsafe extern "C" fn qjs_browser_context_clear_clipboard(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    let mut st = BROWSER_CONTEXT_STATE.lock();
    let c = get_or_create_cursor(&mut st, cursor_id);
    c.clipboard.clear();
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_browser_context_get_cursor_x(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    if let Some((x, _)) = live_cursor_pos(cursor_id) {
        return js_int32(x as i32);
    }
    let st = BROWSER_CONTEXT_STATE.lock();
    let x = find_cursor(&st, cursor_id).map(|c| c.x).unwrap_or(0.0);
    js_int32(x as i32)
}

unsafe extern "C" fn qjs_browser_context_get_cursor_y(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let args = if !argv.is_null() && argc > 0 {
        unsafe { core::slice::from_raw_parts(argv, argc as usize) }
    } else {
        &[]
    };
    let cursor_id = if args.is_empty() {
        BROWSER_CONTEXT_STATE.lock().active_cursor
    } else {
        unsafe { js_get_u32_or(ctx, args[0], 1) }.max(1)
    };
    if let Some((_, y)) = live_cursor_pos(cursor_id) {
        return js_int32(y as i32);
    }
    let st = BROWSER_CONTEXT_STATE.lock();
    let y = find_cursor(&st, cursor_id).map(|c| c.y).unwrap_or(0.0);
    js_int32(y as i32)
}

pub unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(module_name).to_bytes() };
    if name != b"trueos:browser_context" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn qjs_browser_context_module_init(
        ctx: *mut qjs::JSContext,
        m: *mut qjs::JSModuleDef,
    ) -> i32 {
        macro_rules! export_fn {
            ($name:literal, $func:expr, $argc:expr) => {{
                let k = concat!($name, "\0");
                let f = qjs::JS_NewCFunction2(
                    ctx,
                    Some($func),
                    k.as_ptr() as *const c_char,
                    $argc,
                    qjs::JS_CFUNC_GENERIC,
                    0,
                );
                let _ = qjs::JS_SetModuleExport(ctx, m, k.as_ptr() as *const c_char, f);
            }};
        }
        export_fn!("setActiveCursor", qjs_browser_context_set_active_cursor, 1);
        export_fn!("getActiveCursor", qjs_browser_context_get_active_cursor, 0);
        export_fn!("routePointerMove", qjs_browser_context_route_pointer_move, 4);
        export_fn!("routePointerDown", qjs_browser_context_route_pointer_down, 5);
        export_fn!("routePointerUp", qjs_browser_context_route_pointer_up, 4);
        export_fn!("openContextMenu", qjs_browser_context_open_context_menu, 4);
        export_fn!("closeContextMenu", qjs_browser_context_close_context_menu, 1);
        export_fn!("isContextMenuOpen", qjs_browser_context_is_context_menu_open, 1);
        export_fn!("getContextMenuX", qjs_browser_context_get_context_menu_x, 1);
        export_fn!("getContextMenuY", qjs_browser_context_get_context_menu_y, 1);
        export_fn!(
            "getContextMenuTarget",
            qjs_browser_context_get_context_menu_target,
            1
        );
        export_fn!("getFocusedTarget", qjs_browser_context_get_focused_target, 1);
        export_fn!("getHoveredTarget", qjs_browser_context_get_hovered_target, 1);
        export_fn!("setClipboardText", qjs_browser_context_set_clipboard_text, 2);
        export_fn!("getClipboardText", qjs_browser_context_get_clipboard_text, 1);
        export_fn!("clearClipboard", qjs_browser_context_clear_clipboard, 1);
        export_fn!("getCursorX", qjs_browser_context_get_cursor_x, 1);
        export_fn!("getCursorY", qjs_browser_context_get_cursor_y, 1);
        export_fn!("getPointerDownSeq", qjs_browser_context_get_pointer_down_seq, 1);
        export_fn!(
            "getPointerDownButton",
            qjs_browser_context_get_pointer_down_button,
            1
        );
        0
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(qjs_browser_context_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    macro_rules! add_export {
        ($name:literal) => {{
            let k = concat!($name, "\0");
            let _ = qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char);
        }};
    }
    add_export!("setActiveCursor");
    add_export!("getActiveCursor");
    add_export!("routePointerMove");
    add_export!("routePointerDown");
    add_export!("routePointerUp");
    add_export!("openContextMenu");
    add_export!("closeContextMenu");
    add_export!("isContextMenuOpen");
    add_export!("getContextMenuX");
    add_export!("getContextMenuY");
    add_export!("getContextMenuTarget");
    add_export!("getFocusedTarget");
    add_export!("getHoveredTarget");
    add_export!("setClipboardText");
    add_export!("getClipboardText");
    add_export!("clearClipboard");
    add_export!("getCursorX");
    add_export!("getCursorY");
    add_export!("getPointerDownSeq");
    add_export!("getPointerDownButton");

    m
}