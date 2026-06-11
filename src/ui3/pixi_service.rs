use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use libm::{ceilf, floorf};
use spin::{Mutex, Once};

use trueos_qjs as qjs;

use super::intel_present::{
    Ui3IntelPresentSummary, present_ui3_frame_damage_to_intel_primary,
    present_ui3_frame_to_intel_primary,
};
use super::{
    Ui3Command, Ui3GeometryFrame, Ui3NodeKind, Ui3PixiHost, Ui3PointerEventKind, Ui3Rect,
    Ui3RenderFrame, lower_ui3_frame_geometry,
};

const TASK_NAME: &str = "ui3-pixi-service";
const PIXI_SERVICE_PARK_MS: u64 = 1_000;
const PIXI_SERVICE_EXEC_POLL_MS: u64 = 1;
const PIXI_SERVICE_CURSOR_POLL_MS: u64 = 33;
const PIXI_SERVICE_MAX_DRAIN_PER_TICK: usize = 512;
const PIXI_STATIC_SCROLL_ONLY: bool = true;
const PIXI_CURSOR_DAMAGE_HALF_RATIO: f32 = 0.010;
const PIXI_CURSOR_DAMAGE_HALF_MIN_PX: f32 = 10.0;
const PIXI_CURSOR_DAMAGE_HALF_MAX_PX: f32 = 18.0;
const PIXI_CURSOR_DAMAGE_MARGIN_PX: f32 = 3.0;
const PIXI_CURSOR_DAMAGE_FAST_MAX_PX: f32 = 96.0;

static PIXI_SERVICE_READY: AtomicBool = AtomicBool::new(false);
static PIXI_SERVICE_PUMP_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_OP_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_FILTERED_OP_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_DRAW_COUNT: AtomicU32 = AtomicU32::new(0);
static PIXI_SERVICE_SCENE_BUILDING: AtomicBool = AtomicBool::new(false);
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
    scene_building: bool,
    op_count: u32,
    filtered_op_count: u32,
    frame_count: u32,
    last_draw_count: u32,
    last_present: Ui3IntelPresentSummary,
    last_geometry: Option<Ui3GeometryFrame>,
    last_cursor_signature: u64,
    last_cursor_event_seq: u64,
    last_keyboard_seq: u64,
    cursor_hits: Vec<Ui3CursorHitState>,
    cursor_hit_log_count: u32,
    cursor_event_log_count: u32,
    cursor_empty_log_count: u32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct Ui3CursorHitState {
    cursor_id: u32,
    slot_id: u32,
    x: i32,
    y: i32,
    target_node: u32,
    buttons: u32,
}

impl Ui3PixiServiceRuntime {
    fn new() -> Self {
        Self {
            host: Ui3PixiHost::new(),
            browser_id: 0,
            root_id: 0,
            scene_started: false,
            scene_building: false,
            op_count: 0,
            filtered_op_count: 0,
            frame_count: 0,
            last_draw_count: 0,
            last_present: Ui3IntelPresentSummary::default(),
            last_geometry: None,
            last_cursor_signature: 0,
            last_cursor_event_seq: 0,
            last_keyboard_seq: 0,
            cursor_hits: Vec::new(),
            cursor_hit_log_count: 0,
            cursor_event_log_count: 0,
            cursor_empty_log_count: 0,
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
    PIXI_SERVICE_FRAME_COUNT.load(Ordering::Acquire)
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
    PIXI_SERVICE_OP_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_FILTERED_OP_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_FRAME_COUNT.store(0, Ordering::Release);
    PIXI_SERVICE_DRAW_COUNT.store(0, Ordering::Release);
    *pixi_runtime().lock() = Ui3PixiServiceRuntime::new();
    pixi_queue().lock().clear();

    let font_assets = super::ui3_asset_service::ui3_font_sprite64_asset_status();
    PIXI_SERVICE_READY.store(true, Ordering::Release);
    crate::log!(
        "ui3-pixi-service: retained-sink ready=1 qjs_host=0 ops=0 filtered_ops=0 frames=0 draws=0 font_sprite64_ready={} font_ready_seq={} font_slots={}\n",
        font_assets.ready as u8,
        font_assets.ready_seq,
        font_assets.atlas_slots
    );

    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!("ui3-pixi-service: stop requested; retained sink exit\n");
            break;
        }
        let drained = drain_service_queue(PIXI_SERVICE_MAX_DRAIN_PER_TICK);
        let cursor_refreshed = if drained == 0 {
            poll_retained_scene_cursor_input()
        } else {
            false
        };
        let keyboard_queued = if drained == 0 && !PIXI_STATIC_SCROLL_ONLY {
            queue_kernel_keyboard_events_for_qjs()
        } else {
            false
        };
        let delay = if drained > 0 {
            PIXI_SERVICE_EXEC_POLL_MS
        } else if cursor_refreshed || keyboard_queued || pixi_runtime().lock().scene_started {
            PIXI_SERVICE_CURSOR_POLL_MS
        } else {
            PIXI_SERVICE_PARK_MS
        };
        Timer::after(EmbassyDuration::from_millis(delay)).await;
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
    PIXI_SERVICE_SCENE_BUILDING.store(true, Ordering::Release);
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

pub(super) fn flush_service_queue(max_requests: usize) -> usize {
    drain_service_queue(max_requests)
}

fn reset_runtime_for_scene(
    runtime: &mut Ui3PixiServiceRuntime,
    browser_id: u32,
    root_id: u32,
    scene_started: bool,
) {
    let preserve_cursor_hits =
        runtime.scene_started && runtime.browser_id == browser_id && runtime.root_id == root_id;
    let previous_cursor_hits = if preserve_cursor_hits {
        runtime.cursor_hits.clone()
    } else {
        Vec::new()
    };
    let previous_cursor_event_seq = if preserve_cursor_hits {
        runtime.last_cursor_event_seq
    } else {
        0
    };
    let previous_keyboard_seq = if preserve_cursor_hits {
        runtime.last_keyboard_seq
    } else {
        0
    };
    runtime.host = Ui3PixiHost::new();
    runtime.browser_id = browser_id;
    runtime.root_id = root_id;
    runtime.scene_started = scene_started;
    runtime.scene_building = scene_started;
    runtime.op_count = 0;
    runtime.filtered_op_count = 0;
    runtime.frame_count = 0;
    runtime.last_draw_count = 0;
    runtime.last_present = Ui3IntelPresentSummary::default();
    runtime.last_geometry = None;
    runtime.last_cursor_signature = cursor_snapshot_signature();
    runtime.last_cursor_event_seq = previous_cursor_event_seq;
    runtime.last_keyboard_seq = previous_keyboard_seq;
    runtime.cursor_hits = previous_cursor_hits;
    runtime.cursor_hit_log_count = 0;
    runtime.cursor_event_log_count = 0;
    runtime.cursor_empty_log_count = 0;
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

fn poll_retained_scene_cursor_input() -> bool {
    if drain_kernel_cursor_events_for_qjs() {
        return true;
    }
    refresh_retained_scene_for_cursor_changes()
}

fn drain_kernel_cursor_events_for_qjs() -> bool {
    let mut events = [crate::usb2::hid::TrueosHidCursorEvent::default(); 32];
    let mut runtime = pixi_runtime().lock();
    if !runtime.scene_started
        || runtime.scene_building
        || PIXI_SERVICE_SCENE_BUILDING.load(Ordering::Acquire)
    {
        return false;
    }

    let (next_seq, dropped, wrote) =
        crate::usb2::hid::read_cursor_events_since(runtime.last_cursor_event_seq, &mut events);
    runtime.last_cursor_event_seq = next_seq;
    if wrote == 0 {
        return false;
    }

    if dropped != 0 {
        crate::log!(
            "ui3-pixi-service: cursor-events dropped={} browser={} next_seq={}\n",
            dropped,
            runtime.browser_id,
            next_seq
        );
    }
    if runtime.cursor_event_log_count < 16 {
        crate::log!(
            "ui3-pixi-service: cursor-events read browser={} wrote={} next_seq={}\n",
            runtime.browser_id,
            wrote,
            next_seq
        );
    }

    if !PIXI_STATIC_SCROLL_ONLY && let Some(frame) = runtime.host.last_frame().cloned() {
        let browser_id = runtime.browser_id;
        present_cursor_damage_or_fallback(&mut runtime, browser_id, &frame);
        runtime.last_cursor_signature = cursor_snapshot_signature();
    }

    let (view_w, view_h) = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| (w.max(1), h.max(1)))
        .unwrap_or((1920, 1080));
    let entry_count = runtime.host.hit_scene().entries().len();
    for event in events.iter().take(wrote) {
        process_cursor_event_for_qjs(&mut runtime, *event, view_w, view_h, entry_count);
    }

    true
}

fn refresh_retained_scene_for_cursor_changes() -> bool {
    if PIXI_STATIC_SCROLL_ONLY {
        return false;
    }
    let signature = cursor_snapshot_signature();
    let mut runtime = pixi_runtime().lock();
    if !runtime.scene_started
        || runtime.scene_building
        || PIXI_SERVICE_SCENE_BUILDING.load(Ordering::Acquire)
        || runtime.last_cursor_signature == signature
    {
        return false;
    }
    let Some(frame) = runtime.host.last_frame().cloned() else {
        runtime.last_cursor_signature = signature;
        return false;
    };
    runtime.last_cursor_signature = signature;
    let browser_id = runtime.browser_id;
    present_cursor_damage_or_fallback(&mut runtime, browser_id, &frame);
    update_cursor_hits(&mut runtime);
    true
}

fn present_cursor_damage_or_fallback(
    runtime: &mut Ui3PixiServiceRuntime,
    browser_id: u32,
    frame: &Ui3RenderFrame,
) {
    if present_cached_cursor_damage(runtime, browser_id, frame.root) {
        return;
    }
    lower_present_and_count(runtime, browser_id, frame, 0, false);
}

fn present_cached_cursor_damage(
    runtime: &mut Ui3PixiServiceRuntime,
    browser_id: u32,
    root_id: u32,
) -> bool {
    let Some(damage) = cursor_damage_rect(runtime) else {
        return false;
    };
    if damage.w > PIXI_CURSOR_DAMAGE_FAST_MAX_PX || damage.h > PIXI_CURSOR_DAMAGE_FAST_MAX_PX {
        if runtime.cursor_hit_log_count < 8 {
            crate::log!(
                "ui3-pixi-service: cursor-damage-skip browser={} root={} reason=large-damage damage={}x{}@{},{}\n",
                browser_id,
                root_id,
                damage.w as i32,
                damage.h as i32,
                damage.x as i32,
                damage.y as i32
            );
        }
        return false;
    }
    let Some(geometry) = runtime.last_geometry.as_ref() else {
        return false;
    };

    let present_start_ms = super::now_ms();
    let present = present_ui3_frame_damage_to_intel_primary(geometry, damage);
    let present_wall_ms = super::now_ms().saturating_sub(present_start_ms);
    let present_gap_ms = present_wall_ms.saturating_sub(present.total_ms);
    let draw_count = geometry.draws.len();
    runtime.frame_count = runtime.frame_count.wrapping_add(1).max(1);
    runtime.last_draw_count = draw_count.min(u32::MAX as usize) as u32;
    runtime.last_present = present;
    PIXI_SERVICE_FRAME_COUNT.store(runtime.frame_count, Ordering::Release);
    PIXI_SERVICE_DRAW_COUNT.store(runtime.last_draw_count, Ordering::Release);

    if runtime.frame_count <= 4 || runtime.cursor_hit_log_count < 8 {
        crate::log!(
            "ui3-pixi-service: cursor-damage-present browser={} root={} damage={}x{}@{},{} cached_draws={} solid_rects={} cursor_rects={} meshes={} textures={} text={} presented={} fill_descs={} blend_descs={} present_wall_ms={} rect_ms={} mesh_ms={} sprite_ms={} publish_ms={} present_ms={} total_ms={} gap_ms={}\n",
            browser_id,
            root_id,
            damage.w as i32,
            damage.h as i32,
            damage.x as i32,
            damage.y as i32,
            draw_count,
            present.solid_rects,
            present.cursor_rects,
            present.mesh_draws,
            present.texture_draws,
            present.text_runs,
            present.presented as u8,
            present.fill_descs,
            present.blend_descs,
            present_wall_ms,
            present.rect_ms,
            present.mesh_ms,
            present.sprite_ms,
            present.publish_ms,
            present.present_ms,
            present.total_ms,
            present_gap_ms
        );
    }
    true
}

fn process_cursor_event_for_qjs(
    runtime: &mut Ui3PixiServiceRuntime,
    event: crate::usb2::hid::TrueosHidCursorEvent,
    view_w: u32,
    view_h: u32,
    entry_count: usize,
) {
    if event.slot_id == 0 || !event.x.is_finite() || !event.y.is_finite() {
        return;
    }

    let x = (event.x.clamp(0.0, 1.0) as f32) * view_w.saturating_sub(1).max(1) as f32;
    let y = (event.y.clamp(0.0, 1.0) as f32) * view_h.saturating_sub(1).max(1) as f32;
    let target = runtime.host.hit_scene().hit_at(x, y);
    let target_node = target.map(|hit| hit.node).unwrap_or(0);
    let x_i = x as i32;
    let y_i = y as i32;
    let state = Ui3CursorHitState {
        cursor_id: cursor_id_for_slot(event.slot_id),
        slot_id: event.slot_id,
        x: x_i,
        y: y_i,
        target_node,
        buttons: event.buttons_down,
    };
    let previous = runtime
        .cursor_hits
        .iter()
        .copied()
        .find(|prev| ui3_cursor_source_matches(*prev, state));

    if event.wheel != 0 {
        let wheel_target_node = if target_node != 0 {
            target_node
        } else {
            runtime
                .host
                .last_frame()
                .map(|frame| frame.root)
                .unwrap_or(0)
        };
        if wheel_target_node != 0 {
            let delta_y = (event.wheel as i32).saturating_mul(-96);
            let _ = qjs::browser_task::queue_ui3_wheel_event_for_browser(
                runtime.browser_id,
                wheel_target_node,
                state.x,
                state.y,
                ui3_pointer_id_for_cursor(state),
                state.buttons,
                delta_y,
            );
        }
    }
    if PIXI_STATIC_SCROLL_ONLY {
        return;
    }

    emit_cursor_events(runtime, previous, state, entry_count);

    if let Some(pos) = runtime
        .cursor_hits
        .iter()
        .position(|prev| ui3_cursor_source_matches(*prev, state))
    {
        runtime.cursor_hits[pos] = state;
    } else {
        runtime.cursor_hits.push(state);
    }

    if previous != Some(state) || runtime.cursor_hit_log_count < 8 {
        runtime.cursor_hit_log_count = runtime.cursor_hit_log_count.saturating_add(1);
        crate::log!(
            "ui3-pixi-service: cursor-hit browser={} cursor={} slot={} pointer={} x={} y={} buttons=0x{:X} target={} hit_entries={} seq={} log_count={}\n",
            runtime.browser_id,
            state.cursor_id,
            state.slot_id,
            ui3_pointer_id_for_cursor(state),
            x_i,
            y_i,
            state.buttons,
            target_node,
            entry_count,
            event.seq,
            runtime.cursor_hit_log_count
        );
    }
}

fn queue_kernel_keyboard_events_for_qjs() -> bool {
    let mut events = [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); 32];
    let (browser_id, wrote, dropped) = {
        let mut runtime = pixi_runtime().lock();
        if !runtime.scene_started
            || runtime.scene_building
            || PIXI_SERVICE_SCENE_BUILDING.load(Ordering::Acquire)
        {
            return false;
        }
        let (next_seq, dropped, wrote) =
            crate::r::keyboard::read_output_events_since(runtime.last_keyboard_seq, &mut events);
        runtime.last_keyboard_seq = next_seq;
        (runtime.browser_id, wrote, dropped)
    };

    if wrote == 0 {
        return false;
    }
    if dropped != 0 {
        crate::log!(
            "ui3-pixi-service: keyboard-events dropped={} browser={}\n",
            dropped,
            browser_id
        );
    }

    let mut queued = 0u32;
    for event in events.iter().take(wrote) {
        let Some(key) = keyboard_output_event_key(event) else {
            continue;
        };
        let pointer_id = ui3_pointer_id_for_keyboard_slot(event.slot_id);
        if qjs::browser_task::queue_ui3_keyboard_event_for_browser(
            browser_id,
            key.clone(),
            event.slot_id,
            pointer_id,
            u32::from(event.modifiers),
        ) {
            queued = queued.saturating_add(1);
            if queued <= 16 {
                crate::log!(
                    "ui3-pixi-service: keyboard-event browser={} slot={} pointer={} key={} modifiers=0x{:X} seq={} queued={}\n",
                    browser_id,
                    event.slot_id,
                    pointer_id,
                    key,
                    event.modifiers,
                    event.seq,
                    queued
                );
            }
        }
    }

    queued != 0
}

fn keyboard_output_event_key(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> Option<String> {
    if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT && event.utf8_len != 0 {
        let len = usize::from(event.utf8_len).min(event.utf8.len());
        if let Ok(text) = core::str::from_utf8(&event.utf8[..len]) {
            if !text.is_empty() {
                return Some(String::from(text));
            }
        }
    }

    let key = match event.key_code {
        crate::r::keyboard::KEYBOARD_KEY_BACKSPACE => "Backspace",
        crate::r::keyboard::KEYBOARD_KEY_TAB => "Tab",
        crate::r::keyboard::KEYBOARD_KEY_ENTER => "Enter",
        crate::r::keyboard::KEYBOARD_KEY_ESCAPE => "Escape",
        crate::r::keyboard::KEYBOARD_KEY_SPACE => " ",
        crate::r::keyboard::KEYBOARD_KEY_DELETE => "Delete",
        crate::r::keyboard::KEYBOARD_KEY_HOME => "Home",
        crate::r::keyboard::KEYBOARD_KEY_END => "End",
        crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => "ArrowLeft",
        crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => "ArrowRight",
        crate::r::keyboard::KEYBOARD_KEY_ARROW_UP => "ArrowUp",
        crate::r::keyboard::KEYBOARD_KEY_ARROW_DOWN => "ArrowDown",
        _ => return None,
    };
    Some(String::from(key))
}

const fn ui3_pointer_id_for_keyboard_slot(slot_id: u32) -> u32 {
    if slot_id != 0 { slot_id } else { 1 }
}

fn update_cursor_hits(runtime: &mut Ui3PixiServiceRuntime) {
    let (view_w, view_h) = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| (w.max(1), h.max(1)))
        .unwrap_or((1920, 1080));
    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons();
    let entry_count = runtime.host.hit_scene().entries().len();
    let mut next = Vec::new();
    if cursors.is_empty() && entry_count != 0 && runtime.cursor_empty_log_count < 8 {
        runtime.cursor_empty_log_count = runtime.cursor_empty_log_count.saturating_add(1);
        crate::log!(
            "ui3-pixi-service: cursor-hit skipped browser={} reason=no-kernel-cursor-snapshots hit_entries={} log_count={}\n",
            runtime.browser_id,
            entry_count,
            runtime.cursor_empty_log_count
        );
    }

    for (idx, (slot_id, nx, ny, buttons)) in cursors.into_iter().enumerate() {
        if !nx.is_finite() || !ny.is_finite() {
            continue;
        }
        let cursor_id = (idx as u32).saturating_add(1);
        let x = (nx.clamp(0.0, 1.0) as f32) * view_w.saturating_sub(1).max(1) as f32;
        let y = (ny.clamp(0.0, 1.0) as f32) * view_h.saturating_sub(1).max(1) as f32;
        let target = runtime.host.hit_scene().hit_at(x, y);
        let target_node = target.map(|hit| hit.node).unwrap_or(0);
        let x_i = x as i32;
        let y_i = y as i32;
        let state = Ui3CursorHitState {
            cursor_id,
            slot_id,
            x: x_i,
            y: y_i,
            target_node,
            buttons,
        };
        let previous = runtime
            .cursor_hits
            .iter()
            .copied()
            .find(|prev| ui3_cursor_source_matches(*prev, state));

        emit_cursor_events(runtime, previous, state, entry_count);

        if previous != Some(state) || runtime.cursor_hit_log_count < 8 {
            runtime.cursor_hit_log_count = runtime.cursor_hit_log_count.saturating_add(1);
            crate::log!(
                "ui3-pixi-service: cursor-hit browser={} cursor={} slot={} pointer={} x={} y={} buttons=0x{:X} target={} hit_entries={} log_count={}\n",
                runtime.browser_id,
                cursor_id,
                slot_id,
                ui3_pointer_id_for_cursor(state),
                x_i,
                y_i,
                buttons,
                target_node,
                entry_count,
                runtime.cursor_hit_log_count
            );
        }
        next.push(state);
    }

    for previous in runtime.cursor_hits.clone() {
        let still_present = next
            .iter()
            .any(|state| ui3_cursor_source_matches(previous, *state));
        if still_present {
            continue;
        }
        if previous.buttons != 0 {
            emit_cursor_event(
                runtime,
                Ui3PointerEventKind::PointerUpOutside,
                previous,
                previous.target_node,
                previous.target_node,
                entry_count,
            );
        }
        if previous.target_node != 0 {
            emit_cursor_event(
                runtime,
                Ui3PointerEventKind::PointerOut,
                previous,
                previous.target_node,
                previous.target_node,
                entry_count,
            );
        }
    }

    runtime.cursor_hits = next;
}

fn cursor_damage_rect(runtime: &Ui3PixiServiceRuntime) -> Option<Ui3Rect> {
    let (view_w, view_h) = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| (w.max(1), h.max(1)))
        .unwrap_or((1920, 1080));
    let half = ((view_h as f32) * PIXI_CURSOR_DAMAGE_HALF_RATIO)
        .clamp(PIXI_CURSOR_DAMAGE_HALF_MIN_PX, PIXI_CURSOR_DAMAGE_HALF_MAX_PX)
        + PIXI_CURSOR_DAMAGE_MARGIN_PX;
    let mut damage = None;

    for previous in &runtime.cursor_hits {
        let x = previous.x as f32;
        let y = previous.y as f32;
        damage = union_damage_rect(damage, cursor_damage_rect_at(x, y, half, view_w, view_h));
    }

    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons();
    for (_slot_id, nx, ny, _buttons) in cursors {
        if !nx.is_finite() || !ny.is_finite() {
            continue;
        }
        let x = (nx.clamp(0.0, 1.0) as f32) * view_w.saturating_sub(1).max(1) as f32;
        let y = (ny.clamp(0.0, 1.0) as f32) * view_h.saturating_sub(1).max(1) as f32;
        damage = union_damage_rect(damage, cursor_damage_rect_at(x, y, half, view_w, view_h));
    }

    damage
}

fn cursor_damage_rect_at(x: f32, y: f32, half: f32, view_w: u32, view_h: u32) -> Option<Ui3Rect> {
    let max_w = view_w as f32;
    let max_h = view_h as f32;
    let x0 = floorf(x - half).max(0.0).min(max_w);
    let y0 = floorf(y - half).max(0.0).min(max_h);
    let x1 = ceilf(x + half).max(0.0).min(max_w);
    let y1 = ceilf(y + half).max(0.0).min(max_h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Ui3Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

fn union_damage_rect(a: Option<Ui3Rect>, b: Option<Ui3Rect>) -> Option<Ui3Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(union_rect(a, b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn union_rect(a: Ui3Rect, b: Ui3Rect) -> Ui3Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Ui3Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

fn emit_cursor_events(
    runtime: &mut Ui3PixiServiceRuntime,
    previous: Option<Ui3CursorHitState>,
    state: Ui3CursorHitState,
    entry_count: usize,
) {
    if let Some(previous) = previous {
        if previous.target_node != state.target_node {
            if previous.target_node != 0 {
                emit_cursor_event(
                    runtime,
                    Ui3PointerEventKind::PointerOut,
                    state,
                    previous.target_node,
                    previous.target_node,
                    entry_count,
                );
            }
            if state.target_node != 0 {
                emit_cursor_event(
                    runtime,
                    Ui3PointerEventKind::PointerOver,
                    state,
                    state.target_node,
                    previous.target_node,
                    entry_count,
                );
            }
        }

        let move_target = if state.target_node != 0 {
            state.target_node
        } else if previous.buttons != 0 {
            previous.target_node
        } else {
            0
        };
        if move_target != 0
            && (previous.x != state.x
                || previous.y != state.y
                || previous.target_node != state.target_node)
        {
            emit_cursor_event(
                runtime,
                Ui3PointerEventKind::PointerMove,
                state,
                move_target,
                previous.target_node,
                entry_count,
            );
        }

        if previous.buttons == 0 && state.buttons != 0 {
            emit_cursor_event(
                runtime,
                Ui3PointerEventKind::PointerDown,
                state,
                state.target_node,
                previous.target_node,
                entry_count,
            );
        } else if previous.buttons != 0 && state.buttons == 0 {
            let release_target = if state.target_node != 0 {
                state.target_node
            } else {
                previous.target_node
            };
            let kind = if previous.target_node != 0 && state.target_node != previous.target_node {
                Ui3PointerEventKind::PointerUpOutside
            } else {
                Ui3PointerEventKind::PointerUp
            };
            emit_cursor_event(
                runtime,
                kind,
                state,
                release_target,
                previous.target_node,
                entry_count,
            );
        }
    } else {
        if state.target_node != 0 {
            emit_cursor_event(
                runtime,
                Ui3PointerEventKind::PointerOver,
                state,
                state.target_node,
                0,
                entry_count,
            );
            if state.buttons != 0 {
                emit_cursor_event(
                    runtime,
                    Ui3PointerEventKind::PointerDown,
                    state,
                    state.target_node,
                    0,
                    entry_count,
                );
            }
            emit_cursor_event(
                runtime,
                Ui3PointerEventKind::PointerMove,
                state,
                state.target_node,
                0,
                entry_count,
            );
        }
        if state.buttons != 0 && state.target_node == 0 {
            emit_cursor_event(
                runtime,
                Ui3PointerEventKind::PointerDown,
                state,
                state.target_node,
                0,
                entry_count,
            );
        }
    }
}

fn emit_cursor_event(
    runtime: &mut Ui3PixiServiceRuntime,
    kind: Ui3PointerEventKind,
    state: Ui3CursorHitState,
    target_node: u32,
    previous_target: u32,
    entry_count: usize,
) {
    runtime.cursor_event_log_count = runtime.cursor_event_log_count.saturating_add(1);
    let target_node = if target_node != 0 {
        target_node
    } else if kind == Ui3PointerEventKind::PointerDown {
        runtime
            .host
            .last_frame()
            .map(|frame| frame.root)
            .unwrap_or(0)
    } else {
        0
    };
    let listener_count = runtime
        .host
        .node(target_node)
        .map(|node| node.listeners.len())
        .unwrap_or(0);
    let exact_listener = runtime
        .host
        .node(target_node)
        .map(|node| node.listeners.contains(&kind))
        .unwrap_or(false);
    let should_queue = target_node != 0
        && (exact_listener
            || kind == Ui3PointerEventKind::PointerDown
            || (kind == Ui3PointerEventKind::PointerMove && state.buttons != 0)
            || kind == Ui3PointerEventKind::PointerUp
            || kind == Ui3PointerEventKind::PointerUpOutside
            || kind == Ui3PointerEventKind::Wheel);
    let pointer_id = ui3_pointer_id_for_cursor(state);
    if should_queue {
        let _ = qjs::browser_task::queue_ui3_pointer_event_for_browser(
            runtime.browser_id,
            target_node,
            pointer_event_kind_name(kind),
            state.x,
            state.y,
            pointer_id,
            state.buttons,
        );
    }
    if kind != Ui3PointerEventKind::PointerMove
        || state.buttons != 0
        || runtime.cursor_event_log_count <= 96
    {
        crate::log!(
            "ui3-pixi-service: cursor-event browser={} cursor={} slot={} pointer={} kind={} x={} y={} buttons=0x{:X} target={} previous={} listeners={} exact_listener={} hit_entries={} event_count={}\n",
            runtime.browser_id,
            state.cursor_id,
            state.slot_id,
            pointer_id,
            pointer_event_kind_name(kind),
            state.x,
            state.y,
            state.buttons,
            target_node,
            previous_target,
            listener_count,
            exact_listener as u8,
            entry_count,
            runtime.cursor_event_log_count
        );
    }
}

const fn ui3_pointer_id_for_cursor(state: Ui3CursorHitState) -> u32 {
    if state.slot_id != 0 {
        state.slot_id
    } else if state.cursor_id != 0 {
        state.cursor_id
    } else {
        1
    }
}

const fn ui3_cursor_source_matches(a: Ui3CursorHitState, b: Ui3CursorHitState) -> bool {
    if a.slot_id != 0 || b.slot_id != 0 {
        a.slot_id == b.slot_id
    } else {
        a.cursor_id == b.cursor_id
    }
}

fn cursor_id_for_slot(slot_id: u32) -> u32 {
    if slot_id == 0 {
        return 1;
    }
    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons();
    for (idx, (cursor_slot_id, _, _, _)) in cursors.into_iter().enumerate() {
        if cursor_slot_id == slot_id {
            return (idx as u32).saturating_add(1);
        }
    }
    1
}

const fn pointer_event_kind_name(kind: Ui3PointerEventKind) -> &'static str {
    match kind {
        Ui3PointerEventKind::PointerDown => "pointerdown",
        Ui3PointerEventKind::PointerUp => "pointerup",
        Ui3PointerEventKind::PointerMove => "pointermove",
        Ui3PointerEventKind::PointerOver => "pointerover",
        Ui3PointerEventKind::PointerOut => "pointerout",
        Ui3PointerEventKind::PointerUpOutside => "pointerupoutside",
        Ui3PointerEventKind::ContextMenu => "contextmenu",
        Ui3PointerEventKind::Wheel => "wheel",
        Ui3PointerEventKind::Unknown => "unknown",
    }
}

fn cursor_snapshot_signature() -> u64 {
    let mut sig = 0xcbf2_9ce4_8422_2325u64;
    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons();
    sig ^= cursors.len() as u64;
    sig = sig.wrapping_mul(0x1000_0000_01b3);
    for (slot_id, x, y, buttons) in cursors {
        let qx = if x.is_finite() {
            (x.clamp(0.0, 1.0) * 65535.0) as u64
        } else {
            0
        };
        let qy = if y.is_finite() {
            (y.clamp(0.0, 1.0) * 65535.0) as u64
        } else {
            0
        };
        sig ^= slot_id as u64;
        sig = sig.wrapping_mul(0x1000_0000_01b3);
        sig ^= qx << 16 | qy;
        sig = sig.wrapping_mul(0x1000_0000_01b3);
        sig ^= buttons as u64;
        sig = sig.wrapping_mul(0x1000_0000_01b3);
    }
    sig
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
            Ui3Command::RenderDamage { root, .. } => *root,
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

    let render_damage = match &command {
        Ui3Command::RenderDamage { damage, .. } => Some(*damage),
        _ => None,
    };
    let is_render = matches!(command, Ui3Command::Render { .. } | Ui3Command::RenderDamage { .. });
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
        if let Some(damage) = render_damage {
            lower_present_damage_and_count(
                &mut runtime,
                browser_id,
                &frame,
                damage,
                apply_ms,
                is_render,
            );
        } else {
            lower_present_and_count(&mut runtime, browser_id, &frame, apply_ms, is_render);
        }
    }
    if is_render {
        runtime.scene_building = false;
        PIXI_SERVICE_SCENE_BUILDING.store(false, Ordering::Release);
        update_cursor_hits(&mut runtime);
    }
    0
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
    let geometry = lower_ui3_frame_geometry(&runtime.host, frame);
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
            "ui3-pixi-service: render browser={} root={} ops={} filtered_ops={} draws={} nodes={} hit_entries={} solid_rects={} cursor_rects={} meshes={} textures={} text={} presented={} fill_descs={} blend_descs={} apply_ms={} lower_ms={} present_wall_ms={} rect_ms={} mesh_ms={} sprite_ms={} publish_ms={} present_ms={} total_ms={} gap_ms={}\n",
            browser_id,
            frame.root,
            runtime.op_count,
            runtime.filtered_op_count,
            geometry.draws.len(),
            frame.ordered_nodes.len(),
            runtime.host.hit_scene().entries().len(),
            present.solid_rects,
            present.cursor_rects,
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
    runtime.last_geometry = Some(geometry);
}

fn lower_present_damage_and_count(
    runtime: &mut Ui3PixiServiceRuntime,
    browser_id: u32,
    frame: &Ui3RenderFrame,
    damage: Ui3Rect,
    apply_ms: u64,
    force_log: bool,
) {
    runtime.frame_count = runtime.frame_count.wrapping_add(1).max(1);
    let lower_start_ms = super::now_ms();
    let geometry = lower_ui3_frame_geometry(&runtime.host, frame);
    let lower_ms = super::now_ms().saturating_sub(lower_start_ms);
    let present_start_ms = super::now_ms();
    let present = present_ui3_frame_damage_to_intel_primary(&geometry, damage);
    let present_wall_ms = super::now_ms().saturating_sub(present_start_ms);
    let present_gap_ms = present_wall_ms.saturating_sub(present.total_ms);
    runtime.last_draw_count = geometry.draws.len().min(u32::MAX as usize) as u32;
    runtime.last_present = present;
    PIXI_SERVICE_FRAME_COUNT.store(runtime.frame_count, Ordering::Release);
    PIXI_SERVICE_DRAW_COUNT.store(runtime.last_draw_count, Ordering::Release);

    if runtime.frame_count <= 4 || force_log {
        crate::log!(
            "ui3-pixi-service: render-damage browser={} root={} damage={}x{}@{},{} ops={} filtered_ops={} cached_draws={} nodes={} hit_entries={} solid_rects={} cursor_rects={} meshes={} textures={} text={} presented={} fill_descs={} blend_descs={} apply_ms={} lower_ms={} present_wall_ms={} rect_ms={} mesh_ms={} sprite_ms={} publish_ms={} present_ms={} total_ms={} gap_ms={}\n",
            browser_id,
            frame.root,
            damage.w as i32,
            damage.h as i32,
            damage.x as i32,
            damage.y as i32,
            runtime.op_count,
            runtime.filtered_op_count,
            geometry.draws.len(),
            frame.ordered_nodes.len(),
            runtime.host.hit_scene().entries().len(),
            present.solid_rects,
            present.cursor_rects,
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
    }
    runtime.last_geometry = Some(geometry);
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
