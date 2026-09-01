//! Quarantined xHCI admission and first-step recovery service.
//!
//! Delivered Intel and QEMU controller families enter their known CrabUSB
//! profiles. Every other vendor remains owned by Heal before normal
//! `USBHost::init()`. The initial recovery primitive is intentionally narrow:
//! snapshot the controller and power software-controlled root ports with the
//! same PORTSC-neutral write discipline as the xHCI laboratory.

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::crabusb::{
    self,
    diag::{XhciControllerSnapshot, XhciDirectRequest, XhciDirectResponse},
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
const PORT_OCA: u32 = 1 << 3;
const PORT_PLS_MASK: u32 = 0x0f << 5;
const PORT_PP: u32 = 1 << 9;
const PORT_SPEED_MASK: u32 = 0x0f << 10;
const PORT_PIC_MASK: u32 = 0x03 << 14;
const PORT_WAKE_MASK: u32 = 0x07 << 25;
const PORT_DR: u32 = 1 << 30;
const PORT_RO_MASK: u32 = PORT_CCS | PORT_OCA | PORT_SPEED_MASK | PORT_DR;
const PORT_RWS_MASK: u32 = PORT_PLS_MASK | PORT_PP | PORT_PIC_MASK | PORT_WAKE_MASK;
const REPORT_REFRESH_MS: u64 = 1_000;

static LATEST_SELECTION: Mutex<Option<XhciBackendSelection>> = Mutex::new(None);
static LATEST_REPORT: Mutex<Option<HealServiceReport>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealServiceStage {
    Quarantined,
    ControllerConstructed,
    CapabilitiesReadable,
    RootPortsPowered,
    AwaitingRecipe,
    Failed,
}

impl HealServiceStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quarantined => "quarantined",
            Self::ControllerConstructed => "controller-constructed",
            Self::CapabilitiesReadable => "capabilities-readable",
            Self::RootPortsPowered => "root-ports-powered",
            Self::AwaitingRecipe => "awaiting-recipe",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HealPortReport {
    pub port_id: u8,
    pub connected: bool,
    pub power_attempted: bool,
    pub powered_before: bool,
    pub powered_after: bool,
    pub before_portsc: u32,
    pub after_portsc: u32,
}

#[derive(Clone, Debug)]
pub struct HealServiceReport {
    pub seed: HealXhciSeed,
    pub stage: HealServiceStage,
    pub hciversion: Option<u16>,
    pub max_slots: Option<u8>,
    pub max_ports: Option<u8>,
    pub controller_not_ready: Option<bool>,
    pub port_power_control: Option<bool>,
    pub ports: Vec<HealPortReport>,
    pub failure: Option<String>,
}

impl HealServiceReport {
    fn new(seed: HealXhciSeed) -> Self {
        Self {
            seed,
            stage: HealServiceStage::Quarantined,
            hciversion: None,
            max_slots: None,
            max_ports: None,
            controller_not_ready: None,
            port_power_control: None,
            ports: Vec::new(),
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

pub(crate) fn select_first_backend() -> Option<XhciBackendSelection> {
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
    Some(selection)
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
    let mut report = HealServiceReport::new(seed);
    publish(&report);
    crate::log!(
        "xhci-heal: state=quarantined pci={:02x}:{:02x}.{} vid={:04x} pid={:04x} rev={:02x} sub={:04x}:{:04x} normal-init=blocked\n",
        seed.bus,
        seed.slot,
        seed.function,
        seed.vendor_id,
        seed.device_id,
        seed.revision,
        seed.subsystem_vendor_id,
        seed.subsystem_device_id
    );

    // Normal `USBHost::init()` is deliberately unreachable in this branch.
    // The constructor needs a root-hub profile, but that profile cannot run
    // while Heal owns the controller, so the least-mutating delivered profile
    // is only an inert construction seed.
    let mut host = match crabusb::USBHost::new_xhci_with_root_hub_init_policy_and_mmio_len(
        mmio,
        mmio_len,
        kernel,
        crabusb::XhciRootHubInitPolicy::SelectivePorts3And4Skip11,
    ) {
        Ok(host) => host,
        Err(error) => {
            fail(
                &mut report,
                format!("quarantined controller construction failed: {error:?}"),
            );
            hold_quarantine().await
        }
    };

    report.stage = HealServiceStage::ControllerConstructed;
    publish(&report);

    let initial = match direct_snapshot(&mut host).await {
        Ok(snapshot) => snapshot,
        Err(reason) => {
            fail(&mut report, reason);
            hold_quarantine().await
        }
    };
    update_capabilities(&mut report, &initial);
    report.stage = HealServiceStage::CapabilitiesReadable;
    publish(&report);

    if initial.usbsts & USBSTS_CONTROLLER_NOT_READY != 0 {
        fail(
            &mut report,
            format!(
                "controller not ready before safe power step: USBSTS=0x{:08x}",
                initial.usbsts
            ),
        );
        hold_quarantine_with_host(&mut host, &mut report).await
    }

    match power_root_ports(&mut host, &initial).await {
        Ok(ports) => {
            report.ports = ports;
            report.stage = HealServiceStage::RootPortsPowered;
            publish(&report);
        }
        Err(reason) => {
            fail(&mut report, reason);
            hold_quarantine_with_host(&mut host, &mut report).await
        }
    }

    report.stage = HealServiceStage::AwaitingRecipe;
    publish(&report);
    crate::log!(
        "xhci-heal: state={} pci={:02x}:{:02x}.{} hciversion=0x{:04x} max_slots={} max_ports={} ppc={} connected={} powered={} normal-init=blocked\n",
        report.stage.as_str(),
        seed.bus,
        seed.slot,
        seed.function,
        report.hciversion.unwrap_or(0),
        report.max_slots.unwrap_or(0),
        report.max_ports.unwrap_or(0),
        report.port_power_control.unwrap_or(false) as u8,
        report.ports.iter().filter(|port| port.connected).count(),
        report.ports.iter().filter(|port| port.powered_after).count(),
    );

    hold_quarantine_with_host(&mut host, &mut report).await
}

async fn power_root_ports(
    host: &mut crabusb::USBHost,
    initial: &XhciControllerSnapshot,
) -> Result<Vec<HealPortReport>, String> {
    let power_control = initial.hccparams1 & HCCPARAMS1_PORT_POWER_CONTROL != 0;
    let mut reports = initial
        .ports
        .iter()
        .map(|port| HealPortReport {
            port_id: port.port_id,
            connected: port.portsc & PORT_CCS != 0,
            power_attempted: false,
            powered_before: port.portsc & PORT_PP != 0,
            powered_after: port.portsc & PORT_PP != 0,
            before_portsc: port.portsc,
            after_portsc: port.portsc,
        })
        .collect::<Vec<_>>();

    if power_control {
        for port in reports.iter_mut().filter(|port| !port.powered_before) {
            let offset = portsc_offset(initial, port.port_id)?;
            let requested = neutral_portsc(port.before_portsc) | PORT_PP;
            match host
                .xhci_direct(XhciDirectRequest::Write32 {
                    offset,
                    value: requested,
                })
                .await
            {
                Ok(XhciDirectResponse::Write32(_)) => port.power_attempted = true,
                Ok(_) => {
                    return Err(format!(
                        "power-on port {} returned an unexpected xHCI response",
                        port.port_id
                    ));
                }
                Err(error) => {
                    return Err(format!(
                        "power-on port {} failed at offset 0x{offset:x}: {error:?}",
                        port.port_id
                    ));
                }
            }
        }
    }

    Timer::after(Duration::from_millis(2)).await;
    let after = direct_snapshot(host).await?;
    refresh_ports(&mut reports, &after);
    if power_control
        && let Some(port) = reports.iter().find(|port| !port.powered_after)
    {
        return Err(format!(
            "power-on verification failed for root port {}: PORTSC=0x{:08x}",
            port.port_id, port.after_portsc
        ));
    }
    Ok(reports)
}

async fn direct_snapshot(host: &mut crabusb::USBHost) -> Result<XhciControllerSnapshot, String> {
    match host.xhci_direct(XhciDirectRequest::Snapshot).await {
        Ok(XhciDirectResponse::Snapshot(snapshot)) => Ok(snapshot),
        Ok(_) => Err(String::from("xHCI snapshot returned an unexpected response")),
        Err(error) => Err(format!("xHCI capability snapshot failed: {error:?}")),
    }
}

fn update_capabilities(report: &mut HealServiceReport, snapshot: &XhciControllerSnapshot) {
    report.hciversion = Some(snapshot.hciversion);
    report.max_slots = Some((snapshot.hcsparams1 & 0xff) as u8);
    report.max_ports = Some(((snapshot.hcsparams1 >> 24) & 0xff) as u8);
    report.controller_not_ready = Some(snapshot.usbsts & USBSTS_CONTROLLER_NOT_READY != 0);
    report.port_power_control =
        Some(snapshot.hccparams1 & HCCPARAMS1_PORT_POWER_CONTROL != 0);
}

fn refresh_ports(reports: &mut [HealPortReport], snapshot: &XhciControllerSnapshot) {
    for report in reports {
        let Some(current) = snapshot
            .ports
            .iter()
            .find(|port| port.port_id == report.port_id)
        else {
            continue;
        };
        report.connected = current.portsc & PORT_CCS != 0;
        report.powered_after = current.portsc & PORT_PP != 0;
        report.after_portsc = current.portsc;
    }
}

fn portsc_offset(snapshot: &XhciControllerSnapshot, port_id: u8) -> Result<usize, String> {
    let index = usize::from(
        port_id
            .checked_sub(1)
            .ok_or_else(|| String::from("xHCI root port zero is invalid"))?,
    );
    let offset = usize::from(snapshot.caplength)
        .checked_add(0x400)
        .and_then(|base| {
            index
                .checked_mul(0x10)
                .and_then(|delta| base.checked_add(delta))
        })
        .ok_or_else(|| String::from("xHCI PORTSC offset overflow"))?;
    let end = offset
        .checked_add(core::mem::size_of::<u32>())
        .ok_or_else(|| String::from("xHCI PORTSC end overflow"))?;
    if end > snapshot.mmio_len {
        return Err(format!(
            "xHCI PORTSC port {} offset 0x{offset:x} exceeds aperture 0x{:x}",
            port_id, snapshot.mmio_len
        ));
    }
    Ok(offset)
}

fn neutral_portsc(portsc: u32) -> u32 {
    (portsc & PORT_RO_MASK) | (portsc & PORT_RWS_MASK)
}

fn publish(report: &HealServiceReport) {
    *LATEST_REPORT.lock() = Some(report.clone());
}

fn fail(report: &mut HealServiceReport, reason: String) {
    report.stage = HealServiceStage::Failed;
    report.failure = Some(reason.clone());
    publish(report);
    crate::log!(
        "xhci-heal: state=failed pci={:02x}:{:02x}.{} vid={:04x} pid={:04x} reason={} normal-init=blocked\n",
        report.seed.bus,
        report.seed.slot,
        report.seed.function,
        report.seed.vendor_id,
        report.seed.device_id,
        reason
    );
}

async fn hold_quarantine() -> ! {
    loop {
        Timer::after(Duration::from_millis(REPORT_REFRESH_MS)).await;
    }
}

async fn hold_quarantine_with_host(
    host: &mut crabusb::USBHost,
    report: &mut HealServiceReport,
) -> ! {
    loop {
        Timer::after(Duration::from_millis(REPORT_REFRESH_MS)).await;
        if let Ok(snapshot) = direct_snapshot(host).await {
            update_capabilities(report, &snapshot);
            refresh_ports(report.ports.as_mut_slice(), &snapshot);
            publish(report);
        }
    }
}
