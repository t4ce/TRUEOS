use super::super::*;

const UI2_BROWSER_GADGET_PAD_X: f32 = 10.0;
const UI2_BROWSER_GADGET_PAD_Y: f32 = 8.0;
const UI2_BROWSER_GADGET_BG_RGBA: (u8, u8, u8, u8) = (0xFB, 0xFB, 0xF8, 0xFF);
const UI2_BROWSER_BUTTON_PAD_X: f32 = 12.0;
const UI2_BROWSER_BUTTON_PAD_Y: f32 = 6.0;
const UI2_BROWSER_BUTTON_BORDER_PX: f32 = 1.0;

#[inline]
fn ui2_browser_text_rgba(rgb: u32) -> (u8, u8, u8, u8) {
    (((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8, 0xFF)
}

fn draw_browser_button_outline(rect: Ui2Rect, rgba: (u8, u8, u8, u8), view_w: u32, view_h: u32) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }

    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        rect.x,
        rect.y,
        rect.w,
        UI2_BROWSER_BUTTON_BORDER_PX,
        rgba,
        view_w,
        view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        rect.x,
        rect.y + rect.h - UI2_BROWSER_BUTTON_BORDER_PX,
        rect.w,
        UI2_BROWSER_BUTTON_BORDER_PX,
        rgba,
        view_w,
        view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        rect.x,
        rect.y,
        UI2_BROWSER_BUTTON_BORDER_PX,
        rect.h,
        rgba,
        view_w,
        view_h,
    );
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        rect.x + rect.w - UI2_BROWSER_BUTTON_BORDER_PX,
        rect.y,
        UI2_BROWSER_BUTTON_BORDER_PX,
        rect.h,
        rgba,
        view_w,
        view_h,
    );
}

pub(crate) fn draw_hosted_browser_gadget_scene(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
) -> bool {
    let snapshot = &window.hosted_browser_snapshot.gadget_snapshot;
    if snapshot.gadgets.is_empty() {
        return false;
    }

    let surface_state = browser_surface_state_for_window(window);
    let panel_rgba = modulate_rgba_alpha(UI2_BROWSER_GADGET_BG_RGBA, window.alpha);
    let _ = crate::gfx::lyon::draw_solid_rect_no_present(
        content.x,
        content.y,
        content.w,
        content.h,
        panel_rgba,
        state.view_w,
        state.view_h,
    );

    let default_font_px = ui2_font_native_line_height_px(Ui2FontTier::Half) as f32;
    let scroll_x = surface_state.scroll_x as f32;
    let scroll_y = surface_state.scroll_y as f32;
    let visible_right = content.x + content.w - UI2_BROWSER_GADGET_PAD_X;
    let visible_bottom = content.y + content.h - UI2_BROWSER_GADGET_PAD_Y;
    let mut drew_any = false;

    for gadget in &snapshot.gadgets {
        let is_image = gadget.tex_id != 0;
        if gadget.text.is_empty() && !is_image {
            continue;
        }
        let x = content.x + UI2_BROWSER_GADGET_PAD_X + gadget.x_px as f32 - scroll_x;
        let y = content.y + UI2_BROWSER_GADGET_PAD_Y + gadget.y_px as f32 - scroll_y;
        if is_image {
            let mut draw_w = gadget.width_px.max(1) as f32;
            let mut draw_h = gadget.height_px.max(1) as f32;
            if y + draw_h <= content.y || y >= visible_bottom {
                continue;
            }
            if x >= visible_right {
                continue;
            }
            let max_w = (visible_right - x).max(0.0);
            if max_w <= 0.0 {
                continue;
            }
            if draw_w > max_w {
                let scale = max_w / draw_w;
                draw_w = max_w;
                draw_h = (draw_h * scale).max(1.0);
            }
            drew_any |= texture_is_drawable(gadget.tex_id)
                && draw_texture_rect_no_present(
                    gadget.tex_id,
                    x,
                    y,
                    draw_w,
                    draw_h,
                    state.view_w,
                    state.view_h,
                    true,
                    window.alpha,
                );
            continue;
        }
        let font_px = gadget.font_size_px.max(1) as f32;
        let text_metrics = ui2_font_measure_text_for_px(gadget.text.as_str(), font_px);
        let line_height_px = gadget
            .line_height_px
            .max(u32::from(text_metrics.line_height_px))
            .max(1) as f32;
        let h = gadget.height_px.max(line_height_px as u32) as f32;
        if y + h <= content.y || y >= visible_bottom {
            continue;
        }
        if x >= visible_right {
            continue;
        }

        let text_rgba =
            modulate_rgba_alpha(ui2_browser_text_rgba(gadget.text_color_rgb), window.alpha);

        if gadget.button_like {
            let button_w = (gadget.width_px.max(text_metrics.width_px) as f32
                + (UI2_BROWSER_BUTTON_PAD_X * 2.0))
                .max(text_metrics.width_px as f32 + (UI2_BROWSER_BUTTON_PAD_X * 2.0));
            let button_h = (gadget.height_px.max(text_metrics.height_px) as f32
                + (UI2_BROWSER_BUTTON_PAD_Y * 2.0))
                .max(line_height_px + (UI2_BROWSER_BUTTON_PAD_Y * 2.0));
            let button_rect =
                Ui2Rect::new(x, y, (visible_right - x).min(button_w).max(0.0), button_h);
            if button_rect.w <= 0.0 || button_rect.y >= visible_bottom {
                continue;
            }

            draw_browser_button_outline(button_rect, text_rgba, state.view_w, state.view_h);

            let text_rect = Ui2Rect::new(
                button_rect.x + UI2_BROWSER_BUTTON_PAD_X,
                button_rect.y + UI2_BROWSER_BUTTON_PAD_Y,
                (button_rect.w - (UI2_BROWSER_BUTTON_PAD_X * 2.0)).max(0.0),
                (button_rect.h - (UI2_BROWSER_BUTTON_PAD_Y * 2.0)).max(line_height_px),
            );
            if text_rect.w <= 0.0 {
                continue;
            }

            drew_any |= ui2_font_draw_text_line_in_rect_rgba_no_present(
                gadget.text.as_str(),
                text_rect,
                font_px,
                Ui2FontTextAlign::Center,
                Ui2FontVerticalAlign::Center,
                state.view_w,
                state.view_h,
                text_rgba,
            );
            continue;
        }

        let text_rect = Ui2Rect::new(
            x,
            y,
            (visible_right - x)
                .min(gadget.width_px.max(1) as f32)
                .max(0.0),
            h.max(default_font_px),
        );
        if text_rect.w <= 0.0 {
            continue;
        }

        drew_any |= ui2_font_draw_text_line_in_rect_rgba_no_present(
            gadget.text.as_str(),
            text_rect,
            font_px,
            Ui2FontTextAlign::Left,
            Ui2FontVerticalAlign::Top,
            state.view_w,
            state.view_h,
            text_rgba,
        );
    }

    drew_any
}
