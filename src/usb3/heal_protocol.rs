//! Owner-routed JSON boundary for xHCI observation and explicit, guarded repair.
//!
//! Live controllers retain their normal event pump. Whole-controller transport
//! preparation is only permitted in quarantine, before any device is admitted.

use alloc::{
    collections::VecDeque,
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_sync::signal::Signal;
use serde::{Deserialize, Serialize};
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use super::{crabusb, heal_service};

pub const HEAL_API_VERSION: u16 = 1;
pub const HEAL_MAX_REQUEST_BYTES: usize = 8 * 1024;
const REQUEST_CAPACITY: usize = 8;
const REQUEST_TIMEOUT_MS: u64 = 8_000;
const IDLE_MS: u64 = 10;
const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const CANCELLED: u8 = 2;
const COMPLETE: u8 = 3;

static REQUESTS: Mutex<VecDeque<QueuedRequest>> = Mutex::new(VecDeque::new());
type ReplySignal = Signal<crate::wait::EmbassySpinRawMutex, HealResponse>;

struct QueuedRequest {
    request: HealRequest,
    reply: Arc<ReplySignal>,
    state: Arc<AtomicU8>,
    deadline: Instant,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HealPortSelector {
    FirstConnected,
    Port { port_id: u8 },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "name", rename_all = "snake_case", deny_unknown_fields)]
pub enum HealPattern {
    ControllerSnapshotV1,
    ControllerTransportV1 {
        max_slots: u8,
        noop_timeout_ms: u32,
    },
    Usb2PortResetV1 {
        port: HealPortSelector,
        timeout_ms: u32,
    },
    Usb2BootHidV1 {
        port: HealPortSelector,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum HealRequest {
    Describe {
        api: u16,
        #[serde(default)]
        controller: usize,
    },
    Observe {
        api: u16,
        #[serde(default)]
        controller: usize,
    },
    Run {
        api: u16,
        #[serde(default)]
        controller: usize,
        expected_revision: u64,
        pattern: HealPattern,
    },
    Commit {
        api: u16,
        #[serde(default)]
        controller: usize,
        expected_revision: u64,
        proof_id: String,
    },
}

impl HealRequest {
    const fn api(&self) -> u16 {
        match self {
            Self::Describe { api, .. }
            | Self::Observe { api, .. }
            | Self::Run { api, .. }
            | Self::Commit { api, .. } => *api,
        }
    }

    const fn controller(&self) -> usize {
        match self {
            Self::Describe { controller, .. }
            | Self::Observe { controller, .. }
            | Self::Run { controller, .. }
            | Self::Commit { controller, .. } => *controller,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealResponseStatus {
    Ok,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealErrorCode {
    InvalidApi,
    StaleRevision,
    ControllerUnavailable,
    TransportFailure,
    LiveControllerProtected,
    ObservationFailure,
    PortResetFailure,
    PortActivationPending,
    GoalNotProven,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealPatternDescription {
    pub name: &'static str,
    pub executable: bool,
    pub available_on: &'static str,
    pub proves: &'static str,
    pub boundary: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealApiDescription {
    pub api: u16,
    pub max_request_bytes: usize,
    pub request_capacity: usize,
    pub request_timeout_ms: u64,
    pub controller_selection: &'static str,
    pub patterns: Vec<HealPatternDescription>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealResponse {
    pub api: u16,
    pub session_id: u64,
    pub revision: u64,
    pub status: HealResponseStatus,
    pub error: Option<HealErrorCode>,
    pub detail: Option<String>,
    pub description: Option<HealApiDescription>,
    pub report: Option<heal_service::HealServiceReport>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealSubmitError {
    OwnerBusy,
    TooLarge,
    InvalidJson,
    QueueFull,
    ControllerUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealWaitError {
    ExpiredBeforeExecution,
    OperationInProgress,
}

pub struct PendingHealRequest {
    reply: Arc<ReplySignal>,
    state: Arc<AtomicU8>,
}

impl PendingHealRequest {
    pub async fn wait(self) -> Result<HealResponse, HealWaitError> {
        match trueos_time::with_timeout(
            Duration::from_millis(REQUEST_TIMEOUT_MS),
            self.reply.wait(),
        )
        .await
        {
            Ok(response) => Ok(response),
            Err(_) => {
                // Atomically cancel only queued work. An owner that already
                // started a bounded operation keeps its future and DMA alive.
                match self.state.compare_exchange(
                    QUEUED,
                    CANCELLED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => Err(HealWaitError::ExpiredBeforeExecution),
                    Err(CANCELLED) => Err(HealWaitError::ExpiredBeforeExecution),
                    Err(_) => Err(HealWaitError::OperationInProgress),
                }
            }
        }
    }
}

impl Drop for PendingHealRequest {
    fn drop(&mut self) {
        let _ = self
            .state
            .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub fn submit_json(bytes: &[u8]) -> Result<PendingHealRequest, HealSubmitError> {
    if bytes.len() > HEAL_MAX_REQUEST_BYTES {
        return Err(HealSubmitError::TooLarge);
    }
    let request = serde_json::from_slice(bytes).map_err(|_| HealSubmitError::InvalidJson)?;
    submit(request)
}

pub fn submit(request: HealRequest) -> Result<PendingHealRequest, HealSubmitError> {
    if !heal_service::reports()
        .iter()
        .any(|report| report.controller_index == request.controller())
    {
        return Err(HealSubmitError::ControllerUnavailable);
    }
    if matches!(
        &request,
        HealRequest::Run {
            pattern: HealPattern::Usb2PortResetV1 { .. },
            ..
        }
    ) {
        let owner = super::runtime::snapshot(request.controller());
        if !owner.event_ready || owner.probe_requested || owner.operation_requested {
            return Err(HealSubmitError::OwnerBusy);
        }
    }
    let mut queue = REQUESTS.lock();
    queue.retain(|queued| {
        queued.state.load(Ordering::Acquire) == QUEUED && Instant::now() < queued.deadline
    });
    if queue.len() >= REQUEST_CAPACITY {
        return Err(HealSubmitError::QueueFull);
    }
    let reply = Arc::new(Signal::new());
    let state = Arc::new(AtomicU8::new(QUEUED));
    queue.push_back(QueuedRequest {
        request,
        reply: Arc::clone(&reply),
        state: Arc::clone(&state),
        deadline: Instant::now() + Duration::from_millis(REQUEST_TIMEOUT_MS),
    });
    Ok(PendingHealRequest { reply, state })
}

pub fn api_description_json() -> Vec<u8> {
    serde_json::to_vec(&api_description()).unwrap_or_default()
}

/// Immediate cached census remains available if a host is busy in enumeration.
pub fn latest_report_json() -> Vec<u8> {
    serde_json::to_vec(&heal_service::reports()).unwrap_or_default()
}

fn take_for_controller(controller: usize) -> Option<QueuedRequest> {
    let mut queue = REQUESTS.lock();
    queue.retain(|queued| {
        if Instant::now() >= queued.deadline {
            let _ = queued.state.compare_exchange(
                QUEUED,
                CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        queued.state.load(Ordering::Acquire) == QUEUED
    });
    let index = queue
        .iter()
        .position(|queued| queued.request.controller() == controller)?;
    let queued = queue.remove(index)?;
    // A timed-out waiter and the owner race through this one transition. The
    // loser cannot execute work that the caller successfully cancelled.
    if queued
        .state
        .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return None;
    }
    Some(queued)
}

pub(crate) async fn service_one(
    host: &mut crabusb::USBHost,
    report: &mut heal_service::HealServiceReport,
) -> bool {
    let Some(queued) = take_for_controller(report.controller_index) else {
        return false;
    };
    let response = execute(Some(host), report, queued.request).await;
    queued.state.store(COMPLETE, Ordering::Release);
    queued.reply.signal(response);
    true
}

pub(crate) async fn service_without_controller(mut report: heal_service::HealServiceReport) -> ! {
    loop {
        if let Some(queued) = take_for_controller(report.controller_index) {
            let response = execute(None, &mut report, queued.request).await;
            queued.state.store(COMPLETE, Ordering::Release);
            queued.reply.signal(response);
        } else {
            Timer::after(Duration::from_millis(IDLE_MS)).await;
        }
    }
}

async fn execute(
    host: Option<&mut crabusb::USBHost>,
    report: &mut heal_service::HealServiceReport,
    request: HealRequest,
) -> HealResponse {
    if request.api() != HEAL_API_VERSION {
        return rejected(
            report,
            HealErrorCode::InvalidApi,
            format!(
                "requested API {} but kernel exposes {}",
                request.api(),
                HEAL_API_VERSION
            ),
        );
    }
    match request {
        HealRequest::Describe { .. } => {
            let mut response = ok(report, None);
            response.description = Some(api_description());
            response
        }
        HealRequest::Observe { .. } => {
            let Some(host) = host else {
                return ok(
                    report,
                    Some("cached report; controller construction failed".to_string()),
                );
            };
            match heal_service::observe_controller(host, report).await {
                Ok(()) => ok(
                    report,
                    Some("fresh owner-thread register snapshot".to_string()),
                ),
                Err(detail) => rejected(report, HealErrorCode::ObservationFailure, detail),
            }
        }
        HealRequest::Run {
            expected_revision,
            pattern,
            ..
        } => {
            if expected_revision != report.revision {
                return stale(report, expected_revision);
            }
            let Some(host) = host else {
                return rejected(
                    report,
                    HealErrorCode::ControllerUnavailable,
                    "controller construction failed".to_string(),
                );
            };
            match pattern {
                HealPattern::ControllerSnapshotV1 => match heal_service::observe_controller(host, report).await {
                    Ok(()) => ok(report, Some("fresh owner-thread register snapshot".to_string())),
                    Err(detail) => rejected(report, HealErrorCode::ObservationFailure, detail),
                },
                HealPattern::ControllerTransportV1 { max_slots, noop_timeout_ms } => {
                    if report.mode != heal_service::HealControllerMode::Quarantined {
                        return rejected(report, HealErrorCode::LiveControllerProtected,
                            "whole-controller transport proof is restricted to quarantine; live/parked DMA and event ownership are preserved".to_string());
                    }
                    match heal_service::prove_transport(host, report, max_slots.clamp(1,32), noop_timeout_ms.clamp(100,5_000)).await {
                        Ok(()) => ok(report, Some("controller transport proven".to_string())),
                        Err(detail) => rejected(report, HealErrorCode::TransportFailure, detail),
                    }
                }
                HealPattern::Usb2PortResetV1 { port, timeout_ms } =>
                    match heal_service::reset_unprobed_usb2_port(host, report, port, timeout_ms).await {
                        Ok(()) => ok(report, Some("unprobed USB2 port active; normal device discovery requested; HID binding is not yet proven".to_string())),
                        Err(detail) => rejected(report, HealErrorCode::PortResetFailure, detail),
                    },
                HealPattern::Usb2BootHidV1 { port } => rejected(report, HealErrorCode::PortActivationPending,
                    format!("boot-HID descriptor/class proof is not implemented at {port:?}; use usb2_port_reset_v1 for guarded unprobed-port repair and inspect normal class discovery")),
            }
        }
        HealRequest::Commit {
            expected_revision,
            proof_id,
            ..
        } => {
            if expected_revision != report.revision {
                return stale(report, expected_revision);
            }
            rejected(
                report,
                HealErrorCode::GoalNotProven,
                format!(
                    "proof {proof_id} cannot authorize quarantined class binding without a boot-HID descriptor proof"
                ),
            )
        }
    }
}

fn api_description() -> HealApiDescription {
    HealApiDescription {
        api: HEAL_API_VERSION,
        max_request_bytes: HEAL_MAX_REQUEST_BYTES,
        request_capacity: REQUEST_CAPACITY,
        request_timeout_ms: REQUEST_TIMEOUT_MS,
        controller_selection: "controller is the zero-based discovery index; default 0; observe lists PCI BDF and independent session/revision per controller",
        patterns: vec![
            HealPatternDescription {
                name: "controller_snapshot_v1",
                executable: true,
                available_on: "live, parked, quarantine",
                proves: "fresh read-only owner-thread register/protocol/root-port census",
                boundary: "never consumes events or writes registers",
            },
            HealPatternDescription {
                name: "controller_transport_v1",
                executable: true,
                available_on: "quarantine only",
                proves: "bounded reset, conservative DMA rings and xHCI No-op completion",
                boundary: "blocked on normal live or parked controllers; no downstream device binding",
            },
            HealPatternDescription {
                name: "usb2_port_reset_v1",
                executable: true,
                available_on: "live only; connected-disabled USB2 root ports never admitted to addressing",
                proves: "PORTSC before/after plus bounded reset completion; requests normal owner discovery",
                boundary: "refuses enabled, disconnected, resetting, ignored-policy and ever-probed ports; does not claim HID proof",
            },
            HealPatternDescription {
                name: "usb2_boot_hid_v1",
                executable: false,
                available_on: "unavailable",
                proves: "not implemented",
                boundary: "returns port_activation_pending without port mutation; commit cannot unseal class binding",
            },
        ],
    }
}

fn stale(report: &heal_service::HealServiceReport, expected: u64) -> HealResponse {
    rejected(
        report,
        HealErrorCode::StaleRevision,
        format!(
            "expected revision {} but current revision is {}",
            expected, report.revision
        ),
    )
}

fn ok(report: &heal_service::HealServiceReport, detail: Option<String>) -> HealResponse {
    HealResponse {
        api: HEAL_API_VERSION,
        session_id: report.session_id,
        revision: report.revision,
        status: HealResponseStatus::Ok,
        error: None,
        detail,
        description: None,
        report: Some(heal_service::response_report(report)),
    }
}

fn rejected(
    report: &heal_service::HealServiceReport,
    code: HealErrorCode,
    detail: String,
) -> HealResponse {
    HealResponse {
        api: HEAL_API_VERSION,
        session_id: report.session_id,
        revision: report.revision,
        status: HealResponseStatus::Rejected,
        error: Some(code),
        detail: Some(detail),
        description: None,
        report: Some(heal_service::response_report(report)),
    }
}
