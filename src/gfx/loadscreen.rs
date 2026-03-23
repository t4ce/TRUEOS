use embassy_time::{Duration as EmbassyDuration, Timer};

const LOADSCREEN_MIN_LIFETIME_MS: u64 = 5_000;
const LOADSCREEN_WAIT_POLL_MS: u64 = 100;

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    const LOADSCREEN_BG_RGB: u32 = 0xFFFFFF;
    const MSG: &[u8] = b"TRUE OS";

    let (fb_w, fb_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as f32, fb.height() as f32))
        .unwrap_or((1024.0, 768.0));
    let start_ms = boot_probe_ms();
    let min_end_ms = start_ms.saturating_add(LOADSCREEN_MIN_LIFETIME_MS);
    let tile_h = (fb_h * 0.16).clamp(56.0, 140.0);
    let text_layout = crate::gfx::imbafont::layout_text_centered(
        crate::gfx::imbafont::ImbaFontFace::Grow,
        MSG,
        fb_w,
        fb_h,
        tile_h,
    );

    crate::log!("boot-probe: loadscreen start ms={}\n", start_ms);

    crate::gfx::with_cabi_frame_lock(|| {
        let begin_rc =
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(LOADSCREEN_BG_RGB) };
        if begin_rc == 0 {
            if let Some(layout) = text_layout {
                let _ = crate::gfx::imbafont::draw_text_in_frame(
                    crate::gfx::imbafont::ImbaFontFace::Grow,
                    MSG,
                    &layout,
                    fb_w as u32,
                    fb_h as u32,
                    (0, 0, 0),
                    255,
                );
            }
            unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
        } else {
            crate::log!("gfx-loadscreen: begin_frame failed rc={}\n", begin_rc);
        }
    });

    loop {
        let now_ms = boot_probe_ms();
        let min_lifetime_elapsed = now_ms >= min_end_ms;
        let ui2_ready = crate::r::readiness::is_set(crate::r::readiness::UI2_READY);
        if ui2_ready && min_lifetime_elapsed {
            crate::r::readiness::set(crate::r::readiness::LOADSCREEN_END);
            break;
        }

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_WAIT_POLL_MS)).await;
    }

    crate::log!(
        "boot-probe: loadscreen end ms={} lived_ms={}\n",
        boot_probe_ms(),
        boot_probe_ms().saturating_sub(start_ms)
    );
}
