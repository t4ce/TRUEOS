use embassy_time::{Duration as EmbassyDuration, Timer};

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    const LOADSCREEN_BG_RGB: u32 = 0xF4F4F4;
    const MSG: &[u8] = b"TRUE OS \xA7";
    const TEXT_PAD_X: f32 = 12.0;
    const TEXT_PAD_Y: f32 = 10.0;

    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));

    let atlas = crate::gfx::text::font_atlas_large_view();
    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let mut text_w = 0.0f32;
    for &ch in MSG {
        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        text_w += atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32;
    }
    let text_x = ((fb_w - text_w) * 0.5).max(0.0);
    let text_y = ((fb_h - atlas.cell_h as f32) * 0.5).max(0.0);
    let clear_x = (text_x - TEXT_PAD_X).max(0.0);
    let clear_y = (text_y - TEXT_PAD_Y).max(0.0);
    let clear_w = (text_w + (TEXT_PAD_X * 2.0)).min(fb_w - clear_x);
    let clear_h = (atlas.cell_h as f32 + (TEXT_PAD_Y * 2.0)).min(fb_h - clear_y);

    let begin_rc = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(LOADSCREEN_BG_RGB) };
    if begin_rc == 0 {
        let _ = crate::gfx::lyon::lyon_geom_api_demo_no_present(fb_w as u32, fb_h as u32);
        crate::gfx::text::draw_atlas_text_in_frame_alpha(
            MSG,
            text_x,
            text_y,
            fb_w as u32,
            fb_h as u32,
            255,
        );
        unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
    }

    let mut frame: u32 = 0;
    while !crate::v::readiness::is_set(crate::v::readiness::LOADSCREEN_END) {
        let phase = (frame as f32) * (core::f32::consts::TAU / 120.0);
        let alpha = ((libm::sinf(phase) * 0.5 + 0.5) * 255.0) as u8;
        let begin_rc =
            unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame_preserve(LOADSCREEN_BG_RGB) };
        if begin_rc == 0 {
            let _ =
                unsafe { crate::surface::io::cabi::trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
            let _ = crate::gfx::lyon::draw_solid_rect_no_present(
                clear_x,
                clear_y,
                clear_w,
                clear_h,
                (0xF4, 0xF4, 0xF4, 0xFF),
                fb_w as u32,
                fb_h as u32,
            );
            crate::gfx::text::draw_atlas_text_in_frame_alpha(
                MSG,
                text_x,
                text_y,
                fb_w as u32,
                fb_h as u32,
                alpha,
            );
            unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
        }
        frame = frame.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
