use alloc::{string::String, vec, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::ui2::{self, Ui2FontTier, Ui2HostedInteractiveRect, Ui2Rect};

const UI2_USB_AUDIO_TEX_ID: u32 = 4_902;
const UI2_USB_AUDIO_W: u32 = 420;
const UI2_USB_AUDIO_H: u32 = 168;
const UI2_USB_AUDIO_X: f32 = 280.0;
const UI2_USB_AUDIO_Y: f32 = 180.0;
const UI2_USB_AUDIO_Z: i16 = 35;
const UI2_USB_AUDIO_BG_RGBA: [u8; 4] = [0x12, 0x18, 0x1B, 0xFF];
const UI2_USB_AUDIO_PANEL_RGBA: [u8; 4] = [0x1E, 0x29, 0x2D, 0xFF];
const UI2_USB_AUDIO_BORDER_RGBA: [u8; 4] = [0x3F, 0x57, 0x61, 0xFF];
const UI2_USB_AUDIO_TEXT_RGBA: [u8; 4] = [0xE9, 0xEF, 0xF1, 0xFF];
const UI2_USB_AUDIO_MUTED_RGBA: [u8; 4] = [0x97, 0xA7, 0xAE, 0xFF];
const UI2_USB_AUDIO_PLAY_RGBA: [u8; 4] = [0x27, 0x9A, 0x61, 0xFF];
const UI2_USB_AUDIO_STOP_RGBA: [u8; 4] = [0xB0, 0x47, 0x3C, 0xFF];
const UI2_USB_AUDIO_BUTTON_ID: u32 = 1;
const UI2_USB_AUDIO_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_USB_AUDIO_FONT_SIZE_CASE: usize = UI2_USB_AUDIO_FONT_TIER.size_case();

fn fill_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let end_y = y.saturating_add(h).min(dst_height);
    let end_x = x.saturating_add(w).min(dst_width);
    for row in y.min(dst_height)..end_y {
        for col in x.min(dst_width)..end_x {
            let idx = (row * dst_width + col) * 4;
            dst[idx] = rgba[0];
            dst[idx + 1] = rgba[1];
            dst[idx + 2] = rgba[2];
            dst[idx + 3] = rgba[3];
        }
    }
}

fn stroke_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    if w == 0 || h == 0 {
        return;
    }
    fill_rect_rgba(dst, dst_width, dst_height, x, y, w, 1, rgba);
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        x,
        y.saturating_add(h.saturating_sub(1)),
        w,
        1,
        rgba,
    );
    fill_rect_rgba(dst, dst_width, dst_height, x, y, 1, h, rgba);
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        x.saturating_add(w.saturating_sub(1)),
        y,
        1,
        h,
        rgba,
    );
}

fn draw_text_line(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    x: u32,
    y: u32,
    text: &str,
    fg_rgba: [u8; 4],
) {
    let mut pen_x = x;
    let line_h = u32::from(ui2::ui2_font_native_line_height_px(UI2_USB_AUDIO_FONT_TIER));
    for ch in text.chars() {
        let Some(glyph) = ui2::ui2_font_resolve_glyph(UI2_USB_AUDIO_FONT_TIER, ch)
            .or_else(|| ui2::ui2_font_resolve_glyph(UI2_USB_AUDIO_FONT_TIER, '?'))
        else {
            continue;
        };
        let advance = u32::from(glyph.advance_px.max(1));
        let _ = ui2::ui2_font_blit_glyph_rgba(
            dst,
            dst_width,
            dst_height,
            atlases,
            &glyph,
            Ui2Rect {
                x: pen_x as f32,
                y: y as f32,
                w: advance as f32,
                h: line_h as f32,
            },
            fg_rgba,
        );
        pen_x = pen_x.saturating_add(advance);
    }
}

fn draw_centered_text(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    rect: Ui2Rect,
    text: &str,
    fg_rgba: [u8; 4],
) {
    let metrics = ui2::ui2_font_measure_text(UI2_USB_AUDIO_FONT_TIER, text);
    let x = (rect.x.max(0.0) as u32)
        .saturating_add(((rect.w.max(0.0) as u32).saturating_sub(metrics.width_px)) / 2);
    let y = (rect.y.max(0.0) as u32)
        .saturating_add(((rect.h.max(0.0) as u32).saturating_sub(metrics.height_px)) / 2);
    draw_text_line(dst, dst_width, dst_height, atlases, x, y, text, fg_rgba);
}

fn status_text() -> String {
    let requested = crate::usb2::audio_demo_requested();
    let active = crate::usb2::audio_demo_active();
    let attached = crate::r::readiness::is_set(crate::r::readiness::UAC_ATTACHED);
    if active {
        String::from("USB audio streaming")
    } else if requested && attached {
        String::from("Requested, waiting for stream")
    } else if requested {
        String::from("Requested, waiting for UAC device")
    } else if attached {
        String::from("UAC device attached, playback stopped")
    } else {
        String::from("No UAC sink attached")
    }
}

fn render_frame(atlases: &ui2::Ui2FontCpuAtlases, asset_name: &str, requested: bool) -> Vec<u8> {
    let mut rgba = vec![
        0u8;
        (UI2_USB_AUDIO_W as usize)
            .saturating_mul(UI2_USB_AUDIO_H as usize)
            .saturating_mul(4)
    ];
    fill_rect_rgba(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        0,
        0,
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        UI2_USB_AUDIO_BG_RGBA,
    );
    fill_rect_rgba(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        14,
        14,
        (UI2_USB_AUDIO_W - 28) as usize,
        (UI2_USB_AUDIO_H - 28) as usize,
        UI2_USB_AUDIO_PANEL_RGBA,
    );
    stroke_rect_rgba(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        14,
        14,
        (UI2_USB_AUDIO_W - 28) as usize,
        (UI2_USB_AUDIO_H - 28) as usize,
        UI2_USB_AUDIO_BORDER_RGBA,
    );

    draw_text_line(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        atlases,
        28,
        28,
        "USB Audio Demo",
        UI2_USB_AUDIO_TEXT_RGBA,
    );
    draw_text_line(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        atlases,
        28,
        58,
        asset_name,
        UI2_USB_AUDIO_TEXT_RGBA,
    );
    draw_text_line(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        atlases,
        28,
        84,
        status_text().as_str(),
        UI2_USB_AUDIO_MUTED_RGBA,
    );

    let button_rect = Ui2Rect {
        x: 28.0,
        y: 108.0,
        w: 144.0,
        h: 38.0,
    };
    let button_rgba = if requested {
        UI2_USB_AUDIO_STOP_RGBA
    } else {
        UI2_USB_AUDIO_PLAY_RGBA
    };
    fill_rect_rgba(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        button_rect.x as usize,
        button_rect.y as usize,
        button_rect.w as usize,
        button_rect.h as usize,
        button_rgba,
    );
    stroke_rect_rgba(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        button_rect.x as usize,
        button_rect.y as usize,
        button_rect.w as usize,
        button_rect.h as usize,
        UI2_USB_AUDIO_BORDER_RGBA,
    );
    draw_centered_text(
        rgba.as_mut_slice(),
        UI2_USB_AUDIO_W as usize,
        UI2_USB_AUDIO_H as usize,
        atlases,
        button_rect,
        if requested { "Stop" } else { "Play" },
        UI2_USB_AUDIO_TEXT_RGBA,
    );

    rgba
}

#[embassy_executor::task]
pub async fn ui2_usb_audio_demo_task() {
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_USB_AUDIO_FONT_SIZE_CASE) else {
        return;
    };
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "USB Audio",
        Ui2Rect {
            x: UI2_USB_AUDIO_X,
            y: UI2_USB_AUDIO_Y,
            w: UI2_USB_AUDIO_W as f32,
            h: UI2_USB_AUDIO_H as f32,
        },
        UI2_USB_AUDIO_Z,
        180,
        UI2_USB_AUDIO_TEX_ID,
        true,
        UI2_USB_AUDIO_BG_RGBA,
    ) else {
        return;
    };

    let interactives = [Ui2HostedInteractiveRect {
        item_id: UI2_USB_AUDIO_BUTTON_ID,
        x: 28,
        y: 108,
        width: 144,
        height: 38,
    }];
    let _ = surface.set_interactives(interactives.as_slice());

    let asset_name = crate::usb2::audio_demo_asset_name();
    let mut last_requested = !crate::usb2::audio_demo_requested();
    let mut last_active = !crate::usb2::audio_demo_active();
    let mut last_uac_attached = !crate::r::readiness::is_set(crate::r::readiness::UAC_ATTACHED);
    let mut last_click_seq = 0u32;

    loop {
        if let Some((seq, item_id)) =
            crate::r::ui2::take_window_last_clicked_item(surface.window_id())
            && seq != last_click_seq
            && item_id == UI2_USB_AUDIO_BUTTON_ID
        {
            last_click_seq = seq;
            crate::usb2::set_audio_demo_requested(!crate::usb2::audio_demo_requested());
        }

        let requested = crate::usb2::audio_demo_requested();
        let active = crate::usb2::audio_demo_active();
        let uac_attached = crate::r::readiness::is_set(crate::r::readiness::UAC_ATTACHED);
        if requested != last_requested || active != last_active || uac_attached != last_uac_attached
        {
            let frame = render_frame(&atlases, asset_name, requested);
            if !surface.upload_rgba(frame.as_slice(), "ui2-usb-audio-demo") {
                return;
            }
            last_requested = requested;
            last_active = active;
            last_uac_attached = uac_attached;
        }

        Timer::after(EmbassyDuration::from_millis(80)).await;
    }
}
