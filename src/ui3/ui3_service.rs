use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};
use serde_json::Value;
use trueos_gfx_core::Rgba8;

use super::Ui3Point;

const TASK_NAME: &str = "ui3-service";
const UI3_SERVICE_IDLE_MS: u64 = 16;
const UI3_LAYOUT_TEXT_NODE_MAX: usize = 512;
const UI3_LAYOUT_PLACEMENT_MAX: usize = 4096;
const UI3_LAYOUT_FONT_TIER_HALF: u8 = 1;
const UI3_LAYOUT_TEXT_COLOR: Rgba8 = Rgba8 {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};
const UI3_LAYOUT_SPRITE64_BATCH_MAX: usize = super::font::gpgpu_font::UI3_FONT_SPRITE64_BATCH_MAX;
const UI3_WHEEL_EVENT_BATCH_CAP: usize = 64;
const UI3_WHEEL_SCROLL_PX_PER_NOTCH: f32 = 72.0;

#[derive(Copy, Clone, Debug, Default)]
struct Ui3ServiceStats {
    frames_taken: u32,
    empty_polls: u32,
}

#[derive(Clone, Debug, Default)]
struct Ui3RetainedTextScene {
    frame: crate::surfer::Ui3RenderTreeFrame,
    scroll_y: f32,
    content_height: u32,
    viewport_width: u32,
    viewport_height: u32,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3WheelDrain {
    read_seq: u64,
}

#[embassy_executor::task]
pub async fn ui3_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    crate::log!(
        "ui3-service: starting sink=render-tree-text-primary font=lucida-half mode=single-slot scroll=wheel-redraw\n"
    );

    let mut stats = Ui3ServiceStats::default();
    let mut scene = Ui3RetainedTextScene::default();
    let mut wheel = Ui3WheelDrain::default();
    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!(
                "ui3-service: stop requested frames={} empty_polls={}; exit\n",
                stats.frames_taken,
                stats.empty_polls
            );
            return;
        }

        let wheel_delta = drain_ui3_wheel_delta(&mut wheel);
        let mut redraw_for_scroll = false;
        if wheel_delta != 0 && !scene.frame.layout_trace_json.is_empty() {
            scene.scroll_y =
                (scene.scroll_y - (wheel_delta as f32 * UI3_WHEEL_SCROLL_PX_PER_NOTCH)).max(0.0);
            redraw_for_scroll = true;
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
            consume_render_tree_frame(&mut scene, stats.frames_taken, false);
        }

        if redraw_for_scroll && !took_any {
            consume_render_tree_frame(&mut scene, stats.frames_taken, true);
        }

        if !took_any {
            stats.empty_polls = stats.empty_polls.saturating_add(1);
            Timer::after(EmbassyDuration::from_millis(UI3_SERVICE_IDLE_MS)).await;
        }
    }
}

fn consume_render_tree_frame(
    scene: &mut Ui3RetainedTextScene,
    taken_seq: u32,
    scroll_redraw: bool,
) {
    let present = redraw_layout_text_primary(scene, scroll_redraw);
    let frame = &scene.frame;
    crate::log!(
        "ui3-service: frame taken={} browser={} seq={} render_hash={} layout_hash={} render_bytes={} layout_bytes={} scroll_y={} scroll_redraw={} content_height={} viewport={}x{} text_nodes={} placements={} clipped={} skipped_containers={} skipped_text_nodes={} batches={} clear_ok={} clear_ms={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
        taken_seq,
        frame.browser_instance_id,
        frame.seq,
        frame.render_hash,
        frame.layout_hash,
        frame.render_tree_json.len(),
        frame.layout_trace_json.len(),
        scene.scroll_y as u32,
        scroll_redraw as u8,
        scene.content_height,
        scene.viewport_width,
        scene.viewport_height,
        present.text_nodes,
        present.placements,
        present.clipped,
        present.skipped_containers,
        present.skipped_text_nodes,
        present.batches,
        present.clear_ok as u8,
        present.clear_ms,
        present.presented as u8,
        present.submit_ok as u8,
        present.submit_ms,
        present.present_ms,
        present.total_ms,
        frame.url
    );
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3LayoutPresentResult {
    text_nodes: usize,
    placements: usize,
    clipped: usize,
    skipped_containers: usize,
    skipped_text_nodes: usize,
    batches: usize,
    clear_ok: bool,
    clear_ms: u64,
    presented: bool,
    submit_ok: bool,
    submit_ms: u64,
    present_ms: u64,
    total_ms: u64,
}

fn redraw_layout_text_primary(
    scene: &mut Ui3RetainedTextScene,
    scroll_redraw: bool,
) -> Ui3LayoutPresentResult {
    let total_start = embassy_time_driver::now();
    let frame = &scene.frame;
    let Ok(value) = serde_json::from_str::<Value>(frame.layout_trace_json.as_str()) else {
        crate::log!(
            "ui3-service: layout json parse failed browser={} seq={} bytes={}\n",
            frame.browser_instance_id,
            frame.seq,
            frame.layout_trace_json.len()
        );
        return Ui3LayoutPresentResult::default();
    };

    let Some(layout) = value
        .get("trace")
        .and_then(|trace| trace.get("layout"))
        .or_else(|| value.get("layout"))
    else {
        crate::log!(
            "ui3-service: layout json missing layout browser={} seq={}\n",
            frame.browser_instance_id,
            frame.seq
        );
        return Ui3LayoutPresentResult::default();
    };

    let Some((max_dst_x, max_dst_y)) = crate::intel::gpgpu::sprite64_primary_draw_bounds() else {
        return Ui3LayoutPresentResult::default();
    };
    scene.viewport_width = max_dst_x.saturating_add(64) as u32;
    scene.viewport_height = max_dst_y.saturating_add(64) as u32;
    scene.content_height = json_f32_field(layout, "height")
        .map(|height| ceil_u32(height).max(scene.viewport_height))
        .unwrap_or(scene.viewport_height);
    scene.scroll_y =
        clamp_scroll_y_for_scene(scene.scroll_y, scene.content_height, scene.viewport_height);

    let mut state = Ui3LayoutTextCollectState::default();
    collect_layout_text_placements(layout, 0.0, -scene.scroll_y, &mut state);
    let collected_placements = state.placements.len();
    state
        .placements
        .retain(|placement| sprite64_placement_inside_primary(placement, max_dst_x, max_dst_y));
    let clipped = collected_placements.saturating_sub(state.placements.len());

    let clear = crate::intel::gpgpu::clear_primary_rgba8_white_stats();
    let clear_ok = clear.as_ref().is_some_and(|stats| stats.submits > 0);
    let clear_ms = clear.as_ref().map(|stats| stats.total_ms).unwrap_or(0);
    if state.placements.is_empty() {
        log_layout_text_skips(frame, &state);
        return Ui3LayoutPresentResult {
            text_nodes: state.text_nodes,
            placements: collected_placements,
            clipped,
            skipped_containers: state.skipped_containers,
            skipped_text_nodes: state.skipped_text_nodes,
            clear_ok,
            clear_ms,
            presented: clear_ok,
            submit_ok: clear_ok,
            total_ms: elapsed_ms_since(total_start),
            ..Ui3LayoutPresentResult::default()
        };
    }

    let Some(draw) = submit_primary_text_placements(state.placements.as_slice(), scroll_redraw)
    else {
        log_layout_text_skips(frame, &state);
        return Ui3LayoutPresentResult {
            text_nodes: state.text_nodes,
            placements: collected_placements,
            clipped,
            skipped_containers: state.skipped_containers,
            skipped_text_nodes: state.skipped_text_nodes,
            clear_ok,
            clear_ms,
            submit_ok: false,
            total_ms: elapsed_ms_since(total_start),
            ..Ui3LayoutPresentResult::default()
        };
    };

    log_layout_text_skips(frame, &state);
    Ui3LayoutPresentResult {
        text_nodes: state.text_nodes,
        placements: collected_placements,
        clipped,
        skipped_containers: state.skipped_containers,
        skipped_text_nodes: state.skipped_text_nodes,
        batches: draw.batches,
        clear_ok,
        clear_ms,
        presented: draw.presented,
        submit_ok: clear_ok && draw.submit_ok,
        submit_ms: draw.submit_ms,
        present_ms: draw.present_ms,
        total_ms: elapsed_ms_since(total_start),
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3LayoutTextSubmitResult {
    batches: usize,
    presented: bool,
    submit_ok: bool,
    submit_ms: u64,
    present_ms: u64,
}

fn submit_primary_text_placements(
    placements: &[crate::intel::gpgpu::GpgpuSprite64Placement],
    scroll_redraw: bool,
) -> Option<Ui3LayoutTextSubmitResult> {
    if placements.is_empty() {
        return None;
    }

    let batch_count =
        (placements.len() + UI3_LAYOUT_SPRITE64_BATCH_MAX - 1) / UI3_LAYOUT_SPRITE64_BATCH_MAX;
    let mut aggregate = Ui3LayoutTextSubmitResult {
        submit_ok: true,
        ..Ui3LayoutTextSubmitResult::default()
    };

    for (batch_index, batch) in placements.chunks(UI3_LAYOUT_SPRITE64_BATCH_MAX).enumerate() {
        let present = batch_index + 1 == batch_count;
        let reason = if scroll_redraw {
            "ui3-layout-text-scroll"
        } else {
            "ui3-layout-text-present"
        };
        let result = crate::intel::gpgpu::sprite64_worklist_primary(batch, present, reason)?;
        aggregate.batches = aggregate.batches.saturating_add(1);
        aggregate.presented |= present && result.presented;
        aggregate.submit_ok &= result.ok;
        aggregate.submit_ms = aggregate.submit_ms.saturating_add(result.submit_ms);
        aggregate.present_ms = aggregate.present_ms.saturating_add(result.present_ms);
    }

    Some(aggregate)
}

fn sprite64_placement_inside_primary(
    placement: &crate::intel::gpgpu::GpgpuSprite64Placement,
    max_dst_x: i32,
    max_dst_y: i32,
) -> bool {
    let x = placement.dst_x();
    let y = placement.dst_y();
    x >= 0 && y >= 0 && x <= max_dst_x && y <= max_dst_y
}

#[derive(Default)]
struct Ui3LayoutTextCollectState {
    text_nodes: usize,
    skipped_containers: usize,
    skipped_text_nodes: usize,
    placements: Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
}

fn collect_layout_text_placements(
    node: &Value,
    parent_x: f32,
    parent_y: f32,
    state: &mut Ui3LayoutTextCollectState,
) {
    if state.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX
        || state.placements.len() >= UI3_LAYOUT_PLACEMENT_MAX
    {
        return;
    }

    let x = parent_x + json_f32_field(node, "x").unwrap_or(0.0);
    let y = parent_y + json_f32_field(node, "y").unwrap_or(0.0);
    if should_skip_layout_text_container(node) {
        state.skipped_containers = state.skipped_containers.saturating_add(1);
        state.skipped_text_nodes = state
            .skipped_text_nodes
            .saturating_add(count_text_nodes_in_layout_subtree(node));
        return;
    }

    if node.get("kind").and_then(Value::as_str) == Some("text") {
        if let Some(text) = node.get("text").and_then(Value::as_str)
            && !text.is_empty()
        {
            state.text_nodes = state.text_nodes.saturating_add(1);
            super::font::gpgpu_font::append_ui3_text_run_sprite64_placements(
                &mut state.placements,
                Ui3Point { x, y },
                text,
                UI3_LAYOUT_TEXT_COLOR,
                UI3_LAYOUT_FONT_TIER_HALF,
            );
            if state.placements.len() > UI3_LAYOUT_PLACEMENT_MAX {
                state.placements.truncate(UI3_LAYOUT_PLACEMENT_MAX);
            }
        }
        return;
    }

    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        collect_layout_text_placements(child, x, y, state);
        if state.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX
            || state.placements.len() >= UI3_LAYOUT_PLACEMENT_MAX
        {
            break;
        }
    }
}

fn should_skip_layout_text_container(node: &Value) -> bool {
    let tag_name = node.get("tagName").and_then(Value::as_str).unwrap_or("");
    if tag_name == "dialog" {
        return true;
    }
    if tag_name != "iframe" {
        return false;
    }

    node.get("key").and_then(Value::as_str) != Some("root:internal-iframe")
}

fn count_text_nodes_in_layout_subtree(node: &Value) -> usize {
    let mut total = 0usize;
    if node.get("kind").and_then(Value::as_str) == Some("text")
        && node
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
    {
        total = total.saturating_add(1);
    }

    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            total = total.saturating_add(count_text_nodes_in_layout_subtree(child));
        }
    }
    total
}

fn log_layout_text_skips(
    frame: &crate::surfer::Ui3RenderTreeFrame,
    state: &Ui3LayoutTextCollectState,
) {
    if state.skipped_containers == 0 {
        return;
    }
    crate::log!(
        "ui3-service: TODO skipped floating/embedded layout text browser={} seq={} containers={} text_nodes={} reason=needs-window-plane-or-embedded-surface-concept\n",
        frame.browser_instance_id,
        frame.seq,
        state.skipped_containers,
        state.skipped_text_nodes
    );
}

fn json_f32_field(node: &Value, key: &str) -> Option<f32> {
    let number = node.get(key)?.as_f64()?;
    if number.is_finite() {
        Some(number as f32)
    } else {
        None
    }
}

fn clamp_scroll_y_for_scene(scroll_y: f32, content_height: u32, viewport_height: u32) -> f32 {
    if scroll_y <= 0.0 || content_height <= viewport_height {
        return 0.0;
    }
    scroll_y.min(content_height.saturating_sub(viewport_height) as f32)
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

fn drain_ui3_wheel_delta(state: &mut Ui3WheelDrain) -> i32 {
    let mut out = [crate::usb2::hid::TrueosHidCursorEvent::default(); UI3_WHEEL_EVENT_BATCH_CAP];
    let (next_seq, dropped, wrote) =
        crate::usb2::hid::read_cursor_events_since(state.read_seq, out.as_mut_slice());
    state.read_seq = next_seq;
    let wheel_delta = out
        .iter()
        .take(wrote)
        .fold(0i32, |sum, event| sum.saturating_add(event.wheel as i32));
    if wheel_delta != 0 {
        crate::log!(
            "ui3-service: wheel events={} dropped={} delta={} read_seq={}\n",
            wrote,
            dropped,
            wheel_delta,
            state.read_seq
        );
    }
    wheel_delta
}
