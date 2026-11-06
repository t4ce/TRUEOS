//! Snapshot and quarantine coordination for the live CrabUSB xHCI owner.

use super::crabusb::{
    self,
    diag::{XhciControllerSnapshot, XhciDirectRequest, XhciDirectResponse},
};
use alloc::{
    format,
    string::{String, ToString},
};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

const QUIESCE_TIMEOUT_MS: u64 = 5_000;
static LATEST_SNAPSHOT: [Mutex<Option<XhciControllerSnapshot>>; super::MAX_CRABUSB_CONTROLLERS] =
    [const { Mutex::new(None) }; super::MAX_CRABUSB_CONTROLLERS];
static QUARANTINE_REQUESTED: AtomicBool = AtomicBool::new(false);
static QUARANTINE_ACTIVE: AtomicBool = AtomicBool::new(false);
static UAS_IO_INFLIGHT: AtomicU32 = AtomicU32::new(0);

pub(crate) fn latest_snapshot(index: usize) -> Option<XhciControllerSnapshot> {
    LATEST_SNAPSHOT.get(index)?.lock().clone()
}

pub(crate) async fn refresh_snapshot(
    index: usize,
    host: &mut crabusb::USBHost,
) -> Result<(), String> {
    match snapshot(host).await {
        Ok(snapshot) => {
            let Some(latest) = LATEST_SNAPSHOT.get(index) else {
                return Err(format!(
                    "snapshot controller index {index} exceeds runtime limit"
                ));
            };
            super::lib::reconcile_disconnected_roots(index, &snapshot);
            *latest.lock() = Some(snapshot);
            super::runtime::update(index, |runtime| {
                runtime.snapshot_sequence = runtime.snapshot_sequence.saturating_add(1);
                runtime.snapshot_at_ms = Some(Instant::now().as_millis());
                runtime.snapshot_error = None;
            });
            Ok(())
        }
        Err(error) => {
            super::runtime::update(index, |runtime| {
                runtime.snapshot_error = Some(error.clone())
            });
            Err(error)
        }
    }
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
    // Own the request immediately: cancellation while waiting must also reopen
    // UAS admission, rather than leaving the controller quarantined forever.
    let guard = ControllerQuarantineGuard;
    let deadline = Instant::now() + Duration::from_millis(QUIESCE_TIMEOUT_MS);
    loop {
        let inflight = UAS_IO_INFLIGHT.load(Ordering::Acquire);
        if inflight == 0 {
            QUARANTINE_ACTIVE.store(true, Ordering::Release);
            return Ok(guard);
        }
        if Instant::now() >= deadline {
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
        XhciDirectResponse::Snapshot(snapshot) => Ok(snapshot),
        other => Err(format!("snapshot returned unexpected response: {other:?}")),
    }
}
