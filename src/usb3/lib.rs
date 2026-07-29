use alloc::string::String;
use alloc::vec::Vec;
use core::{alloc::Layout, num::NonZeroUsize, ptr::NonNull, time::Duration};

use crab_usb as crabusb;
use spin::Mutex;

static OBSERVED_USB_DEVICES: Mutex<Vec<TlbUsbDevice>> = Mutex::new(Vec::new());

struct TrueosCrabKernel;

static TRUEOS_CRAB_KERNEL: TrueosCrabKernel = TrueosCrabKernel;

impl crabusb::DmaOp for TrueosCrabKernel {
    fn page_size(&self) -> usize {
        4096
    }

    unsafe fn map_single(
        &self,
        dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        align: usize,
        direction: crabusb::DmaDirection,
    ) -> Result<crabusb::DmaMapHandle, crabusb::DmaError> {
        let size = size.get();
        let layout = Layout::from_size_align(size, align.max(1))?;
        let phys = crate::phys::virt_to_phys_checked(addr.as_ptr())
            .ok_or(crabusb::DmaError::NullPointer)?;
        let dma_addr = crabusb::DmaAddr::from(phys);
        let end = phys.checked_add(size.saturating_sub(1) as u64).ok_or(
            crabusb::DmaError::DmaMaskNotMatch {
                addr: dma_addr,
                mask: dma_mask,
            },
        )?;
        if end > dma_mask || (align > 1 && !phys.is_multiple_of(align as u64)) {
            let max_phys = Some(dma_mask.checked_add(1).unwrap_or(u64::MAX));
            let (bounce_phys, bounce_virt) =
                crate::dma::alloc_with_max(size, layout.align(), max_phys)
                    .ok_or(crabusb::DmaError::NoMemory)?;
            let bounce = NonNull::new(bounce_virt).ok_or(crabusb::DmaError::NullPointer)?;

            if matches!(
                direction,
                crabusb::DmaDirection::ToDevice | crabusb::DmaDirection::Bidirectional
            ) {
                unsafe {
                    core::ptr::copy_nonoverlapping(addr.as_ptr(), bounce.as_ptr(), size);
                }
            }

            if crate::log_os::flags::USB_MASS_UAS_TRACE_LOGS {
                crate::log!(
                    "crabusb: dma remap size={} align={} orig_phys=0x{:X} bounce_phys=0x{:X}\n",
                    size,
                    layout.align(),
                    phys,
                    bounce_phys
                );
            }

            return Ok(unsafe {
                crabusb::DmaMapHandle::new(
                    addr,
                    crabusb::DmaAddr::from(bounce_phys),
                    layout,
                    Some(bounce),
                )
            });
        }
        Ok(unsafe { crabusb::DmaMapHandle::new(addr, dma_addr, layout, None) })
    }

    unsafe fn unmap_single(&self, handle: crabusb::DmaMapHandle) {
        if let Some(virt) = handle.alloc_virt() {
            crate::dma::dealloc(virt.as_ptr(), handle.size());
        }
    }

    unsafe fn alloc_coherent(&self, dma_mask: u64, layout: Layout) -> Option<crabusb::DmaHandle> {
        let max_phys = Some(
            dma_mask
                .checked_add(1)
                .unwrap_or(u64::MAX)
                .min(0x1_0000_0000),
        );
        let (phys, virt) = crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys)?;
        let ptr = NonNull::new(virt)?;
        Some(unsafe { crabusb::DmaHandle::new(ptr, crabusb::DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: crabusb::DmaHandle) {
        crate::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
    }
}

impl crabusb::KernelOp for TrueosCrabKernel {
    fn delay(&self, duration: Duration) {
        let delay_ms = duration.as_millis().try_into().unwrap_or(u64::MAX);
        let _ = crate::wait::spin_until_timeout_no_exec(delay_ms.max(1), || false);
    }
}

pub fn known_xhci_host_inputs()
-> Option<(crabusb::Mmio, usize, &'static dyn crabusb::KernelOp, crabusb::XhciRootHubInitPolicy)> {
    let dev = known_xhci_device()?;
    let root_hub_policy = if is_qemu_xhci_device(dev.vendor_id, dev.device_id) {
        crate::log!(
            "crabusb: xhci controller policy=qemu-full-root-hub-reset pci={:02x}:{:02x}.{} vid={:04x} pid={:04x}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.vendor_id,
            dev.device_id
        );
        crabusb::XhciRootHubInitPolicy::FullAllPorts
    } else {
        crate::log!(
            "crabusb: xhci controller policy=baremetal-selective-3-4-skip-11 pci={:02x}:{:02x}.{} vid={:04x} pid={:04x}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.vendor_id,
            dev.device_id
        );
        crabusb::XhciRootHubInitPolicy::SelectivePorts3And4Skip11
    };
    crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);
    let (bar, phys) = first_mmio_bar(&dev)?;
    let size = crate::pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar)
        .unwrap_or(0x10000)
        .max(0x1000) as usize;
    let mmio = crate::pci::mmio::map_mmio_region_exact(phys, size).ok()?;
    Some((mmio, size, &TRUEOS_CRAB_KERNEL, root_hub_policy))
}

fn known_xhci_device() -> Option<crate::pci::PciDevice> {
    crate::pci::with_devices(|devices| {
        devices
            .iter()
            .copied()
            .find(|dev| dev.class == 0x0c && dev.subclass == 0x03 && dev.prog_if == 0x30)
    })
}

fn is_qemu_xhci_device(vendor_id: u16, device_id: u16) -> bool {
    matches!(
        (vendor_id, device_id),
        // qemu-xhci
        (0x1b36, 0x000d)
            // nec-usb-xhci, commonly used by older QEMU invocations
            | (0x1033, 0x0194)
    )
}

fn first_mmio_bar(dev: &crate::pci::PciDevice) -> Option<(u8, u64)> {
    for bar in 0..6u8 {
        let (lo, hi) = crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, bar);
        if lo == 0 || (lo & 0x1) != 0 {
            continue;
        }
        let phys = ((hi.unwrap_or(0) as u64) << 32) | ((lo & !0xf) as u64);
        if phys != 0 {
            return Some((bar, phys));
        }
    }
    None
}

pub mod xhci {}

#[derive(Clone, Debug, Default)]
pub struct UsbControllerInfo {
    pub index: usize,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub controller_phase: &'static str,
    pub root_hub_lifecycle: &'static str,
    pub event_ready: bool,
    pub root_port_change_seen: bool,
    pub empty_probe_streak: u32,
}

pub fn pci_usb_controllers() -> Vec<UsbControllerInfo> {
    let mut out = Vec::new();
    crate::pci::with_devices(|devices| {
        for dev in devices
            .iter()
            .copied()
            .filter(|dev| dev.class == 0x0c && dev.subclass == 0x03 && dev.prog_if == 0x30)
        {
            out.push(UsbControllerInfo {
                index: out.len(),
                bus: dev.bus,
                slot: dev.slot,
                function: dev.function,
                vendor_id: dev.vendor_id,
                device_id: dev.device_id,
                controller_phase: "crabusb",
                root_hub_lifecycle: "active",
                event_ready: true,
                root_port_change_seen: false,
                empty_probe_streak: 0,
            });
        }
    });
    out
}

pub fn discover_first_controller() -> Option<UsbControllerInfo> {
    pci_usb_controllers().into_iter().next()
}

pub async fn crabusb_bsp_service(_index: usize) {
    core::future::pending::<()>().await;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TlbUsbTopologyNodeKind {
    RootPort,
    Hub,
    Device,
}

#[derive(Clone, Debug, Default)]
pub struct UsbDeviceSummary {
    pub root_port_id: u8,
    pub port: u8,
    pub slot_id: u8,
    pub route_string: u32,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub class: Option<u8>,
    pub subclass: Option<u8>,
    pub protocol: Option<u8>,
    pub kind: &'static str,
    pub product: Option<String>,
    pub stable_id: u32,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbEndpoint {
    pub address: u8,
    pub transfer_type: &'static str,
    pub max_packet_size: u16,
    pub interval: u8,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbInterface {
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<TlbUsbEndpoint>,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbConfiguration {
    pub configuration_value: u8,
    pub attributes: u8,
    pub max_power: u8,
    pub interfaces: Vec<TlbUsbInterface>,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbHubPathHop {
    pub slot_id: u8,
    pub port_id: u8,
    pub hub_depth: u8,
    pub speed: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbDevice {
    pub stable_id: u32,
    pub slot_id: u8,
    pub root_port_id: u8,
    pub port_id: u8,
    pub route_string: u32,
    pub speed: &'static str,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub num_configurations: u8,
    pub max_packet_size_0: u8,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub path: Vec<u8>,
    pub parent_hub_slot_id: Option<u8>,
    pub hub_path: Vec<TlbUsbHubPathHop>,
    pub configurations: Vec<TlbUsbConfiguration>,
}

#[derive(Clone, Debug)]
pub struct TlbUsbTopologyNode {
    pub kind: TlbUsbTopologyNodeKind,
    pub controller_index: usize,
    pub root_port_id: u8,
    pub port_id: u8,
    pub depth: u8,
    pub slot_id: Option<u8>,
    pub parent_slot_id: Option<u8>,
    pub speed: &'static str,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub class: Option<u8>,
    pub subclass: Option<u8>,
    pub protocol: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct TlbUsbSnapshot {
    pub controllers: Vec<UsbControllerInfo>,
    pub devices: Vec<TlbUsbDevice>,
    pub topology: Vec<TlbUsbTopologyNode>,
    pub probe_device_count: Option<usize>,
    pub probe_error: Option<&'static str>,
}

pub fn tlb_usb_snapshot() -> TlbUsbSnapshot {
    let mut topology: Vec<TlbUsbTopologyNode> = super::lab::latest_snapshot()
        .map(|snapshot| {
            snapshot
                .ports
                .iter()
                .map(|port| TlbUsbTopologyNode {
                    kind: TlbUsbTopologyNodeKind::RootPort,
                    controller_index: 0,
                    root_port_id: port.port_id,
                    port_id: port.port_id,
                    depth: 0,
                    slot_id: None,
                    parent_slot_id: None,
                    speed: xhci_port_speed_name(port.portsc),
                    vendor_id: None,
                    product_id: None,
                    class: None,
                    subclass: None,
                    protocol: None,
                })
                .collect()
        })
        .unwrap_or_default();
    let devices = OBSERVED_USB_DEVICES.lock().clone();
    topology.extend(devices.iter().map(|device| TlbUsbTopologyNode {
        kind: if device.class == 0x09 {
            TlbUsbTopologyNodeKind::Hub
        } else {
            TlbUsbTopologyNodeKind::Device
        },
        controller_index: 0,
        root_port_id: device.root_port_id,
        port_id: device.port_id,
        depth: device.path.len().try_into().unwrap_or(u8::MAX),
        slot_id: Some(device.slot_id),
        parent_slot_id: device.parent_hub_slot_id,
        speed: device.speed,
        vendor_id: Some(device.vendor_id),
        product_id: Some(device.product_id),
        class: Some(device.class),
        subclass: Some(device.subclass),
        protocol: Some(device.protocol),
    }));
    TlbUsbSnapshot {
        controllers: pci_usb_controllers(),
        probe_device_count: Some(devices.len()),
        devices,
        topology,
        ..TlbUsbSnapshot::default()
    }
}

pub fn observe_probed_devices(label: &str, devices: &[crabusb::ProbedDevice]) {
    let inferred_root_port = super::lab::latest_snapshot().and_then(|snapshot| {
        let mut connected = snapshot
            .ports
            .iter()
            .filter(|port| (port.portsc & 1) != 0)
            .map(|port| port.port_id);
        let first = connected.next()?;
        connected.next().is_none().then_some(first)
    });

    let mut observed = OBSERVED_USB_DEVICES.lock();
    if label == "initial" {
        observed.clear();
    }
    for probed in devices {
        let next = tlb_device_from_probed(probed, inferred_root_port);
        if let Some(existing) = observed
            .iter_mut()
            .find(|device| device.stable_id == next.stable_id)
        {
            *existing = next;
        } else {
            observed.push(next);
        }
    }
}

fn tlb_device_from_probed(
    probed: &crabusb::ProbedDevice,
    inferred_root_port: Option<u8>,
) -> TlbUsbDevice {
    let descriptor = probed.descriptor();
    let configurations = probed
        .configurations()
        .iter()
        .map(|configuration| TlbUsbConfiguration {
            configuration_value: configuration.configuration_value,
            attributes: configuration.attributes,
            max_power: configuration.max_power,
            interfaces: configuration
                .interfaces
                .iter()
                .flat_map(|interface| interface.alt_settings.iter())
                .map(|interface| TlbUsbInterface {
                    interface_number: interface.interface_number,
                    alternate_setting: interface.alternate_setting,
                    class: interface.class,
                    subclass: interface.subclass,
                    protocol: interface.protocol,
                    endpoints: interface
                        .endpoints
                        .iter()
                        .map(|endpoint| TlbUsbEndpoint {
                            address: endpoint.address,
                            transfer_type: match endpoint.transfer_type {
                                crabusb::usb_if::descriptor::EndpointType::Control => "control",
                                crabusb::usb_if::descriptor::EndpointType::Isochronous => {
                                    "isochronous"
                                }
                                crabusb::usb_if::descriptor::EndpointType::Bulk => "bulk",
                                crabusb::usb_if::descriptor::EndpointType::Interrupt => "interrupt",
                            },
                            max_packet_size: endpoint.max_packet_size,
                            interval: endpoint.interval,
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();
    let slot_id = probed.id().try_into().unwrap_or(u8::MAX);
    let root_port_id = inferred_root_port.unwrap_or(0);
    TlbUsbDevice {
        stable_id: stable_usb_id(
            slot_id,
            descriptor.vendor_id,
            descriptor.product_id,
            descriptor.device_version,
        ),
        slot_id,
        root_port_id,
        port_id: root_port_id,
        route_string: 0,
        speed: "unknown",
        vendor_id: descriptor.vendor_id,
        product_id: descriptor.product_id,
        class: descriptor.class,
        subclass: descriptor.subclass,
        protocol: descriptor.protocol,
        num_configurations: descriptor.num_configurations,
        max_packet_size_0: descriptor.max_packet_size_0,
        product: Some(alloc::format!(
            "{} {:04X}:{:04X}",
            usb_device_kind(descriptor.class),
            descriptor.vendor_id,
            descriptor.product_id
        )),
        path: inferred_root_port.into_iter().collect(),
        configurations,
        ..TlbUsbDevice::default()
    }
}

fn stable_usb_id(slot_id: u8, vendor_id: u16, product_id: u16, device_version: u16) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in [
        slot_id,
        vendor_id as u8,
        (vendor_id >> 8) as u8,
        product_id as u8,
        (product_id >> 8) as u8,
        device_version as u8,
        (device_version >> 8) as u8,
    ] {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

fn usb_device_kind(class: u8) -> &'static str {
    match class {
        0x00 => "USB device",
        0x01 => "USB audio",
        0x02 | 0x0a => "USB communications",
        0x03 => "USB HID",
        0x06 => "USB imaging",
        0x07 => "USB printer",
        0x08 => "USB mass storage",
        0x09 => "USB hub",
        0x0b => "USB smart card",
        0x0e => "USB video",
        0xe0 => "USB wireless",
        0xff => "USB vendor device",
        _ => "USB device",
    }
}

fn xhci_port_speed_name(portsc: u32) -> &'static str {
    match (portsc >> 10) & 0x0f {
        1 => "full",
        2 => "low",
        3 => "high",
        4 => "super",
        5 => "super+",
        _ => "unknown",
    }
}

pub fn crabusb_observed_device_summaries(
    controller_index: usize,
) -> Result<Vec<UsbDeviceSummary>, &'static str> {
    if controller_index != 0 {
        return Ok(Vec::new());
    }
    Ok(OBSERVED_USB_DEVICES
        .lock()
        .iter()
        .map(|device| UsbDeviceSummary {
            root_port_id: device.root_port_id,
            port: device.port_id,
            slot_id: device.slot_id,
            route_string: device.route_string,
            vid: Some(device.vendor_id),
            pid: Some(device.product_id),
            class: Some(device.class),
            subclass: Some(device.subclass),
            protocol: Some(device.protocol),
            kind: usb_device_kind(device.class),
            product: device.product.clone(),
            stable_id: device.stable_id,
        })
        .collect())
}

pub fn crabusb_observed_devices(
    controller_index: usize,
) -> Result<Vec<TlbUsbDevice>, &'static str> {
    if controller_index != 0 {
        return Ok(Vec::new());
    }
    Ok(OBSERVED_USB_DEVICES.lock().clone())
}

#[derive(Clone, Debug, Default)]
pub struct UsbRuntimeDiag {
    pub probe_requested: bool,
    pub probe_fail_streak: u32,
    pub early_fatal_rebind_streak: u32,
    pub last_probe_state: &'static str,
    pub last_probe_device_count: usize,
    pub recovery_quiescent_before_bind: bool,
    pub recovery_quiescent_ms: u64,
    pub recovery_initial_settle_ms: u64,
    pub recovery_probe_quiet_ms: u64,
    pub recovery_skip_delayed_event_handler: bool,
}

pub fn crabusb_runtime_diag(_controller_index: usize) -> UsbRuntimeDiag {
    UsbRuntimeDiag::default()
}

#[derive(Clone, Debug, Default)]
pub struct XhciPortDiag {
    pub port_id: u8,
    pub portsc: u32,
    pub portpmsc: u32,
    pub portli: u32,
}

#[derive(Clone, Debug, Default)]
pub struct XhciMmioDiag {
    pub caplen: u8,
    pub hcsparams1: u32,
    pub hccparams1: u32,
    pub dboff: u32,
    pub rtsoff: u32,
    pub usbcmd: u32,
    pub usbsts: u32,
    pub crcr: u64,
    pub dcbaap: u64,
    pub config: u32,
    pub iman: u32,
    pub imod: u32,
    pub erstsz: u32,
    pub erstba: u64,
    pub erdp: u64,
    pub ports: Vec<XhciPortDiag>,
}

pub fn controller_mmio_diag(controller_index: usize) -> Option<XhciMmioDiag> {
    if controller_index != 0 {
        return None;
    }
    let snapshot = super::lab::latest_snapshot()?;
    Some(XhciMmioDiag {
        caplen: snapshot.caplength,
        hcsparams1: snapshot.hcsparams1,
        hccparams1: snapshot.hccparams1,
        dboff: snapshot.dboff,
        rtsoff: snapshot.rtsoff,
        usbcmd: snapshot.usbcmd,
        usbsts: snapshot.usbsts,
        crcr: snapshot.crcr,
        dcbaap: snapshot.dcbaap,
        config: snapshot.config,
        iman: snapshot.iman,
        imod: snapshot.imod,
        erstsz: snapshot.erstsz,
        erstba: snapshot.erstba,
        erdp: snapshot.erdp,
        ports: snapshot
            .ports
            .iter()
            .map(|port| XhciPortDiag {
                port_id: port.port_id,
                portsc: port.portsc,
                portpmsc: port.portpmsc,
                portli: port.portli,
            })
            .collect(),
    })
}
