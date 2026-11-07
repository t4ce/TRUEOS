//! xHCI admission, per-controller observation and guarded explicit repair.
//!
//! Known Intel/QEMU hosts keep their normal event pumps and class ownership.
//! Unsupported hosts remain quarantined. Read-only snapshots work in either
//! mode; transport reset is quarantine-only. Live USB2 root-port repair uses
//! the normal PORTSC-neutral state machine and refuses ever-addressed ports.

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::crabusb::{
    self,
    diag::XhciControllerSnapshot,
    recover::{
        XhciNoopProof, XhciRecoveryEvent, XhciRecoveryRequest, XhciRecoveryResponse,
        XhciTransportProof,
    },
};

const NORMAL_XHCI_CLAIM_OWNER: &str = "crabusb-xhci";
const HEAL_XHCI_CLAIM_OWNER: &str = "xhci-heal";

const INTEL_VENDOR_ID: u16 = 0x8086;
const INTEL_700_SERIES_XHCI_DEVICE_ID: u16 = 0x7a60;
const QEMU_XHCI_VENDOR_ID: u16 = 0x1b36;
const QEMU_XHCI_DEVICE_ID: u16 = 0x000d;
const NEC_XHCI_VENDOR_ID: u16 = 0x1033;
const NEC_QEMU_XHCI_DEVICE_ID: u16 = 0x0194;

const PCI_REVISION_ID: u16 = 0x08;
const PCI_SUBSYSTEM_VENDOR_ID: u16 = 0x2c;
const PCI_SUBSYSTEM_DEVICE_ID: u16 = 0x2e;

const USBSTS_CONTROLLER_NOT_READY: u32 = 1 << 11;
const HCCPARAMS1_PORT_POWER_CONTROL: u32 = 1 << 3;
const PORT_CCS: u32 = 1 << 0;
const PORT_PED: u32 = 1 << 1;
const PORT_PR: u32 = 1 << 4;
const PORT_PLS_MASK: u32 = 0x0f << 5;
const PORT_PP: u32 = 1 << 9;
const PORT_SPEED_MASK: u32 = 0x0f << 10;
const PORT_WPR: u32 = 1 << 31;
const HEAL_SERVICE_IDLE_MS: u64 = 10;
const DEFAULT_MAX_SLOTS: u8 = 8;
const DEFAULT_NOOP_TIMEOUT_MS: u32 = 1_000;

static LATEST_SELECTION: Mutex<Option<XhciBackendSelection>> = Mutex::new(None);
static REPORTS: Mutex<Vec<HealServiceReport>> = Mutex::new(Vec::new());
static RESCAN_REQUESTED: [core::sync::atomic::AtomicBool; super::MAX_CRABUSB_CONTROLLERS] =
    [const { core::sync::atomic::AtomicBool::new(false) }; super::MAX_CRABUSB_CONTROLLERS];
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum IntelXhciProfile {
    Series700 { revision: u8 },
    CompatibleFamily { device_id: u16, revision: u8 },
}

impl IntelXhciProfile {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Series700 { .. } => "intel-700-series-xhci",
            Self::CompatibleFamily { .. } => "intel-xhci-compatible-family",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum QemuXhciProfile {
    PciXhci,
    NecEmulatedXhci,
}

impl QemuXhciProfile {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::PciXhci => "qemu-xhci",
            Self::NecEmulatedXhci => "qemu-nec-xhci",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct HealXhciSeed {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub revision: u8,
    pub subsystem_vendor_id: u16,
    pub subsystem_device_id: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum XhciBackendSelection {
    KnownIntel(IntelXhciProfile),
    KnownQemu(QemuXhciProfile),
    HealRequired(HealXhciSeed),
}

impl XhciBackendSelection {
    const fn claim_owner(self) -> &'static str {
        match self {
            Self::KnownIntel(_) | Self::KnownQemu(_) => NORMAL_XHCI_CLAIM_OWNER,
            Self::HealRequired(_) => HEAL_XHCI_CLAIM_OWNER,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealServiceStage {
    NormalRuntime,
    Parked,
    Quarantined,
    ControllerConstructed,
    CapabilitiesReadable,
    TransportPrepared,
    TransportProven,
    AwaitingPortActivation,
    Failed,
}

impl HealServiceStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NormalRuntime => "normal-runtime",
            Self::Parked => "parked",
            Self::Quarantined => "quarantined",
            Self::ControllerConstructed => "controller-constructed",
            Self::CapabilitiesReadable => "capabilities-readable",
            Self::TransportPrepared => "transport-prepared",
            Self::TransportProven => "transport-proven",
            Self::AwaitingPortActivation => "awaiting-port-activation",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HealProtocolReport {
    pub major: u8,
    pub minor: u8,
    pub port_offset: u8,
    pub port_count: u8,
    pub slot_type: u8,
    pub psi_count: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealPortReport {
    pub port_id: u8,
    pub protocol_major: Option<u8>,
    pub protocol_minor: Option<u8>,
    pub connected: bool,
    pub enabled: bool,
    pub power_attempted: bool,
    pub powered_before: bool,
    pub powered_after: bool,
    pub reset_active: bool,
    pub link_state: u8,
    pub speed_id: u8,
    pub before_portsc: u32,
    pub after_portsc: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealTransportProof {
    pub proof_id: String,
    pub dma_bits: u8,
    pub max_slots_enabled: u8,
    pub noop_waited_ms: u32,
    pub command_trb_pointer: u64,
    pub completion_slot_id: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealControllerMode {
    Initializing,
    Quarantined,
    Live,
    Parked,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealRegisters {
    pub usbcmd: u32,
    pub usbsts: u32,
    pub config: u32,
    pub crcr: u64,
    pub dcbaap: u64,
    pub erdp: u64,
    pub iman: u32,
    pub mfindex: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealPortResetProof {
    pub port_id: u8,
    pub before_portsc: u32,
    pub after_portsc: u32,
    pub waited_ms: u32,
    pub completed: bool,
    pub rescan_requested: bool,
    pub request_accepted: bool,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealEnumerationFailure {
    pub root_port_id: u8,
    pub port_id: u8,
    pub slot_id: Option<u8>,
    pub stage: &'static str,
    pub reason: String,
    pub reason_truncated: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct HealEnumerationReport {
    pub failure_count: u64,
    pub first_failure: Option<HealEnumerationFailure>,
    pub last_failure: Option<HealEnumerationFailure>,
    pub snapshot_at_ms: Option<u64>,
    pub snapshot_age_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealOwnerStatus {
    pub controller_phase: &'static str,
    pub root_hub_lifecycle: &'static str,
    pub event_ready: bool,
    pub probe_requested: bool,
    pub operation_requested: bool,
    pub operation_stage: &'static str,
    pub operation_elapsed_ms: u64,
    pub operation_watchdog_timeouts: u64,
    pub probe_watchdog_timeouts: u64,
    pub last_probe_state: &'static str,
    pub last_error: Option<String>,
    pub snapshot_sequence: u64,
    pub snapshot_at_ms: Option<u64>,
    pub snapshot_age_ms: Option<u64>,
    pub snapshot_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealServiceReport {
    pub controller_index: usize,
    pub mode: HealControllerMode,
    pub session_id: u64,
    pub revision: u64,
    pub seed: HealXhciSeed,
    pub stage: HealServiceStage,
    pub hciversion: Option<u16>,
    pub max_slots: Option<u8>,
    pub max_ports: Option<u8>,
    pub controller_not_ready: Option<bool>,
    pub port_power_control: Option<bool>,
    pub dma_bits: Option<u8>,
    pub max_slots_enabled: Option<u8>,
    pub protocols: Vec<HealProtocolReport>,
    pub ports: Vec<HealPortReport>,
    pub transport: Option<HealTransportProof>,
    pub failure: Option<String>,
    pub registers: Option<HealRegisters>,
    pub last_port_reset: Option<HealPortResetProof>,
    pub enumeration: HealEnumerationReport,
    pub owner: Option<HealOwnerStatus>,
    pub snapshot_at_ms: Option<u64>,
    pub snapshot_age_ms: Option<u64>,
}

impl HealServiceReport {
    fn new(controller_index: usize, session_id: u64, seed: HealXhciSeed) -> Self {
        Self {
            controller_index,
            mode: HealControllerMode::Quarantined,
            session_id,
            revision: 1,
            seed,
            stage: HealServiceStage::Quarantined,
            hciversion: None,
            max_slots: None,
            max_ports: None,
            controller_not_ready: None,
            port_power_control: None,
            dma_bits: None,
            max_slots_enabled: None,
            protocols: Vec::new(),
            ports: Vec::new(),
            transport: None,
            failure: None,
            registers: None,
            last_port_reset: None,
            enumeration: HealEnumerationReport::default(),
            owner: None,
            snapshot_at_ms: None,
            snapshot_age_ms: None,
        }
    }
}

pub fn reports() -> Vec<HealServiceReport> {
    REPORTS.lock().iter().map(response_report).collect()
}

pub(crate) fn response_report(report: &HealServiceReport) -> HealServiceReport {
    let mut result = report.clone();
    let owner = super::runtime::snapshot(report.controller_index);
    let now_ms = trueos_time::Instant::now().as_millis();
    result.snapshot_age_ms = report
        .snapshot_at_ms
        .map(|at_ms| now_ms.saturating_sub(at_ms));
    // Normal discovery refreshes the lab census independently of this report.
    // Keep GET observation useful after an isolated device failure even when
    // no caller has requested a fresh full register inspection yet.
    if let Some(snapshot) = super::lab::latest_snapshot(report.controller_index) {
        if snapshot.enumeration.failure_count >= result.enumeration.failure_count {
            result.enumeration = enumeration_report(&snapshot.enumeration, owner.snapshot_at_ms);
        }
    }
    result.enumeration.snapshot_age_ms = result
        .enumeration
        .snapshot_at_ms
        .map(|at_ms| now_ms.saturating_sub(at_ms));
    result.owner = Some(HealOwnerStatus {
        controller_phase: owner.controller_phase,
        root_hub_lifecycle: owner.root_hub_lifecycle,
        event_ready: owner.event_ready,
        probe_requested: owner.probe_requested,
        operation_requested: owner.operation_requested,
        operation_stage: owner.operation_stage,
        operation_elapsed_ms: owner.operation_elapsed_ms,
        operation_watchdog_timeouts: owner.operation_watchdog_timeouts,
        probe_watchdog_timeouts: owner.probe_watchdog_timeouts,
        last_probe_state: owner.last_probe_state,
        last_error: owner.last_error,
        snapshot_sequence: owner.snapshot_sequence,
        snapshot_at_ms: owner.snapshot_at_ms,
        snapshot_age_ms: owner
            .snapshot_at_ms
            .map(|at_ms| now_ms.saturating_sub(at_ms)),
        snapshot_error: owner.snapshot_error,
    });
    result
}

pub(crate) fn take_rescan_requested(index: usize) -> bool {
    RESCAN_REQUESTED
        .get(index)
        .map(|request| request.swap(false, Ordering::AcqRel))
        .unwrap_or(false)
}

pub fn latest_report() -> Option<HealServiceReport> {
    REPORTS
        .lock()
        .iter()
        .find(|report| report.controller_index == 0)
        .cloned()
}

pub(crate) fn claim_backend(
    dev: crate::pci::PciDevice,
    publish_selection: bool,
) -> Result<XhciBackendSelection, String> {
    let selection = select_backend(dev);
    if let Err(error) = crate::pci::claim_device(&dev, selection.claim_owner()) {
        crate::log!(
            "xhci-admission: pci={:02x}:{:02x}.{} vid={:04x} pid={:04x} owner={} status=claim-failed error={:?}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            selection.claim_owner(),
            error
        );
        return Err(format!(
            "PCI claim owner={} failed: {error:?}",
            selection.claim_owner()
        ));
    }
    if publish_selection {
        *LATEST_SELECTION.lock() = Some(selection);
    }
    Ok(selection)
}

fn select_backend(dev: crate::pci::PciDevice) -> XhciBackendSelection {
    let revision = crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, PCI_REVISION_ID);
    match (dev.vendor_id, dev.device_id) {
        (QEMU_XHCI_VENDOR_ID, QEMU_XHCI_DEVICE_ID) => {
            XhciBackendSelection::KnownQemu(QemuXhciProfile::PciXhci)
        }
        (NEC_XHCI_VENDOR_ID, NEC_QEMU_XHCI_DEVICE_ID) => {
            XhciBackendSelection::KnownQemu(QemuXhciProfile::NecEmulatedXhci)
        }
        (INTEL_VENDOR_ID, INTEL_700_SERIES_XHCI_DEVICE_ID) => {
            XhciBackendSelection::KnownIntel(IntelXhciProfile::Series700 { revision })
        }
        (INTEL_VENDOR_ID, device_id) => {
            XhciBackendSelection::KnownIntel(IntelXhciProfile::CompatibleFamily {
                device_id,
                revision,
            })
        }
        (vendor_id, device_id) => XhciBackendSelection::HealRequired(HealXhciSeed {
            bus: dev.bus,
            slot: dev.slot,
            function: dev.function,
            vendor_id,
            device_id,
            revision,
            subsystem_vendor_id: crate::pci::config_read_u16(
                dev.bus,
                dev.slot,
                dev.function,
                PCI_SUBSYSTEM_VENDOR_ID,
            ),
            subsystem_device_id: crate::pci::config_read_u16(
                dev.bus,
                dev.slot,
                dev.function,
                PCI_SUBSYSTEM_DEVICE_ID,
            ),
        }),
    }
}

fn seed_for_device(dev: crate::pci::PciDevice) -> HealXhciSeed {
    HealXhciSeed {
        bus: dev.bus,
        slot: dev.slot,
        function: dev.function,
        vendor_id: dev.vendor_id,
        device_id: dev.device_id,
        revision: crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, PCI_REVISION_ID),
        subsystem_vendor_id: crate::pci::config_read_u16(
            dev.bus,
            dev.slot,
            dev.function,
            PCI_SUBSYSTEM_VENDOR_ID,
        ),
        subsystem_device_id: crate::pci::config_read_u16(
            dev.bus,
            dev.slot,
            dev.function,
            PCI_SUBSYSTEM_DEVICE_ID,
        ),
    }
}

pub(crate) async fn register_pending_controller(
    index: usize,
    dev: crate::pci::PciDevice,
    host: &mut crabusb::USBHost,
    _stage: &'static str,
) -> HealServiceReport {
    let mut report = HealServiceReport::new(index, next_session_id(), seed_for_device(dev));
    report.mode = HealControllerMode::Initializing;
    report.stage = HealServiceStage::ControllerConstructed;
    let _ = observe_controller(host, &mut report).await;
    publish(&report);
    report
}

pub(crate) async fn register_live_controller(
    index: usize,
    dev: crate::pci::PciDevice,
    host: &mut crabusb::USBHost,
) -> HealServiceReport {
    let mut report = HealServiceReport::new(index, next_session_id(), seed_for_device(dev));
    report.mode = HealControllerMode::Live;
    report.stage = HealServiceStage::NormalRuntime;
    if let Err(reason) = observe_controller(host, &mut report).await {
        report.failure = Some(reason);
    }
    publish(&report);
    crate::log!(
        "xhci-heal: controller={} pci={:02x}:{:02x}.{} session={} mode=live observe=available usb2-unprobed-reset=explicit whole-controller-reset=blocked\n",
        index,
        dev.bus,
        dev.slot,
        dev.function,
        report.session_id
    );
    report
}

pub(crate) async fn register_failed_controller(
    index: usize,
    dev: crate::pci::PciDevice,
    host: &mut crabusb::USBHost,
    reason: String,
) -> HealServiceReport {
    let mut report = HealServiceReport::new(index, next_session_id(), seed_for_device(dev));
    report.mode = HealControllerMode::Parked;
    report.stage = HealServiceStage::Parked;
    report.failure = Some(reason);
    let _ = observe_controller(host, &mut report).await;
    publish(&report);
    report
}

/// Read registers through the existing owner; never consume the event ring.
pub(crate) async fn observe_controller(
    host: &mut crabusb::USBHost,
    report: &mut HealServiceReport,
) -> Result<(), String> {
    match host.xhci_recover(XhciRecoveryRequest::Inspect).await {
        Ok(XhciRecoveryResponse::Inspection(inspection)) => {
            report.dma_bits = Some(inspection.dma_bits);
            report.max_slots_enabled = Some(inspection.max_slots_enabled);
            update_from_snapshot(report, &inspection.snapshot);
            bump(report);
            Ok(())
        }
        Ok(_) => Err(String::from("unexpected controller inspection response")),
        Err(error) => Err(format!("controller inspection failed: {error:?}")),
    }
}

/// The root-hub API independently refuses every port that has ever been
/// handed to addressing. A disabled register alone is not an ownership proof.
pub(crate) async fn reset_unprobed_usb2_port(
    host: &mut crabusb::USBHost,
    report: &mut HealServiceReport,
    selector: super::heal_protocol::HealPortSelector,
    timeout_ms: u32,
) -> Result<(), String> {
    let owner = super::runtime::snapshot(report.controller_index);
    if !owner.event_ready || owner.probe_requested || owner.operation_requested {
        return Err(String::from(
            "port repair requires a running event pump and an idle controller owner",
        ));
    }
    if report.mode != HealControllerMode::Live {
        return Err(String::from(
            "port repair requires the normal initialized root hub and its event pump",
        ));
    }
    observe_controller(host, report).await?;
    let port = match selector {
        super::heal_protocol::HealPortSelector::FirstConnected => {
            report.ports.iter().find(|port| {
                port.protocol_major == Some(2)
                    && port.connected
                    && !port.enabled
                    && !port.reset_active
            })
        }
        super::heal_protocol::HealPortSelector::Port { port_id } => {
            report.ports.iter().find(|port| port.port_id == port_id)
        }
    }
    .ok_or_else(|| String::from("no matching USB2 root port"))?;
    if port.protocol_major != Some(2) || !port.connected || port.enabled || port.reset_active {
        return Err(String::from(
            "USB2 repair requires a connected, disabled port with no reset in progress",
        ));
    }
    let port_id = port.port_id;
    let before_portsc = port.after_portsc;
    let reset_result = host.request_root_port_reset(port_id).await;
    let request_accepted = reset_result.is_ok();
    let mut failure = reset_result
        .err()
        .map(|error| format!("root-hub reset rejected or failed: {error:?}"));
    let mut waited_ms = 0;
    let timeout_ms = timeout_ms.clamp(10, 1_000);
    let mut completed = false;
    loop {
        if let Err(reason) = observe_controller(host, report).await {
            if failure.is_none() {
                failure = Some(reason);
            }
            break;
        }
        if let Some(port) = report.ports.iter().find(|port| port.port_id == port_id) {
            completed = request_accepted && port.connected && port.enabled && !port.reset_active;
            if !request_accepted || completed || !port.connected || waited_ms >= timeout_ms {
                break;
            }
        } else {
            break;
        }
        Timer::after(Duration::from_millis(10)).await;
        waited_ms += 10;
    }
    // The normal owner performs discovery/handoff after an accepted reset.
    // Even if later observation fails, retain the attempt and schedule that
    // follow-up. A PORTSC transition does not establish a boot-HID class proof.
    if request_accepted {
        RESCAN_REQUESTED[report.controller_index].store(true, Ordering::Release);
        if !completed && failure.is_none() {
            failure = Some(String::from(
                "root-port reset issued but active state was not proven within the deadline; normal discovery requested",
            ));
        }
    }
    let after_portsc = report
        .ports
        .iter()
        .find(|port| port.port_id == port_id)
        .map(|port| port.after_portsc)
        .unwrap_or(before_portsc);
    report.last_port_reset = Some(HealPortResetProof {
        port_id,
        before_portsc,
        after_portsc,
        waited_ms,
        completed,
        rescan_requested: request_accepted,
        request_accepted,
        failure: failure.clone(),
    });
    bump(report);
    crate::log!(
        "xhci-heal: controller={} action=explicit-usb2-unprobed-reset port={} before=0x{:08x} after=0x{:08x} waited_ms={} accepted={} completed={} rescan_requested={} failure={:?}\n",
        report.controller_index,
        port_id,
        before_portsc,
        after_portsc,
        waited_ms,
        request_accepted,
        completed,
        request_accepted,
        failure
    );
    match failure {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

pub(crate) async fn run_quarantined(
    controller_index: usize,
    mmio: crabusb::Mmio,
    mmio_len: usize,
    kernel: &'static dyn crabusb::KernelOp,
    seed: HealXhciSeed,
) -> ! {
    let session_id = next_session_id();
    let mut report = HealServiceReport::new(controller_index, session_id, seed);
    publish(&report);
    crate::log!(
        "xhci-heal: state=quarantined session={} pci={:02x}:{:02x}.{} vid={:04x} pid={:04x} rev={:02x} sub={:04x}:{:04x} normal-init=blocked
",
        session_id,
        seed.bus,
        seed.slot,
        seed.function,
        seed.vendor_id,
        seed.device_id,
        seed.revision,
        seed.subsystem_vendor_id,
        seed.subsystem_device_id
    );

    let mut host = match crabusb::USBHost::new_xhci_with_construction_policy_and_mmio_len(
        mmio,
        mmio_len,
        kernel,
        crabusb::XhciConstructionPolicy::quarantined(),
    ) {
        Ok(host) => host,
        Err(error) => {
            fail(
                &mut report,
                format!("quarantined controller construction failed: {error:?}"),
            );
            super::heal_protocol::service_without_controller(report).await;
        }
    };

    report.stage = HealServiceStage::ControllerConstructed;
    bump(&mut report);
    match host.xhci_recover(XhciRecoveryRequest::Inspect).await {
        Ok(XhciRecoveryResponse::Inspection(inspection)) => {
            report.dma_bits = Some(inspection.dma_bits);
            report.max_slots_enabled = Some(inspection.max_slots_enabled);
            update_from_snapshot(&mut report, &inspection.snapshot);
            report.stage = HealServiceStage::CapabilitiesReadable;
            bump(&mut report);
        }
        Ok(_) => fail(
            &mut report,
            String::from("CrabUSB returned an unexpected inspection response"),
        ),
        Err(error) => fail(
            &mut report,
            format!("semantic controller inspection failed: {error:?}"),
        ),
    }

    if !matches!(report.stage, HealServiceStage::Failed) {
        let _ = prove_transport(
            &mut host,
            &mut report,
            DEFAULT_MAX_SLOTS,
            DEFAULT_NOOP_TIMEOUT_MS,
        )
        .await;
    }

    loop {
        while super::heal_protocol::service_one(&mut host, &mut report).await {}
        if let Ok(XhciRecoveryResponse::Events(events)) =
            host.xhci_recover(XhciRecoveryRequest::PollEvents).await
        {
            update_from_snapshot(&mut report, &events.snapshot);
            if matches!(events.event, XhciRecoveryEvent::Nothing) {
                publish(&report);
            } else {
                bump(&mut report);
            }
        }
        Timer::after(Duration::from_millis(HEAL_SERVICE_IDLE_MS)).await;
    }
}

pub(crate) async fn prove_transport(
    host: &mut crabusb::USBHost,
    report: &mut HealServiceReport,
    max_slots: u8,
    noop_timeout_ms: u32,
) -> Result<(), String> {
    if report.transport.is_some() {
        return Ok(());
    }

    let prepared = match host
        .xhci_recover(XhciRecoveryRequest::PrepareTransport { max_slots })
        .await
    {
        Ok(XhciRecoveryResponse::TransportPrepared(proof)) => proof,
        Ok(_) => {
            let reason = String::from("CrabUSB returned an unexpected transport response");
            fail(report, reason.clone());
            return Err(reason);
        }
        Err(error) => {
            let reason = format!("controller transport preparation failed: {error:?}");
            fail(report, reason.clone());
            return Err(reason);
        }
    };
    apply_transport_prepared(report, &prepared);
    report.stage = HealServiceStage::TransportPrepared;
    bump(report);

    let proof = match host
        .xhci_recover(XhciRecoveryRequest::ProveNoop {
            timeout_ms: noop_timeout_ms,
        })
        .await
    {
        Ok(XhciRecoveryResponse::NoopProven(proof)) => proof,
        Ok(_) => {
            let reason = String::from("CrabUSB returned an unexpected No-op response");
            fail(report, reason.clone());
            return Err(reason);
        }
        Err(error) => {
            let reason = format!("xHCI No-op transport proof failed: {error:?}");
            fail(report, reason.clone());
            return Err(reason);
        }
    };
    apply_noop_proof(report, &proof);
    report.stage = HealServiceStage::TransportProven;
    bump(report);
    report.stage = HealServiceStage::AwaitingPortActivation;
    report.failure = None;
    bump(report);

    crate::log!(
        "xhci-heal: state={} session={} pci={:02x}:{:02x}.{} dma_bits={} max_slots={} noop_ms={} command=0x{:x} port-mutations=0 normal-init=blocked
",
        report.stage.as_str(),
        report.session_id,
        report.seed.bus,
        report.seed.slot,
        report.seed.function,
        proof.dma_bits,
        proof.snapshot.config & 0xff,
        proof.waited_ms,
        proof.command_trb_pointer,
    );
    Ok(())
}

fn apply_transport_prepared(report: &mut HealServiceReport, proof: &XhciTransportProof) {
    report.dma_bits = Some(proof.dma_bits);
    report.max_slots_enabled = Some(proof.max_slots_enabled);
    update_from_snapshot(report, &proof.snapshot);
}

fn apply_noop_proof(report: &mut HealServiceReport, proof: &XhciNoopProof) {
    report.dma_bits = Some(proof.dma_bits);
    report.max_slots_enabled = Some((proof.snapshot.config & 0xff) as u8);
    update_from_snapshot(report, &proof.snapshot);
    report.transport = Some(HealTransportProof {
        proof_id: format!(
            "xhci-transport-{}-{}-{:x}",
            report.session_id,
            report.revision.saturating_add(1),
            proof.command_trb_pointer
        ),
        dma_bits: proof.dma_bits,
        max_slots_enabled: (proof.snapshot.config & 0xff) as u8,
        noop_waited_ms: proof.waited_ms,
        command_trb_pointer: proof.command_trb_pointer,
        completion_slot_id: proof.completion_slot_id,
    });
}

fn enumeration_failure(failure: &crabusb::diag::XhciEnumerationFailure) -> HealEnumerationFailure {
    HealEnumerationFailure {
        root_port_id: failure.root_port_id,
        port_id: failure.port_id,
        slot_id: failure.slot_id,
        stage: failure.stage,
        reason: String::from(failure.reason()),
        reason_truncated: failure.reason_truncated,
    }
}

fn enumeration_report(
    diagnostic: &crabusb::diag::XhciEnumerationDiagnostics,
    snapshot_at_ms: Option<u64>,
) -> HealEnumerationReport {
    HealEnumerationReport {
        failure_count: diagnostic.failure_count,
        first_failure: diagnostic.first_failure.as_ref().map(enumeration_failure),
        last_failure: diagnostic.last_failure.as_ref().map(enumeration_failure),
        snapshot_at_ms,
        snapshot_age_ms: snapshot_at_ms.map(|_| 0),
    }
}

fn update_from_snapshot(report: &mut HealServiceReport, snapshot: &XhciControllerSnapshot) {
    report.snapshot_at_ms = Some(trueos_time::Instant::now().as_millis());
    report.snapshot_age_ms = Some(0);
    report.enumeration = enumeration_report(&snapshot.enumeration, report.snapshot_at_ms);
    report.registers = Some(HealRegisters {
        usbcmd: snapshot.usbcmd,
        usbsts: snapshot.usbsts,
        config: snapshot.config,
        crcr: snapshot.crcr,
        dcbaap: snapshot.dcbaap,
        erdp: snapshot.erdp,
        iman: snapshot.iman,
        mfindex: snapshot.mfindex,
    });
    report.hciversion = Some(snapshot.hciversion);
    report.max_slots = Some((snapshot.hcsparams1 & 0xff) as u8);
    report.max_ports = Some(((snapshot.hcsparams1 >> 24) & 0xff) as u8);
    report.controller_not_ready = Some(snapshot.usbsts & USBSTS_CONTROLLER_NOT_READY != 0);
    report.port_power_control = Some(snapshot.hccparams1 & HCCPARAMS1_PORT_POWER_CONTROL != 0);
    report.protocols = snapshot
        .protocols
        .iter()
        .map(|protocol| HealProtocolReport {
            major: protocol.major,
            minor: protocol.minor,
            port_offset: protocol.port_offset,
            port_count: protocol.port_count,
            slot_type: protocol.slot_type,
            psi_count: protocol.psi_count,
        })
        .collect();
    report.ports = snapshot
        .ports
        .iter()
        .map(|port| {
            let protocol = snapshot.protocols.iter().find(|protocol| {
                let first = u16::from(protocol.port_offset);
                let end = first.saturating_add(u16::from(protocol.port_count));
                let id = u16::from(port.port_id);
                id >= first && id < end
            });
            let powered = port.portsc & PORT_PP != 0;
            HealPortReport {
                port_id: port.port_id,
                protocol_major: protocol.map(|protocol| protocol.major),
                protocol_minor: protocol.map(|protocol| protocol.minor),
                connected: port.portsc & PORT_CCS != 0,
                enabled: port.portsc & PORT_PED != 0,
                power_attempted: false,
                powered_before: powered,
                powered_after: powered,
                reset_active: port.portsc & (PORT_PR | PORT_WPR) != 0,
                link_state: ((port.portsc & PORT_PLS_MASK) >> 5) as u8,
                speed_id: ((port.portsc & PORT_SPEED_MASK) >> 10) as u8,
                before_portsc: port.portsc,
                after_portsc: port.portsc,
            }
        })
        .collect();
}

fn next_session_id() -> u64 {
    loop {
        let id = NEXT_SESSION_ID.fetch_add(1, Ordering::AcqRel);
        if id != 0 {
            return id;
        }
    }
}

fn bump(report: &mut HealServiceReport) {
    report.revision = report.revision.saturating_add(1);
    publish(report);
}

pub(crate) fn publish(report: &HealServiceReport) {
    let mut reports = REPORTS.lock();
    if let Some(previous) = reports
        .iter_mut()
        .find(|item| item.controller_index == report.controller_index)
    {
        *previous = report.clone();
    } else {
        reports.push(report.clone());
        reports.sort_by_key(|item| item.controller_index);
    }
}

pub(crate) fn fail(report: &mut HealServiceReport, reason: String) {
    report.stage = HealServiceStage::Failed;
    report.failure = Some(reason.clone());
    bump(report);
    crate::log!(
        "xhci-heal: state=failed session={} pci={:02x}:{:02x}.{} vid={:04x} pid={:04x} reason={} normal-init=blocked
",
        report.session_id,
        report.seed.bus,
        report.seed.slot,
        report.seed.function,
        report.seed.vendor_id,
        report.seed.device_id,
        reason
    );
}
