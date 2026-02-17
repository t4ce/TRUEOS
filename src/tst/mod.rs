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
    unsafe { trueos_qjs::trueos_smoke::run_pixi_rgbtri_smoke() };
    crate::log!("qjs-pixi-smoke: done\n");
}
