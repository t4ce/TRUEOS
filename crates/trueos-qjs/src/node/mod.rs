#![cfg(feature = "trueos")]

use core::ffi::{c_char, c_int, c_void};

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

unsafe extern "C" fn trueos_node_module_normalize(
    ctx: *mut qjs::JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    _opaque: *mut c_void,
) -> *mut c_char {
    // Delegate to the shared TRUEOS normalizer in Node mode.
    qjs::trueos_module_loader::normalize_with_mode(
        ctx,
        module_base_name,
        module_name,
        qjs::trueos_module_loader::NormalizeMode::Node,
    )
}

/// Install the TRUEOS module loader with Node-ish specifier resolution.
///
/// This composes the existing TRUEOS loader (`trueos_modules::trueos_module_loader`) but
/// upgrades normalization rules:
/// - Some Node builtins are provided natively (e.g. `process`, `path`).
/// - Other common Node builtins (e.g. `events`, `util`, ...) are routed to pinned polyfill
///   packages on esm.sh (since esm.sh does not serve `node:*` specifiers directly).
/// - Unknown `node:*` specifiers are routed through esm.sh by stripping the `node:` prefix.
pub unsafe fn install(rt: *mut qjs::JSRuntime) {
    if rt.is_null() {
        return;
    }

    qjs::JS_SetModuleLoaderFunc(
        rt,
        Some(trueos_node_module_normalize),
        Some(qjs::trueos_module_loader::trueos_module_loader),
        core::ptr::null_mut(),
    );
}

/// Convenience wrapper: Node mode currently reuses the same globals as the base loader.
pub unsafe fn install_globals(ctx: *mut qjs::JSContext) {
    ensure_global_console(ctx);
    ensure_global_timers(ctx);
    ensure_global_intl(ctx);
}

unsafe fn ensure_global_timers(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    qjs::timers::install_globals(ctx, global);
    qjs::js_free_value(ctx, global);
}

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

unsafe fn log_js_args(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    prefix: &str,
) {
    log_str(prefix);
    if !argv.is_null() && argc > 0 {
        let args = core::slice::from_raw_parts(argv, argc as usize);
        for (idx, arg) in args.iter().enumerate() {
            if idx > 0 {
                log_str(" ");
            }
            let mut len: usize = 0;
            let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, *arg, 0);
            if cstr.is_null() {
                log_str("<toString failed>");
                continue;
            }
            let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
            log_bytes(bytes);
            qjs::JS_FreeCString(ctx, cstr);
        }
    }
    log_str("\n");
}

unsafe extern "C" fn console_log(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.log: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_info(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.info: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_debug(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.debug: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_warn(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.warn: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_error(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.error: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_trace(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.trace: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_assert(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut cond = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut cond as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    if cond == 0.0 {
        let rest_ptr = if argc > 1 {
            unsafe { argv.add(1) }
        } else {
            core::ptr::null()
        };
        let rest_argc = if argc > 1 { argc - 1 } else { 0 };
        log_js_args(ctx, rest_argc, rest_ptr, "console.assert: ");
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_time(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.time: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_time_end(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.timeEnd: ");
    qjs::JSValue::undefined()
}

unsafe fn ensure_global_console(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    let existing = qjs::JS_GetPropertyStr(ctx, global, b"console\0".as_ptr() as *const c_char);
    let console = if existing.is_exception()
        || existing.tag == qjs::JS_TAG_UNDEFINED
        || existing.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, existing);
        qjs::JS_NewObject(ctx)
    } else {
        existing
    };
    if console.is_exception() {
        qjs::js_free_value(ctx, console);
        qjs::js_free_value(ctx, global);
        return;
    }

    macro_rules! set_console_fn {
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
            let _ = qjs::JS_SetPropertyStr(ctx, console, k.as_ptr() as *const c_char, f);
        }};
    }

    set_console_fn!("log", console_log, 1);
    set_console_fn!("info", console_info, 1);
    set_console_fn!("debug", console_debug, 1);
    set_console_fn!("warn", console_warn, 1);
    set_console_fn!("error", console_error, 1);
    set_console_fn!("trace", console_trace, 1);
    set_console_fn!("assert", console_assert, 1);
    set_console_fn!("time", console_time, 1);
    set_console_fn!("timeEnd", console_time_end, 1);

    let _ = qjs::JS_SetPropertyStr(ctx, global, b"console\0".as_ptr() as *const c_char, console);
    qjs::js_free_value(ctx, global);
}

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

unsafe fn locale_profile_from_arg0(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> &'static trueos_weather::lang::IntlLocaleProfile {
    if argv.is_null() || argc <= 0 {
        return trueos_weather::lang::intl_locale_profile(
            trueos_weather::lang::DEFAULT_INTL_LOCALE,
        );
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0);
    if cstr.is_null() {
        return trueos_weather::lang::intl_locale_profile(
            trueos_weather::lang::DEFAULT_INTL_LOCALE,
        );
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let profile = match core::str::from_utf8(bytes) {
        Ok(s) => trueos_weather::lang::intl_locale_profile(s),
        Err(_) => {
            trueos_weather::lang::intl_locale_profile(trueos_weather::lang::DEFAULT_INTL_LOCALE)
        }
    };
    qjs::JS_FreeCString(ctx, cstr);
    profile
}

unsafe fn set_str_prop(ctx: *mut qjs::JSContext, obj: qjs::JSValue, key: &[u8], val: &str) {
    let js = qjs::JS_NewStringLen(ctx, val.as_ptr() as *const c_char, val.len());
    let _ = qjs::JS_SetPropertyStr(ctx, obj, key.as_ptr() as *const c_char, js);
}

unsafe fn set_char_prop(ctx: *mut qjs::JSContext, obj: qjs::JSValue, key: &[u8], val: char) {
    let mut buf = [0u8; 4];
    let s = val.encode_utf8(&mut buf);
    set_str_prop(ctx, obj, key, s);
}

unsafe fn make_resolved_options(
    ctx: *mut qjs::JSContext,
    profile: &trueos_weather::lang::IntlLocaleProfile,
    kind: &str,
) -> qjs::JSValue {
    let out = qjs::JS_NewObject(ctx);
    if out.is_exception() {
        return out;
    }
    set_str_prop(ctx, out, b"locale\0", profile.code);
    set_str_prop(ctx, out, b"kind\0", kind);
    set_str_prop(ctx, out, b"numberingSystem\0", "latn");
    set_str_prop(ctx, out, b"calendar\0", "gregory");
    set_char_prop(ctx, out, b"decimalSeparator\0", profile.decimal_separator);
    set_char_prop(ctx, out, b"groupingSeparator\0", profile.grouping_separator);
    set_char_prop(ctx, out, b"minusSign\0", profile.minus_sign);
    set_char_prop(ctx, out, b"percentSign\0", profile.percent_sign);
    set_str_prop(ctx, out, b"datePattern\0", profile.date_pattern);
    set_str_prop(ctx, out, b"timePattern\0", profile.time_pattern);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        out,
        b"firstDayOfWeek\0".as_ptr() as *const c_char,
        js_int32(profile.first_day_of_week as i32),
    );
    out
}

unsafe extern "C" fn intl_resolved_options(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let ro = qjs::JS_GetPropertyStr(
        ctx,
        this_val,
        b"__resolvedOptions\0".as_ptr() as *const c_char,
    );
    if ro.is_exception() || ro.tag == qjs::JS_TAG_UNDEFINED || ro.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, ro);
        return qjs::JS_NewObject(ctx);
    }
    qjs::js_dup_value(ctx, ro)
}

unsafe extern "C" fn intl_format(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0);
    if cstr.is_null() {
        return qjs::JSValue::exception();
    }
    let out = qjs::JS_NewStringLen(ctx, cstr, len);
    qjs::JS_FreeCString(ctx, cstr);
    out
}

unsafe fn make_formatter_object(
    ctx: *mut qjs::JSContext,
    profile: &trueos_weather::lang::IntlLocaleProfile,
    kind: &str,
) -> qjs::JSValue {
    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return obj;
    }
    let ro = make_resolved_options(ctx, profile, kind);
    let format_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_format),
        b"format\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let ro_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_resolved_options),
        b"resolvedOptions\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"format\0".as_ptr() as *const c_char, format_fn);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"__resolvedOptions\0".as_ptr() as *const c_char,
        ro,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"resolvedOptions\0".as_ptr() as *const c_char,
        ro_fn,
    );
    obj
}

unsafe extern "C" fn intl_number_ctor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    make_formatter_object(ctx, profile, "number")
}

unsafe extern "C" fn intl_datetime_ctor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    make_formatter_object(ctx, profile, "dateTime")
}

unsafe extern "C" fn intl_simple_ctor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    make_formatter_object(ctx, profile, "generic")
}

unsafe extern "C" fn intl_get_canonical_locales(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let out = qjs::JS_NewArray(ctx);
    if out.is_exception() {
        return out;
    }
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    let locale = qjs::JS_NewStringLen(
        ctx,
        profile.code.as_ptr() as *const c_char,
        profile.code.len(),
    );
    let _ = qjs::JS_SetPropertyUint32(ctx, out, 0, locale);
    out
}

unsafe extern "C" fn intl_locale_ctor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return obj;
    }
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    let val = qjs::JS_NewStringLen(
        ctx,
        profile.code.as_ptr() as *const c_char,
        profile.code.len(),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"baseName\0".as_ptr() as *const c_char, val);
    obj
}

unsafe fn ensure_global_intl(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }
    let key = b"Intl\0";
    let existing = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    let needs_install = existing.is_exception()
        || existing.tag == qjs::JS_TAG_UNDEFINED
        || existing.tag == qjs::JS_TAG_NULL;
    qjs::js_free_value(ctx, existing);
    if !needs_install {
        qjs::js_free_value(ctx, global);
        return;
    }

    let intl = qjs::JS_NewObject(ctx);
    if intl.is_exception() {
        qjs::js_free_value(ctx, global);
        return;
    }

    let number_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_number_ctor),
        b"NumberFormat\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let dt_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_datetime_ctor),
        b"DateTimeFormat\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let collator_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"Collator\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let plural_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"PluralRules\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let rtf_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"RelativeTimeFormat\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let list_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"ListFormat\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let display_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"DisplayNames\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let locale_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_locale_ctor),
        b"Locale\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let get_can = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_get_canonical_locales),
        b"getCanonicalLocales\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );

    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"NumberFormat\0".as_ptr() as *const c_char,
        number_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"DateTimeFormat\0".as_ptr() as *const c_char,
        dt_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"Collator\0".as_ptr() as *const c_char,
        collator_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"PluralRules\0".as_ptr() as *const c_char,
        plural_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"RelativeTimeFormat\0".as_ptr() as *const c_char,
        rtf_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"ListFormat\0".as_ptr() as *const c_char,
        list_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"DisplayNames\0".as_ptr() as *const c_char,
        display_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"Locale\0".as_ptr() as *const c_char,
        locale_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"getCanonicalLocales\0".as_ptr() as *const c_char,
        get_can,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, intl);
    qjs::js_free_value(ctx, global);
}
