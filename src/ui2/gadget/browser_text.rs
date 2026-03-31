use super::super::*;

const UI2_BROWSER_GADGET_PAD_X: f32 = 10.0;
const UI2_BROWSER_GADGET_PAD_Y: f32 = 8.0;
const UI2_BROWSER_GADGET_BG_RGBA: (u8, u8, u8, u8) = (0xFB, 0xFB, 0xF8, 0xFF);

pub(super) fn draw_hosted_browser_gadget_scene(
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

    let font_px = ui2_font_native_line_height_px(Ui2FontTier::OneX) as f32;
    let scroll_x = surface_state.scroll_x as f32;
    let scroll_y = surface_state.scroll_y as f32;
    let visible_right = content.x + content.w - UI2_BROWSER_GADGET_PAD_X;
    let visible_bottom = content.y + content.h - UI2_BROWSER_GADGET_PAD_Y;
    let mut drew_any = false;

    for gadget in &snapshot.gadgets {
        if gadget.text.is_empty() {
            continue;
        }
        let x = content.x + UI2_BROWSER_GADGET_PAD_X + gadget.x_px as f32 - scroll_x;
        let y = content.y + UI2_BROWSER_GADGET_PAD_Y + gadget.y_px as f32 - scroll_y;
        let h = gadget.height_px.max(gadget.line_height_px.max(1)) as f32;
        if y + h <= content.y || y >= visible_bottom {
            continue;
        }
        if x >= visible_right {
            continue;
        }

        let max_width_px = (visible_right - x)
            .min(gadget.width_px.max(1) as f32)
            .max(0.0);
        if max_width_px <= 0.0 {
            continue;
        }

        drew_any |= ui2_font_draw_text_line_no_present(
            gadget.text.as_str(),
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
