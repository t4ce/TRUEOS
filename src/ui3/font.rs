use alloc::vec::Vec;

use serde_json::Value;

const UI3_LAYOUT_TEXT_NODE_MAX: usize = 512;
const UI3_TEXT_PLACEMENT_MAX: usize = 4096;
const UI3_TEXT_SUBMIT_BATCH_PLACEMENTS: usize = 256;
const UI3_TEXT_COLOR_RGBA: u32 = 0x0000_0000;
const UI3_FLOAT_WINDOW_GRADIENT_MAX: usize = 25;
const UI3_FLOAT_WINDOW_GRADIENT_LEFT_RGBA: u32 = 0xFFAD_D8E6;
const UI3_FLOAT_WINDOW_GRADIENT_RIGHT_RGBA: u32 = 0xFFFF_FFFF;

#[derive(Debug, Default)]
pub(crate) struct Ui3FontScratch {
    placements: Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    gradients: Vec<crate::intel::gpgpu::GpgpuGradientRect>,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui3FontScene {
    pub(crate) scroll_y: f32,
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui3FontDrawResult {
    pub(crate) text_nodes: usize,
    pub(crate) placements: usize,
    pub(crate) batches: usize,
    pub(crate) gradients: usize,
    pub(crate) clipped: usize,
    pub(crate) clear_ok: bool,
    pub(crate) clear_ms: u64,
    pub(crate) rect_ms: u64,
    pub(crate) text_ms: u64,
    pub(crate) show_ms: u64,
    pub(crate) submit_ok: bool,
    pub(crate) presented: bool,
    pub(crate) submit_ms: u64,
    pub(crate) present_ms: u64,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3TextCollectStats {
    text_nodes: usize,
    clipped: usize,
    text_node_cap_hit: bool,
    placement_cap_hit: bool,
}

pub(crate) fn draw_layout_primary(
    layout: &Value,
    scene: Ui3FontScene,
    scratch: &mut Ui3FontScratch,
    present_reason: &str,
) -> Ui3FontDrawResult {
    scratch.placements.clear();
    scratch.gradients.clear();
    let mut collect = Ui3TextCollectStats::default();
    collect_layout_float_window_gradients(
        layout,
        0.0,
        0.0,
        scene.scroll_y,
        scene,
        &mut scratch.gradients,
    );
    collect_layout_text_placements(
        layout,
        0.0,
        0.0,
        scene.scroll_y,
        scene,
        &mut scratch.placements,
        &mut collect,
    );
    let gradient_cap_hit = scratch.gradients.len() >= UI3_FLOAT_WINDOW_GRADIENT_MAX;
    if gradient_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: gradient cap reached gradients={} cap={} scroll_y={} viewport={}x{}\n",
            scratch.gradients.len(),
            UI3_FLOAT_WINDOW_GRADIENT_MAX,
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height
        );
    }
    if collect.text_node_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: text-node cap reached text_nodes={} cap={} placements={} scroll_y={} viewport={}x{}\n",
            collect.text_nodes,
            UI3_LAYOUT_TEXT_NODE_MAX,
            scratch.placements.len(),
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height
        );
    }
    if collect.placement_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: placement cap reached placements={} cap={} text_nodes={} scroll_y={} viewport={}x{}\n",
            scratch.placements.len(),
            UI3_TEXT_PLACEMENT_MAX,
            collect.text_nodes,
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height
        );
    }

    let should_present_clear = scratch.placements.is_empty() && scratch.gradients.is_empty();
    let clear_result = crate::intel::gpgpu::clear_primary_rgba8_white_for_redraw_stats(
        should_present_clear,
        present_reason,
    );
    let gradient_result = if scratch.gradients.is_empty() {
        None
    } else {
        crate::intel::gpgpu::gradient_rects_rgba8_over_primary(
            scratch.gradients.as_slice(),
            scratch.placements.is_empty(),
        )
    };
    let mut text_batches = 0usize;
    let mut text_submitted = false;
    let mut text_presented = false;
    let mut text_submit_ms = 0u64;
    let mut text_present_ms = 0u64;
    if !scratch.placements.is_empty() {
        if scratch.placements.len() >= UI3_TEXT_SUBMIT_BATCH_PLACEMENTS {
            crate::log_warn!(
                target: "ui3";
                "ui3-font: sprite64 submit cap reached placements={} batch_cap={} batches={} scroll_y={} viewport={}x{}\n",
                scratch.placements.len(),
                UI3_TEXT_SUBMIT_BATCH_PLACEMENTS,
                scratch
                    .placements
                    .len()
                    .saturating_add(UI3_TEXT_SUBMIT_BATCH_PLACEMENTS - 1)
                    / UI3_TEXT_SUBMIT_BATCH_PLACEMENTS,
                scene.scroll_y as u32,
                scene.viewport_width,
                scene.viewport_height
            );
        }
        let total_batches = scratch
            .placements
            .len()
            .saturating_add(UI3_TEXT_SUBMIT_BATCH_PLACEMENTS - 1)
            / UI3_TEXT_SUBMIT_BATCH_PLACEMENTS;
        for (batch_index, chunk) in scratch
            .placements
            .chunks(UI3_TEXT_SUBMIT_BATCH_PLACEMENTS)
            .enumerate()
        {
            let present = batch_index + 1 == total_batches;
            let Some(result) =
                crate::intel::gpgpu::sprite64_worklist_primary(chunk, present, present_reason)
            else {
                continue;
            };
            text_batches = text_batches.saturating_add(1);
            text_submitted |= result.submitted;
            text_presented |= result.presented;
            text_submit_ms = text_submit_ms.saturating_add(result.submit_ms);
            text_present_ms = text_present_ms.saturating_add(result.present_ms);
        }
    }

    Ui3FontDrawResult {
        text_nodes: collect.text_nodes,
        placements: scratch.placements.len(),
        batches: text_batches,
        gradients: scratch.gradients.len(),
        clipped: collect.clipped,
        clear_ok: clear_result.is_some(),
        clear_ms: clear_result
            .as_ref()
            .map(|result| result.total_ms)
            .unwrap_or(0),
        rect_ms: gradient_result
            .as_ref()
            .map(|result| result.fill_ms.saturating_add(result.blend_ms))
            .unwrap_or(0),
        text_ms: text_submit_ms,
        show_ms: clear_result
            .as_ref()
            .map(|result| result.present_ms)
            .unwrap_or(0)
            .saturating_add(
                gradient_result
                    .as_ref()
                    .map(|result| result.present_ms)
                    .unwrap_or(0),
            )
            .saturating_add(text_present_ms),
        submit_ok: text_submitted || gradient_result.as_ref().is_some_and(|result| result.ok),
        presented: text_presented
            || gradient_result
                .as_ref()
                .is_some_and(|result| result.presented)
            || clear_result
                .as_ref()
                .is_some_and(|result| result.present_ms != 0),
        submit_ms: clear_result
            .as_ref()
            .map(|result| result.submit_ms)
            .unwrap_or(0)
            .saturating_add(
                gradient_result
                    .as_ref()
                    .map(|result| result.fill_ms.saturating_add(result.blend_ms))
                    .unwrap_or(0),
            )
            .saturating_add(text_submit_ms),
        present_ms: clear_result
            .as_ref()
            .map(|result| result.present_ms)
            .unwrap_or(0)
            .saturating_add(
                gradient_result
                    .as_ref()
                    .map(|result| result.present_ms)
                    .unwrap_or(0),
            )
            .saturating_add(text_present_ms),
    }
}

fn collect_layout_float_window_gradients(
    node: &Value,
    parent_x: f32,
    parent_y: f32,
    scroll_y: f32,
    scene: Ui3FontScene,
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
) {
    if gradients.len() >= UI3_FLOAT_WINDOW_GRADIENT_MAX {
        return;
    }
    let x = parent_x + json_f32_field(node, "x").unwrap_or(0.0);
    let y = parent_y + json_f32_field(node, "y").unwrap_or(0.0);
    if node.get("kind").and_then(Value::as_str) == Some("block")
        && node.get("tagName").and_then(Value::as_str) == Some("dialog")
    {
        push_float_window_gradient(x, y - scroll_y, node, scene, gradients);
    }

    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        collect_layout_float_window_gradients(child, x, y, scroll_y, scene, gradients);
        if gradients.len() >= UI3_FLOAT_WINDOW_GRADIENT_MAX {
            break;
        }
    }
}

fn push_float_window_gradient(
    x: f32,
    y: f32,
    node: &Value,
    scene: Ui3FontScene,
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
) {
    let Some(width) = json_u32_field(node, "width") else {
        return;
    };
    let Some(height) = json_u32_field(node, "height") else {
        return;
    };
    if width == 0 || height == 0 || y + height as f32 <= 0.0 || y >= scene.viewport_height as f32 {
        return;
    }
    if x + width as f32 <= 0.0 || x >= scene.viewport_width as f32 {
        return;
    }
    gradients.push(crate::intel::gpgpu::GpgpuGradientRect {
        rect: crate::intel::gpgpu::GpgpuRect::new(floor_i32(x), floor_i32(y), width, height),
        color0_rgba: UI3_FLOAT_WINDOW_GRADIENT_LEFT_RGBA,
        color1_rgba: UI3_FLOAT_WINDOW_GRADIENT_RIGHT_RGBA,
        vertical: false,
    });
}

fn collect_layout_text_placements(
    node: &Value,
    parent_x: f32,
    parent_y: f32,
    scroll_y: f32,
    scene: Ui3FontScene,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    if stats.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX || placements.len() >= UI3_TEXT_PLACEMENT_MAX {
        if stats.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX {
            stats.text_node_cap_hit = true;
        }
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            stats.placement_cap_hit = true;
        }
        return;
    }
    let x = parent_x + json_f32_field(node, "x").unwrap_or(0.0);
    let y = parent_y + json_f32_field(node, "y").unwrap_or(0.0);
    if node.get("kind").and_then(Value::as_str) == Some("text") {
        let Some(text) = node.get("text").and_then(Value::as_str) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        stats.text_nodes = stats.text_nodes.saturating_add(1);
        push_text_placements(text, x, y - scroll_y, scene, placements, stats);
        return;
    }

    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        collect_layout_text_placements(child, x, y, scroll_y, scene, placements, stats);
        if stats.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX
            || placements.len() >= UI3_TEXT_PLACEMENT_MAX
        {
            if stats.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX {
                stats.text_node_cap_hit = true;
            }
            if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
                stats.placement_cap_hit = true;
            }
            break;
        }
    }
}

fn push_text_placements(
    text: &str,
    x: f32,
    y: f32,
    scene: Ui3FontScene,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let face = crate::ui3::althlasfont::bitmapfont::ATHLAS_FONT_FACE_LUCIDA_HALF;
    let line_height =
        crate::ui3::althlasfont::bitmapfont::athlas_font_line_height_px(face).unwrap_or(22) as f32;
    if y + line_height < 0.0 || y > scene.viewport_height as f32 {
        stats.clipped = stats.clipped.saturating_add(text.chars().count());
        return;
    }

    let mut pen_x = x;
    for ch in text.chars() {
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            stats.placement_cap_hit = true;
            break;
        }
        if ch.is_control() {
            continue;
        }
        if ch.is_whitespace() {
            pen_x += line_height * 0.35;
            continue;
        }
        let Some(region) =
            crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, ch)
        else {
            pen_x += line_height * 0.35;
            continue;
        };
        let advance = f32::from(region.src_w.max(1)).max(line_height * 0.35);
        if pen_x + advance < 0.0 || pen_x > scene.viewport_width as f32 {
            stats.clipped = stats.clipped.saturating_add(1);
            pen_x += advance;
            continue;
        }
        let Some(slot) = crate::intel::gpgpu::sprite64_font_slot_for_region(face, region) else {
            pen_x += advance;
            continue;
        };
        placements.push(crate::intel::gpgpu::GpgpuSprite64Placement::tinted_src_over(
            slot,
            floor_i32(pen_x),
            floor_i32(y),
            UI3_TEXT_COLOR_RGBA,
        ));
        pen_x += advance;
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

fn json_u32_field(node: &Value, key: &str) -> Option<u32> {
    let number = node.get(key)?.as_u64()?;
    u32::try_from(number).ok()
}

fn floor_i32(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    libm::floorf(value).clamp(i32::MIN as f32, i32::MAX as f32) as i32
}
