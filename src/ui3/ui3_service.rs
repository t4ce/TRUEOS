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

#[derive(Copy, Clone, Debug, Default)]
struct Ui3ServiceStats {
    frames_taken: u32,
    empty_polls: u32,
}

#[embassy_executor::task]
pub async fn ui3_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    crate::log!(
        "ui3-service: starting sink=render-tree-retained-image font=lucida-half mode=single-slot\n"
    );

    let mut stats = Ui3ServiceStats::default();
    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!(
                "ui3-service: stop requested frames={} empty_polls={}; exit\n",
                stats.frames_taken,
                stats.empty_polls
            );
            return;
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
            consume_render_tree_frame(frame, stats.frames_taken);
        }

        if !took_any {
            stats.empty_polls = stats.empty_polls.saturating_add(1);
            Timer::after(EmbassyDuration::from_millis(UI3_SERVICE_IDLE_MS)).await;
        }
    }
}

fn consume_render_tree_frame(frame: crate::surfer::Ui3RenderTreeFrame, taken_seq: u32) {
    let render_bytes = frame.render_tree_json.len();
    let layout_bytes = frame.layout_trace_json.len();
    let present = present_layout_text_once(&frame);
    crate::log!(
        "ui3-service: frame taken={} browser={} seq={} render_hash={} layout_hash={} render_bytes={} layout_bytes={} text_nodes={} placements={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} primary={}x{} url={}\n",
        taken_seq,
        frame.browser_instance_id,
        frame.seq,
        frame.render_hash,
        frame.layout_hash,
        render_bytes,
        layout_bytes,
        present.text_nodes,
        present.placements,
        present.presented as u8,
        present.submit_ok as u8,
        present.submit_ms,
        present.present_ms,
        present.total_ms,
        present.primary_width,
        present.primary_height,
        frame.url
    );
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3LayoutPresentResult {
    text_nodes: usize,
    placements: usize,
    presented: bool,
    submit_ok: bool,
    submit_ms: u64,
    present_ms: u64,
    total_ms: u64,
    primary_width: u32,
    primary_height: u32,
}

fn present_layout_text_once(frame: &crate::surfer::Ui3RenderTreeFrame) -> Ui3LayoutPresentResult {
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

    let mut state = Ui3LayoutTextCollectState::default();
    collect_layout_text_placements(layout, 0.0, 0.0, &mut state);
    if state.placements.is_empty() {
        return Ui3LayoutPresentResult {
            text_nodes: state.text_nodes,
            ..Ui3LayoutPresentResult::default()
        };
    }

    let Some(result) = crate::intel::gpgpu::sprite64_worklist_primary(
        state.placements.as_slice(),
        true,
        "ui3-layout-text-present-once",
    ) else {
        return Ui3LayoutPresentResult {
            text_nodes: state.text_nodes,
            placements: state.placements.len(),
            ..Ui3LayoutPresentResult::default()
        };
    };

    Ui3LayoutPresentResult {
        text_nodes: state.text_nodes,
        placements: state.placements.len(),
        presented: result.presented,
        submit_ok: result.ok,
        submit_ms: result.submit_ms,
        present_ms: result.present_ms,
        total_ms: result.total_ms,
        primary_width: result.primary_width,
        primary_height: result.primary_height,
    }
}

#[derive(Default)]
struct Ui3LayoutTextCollectState {
    text_nodes: usize,
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

fn json_f32_field(node: &Value, key: &str) -> Option<f32> {
    let number = node.get(key)?.as_f64()?;
    if number.is_finite() {
        Some(number as f32)
    } else {
        None
    }
}
