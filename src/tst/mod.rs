pub mod html;
pub mod http_trueosfs;
pub mod nalgebra_demo;
pub mod smoke_fs;
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
pub(crate) async fn boot_pixi_rect_smoke_task() {
    use embassy_time::{Duration as EmbassyDuration, Timer};

    crate::log!("qjs-pixi-rect-smoke: waiting for VirtioSw gfx\n");

    // Wait until the user (or system) switches gfx into a virtio-backed scanout.
    // This avoids drawing into the Limine/VGA console framebuffer.
    loop {
        if crate::gfx::backend_kind() == Some(crate::gfx::BackendKind::VirtioSw) {
            crate::log!("qjs-pixi-rect-smoke: starting\n");
            unsafe { trueos_qjs::trueos_smoke::run_pixi_rect_smoke() };
            crate::log!("qjs-pixi-rect-smoke: done\n");
            return;
        }
        Timer::after(EmbassyDuration::from_millis(100)).await;
    }
}
