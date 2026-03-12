use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

static LOADSCREEN_ACTIVE: AtomicBool = AtomicBool::new(true);

pub fn stop_loadscreen() {
    LOADSCREEN_ACTIVE.store(false, Ordering::Release);
}

fn centered_text_origin(msg: &[u8], fb_w: f32, fb_h: f32) -> (f32, f32) {
    let atlas = crate::gfx::text::font_atlas_large_view();
    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);

    let glyph_slot = |ch: u8| {
        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
        if slot == u16::MAX {
            slot = fallback;
        }
        slot
    };

    let glyph_advance_px = |ch: u8| {
        let slot = glyph_slot(ch);
        atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32
    };

    let mut width_px = 0.0f32;
    for &ch in msg {
        width_px += glyph_advance_px(ch);
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

    // Initial full frame: lyon decorations + text at full opacity
    let begin_rc = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(0xF4F4F4) };
    if begin_rc != 0 {
        crate::log!("gfx-loadscreen: begin_frame failed rc={}\n", begin_rc);
    }
    Timer::after(EmbassyDuration::from_millis(16)).await;
    let lyon_ok = crate::gfx::lyon::lyon_geom_api_demo_no_present(fb_w as u32, fb_h as u32);
    let text_ok =
        crate::gfx::text::draw_atlas_text_in_frame(MSG, text_x, text_y, fb_w as u32, fb_h as u32);
    let end_rc = unsafe { crate::surface::io::cabi::trueos_cabi_gfx_end_frame() };
    if !lyon_ok || !text_ok || end_rc != 0 {
        crate::log!(
            "gfx-loadscreen: lyon_ok={} text_ok={} end_rc={}\n",
            lyon_ok,
            text_ok,
            end_rc
        );
    }

    // 60 Hz pulse loop — redraws the full frame each tick but only varies text alpha.
    // A 2-second sin period gives a gentle breathing effect (0 → 255 → 0).
    let mut frame: u32 = 0;
    loop {
        Timer::after(EmbassyDuration::from_millis(16)).await;
        if !LOADSCREEN_ACTIVE.load(Ordering::Acquire) {
            break;
        }
        let phase = (frame as f32) * (core::f32::consts::TAU / 120.0);
        let alpha = ((libm::sinf(phase) * 0.5 + 0.5) * 255.0) as u8;
        let begin_rc =
            unsafe { crate::surface::io::cabi::trueos_cabi_gfx_begin_frame(0xF4F4F4) };
        if begin_rc == 0 {
            crate::gfx::lyon::lyon_geom_api_demo_no_present(fb_w as u32, fb_h as u32);
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
    }

    crate::v::readiness::set(crate::v::readiness::WGPU_TEXT_DONE);
}
