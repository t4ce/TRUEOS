use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};
use libm::roundf;
use serde_json::Value;
use trueos_gfx_core::Rgba8;

use super::Ui3Point;

const TASK_NAME: &str = "ui3-service";
const UI3_SERVICE_IDLE_MS: u64 = 16;
const UI3_LAYOUT_TEXT_NODE_MAX: usize = 512;
const UI3_LAYOUT_PLACEMENT_MAX: usize = 4096;
const UI3_LAYOUT_RECT_NODE_MAX: usize = 512;
const UI3_LAYOUT_RECT_MAX: usize = 2048;
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
        "ui3-service: frame taken={} browser={} seq={} render_hash={} layout_hash={} render_bytes={} layout_bytes={} rect_nodes={} rects={} text_nodes={} placements={} rect_ok={} rect_ms={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} primary={}x{} url={}\n",
        taken_seq,
        frame.browser_instance_id,
        frame.seq,
        frame.render_hash,
        frame.layout_hash,
        render_bytes,
        layout_bytes,
        present.rect_nodes,
        present.rects,
        present.text_nodes,
        present.placements,
        present.rect_ok as u8,
        present.rect_ms,
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
    rect_nodes: usize,
    rects: usize,
    text_nodes: usize,
    placements: usize,
    rect_ok: bool,
    rect_ms: u64,
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

    let mut state = Ui3LayoutCollectState::default();
    collect_layout_primitives(layout, 0.0, 0.0, &mut state);

    let background_result = state.background.and_then(|background| {
        crate::intel::gpgpu::solid_rects_rgba8_over_primary(
            core::slice::from_ref(&background),
            false,
        )
    });
    let rect_result = if state.rects.is_empty() {
        None
    } else {
        crate::intel::gpgpu::solid_rects_rgba8_over_primary(
            state.rects.as_slice(),
            state.placements.is_empty(),
        )
    };
    let background_ok = if state.background.is_some() {
        background_result.map(|result| result.ok).unwrap_or(false)
    } else {
        true
    };
    let outlines_ok = if state.rects.is_empty() {
        true
    } else {
        rect_result.map(|result| result.ok).unwrap_or(false)
    };
    let rect_ok = background_ok && outlines_ok;
    let rect_ms = background_result
        .map(|result| result.total_ms)
        .unwrap_or(0)
        .saturating_add(rect_result.map(|result| result.total_ms).unwrap_or(0));

    if state.placements.is_empty() {
        return Ui3LayoutPresentResult {
            rect_nodes: state.rect_nodes,
            rects: state.rects.len(),
            text_nodes: state.text_nodes,
            rect_ok,
            rect_ms,
            presented: rect_result.map(|result| result.presented).unwrap_or(false),
            submit_ok: rect_ok,
            ..Ui3LayoutPresentResult::default()
        };
    }

    let Some(result) = crate::intel::gpgpu::sprite64_worklist_primary(
        state.placements.as_slice(),
        true,
        "ui3-layout-text-present-once",
    ) else {
        return Ui3LayoutPresentResult {
            rect_nodes: state.rect_nodes,
            rects: state.rects.len(),
            text_nodes: state.text_nodes,
            placements: state.placements.len(),
            rect_ok,
            rect_ms,
            ..Ui3LayoutPresentResult::default()
        };
    };

    Ui3LayoutPresentResult {
        rect_nodes: state.rect_nodes,
        rects: state.rects.len(),
        text_nodes: state.text_nodes,
        placements: state.placements.len(),
        rect_ok,
        rect_ms,
        presented: result.presented,
        submit_ok: rect_ok && result.ok,
        submit_ms: result.submit_ms,
        present_ms: result.present_ms,
        total_ms: result.total_ms,
        primary_width: result.primary_width,
        primary_height: result.primary_height,
    }
}

#[derive(Default)]
struct Ui3LayoutCollectState {
    background: Option<crate::intel::gpgpu::GpgpuSolidRect>,
    rect_nodes: usize,
    rects: Vec<crate::intel::gpgpu::GpgpuSolidRect>,
    text_nodes: usize,
    placements: Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
}

fn collect_layout_primitives(
    node: &Value,
    parent_x: f32,
    parent_y: f32,
    state: &mut Ui3LayoutCollectState,
) {
    if state.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX && state.rect_nodes >= UI3_LAYOUT_RECT_NODE_MAX
    {
        return;
    }

    let x = parent_x + json_f32_field(node, "x").unwrap_or(0.0);
    let y = parent_y + json_f32_field(node, "y").unwrap_or(0.0);
    let kind = node.get("kind").and_then(Value::as_str);
    if kind == Some("text") {
        if state.text_nodes < UI3_LAYOUT_TEXT_NODE_MAX
            && state.placements.len() < UI3_LAYOUT_PLACEMENT_MAX
            && let Some(text) = node.get("text").and_then(Value::as_str)
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

    if kind == Some("block") && state.rect_nodes < UI3_LAYOUT_RECT_NODE_MAX {
        collect_layout_block_rect(node, x, y, state);
    }

    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        collect_layout_primitives(child, x, y, state);
        if state.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX
            && state.rect_nodes >= UI3_LAYOUT_RECT_NODE_MAX
        {
            break;
        }
    }
}

fn collect_layout_block_rect(node: &Value, x: f32, y: f32, state: &mut Ui3LayoutCollectState) {
    let tag_name = node.get("tagName").and_then(Value::as_str).unwrap_or("div");
    let Some(rect) = layout_rect_from_node(node, x, y) else {
        return;
    };

    if tag_name == "root" {
        state.background = Some(crate::intel::gpgpu::GpgpuSolidRect {
            rect,
            color_rgba: rgba8_to_kernel_rgba(255, 255, 255, 255),
        });
        return;
    }

    state.rect_nodes = state.rect_nodes.saturating_add(1);
    let color = layout_outline_color_rgba(tag_name);
    push_rect_outline(&mut state.rects, rect, color);
}

fn layout_rect_from_node(node: &Value, x: f32, y: f32) -> Option<crate::intel::gpgpu::GpgpuRect> {
    let width = json_f32_field(node, "width")?;
    let height = json_f32_field(node, "height")?;
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return None;
    }
    Some(crate::intel::gpgpu::GpgpuRect::new(
        round_i32(x)?,
        round_i32(y)?,
        round_u32(width)?.max(1),
        round_u32(height)?.max(1),
    ))
}

fn push_rect_outline(
    out: &mut Vec<crate::intel::gpgpu::GpgpuSolidRect>,
    rect: crate::intel::gpgpu::GpgpuRect,
    color_rgba: u32,
) {
    if out.len() >= UI3_LAYOUT_RECT_MAX || rect.is_empty() {
        return;
    }

    push_solid_rect(out, rect.x, rect.y, rect.width, 1, color_rgba);
    if rect.height > 1 {
        push_solid_rect(
            out,
            rect.x,
            rect.y.saturating_add(rect.height.saturating_sub(1) as i32),
            rect.width,
            1,
            color_rgba,
        );
    }
    if rect.height > 2 {
        push_solid_rect(out, rect.x, rect.y, 1, rect.height, color_rgba);
        if rect.width > 1 {
            push_solid_rect(
                out,
                rect.x.saturating_add(rect.width.saturating_sub(1) as i32),
                rect.y,
                1,
                rect.height,
                color_rgba,
            );
        }
    }
}

fn push_solid_rect(
    out: &mut Vec<crate::intel::gpgpu::GpgpuSolidRect>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color_rgba: u32,
) {
    if out.len() >= UI3_LAYOUT_RECT_MAX || width == 0 || height == 0 {
        return;
    }
    out.push(crate::intel::gpgpu::GpgpuSolidRect {
        rect: crate::intel::gpgpu::GpgpuRect::new(x, y, width, height),
        color_rgba,
    });
}

fn layout_outline_color_rgba(tag_name: &str) -> u32 {
    match tag_name {
        "iframe" => rgba8_to_kernel_rgba(72, 112, 255, 255),
        "details" | "summary" => rgba8_to_kernel_rgba(56, 130, 210, 255),
        "table" | "tbody" | "tr" | "td" | "th" => rgba8_to_kernel_rgba(25, 135, 84, 255),
        "input" | "button" | "select" | "textarea" | "timeinput" | "dateinput" | "monthinput"
        | "weekinput" | "datetimelocalinput" | "searchbutton" => {
            rgba8_to_kernel_rgba(120, 120, 120, 255)
        }
        "svg" | "canvas" | "img" | "color" => rgba8_to_kernel_rgba(185, 75, 170, 255),
        "hr" => rgba8_to_kernel_rgba(80, 80, 80, 255),
        _ => rgba8_to_kernel_rgba(190, 190, 190, 255),
    }
}

#[inline]
const fn rgba8_to_kernel_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

fn round_i32(value: f32) -> Option<i32> {
    if value.is_finite() && value >= i32::MIN as f32 && value <= i32::MAX as f32 {
        Some(roundf(value) as i32)
    } else {
        None
    }
}

fn round_u32(value: f32) -> Option<u32> {
    if value.is_finite() && value >= 0.0 && value <= u32::MAX as f32 {
        Some(roundf(value) as u32)
    } else {
        None
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
