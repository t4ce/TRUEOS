use alloc::vec::Vec;
use core::ffi::c_char;

use embassy_time::{Duration as EmbassyDuration, Timer};
use trueos_qjs as qjs;

const UI2_OPENTYPE_DEMO_TEX_ID: u32 = 4_706;
const UI2_OPENTYPE_DEMO_WINDOW_X: f32 = 640.0;
const UI2_OPENTYPE_DEMO_WINDOW_Y: f32 = 140.0;
const UI2_OPENTYPE_DEMO_WINDOW_Z: i16 = 32;
const OPENTYPE_DEMO_GLOBAL_PROP: &[u8] = b"__trueosOpentDemo\0";
const OPENTYPE_DEMO_FONT_BYTES_PROP: &[u8] = b"__trueosOpentDemoFontBytes\0";
const OPENTYPE_DEMO_FONT_ARCHIVE: &[u8] = include_bytes!("../luci.7z");

unsafe fn js_prop_u32(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &[u8]) -> Option<u32> {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    let mut out: f64 = 0.0;
    let ok =
        qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite() && out >= 0.0;
    qjs::js_free_value(ctx, value);
    if ok {
        Some(out as u32)
    } else {
        None
    }
}

unsafe fn js_value_to_u8_vec(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValueConst,
) -> Option<Vec<u8>> {
    let mut byte_off: usize = 0;
    let mut byte_len: usize = 0;
    let mut bytes_per_element: usize = 0;
    let ab = qjs::JS_GetTypedArrayBuffer(
        ctx,
        value,
        &mut byte_off as *mut usize,
        &mut byte_len as *mut usize,
        &mut bytes_per_element as *mut usize,
    );

    if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED && ab.tag != qjs::JS_TAG_NULL {
        let mut buf_len: usize = 0;
        let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab);
        let out = if ptr.is_null() {
            None
        } else {
            let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
            Some(alloc::vec::Vec::from(core::slice::from_raw_parts(
                ptr.add(byte_off) as *const u8,
                usable,
            )))
        };
        qjs::js_free_value(ctx, ab);
        return out;
    }
    if !ab.is_exception() {
        qjs::js_free_value(ctx, ab);
    }

    let mut buf_len: usize = 0;
    let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, value);
    if ptr.is_null() {
        return None;
    }
    Some(alloc::vec::Vec::from(core::slice::from_raw_parts(
        ptr as *const u8,
        buf_len,
    )))
}

unsafe fn js_prop_u8_vec(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
) -> Option<Vec<u8>> {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    let out = js_value_to_u8_vec(ctx, value);
    qjs::js_free_value(ctx, value);
    out
}

unsafe fn js_prop_string(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
) -> Option<alloc::string::String> {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    let mut len: usize = 0;
    let c = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, value, 0);
    let out = if c.is_null() || len == 0 {
        None
    } else {
        let bytes = core::slice::from_raw_parts(c as *const u8, len);
        core::str::from_utf8(bytes)
            .ok()
            .map(alloc::string::String::from)
    };
    if !c.is_null() {
        qjs::JS_FreeCString(ctx, c);
    }
    qjs::js_free_value(ctx, value);
    out
}

unsafe fn install_demo_font_bytes(ctx: *mut qjs::JSContext, bytes: &[u8]) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return false;
    }

    let ab = qjs::JS_NewArrayBufferCopy(ctx, bytes.as_ptr(), bytes.len());
    if ab.is_exception() {
        qjs::js_free_value(ctx, global);
        return false;
    }

    let rc = qjs::JS_SetPropertyStr(
        ctx,
        global,
        OPENTYPE_DEMO_FONT_BYTES_PROP.as_ptr() as *const c_char,
        ab,
    );
    qjs::js_free_value(ctx, global);
    rc == 1
}

async fn build_demo_surface_rgba() -> Option<(u32, u32, Vec<u8>)> {
    crate::log!(
        "ui2-opentype-demo: begin archive_bytes={}\n",
        OPENTYPE_DEMO_FONT_ARCHIVE.len()
    );
    let font_bytes = match crate::z7::extract_single_file_to_vec(OPENTYPE_DEMO_FONT_ARCHIVE) {
        Ok(bytes) => bytes,
        Err(err) => {
            crate::log!("ui2-opentype-demo: embedded font decode failed {:?}\n", err);
            return None;
        }
    };
    crate::log!(
        "ui2-opentype-demo: font unpack ok bytes={}\n",
        font_bytes.len()
    );

    unsafe {
        crate::log!("ui2-opentype-demo: creating JS runtime\n");
        let Some(vm) = qjs::vm::QjsVm::new_node_with_profile(qjs::node::RuntimeProfile::Browser)
        else {
            crate::log!("ui2-opentype-demo: JS runtime init failed\n");
            return None;
        };
        let ctx = vm.ctx_ptr();
        let rt = vm.rt_ptr();

        crate::log!("ui2-opentype-demo: injecting font bytes into JS\n");
        if !install_demo_font_bytes(ctx, font_bytes.as_slice()) {
            crate::log!("ui2-opentype-demo: failed to inject font bytes into JS\n");
            let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;
            return None;
        }

        let filename = b"<ui2-opentype-demo>\0";
        let src = br#"
import { renderTextDemoAsync } from '/qjs/font/opent.mjs';
globalThis.__trueosOpentDemo = null;
globalThis.__trueosOpentDemoError = "";
(async () => {
  try {
    globalThis.__trueosOpentDemo = await renderTextDemoAsync();
  } catch (err) {
    globalThis.__trueosOpentDemoError = String(err && err.stack ? err.stack : err);
  }
})();
"#;
        crate::log!("ui2-opentype-demo: evaluating bootstrap module\n");
        let boot = qjs::js_eval_bytes(
            ctx,
            src,
            filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
        );
        if boot.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "ui2-opentype-demo init");
            qjs::js_free_value(ctx, boot);
            let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;
            return None;
        }
        qjs::js_free_value(ctx, boot);
        crate::log!("ui2-opentype-demo: bootstrap submitted, waiting for JS render\n");

        let mut ready = false;
        for attempt in 0..128 {
            if !qjs::vm::pump_runtime_once(rt, ctx, "ui2-opentype-demo") {
                let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;
                return None;
            }
            let global = qjs::JS_GetGlobalObject(ctx);
            let demo = qjs::JS_GetPropertyStr(
                ctx,
                global,
                OPENTYPE_DEMO_GLOBAL_PROP.as_ptr() as *const c_char,
            );
            let is_ready = !demo.is_exception()
                && demo.tag != qjs::JS_TAG_UNDEFINED
                && demo.tag != qjs::JS_TAG_NULL;
            qjs::js_free_value(ctx, demo);

            let err = qjs::JS_GetPropertyStr(
                ctx,
                global,
                b"__trueosOpentDemoError\0".as_ptr() as *const c_char,
            );
            let mut err_len: usize = 0;
            let err_c = qjs::JS_ToCStringLen2(ctx, &mut err_len as *mut usize, err, 0);
            let has_err = !err_c.is_null() && err_len > 0;
            if has_err {
                let err_str = core::slice::from_raw_parts(err_c as *const u8, err_len);
                if let Ok(msg) = core::str::from_utf8(err_str) {
                    crate::log!("ui2-opentype-demo: JS error {}\n", msg);
                } else {
                    crate::log!("ui2-opentype-demo: JS error (non-utf8)\n");
                }
            }
            if !err_c.is_null() {
                qjs::JS_FreeCString(ctx, err_c);
            }
            qjs::js_free_value(ctx, err);
            qjs::js_free_value(ctx, global);
            if has_err {
                let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;
                return None;
            }
            if is_ready {
                ready = true;
                crate::log!(
                    "ui2-opentype-demo: JS render ready after {} polls\n",
                    attempt + 1
                );
                break;
            }
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }

        if !ready {
            crate::log!("ui2-opentype-demo: timed out waiting for JS render\n");
            let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;
            return None;
        }

        let global = qjs::JS_GetGlobalObject(ctx);
        let demo = qjs::JS_GetPropertyStr(
            ctx,
            global,
            OPENTYPE_DEMO_GLOBAL_PROP.as_ptr() as *const c_char,
        );

        if demo.is_exception() || demo.tag == qjs::JS_TAG_UNDEFINED || demo.tag == qjs::JS_TAG_NULL
        {
            if demo.is_exception() {
                qjs::qjs_diag::dump_last_exception(ctx, "ui2-opentype-demo result");
            } else {
                crate::log!("ui2-opentype-demo: missing JS result object\n");
            }
            qjs::js_free_value(ctx, demo);
            qjs::js_free_value(ctx, global);
            let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;
            return None;
        }

        let width = js_prop_u32(ctx, demo, b"width\0");
        let height = js_prop_u32(ctx, demo, b"height\0");
        let rgba = js_prop_u8_vec(ctx, demo, b"rgba\0");
        let text = js_prop_string(ctx, demo, b"text\0");
        let units_per_em = js_prop_u32(ctx, demo, b"unitsPerEm\0");
        let glyph_count = js_prop_u32(ctx, demo, b"glyphCount\0");
        let ascender = js_prop_u32(ctx, demo, b"ascender\0");
        let descender = js_prop_u32(ctx, demo, b"descender\0");
        let debug_rects = js_prop_string(ctx, demo, b"debugRects\0");

        qjs::js_free_value(ctx, demo);
        qjs::js_free_value(ctx, global);
        let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;

        let (Some(width), Some(height), Some(rgba)) = (width, height, rgba) else {
            crate::log!("ui2-opentype-demo: failed to decode JS demo buffer\n");
            return None;
        };
        crate::log!(
            "ui2-opentype-demo: text={:?} unitsPerEm={:?} glyphCount={:?} asc={:?} desc={:?}\n",
            text,
            units_per_em,
            glyph_count,
            ascender,
            descender
        );
        crate::log!("ui2-opentype-demo: rects={:?}\n", debug_rects);
        Some((width, height, rgba))
    }
}

#[embassy_executor::task]
pub async fn ui2_opentype_demo_task() {
    let Some((width, height, rgba)) = build_demo_surface_rgba().await else {
        return;
    };

    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Demo OpenType.js",
        crate::r::ui2::Ui2Rect {
            x: UI2_OPENTYPE_DEMO_WINDOW_X,
            y: UI2_OPENTYPE_DEMO_WINDOW_Y,
            w: width as f32,
            h: height as f32,
        },
        UI2_OPENTYPE_DEMO_WINDOW_Z,
        255,
        UI2_OPENTYPE_DEMO_TEX_ID,
        false,
        [0xFF, 0xFF, 0xFF, 0xFF],
    ) else {
        crate::log!(
            "ui2-opentype-demo: window creation failed tex={} size={}x{}\n",
            UI2_OPENTYPE_DEMO_TEX_ID,
            width,
            height
        );
        return;
    };

    if !surface.upload_rgba(rgba.as_slice(), "ui2-opentype-demo-upload") {
        crate::log!(
            "ui2-opentype-demo: upload failed window={} tex={} size={}x{}\n",
            surface.window_id(),
            surface.tex_id(),
            width,
            height
        );
        return;
    }

    crate::log!(
        "ui2-opentype-demo: window={} tex={} size={}x{}\n",
        surface.window_id(),
        surface.tex_id(),
        width,
        height
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}
