use embassy_time::{Duration as EmbassyDuration, Timer};

const LOADSCREEN_BG_RGB: u32 = 0xF2EEE8;
const LOADSCREEN_WAIT_POLL_MS: u64 = 250;
const LOADSCREEN_MIN_LIFETIME_MS: u64 = 4_000;
const LOADSCREEN_ANIM_FRAME_MS: u64 = 250;

fn render_loadscreen_frame(bg_rgb: u32) -> bool {
    let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(bg_rgb) };
    if begin_rc != 0 {
        crate::log!("gfx-loadscreen: begin_frame failed rc={}\n", begin_rc);
        return false;
    }

    unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
    true
}

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[embassy_executor::task]
pub async fn gfx_loadscreen_task() {
    crate::r::readiness::set_loadscreen_expire_requested(false);

    let mut first_frame_ms = 0u64;

    loop {
        crate::gfx::with_cabi_frame_lock(|| {
            if render_loadscreen_frame(LOADSCREEN_BG_RGB) && first_frame_ms == 0 {
                first_frame_ms = boot_probe_ms();
                crate::r::readiness::set(crate::r::readiness::LOADSCREEN_FRAME_READY);
            }
        });

        if first_frame_ms != 0 {
            break;
        }

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_WAIT_POLL_MS)).await;
    }

    let min_end_ms = first_frame_ms.saturating_add(LOADSCREEN_MIN_LIFETIME_MS);

    loop {
        let now_ms = boot_probe_ms();
        let min_lifetime_reached = now_ms >= min_end_ms;
        let expire_requested = crate::r::readiness::loadscreen_expire_requested();
        if min_lifetime_reached && expire_requested {
            crate::r::readiness::set(crate::r::readiness::LOADSCREEN_END);
            break;
        }

        crate::gfx::with_cabi_frame_lock(|| {
            let _ = render_loadscreen_frame(LOADSCREEN_BG_RGB);
        });

        Timer::after(EmbassyDuration::from_millis(LOADSCREEN_ANIM_FRAME_MS)).await;
    }

    crate::log!(
        "boot-probe: loadscreen end ms={} lived_ms={}\n",
        boot_probe_ms(),
        boot_probe_ms().saturating_sub(first_frame_ms)
    );
}
