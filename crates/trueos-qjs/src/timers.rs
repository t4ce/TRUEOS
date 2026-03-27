#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::ffi::c_int;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant};
use spin::Mutex;

use crate as qjs;

#[derive(Clone)]
struct TimerEntry {
    id: u32,
    due: Instant,
    interval: EmbassyDuration,
    repeating: bool,
    cb: qjs::JSValue,
    args: Vec<qjs::JSValue>,
}

// QuickJS execution is expected to remain within a single owning task, but we
// store timers in a static registry like async_ops/workers do.
unsafe impl Send for TimerEntry {}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static TIMERS: Mutex<BTreeMap<usize, Vec<TimerEntry>>> = Mutex::new(BTreeMap::new());

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

#[inline]
unsafe fn js_get_f64(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<f64> {
    let mut out = 0.0f64;
    let rc = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, v);
    if rc == 0 { Some(out) } else { None }
}

#[inline]
unsafe fn js_to_delay_ms(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> u64 {
    match js_get_f64(ctx, v) {
        Some(n) if n.is_finite() && n > 0.0 => n.min(u32::MAX as f64) as u64,
        _ => 0,
    }
}

fn create_timer(
    ctx: *mut qjs::JSContext,
    cb: qjs::JSValueConst,
    delay_ms: u64,
    repeating: bool,
    args: Vec<qjs::JSValueConst>,
) -> u32 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let entry = TimerEntry {
        id,
        due: Instant::now() + EmbassyDuration::from_millis(delay_ms),
        interval: EmbassyDuration::from_millis(delay_ms.max(1)),
        repeating,
        cb: unsafe { qjs::js_dup_value(ctx, cb) },
        args: args
            .into_iter()
            .map(|v| unsafe { qjs::js_dup_value(ctx, v) })
            .collect(),
    };
    TIMERS.lock().entry(ctx as usize).or_default().push(entry);
    id
}

fn cancel_timer(ctx: *mut qjs::JSContext, id: u32) {
    let mut timers = TIMERS.lock();
    if let Some(entries) = timers.get_mut(&(ctx as usize)) {
        let mut i = 0usize;
        while i < entries.len() {
            if entries[i].id == id {
                let t = entries.remove(i);
                unsafe {
                    qjs::js_free_value(ctx, t.cb);
                    for a in t.args {
                        qjs::js_free_value(ctx, a);
                    }
                }
                break;
            }
            i += 1;
        }
    }
}

/// Pump due timers (setTimeout/setInterval) for the given context.
///
/// Returns `true` if it fired at least one callback.
pub unsafe fn pump(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }

    let now = Instant::now();
    let mut fired_any = false;

    // Fire all due timers for this context. To avoid holding the lock across JS
    // calls, we first extract a list of timer IDs to fire.
    let mut due_ids: Vec<u32> = Vec::new();
    {
        let timers = TIMERS.lock();
        if let Some(entries) = timers.get(&(ctx as usize)) {
            for t in entries.iter() {
                if now >= t.due {
                    due_ids.push(t.id);
                }
            }
        }
    }

    for id in due_ids {
        // Re-check and take/update under lock.
        let (cb, args, repeating, interval) = {
            let mut timers = TIMERS.lock();
            let Some(entries) = timers.get_mut(&(ctx as usize)) else {
                continue;
            };
            let pos = entries.iter().position(|t| t.id == id);
            let Some(pos) = pos else {
                continue;
            };

            // Skip if it was rescheduled into the future.
            if now < entries[pos].due {
                continue;
            }

            // Snapshot call data.
            let cb = qjs::js_dup_value(ctx, entries[pos].cb);
            let args = entries[pos]
                .args
                .iter()
                .map(|v| qjs::js_dup_value(ctx, *v))
                .collect::<Vec<_>>();
            let repeating = entries[pos].repeating;
            let interval = entries[pos].interval;

            if repeating {
                entries[pos].due = Instant::now() + interval;
            } else {
                let t = entries.remove(pos);
                qjs::js_free_value(ctx, t.cb);
                for a in t.args {
                    qjs::js_free_value(ctx, a);
                }
            }

            (cb, args, repeating, interval)
        };

        // Call outside the lock.
        let argc = args.len() as c_int;
        let r = qjs::JS_Call(
            ctx,
            cb,
            qjs::JSValue::undefined(),
            argc,
            args.as_ptr() as *const qjs::JSValueConst,
        );
        if r.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "timer callback");
        }
        qjs::js_free_value(ctx, r);
        qjs::js_free_value(ctx, cb);
        for a in args {
            qjs::js_free_value(ctx, a);
        }

        // Minor hint to the optimizer: keep vars used.
        let _ = (repeating, interval);

        fired_any = true;
    }

    fired_any
}

pub unsafe fn install_globals(ctx: *mut qjs::JSContext, target: qjs::JSValue) {
    unsafe extern "C" fn set_timeout(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_int32(0);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let cb = args[0];
        if cb.tag == qjs::JS_TAG_UNDEFINED || cb.tag == qjs::JS_TAG_NULL {
            return js_int32(0);
        }
        let delay_ms = if args.len() >= 2 {
            unsafe { js_to_delay_ms(ctx, args[1]) }
        } else {
            0
        };
        let extra = if args.len() > 2 {
            args[2..].to_vec()
        } else {
            Vec::new()
        };
        let id = create_timer(ctx, cb, delay_ms, false, extra);
        js_int32(id as i32)
    }

    unsafe extern "C" fn clear_timeout(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let id = unsafe { js_get_f64(ctx, args[0]).unwrap_or(0.0) };
        if id.is_finite() && id > 0.0 {
            cancel_timer(ctx, id as u32);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn set_interval(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_int32(0);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let cb = args[0];
        if cb.tag == qjs::JS_TAG_UNDEFINED || cb.tag == qjs::JS_TAG_NULL {
            return js_int32(0);
        }
        let delay_ms = if args.len() >= 2 {
            unsafe { js_to_delay_ms(ctx, args[1]) }
        } else {
            0
        };
        let extra = if args.len() > 2 {
            args[2..].to_vec()
        } else {
            Vec::new()
        };
        let id = create_timer(ctx, cb, delay_ms, true, extra);
        js_int32(id as i32)
    }

    unsafe extern "C" fn clear_interval(
        ctx: *mut qjs::JSContext,
        this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // clearInterval is identical to clearTimeout.
        clear_timeout(ctx, this_val, argc, argv)
    }

    macro_rules! install_fn {
        ($name:literal, $func:expr, $argc:expr) => {{
            let k = concat!($name, "\0");
            let _ = qjs::jsbind::install_fn(ctx, target, k.as_bytes(), $argc, Some($func));
        }};
    }

    install_fn!("setTimeout", set_timeout, 2);
    install_fn!("clearTimeout", clear_timeout, 1);
    install_fn!("setInterval", set_interval, 2);
    install_fn!("clearInterval", clear_interval, 1);
}

pub unsafe fn drain_all_for_context(ctx: *mut qjs::JSContext) {
    let entries = TIMERS.lock().remove(&(ctx as usize)).unwrap_or_default();
    for t in entries {
        qjs::js_free_value(ctx, t.cb);
        for a in t.args {
            qjs::js_free_value(ctx, a);
        }
    }
}

pub fn has_pending(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }
    TIMERS
        .lock()
        .get(&(ctx as usize))
        .is_some_and(|entries| !entries.is_empty())
}
