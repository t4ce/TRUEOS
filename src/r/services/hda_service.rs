//! Owns the Intel HDA playback controller and drives the vHDA mixer.

#[trueos_executor::task]
pub(crate) async fn hda_service_task() {
    crate::intel::hda::service().await;
}
