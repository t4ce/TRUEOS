use alloc::vec::Vec;

use serde_json::Value;

const UI3_LAYOUT_TEXT_NODE_MAX: usize = 512;
const UI3_TEXT_PLACEMENT_MAX: usize = 4096;
const UI3_TEXT_SUBMIT_BATCH_PLACEMENTS: usize = 256;
const UI3_TEXT_COLOR_RGBA: u32 = 0x0000_0000;
const UI3_PAINTED_BOX_MAX: usize = 25;
const UI3_GRADIENT_DESCS_PER_PAINTED_BOX_MAX: usize = 5;
const UI3_GRADIENT_DESC_MAX: usize = UI3_PAINTED_BOX_MAX * UI3_GRADIENT_DESCS_PER_PAINTED_BOX_MAX;
const UI3_SUMMARY_ICON_CLOSED: char = '\u{25B6}';
const UI3_SUMMARY_ICON_OPEN: char = '\u{1F53D}';
const UI3_SUMMARY_ICON_X_PAD: f32 = 4.0;

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

pub(crate) fn draw_paint_plan_primary(
    plan: &Value,
    scene: Ui3FontScene,
    scratch: &mut Ui3FontScratch,
    present_reason: &str,
) -> Ui3FontDrawResult {
    scratch.placements.clear();
    scratch.gradients.clear();
    scratch.control_gradients.clear();
    let mut collect = Ui3TextCollectStats::default();
    let mut gradient_collect = Ui3GradientCollectStats::default();
    collect_paint_plan_rect_gradients(
        plan,
        scene.scroll_y,
        scene,
        &mut scratch.gradients,
        &mut scratch.control_gradients,
        &mut gradient_collect,
    );
    collect_paint_plan_summary_icons(
        plan,
        scene.scroll_y,
        scene,
        &mut scratch.placements,
        &mut collect,
    );
    collect_paint_plan_text_placements(
        plan,
        scene.scroll_y,
        scene,
        &mut scratch.placements,
        &mut collect,
    );
    finish_draw_primary(scratch, collect, gradient_collect, scene, present_reason)
}

fn finish_draw_primary(
    scratch: &mut Ui3FontScratch,
    collect: Ui3TextCollectStats,
    gradient_collect: Ui3GradientCollectStats,
    scene: Ui3FontScene,
    present_reason: &str,
) -> Ui3FontDrawResult {
    let gradient_count = scratch
        .gradients
        .len()
        .saturating_add(scratch.control_gradients.len());
    log_collect_caps(scratch, collect, gradient_collect, gradient_count, scene);
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

fn log_collect_caps(
    scratch: &Ui3FontScratch,
    collect: Ui3TextCollectStats,
    gradient_collect: Ui3GradientCollectStats,
    gradient_count: usize,
    scene: Ui3FontScene,
) {
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
}

fn collect_paint_plan_rect_gradients(
    plan: &Value,
    scroll_y: f32,
    scene: Ui3FontScene,
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    control_gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    stats: &mut Ui3GradientCollectStats,
) {
    let Some(boxes) = plan.get("paintedBoxes").and_then(Value::as_array) else {
        return;
    };
    for item in boxes {
        if stats.painted_boxes >= UI3_PAINTED_BOX_MAX {
            stats.painted_box_cap_hit = true;
            break;
        }
        if paint_plan_gradient_count(gradients, control_gradients) >= UI3_GRADIENT_DESC_MAX {
            stats.gradient_desc_cap_hit = true;
            break;
        }
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        let remaining = UI3_GRADIENT_DESC_MAX
            .saturating_sub(paint_plan_gradient_count(gradients, control_gradients));
        let pushed = match role {
            "dialog" => push_painted_box_gradients(
                json_f32_field(item, "x").unwrap_or(0.0),
                json_f32_field(item, "y").unwrap_or(0.0) - scroll_y,
                item,
                scene,
                remaining,
                gradients,
            ),
            "button" | "iframe" | "link" | "table" | "table-cell" => push_painted_box_gradients(
                json_f32_field(item, "x").unwrap_or(0.0),
                json_f32_field(item, "y").unwrap_or(0.0) - scroll_y,
                item,
                scene,
                remaining,
                control_gradients,
            ),
            _ => 0,
        };
        if pushed != 0 {
            stats.painted_boxes = stats.painted_boxes.saturating_add(1);
        }
    }
}

fn collect_paint_plan_summary_icons(
    plan: &Value,
    scroll_y: f32,
    scene: Ui3FontScene,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let Some(icons) = plan.get("summaryIcons").and_then(Value::as_array) else {
        return;
    };
    for icon in icons {
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            stats.placement_cap_hit = true;
            break;
        }
        push_summary_icon_placement_resolved(
            json_f32_field(icon, "x").unwrap_or(0.0),
            json_f32_field(icon, "y").unwrap_or(0.0) - scroll_y,
            json_f32_field(icon, "height").unwrap_or(0.0),
            json_bool_field(icon, "open").unwrap_or(false),
            scene,
            placements,
            stats,
        );
    }
}

fn collect_paint_plan_text_placements(
    plan: &Value,
    scroll_y: f32,
    scene: Ui3FontScene,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let Some(text_runs) = plan.get("textRuns").and_then(Value::as_array) else {
        return;
    };
    for run in text_runs {
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
        let Some(text) = run.get("text").and_then(Value::as_str) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        stats.text_nodes = stats.text_nodes.saturating_add(1);
        let text_color = json_rgb24_field(run, "textColor")
            .map(rgb24_to_sprite_tint_word)
            .unwrap_or(UI3_TEXT_COLOR_RGBA);
        let preserve_whitespace = paint_plan_preserve_whitespace(run);
        let x = json_f32_field(run, "x").unwrap_or(0.0);
        let y = json_f32_field(run, "y").unwrap_or(0.0) - scroll_y;
        if let Some(lines) = run.get("lines").and_then(Value::as_array) {
            let line_count = lines.len().max(1);
            let layout_line_height = json_f32_field(run, "height")
                .map(|height| height / line_count as f32)
                .filter(|height| height.is_finite() && *height > 0.0);
            push_layout_text_lines(
                lines,
                x,
                y,
                layout_line_height,
                scene,
                text_color,
                preserve_whitespace,
                placements,
                stats,
            );
        } else {
            push_text_placements(
                text,
                x,
                y,
                None,
                scene,
                text_color,
                preserve_whitespace,
                placements,
                stats,
            );
        }
    }
}

fn paint_plan_gradient_count(
    gradients: &[crate::intel::gpgpu::GpgpuGradientRect],
    control_gradients: &[crate::intel::gpgpu::GpgpuGradientRect],
) -> usize {
    gradients.len().saturating_add(control_gradients.len())
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

fn push_summary_icon_placement_resolved(
    x: f32,
    y: f32,
    height: f32,
    open: bool,
    scene: Ui3FontScene,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
        stats.placement_cap_hit = true;
        return;
    }
    let Some(slot) = summary_icon_slot(open) else {
        return;
    };
    let cell = crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS as f32;
    let y_offset = if height > cell {
        (height - cell) * 0.5
    } else {
        0.0
    };
    let dst_x = floor_i32(x + UI3_SUMMARY_ICON_X_PAD);
    let dst_y = floor_i32(y + y_offset);

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
    if dst_x < 0 || dst_x > max_draw_x || dst_y < 0 || dst_y > max_draw_y {
        stats.clipped = stats.clipped.saturating_add(1);
        return;
    }

    placements.push(crate::intel::gpgpu::GpgpuSprite64Placement::src_over(slot, dst_x, dst_y));
}

fn summary_icon_slot(open: bool) -> Option<u16> {
    let primary = if open {
        UI3_SUMMARY_ICON_OPEN
    } else {
        UI3_SUMMARY_ICON_CLOSED
    };
    crate::ui3::althlasfont::twemoji::twemoji_lookup_glyph_region(primary)
        .or_else(|| {
            crate::ui3::althlasfont::twemoji::twemoji_lookup_glyph_region(UI3_SUMMARY_ICON_CLOSED)
        })
        .map(|region| region.slot)
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

fn push_text_placements(
    text: &str,
    x: f32,
    y: f32,
    line_advance: Option<f32>,
    scene: Ui3FontScene,
    color_rgba: u32,
    preserve_whitespace: bool,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let face = crate::ui3::althlasfont::bitmapfont::ATHLAS_FONT_FACE_LUCIDA_HALF;
    let font_line_height =
        crate::ui3::althlasfont::bitmapfont::athlas_font_line_height_px(face).unwrap_or(22) as f32;
    let line_advance = line_advance
        .filter(|height| height.is_finite() && *height > 0.0)
        .unwrap_or(font_line_height);
    for (line_index, line) in text.split('\n').enumerate() {
        push_text_line_placements(
            line,
            x,
            y + line_advance * line_index as f32,
            font_line_height,
            scene,
            color_rgba,
            preserve_whitespace,
            placements,
            stats,
        );
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            break;
        }
    }
}

fn push_layout_text_lines(
    lines: &[Value],
    x: f32,
    y: f32,
    line_advance: Option<f32>,
    scene: Ui3FontScene,
    color_rgba: u32,
    preserve_whitespace: bool,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let face = crate::ui3::althlasfont::bitmapfont::ATHLAS_FONT_FACE_LUCIDA_HALF;
    let font_line_height =
        crate::ui3::althlasfont::bitmapfont::athlas_font_line_height_px(face).unwrap_or(22) as f32;
    let line_advance = line_advance
        .filter(|height| height.is_finite() && *height > 0.0)
        .unwrap_or(font_line_height);
    for (line_index, line) in lines.iter().enumerate() {
        let Some(text) = line.as_str() else {
            continue;
        };
        push_text_line_placements(
            text,
            x,
            y + line_advance * line_index as f32,
            font_line_height,
            scene,
            color_rgba,
            preserve_whitespace,
            placements,
            stats,
        );
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            break;
        }
    }
}

fn push_text_line_placements(
    text: &str,
    x: f32,
    y: f32,
    line_height: f32,
    scene: Ui3FontScene,
    color_rgba: u32,
    preserve_whitespace: bool,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let face = crate::ui3::althlasfont::bitmapfont::ATHLAS_FONT_FACE_LUCIDA_HALF;
    let preserved_space_advance = if preserve_whitespace {
        preserved_text_space_advance(face, line_height)
    } else {
        line_height * 0.35
    };
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
            pen_x += if preserve_whitespace && ch == '\t' {
                preserved_space_advance * 4.0
            } else {
                preserved_space_advance
            };
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
            slot, dst_x, dst_y, color_rgba,
        ));
        pen_x += advance;
    }
}

fn preserved_text_space_advance(
    face: crate::ui3::althlasfont::bitmapfont::AthlasFontFace,
    line_height: f32,
) -> f32 {
    crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, '█')
        .or_else(|| crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, 'M'))
        .map(|region| f32::from(region.src_w.max(1)))
        .unwrap_or(line_height * 0.58)
}

fn paint_plan_preserve_whitespace(node: &Value) -> bool {
    node.get("preserveWhitespace")
        .and_then(Value::as_bool)
        .unwrap_or(false)
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

fn json_bool_field(node: &Value, key: &str) -> Option<bool> {
    node.get(key)?.as_bool()
}

fn json_rgb24_field(node: &Value, key: &str) -> Option<u32> {
    json_u32_field(node, key).map(|rgb| rgb & 0x00FF_FFFF)
}

fn rgb24_to_rgba8_word(rgb: u32) -> u32 {
    0xFF00_0000 | (rgb & 0x00FF_FFFF)
}

fn rgb24_to_sprite_tint_word(rgb: u32) -> u32 {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

fn floor_i32(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    libm::floorf(value).clamp(i32::MIN as f32, i32::MAX as f32) as i32
}
