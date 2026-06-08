use alloc::string::String;
use alloc::vec;
use core::ffi::{c_char, c_int};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};

use trueos_qjs as qjs;

use super::intel_present::{Ui3IntelPresentSummary, present_ui3_frame_to_intel_primary};
use super::pixi_host::pointer_event_kind_from_name;
use super::{
    Ui3Color, Ui3Command, Ui3NodeKind, Ui3PixiHost, Ui3Point, Ui3Rect, Ui3RenderFrame,
    Ui3TextParam, lower_ui3_frame_geometry,
};

const TASK_NAME: &str = "ui3-pixi-service";
const PIXI_HOST_PRELUDE_SOURCE: &[u8] = include_bytes!("pixi_host_prelude.js");
const PIXI_BUNDLE_SOURCE: &[u8] = include_bytes!("pixi_bundle.min.js");
const PIXI_CAPTURE_ADAPTER_SOURCE: &[u8] = include_bytes!("pixi_capture_adapter.js");
const PIXI_EMPTY_SMOKE_SOURCE: &[u8] = include_bytes!("pixi_empty_smoke.js");
const PIXI_HOST_PRELUDE_FILENAME: &[u8] = b"<ui3-pixi-host-prelude>\0";
const PIXI_BUNDLE_FILENAME: &[u8] = b"<ui3-pixi-bundle>\0";
const PIXI_CAPTURE_ADAPTER_FILENAME: &[u8] = b"<ui3-pixi-capture-adapter>\0";
const PIXI_EMPTY_SMOKE_FILENAME: &[u8] = b"<ui3-pixi-empty-smoke>\0";
const PIXI_SERVICE_PARK_MS: u64 = 1_000;
const PIXI_AUTORUN_TRUESURFER_HTML_ENABLE: bool = false;
const PIXI_AUTORUN_TRUESURFER_HTML_URL: &str = "inline://trueos/ui3-hello.html";
const PIXI_AUTORUN_TRUESURFER_HTML_SOURCE: &str = "<!doctype html><html><head><title>UI3 Hello</title></head><body><h1>Hello UI3</h1><p>Parse5 handoff smoke.</p></body></html>";

static PIXI_SERVICE_READY: AtomicBool = AtomicBool::new(false);
static PIXI_AUTORUN_TRUESURFER_HTML_STARTED: AtomicBool = AtomicBool::new(false);
static PIXI_SERVICE_PUMP_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_RENDER_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_OP_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_DRAW_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_RUNTIME: Once<Mutex<Ui3PixiServiceRuntime>> = Once::new();

struct Ui3PixiServiceRuntime {
    host: Ui3PixiHost,
    op_count: u32,
    frame_count: u32,
    last_draw_count: u32,
    last_present: Ui3IntelPresentSummary,
}

impl Ui3PixiServiceRuntime {
    fn new() -> Self {
        Self {
            host: Ui3PixiHost::new(),
            op_count: 0,
            frame_count: 0,
            last_draw_count: 0,
            last_present: Ui3IntelPresentSummary::default(),
        }
    }
}

pub fn pixi_service_ready() -> bool {
    PIXI_SERVICE_READY.load(Ordering::Acquire)
}

pub fn pixi_service_pump_count() -> u32 {
    PIXI_SERVICE_PUMP_COUNT.load(Ordering::Acquire)
}

pub fn pixi_service_render_count() -> u32 {
    PIXI_SERVICE_RENDER_COUNT.load(Ordering::Acquire)
}

pub fn pixi_service_op_count() -> u32 {
    PIXI_SERVICE_OP_COUNT.load(Ordering::Acquire)
}

pub fn pixi_service_frame_count() -> u32 {
    PIXI_SERVICE_FRAME_COUNT.load(Ordering::Acquire)
}

pub fn pixi_service_draw_count() -> u32 {
    PIXI_SERVICE_DRAW_COUNT.load(Ordering::Acquire)
}

#[embassy_executor::task]
pub async fn pixi_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    PIXI_SERVICE_READY.store(false, Ordering::Release);
    PIXI_SERVICE_PUMP_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_RENDER_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_OP_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_FRAME_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_DRAW_COUNT.store(0, Ordering::Release);
    *pixi_runtime().lock() = Ui3PixiServiceRuntime::new();
    crate::log!("ui3-pixi-service: starting qjs host on ap1\n");

    unsafe {
        let Some(vm) = qjs::vm::QjsVm::new_node_with_profile(qjs::node::RuntimeProfile::Browser)
        else {
            crate::log!("ui3-pixi-service: qjs runtime init failed\n");
            return;
        };
        let ctx = vm.ctx_ptr();

        if !eval_global_script(
            ctx,
            PIXI_HOST_PRELUDE_SOURCE,
            PIXI_HOST_PRELUDE_FILENAME,
            "ui3 pixi host prelude",
        ) {
            return;
        }

        if !eval_global_script(ctx, PIXI_BUNDLE_SOURCE, PIXI_BUNDLE_FILENAME, "ui3 pixi bundle") {
            return;
        }

        install_native_hooks(ctx);

        if !eval_global_script(
            ctx,
            PIXI_CAPTURE_ADAPTER_SOURCE,
            PIXI_CAPTURE_ADAPTER_FILENAME,
            "ui3 pixi capture adapter",
        ) {
            return;
        }

        if !eval_global_script(
            ctx,
            PIXI_EMPTY_SMOKE_SOURCE,
            PIXI_EMPTY_SMOKE_FILENAME,
            "ui3 pixi empty smoke",
        ) {
            return;
        }

        let ready = true;
        PIXI_SERVICE_READY.store(ready, Ordering::Release);
        crate::log!(
            "ui3-pixi-service: ready={} renders={} ops={} frames={} draws={} bundle_bytes={} adapter_bytes={} profile=browser\n",
            if ready { 1 } else { 0 },
            PIXI_SERVICE_RENDER_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_OP_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_FRAME_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_DRAW_COUNT.load(Ordering::Acquire),
            PIXI_BUNDLE_SOURCE.len(),
            PIXI_CAPTURE_ADAPTER_SOURCE.len()
        );

        if ready && PIXI_AUTORUN_TRUESURFER_HTML_ENABLE {
            autorun_truesurfer_html_after_ready().await;
        }

        crate::log!("ui3-pixi-service: vm parked retained=1\n");
        loop {
            if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
                crate::log!("ui3-pixi-service: stop requested; teardown begin\n");
                let drained = qjs::vm::teardown_main_context(vm.rt_ptr(), ctx, 500).await;
                crate::log!(
                    "ui3-pixi-service: teardown done drained={}; exiting\n",
                    if drained { 1 } else { 0 }
                );
                break;
            }
            Timer::after(EmbassyDuration::from_millis(PIXI_SERVICE_PARK_MS)).await;
        }
    }
}

async fn autorun_truesurfer_html_after_ready() {
    if PIXI_AUTORUN_TRUESURFER_HTML_STARTED.swap(true, Ordering::AcqRel) {
        crate::log!("ui3-pixi-service: truesurfer autorun skipped reason=already-started\n");
        return;
    }

    let html = crate::surfer::html_shack::Html::new(
        PIXI_AUTORUN_TRUESURFER_HTML_URL,
        PIXI_AUTORUN_TRUESURFER_HTML_SOURCE,
    );
    crate::log!(
        "ui3-pixi-service: truesurfer autorun submit source=inline-hello bytes={} url={}\n",
        html.html.len(),
        html.url
    );
    let ok = crate::surfer::html_shack::handoff_html_to_truesurfer(html).await;
    crate::log!("ui3-pixi-service: truesurfer autorun queued={}\n", if ok { 1 } else { 0 });
}

fn pixi_runtime() -> &'static Mutex<Ui3PixiServiceRuntime> {
    PIXI_SERVICE_RUNTIME.call_once(|| Mutex::new(Ui3PixiServiceRuntime::new()))
}

unsafe fn install_native_hooks(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }
    let _ = qjs::jsbind::install_fn(ctx, global, b"__trueosPixiOp\0", 6, Some(trueos_pixi_op));
    let _ = qjs::jsbind::install_fn(ctx, global, b"__trueosRender\0", 1, Some(trueos_render));
    qjs::js_free_value(ctx, global);
}

unsafe extern "C" fn trueos_pixi_op(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(op) = read_arg_string(ctx, argc, argv, 0) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };

    let mut runtime = pixi_runtime().lock();
    runtime.op_count = runtime.op_count.wrapping_add(1).max(1);
    PIXI_SERVICE_OP_COUNT.store(runtime.op_count, Ordering::Release);

    if runtime.op_count <= 16 {
        let id = read_arg_u32(ctx, argc, argv, 1).unwrap_or(0);
        crate::log!("ui3-pixi-service: pixi-op #{} {} id={}\n", runtime.op_count, op.as_str(), id);
    }

    match op.as_str() {
        "node" => {
            if let (Some(node), Some(kind)) =
                (read_arg_u32(ctx, argc, argv, 1), read_arg_string(ctx, argc, argv, 2))
            {
                runtime.host.declare_node(node, node_kind_from_name(&kind));
            }
        }
        _ => {
            if let Some(command) = command_from_pixi_op(ctx, argc, argv, &op)
                && let Some(frame) = runtime.host.apply(command).cloned()
            {
                lower_present_and_count(&mut runtime, &frame);
            }
        }
    }

    qjs::JS_NewFloat64(ctx, runtime.op_count as f64)
}

unsafe extern "C" fn trueos_render(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let call = PIXI_SERVICE_RENDER_COUNT
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let children = if argc > 0 && !argv.is_null() {
        read_stage_child_count(ctx, *argv)
    } else {
        0
    };
    let root = if argc > 0 && !argv.is_null() {
        read_trueos_id(ctx, *argv).unwrap_or(0)
    } else {
        0
    };
    if root != 0 {
        let mut runtime = pixi_runtime().lock();
        if let Some(frame) = runtime.host.apply(Ui3Command::Render { root }).cloned() {
            lower_present_and_count(&mut runtime, &frame);
        }
    }

    if call <= 4 {
        let present = pixi_runtime().lock().last_present;
        crate::log!(
            "ui3-pixi-service: __trueosRender call={} root={} root_children={} ops={} draws={} solid_rects={} meshes={} text={} presented={} fill_descs={} blend_descs={} rect_ms={} sprite_ms={} publish_ms={} present_ms={} total_ms={}\n",
            call,
            root,
            children,
            PIXI_SERVICE_OP_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_DRAW_COUNT.load(Ordering::Acquire),
            present.solid_rects,
            present.mesh_draws,
            present.text_runs,
            present.presented as u8,
            present.fill_descs,
            present.blend_descs,
            present.rect_ms,
            present.sprite_ms,
            present.publish_ms,
            present.present_ms,
            present.total_ms
        );
    }

    qjs::JS_NewFloat64(ctx, call as f64)
}

fn lower_present_and_count(runtime: &mut Ui3PixiServiceRuntime, frame: &Ui3RenderFrame) {
    runtime.frame_count = runtime.frame_count.wrapping_add(1).max(1);
    let geometry = lower_ui3_frame_geometry(&runtime.host, frame);
    runtime.last_draw_count = geometry.draws.len().min(u32::MAX as usize) as u32;
    runtime.last_present = present_ui3_frame_to_intel_primary(&geometry);
    PIXI_SERVICE_FRAME_COUNT.store(runtime.frame_count, Ordering::Release);
    PIXI_SERVICE_DRAW_COUNT.store(runtime.last_draw_count, Ordering::Release);
}

unsafe fn command_from_pixi_op(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    op: &str,
) -> Option<Ui3Command> {
    match op {
        "addChild" => Some(Ui3Command::AddChild {
            parent: read_arg_u32(ctx, argc, argv, 1)?,
            child: read_arg_u32(ctx, argc, argv, 2)?,
        }),
        "addChildAt" => Some(Ui3Command::AddChildAt {
            parent: read_arg_u32(ctx, argc, argv, 1)?,
            child: read_arg_u32(ctx, argc, argv, 2)?,
            index: read_arg_usize(ctx, argc, argv, 3).unwrap_or(0),
        }),
        "setChildIndex" => Some(Ui3Command::SetChildIndex {
            parent: read_arg_u32(ctx, argc, argv, 1)?,
            child: read_arg_u32(ctx, argc, argv, 2)?,
            index: read_arg_usize(ctx, argc, argv, 3).unwrap_or(0),
        }),
        "removeChild" => Some(Ui3Command::RemoveChild {
            parent: read_arg_u32(ctx, argc, argv, 1)?,
            child: read_arg_u32(ctx, argc, argv, 2)?,
        }),
        "removeFromParent" => Some(Ui3Command::RemoveFromParent {
            node: read_arg_u32(ctx, argc, argv, 1)?,
        }),
        "removeChildren" => Some(Ui3Command::RemoveChildren {
            parent: read_arg_u32(ctx, argc, argv, 1)?,
        }),
        "position" => Some(Ui3Command::SetPosition {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            position: Ui3Point {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(0.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(0.0),
            },
        }),
        "visible" => Some(Ui3Command::SetVisible {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            visible: read_arg_bool(ctx, argc, argv, 2).unwrap_or(true),
        }),
        "listen" => Some(Ui3Command::Listen {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            event: pointer_event_kind_from_name(&read_arg_string(ctx, argc, argv, 2)?),
        }),
        "removeAllListeners" => Some(Ui3Command::RemoveAllListeners {
            node: read_arg_u32(ctx, argc, argv, 1)?,
        }),
        "clear" => Some(Ui3Command::GraphicsClear {
            node: read_arg_u32(ctx, argc, argv, 1)?,
        }),
        "rect" => Some(Ui3Command::GraphicsRect {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            rect: Ui3Rect {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(0.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(0.0),
                w: read_arg_f32(ctx, argc, argv, 4).unwrap_or(0.0),
                h: read_arg_f32(ctx, argc, argv, 5).unwrap_or(0.0),
            },
        }),
        "circle" => Some(Ui3Command::GraphicsCircle {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            center: Ui3Point {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(0.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(0.0),
            },
            radius: read_arg_f32(ctx, argc, argv, 4).unwrap_or(0.0),
        }),
        "moveTo" => Some(Ui3Command::GraphicsMoveTo {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            to: Ui3Point {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(0.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(0.0),
            },
        }),
        "lineTo" => Some(Ui3Command::GraphicsLineTo {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            to: Ui3Point {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(0.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(0.0),
            },
        }),
        "fill" => Some(Ui3Command::GraphicsFill {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            color: pixi_color(
                read_arg_u32(ctx, argc, argv, 2).unwrap_or(0x00ff_ffff),
                read_arg_f32(ctx, argc, argv, 3).unwrap_or(1.0),
            ),
        }),
        "stroke" => Some(Ui3Command::GraphicsStroke {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            color: pixi_color(
                read_arg_u32(ctx, argc, argv, 2).unwrap_or(0x00ff_ffff),
                read_arg_f32(ctx, argc, argv, 3).unwrap_or(1.0),
            ),
            width: read_arg_f32(ctx, argc, argv, 4).unwrap_or(1.0),
        }),
        "text" => Some(Ui3Command::Text {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            params: vec![Ui3TextParam::Text(
                read_arg_string(ctx, argc, argv, 2).unwrap_or_default(),
            )],
        }),
        "textFill" => Some(Ui3Command::Text {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            params: vec![Ui3TextParam::Fill(pixi_color(
                read_arg_u32(ctx, argc, argv, 2).unwrap_or(0x00ff_ffff),
                read_arg_f32(ctx, argc, argv, 3).unwrap_or(1.0),
            ))],
        }),
        _ => None,
    }
}

fn node_kind_from_name(kind: &str) -> Ui3NodeKind {
    match kind.as_bytes() {
        b"Graphics" => Ui3NodeKind::Graphics,
        b"Text" => Ui3NodeKind::Text,
        _ => Ui3NodeKind::Container,
    }
}

fn pixi_color(rgb: u32, alpha: f32) -> Ui3Color {
    Ui3Color {
        rgba: 0xff00_0000 | (rgb & 0x00ff_ffff),
        alpha,
    }
}

unsafe fn read_arg_value(
    argc: c_int,
    argv: *const qjs::JSValueConst,
    index: usize,
) -> Option<qjs::JSValueConst> {
    if argv.is_null() || index >= argc.max(0) as usize {
        return None;
    }
    Some(*argv.add(index))
}

unsafe fn read_arg_f64(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    index: usize,
) -> Option<f64> {
    let value = read_arg_value(argc, argv, index)?;
    let mut out = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite() {
        Some(out)
    } else {
        None
    }
}

unsafe fn read_arg_f32(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    index: usize,
) -> Option<f32> {
    read_arg_f64(ctx, argc, argv, index).map(|value| value as f32)
}

unsafe fn read_arg_u32(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    index: usize,
) -> Option<u32> {
    let value = read_arg_f64(ctx, argc, argv, index)?;
    if value >= 0.0 {
        Some(value.min(u32::MAX as f64) as u32)
    } else {
        None
    }
}

unsafe fn read_arg_usize(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    index: usize,
) -> Option<usize> {
    let value = read_arg_f64(ctx, argc, argv, index)?;
    if value >= 0.0 {
        Some(value.min(usize::MAX as f64) as usize)
    } else {
        None
    }
}

unsafe fn read_arg_bool(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    index: usize,
) -> Option<bool> {
    read_arg_f64(ctx, argc, argv, index).map(|value| value != 0.0)
}

unsafe fn read_arg_string(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    index: usize,
) -> Option<String> {
    qjs::jsbind::to_string(ctx, read_arg_value(argc, argv, index)?)
}

unsafe fn read_trueos_id(ctx: *mut qjs::JSContext, object: qjs::JSValueConst) -> Option<u32> {
    let id = qjs::JS_GetPropertyStr(ctx, object, b"__trueosPixiId\0".as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok = !id.is_exception() && qjs::JS_ToFloat64(ctx, &mut out as *mut f64, id) == 0;
    qjs::js_free_value(ctx, id);
    if ok && out.is_finite() && out > 0.0 {
        Some(out.min(u32::MAX as f64) as u32)
    } else {
        None
    }
}

unsafe fn read_stage_child_count(ctx: *mut qjs::JSContext, stage: qjs::JSValueConst) -> u32 {
    let children = qjs::JS_GetPropertyStr(ctx, stage, b"children\0".as_ptr() as *const c_char);
    if children.is_exception() {
        return 0;
    }
    let length = qjs::JS_GetPropertyStr(ctx, children, b"length\0".as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok = !length.is_exception() && qjs::JS_ToFloat64(ctx, &mut out as *mut f64, length) == 0;
    qjs::js_free_value(ctx, length);
    qjs::js_free_value(ctx, children);
    if ok && out.is_finite() && out > 0.0 {
        out.min(u32::MAX as f64) as u32
    } else {
        0
    }
}

unsafe fn eval_global_script(
    ctx: *mut qjs::JSContext,
    source: &[u8],
    filename: &[u8],
    diag_label: &str,
) -> bool {
    let value = qjs::js_eval_bytes(
        ctx,
        source,
        filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );
    if value.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, diag_label);
        qjs::js_free_value(ctx, value);
        return false;
    }
    qjs::js_free_value(ctx, value);
    true
}
