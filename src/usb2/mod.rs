extern crate alloc;

use alloc::vec::Vec;
use core::ptr::NonNull;
use spin::Mutex;

pub(crate) mod xhci {
    pub const MAX_XHCI_CONTROLLERS: usize = 4;
}

pub(crate) mod api;
mod crabusb_service;
pub(crate) mod hid;
mod mass;
#[path = "device/midi.rs"]
pub(crate) mod midi;
#[path = "device/pen.rs"]
pub(crate) mod pen;
pub(crate) mod scsi;

#[derive(Clone, Copy)]
struct CachedUsbControllerMmio {
    bus: u8,
    slot: u8,
    function: u8,
    phys_base: u64,
    map_len: usize,
    virt_base: usize,
}

static USB_CONTROLLER_MMIO_CACHE: Mutex<Vec<CachedUsbControllerMmio>> = Mutex::new(Vec::new());

pub(crate) use self::hid::TrueosHidCursorEvent;
pub(crate) use self::hid::{
    handle_keyboard_boot_report, handle_mouse_boot_report, remove_hid_slot,
};
pub(crate) use self::hid::{hut, input};

#[derive(Copy, Clone, Debug)]
pub(crate) struct UsbDeviceSummary {
    pub stable_id: u32,
    pub slot_id: u32,
    pub port: u8,
    pub root_port_id: u8,
    pub route_string: u32,
    pub kind: &'static str,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub class: Option<u8>,
    pub subclass: Option<u8>,
    pub protocol: Option<u8>,
}

#[derive(Clone, Copy)]
pub(crate) struct TlbUsbController {
    pub index: usize,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub mmio_base: NonNull<u8>,
    pub controller_phase: &'static str,
    pub root_hub_lifecycle: &'static str,
    pub event_ready: bool,
    pub root_port_change_seen: bool,
    pub empty_probe_streak: u32,
}

#[derive(Clone)]
pub(crate) struct TlbUsbEndpoint {
    pub address: u8,
    pub transfer_type: &'static str,
    pub max_packet_size: u16,
    pub interval: u8,
}

#[derive(Clone)]
pub(crate) struct TlbUsbInterface {
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<TlbUsbEndpoint>,
}

#[derive(Clone)]
pub(crate) struct TlbUsbConfiguration {
    pub configuration_value: u8,
    pub attributes: u8,
    pub max_power: u8,
    pub interfaces: Vec<TlbUsbInterface>,
}

#[derive(Clone)]
pub(crate) struct TlbUsbPathHop {
    pub slot_id: u32,
    pub port_id: u8,
    pub hub_depth: u8,
    pub speed: &'static str,
}

#[derive(Clone)]
pub(crate) struct TlbUsbDevice {
    pub controller_index: usize,
    pub stable_id: u32,
    pub slot_id: u32,
    pub root_port_id: u8,
    pub route_string: u32,
    pub path: Vec<u8>,
    pub port_id: u8,
    pub speed: &'static str,
    pub parent_hub_slot_id: Option<u32>,
    pub hub_path: Vec<TlbUsbPathHop>,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub num_configurations: u8,
    pub max_packet_size_0: u8,
    pub configurations: Vec<TlbUsbConfiguration>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TlbUsbTopologyNodeKind {
    RootPort,
    Hub,
    Device,
}

#[derive(Clone)]
pub(crate) struct TlbUsbTopologyNode {
    pub controller_index: usize,
    pub kind: TlbUsbTopologyNodeKind,
    pub slot_id: Option<u32>,
    pub root_port_id: u8,
    pub port_id: u8,
    pub depth: u8,
    pub parent_slot_id: Option<u32>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub class: Option<u8>,
    pub subclass: Option<u8>,
    pub protocol: Option<u8>,
    pub speed: &'static str,
}

pub(crate) struct TlbUsbSnapshot {
    pub controllers: Vec<TlbUsbController>,
    pub devices: Vec<TlbUsbDevice>,
    pub topology: Vec<TlbUsbTopologyNode>,
    pub probe_error: Option<&'static str>,
    pub probe_device_count: Option<u32>,
}

#[derive(Clone, Copy)]
pub(crate) struct UsbControllerRuntimeDiag {
    pub event_handler_ready: bool,
    pub probe_requested: bool,
    pub root_port_change_seen: bool,
    pub controller_phase: &'static str,
    pub root_hub_lifecycle: &'static str,
    pub empty_probe_streak: u32,
    pub probe_fail_streak: u32,
    pub last_probe_state: &'static str,
    pub last_probe_device_count: u32,
}

fn decode_mmio_bar(bar_lo: u32, bar_hi: Option<u32>) -> Option<u64> {
    if bar_lo == 0 || bar_lo == 0xFFFF_FFFF || (bar_lo & 0x1) != 0 {
        return None;
    }

    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    let base = if is_64 {
        (((bar_hi.unwrap_or(0) as u64) << 32) | (bar_lo as u64)) & !0xFu64
    } else {
        (bar_lo as u64) & !0xFu64
    };
    (base != 0).then_some(base)
}

fn controller_mmio_map(bus: u8, slot: u8, function: u8) -> Option<NonNull<u8>> {
    let (bar0_lo, bar0_hi) = crate::pci::read_bar0_raw(bus, slot, function);
    let phys_base = decode_mmio_bar(bar0_lo, bar0_hi)?;

    let mut map_len = crate::pci::bar_size_bytes(bus, slot, function, 0)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0x10_000);
    if map_len < 0x10_000 {
        map_len = 0x10_000;
    }
    if map_len > 0x10_0000 {
        map_len = 0x10_0000;
    }

    {
        let cache = USB_CONTROLLER_MMIO_CACHE.lock();
        if let Some(existing) = cache.iter().find(|entry| {
            entry.bus == bus
                && entry.slot == slot
                && entry.function == function
                && entry.phys_base == phys_base
                && entry.map_len == map_len
        }) {
            return NonNull::new(existing.virt_base as *mut u8);
        }
    }

    let virt_base = crate::pci::mmio::map_mmio_region_exact(phys_base, map_len).ok()?;

    let mut cache = USB_CONTROLLER_MMIO_CACHE.lock();
    if let Some(existing) = cache.iter().find(|entry| {
        entry.bus == bus
            && entry.slot == slot
            && entry.function == function
            && entry.phys_base == phys_base
            && entry.map_len == map_len
    }) {
        return NonNull::new(existing.virt_base as *mut u8);
    }

    cache.push(CachedUsbControllerMmio {
        bus,
        slot,
        function,
        phys_base,
        map_len,
        virt_base: virt_base.as_ptr() as usize,
    });

    Some(virt_base)
}

pub(crate) fn pci_usb_controllers() -> Vec<TlbUsbController> {
    const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;
    const PCI_SUBCLASS_USB: u8 = 0x03;
    const PCI_PROGIF_XHCI: u8 = 0x30;

    crate::pci::enumerate_impl();

    let mut ctrls = Vec::new();
    crate::pci::with_devices(|list| {
        for dev in list.iter() {
            if dev.class != PCI_CLASS_SERIAL_BUS
                || dev.subclass != PCI_SUBCLASS_USB
                || dev.prog_if != PCI_PROGIF_XHCI
            {
                continue;
            }

            let Some(mmio_base) = controller_mmio_map(dev.bus, dev.slot, dev.function) else {
                continue;
            };

            ctrls.push(TlbUsbController {
                index: ctrls.len(),
                bus: dev.bus,
                slot: dev.slot,
                function: dev.function,
                vendor_id: dev.vendor,
                device_id: dev.device,
                mmio_base,
                controller_phase: self::crabusb_service::diag_phase(ctrls.len())
                    .unwrap_or("startup"),
                root_hub_lifecycle: self::crabusb_service::diag_root_hub_lifecycle(ctrls.len())
                    .unwrap_or("init"),
                event_ready: false,
                root_port_change_seen: false,
                empty_probe_streak: 0,
            });
        }
    });

    for ctrl in ctrls.iter_mut() {
        if let Some((event_ready, root_port_change_seen, empty_probe_streak)) =
            self::crabusb_service::diag_counters(ctrl.index)
        {
            ctrl.event_ready = event_ready;
            ctrl.root_port_change_seen = root_port_change_seen;
            ctrl.empty_probe_streak = empty_probe_streak;
        }
    }

    ctrls
}

#[inline]
pub(crate) fn discover_first_controller() -> Option<TlbUsbController> {
    pci_usb_controllers().into_iter().next()
}

#[inline]
fn controller_by_index(controller_id: usize) -> Option<TlbUsbController> {
    pci_usb_controllers()
        .into_iter()
        .find(|info| info.index == controller_id)
}

pub(crate) fn request_probe(controller_id: usize) -> Result<(), &'static str> {
    if controller_by_index(controller_id).is_none() {
        return Err("controller not found");
    }

    if self::crabusb_service::request_probe(controller_id) {
        Ok(())
    } else {
        Err("controller out of range")
    }
}

pub(crate) fn request_rebind(controller_id: usize) -> Result<(), &'static str> {
    if controller_by_index(controller_id).is_none() {
        return Err("controller not found");
    }

    if self::crabusb_service::request_rebind(controller_id) {
        Ok(())
    } else {
        Err("controller out of range")
    }
}

pub(crate) fn runtime_diag(controller_id: usize) -> Option<UsbControllerRuntimeDiag> {
    self::crabusb_service::runtime_diag(controller_id).map(|diag| UsbControllerRuntimeDiag {
        event_handler_ready: diag.event_handler_ready,
        probe_requested: diag.probe_requested,
        root_port_change_seen: diag.root_port_change_seen,
        controller_phase: diag.controller_phase,
        root_hub_lifecycle: diag.root_hub_lifecycle,
        empty_probe_streak: diag.empty_probe_streak,
        probe_fail_streak: diag.probe_fail_streak,
        last_probe_state: diag.last_probe_state,
        last_probe_device_count: diag.last_probe_device_count,
    })
}

fn classify_descriptor_kind(desc: &crab_usb::usb_if::descriptor::DeviceDescriptor) -> &'static str {
    match desc.class {
        0x03 => "hid",
        0x08 => "mass",
        0x07 => "printer",
        0x02 => "cdc",
        0x01 => "uac",
        _ => "unknown",
    }
}

pub(crate) fn list_device_summaries(controller_id: usize) -> Vec<UsbDeviceSummary> {
    let Some(info) = controller_by_index(controller_id) else {
        return Vec::new();
    };

    crate::wait::spawn_and_wait_local(async move {
        crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);

        let mut host = match crab_usb::USBHost::new_xhci(
            info.mmio_base,
            &self::crabusb_service::CRABUSB_KERNEL,
        ) {
            Ok(host) => host,
            Err(_) => return Vec::new(),
        };
        if host.init().await.is_err() {
            return Vec::new();
        }

        let topology = match host.topology().await {
            Ok(tree) => tree,
            Err(_) => return Vec::new(),
        };

        let mut out = Vec::new();
        for handle in topology.iter().filter(|node| !node.is_hub).cloned().map(crab_usb::DeviceHandle::from) {
            let desc = handle.descriptor();
            let slot_id = match host.open_handle(&handle).await {
                Ok(device) => u32::from(device.slot_id()),
                Err(_) => 0,
            };
            out.push(UsbDeviceSummary {
                stable_id: handle.id().raw(),
                slot_id,
                port: handle.port(),
                root_port_id: handle.location().root_port,
                route_string: handle.location().route_string,
                kind: classify_descriptor_kind(desc),
                vid: Some(desc.vendor_id),
                pid: Some(desc.product_id),
                class: Some(desc.class),
                subclass: Some(desc.subclass),
                protocol: Some(desc.protocol),
            });
        }
        out
    })
}

pub(crate) mod syscall {
    use alloc::vec;
    use alloc::vec::Vec;

    use crab_usb::DeviceId;
    use crab_usb::usb_if::descriptor::DescriptorType;
    use crab_usb::usb_if::host::ControlSetup;
    use crab_usb::usb_if::transfer::{Recipient, Request, RequestType};

    const DESC_MAX: usize = 256;

    fn with_device_descriptor_read_by_id(
        controller_id: usize,
        stable_id: u32,
        setup: ControlSetup,
        length: u16,
    ) -> Option<Vec<u8>> {
        let info = super::controller_by_index(controller_id)?;
        crate::wait::spawn_and_wait_local(async move {
            crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);
            let mut host = crab_usb::USBHost::new_xhci(
                info.mmio_base,
                &super::crabusb_service::CRABUSB_KERNEL,
            )
            .ok()?;
            host.init().await.ok()?;
            let handle = host.device(DeviceId(stable_id)).await.ok()??;
            let mut device = host.open_handle(&handle).await.ok()?;
            let mut buf = vec![0u8; usize::from(length).min(DESC_MAX)];
            let read = device.control_in(setup, buf.as_mut_slice()).await.ok()?;
            buf.truncate(read.min(buf.len()));
            Some(buf)
        })
    }

    fn stable_id_for_slot(controller_id: usize, slot_id: u32) -> Option<u32> {
        super::crabusb_service::diag_devices(controller_id)
            .into_iter()
            .find(|dev| dev.slot_id == slot_id)
            .map(|dev| dev.stable_id)
    }

    pub fn port_reset(_controller_id: usize, _port_idx: usize) -> i32 {
        -1
    }

    pub fn control_get_descriptor(
        controller_id: usize,
        slot_id: u32,
        desc_type: u8,
        desc_index: u8,
        length: u16,
        _timeout_ms: u64,
    ) -> Option<Vec<u8>> {
        let stable_id = stable_id_for_slot(controller_id, slot_id)?;
        control_get_descriptor_by_id(
            controller_id,
            stable_id,
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Device,
                request: Request::GetDescriptor,
                value: ((DescriptorType::from(desc_type).0 as u16) << 8) | u16::from(desc_index),
                index: 0,
            },
            length,
        )
    }

    pub fn control_get_descriptor_by_id(
        controller_id: usize,
        stable_id: u32,
        setup: ControlSetup,
        length: u16,
    ) -> Option<Vec<u8>> {
        with_device_descriptor_read_by_id(controller_id, stable_id, setup, length)
    }

    pub fn control_get_hid_descriptor(
        controller_id: usize,
        slot_id: u32,
        interface_number: u16,
        length: u16,
        _timeout_ms: u64,
    ) -> Option<Vec<u8>> {
        let stable_id = stable_id_for_slot(controller_id, slot_id)?;
        control_get_hid_descriptor_by_id(
            controller_id,
            stable_id,
            interface_number,
            length,
        )
    }

    pub fn control_get_hid_descriptor_by_id(
        controller_id: usize,
        stable_id: u32,
        interface_number: u16,
        length: u16,
    ) -> Option<Vec<u8>> {
        with_device_descriptor_read_by_id(
            controller_id,
            stable_id,
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Interface,
                request: Request::GetDescriptor,
                value: (0x21u16) << 8,
                index: interface_number,
            },
            length,
        )
    }

    pub fn control_get_hid_report_descriptor(
        controller_id: usize,
        slot_id: u32,
        interface_number: u16,
        length: u16,
        _timeout_ms: u64,
    ) -> Option<Vec<u8>> {
        let stable_id = stable_id_for_slot(controller_id, slot_id)?;
        control_get_hid_report_descriptor_by_id(
            controller_id,
            stable_id,
            interface_number,
            length,
        )
    }

    pub fn control_get_hid_report_descriptor_by_id(
        controller_id: usize,
        stable_id: u32,
        interface_number: u16,
        length: u16,
    ) -> Option<Vec<u8>> {
        with_device_descriptor_read_by_id(
            controller_id,
            stable_id,
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Interface,
                request: Request::GetDescriptor,
                value: (0x22u16) << 8,
                index: interface_number,
            },
            length,
        )
    }

    pub fn read_transfer_event(
        _controller_id: usize,
        _slot_id: u32,
        _ep_target: u32,
    ) -> Option<(u32, u32)> {
        None
    }
}

pub(crate) fn tlb_snapshot() -> TlbUsbSnapshot {
    let controllers = pci_usb_controllers();
    if controllers.is_empty() {
        return TlbUsbSnapshot {
            controllers,
            devices: Vec::new(),
            topology: Vec::new(),
            probe_error: None,
            probe_device_count: None,
        };
    }

    // `tlb usb` is a diagnostic dump, not an ownership handoff. Reinitializing or
    // reprobeing a live XHCI controller here races the active crabusb BSP service.
    // Keep this passive by reading the device cache populated by the running service.
    let mut devices = Vec::new();
    let mut topology = Vec::new();
    let mut probe_error = None;
    let mut probe_device_count = None;
    for ctrl in controllers.iter() {
        devices.extend(self::crabusb_service::diag_devices(ctrl.index));
        topology.extend(self::crabusb_service::diag_topology(ctrl.index));
        if probe_error.is_none() {
            probe_error = self::crabusb_service::diag_probe_error(ctrl.index);
        }
        if probe_device_count.is_none() {
            probe_device_count = self::crabusb_service::diag_probe_device_count(ctrl.index);
        }
    }

    TlbUsbSnapshot {
        controllers,
        devices,
        topology,
        probe_error,
        probe_device_count,
    }
}

pub(crate) use self::crabusb_service::{
    audio_task as crabusb_audio_task, bsp_service as crabusb_bsp_service,
    event_pump_task as crabusb_event_pump_task, truekey_task as crabusb_truekey_task,
};
