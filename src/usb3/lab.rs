//! Snapshot and quarantine coordination for the live CrabUSB xHCI owner.

use alloc::{format, string::{String, ToString}};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};
use super::crabusb::{self, diag::{XhciControllerSnapshot, XhciDirectRequest, XhciDirectResponse}};

const QUIESCE_TIMEOUT_MS: u64 = 5_000;
static LATEST_SNAPSHOT: Mutex<Option<XhciControllerSnapshot>> = Mutex::new(None);
static QUARANTINE_REQUESTED: AtomicBool = AtomicBool::new(false);
static QUARANTINE_ACTIVE: AtomicBool = AtomicBool::new(false);
static UAS_IO_INFLIGHT: AtomicU32 = AtomicU32::new(0);

pub(crate) fn latest_snapshot() -> Option<XhciControllerSnapshot> {
    LATEST_SNAPSHOT.lock().clone()
}

pub(crate) async fn refresh_snapshot(host: &mut crabusb::USBHost) -> Result<(), String> {
    snapshot(host).await.map(|_| ())
}




pub(crate) struct UasIoGuard;

impl Drop for UasIoGuard {
    fn drop(&mut self) {
        UAS_IO_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Admit one UAS operation unless a laboratory run is requesting quiescence.
/// The second check closes the race between admission and quarantine entry.
fn try_begin_uas_io() -> crate::disc::block::Result<UasIoGuard> {
    if QUARANTINE_REQUESTED.load(Ordering::Acquire) {
        return Err(crate::disc::block::Error::NotReady);
    }
    UAS_IO_INFLIGHT.fetch_add(1, Ordering::AcqRel);
    if QUARANTINE_REQUESTED.load(Ordering::Acquire) {
        UAS_IO_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        return Err(crate::disc::block::Error::NotReady);
    }
    Ok(UasIoGuard)
}

pub(crate) async fn begin_uas_io_when_available() -> UasIoGuard {
    loop {
        match try_begin_uas_io() {
            Ok(guard) => return guard,
            Err(_) => Timer::after(Duration::from_millis(1)).await,
        }
    }
}

pub(crate) struct ControllerQuarantineGuard;

impl Drop for ControllerQuarantineGuard {
    fn drop(&mut self) {
        QUARANTINE_ACTIVE.store(false, Ordering::Release);
        QUARANTINE_REQUESTED.store(false, Ordering::Release);
    }
}

pub(crate) async fn enter_controller_quarantine() -> Result<ControllerQuarantineGuard, String> {
    if QUARANTINE_REQUESTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("xhci-lab quarantine is already requested".to_string());
    }
    let deadline = Instant::now() + Duration::from_millis(QUIESCE_TIMEOUT_MS);
    loop {
        let inflight = UAS_IO_INFLIGHT.load(Ordering::Acquire);
        if inflight == 0 {
            QUARANTINE_ACTIVE.store(true, Ordering::Release);
            return Ok(ControllerQuarantineGuard);
        }
        if Instant::now() >= deadline {
            QUARANTINE_REQUESTED.store(false, Ordering::Release);
            return Err(format!(
                "xhci-lab quarantine timed out with {inflight} UAS operation(s) in flight"
            ));
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}





async fn snapshot(host: &mut crabusb::USBHost) -> Result<XhciControllerSnapshot, String> {
    let response = host
        .xhci_direct(XhciDirectRequest::Snapshot)
        .await
        .map_err(|err| format!("xHCI snapshot failed: {err:?}"))?;
    match response {
        XhciDirectResponse::Snapshot(snapshot) => {
            *LATEST_SNAPSHOT.lock() = Some(snapshot.clone());
            Ok(snapshot)
        }
        other => Err(format!("snapshot returned unexpected response: {other:?}")),
    }
}
