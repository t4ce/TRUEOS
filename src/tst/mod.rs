pub mod html;
pub mod http_trueosfs;
pub mod nalgebra_demo;
pub mod smoke_fs;
pub mod tls_demo;

use embassy_executor::task;
use embassy_time::Duration as EmbassyDuration;

#[task]
pub(crate) async fn boot_parse5_smoke_task() {
    if !crate::v::readiness::wait_for_timeout(
        crate::v::readiness::QJS_ASYNC_FS_READY,
        EmbassyDuration::from_secs(5),
    )
    .await
    {
        crate::log!("qjs-parse5-smoke: skipped (qjs async fs not ready)\n");
        return;
    }
    crate::log!("qjs-parse5-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_parse5_smoke() };
    crate::log!("qjs-parse5-smoke: done\n");
}
