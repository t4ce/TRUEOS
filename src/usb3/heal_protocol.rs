//! JSON/Serde boundary for the kernel xHCI Heal service.
//!
//! The protocol exposes trusted patterns, not raw register writes. The first
//! executable pattern proves controller transport. USB2 port activation is
//! intentionally declared but rejected until its state machine lands.

use alloc::{
    collections::VecDeque,
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};

use embassy_sync::signal::Signal;
use serde::{Deserialize, Serialize};
use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::{crabusb, heal_service};

pub const HEAL_API_VERSION: u16 = 1;
pub const HEAL_MAX_REQUEST_BYTES: usize = 8 * 1024;
const REQUEST_CAPACITY: usize = 8;
const IDLE_MS: u64 = 10;

static REQUESTS: Mutex<VecDeque<QueuedRequest>> = Mutex::new(VecDeque::new());

type ReplySignal = Signal<crate::wait::EmbassySpinRawMutex, HealResponse>;

struct QueuedRequest {
    request: HealRequest,
    reply: Arc<ReplySignal>,
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
    ControllerTransportV1 { max_slots: u8, noop_timeout_ms: u32 },
    Usb2BootHidV1 { port: HealPortSelector },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum HealRequest {
    Describe {
        api: u16,
    },
    Observe {
        api: u16,
    },
    Run {
        api: u16,
        expected_revision: u64,
        pattern: HealPattern,
    },
    Commit {
        api: u16,
        expected_revision: u64,
        proof_id: String,
    },
}

impl HealRequest {
    const fn api(&self) -> u16 {
        match self {
            Self::Describe { api }
            | Self::Observe { api }
            | Self::Run { api, .. }
            | Self::Commit { api, .. } => *api,
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
    PortActivationPending,
    GoalNotProven,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealPatternDescription {
    pub name: &'static str,
    pub executable: bool,
    pub proves: &'static str,
    pub boundary: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealApiDescription {
    pub api: u16,
    pub max_request_bytes: usize,
    pub request_capacity: usize,
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
    TooLarge,
    InvalidJson,
    QueueFull,
}

pub struct PendingHealRequest {
    reply: Arc<ReplySignal>,
}

impl PendingHealRequest {
    pub async fn wait(self) -> HealResponse {
        self.reply.wait().await
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
    let mut queue = REQUESTS.lock();
    if queue.len() >= REQUEST_CAPACITY {
        return Err(HealSubmitError::QueueFull);
    }
    let reply = Arc::new(Signal::new());
    queue.push_back(QueuedRequest {
        request,
        reply: Arc::clone(&reply),
    });
    Ok(PendingHealRequest { reply })
}

pub fn api_description_json() -> Vec<u8> {
    serde_json::to_vec(&api_description()).unwrap_or_default()
}

pub fn latest_report_json() -> Vec<u8> {
    serde_json::to_vec(&heal_service::latest_report()).unwrap_or_default()
}

pub(crate) async fn service_one(
    host: &mut crabusb::USBHost,
    report: &mut heal_service::HealServiceReport,
) -> bool {
    let Some(queued) = REQUESTS.lock().pop_front() else {
        return false;
    };
    let response = execute(Some(host), report, queued.request).await;
    queued.reply.signal(response);
    true
}

pub(crate) async fn service_without_controller(mut report: heal_service::HealServiceReport) -> ! {
    loop {
        if let Some(queued) = REQUESTS.lock().pop_front() {
            let response = execute(None, &mut report, queued.request).await;
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
            format!("requested API {} but kernel exposes {}", request.api(), HEAL_API_VERSION),
        );
    }

    match request {
        HealRequest::Describe { .. } => HealResponse {
            api: HEAL_API_VERSION,
            session_id: report.session_id,
            revision: report.revision,
            status: HealResponseStatus::Ok,
            error: None,
            detail: None,
            description: Some(api_description()),
            report: Some(report.clone()),
        },
        HealRequest::Observe { .. } => ok(report, None),
        HealRequest::Run {
            expected_revision,
            pattern,
            ..
        } => {
            if expected_revision != report.revision {
                return rejected(
                    report,
                    HealErrorCode::StaleRevision,
                    format!(
                        "expected revision {} but current revision is {}",
                        expected_revision, report.revision
                    ),
                );
            }
            let Some(host) = host else {
                return rejected(
                    report,
                    HealErrorCode::ControllerUnavailable,
                    "the quarantined controller was not constructed".to_string(),
                );
            };
            match pattern {
                HealPattern::ControllerTransportV1 {
                    max_slots,
                    noop_timeout_ms,
                } => match heal_service::prove_transport(
                    host,
                    report,
                    max_slots.clamp(1, 32),
                    noop_timeout_ms.clamp(100, 5_000),
                )
                .await
                {
                    Ok(()) => ok(report, Some("controller transport proven".to_string())),
                    Err(detail) => rejected(report, HealErrorCode::TransportFailure, detail),
                },
                HealPattern::Usb2BootHidV1 { port } => rejected(
                    report,
                    HealErrorCode::PortActivationPending,
                    format!(
                        "usb2_boot_hid_v1 reached the explicit connected-to-active boundary at {:?}; no port mutation was performed",
                        port
                    ),
                ),
            }
        }
        HealRequest::Commit {
            expected_revision,
            proof_id,
            ..
        } => {
            if expected_revision != report.revision {
                return rejected(
                    report,
                    HealErrorCode::StaleRevision,
                    format!(
                        "expected revision {} but current revision is {}",
                        expected_revision, report.revision
                    ),
                );
            }
            rejected(
                report,
                HealErrorCode::GoalNotProven,
                format!(
                    "transport proof {proof_id} cannot unseal the controller before USB2 port activation and boot-HID descriptor proof"
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
        patterns: vec![
            HealPatternDescription {
                name: "controller_transport_v1",
                executable: true,
                proves: "bounded reset, conservative DMA command/event rings, and an xHCI No-op completion",
                boundary: "does not power, reset, address, configure, or bind a downstream USB device",
            },
            HealPatternDescription {
                name: "usb2_boot_hid_v1",
                executable: false,
                proves: "future connected-to-active USB2 port sequence and boot-HID descriptor proof",
                boundary: "currently returns port_activation_pending without touching PORTSC",
            },
        ],
    }
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
        report: Some(report.clone()),
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
        report: Some(report.clone()),
    }
}
