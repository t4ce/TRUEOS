//! Quarantined xHCI admission and first-step recovery service.
//!
//! Delivered Intel and QEMU controller families enter their known CrabUSB
//! profiles. Every other vendor remains owned by Heal before normal
//! `USBHost::init()`. The initial recovery primitive is intentionally narrow:
//! snapshot the controller and power software-controlled root ports with the
//! same PORTSC-neutral write discipline as the xHCI laboratory.

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
static LATEST_REPORT: Mutex<Option<HealServiceReport>> = Mutex::new(None);
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

#[derive(Clone, Debug, Serialize)]
pub struct HealServiceReport {
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
}

impl HealServiceReport {
    fn new(session_id: u64, seed: HealXhciSeed) -> Self {
        Self {
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
        }
    }
}

pub fn latest_selection() -> Option<XhciBackendSelection> {
    *LATEST_SELECTION.lock()
}

pub fn latest_report() -> Option<HealServiceReport> {
    LATEST_REPORT.lock().clone()
}

pub(crate) fn select_first_backend() -> Option<(XhciBackendSelection, crate::pci::PciDevice)> {
    let dev = crate::pci::with_devices(|devices| {
        devices
            .iter()
            .copied()
            .find(|dev| dev.class == 0x0c && dev.subclass == 0x03 && dev.prog_if == 0x30)
    })?;
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
        return None;
    }
    *LATEST_SELECTION.lock() = Some(selection);
    Some((selection, dev))
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

pub(crate) async fn run_quarantined(
    mmio: crabusb::Mmio,
    mmio_len: usize,
    kernel: &'static dyn crabusb::KernelOp,
    seed: HealXhciSeed,
) -> ! {
    let session_id = next_session_id();
    let mut report = HealServiceReport::new(session_id, seed);
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
            fail(&mut report, format!("quarantined controller construction failed: {error:?}"));
            return super::heal_protocol::service_without_controller(report).await;
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
        Ok(_) => {
            fail(&mut report, String::from("CrabUSB returned an unexpected inspection response"))
        }
        Err(error) => {
            fail(&mut report, format!("semantic controller inspection failed: {error:?}"))
        }
    }

    if !matches!(report.stage, HealServiceStage::Failed) {
        let _ = prove_transport(&mut host, &mut report, DEFAULT_MAX_SLOTS, DEFAULT_NOOP_TIMEOUT_MS)
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

fn update_from_snapshot(report: &mut HealServiceReport, snapshot: &XhciControllerSnapshot) {
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
    *LATEST_REPORT.lock() = Some(report.clone());
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
