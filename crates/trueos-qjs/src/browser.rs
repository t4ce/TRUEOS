use core::ffi::{c_char, c_int};
use core::sync::atomic::{AtomicU32, Ordering};

use crate as qjs;
use crate::trueos_shims;

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

#[inline]
fn js_bool(b: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion {
            int32: if b { 1 } else { 0 },
        },
        tag: qjs::JS_TAG_BOOL,
    }
}

#[inline]
unsafe fn js_get_f64(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<f64> {
    let mut out = 0.0f64;
    let rc = unsafe { qjs::JS_ToFloat64(ctx, &mut out as *mut f64, v) };
    if rc == 0 {
        Some(out)
    } else {
        None
    }
}

#[inline]
unsafe fn js_to_len_u32(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> u32 {
    match unsafe { js_get_f64(ctx, v) } {
        Some(n) if n.is_finite() && n > 0.0 => n.min(u32::MAX as f64) as u32,
        _ => 0,
    }
}

#[inline]
unsafe fn js_value_strict_eq(a: qjs::JSValueConst, b: qjs::JSValueConst) -> bool {
    if a.tag != b.tag {
        return false;
    }
    match a.tag {
        qjs::JS_TAG_INT | qjs::JS_TAG_BOOL | qjs::JS_TAG_NULL | qjs::JS_TAG_UNDEFINED => unsafe {
            a.u.int32 == b.u.int32
        },
        qjs::JS_TAG_FLOAT64 => unsafe { a.u.float64 == b.u.float64 },
        _ => unsafe { a.u.ptr == b.u.ptr },
    }
}

pub unsafe fn ensure_global_event_target_stubs(ctx: *mut qjs::JSContext, target: qjs::JSValue) {
    static RAF_ID: AtomicU32 = AtomicU32::new(1);

    unsafe fn get_or_create_listeners_obj(
        ctx: *mut qjs::JSContext,
        target: qjs::JSValueConst,
    ) -> qjs::JSValue {
        let v = qjs::JS_GetPropertyStr(
            ctx,
            target,
            b"__trueos_listeners\0".as_ptr() as *const c_char,
        );
        if !v.is_exception() && v.tag != qjs::JS_TAG_UNDEFINED && v.tag != qjs::JS_TAG_NULL {
            return v;
        }
        qjs::js_free_value(ctx, v);
        let o = qjs::JS_NewObject(ctx);
        if o.is_exception() {
            return o;
        }
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            target,
            b"__trueos_listeners\0".as_ptr() as *const c_char,
            qjs::js_dup_value(ctx, o),
        );
        o
    }

    unsafe fn get_array_len_u32(ctx: *mut qjs::JSContext, arr: qjs::JSValueConst) -> u32 {
        let lv = qjs::JS_GetPropertyStr(ctx, arr, b"length\0".as_ptr() as *const c_char);
        let out = unsafe { js_to_len_u32(ctx, lv) };
        qjs::js_free_value(ctx, lv);
        out
    }

    unsafe extern "C" fn dom_add_event_listener(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let ty_c = qjs::js_to_cstring(ctx, args[0]);
        if ty_c.is_null() {
            return qjs::JSValue::undefined();
        }
        let listeners = get_or_create_listeners_obj(ctx, this_val);
        if listeners.is_exception() {
            qjs::JS_FreeCString(ctx, ty_c);
            return qjs::JSValue::undefined();
        }
        let arr0 = qjs::JS_GetPropertyStr(ctx, listeners, ty_c);
        let arr = if arr0.is_exception()
            || arr0.tag == qjs::JS_TAG_UNDEFINED
            || arr0.tag == qjs::JS_TAG_NULL
        {
            qjs::js_free_value(ctx, arr0);
            let a = qjs::JS_NewArray(ctx);
            let _ = qjs::JS_SetPropertyStr(ctx, listeners, ty_c, qjs::js_dup_value(ctx, a));
            a
        } else {
            arr0
        };

        let len = get_array_len_u32(ctx, arr);
        let _ = qjs::JS_SetPropertyUint32(ctx, arr, len, qjs::js_dup_value(ctx, args[1]));
        qjs::JS_FreeCString(ctx, ty_c);
        qjs::js_free_value(ctx, arr);
        qjs::js_free_value(ctx, listeners);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn dom_remove_event_listener(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let ty_c = qjs::js_to_cstring(ctx, args[0]);
        if ty_c.is_null() {
            return qjs::JSValue::undefined();
        }
        let listeners = qjs::JS_GetPropertyStr(
            ctx,
            this_val,
            b"__trueos_listeners\0".as_ptr() as *const c_char,
        );
        if listeners.is_exception()
            || listeners.tag == qjs::JS_TAG_UNDEFINED
            || listeners.tag == qjs::JS_TAG_NULL
        {
            qjs::JS_FreeCString(ctx, ty_c);
            qjs::js_free_value(ctx, listeners);
            return qjs::JSValue::undefined();
        }
        let arr = qjs::JS_GetPropertyStr(ctx, listeners, ty_c);
        if arr.is_exception() || arr.tag == qjs::JS_TAG_UNDEFINED || arr.tag == qjs::JS_TAG_NULL {
            qjs::JS_FreeCString(ctx, ty_c);
            qjs::js_free_value(ctx, arr);
            qjs::js_free_value(ctx, listeners);
            return qjs::JSValue::undefined();
        }
        let len = get_array_len_u32(ctx, arr);
        for i in 0..len {
            let cb = qjs::JS_GetPropertyUint32(ctx, arr, i);
            let same = unsafe { js_value_strict_eq(cb, args[1]) };
            qjs::js_free_value(ctx, cb);
            if same {
                let _ = qjs::JS_SetPropertyUint32(ctx, arr, i, qjs::JSValue::undefined());
                break;
            }
        }
        qjs::JS_FreeCString(ctx, ty_c);
        qjs::js_free_value(ctx, arr);
        qjs::js_free_value(ctx, listeners);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn dom_dispatch_event(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_bool(true);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let evt = args[0];
        if evt.tag != qjs::JS_TAG_OBJECT {
            return js_bool(true);
        }
        let ty_v = qjs::JS_GetPropertyStr(ctx, evt, b"type\0".as_ptr() as *const c_char);
        let ty_c = qjs::js_to_cstring(ctx, ty_v);
        qjs::js_free_value(ctx, ty_v);
        if ty_c.is_null() {
            return js_bool(true);
        }

        let listeners = qjs::JS_GetPropertyStr(
            ctx,
            this_val,
            b"__trueos_listeners\0".as_ptr() as *const c_char,
        );
        if listeners.is_exception()
            || listeners.tag == qjs::JS_TAG_UNDEFINED
            || listeners.tag == qjs::JS_TAG_NULL
        {
            qjs::JS_FreeCString(ctx, ty_c);
            qjs::js_free_value(ctx, listeners);
            return js_bool(true);
        }
        let arr = qjs::JS_GetPropertyStr(ctx, listeners, ty_c);
        qjs::JS_FreeCString(ctx, ty_c);
        if arr.is_exception() || arr.tag == qjs::JS_TAG_UNDEFINED || arr.tag == qjs::JS_TAG_NULL {
            qjs::js_free_value(ctx, arr);
            qjs::js_free_value(ctx, listeners);
            return js_bool(true);
        }

        let len = get_array_len_u32(ctx, arr);
        for i in 0..len {
            let cb = qjs::JS_GetPropertyUint32(ctx, arr, i);
            if cb.is_exception() || cb.tag == qjs::JS_TAG_UNDEFINED || cb.tag == qjs::JS_TAG_NULL {
                qjs::js_free_value(ctx, cb);
                continue;
            }
            let argv2 = [qjs::js_dup_value(ctx, evt)];
            let this_arg = qjs::js_dup_value(ctx, this_val);
            let r = qjs::JS_Call(
                ctx,
                cb,
                this_arg,
                1,
                argv2.as_ptr() as *const qjs::JSValueConst,
            );
            qjs::js_free_value(ctx, r);
            qjs::js_free_value(ctx, argv2[0]);
            qjs::js_free_value(ctx, cb);
        }
        qjs::js_free_value(ctx, arr);
        qjs::js_free_value(ctx, listeners);
        js_bool(true)
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
            let next_tick =
                qjs::JS_GetPropertyStr(ctx, proc, b"nextTick\0".as_ptr() as *const c_char);
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

    install_fn!("addEventListener", dom_add_event_listener, 3);
    install_fn!("removeEventListener", dom_remove_event_listener, 3);
    install_fn!("dispatchEvent", dom_dispatch_event, 1);
    install_fn!("requestAnimationFrame", dom_request_animation_frame, 1);
    install_fn!("cancelAnimationFrame", dom_dispatch_event, 1);
}

pub unsafe fn install_mouse_api(ctx: *mut qjs::JSContext, target: qjs::JSValue) {
    unsafe extern "C" fn mouse_poll_fn(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let mut st = trueos_shims::TrueosMouseState::default();
        let _ = unsafe { trueos_shims::trueos_cabi_mouse_poll(&mut st as *mut _) };
        let o = qjs::JS_NewObject(ctx);
        if o.is_exception() {
            return o;
        }
        let _ = qjs::JS_SetPropertyStr(ctx, o, b"x\0".as_ptr() as *const c_char, js_int32(st.x));
        let _ = qjs::JS_SetPropertyStr(ctx, o, b"y\0".as_ptr() as *const c_char, js_int32(st.y));
        let _ = qjs::JS_SetPropertyStr(ctx, o, b"dx\0".as_ptr() as *const c_char, js_int32(st.dx));
        let _ = qjs::JS_SetPropertyStr(ctx, o, b"dy\0".as_ptr() as *const c_char, js_int32(st.dy));
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            o,
            b"wheel\0".as_ptr() as *const c_char,
            js_int32(st.wheel),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            o,
            b"buttons\0".as_ptr() as *const c_char,
            qjs::JS_NewFloat64(ctx, st.buttons as f64),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            o,
            b"seq\0".as_ptr() as *const c_char,
            qjs::JS_NewFloat64(ctx, st.seq as f64),
        );
        o
    }

    unsafe extern "C" fn mouse_pump_fn(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // Pop one kernel-coalesced mouse event (motion is capped to <= 1/25ms in kernel).
        let mut st = trueos_shims::TrueosMouseState::default();
        let rc = unsafe { trueos_shims::trueos_cabi_qjs_mouse_pop(&mut st as *mut _) };
        if rc != 0 {
            return qjs::JSValue::undefined();
        }

        // Track last buttons on the window object.
        let last_btn_v = qjs::JS_GetPropertyStr(
            ctx,
            this_val,
            b"__trueos_mouse_last_buttons\0".as_ptr() as *const c_char,
        );
        let last_buttons = js_get_f64(ctx, last_btn_v).unwrap_or(0.0).max(0.0) as u32;
        qjs::js_free_value(ctx, last_btn_v);
        let cur_buttons = st.buttons as u32;
        let changed = last_buttons ^ cur_buttons;
        let any_btn_change = changed != 0;
        if any_btn_change {
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                this_val,
                b"__trueos_mouse_last_buttons\0".as_ptr() as *const c_char,
                qjs::JS_NewFloat64(ctx, cur_buttons as f64),
            );
        }

        if st.dx == 0 && st.dy == 0 && st.wheel == 0 && !any_btn_change {
            return qjs::JSValue::undefined();
        }

        // Find primary canvas if present.
        let canvas = qjs::JS_GetPropertyStr(
            ctx,
            this_val,
            b"__trueos_primary_canvas\0".as_ptr() as *const c_char,
        );

        let mk_evt = |ty: &[u8]| -> qjs::JSValue {
            let e = qjs::JS_NewObject(ctx);
            if e.is_exception() {
                return e;
            }
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                e,
                b"type\0".as_ptr() as *const c_char,
                qjs::JS_NewStringLen(ctx, ty.as_ptr() as *const c_char, ty.len()),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                e,
                b"clientX\0".as_ptr() as *const c_char,
                js_int32(st.x),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                e,
                b"clientY\0".as_ptr() as *const c_char,
                js_int32(st.y),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                e,
                b"movementX\0".as_ptr() as *const c_char,
                js_int32(st.dx),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                e,
                b"movementY\0".as_ptr() as *const c_char,
                js_int32(st.dy),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                e,
                b"buttons\0".as_ptr() as *const c_char,
                qjs::JS_NewFloat64(ctx, st.buttons as f64),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                e,
                b"deltaY\0".as_ptr() as *const c_char,
                js_int32(st.wheel),
            );
            e
        };

        let dispatch_to = |target: qjs::JSValue, evt: qjs::JSValue| {
            if target.is_exception() || target.tag != qjs::JS_TAG_OBJECT {
                return;
            }
            let disp =
                qjs::JS_GetPropertyStr(ctx, target, b"dispatchEvent\0".as_ptr() as *const c_char);
            if disp.is_exception() || disp.tag == qjs::JS_TAG_UNDEFINED {
                qjs::js_free_value(ctx, disp);
                return;
            }
            let argv2 = [qjs::js_dup_value(ctx, evt)];
            let this_arg = qjs::js_dup_value(ctx, target);
            let r = qjs::JS_Call(
                ctx,
                disp,
                this_arg,
                1,
                argv2.as_ptr() as *const qjs::JSValueConst,
            );
            qjs::js_free_value(ctx, r);
            qjs::js_free_value(ctx, argv2[0]);
            qjs::js_free_value(ctx, disp);
        };

        // Dispatch mousemove + wheel to window and canvas.
        if st.dx != 0 || st.dy != 0 {
            let evt = mk_evt(b"mousemove");
            dispatch_to(this_val, evt);
            dispatch_to(canvas, evt);
            qjs::js_free_value(ctx, evt);
        }
        if st.wheel != 0 {
            let evt = mk_evt(b"wheel");
            dispatch_to(this_val, evt);
            dispatch_to(canvas, evt);
            qjs::js_free_value(ctx, evt);
        }

        // Dispatch button transitions.
        if any_btn_change {
            // Bits: 0=left,1=right,2=middle (boot mouse).
            for (bit, button_idx) in [(0u32, 0i32), (1u32, 2i32), (2u32, 1i32)] {
                let mask = 1u32 << bit;
                if (changed & mask) == 0 {
                    continue;
                }
                let down = (cur_buttons & mask) != 0;
                let ty: &[u8] = if down {
                    &b"mousedown"[..]
                } else {
                    &b"mouseup"[..]
                };
                let evt = mk_evt(ty);
                let _ = qjs::JS_SetPropertyStr(
                    ctx,
                    evt,
                    b"button\0".as_ptr() as *const c_char,
                    js_int32(button_idx),
                );
                dispatch_to(this_val, evt);
                dispatch_to(canvas, evt);
                qjs::js_free_value(ctx, evt);

                if !down {
                    let evt2 = mk_evt(b"click");
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        evt2,
                        b"button\0".as_ptr() as *const c_char,
                        js_int32(button_idx),
                    );
                    dispatch_to(this_val, evt2);
                    dispatch_to(canvas, evt2);
                    qjs::js_free_value(ctx, evt2);
                }
            }
        }

        qjs::js_free_value(ctx, canvas);
        qjs::JSValue::undefined()
    }

    let poll = qjs::JS_NewCFunction2(
        ctx,
        Some(mouse_poll_fn),
        b"__trueos_mouse_poll\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let pump = qjs::JS_NewCFunction2(
        ctx,
        Some(mouse_pump_fn),
        b"__trueos_mouse_pump\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        target,
        b"__trueos_mouse_poll\0".as_ptr() as *const c_char,
        poll,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        target,
        b"__trueos_mouse_pump\0".as_ptr() as *const c_char,
        pump,
    );
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
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            child,
            b"parentNode\0".as_ptr() as *const c_char,
            js_null(),
        );
        child
    }

    unsafe extern "C" fn dom_contains(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_bool(false);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let needle = args[0];
        if needle.is_exception()
            || needle.tag == qjs::JS_TAG_UNDEFINED
            || needle.tag == qjs::JS_TAG_NULL
        {
            return js_bool(false);
        }

        // Fast path: `node.contains(node)` is true.
        if js_value_strict_eq(this_val, needle) {
            return js_bool(true);
        }

        // Walk parentNode chain.
        let mut cur = qjs::js_dup_value(ctx, needle);
        for _ in 0..64 {
            if cur.is_exception()
                || cur.tag == qjs::JS_TAG_UNDEFINED
                || cur.tag == qjs::JS_TAG_NULL
            {
                qjs::js_free_value(ctx, cur);
                return js_bool(false);
            }
            if js_value_strict_eq(cur, this_val) {
                qjs::js_free_value(ctx, cur);
                return js_bool(true);
            }
            let next =
                qjs::JS_GetPropertyStr(ctx, cur, b"parentNode\0".as_ptr() as *const c_char);
            qjs::js_free_value(ctx, cur);
            cur = next;
        }
        qjs::js_free_value(ctx, cur);
        js_bool(false)
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
        let attrs_obj = if attrs.is_exception()
            || attrs.tag == qjs::JS_TAG_UNDEFINED
            || attrs.tag == qjs::JS_TAG_NULL
        {
            qjs::js_free_value(ctx, attrs);
            let o = qjs::JS_NewObject(ctx);
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                this_val,
                b"__attrs\0".as_ptr() as *const c_char,
                qjs::js_dup_value(ctx, o),
            );
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
        if attrs.is_exception()
            || attrs.tag == qjs::JS_TAG_UNDEFINED
            || attrs.tag == qjs::JS_TAG_NULL
        {
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

    let _ = qjs::JS_SetPropertyStr(
        ctx,
        node,
        b"parentNode\0".as_ptr() as *const c_char,
        js_null(),
    );

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

    // Install a working EventTarget API on all DOM-like nodes.
    ensure_global_event_target_stubs(ctx, node);
    node_fn!("appendChild", dom_append_child, 1);
    node_fn!("removeChild", dom_remove_child, 1);
    node_fn!("contains", dom_contains, 1);
    node_fn!("setAttribute", dom_set_attribute, 2);
    node_fn!("getAttribute", dom_get_attribute, 1);
    node_fn!("remove", dom_noop, 0);
    node_fn!("focus", dom_noop, 0);
    node_fn!("blur", dom_noop, 0);
    node_fn!("getBoundingClientRect", dom_get_bounding_client_rect, 0);

    node
}
