extern crate alloc;

use core::ffi::c_char;
use core::ffi::c_int;

use trueos_qjs as qjs;
use v::{vgfx, vshell};

const LED_TOOL_MAX_PAYLOAD_BYTES: usize = 2048;

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
fn js_bool(v: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion {
            int32: if v { 1 } else { 0 },
        },
        tag: qjs::JS_TAG_BOOL,
    }
}

#[inline]
unsafe fn js_to_i32(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<i32> {
    if v.tag == qjs::JS_TAG_INT || v.tag == qjs::JS_TAG_BOOL {
        return Some(v.u.int32);
    }
    let mut out = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut out as *mut f64, v) != 0 {
        return None;
    }
    Some(out as i32)
}

#[inline]
unsafe fn js_set_string_prop(ctx: *mut qjs::JSContext, obj: qjs::JSValue, key: &[u8], value: &str) {
    let js = qjs::JS_NewStringLen(ctx, value.as_ptr() as *const c_char, value.len());
    let _ = qjs::JS_SetPropertyStr(ctx, obj, key.as_ptr() as *const c_char, js);
}

unsafe extern "C" fn trueos_browser_navigate_submit_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return js_int32(0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut url_len: usize = 0;
    let url_c = qjs::JS_ToCStringLen2(ctx, &mut url_len as *mut usize, args[0], 0);
    if url_c.is_null() {
        return js_int32(0);
    }
    let url_bytes = core::slice::from_raw_parts(url_c as *const u8, url_len);
    let url = match core::str::from_utf8(url_bytes) {
        Ok(text) => text,
        Err(_) => {
            qjs::JS_FreeCString(ctx, url_c);
            return js_int32(0);
        }
    };
    let browser_instance_id = if argc >= 2 {
        js_to_i32(ctx, args[1]).unwrap_or(1).max(0) as u32
    } else {
        trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID
    };
    let op_id = crate::r::browser_net::submit_navigation(browser_instance_id, url);
    qjs::JS_FreeCString(ctx, url_c);
    js_int32(op_id as i32)
}

unsafe extern "C" fn trueos_browser_navigate_status_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let op_id = js_to_i32(ctx, args[0]).unwrap_or(0).max(0) as u32;
    let Some(status) = crate::r::browser_net::status(op_id) else {
        return js_null();
    };

    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return js_null();
    }
    let state = status.state.as_str();
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"opId\0".as_ptr() as *const c_char,
        js_int32(status.op_id as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"browserInstanceId\0".as_ptr() as *const c_char,
        js_int32(status.browser_instance_id as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"bytes\0".as_ptr() as *const c_char,
        js_int32(status.bytes.min(i32::MAX as usize) as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"delivered\0".as_ptr() as *const c_char,
        js_bool(status.delivered),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"done\0".as_ptr() as *const c_char,
        js_bool(matches!(
            status.state,
            crate::r::browser_net::BrowserNetState::Succeeded
                | crate::r::browser_net::BrowserNetState::Failed
                | crate::r::browser_net::BrowserNetState::Superseded
        )),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"loading\0".as_ptr() as *const c_char,
        js_bool(matches!(
            status.state,
            crate::r::browser_net::BrowserNetState::Queued
                | crate::r::browser_net::BrowserNetState::Loading
        )),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"failed\0".as_ptr() as *const c_char,
        js_bool(matches!(
            status.state,
            crate::r::browser_net::BrowserNetState::Failed
        )),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"superseded\0".as_ptr() as *const c_char,
        js_bool(matches!(
            status.state,
            crate::r::browser_net::BrowserNetState::Superseded
        )),
    );
    js_set_string_prop(ctx, obj, b"state\0", state);
    js_set_string_prop(ctx, obj, b"url\0", status.url.as_str());
    if let Some(error) = status.error.as_ref() {
        js_set_string_prop(ctx, obj, b"error\0", error.as_str());
    }
    obj
}

fn parse_hex_payload(s: &str, out: &mut [u8]) -> Option<usize> {
    let bytes = s.as_bytes();
    if (bytes.len() & 1) != 0 {
        return None;
    }
    let want = bytes.len() / 2;
    if want > out.len() {
        return None;
    }

    fn nybble(v: u8) -> Option<u8> {
        match v {
            b'0'..=b'9' => Some(v - b'0'),
            b'a'..=b'f' => Some(v - b'a' + 10),
            b'A'..=b'F' => Some(v - b'A' + 10),
            _ => None,
        }
    }

    let mut n = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let hi = nybble(bytes[i])?;
        let lo = nybble(bytes[i + 1])?;
        out[n] = (hi << 4) | lo;
        n += 1;
        i += 2;
    }
    Some(n)
}

fn lower_hex_string(bytes: &[u8]) -> alloc::string::String {
    let mut hex = alloc::string::String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let hi = b >> 4;
        let lo = b & 0xF;
        hex.push(if hi < 10 {
            (b'0' + hi) as char
        } else {
            (b'a' + hi - 10) as char
        });
        hex.push(if lo < 10 {
            (b'0' + lo) as char
        } else {
            (b'a' + lo - 10) as char
        });
    }
    hex
}

unsafe extern "C" fn trueos_uart1_shell_write_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return js_int32(0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0);
    if cstr.is_null() {
        return js_int32(0);
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let wrote = vshell::uart1_shell_write(bytes);
    qjs::JS_FreeCString(ctx, cstr);
    js_int32(wrote as i32)
}

unsafe extern "C" fn trueos_shell2_print_line_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return js_int32(0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0);
    if cstr.is_null() {
        return js_int32(0);
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let wrote = match core::str::from_utf8(bytes) {
        Ok(text) => {
            crate::shell2::print_shell_line(&crate::shell2::UART1_COM1_BACKEND, text);
            crate::shell2::print_shell_line(&crate::shell2::NET_TCP_SHELL_BACKEND, text);
            len
        }
        Err(_) => 0,
    };
    qjs::JS_FreeCString(ctx, cstr);
    js_int32(wrote as i32)
}

unsafe extern "C" fn trueos_shell1_submit_input_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return js_int32(0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0);
    if cstr.is_null() {
        return js_int32(0);
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let wrote = vshell::shell1_submit_input(bytes);
    qjs::JS_FreeCString(ctx, cstr);
    js_int32(wrote as i32)
}

unsafe extern "C" fn trueos_shell1_history_total_lines_js(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    js_int32(vshell::shell1_history_total_lines() as i32)
}

unsafe extern "C" fn trueos_shell1_history_text_since_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let start_line = js_to_i32(ctx, args[0]).unwrap_or(0).max(0) as usize;
    let max_lines = if argc >= 2 {
        js_to_i32(ctx, args[1]).unwrap_or(64).max(0) as usize
    } else {
        64usize
    };
    let bytes = vshell::shell1_history_text_since(start_line, max_lines)
        .map(|text| text.into_bytes())
        .unwrap_or_default();
    qjs::JS_NewStringLen(ctx, bytes.as_ptr() as *const c_char, bytes.len())
}

unsafe extern "C" fn trueos_capture_screenshot_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(data_url) = vgfx::capture_screenshot_data_url() else {
        return js_null();
    };
    let bytes = data_url.into_bytes();
    qjs::JS_NewStringLen(ctx, bytes.as_ptr() as *const c_char, bytes.len())
}

unsafe extern "C" fn trueos_cpu_profile_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(profile) = crate::cpu::CpuProfile::current() else {
        return js_null();
    };

    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return js_null();
    }

    let slot_val = js_int32(profile.slot() as i32);
    let lapic_val = js_int32(profile.lapic_id() as i32);
    let kind_val = js_int32(profile.core_kind() as i32);
    let restart_val = js_int32(if crate::cpu::can_restart_current_worker_ap_from_panic() {
        1
    } else {
        0
    });
    let kind_name = profile.core_kind_name().as_bytes();
    let kind_name_val =
        qjs::JS_NewStringLen(ctx, kind_name.as_ptr() as *const c_char, kind_name.len());

    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"slot\0".as_ptr() as *const c_char, slot_val);
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"lapic_id\0".as_ptr() as *const c_char, lapic_val);
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"core_kind\0".as_ptr() as *const c_char, kind_val);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"core_kind_name\0".as_ptr() as *const c_char,
        kind_name_val,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"panic_restartable\0".as_ptr() as *const c_char,
        restart_val,
    );

    obj
}

unsafe extern "C" fn trueos_panic_test_js(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(profile) = crate::cpu::CpuProfile::current() else {
        return js_int32(-1);
    };

    if !crate::cpu::can_restart_current_worker_ap_from_panic() {
        crate::log!(
            "PANIC PANIC PANIC: qjs panic test refused on slot={} lapic={}\n",
            profile.slot(),
            profile.lapic_id()
        );
        return js_int32(-1);
    }

    panic!(
        "PANIC PANIC PANIC: qjs requested worker panic test on slot={} lapic={}",
        profile.slot(),
        profile.lapic_id()
    );
}

unsafe extern "C" fn trueos_xhci_list_devices_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    use alloc::string::String;
    let mut json = String::from("[");
    let mut first = true;
    for cid in 0..crate::usb2::xhci::MAX_XHCI_CONTROLLERS {
        for dev in crate::usb2::list_device_summaries(cid) {
            let handle = ((cid as u32) << 24) | dev.slot_id;
            if !first {
                json.push(',');
            }
            first = false;
            json.push_str(&alloc::format!(
                r#"{{"handle":{},"controller_id":{},"slot_id":{},"port":{},"kind":"{}","vid":"{}","pid":"{}"}}"#,
                handle,
                cid,
                dev.slot_id,
                dev.port,
                dev.kind,
                dev.vid.map(|v| alloc::format!("{:04x}", v)).unwrap_or_default(),
                dev.pid.map(|v| alloc::format!("{:04x}", v)).unwrap_or_default(),
            ));
        }
    }
    json.push(']');
    qjs::JS_NewStringLen(ctx, json.as_ptr() as *const c_char, json.len())
}

unsafe extern "C" fn trueos_xhci_port_reset_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return js_int32(-1);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(cid) = js_to_i32(ctx, args[0]) else {
        return js_int32(-1);
    };
    let Some(port) = js_to_i32(ctx, args[1]) else {
        return js_int32(-1);
    };
    js_int32(crate::usb2::syscall::port_reset(
        cid as usize,
        port as usize,
    ))
}

unsafe extern "C" fn trueos_xhci_get_descriptor_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 4 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_null();
    };
    let Some(desc_type) = js_to_i32(ctx, args[1]) else {
        return js_null();
    };
    let Some(desc_index) = js_to_i32(ctx, args[2]) else {
        return js_null();
    };
    let length = js_to_i32(ctx, args[3]).unwrap_or(64);
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    let bytes = match crate::usb2::syscall::control_get_descriptor(
        cid,
        slot,
        desc_type as u8,
        desc_index as u8,
        length as u16,
        500,
    ) {
        Some(b) => b,
        None => return js_null(),
    };
    let mut hex = alloc::string::String::with_capacity(bytes.len() * 2);
    for &b in bytes.iter() {
        let hi = b >> 4;
        let lo = b & 0xF;
        hex.push(if hi < 10 {
            (b'0' + hi) as char
        } else {
            (b'a' + hi - 10) as char
        });
        hex.push(if lo < 10 {
            (b'0' + lo) as char
        } else {
            (b'a' + lo - 10) as char
        });
    }
    qjs::JS_NewStringLen(ctx, hex.as_ptr() as *const c_char, hex.len())
}

unsafe extern "C" fn trueos_xhci_read_transfer_event_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_null();
    };
    let Some(ep_target) = js_to_i32(ctx, args[1]) else {
        return js_null();
    };
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    let (cc, residual) =
        match crate::usb2::syscall::read_transfer_event(cid, slot, ep_target as u32) {
            Some(r) => r,
            None => return js_null(),
        };
    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return js_null();
    }
    let cc_val = js_int32(cc as i32);
    let residual_val = js_int32(residual as i32);
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"cc\0".as_ptr() as *const c_char, cc_val);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"residual\0".as_ptr() as *const c_char,
        residual_val,
    );
    obj
}

unsafe extern "C" fn trueos_xhci_get_hid_descriptor_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 3 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_null();
    };
    let Some(interface_number) = js_to_i32(ctx, args[1]) else {
        return js_null();
    };
    let length = js_to_i32(ctx, args[2]).unwrap_or(64);
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    let bytes = match crate::usb2::syscall::control_get_hid_descriptor(
        cid,
        slot,
        interface_number as u16,
        length as u16,
        500,
    ) {
        Some(b) => b,
        None => return js_null(),
    };
    let hex = lower_hex_string(bytes.as_slice());
    qjs::JS_NewStringLen(ctx, hex.as_ptr() as *const c_char, hex.len())
}

unsafe extern "C" fn trueos_xhci_get_hid_report_descriptor_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 3 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_null();
    };
    let Some(interface_number) = js_to_i32(ctx, args[1]) else {
        return js_null();
    };
    let length = js_to_i32(ctx, args[2]).unwrap_or(256);
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    let bytes = match crate::usb2::syscall::control_get_hid_report_descriptor(
        cid,
        slot,
        interface_number as u16,
        length as u16,
        500,
    ) {
        Some(b) => b,
        None => return js_null(),
    };
    let hex = lower_hex_string(bytes.as_slice());
    qjs::JS_NewStringLen(ctx, hex.as_ptr() as *const c_char, hex.len())
}

unsafe extern "C" fn trueos_xhci_hid_get_protocol_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return js_int32(-1);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_int32(-1);
    };
    let Some(interface_number) = js_to_i32(ctx, args[1]) else {
        return js_int32(-1);
    };
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    match crate::usb2::hid::classreq::get_protocol_slot_sync(cid, slot, interface_number as u8, 500)
    {
        Some(protocol) => js_int32(protocol as i32),
        None => js_int32(-1),
    }
}

unsafe extern "C" fn trueos_xhci_hid_set_protocol_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 3 {
        return js_int32(-1);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_int32(-1);
    };
    let Some(interface_number) = js_to_i32(ctx, args[1]) else {
        return js_int32(-1);
    };
    let Some(protocol) = js_to_i32(ctx, args[2]) else {
        return js_int32(-1);
    };
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    match crate::usb2::hid::classreq::set_protocol_slot_sync(
        cid,
        slot,
        interface_number as u8,
        protocol as u8,
        500,
    ) {
        Some(cc) => js_int32(cc as i32),
        None => js_int32(-1),
    }
}

unsafe extern "C" fn trueos_xhci_hid_get_idle_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 3 {
        return js_int32(-1);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_int32(-1);
    };
    let Some(interface_number) = js_to_i32(ctx, args[1]) else {
        return js_int32(-1);
    };
    let Some(report_id) = js_to_i32(ctx, args[2]) else {
        return js_int32(-1);
    };
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    match crate::usb2::hid::classreq::get_idle_slot_sync(
        cid,
        slot,
        interface_number as u8,
        report_id as u8,
        500,
    ) {
        Some(duration) => js_int32(duration as i32),
        None => js_int32(-1),
    }
}

unsafe extern "C" fn trueos_xhci_hid_set_idle_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 4 {
        return js_int32(-1);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_int32(-1);
    };
    let Some(interface_number) = js_to_i32(ctx, args[1]) else {
        return js_int32(-1);
    };
    let Some(report_id) = js_to_i32(ctx, args[2]) else {
        return js_int32(-1);
    };
    let Some(duration_4ms) = js_to_i32(ctx, args[3]) else {
        return js_int32(-1);
    };
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    match crate::usb2::hid::classreq::set_idle_slot_sync(
        cid,
        slot,
        interface_number as u8,
        report_id as u8,
        duration_4ms as u8,
        500,
    ) {
        Some(cc) => js_int32(cc as i32),
        None => js_int32(-1),
    }
}

unsafe extern "C" fn trueos_xhci_hid_get_report_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 5 {
        return js_null();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_null();
    };
    let Some(interface_number) = js_to_i32(ctx, args[1]) else {
        return js_null();
    };
    let Some(report_type) = js_to_i32(ctx, args[2]) else {
        return js_null();
    };
    let Some(report_id) = js_to_i32(ctx, args[3]) else {
        return js_null();
    };
    let length = js_to_i32(ctx, args[4]).unwrap_or(64).clamp(1, 256) as usize;
    let report_type = match report_type {
        1 => crate::usb2::hid::classreq::HidReportType::Input,
        2 => crate::usb2::hid::classreq::HidReportType::Output,
        3 => crate::usb2::hid::classreq::HidReportType::Feature,
        _ => return js_null(),
    };
    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    let bytes = match crate::usb2::hid::classreq::get_report_slot_sync(
        cid,
        slot,
        interface_number as u8,
        report_type,
        report_id as u8,
        length,
        800,
    ) {
        Some(bytes) => bytes,
        None => return js_null(),
    };
    let hex = lower_hex_string(bytes.as_slice());
    qjs::JS_NewStringLen(ctx, hex.as_ptr() as *const c_char, hex.len())
}

unsafe extern "C" fn trueos_xhci_hid_set_report_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 5 {
        return js_int32(-1);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);

    let Some(handle) = js_to_i32(ctx, args[0]) else {
        return js_int32(-1);
    };
    let Some(interface_number) = js_to_i32(ctx, args[1]) else {
        return js_int32(-1);
    };
    let Some(report_type) = js_to_i32(ctx, args[2]) else {
        return js_int32(-1);
    };
    let Some(report_id) = js_to_i32(ctx, args[3]) else {
        return js_int32(-1);
    };

    let mut text_len: usize = 0;
    let text_ptr = qjs::JS_ToCStringLen2(ctx, &mut text_len as *mut usize, args[4], 0);
    if text_ptr.is_null() {
        return js_int32(-1);
    }
    let text_bytes = core::slice::from_raw_parts(text_ptr as *const u8, text_len);
    let text = match core::str::from_utf8(text_bytes) {
        Ok(v) => v,
        Err(_) => {
            qjs::JS_FreeCString(ctx, text_ptr);
            return js_int32(-1);
        }
    };

    let mut payload = [0u8; 256];
    let payload_len = match parse_hex_payload(text, &mut payload) {
        Some(n) => n,
        None => {
            qjs::JS_FreeCString(ctx, text_ptr);
            return js_int32(-1);
        }
    };
    qjs::JS_FreeCString(ctx, text_ptr);

    let cid = ((handle as u32) >> 24) as usize;
    let slot = (handle as u32) & 0xFF_FFFF;
    let report_type = match report_type {
        1 => crate::usb2::hid::classreq::HidReportType::Input,
        2 => crate::usb2::hid::classreq::HidReportType::Output,
        3 => crate::usb2::hid::classreq::HidReportType::Feature,
        _ => return js_int32(-1),
    };
    let rc = crate::usb2::hid::classreq::set_report_slot_sync(
        cid,
        slot,
        interface_number as u8,
        report_type,
        report_id as u8,
        &payload[..payload_len],
        500,
    );

    match rc {
        Some(cc) => js_int32(cc as i32),
        None => js_int32(-1),
    }
}

/// Called once per new JS context to install kernel-service bindings.
///
/// Registered via `trueos_qjs::host_api_hook::set_context_init_hook` at boot.
/// This is the kernel side of the QJS host API surface: uart, shell, gfx, etc.
pub unsafe fn install(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_uart1_shell_write_js),
        b"__trueosUart1ShellWrite\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosUart1ShellWrite\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_shell2_print_line_js),
        b"__trueosShell2PrintLine\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosShell2PrintLine\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_shell1_submit_input_js),
        b"__trueosShell1SubmitInput\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosShell1SubmitInput\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_shell1_history_total_lines_js),
        b"__trueosShell1HistoryTotalLines\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosShell1HistoryTotalLines\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_shell1_history_text_since_js),
        b"__trueosShell1HistoryTextSince\0".as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosShell1HistoryTextSince\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_capture_screenshot_js),
        b"__trueosCaptureScreenshot\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosCaptureScreenshot\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_cpu_profile_js),
        b"__trueosCpuProfile\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosCpuProfile\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_browser_navigate_submit_js),
        b"__trueosBrowserNavigateSubmit\0".as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosBrowserNavigateSubmit\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_browser_navigate_status_js),
        b"__trueosBrowserNavigateStatus\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosBrowserNavigateStatus\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_panic_test_js),
        b"__trueosPanicTest\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosPanicTest\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_list_devices_js),
        b"__trueosXhciListDevices\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciListDevices\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_port_reset_js),
        b"__trueosXhciPortReset\0".as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciPortReset\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_get_descriptor_js),
        b"__trueosXhciGetDescriptor\0".as_ptr() as *const c_char,
        4,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciGetDescriptor\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_read_transfer_event_js),
        b"__trueosXhciReadTransferEvent\0".as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciReadTransferEvent\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_get_hid_descriptor_js),
        b"__trueosXhciGetHidDescriptor\0".as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciGetHidDescriptor\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_get_hid_report_descriptor_js),
        b"__trueosXhciGetHidReportDescriptor\0".as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciGetHidReportDescriptor\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_hid_get_protocol_js),
        b"__trueosXhciHidGetProtocol\0".as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciHidGetProtocol\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_hid_set_protocol_js),
        b"__trueosXhciHidSetProtocol\0".as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciHidSetProtocol\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_hid_get_idle_js),
        b"__trueosXhciHidGetIdle\0".as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciHidGetIdle\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_hid_set_idle_js),
        b"__trueosXhciHidSetIdle\0".as_ptr() as *const c_char,
        4,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciHidSetIdle\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_hid_get_report_js),
        b"__trueosXhciHidGetReport\0".as_ptr() as *const c_char,
        5,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciHidGetReport\0".as_ptr() as *const c_char,
        f,
    );

    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(trueos_xhci_hid_set_report_js),
        b"__trueosXhciHidSetReport\0".as_ptr() as *const c_char,
        5,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosXhciHidSetReport\0".as_ptr() as *const c_char,
        f,
    );

    qjs::js_free_value(ctx, global);

    install_shell1_runtime(ctx);
}

unsafe fn install_shell1_runtime(ctx: *mut qjs::JSContext) {
    let Some(json) = vshell::shell_command_registry_json() else {
        return;
    };

    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }
    let registry_json = qjs::JS_NewStringLen(ctx, json.as_ptr() as *const c_char, json.len());
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosShell1CommandRegistryJson\0".as_ptr() as *const c_char,
        registry_json,
    );
    qjs::js_free_value(ctx, global);

    let shim_src = br#"
(function (G) {
    if (!G) return;
    const raw = G.__trueosShell1CommandRegistryJson;
    const parsed = typeof raw === 'string' && raw ? JSON.parse(raw) : [];
    const commands = Array.isArray(parsed) ? parsed.map((entry) => {
        const args = Array.isArray(entry && entry.args)
            ? entry.args.map((arg) => Object.freeze({
                name: String(arg && arg.name ? arg.name : ''),
                type: String(arg && arg.type ? arg.type : 'str'),
                required: !!(arg && arg.required),
            }))
            : [];
        return Object.freeze({
            command: String(entry && entry.command ? entry.command : ''),
            args: Object.freeze(args),
        });
    }) : [];
    G.__trueosShell1Runtime = Object.freeze({
        commands: Object.freeze(commands),
        historyTotalLines: () => (typeof G.__trueosShell1HistoryTotalLines === 'function'
            ? Number(G.__trueosShell1HistoryTotalLines()) || 0
            : 0),
        historyTextSince: (startLine, maxLines) => {
            if (typeof G.__trueosShell1HistoryTextSince !== 'function') {
                return '';
            }
            return String(G.__trueosShell1HistoryTextSince(
                Number(startLine) || 0,
                Number(maxLines) || 64,
            ) || '');
        },
    });
})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

    let shim = qjs::js_eval_bytes(
        ctx,
        shim_src,
        b"<node-shell1-runtime-shim>\0".as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );
    if shim.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "node shell1 runtime shim");
    }
    qjs::js_free_value(ctx, shim);
}
