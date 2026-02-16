pub mod html;
pub mod http_trueosfs;
pub mod tls_demo;
pub mod ws_smoke;

use embassy_executor::task;

#[task]
pub(crate) async fn boot_parse5_smoke_task() {
    crate::log!("qjs-parse5-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_parse5_smoke() };
    crate::log!("qjs-common-modules-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_common_modules_smoke() };
    crate::log!("qjs-parse5-smoke: done\n");
    crate::v::readiness::set(crate::v::readiness::QJS_PARSE5_SMOKE_DONE);
}

#[task]
pub(crate) async fn boot_pixi_smoke_task() {
    crate::log!("qjs-pixi-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_pixi_import_smoke() };
    crate::log!("qjs-pixi-smoke: done\n");
}

#[task]
pub(crate) async fn boot_gfx_virtio_sw_prepare_task() {
    use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

    async fn wait_for_virtio_sw_stable(min_epoch: u64, timeout_ms: u64, settle_ms: u64) -> bool {
        let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
        let mut stable_ms: u64 = 0;
        loop {
            let kind_ok = crate::gfx::backend_kind() == Some(crate::gfx::BackendKind::VirtioSw);
            let epoch_ok = crate::gfx::backend_epoch() >= min_epoch;
            if kind_ok && epoch_ok {
                stable_ms = stable_ms.saturating_add(25);
                if stable_ms >= settle_ms {
                    return true;
                }
            } else {
                stable_ms = 0;
            }
            if Instant::now() >= deadline {
                return false;
            }
            Timer::after(EmbassyDuration::from_millis(25)).await;
        }
    }

    crate::log!("qjs-gfx-prepare: waiting for stable VirtioSw gfx\n");
    let epoch0 = crate::gfx::backend_epoch();

    // First chance: if another path already switched to VirtioSw, wait for brief stability.
    if wait_for_virtio_sw_stable(epoch0, 1500, 250).await {
        crate::v::readiness::set(crate::v::readiness::GFX_VIRTIO_SW_READY);
        crate::log!("qjs-gfx-prepare: ready (virtio_sw already active)\n");
        return;
    }

    crate::log!("qjs-gfx-prepare: forcing gfx switch_to_virtio_sw\n");
    if !crate::gfx::switch_to_virtio_sw() {
        crate::log!("qjs-gfx-prepare: failed (switch_to_virtio_sw failed)\n");
        return;
    }

    let target_epoch = epoch0.saturating_add(1);
    if !wait_for_virtio_sw_stable(target_epoch, 2500, 300).await {
        crate::log!("qjs-gfx-prepare: failed (virtio_sw not stable)\n");
        return;
    }
    crate::v::readiness::set(crate::v::readiness::GFX_VIRTIO_SW_READY);
    crate::log!(
        "qjs-gfx-prepare: ready epoch={}\n",
        crate::gfx::backend_epoch()
    );
}

#[task]
pub(crate) async fn boot_pixi_rect_smoke_task() {
    use embassy_time::Duration as EmbassyDuration;

    if !crate::v::readiness::wait_for_timeout(
        crate::v::readiness::GFX_VIRTIO_SW_READY,
        EmbassyDuration::from_secs(8),
    )
    .await
    {
        crate::log!("qjs-pixi-rect-smoke: skip (gfx prepare timeout)\n");
        return;
    }

    crate::log!("qjs-pixi-rect-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_pixi_rect_smoke() };
    crate::log!("qjs-pixi-rect-smoke: done\n");
}
