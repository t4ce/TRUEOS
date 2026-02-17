use core::ffi::{c_char, c_int};
use core::sync::atomic::{AtomicU32, Ordering};

use crate as qjs;

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

#[inline]
fn js_null() -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: 0 },
        tag: qjs::JS_TAG_NULL,
    }
}

pub unsafe fn ensure_global_event_target_stubs(ctx: *mut qjs::JSContext, target: qjs::JSValue) {
    static RAF_ID: AtomicU32 = AtomicU32::new(1);

    unsafe extern "C" fn dom_noop(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn dom_dispatch_event(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue {
            u: qjs::JSValueUnion { int32: 1 },
            tag: qjs::JS_TAG_BOOL,
        }
    }

    unsafe extern "C" fn dom_request_animation_frame(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_int32(0);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let cb = qjs::js_dup_value(ctx, args[0]);

        let global = qjs::JS_GetGlobalObject(ctx);
        let proc = qjs::JS_GetPropertyStr(ctx, global, b"process\0".as_ptr() as *const c_char);
        if !proc.is_exception() && proc.tag != qjs::JS_TAG_UNDEFINED {
            let next_tick = qjs::JS_GetPropertyStr(ctx, proc, b"nextTick\0".as_ptr() as *const c_char);
            if !next_tick.is_exception() && next_tick.tag != qjs::JS_TAG_UNDEFINED {
                let argv2 = [cb];
                let _ = qjs::JS_Call(
                    ctx,
                    next_tick,
                    qjs::JSValue::undefined(),
                    1,
                    argv2.as_ptr() as *const qjs::JSValueConst,
                );
            }
            qjs::js_free_value(ctx, next_tick);
        }
        qjs::js_free_value(ctx, proc);
        qjs::js_free_value(ctx, global);
        qjs::js_free_value(ctx, cb);

        let id = RAF_ID.fetch_add(1, Ordering::Relaxed);
        js_int32(id as i32)
    }

    macro_rules! install_fn {
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
            let _ = qjs::JS_SetPropertyStr(ctx, target, k.as_ptr() as *const c_char, f);
        }};
    }

    install_fn!("addEventListener", dom_noop, 3);
    install_fn!("removeEventListener", dom_noop, 3);
    install_fn!("dispatchEvent", dom_dispatch_event, 1);
    install_fn!("requestAnimationFrame", dom_request_animation_frame, 1);
    install_fn!("cancelAnimationFrame", dom_noop, 1);
}

pub unsafe fn make_dom_like_element(ctx: *mut qjs::JSContext) -> qjs::JSValue {
    unsafe extern "C" fn dom_noop(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn dom_append_child(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let child = qjs::js_dup_value(ctx, args[0]);
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            child,
            b"parentNode\0".as_ptr() as *const c_char,
            qjs::js_dup_value(ctx, this_val),
        );
        child
    }

    unsafe extern "C" fn dom_remove_child(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let child = qjs::js_dup_value(ctx, args[0]);
        let _ = qjs::JS_SetPropertyStr(ctx, child, b"parentNode\0".as_ptr() as *const c_char, js_null());
        child
    }

    unsafe extern "C" fn dom_set_attribute(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let key = qjs::js_to_cstring(ctx, args[0]);
        if key.is_null() {
            return qjs::JSValue::undefined();
        }
        let attrs = qjs::JS_GetPropertyStr(ctx, this_val, b"__attrs\0".as_ptr() as *const c_char);
        let attrs_obj = if attrs.is_exception() || attrs.tag == qjs::JS_TAG_UNDEFINED || attrs.tag == qjs::JS_TAG_NULL {
            qjs::js_free_value(ctx, attrs);
            let o = qjs::JS_NewObject(ctx);
            let _ = qjs::JS_SetPropertyStr(ctx, this_val, b"__attrs\0".as_ptr() as *const c_char, qjs::js_dup_value(ctx, o));
            o
        } else {
            attrs
        };
        let _ = qjs::JS_SetPropertyStr(ctx, attrs_obj, key, qjs::js_dup_value(ctx, args[1]));
        qjs::JS_FreeCString(ctx, key);
        qjs::js_free_value(ctx, attrs_obj);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn dom_get_attribute(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_null();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let key = qjs::js_to_cstring(ctx, args[0]);
        if key.is_null() {
            return js_null();
        }
        let attrs = qjs::JS_GetPropertyStr(ctx, this_val, b"__attrs\0".as_ptr() as *const c_char);
        if attrs.is_exception() || attrs.tag == qjs::JS_TAG_UNDEFINED || attrs.tag == qjs::JS_TAG_NULL {
            qjs::JS_FreeCString(ctx, key);
            qjs::js_free_value(ctx, attrs);
            return js_null();
        }
        let out = qjs::JS_GetPropertyStr(ctx, attrs, key);
        qjs::JS_FreeCString(ctx, key);
        qjs::js_free_value(ctx, attrs);
        if out.is_exception() || out.tag == qjs::JS_TAG_UNDEFINED {
            qjs::js_free_value(ctx, out);
            return js_null();
        }
        out
    }

    unsafe extern "C" fn dom_get_bounding_client_rect(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let rect = qjs::JS_NewObject(ctx);
        if rect.is_exception() {
            return rect;
        }
        let width_v = qjs::JS_GetPropertyStr(ctx, this_val, b"width\0".as_ptr() as *const c_char);
        let height_v = qjs::JS_GetPropertyStr(ctx, this_val, b"height\0".as_ptr() as *const c_char);
        let mut w = 0.0f64;
        let mut h = 0.0f64;
        if !width_v.is_exception() {
            let _ = qjs::JS_ToFloat64(ctx, &mut w as *mut f64, width_v);
        }
        if !height_v.is_exception() {
            let _ = qjs::JS_ToFloat64(ctx, &mut h as *mut f64, height_v);
        }
        qjs::js_free_value(ctx, width_v);
        qjs::js_free_value(ctx, height_v);
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            rect,
            b"x\0".as_ptr() as *const c_char,
            qjs::JS_NewFloat64(ctx, 0.0),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            rect,
            b"y\0".as_ptr() as *const c_char,
            qjs::JS_NewFloat64(ctx, 0.0),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            rect,
            b"left\0".as_ptr() as *const c_char,
            qjs::JS_NewFloat64(ctx, 0.0),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            rect,
            b"top\0".as_ptr() as *const c_char,
            qjs::JS_NewFloat64(ctx, 0.0),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            rect,
            b"width\0".as_ptr() as *const c_char,
            qjs::JS_NewFloat64(ctx, w),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            rect,
            b"height\0".as_ptr() as *const c_char,
            qjs::JS_NewFloat64(ctx, h),
        );
        rect
    }

    let node = qjs::JS_NewObject(ctx);
    if node.is_exception() {
        return node;
    }

    let style = qjs::JS_NewObject(ctx);
    if !style.is_exception() {
        let _ = qjs::JS_SetPropertyStr(ctx, node, b"style\0".as_ptr() as *const c_char, style);
    } else {
        qjs::js_free_value(ctx, style);
    }

    let _ = qjs::JS_SetPropertyStr(ctx, node, b"parentNode\0".as_ptr() as *const c_char, js_null());

    macro_rules! node_fn {
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
            let _ = qjs::JS_SetPropertyStr(ctx, node, k.as_ptr() as *const c_char, f);
        }};
    }

    node_fn!("addEventListener", dom_noop, 3);
    node_fn!("removeEventListener", dom_noop, 3);
    node_fn!("appendChild", dom_append_child, 1);
    node_fn!("removeChild", dom_remove_child, 1);
    node_fn!("setAttribute", dom_set_attribute, 2);
    node_fn!("getAttribute", dom_get_attribute, 1);
    node_fn!("remove", dom_noop, 0);
    node_fn!("focus", dom_noop, 0);
    node_fn!("blur", dom_noop, 0);
    node_fn!("getBoundingClientRect", dom_get_bounding_client_rect, 0);

    node
}
