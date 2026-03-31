use alloc::{format, string::String};

use super::*;

#[inline]
fn hosted_browser_scene_font_px(font_size_px: u32, fallback_px: f32) -> f32 {
    let px = font_size_px as f32;
    ui2_font_native_line_height_px(ui2_font_pick_tier_for_px(if px.is_finite() && px >= 1.0 {
        px
    } else {
        fallback_px.max(1.0)
    })) as f32
}

#[inline]
fn hosted_browser_scene_line_px(line_height_px: u32, font_px: f32, fallback_px: f32) -> f32 {
    let px = line_height_px as f32;
    if px.is_finite() && px >= 1.0 {
        px.max(font_px)
    } else {
        fallback_px.max(font_px).max(1.0)
    }
}

fn draw_hosted_browser_text_scene(state: &Ui2State, window: &Ui2Window, content: Ui2Rect) -> bool {
    let text_state = &window.hosted_browser_snapshot.text;
    if text_state.rows.is_empty() {
        return false;
    }

    let surface_state = browser_surface_state_for_window(window);
    let panel_rgba = modulate_rgba_alpha((0xFB, 0xFB, 0xF8, 0xFF), window.alpha);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        content.x,
        content.y,
        content.w,
        content.h,
        panel_rgba,
        state.view_w,
        state.view_h,
    );

    let pad_x = 10.0f32;
    let pad_y = 8.0f32;
    let default_text_px_h = ui2_font_native_line_height_px(Ui2FontTier::OneX) as f32;
    let visible_bottom = content.y + content.h - pad_y;
    let scroll_x = surface_state.scroll_x as f32;
    let scroll_y = surface_state.scroll_y as f32;
    let content_right = content.x + content.w - pad_x;
    let mut drew_any = false;
    let mut row_y = content.y + pad_y;

    for row in text_state.rows.iter() {
        let font_px = hosted_browser_scene_font_px(row.font_size_px, default_text_px_h);
        let line_px = hosted_browser_scene_line_px(row.line_height_px, font_px, font_px + 4.0);
        let x = content.x + pad_x + row.indent_px as f32 - scroll_x;
        let y = row_y - scroll_y;
        row_y += line_px;
        if y + line_px <= content.y || y >= visible_bottom {
            continue;
        }
        if x >= content_right || row.text.is_empty() {
            continue;
        }

        let max_width_px = (content_right - x).max(0.0);
        if max_width_px <= 0.0 {
            continue;
        }

        drew_any |= ui2_font_draw_text_line_no_present(
            row.text.as_str(),
            x,
            y,
            max_width_px,
            font_px,
            state.view_w,
            state.view_h,
            window.alpha,
        );
    }

    drew_any
}

fn draw_hosted_browser_layout_scene(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
) -> bool {
    let layout_state = &window.hosted_browser_snapshot.layout;
    if layout_state.nodes.is_empty() {
        return false;
    }

    let panel_rgba = modulate_rgba_alpha((0xF8, 0xF8, 0xF4, 0xFF), window.alpha);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        content.x,
        content.y,
        content.w,
        content.h,
        panel_rgba,
        state.view_w,
        state.view_h,
    );

    let mut y_cursor = content.y + 8.0;
    let bottom = content.y + content.h - 4.0;
    let mut drew_any = false;

    for node in layout_state.nodes.iter().take(24) {
        if node.parent_id == 0 {
            continue;
        }
        let margin_top = node.margin_top_px as f32;
        let margin_bottom = node.margin_bottom_px as f32;
        let text_top = y_cursor + margin_top;
        if text_top >= bottom {
            continue;
        }
        let text = if !node.text.is_empty() {
            node.text.clone()
        } else if !node.tag.is_empty() {
            format!("<{}>", node.tag)
        } else {
            String::new()
        };

        let fallback_line_h = node
            .intrinsic_height_px
            .max(node.min_height_px)
            .clamp(12, 24) as f32;
        let font_px = hosted_browser_scene_font_px(node.font_size_px, fallback_line_h);
        let _line_h = hosted_browser_scene_line_px(node.line_height_px, font_px, fallback_line_h);
        let block_h = node.intrinsic_height_px.max(node.min_height_px).max(10) as f32
            + margin_top
            + margin_bottom
            + node.padding_top_px as f32
            + node.padding_bottom_px as f32;
        if !text.is_empty() {
            let left = content.x + 8.0 + node.margin_left_px as f32 + node.padding_left_px as f32;
            let max_width_px = (content.x + content.w - 8.0 - left).max(0.0);
            if max_width_px > 0.0 {
                drew_any |= ui2_font_draw_text_line_no_present(
                    text.as_str(),
                    left,
                    text_top,
                    max_width_px,
                    font_px,
                    state.view_w,
                    state.view_h,
                    window.alpha,
                );
            }
        }
        y_cursor += block_h.max(8.0);
    }

    drew_any
}

pub(super) fn log_browser_surface_updates(state: &mut Ui2State) {
    let windows = state.windows.clone();
    for window in &windows {
        if window.kind != Ui2WindowKind::HostedBrowser || !window.visible {
            continue;
        }
        let snapshot = browser_surface_state_for_window(window);
        if let Some(content) = window_content_rect(state, window) {
            let (_, _, want_w, want_h) = snap_browser_content_rect(content);
            if snapshot.viewport_width != want_w || snapshot.viewport_height != want_h {
                let _ = note_window_viewport_sync_needed(state, window.id);
                crate::log!(
                    "ui2: browser-viewport-mismatch window={} have={}x{} want={}x{}\n",
                    window.id,
                    snapshot.viewport_width,
                    snapshot.viewport_height,
                    want_w,
                    want_h
                );
            }
        }
    }
}

pub(super) fn draw_hosted_browser_window_content(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
    chrome_ms: u64,
) -> Ui2WindowDrawTiming {
    let scene_started_at = Instant::now();
    if !window.hosted_browser_snapshot.layout.nodes.is_empty()
        && draw_hosted_browser_layout_scene(state, window, content)
    {
        return Ui2WindowDrawTiming {
            chrome_ms,
            texture_ms: 0,
            placeholder_ms: elapsed_ms_since(scene_started_at),
            content_path: "browser-layout-scene",
        };
    }
    if !window.hosted_browser_snapshot.text.rows.is_empty()
        && draw_hosted_browser_text_scene(state, window, content)
    {
        return Ui2WindowDrawTiming {
            chrome_ms,
            texture_ms: 0,
            placeholder_ms: elapsed_ms_since(scene_started_at),
            content_path: "browser-text-scene",
        };
    }

    let content_started_at = Instant::now();
    if texture_is_drawable(window.content_tex_id)
        && draw_texture_rect_no_present(
            window.content_tex_id,
            content.x,
            content.y,
            content.w,
            content.h,
            state.view_w,
            state.view_h,
            true,
            window.alpha,
        )
    {
        return Ui2WindowDrawTiming {
            chrome_ms,
            texture_ms: elapsed_ms_since(content_started_at),
            placeholder_ms: 0,
            content_path: "browser-texture",
        };
    }

    Ui2WindowDrawTiming {
        chrome_ms,
        texture_ms: 0,
        placeholder_ms: elapsed_ms_since(scene_started_at),
        content_path: "browser-empty",
    }
}
