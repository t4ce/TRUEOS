use alloc::{string::String, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};
use serde_json::Value;

const TASK_NAME: &str = "ui3-service";
const UI3_SERVICE_IDLE_MS: u64 = 16;
const UI3_WHEEL_EVENT_BATCH_CAP: usize = 64;
const UI3_WHEEL_SCROLL_PX_PER_NOTCH: f32 = 72.0;
const UI3_CLICK_MAX_MOVE_PX: u32 = 6;

#[derive(Copy, Clone, Debug, Default)]
struct Ui3ServiceStats {
    frames_taken: u32,
    empty_polls: u32,
}

#[derive(Clone, Debug, Default)]
struct Ui3Scene {
    frame: crate::surfer::Ui3RenderTreeFrame,
    scroll_y: f32,
    content_height: u32,
    viewport_width: u32,
    viewport_height: u32,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3LiveOverlayState {
    context_menu_open: bool,
    context_menu_x: u32,
    context_menu_y: u32,
    selection_probe_active: bool,
    selection_probe_start_x: u32,
    selection_probe_start_y: u32,
    selection_probe_current_x: u32,
    selection_probe_current_y: u32,
    last_buttons_down: u32,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3CursorInput {
    wheel_delta: i32,
    overlay_dirty: bool,
}

#[derive(Clone, Debug)]
struct Ui3ActivationHit {
    key: String,
    kind: String,
    url: String,
}

#[embassy_executor::task]
pub async fn ui3_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    crate::log!("ui3-service: starting sink=render-tree-text-primary scroll=redraw\n");

    let mut stats = Ui3ServiceStats::default();
    let mut scene = Ui3Scene::default();
    let mut cursor_events = crate::ui3::ui3_hid::Ui3CursorEventDrain::default();
    let mut live_overlay = Ui3LiveOverlayState::default();
    let mut font = crate::ui3::ui3_font::Ui3FontScratch::default();
    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!(
                "ui3-service: stop requested frames={} empty_polls={}; exit\n",
                stats.frames_taken,
                stats.empty_polls
            );
            return;
        }

        let cursor_input = drain_ui3_cursor_input(&mut cursor_events, &mut live_overlay, &scene);
        if cursor_input.overlay_dirty {
            let _ = redraw_live_overlay(&scene, &live_overlay, "ui3-live-overlay-cursor");
        }
        let wheel_delta = cursor_input.wheel_delta;
        if wheel_delta != 0 && !scene.frame.layout_trace_json.is_empty() {
            let old_scroll_y = scene.scroll_y;
            let new_scroll_y = clamp_scroll_y_for_scene(
                (old_scroll_y - (wheel_delta as f32 * UI3_WHEEL_SCROLL_PX_PER_NOTCH)).max(0.0),
                scene.content_height,
                scene.viewport_height,
            );
            if new_scroll_y == old_scroll_y {
                crate::log!(
                    "ui3-service: scroll noop reason=bounds delta={} scroll_y={} content_height={} viewport={}x{}\n",
                    wheel_delta,
                    scene.scroll_y as u32,
                    scene.content_height,
                    scene.viewport_width,
                    scene.viewport_height
                );
            } else {
                scene.scroll_y = new_scroll_y;
                redraw_scene_text(&mut scene, &mut font, 0, true);
            }
        }

        let mut took_any = false;
        for browser_instance_id in 1..=crate::surfer::MAX_BROWSER_INSTANCE_ID {
            let Some(frame) =
                crate::surfer::take_ui3_render_tree_frame_for_browser(browser_instance_id)
            else {
                continue;
            };
            took_any = true;
            stats.frames_taken = stats.frames_taken.saturating_add(1);
            scene.frame = frame;
            consume_render_tree_frame(&mut scene, stats.frames_taken, &mut font);
        }

        if !took_any {
            let asset_ready_mask = crate::surfer::take_ui3_asset_batch_ready_mask();
            if !scene.frame.layout_trace_json.is_empty()
                && browser_mask_has(asset_ready_mask, scene.frame.browser_instance_id)
            {
                let present = redraw_scene_text(&mut scene, &mut font, 0, false);
                crate::log!(
                    "ui3-service: asset batch redraw browser={} seq={} scroll_y={} content_height={} viewport={}x{} text_nodes={} placements={} gradients={} assets={} layout_shift={} embedded_scenes={} clipped={} batches={} clear_ok={} clear_ms={} rect_ms={} asset_ms={} text_ms={} show_ms={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
                    scene.frame.browser_instance_id,
                    scene.frame.seq,
                    scene.scroll_y as u32,
                    scene.content_height,
                    scene.viewport_width,
                    scene.viewport_height,
                    present.text_nodes,
                    present.placements,
                    present.gradients,
                    present.assets,
                    present.layout_shift_px,
                    present.embedded_scenes,
                    present.clipped,
                    present.batches,
                    present.clear_ok as u8,
                    present.clear_ms,
                    present.rect_ms,
                    present.asset_ms,
                    present.text_ms,
                    present.show_ms,
                    present.presented as u8,
                    present.submit_ok as u8,
                    present.submit_ms,
                    present.present_ms,
                    present.total_ms,
                    scene.frame.url
                );
            }
            stats.empty_polls = stats.empty_polls.saturating_add(1);
            Timer::after(EmbassyDuration::from_millis(UI3_SERVICE_IDLE_MS)).await;
        }
    }
}

fn consume_render_tree_frame(
    scene: &mut Ui3Scene,
    taken_seq: u32,
    font: &mut crate::ui3::ui3_font::Ui3FontScratch,
) {
    let present = redraw_scene_text(scene, font, taken_seq, false);
    let frame = &scene.frame;
    crate::log!(
        "ui3-service: frame taken={} browser={} seq={} render_hash={} layout_hash={} render_bytes={} layout_bytes={} scroll_y={} scroll_redraw=0 content_height={} viewport={}x{} text_nodes={} placements={} gradients={} assets={} layout_shift={} embedded_scenes={} clipped={} batches={} clear_ok={} clear_ms={} rect_ms={} asset_ms={} text_ms={} show_ms={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
        taken_seq,
        frame.browser_instance_id,
        frame.seq,
        frame.render_hash,
        frame.layout_hash,
        frame.render_tree_json.len(),
        frame.layout_trace_json.len(),
        scene.scroll_y as u32,
        scene.content_height,
        scene.viewport_width,
        scene.viewport_height,
        present.text_nodes,
        present.placements,
        present.gradients,
        present.assets,
        present.layout_shift_px,
        present.embedded_scenes,
        present.clipped,
        present.batches,
        present.clear_ok as u8,
        present.clear_ms,
        present.rect_ms,
        present.asset_ms,
        present.text_ms,
        present.show_ms,
        present.presented as u8,
        present.submit_ok as u8,
        present.submit_ms,
        present.present_ms,
        present.total_ms,
        frame.url
    );
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3LayoutInspectResult {
    text_nodes: usize,
    placements: usize,
    assets: usize,
    layout_shift_px: u32,
    batches: usize,
    gradients: usize,
    embedded_scenes: usize,
    clipped: usize,
    submit_ok: bool,
    presented: bool,
    clear_ok: bool,
    clear_ms: u64,
    rect_ms: u64,
    asset_ms: u64,
    text_ms: u64,
    show_ms: u64,
    submit_ms: u64,
    present_ms: u64,
    total_ms: u64,
}

fn redraw_scene_text(
    scene: &mut Ui3Scene,
    font: &mut crate::ui3::ui3_font::Ui3FontScratch,
    taken_seq: u32,
    is_scroll: bool,
) -> Ui3LayoutInspectResult {
    let total_start = embassy_time_driver::now();
    let frame = &scene.frame;
    let Ok(value) = serde_json::from_str::<Value>(frame.layout_trace_json.as_str()) else {
        crate::log!(
            "ui3-service: layout json parse failed browser={} seq={} bytes={}\n",
            frame.browser_instance_id,
            frame.seq,
            frame.layout_trace_json.len()
        );
        return Ui3LayoutInspectResult::default();
    };

    let embedded_scenes = value
        .get("trace")
        .and_then(|trace| trace.get("embeddedScenes"))
        .or_else(|| value.get("embeddedScenes"))
        .and_then(Value::as_array)
        .map(|scenes| scenes.len())
        .unwrap_or(0);
    let Some(paint_plan) = value
        .get("trace")
        .and_then(|trace| trace.get("ui3PaintPlan"))
        .or_else(|| value.get("ui3PaintPlan"))
    else {
        crate::log!(
            "ui3-service: layout json missing ui3PaintPlan browser={} seq={}\n",
            frame.browser_instance_id,
            frame.seq
        );
        return Ui3LayoutInspectResult::default();
    };

    if let Some((viewport_width, viewport_height)) = crate::intel::active_scanout_dimensions() {
        scene.viewport_width = viewport_width;
        scene.viewport_height = viewport_height;
    }
    scene.content_height = json_f32_field(paint_plan, "contentHeight")
        .map(|height| ceil_u32(height).max(scene.viewport_height))
        .unwrap_or(scene.viewport_height);
    scene.scroll_y =
        clamp_scroll_y_for_scene(scene.scroll_y, scene.content_height, scene.viewport_height);

    let font_scene = crate::ui3::ui3_font::Ui3FontScene {
        browser_instance_id: frame.browser_instance_id,
        scroll_y: scene.scroll_y,
        viewport_width: scene.viewport_width,
        viewport_height: scene.viewport_height,
    };
    let present_reason = if is_scroll {
        "ui3-text-scroll-primary"
    } else {
        "ui3-text-frame-primary"
    };
    let draw =
        crate::ui3::ui3_font::draw_paint_plan_primary(paint_plan, font_scene, font, present_reason);
    scene.content_height = scene.content_height.saturating_add(draw.layout_shift_px);
    let total_ms = elapsed_ms_since(total_start);

    if is_scroll {
        crate::log!(
            "ui3-service: scroll taken={} browser={} seq={} scroll_y={} content_height={} viewport={}x{} text_nodes={} placements={} gradients={} assets={} layout_shift={} embedded_scenes={} clipped={} batches={} clear_ok={} clear_ms={} rect_ms={} asset_ms={} text_ms={} show_ms={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
            taken_seq,
            frame.browser_instance_id,
            frame.seq,
            scene.scroll_y as u32,
            scene.content_height,
            scene.viewport_width,
            scene.viewport_height,
            draw.text_nodes,
            draw.placements,
            draw.gradients,
            draw.assets,
            draw.layout_shift_px,
            embedded_scenes,
            draw.clipped,
            draw.batches,
            draw.clear_ok as u8,
            draw.clear_ms,
            draw.rect_ms,
            draw.asset_ms,
            draw.text_ms,
            draw.show_ms,
            draw.presented as u8,
            draw.submit_ok as u8,
            draw.submit_ms,
            draw.present_ms,
            total_ms,
            frame.url
        );
    }

    Ui3LayoutInspectResult {
        text_nodes: draw.text_nodes,
        placements: draw.placements,
        assets: draw.assets,
        layout_shift_px: draw.layout_shift_px,
        batches: draw.batches,
        gradients: draw.gradients,
        embedded_scenes,
        clipped: draw.clipped,
        submit_ok: draw.submit_ok,
        presented: draw.presented,
        clear_ok: draw.clear_ok,
        clear_ms: draw.clear_ms,
        rect_ms: draw.rect_ms,
        asset_ms: draw.asset_ms,
        text_ms: draw.text_ms,
        show_ms: draw.show_ms,
        submit_ms: draw.submit_ms,
        present_ms: draw.present_ms,
        total_ms: elapsed_ms_since(total_start),
    }
}

fn json_f32_field(node: &Value, key: &str) -> Option<f32> {
    let number = node.get(key)?.as_f64()?;
    if number.is_finite() {
        Some(number as f32)
    } else {
        None
    }
}

fn json_string_field(node: &Value, key: &str) -> Option<String> {
    node.get(key).and_then(Value::as_str).map(String::from)
}

fn clamp_scroll_y_for_scene(scroll_y: f32, content_height: u32, viewport_height: u32) -> f32 {
    if scroll_y <= 0.0 || content_height <= viewport_height {
        return 0.0;
    }
    scroll_y.min(content_height.saturating_sub(viewport_height) as f32)
}

fn browser_mask_has(mask: u64, browser_instance_id: u32) -> bool {
    if browser_instance_id == 0 || browser_instance_id > 64 {
        return false;
    }
    (mask & (1u64 << browser_instance_id.saturating_sub(1))) != 0
}

fn ceil_u32(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    libm::ceilf(value).min(u32::MAX as f32) as u32
}

fn elapsed_ms_since(start: u64) -> u64 {
    let now = embassy_time_driver::now();
    let ticks = now.saturating_sub(start);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
    }
}

fn drain_ui3_cursor_input(
    state: &mut crate::ui3::ui3_hid::Ui3CursorEventDrain,
    live_overlay: &mut Ui3LiveOverlayState,
    scene: &Ui3Scene,
) -> Ui3CursorInput {
    let mut out = [crate::usb2::hid::TrueosHidCursorEvent::default(); UI3_WHEEL_EVENT_BATCH_CAP];
    let read = crate::ui3::ui3_hid::drain_cursor_events(state, out.as_mut_slice());
    let mut input = Ui3CursorInput::default();
    let (viewport_width, viewport_height) = ui3_overlay_viewport(scene);
    for event in out.iter().take(read.wrote) {
        input.wheel_delta = input
            .wheel_delta
            .saturating_add(crate::ui3::ui3_hid::event_wheel_delta(*event));
        if (event.flags & crate::ui3::ui3_hid::UI3_CURSOR_EVENT_FLAG_MOTION) != 0 {
            let (x, y) =
                crate::ui3::ui3_hid::event_position_px(*event, viewport_width, viewport_height);
            if live_overlay.selection_probe_active
                && (event.buttons_down & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_LEFT) != 0
            {
                live_overlay.selection_probe_current_x = x;
                live_overlay.selection_probe_current_y = y;
            }
            input.overlay_dirty = true;
        }
        if crate::ui3::ui3_hid::event_has_button_change(*event) {
            let was_right = (live_overlay.last_buttons_down
                & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_RIGHT)
                != 0;
            let was_left =
                (live_overlay.last_buttons_down & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_LEFT) != 0;
            let is_right = crate::ui3::ui3_hid::event_has_right_button(*event);
            let is_left = (event.buttons_down & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_LEFT) != 0;
            if is_right && !was_right {
                let (x, y) =
                    crate::ui3::ui3_hid::event_position_px(*event, viewport_width, viewport_height);
                live_overlay.context_menu_open = true;
                live_overlay.selection_probe_active = false;
                live_overlay.context_menu_x = x;
                live_overlay.context_menu_y = y;
                input.overlay_dirty = true;
            } else if live_overlay.context_menu_open && is_left {
                live_overlay.context_menu_open = false;
                input.overlay_dirty = true;
            } else if is_left && !was_left {
                let (x, y) =
                    crate::ui3::ui3_hid::event_position_px(*event, viewport_width, viewport_height);
                live_overlay.selection_probe_active = true;
                live_overlay.selection_probe_start_x = x;
                live_overlay.selection_probe_start_y = y;
                live_overlay.selection_probe_current_x = x;
                live_overlay.selection_probe_current_y = y;
                input.overlay_dirty = true;
            } else if !is_left && was_left && live_overlay.selection_probe_active {
                let (x, y) =
                    crate::ui3::ui3_hid::event_position_px(*event, viewport_width, viewport_height);
                let _ = activate_ui3_click_if_any(scene, live_overlay, x, y);
                live_overlay.selection_probe_active = false;
                input.overlay_dirty = true;
            }
            live_overlay.last_buttons_down = event.buttons_down;
        }
    }
    if input.wheel_delta != 0 {
        crate::log!(
            "ui3-service: wheel events={} dropped={} delta={} read_seq={}\n",
            read.wrote,
            read.dropped,
            input.wheel_delta,
            read.next_seq
        );
    }
    input
}

fn activate_ui3_click_if_any(
    scene: &Ui3Scene,
    live_overlay: &Ui3LiveOverlayState,
    release_x: u32,
    release_y: u32,
) -> bool {
    if !ui3_click_within_threshold(
        live_overlay.selection_probe_start_x,
        live_overlay.selection_probe_start_y,
        release_x,
        release_y,
    ) {
        return false;
    }

    let Some(press_hit) = ui3_activation_hit_at(
        scene,
        live_overlay.selection_probe_start_x,
        live_overlay.selection_probe_start_y,
    ) else {
        return false;
    };
    let Some(release_hit) = ui3_activation_hit_at(scene, release_x, release_y) else {
        return false;
    };
    if press_hit.key != release_hit.key || press_hit.kind != release_hit.kind {
        return false;
    }

    if release_hit.kind == "navigate" && !release_hit.url.is_empty() {
        let queued = crate::surfer::queue_browser_navigation(
            scene.frame.browser_instance_id,
            release_hit.url.as_str(),
        );
        crate::log!(
            "ui3-service: activate kind=navigate queued={} browser={} key={} url={}\n",
            if queued { 1 } else { 0 },
            scene.frame.browser_instance_id,
            release_hit.key,
            release_hit.url
        );
        return queued;
    }

    false
}

fn ui3_click_within_threshold(start_x: u32, start_y: u32, end_x: u32, end_y: u32) -> bool {
    start_x.abs_diff(end_x) <= UI3_CLICK_MAX_MOVE_PX
        && start_y.abs_diff(end_y) <= UI3_CLICK_MAX_MOVE_PX
}

fn ui3_activation_hit_at(
    scene: &Ui3Scene,
    screen_x: u32,
    screen_y: u32,
) -> Option<Ui3ActivationHit> {
    if scene.frame.layout_trace_json.is_empty() {
        return None;
    }
    let value = serde_json::from_str::<Value>(scene.frame.layout_trace_json.as_str()).ok()?;
    let paint_plan = value
        .get("trace")
        .and_then(|trace| trace.get("ui3PaintPlan"))
        .or_else(|| value.get("ui3PaintPlan"))?;
    let hit_boxes = paint_plan.get("hitBoxes").and_then(Value::as_array)?;
    let content_x = screen_x as f32;
    let content_y = screen_y as f32 + scene.scroll_y;
    let mut best: Option<Ui3ActivationHit> = None;
    for hit in hit_boxes {
        let x = json_f32_field(hit, "x").unwrap_or(0.0);
        let y = json_f32_field(hit, "y").unwrap_or(0.0);
        let width = json_f32_field(hit, "width").unwrap_or(0.0);
        let height = json_f32_field(hit, "height").unwrap_or(0.0);
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        if content_x < x || content_y < y || content_x >= x + width || content_y >= y + height {
            continue;
        }
        let Some(activation) = hit.get("activation").and_then(Value::as_object) else {
            continue;
        };
        let kind = activation
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if kind.is_empty() {
            continue;
        }
        let url = activation
            .get("resolvedHref")
            .or_else(|| activation.get("href"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        best = Some(Ui3ActivationHit {
            key: json_string_field(hit, "key").unwrap_or_default(),
            kind: String::from(kind),
            url: String::from(url),
        });
    }
    best
}

fn redraw_live_overlay(scene: &Ui3Scene, state: &Ui3LiveOverlayState, reason: &str) -> bool {
    let (viewport_width, viewport_height) = ui3_overlay_viewport(scene);
    if viewport_width == 0 || viewport_height == 0 {
        return false;
    }
    let mut rects: Vec<crate::intel::LiveOverlayRect> = Vec::new();
    if state.context_menu_open {
        crate::ui3::ui3_hid::push_context_menu_rects(
            &mut rects,
            state.context_menu_x,
            state.context_menu_y,
            viewport_width,
            viewport_height,
        );
    }
    if state.selection_probe_active {
        crate::ui3::ui3_hid::push_drag_selection_probe_rects(
            &mut rects,
            state.selection_probe_start_x,
            state.selection_probe_start_y,
            state.selection_probe_current_x,
            state.selection_probe_current_y,
        );
    }
    crate::ui3::ui3_hid::push_software_cursor_rects(&mut rects, viewport_width, viewport_height);
    let preserve = crate::ui3::ui3_canvas::live_overlay_preserve_rect(rects.as_slice());
    crate::intel::present_live_overlay_rects_preserving(rects.as_slice(), preserve, reason)
}

fn ui3_overlay_viewport(scene: &Ui3Scene) -> (u32, u32) {
    if scene.viewport_width != 0 && scene.viewport_height != 0 {
        return (scene.viewport_width, scene.viewport_height);
    }
    crate::intel::active_scanout_dimensions().unwrap_or((0, 0))
}
