use embassy_time::{Duration as EmbassyDuration, Timer};

fn centered_text_origin(msg: &[u8], fb_w: f32, fb_h: f32) -> (f32, f32) {
    let atlas = crate::gfx::webgpu_font::font_atlas_large_view();
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
    let _ =
        crate::gfx::text::draw_atlas_text_in_frame(MSG, text_x, text_y, fb_w as u32, fb_h as u32);
    unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
    Timer::after(EmbassyDuration::from_millis(10000)).await;
    crate::v::readiness::set(crate::v::readiness::WGPU_TEXT_DONE);
}
