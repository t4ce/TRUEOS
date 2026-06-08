use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use core::ffi::{c_char, c_int};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};
use trueos_gfx_core::Rgba8;

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
const PIXI_HOST_PRELUDE_FILENAME: &[u8] = b"<ui3-pixi-host-prelude>\0";
const PIXI_BUNDLE_FILENAME: &[u8] = b"<ui3-pixi-bundle>\0";
const PIXI_CAPTURE_ADAPTER_FILENAME: &[u8] = b"<ui3-pixi-capture-adapter>\0";
const PIXI_SERVICE_PARK_MS: u64 = 1_000;
const PIXI_SERVICE_EXEC_POLL_MS: u64 = 1;
const PIXI_SERVICE_MAX_DRAIN_PER_TICK: usize = 512;
const PIXI_SERVICE_NOOP_AFTER_SPAWN: bool = true;

static PIXI_SERVICE_READY: AtomicBool = AtomicBool::new(false);
static PIXI_SERVICE_PUMP_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_RENDER_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_OP_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_FILTERED_OP_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_DRAW_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_RUNTIME: Once<Mutex<Ui3PixiServiceRuntime>> = Once::new();
static PIXI_SERVICE_QUEUE: Once<Mutex<VecDeque<Ui3PixiServiceRequest>>> = Once::new();

enum Ui3PixiServiceRequest {
    Begin {
        browser_id: u32,
        root_id: u32,
    },
    DeclareNode {
        browser_id: u32,
        node_id: u32,
        kind: Ui3NodeKind,
    },
    Command {
        browser_id: u32,
        command: Ui3Command,
    },
}

struct Ui3PixiServiceRuntime {
    host: Ui3PixiHost,
    browser_id: u32,
    root_id: u32,
    scene_started: bool,
    op_count: u32,
    filtered_op_count: u32,
    frame_count: u32,
    last_draw_count: u32,
    last_present: Ui3IntelPresentSummary,
}

impl Ui3PixiServiceRuntime {
    fn new() -> Self {
        Self {
            host: Ui3PixiHost::new(),
            browser_id: 0,
            root_id: 0,
            scene_started: false,
            op_count: 0,
            filtered_op_count: 0,
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

pub fn pixi_service_filtered_op_count() -> u32 {
    PIXI_SERVICE_FILTERED_OP_COUNT.load(Ordering::Acquire)
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
    PIXI_SERVICE_FILTERED_OP_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_FRAME_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_DRAW_COUNT.store(0, Ordering::Release);
    *pixi_runtime().lock() = Ui3PixiServiceRuntime::new();
    pixi_queue().lock().clear();

    if PIXI_SERVICE_NOOP_AFTER_SPAWN {
        let font_assets = super::ui3_asset_service::ui3_font_sprite64_asset_status();
        PIXI_SERVICE_READY.store(true, Ordering::Release);
        crate::log!(
            "ui3-pixi-service: spawned executor=1 qjs_host=0 ready=1 reason=qjs-pixi-path-disabled renders=0 ops=0 filtered_ops=0 frames=0 draws=0 font_sprite64_ready={} font_ready_seq={} font_slots={} bundle_bytes={} adapter_bytes={}\n",
            font_assets.ready as u8,
            font_assets.ready_seq,
            font_assets.atlas_slots,
            PIXI_BUNDLE_SOURCE.len(),
            PIXI_CAPTURE_ADAPTER_SOURCE.len()
        );
        loop {
            if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
                crate::log!("ui3-pixi-service: stop requested; executor exit\n");
                break;
            }
            let drained = drain_service_queue(PIXI_SERVICE_MAX_DRAIN_PER_TICK);
            let delay = if drained > 0 {
                PIXI_SERVICE_EXEC_POLL_MS
            } else {
                PIXI_SERVICE_PARK_MS
            };
            Timer::after(EmbassyDuration::from_millis(delay)).await;
        }
        return;
    }

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

        let ready = true;
        PIXI_SERVICE_READY.store(ready, Ordering::Release);
        let font_assets = super::ui3_asset_service::ui3_font_sprite64_asset_status();
        crate::log!(
            "ui3-pixi-service: ready={} renders={} ops={} filtered_ops={} frames={} draws={} font_sprite64_ready={} font_ready_seq={} font_slots={} bundle_bytes={} adapter_bytes={} profile=browser\n",
            if ready { 1 } else { 0 },
            PIXI_SERVICE_RENDER_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_OP_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_FILTERED_OP_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_FRAME_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_DRAW_COUNT.load(Ordering::Acquire),
            font_assets.ready as u8,
            font_assets.ready_seq,
            font_assets.atlas_slots,
            PIXI_BUNDLE_SOURCE.len(),
            PIXI_CAPTURE_ADAPTER_SOURCE.len()
        );

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
            let drained = drain_service_queue(PIXI_SERVICE_MAX_DRAIN_PER_TICK);
            let delay = if drained > 0 {
                PIXI_SERVICE_EXEC_POLL_MS
            } else {
                PIXI_SERVICE_PARK_MS
            };
            Timer::after(EmbassyDuration::from_millis(delay)).await;
        }
    }
}

fn pixi_runtime() -> &'static Mutex<Ui3PixiServiceRuntime> {
    PIXI_SERVICE_RUNTIME.call_once(|| Mutex::new(Ui3PixiServiceRuntime::new()))
}

fn pixi_queue() -> &'static Mutex<VecDeque<Ui3PixiServiceRequest>> {
    PIXI_SERVICE_QUEUE.call_once(|| Mutex::new(VecDeque::new()))
}

fn enqueue_request(request: Ui3PixiServiceRequest) -> i32 {
    pixi_queue().lock().push_back(request);
    0
}

pub(super) fn queue_scene_begin(browser_id: u32, root_id: u32) -> i32 {
    enqueue_request(Ui3PixiServiceRequest::Begin {
        browser_id,
        root_id,
    })
}

pub(super) fn queue_scene_node(browser_id: u32, node_id: u32, kind: Ui3NodeKind) -> i32 {
    enqueue_request(Ui3PixiServiceRequest::DeclareNode {
        browser_id,
        node_id,
        kind,
    })
}

pub(super) fn queue_scene_command(browser_id: u32, command: Ui3Command) -> i32 {
    enqueue_request(Ui3PixiServiceRequest::Command {
        browser_id,
        command,
    })
}

fn drain_service_queue(max_requests: usize) -> usize {
    let mut drained = 0usize;
    while drained < max_requests {
        let request = {
            let mut queue = pixi_queue().lock();
            queue.pop_front()
        };
        let Some(request) = request else {
            break;
        };
        apply_service_request(request);
        drained += 1;
    }
    if drained > 0 {
        PIXI_SERVICE_PUMP_COUNT.fetch_add(1, Ordering::AcqRel);
    }
    drained
}

fn reset_runtime_for_scene(
    runtime: &mut Ui3PixiServiceRuntime,
    browser_id: u32,
    root_id: u32,
    scene_started: bool,
) {
    runtime.host = Ui3PixiHost::new();
    runtime.browser_id = browser_id;
    runtime.root_id = root_id;
    runtime.scene_started = scene_started;
    runtime.op_count = 0;
    runtime.filtered_op_count = 0;
    runtime.frame_count = 0;
    runtime.last_draw_count = 0;
    runtime.last_present = Ui3IntelPresentSummary::default();
    PIXI_SERVICE_OP_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_FILTERED_OP_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_FRAME_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_DRAW_COUNT.store(0, Ordering::Release);
    crate::log!(
        "ui3-pixi-service: scene runtime reset browser={} root={} started={}\n",
        browser_id,
        root_id,
        scene_started as u8
    );
}

fn apply_service_request(request: Ui3PixiServiceRequest) {
    match request {
        Ui3PixiServiceRequest::Begin {
            browser_id,
            root_id,
        } => {
            let mut runtime = pixi_runtime().lock();
            reset_runtime_for_scene(&mut runtime, browser_id, root_id, true);
            runtime.host.declare_node(root_id, Ui3NodeKind::Container);
            crate::log!("ui3-pixi-service: scene begin browser={} root={}\n", browser_id, root_id);
        }
        Ui3PixiServiceRequest::DeclareNode {
            browser_id,
            node_id,
            kind,
        } => {
            let mut runtime = pixi_runtime().lock();
            if !runtime.scene_started || runtime.browser_id != browser_id {
                // A node without a preceding begin is still accepted, but starts a
                // fresh service-owned scene for this producer.
                reset_runtime_for_scene(&mut runtime, browser_id, node_id, true);
            }
            runtime.host.declare_node(node_id, kind);
            runtime.op_count = runtime.op_count.wrapping_add(1).max(1);
            PIXI_SERVICE_OP_COUNT.store(runtime.op_count, Ordering::Release);
        }
        Ui3PixiServiceRequest::Command {
            browser_id,
            command,
        } => {
            apply_service_command(browser_id, command);
        }
    }
}

fn apply_service_command(browser_id: u32, command: Ui3Command) -> i32 {
    let mut runtime = pixi_runtime().lock();
    if !runtime.scene_started || runtime.browser_id != browser_id {
        let root_id = match &command {
            Ui3Command::Render { root } => *root,
            _ => 0,
        };
        reset_runtime_for_scene(&mut runtime, browser_id, root_id, true);
        if root_id != 0 {
            runtime.host.declare_node(root_id, Ui3NodeKind::Container);
        }
    }
    runtime.op_count = runtime.op_count.wrapping_add(1).max(1);
    PIXI_SERVICE_OP_COUNT.store(runtime.op_count, Ordering::Release);
    if pixi_light_filter_command(&mut runtime, &command) {
        return 0;
    }

    let is_render = matches!(command, Ui3Command::Render { .. });
    let apply_start_ms = if is_render { super::now_ms() } else { 0 };
    if is_render {
        crate::log!(
            "ui3-pixi-service: render request browser={} ops={} filtered_ops={} frames={}\n",
            browser_id,
            runtime.op_count,
            runtime.filtered_op_count,
            runtime.frame_count
        );
    }
    let frame = runtime.host.apply(command).cloned();
    let apply_ms = if is_render {
        super::now_ms().saturating_sub(apply_start_ms)
    } else {
        0
    };
    if let Some(frame) = frame {
        lower_present_and_count(&mut runtime, browser_id, &frame, apply_ms, is_render);
    }
    0
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
        crate::log!(
            "ui3-pixi-service: pixi-op #{} {} id={} ignored reason=pixi-service-noop\n",
            runtime.op_count,
            op.as_str(),
            id
        );
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

    if call <= 4 {
        let present = pixi_runtime().lock().last_present;
        let font_assets = super::ui3_asset_service::ui3_font_sprite64_asset_status();
        crate::log!(
            "ui3-pixi-service: __trueosRender call={} root={} root_children={} ops={} filtered_ops={} draws={} solid_rects={} meshes={} text={} presented={} fill_descs={} blend_descs={} rect_ms={} sprite_ms={} publish_ms={} present_ms={} total_ms={} font_sprite64_ready={} font_ready_seq={} ignored=pixi-service-noop\n",
            call,
            root,
            children,
            PIXI_SERVICE_OP_COUNT.load(Ordering::Acquire),
            PIXI_SERVICE_FILTERED_OP_COUNT.load(Ordering::Acquire),
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
            present.total_ms,
            font_assets.ready as u8,
            font_assets.ready_seq
        );
    }

    qjs::JS_NewFloat64(ctx, call as f64)
}

fn lower_present_and_count(
    runtime: &mut Ui3PixiServiceRuntime,
    browser_id: u32,
    frame: &Ui3RenderFrame,
    apply_ms: u64,
    force_log: bool,
) {
    runtime.frame_count = runtime.frame_count.wrapping_add(1).max(1);
    let lower_start_ms = super::now_ms();
    let mut geometry = lower_ui3_frame_geometry(&runtime.host, frame);
    let (cursor_draws, cursor_hits) = append_kernel_cursor_overlays(&runtime.host, &mut geometry);
    let lower_ms = super::now_ms().saturating_sub(lower_start_ms);
    let present_start_ms = super::now_ms();
    let present = present_ui3_frame_to_intel_primary(&geometry);
    let present_wall_ms = super::now_ms().saturating_sub(present_start_ms);
    let present_gap_ms = present_wall_ms.saturating_sub(present.total_ms);
    runtime.last_draw_count = geometry.draws.len().min(u32::MAX as usize) as u32;
    runtime.last_present = present;
    PIXI_SERVICE_FRAME_COUNT.store(runtime.frame_count, Ordering::Release);
    PIXI_SERVICE_DRAW_COUNT.store(runtime.last_draw_count, Ordering::Release);

    if runtime.frame_count <= 4 || force_log {
        let mut rect_fill_draws = 0usize;
        let mut rect_stroke_draws = 0usize;
        let mut axis_line_draws = 0usize;
        let mut mesh_fill_draws = 0usize;
        let mut mesh_stroke_draws = 0usize;
        let mut texture_draws = 0usize;
        for draw in &geometry.draws {
            match draw {
                super::Ui3LoweredDraw::SolidRect { kind, .. } => match kind {
                    super::Ui3SolidRectKind::Fill => {
                        rect_fill_draws = rect_fill_draws.saturating_add(1)
                    }
                    super::Ui3SolidRectKind::RectStroke => {
                        rect_stroke_draws = rect_stroke_draws.saturating_add(1)
                    }
                    super::Ui3SolidRectKind::AxisLineStroke => {
                        axis_line_draws = axis_line_draws.saturating_add(1)
                    }
                },
                super::Ui3LoweredDraw::Mesh { kind, .. } => match kind {
                    super::Ui3MeshKind::Fill => mesh_fill_draws = mesh_fill_draws.saturating_add(1),
                    super::Ui3MeshKind::Stroke => {
                        mesh_stroke_draws = mesh_stroke_draws.saturating_add(1)
                    }
                },
                super::Ui3LoweredDraw::TextureRect { .. } => {
                    texture_draws = texture_draws.saturating_add(1)
                }
                super::Ui3LoweredDraw::TextRun { .. } => {}
            }
        }
        crate::log!(
            "ui3-pixi-service: render browser={} root={} ops={} filtered_ops={} draws={} nodes={} cursor_draws={} cursor_hits={} solid_rects={} meshes={} textures={} text={} presented={} fill_descs={} blend_descs={} apply_ms={} lower_ms={} present_wall_ms={} rect_ms={} mesh_ms={} sprite_ms={} publish_ms={} present_ms={} total_ms={} gap_ms={}\n",
            browser_id,
            frame.root,
            runtime.op_count,
            runtime.filtered_op_count,
            geometry.draws.len(),
            frame.ordered_nodes.len(),
            cursor_draws,
            cursor_hits,
            present.solid_rects,
            present.mesh_draws,
            present.texture_draws,
            present.text_runs,
            present.presented as u8,
            present.fill_descs,
            present.blend_descs,
            apply_ms,
            lower_ms,
            present_wall_ms,
            present.rect_ms,
            present.mesh_ms,
            present.sprite_ms,
            present.publish_ms,
            present.present_ms,
            present.total_ms,
            present_gap_ms
        );
        crate::log!(
            "ui3-pixi-service: materialized browser={} root={} rect_fill={} rect_stroke={} axis_line={} mesh_fill={} mesh_stroke={} texture={} text={}\n",
            browser_id,
            frame.root,
            rect_fill_draws,
            rect_stroke_draws,
            axis_line_draws,
            mesh_fill_draws,
            mesh_stroke_draws,
            texture_draws,
            present.text_runs
        );
    }
}

fn append_kernel_cursor_overlays(
    host: &Ui3PixiHost,
    geometry: &mut super::Ui3GeometryFrame,
) -> (usize, usize) {
    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons();
    if cursors.is_empty() {
        return (0, 0);
    }
    let (view_w, view_h) = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| (w.max(1) as f32, h.max(1) as f32))
        .unwrap_or((1920.0, 1080.0));
    let mut draw_count = 0usize;
    let mut hit_count = 0usize;
    for (idx, (slot_id, nx, ny, buttons)) in cursors.iter().copied().enumerate() {
        let cursor_id = (idx as u32).saturating_add(1);
        let x = (nx as f32 * view_w).clamp(0.0, view_w - 1.0);
        let y = (ny as f32 * view_h).clamp(0.0, view_h - 1.0);
        let color = crate::r::ui2::cursor_color_rgba8_for_cursor_id(cursor_id);
        let target = host.hit_scene().hit_at(x, y);
        if target.is_some() {
            hit_count = hit_count.saturating_add(1);
        }
        append_cursor_cross(geometry, cursor_id, x, y, color, buttons != 0);
        draw_count = draw_count.saturating_add(3);
        if idx < 4 {
            crate::log!(
                "ui3-pixi-service: cursor slot={} cursor={} x={} y={} buttons=0x{:x} hit_node={}\n",
                slot_id,
                cursor_id,
                x as i32,
                y as i32,
                buttons,
                target.map(|hit| hit.node).unwrap_or(0)
            );
        }
    }
    (draw_count, hit_count)
}

fn append_cursor_cross(
    geometry: &mut super::Ui3GeometryFrame,
    cursor_id: u32,
    x: f32,
    y: f32,
    color: Rgba8,
    pressed: bool,
) {
    let size = if pressed { 14.0 } else { 11.0 };
    let arm = if pressed { 3.0 } else { 2.0 };
    let dark = Rgba8::new(0, 0, 0, 220);
    push_cursor_rect(geometry, cursor_id, x - size, y - arm * 0.5, size * 2.0, arm, dark);
    push_cursor_rect(geometry, cursor_id, x - arm * 0.5, y - size, arm, size * 2.0, dark);
    push_cursor_rect(
        geometry,
        cursor_id,
        x - size + 1.0,
        y - arm * 0.5 + 1.0,
        size * 2.0 - 2.0,
        arm.max(1.0),
        color,
    );
}

fn push_cursor_rect(
    geometry: &mut super::Ui3GeometryFrame,
    cursor_id: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Rgba8,
) {
    geometry.draws.push(super::Ui3LoweredDraw::SolidRect {
        node: 0xC000_0000 | cursor_id,
        rect: Ui3Rect { x, y, w, h },
        color,
        kind: super::Ui3SolidRectKind::Fill,
        clip: None,
    });
}

fn pixi_light_filter_command(runtime: &mut Ui3PixiServiceRuntime, command: &Ui3Command) -> bool {
    let Some(reason) = super::ui3_light_filter_reason(command) else {
        return false;
    };
    runtime.filtered_op_count = runtime.filtered_op_count.wrapping_add(1).max(1);
    PIXI_SERVICE_FILTERED_OP_COUNT.store(runtime.filtered_op_count, Ordering::Release);
    if runtime.filtered_op_count <= 8 {
        crate::log!(
            "ui3-pixi-service: light-filter op={} reason={}\n",
            runtime.filtered_op_count,
            reason
        );
    }
    true
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
        "alpha" => Some(Ui3Command::SetAlpha {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            alpha: read_arg_f32(ctx, argc, argv, 2).unwrap_or(1.0),
        }),
        "scale" => Some(Ui3Command::SetScale {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            scale: Ui3Point {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(1.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(1.0),
            },
        }),
        "mask" => {
            let mask_id = read_arg_u32(ctx, argc, argv, 2).unwrap_or(0);
            Some(Ui3Command::SetMask {
                node: read_arg_u32(ctx, argc, argv, 1)?,
                mask: if mask_id > 0 { Some(mask_id) } else { None },
            })
        }
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
        "roundRect" => Some(Ui3Command::GraphicsRoundRect {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            rect: Ui3Rect {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(0.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(0.0),
                w: read_arg_f32(ctx, argc, argv, 4).unwrap_or(0.0),
                h: read_arg_f32(ctx, argc, argv, 5).unwrap_or(0.0),
            },
            radius: read_arg_f32(ctx, argc, argv, 6).unwrap_or(0.0),
        }),
        "circle" => Some(Ui3Command::GraphicsCircle {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            center: Ui3Point {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(0.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(0.0),
            },
            radius: read_arg_f32(ctx, argc, argv, 4).unwrap_or(0.0),
        }),
        "ellipse" => Some(Ui3Command::GraphicsEllipse {
            node: read_arg_u32(ctx, argc, argv, 1)?,
            center: Ui3Point {
                x: read_arg_f32(ctx, argc, argv, 2).unwrap_or(0.0),
                y: read_arg_f32(ctx, argc, argv, 3).unwrap_or(0.0),
            },
            rx: read_arg_f32(ctx, argc, argv, 4).unwrap_or(0.0),
            ry: read_arg_f32(ctx, argc, argv, 5).unwrap_or(0.0),
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
        "closePath" => Some(Ui3Command::GraphicsClosePath {
            node: read_arg_u32(ctx, argc, argv, 1)?,
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
