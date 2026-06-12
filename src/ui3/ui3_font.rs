use alloc::vec::Vec;

use serde_json::Value;

const UI3_LAYOUT_TEXT_NODE_MAX: usize = 512;
const UI3_TEXT_PLACEMENT_MAX: usize = 4096;
const UI3_TEXT_SUBMIT_BATCH_PLACEMENTS: usize = 256;
const UI3_TEXT_COLOR_RGBA: u32 = 0x0000_0000;
const UI3_PAINTED_BOX_MAX: usize = 25;
const UI3_GRADIENT_DESCS_PER_PAINTED_BOX_MAX: usize = 5;
const UI3_GRADIENT_DESC_MAX: usize = UI3_PAINTED_BOX_MAX * UI3_GRADIENT_DESCS_PER_PAINTED_BOX_MAX;

#[derive(Debug, Default)]
pub(crate) struct Ui3FontScratch {
    placements: Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    gradients: Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    control_gradients: Vec<crate::intel::gpgpu::GpgpuGradientRect>,
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

#[derive(Copy, Clone, Debug, Default)]
struct Ui3GradientCollectStats {
    painted_boxes: usize,
    painted_box_cap_hit: bool,
    gradient_desc_cap_hit: bool,
}

pub(crate) fn draw_layout_primary(
    layout: &Value,
    scene: Ui3FontScene,
    scratch: &mut Ui3FontScratch,
    present_reason: &str,
) -> Ui3FontDrawResult {
    scratch.placements.clear();
    scratch.gradients.clear();
    scratch.control_gradients.clear();
    let mut collect = Ui3TextCollectStats::default();
    let mut gradient_collect = Ui3GradientCollectStats::default();
    collect_layout_rect_gradients(
        layout,
        0.0,
        0.0,
        scene.scroll_y,
        scene,
        &mut scratch.gradients,
        &mut scratch.control_gradients,
        &mut gradient_collect,
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
    let gradient_count = scratch
        .gradients
        .len()
        .saturating_add(scratch.control_gradients.len());
    if gradient_collect.painted_box_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: painted-box cap reached boxes={} cap={} gradients={} scroll_y={} viewport={}x{}\n",
            gradient_collect.painted_boxes,
            UI3_PAINTED_BOX_MAX,
            gradient_count,
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height
        );
    }
    if gradient_collect.gradient_desc_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: gradient-desc cap reached gradients={} cap={} boxes={} scroll_y={} viewport={}x{}\n",
            gradient_count,
            UI3_GRADIENT_DESC_MAX,
            gradient_collect.painted_boxes,
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

    let should_present_clear = scratch.placements.is_empty()
        && scratch.gradients.is_empty()
        && scratch.control_gradients.is_empty();
    let clear_result = crate::intel::gpgpu::clear_primary_rgba8_white_for_redraw_stats(
        should_present_clear,
        present_reason,
    );
    let gradient_result = if scratch.gradients.is_empty() {
        None
    } else {
        crate::intel::gpgpu::gradient_rects_rgba8_over_primary(
            scratch.gradients.as_slice(),
            scratch.placements.is_empty() && scratch.control_gradients.is_empty(),
        )
    };
    let control_gradient_result = if scratch.control_gradients.is_empty() {
        None
    } else {
        crate::intel::gpgpu::gradient_rects_rgba8_over_primary(
            scratch.control_gradients.as_slice(),
            scratch.placements.is_empty(),
        )
    };
    let mut text_batches = 0usize;
    let mut text_submitted = false;
    let mut text_presented = false;
    let mut text_submit_ms = 0u64;
    let mut text_present_ms = 0u64;
    if !scratch.placements.is_empty() {
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
        gradients: gradient_count,
        clipped: collect.clipped,
        clear_ok: clear_result.is_some(),
        clear_ms: clear_result
            .as_ref()
            .map(|result| result.total_ms)
            .unwrap_or(0),
        rect_ms: gradient_result
            .as_ref()
            .map(|result| result.fill_ms.saturating_add(result.blend_ms))
            .unwrap_or(0)
            .saturating_add(
                control_gradient_result
                    .as_ref()
                    .map(|result| result.fill_ms.saturating_add(result.blend_ms))
                    .unwrap_or(0),
            ),
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
            .saturating_add(
                control_gradient_result
                    .as_ref()
                    .map(|result| result.present_ms)
                    .unwrap_or(0),
            )
            .saturating_add(text_present_ms),
        submit_ok: text_submitted
            || gradient_result.as_ref().is_some_and(|result| result.ok)
            || control_gradient_result
                .as_ref()
                .is_some_and(|result| result.ok),
        presented: text_presented
            || gradient_result
                .as_ref()
                .is_some_and(|result| result.presented)
            || control_gradient_result
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
            .saturating_add(
                control_gradient_result
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
            .saturating_add(
                control_gradient_result
                    .as_ref()
                    .map(|result| result.present_ms)
                    .unwrap_or(0),
            )
            .saturating_add(text_present_ms),
    }
}

fn collect_layout_rect_gradients(
    node: &Value,
    parent_x: f32,
    parent_y: f32,
    scroll_y: f32,
    scene: Ui3FontScene,
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    control_gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    stats: &mut Ui3GradientCollectStats,
) {
    if stats.painted_boxes >= UI3_PAINTED_BOX_MAX {
        stats.painted_box_cap_hit = true;
        return;
    }
    if layout_gradient_count(gradients, control_gradients) >= UI3_GRADIENT_DESC_MAX {
        stats.gradient_desc_cap_hit = true;
        return;
    }
    let x = parent_x + json_f32_field(node, "x").unwrap_or(0.0);
    let y = parent_y + json_f32_field(node, "y").unwrap_or(0.0);
    if node.get("kind").and_then(Value::as_str) == Some("block") {
        match layout_paint_role(node) {
            Some("dialog") => {
                let remaining = UI3_GRADIENT_DESC_MAX
                    .saturating_sub(layout_gradient_count(gradients, control_gradients));
                if push_painted_box_gradients(x, y - scroll_y, node, scene, remaining, gradients)
                    != 0
                {
                    stats.painted_boxes = stats.painted_boxes.saturating_add(1);
                }
            }
            Some("button") | Some("iframe") | Some("table") | Some("table-cell") => {
                let remaining = UI3_GRADIENT_DESC_MAX
                    .saturating_sub(layout_gradient_count(gradients, control_gradients));
                if push_painted_box_gradients(
                    x,
                    y - scroll_y,
                    node,
                    scene,
                    remaining,
                    control_gradients,
                ) != 0
                {
                    stats.painted_boxes = stats.painted_boxes.saturating_add(1);
                }
            }
            _ => {}
        }
        if stats.painted_boxes >= UI3_PAINTED_BOX_MAX {
            stats.painted_box_cap_hit = true;
            return;
        }
        if layout_gradient_count(gradients, control_gradients) >= UI3_GRADIENT_DESC_MAX {
            stats.gradient_desc_cap_hit = true;
            return;
        }
    }

    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        collect_layout_rect_gradients(
            child,
            x,
            y,
            scroll_y,
            scene,
            gradients,
            control_gradients,
            stats,
        );
        if stats.painted_boxes >= UI3_PAINTED_BOX_MAX {
            stats.painted_box_cap_hit = true;
            break;
        }
        if layout_gradient_count(gradients, control_gradients) >= UI3_GRADIENT_DESC_MAX {
            stats.gradient_desc_cap_hit = true;
            break;
        }
    }
}

fn layout_gradient_count(
    gradients: &[crate::intel::gpgpu::GpgpuGradientRect],
    control_gradients: &[crate::intel::gpgpu::GpgpuGradientRect],
) -> usize {
    gradients.len().saturating_add(control_gradients.len())
}

fn layout_paint_role(node: &Value) -> Option<&str> {
    node.get("paint")?.get("role")?.as_str()
}

fn push_painted_box_gradients(
    x: f32,
    y: f32,
    node: &Value,
    scene: Ui3FontScene,
    remaining: usize,
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
) -> usize {
    if remaining == 0 {
        return 0;
    }
    let Some(width) = json_u32_field(node, "width") else {
        return 0;
    };
    let Some(height) = json_u32_field(node, "height") else {
        return 0;
    };
    if width == 0 || height == 0 || y + height as f32 <= 0.0 || y >= scene.viewport_height as f32 {
        return 0;
    }
    if x + width as f32 <= 0.0 || x >= scene.viewport_width as f32 {
        return 0;
    }
    let Some(paint) = node.get("paint") else {
        return 0;
    };
    let border_width = json_u32_field(paint, "borderWidth")
        .unwrap_or(0)
        .min(width / 2)
        .min(height / 2);

    let mut pushed = 0usize;
    if border_width > 0 {
        let Some(border_color) = json_rgb24_field(paint, "borderColor").map(rgb24_to_rgba8_word)
        else {
            return 0;
        };
        pushed = pushed.saturating_add(push_solid_gradient_rect(
            gradients,
            remaining.saturating_sub(pushed),
            floor_i32(x),
            floor_i32(y),
            width,
            border_width,
            border_color,
        ));
        pushed = pushed.saturating_add(push_solid_gradient_rect(
            gradients,
            remaining.saturating_sub(pushed),
            floor_i32(x),
            floor_i32(y + (height - border_width) as f32),
            width,
            border_width,
            border_color,
        ));
        pushed = pushed.saturating_add(push_solid_gradient_rect(
            gradients,
            remaining.saturating_sub(pushed),
            floor_i32(x),
            floor_i32(y + border_width as f32),
            border_width,
            height.saturating_sub(border_width.saturating_mul(2)),
            border_color,
        ));
        pushed = pushed.saturating_add(push_solid_gradient_rect(
            gradients,
            remaining.saturating_sub(pushed),
            floor_i32(x + (width - border_width) as f32),
            floor_i32(y + border_width as f32),
            border_width,
            height.saturating_sub(border_width.saturating_mul(2)),
            border_color,
        ));
    }

    let Some(color0) = json_rgb24_field(paint, "color0").map(rgb24_to_rgba8_word) else {
        return pushed;
    };
    let color1 = json_rgb24_field(paint, "color1")
        .map(rgb24_to_rgba8_word)
        .unwrap_or(color0);
    let fill_x = x + border_width as f32;
    let fill_y = y + border_width as f32;
    let fill_width = width.saturating_sub(border_width.saturating_mul(2));
    let fill_height = height.saturating_sub(border_width.saturating_mul(2));
    if fill_width == 0 || fill_height == 0 || pushed >= remaining {
        return pushed;
    }
    gradients.push(crate::intel::gpgpu::GpgpuGradientRect {
        rect: crate::intel::gpgpu::GpgpuRect::new(
            floor_i32(fill_x),
            floor_i32(fill_y),
            fill_width,
            fill_height,
        ),
        color0_rgba: color0,
        color1_rgba: color1,
        vertical: false,
    });
    pushed.saturating_add(1)
}

fn push_solid_gradient_rect(
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    remaining: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color_rgba: u32,
) -> usize {
    if remaining == 0 || width == 0 || height == 0 {
        return 0;
    }
    gradients.push(crate::intel::gpgpu::GpgpuGradientRect {
        rect: crate::intel::gpgpu::GpgpuRect::new(x, y, width, height),
        color0_rgba: color_rgba,
        color1_rgba: color_rgba,
        vertical: false,
    });
    1
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
    let fallback_max_x = scene
        .viewport_width
        .saturating_sub(crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS)
        as i32;
    let fallback_max_y = scene
        .viewport_height
        .saturating_sub(crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS)
        as i32;
    let (max_draw_x, max_draw_y) = crate::intel::gpgpu::sprite64_primary_draw_bounds()
        .unwrap_or((fallback_max_x, fallback_max_y));
    let dst_y = floor_i32(y);
    if dst_y < 0 || dst_y > max_draw_y {
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
        let dst_x = floor_i32(pen_x);
        if dst_x < 0 || dst_x > max_draw_x {
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
            dst_x,
            dst_y,
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

fn json_rgb24_field(node: &Value, key: &str) -> Option<u32> {
    json_u32_field(node, key).map(|rgb| rgb & 0x00FF_FFFF)
}

fn rgb24_to_rgba8_word(rgb: u32) -> u32 {
    0xFF00_0000 | (rgb & 0x00FF_FFFF)
}

fn floor_i32(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    libm::floorf(value).clamp(i32::MIN as f32, i32::MAX as f32) as i32
}
