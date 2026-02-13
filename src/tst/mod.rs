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
    crate::log!("qjs-parse5-smoke: done\n");
}
