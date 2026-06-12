use alloc::vec::Vec;

use serde_json::Value;

const UI3_LAYOUT_TEXT_NODE_MAX: usize = 512;
const UI3_TEXT_PLACEMENT_MAX: usize = 4096;
const UI3_TEXT_COLOR_RGBA: u32 = 0x0000_0000;

#[derive(Debug, Default)]
pub(crate) struct Ui3FontScratch {
    placements: Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
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
    pub(crate) clipped: usize,
    pub(crate) submit_ok: bool,
    pub(crate) presented: bool,
    pub(crate) submit_ms: u64,
    pub(crate) present_ms: u64,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3TextCollectStats {
    text_nodes: usize,
    clipped: usize,
}

pub(crate) fn draw_layout_primary(
    layout: &Value,
    scene: Ui3FontScene,
    scratch: &mut Ui3FontScratch,
    present_reason: &str,
) -> Ui3FontDrawResult {
    scratch.placements.clear();
    let mut collect = Ui3TextCollectStats::default();
    collect_layout_text_placements(
        layout,
        0.0,
        0.0,
        scene.scroll_y,
        scene,
        &mut scratch.placements,
        &mut collect,
    );

    let result = if scratch.placements.is_empty() {
        None
    } else {
        crate::intel::gpgpu::sprite64_worklist_primary(
            scratch.placements.as_slice(),
            true,
            present_reason,
        )
    };

    Ui3FontDrawResult {
        text_nodes: collect.text_nodes,
        placements: scratch.placements.len(),
        clipped: collect.clipped,
        submit_ok: result.as_ref().is_some_and(|result| result.submitted),
        presented: result.as_ref().is_some_and(|result| result.presented),
        submit_ms: result.as_ref().map(|result| result.submit_ms).unwrap_or(0),
        present_ms: result.as_ref().map(|result| result.present_ms).unwrap_or(0),
    }
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

fn floor_i32(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    libm::floorf(value).clamp(i32::MIN as f32, i32::MAX as f32) as i32
}
