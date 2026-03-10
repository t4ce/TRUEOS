use embassy_time::{Duration as EmbassyDuration, Timer};

fn draw_de_flag_bottom_left_in_frame(view_w: u32, view_h: u32) -> bool {
    let vw = view_w.max(1) as f32;
    let vh = view_h.max(1) as f32;

    let flag_w = 128.0f32.min(vw);
    let flag_h = 128.0f32.min(vh);
    let x = 0.0f32;
    let y = (vh - flag_h).max(0.0);
    let stripe_h = flag_h / 3.0;

    // Germany flag: black, red, gold.
    let top_ok = crate::gfx::lyon::draw_solid_rect_no_present(
        x,
        y,
        flag_w,
        stripe_h,
        (0x00, 0x00, 0x00, 0xFF),
        view_w,
        view_h,
    );
    let mid_ok = crate::gfx::lyon::draw_solid_rect_no_present(
        x,
        y + stripe_h,
        flag_w,
        stripe_h,
        (0xDD, 0x00, 0x00, 0xFF),
        view_w,
        view_h,
    );
    let bot_ok = crate::gfx::lyon::draw_solid_rect_no_present(
        x,
        y + (2.0 * stripe_h),
        flag_w,
        flag_h - (2.0 * stripe_h),
        (0xFF, 0xCE, 0x00, 0xFF),
        view_w,
        view_h,
    );
    top_ok && mid_ok && bot_ok
}

fn centered_text_origin(msg: &[u8], fb_w: f32, fb_h: f32) -> (f32, f32) {
    let atlas = crate::gfx::text::font_atlas_large_view();
    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let space_adv_px = atlas.cell_w as f32 * 0.60;

    let glyph_slot = |ch: u8| {
        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        slot
    };

    let mut width_px = 0.0f32;
    for &ch in msg {
        if ch == b' ' {
            width_px += space_adv_px;
            continue;
        }
        let slot = glyph_slot(ch);
        let glyph_w_px = atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32;
        width_px += glyph_w_px;
    }

    let x = ((fb_w - width_px) * 0.5).max(0.0);
    let y = ((fb_h - atlas.cell_h as f32) * 0.5).max(0.0);
    (x, y)
}

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    const MSG: &[u8] = b"TRUE OS \xA7";
    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));
    let (text_x, text_y) = centered_text_origin(MSG, fb_w, fb_h);
    crate::log!("GFX Loadscreen\n");
    unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(0xF4F4F4) };
    Timer::after(EmbassyDuration::from_millis(16)).await;
    crate::gfx::lyon::lyon_geom_api_demo_no_present(fb_w as u32, fb_h as u32);
    let _ = draw_de_flag_bottom_left_in_frame(fb_w as u32, fb_h as u32);
    let _ =
        crate::gfx::text::draw_atlas_text_in_frame(MSG, text_x, text_y, fb_w as u32, fb_h as u32);
    unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
    Timer::after(EmbassyDuration::from_millis(3000)).await;
    crate::v::readiness::set(crate::v::readiness::WGPU_TEXT_DONE);
}
