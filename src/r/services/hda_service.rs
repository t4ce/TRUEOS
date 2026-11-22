//! Owns the Intel HDA playback controller and feeds it PCM from the shared audio sink.

#[trueos_executor::task]
pub(crate) async fn hda_service_task() {
    crate::intel::hda::service().await;
}
