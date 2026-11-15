//! Idle owner for the Intel HDA PCI family. No playback or capture path.

#[trueos_executor::task]
pub(crate) async fn hda_service_task() {
    crate::intel::hda::claim_devices();
    core::future::pending::<()>().await;
}
