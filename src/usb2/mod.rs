extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::ptr::NonNull;
use spin::Mutex;

pub(crate) mod xhci {
    pub const MAX_XHCI_CONTROLLERS: usize = 4;
}

pub(crate) mod api;
pub(crate) mod class;
mod crabusb_service;
pub(crate) mod descriptor;
pub(crate) mod hid;
pub(crate) mod mass;
#[path = "device/midi.rs"]
pub(crate) mod midi;
#[path = "device/pen.rs"]
pub(crate) mod pen;
pub(crate) mod scsi;
#[path = "device/skhynix_green.rs"]
pub(crate) mod skhynix_green;
pub(crate) mod sound;
pub(crate) mod video;

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

pub(crate) use self::hid::{hut, input};

#[derive(Clone, Debug)]
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
    pub product: Option<String>,
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
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
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
    pub early_fatal_rebind_streak: u32,
    pub recovery_quiescent_before_bind: bool,
    pub recovery_quiescent_ms: u64,
    pub recovery_skip_delayed_event_handler: bool,
    pub recovery_initial_settle_ms: u64,
    pub recovery_probe_quiet_ms: u64,
}

impl UsbControllerRuntimeDiag {
    pub(crate) const fn new() -> Self {
        Self {
            event_handler_ready: false,
            probe_requested: false,
            root_port_change_seen: false,
            controller_phase: "init",
            root_hub_lifecycle: "init",
            empty_probe_streak: 0,
            probe_fail_streak: 0,
            last_probe_state: "never",
            last_probe_device_count: 0,
            early_fatal_rebind_streak: 0,
            recovery_quiescent_before_bind: false,
            recovery_quiescent_ms: 0,
            recovery_skip_delayed_event_handler: false,
            recovery_initial_settle_ms: 0,
            recovery_probe_quiet_ms: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct UsbPortRuntimeDiag {
    pub port_id: u8,
    pub portsc: u32,
    pub portpmsc: u32,
    pub portli: u32,
}

#[derive(Clone)]
pub(crate) struct UsbControllerMmioDiag {
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
    pub ports: Vec<UsbPortRuntimeDiag>,
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

            let index = ctrls.len();
            let runtime = self::crabusb_service::runtime_diag(index);
            ctrls.push(TlbUsbController {
                index,
                bus: dev.bus,
                slot: dev.slot,
                function: dev.function,
                vendor_id: dev.vendor,
                device_id: dev.device,
                mmio_base,
                controller_phase: runtime.controller_phase,
                root_hub_lifecycle: runtime.root_hub_lifecycle,
                event_ready: runtime.event_handler_ready,
                root_port_change_seen: runtime.root_port_change_seen,
                empty_probe_streak: runtime.empty_probe_streak,
            });
        }
    });

    ctrls
}

#[inline]
pub(crate) fn discover_first_controller() -> Option<TlbUsbController> {
    pci_usb_controllers().into_iter().next()
}

#[inline]
pub(crate) fn controller_by_index(controller_id: usize) -> Option<TlbUsbController> {
    pci_usb_controllers()
        .into_iter()
        .find(|info| info.index == controller_id)
}

#[inline]
unsafe fn read_mmio32(base: *const u8, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile(base.add(offset) as *const u32) }
}

#[inline]
unsafe fn read_mmio64(base: *const u8, offset: usize) -> u64 {
    let lo = unsafe { read_mmio32(base, offset) } as u64;
    let hi = unsafe { read_mmio32(base, offset + 4) } as u64;
    lo | (hi << 32)
}

pub(crate) fn controller_mmio_diag(controller_id: usize) -> Option<UsbControllerMmioDiag> {
    let info = controller_by_index(controller_id)?;
    crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);
    let mmio = info.mmio_base.as_ptr() as *const u8;

    unsafe {
        let caplen = (read_mmio32(mmio, 0x00) & 0xFF) as u8;
        let hcsparams1 = read_mmio32(mmio, 0x04);
        let hccparams1 = read_mmio32(mmio, 0x10);
        let dboff = read_mmio32(mmio, 0x14) & !0x3;
        let rtsoff = read_mmio32(mmio, 0x18) & !0x1F;
        let op_base = usize::from(caplen);
        let usbcmd = read_mmio32(mmio, op_base);
        let usbsts = read_mmio32(mmio, op_base + 0x04);
        let crcr = read_mmio64(mmio, op_base + 0x18);
        let dcbaap = read_mmio64(mmio, op_base + 0x30);
        let config = read_mmio32(mmio, op_base + 0x38);
        let runtime_base = rtsoff as usize;
        let iman = read_mmio32(mmio, runtime_base + 0x20);
        let imod = read_mmio32(mmio, runtime_base + 0x24);
        let erstsz = read_mmio32(mmio, runtime_base + 0x28);
        let erstba = read_mmio64(mmio, runtime_base + 0x30);
        let erdp = read_mmio64(mmio, runtime_base + 0x38);

        let max_ports = ((hcsparams1 >> 24) & 0xFF) as usize;
        let mut ports = Vec::with_capacity(max_ports);
        for index in 0..max_ports {
            let port_base = op_base + 0x400 + (index * 0x10);
            ports.push(UsbPortRuntimeDiag {
                port_id: (index + 1) as u8,
                portsc: read_mmio32(mmio, port_base),
                portpmsc: read_mmio32(mmio, port_base + 0x04),
                portli: read_mmio32(mmio, port_base + 0x08),
            });
        }

        Some(UsbControllerMmioDiag {
            caplen,
            hcsparams1,
            hccparams1,
            dboff,
            rtsoff,
            usbcmd,
            usbsts,
            crcr,
            dcbaap,
            config,
            iman,
            imod,
            erstsz,
            erstba,
            erdp,
            ports,
        })
    }
}

pub(crate) fn usb_topology_nodes(
    controllers: &[TlbUsbController],
    devices: &[TlbUsbDevice],
) -> Vec<TlbUsbTopologyNode> {
    let mut nodes = Vec::new();
    for ctrl in controllers {
        if let Some(diag) = controller_mmio_diag(ctrl.index) {
            for port in diag.ports {
                nodes.push(TlbUsbTopologyNode {
                    controller_index: ctrl.index,
                    kind: TlbUsbTopologyNodeKind::RootPort,
                    slot_id: None,
                    root_port_id: port.port_id,
                    port_id: port.port_id,
                    depth: 0,
                    parent_slot_id: None,
                    vendor_id: None,
                    product_id: None,
                    class: None,
                    subclass: None,
                    protocol: None,
                    speed: "root",
                });
            }
        }
    }

    for dev in devices {
        let kind = if dev.class == 0x09 {
            TlbUsbTopologyNodeKind::Hub
        } else {
            TlbUsbTopologyNodeKind::Device
        };
        nodes.push(TlbUsbTopologyNode {
            controller_index: dev.controller_index,
            kind,
            slot_id: Some(dev.slot_id),
            root_port_id: dev.root_port_id,
            port_id: dev.port_id,
            depth: dev.hub_path.len() as u8,
            parent_slot_id: dev.parent_hub_slot_id,
            vendor_id: Some(dev.vendor_id),
            product_id: Some(dev.product_id),
            class: Some(dev.class),
            subclass: Some(dev.subclass),
            protocol: Some(dev.protocol),
            speed: dev.speed,
        });
    }
    nodes
}

pub(crate) fn tlb_usb_snapshot() -> TlbUsbSnapshot {
    let controllers = pci_usb_controllers();
    let mut devices = Vec::new();
    let mut probe_error = None;
    for ctrl in controllers.iter() {
        match crabusb_observed_devices(ctrl.index) {
            Ok(mut observed) => devices.append(&mut observed),
            Err(err) => probe_error = Some(err),
        }
    }
    let topology = usb_topology_nodes(controllers.as_slice(), devices.as_slice());
    let probe_device_count = Some(devices.len() as u32);
    TlbUsbSnapshot {
        controllers,
        devices,
        topology,
        probe_error,
        probe_device_count,
    }
}

pub(crate) mod syscall {}

pub(crate) use self::crabusb_service::bsp_service as crabusb_bsp_service;
pub(crate) use self::crabusb_service::observed_device_summaries as crabusb_observed_device_summaries;
pub(crate) use self::crabusb_service::observed_devices as crabusb_observed_devices;
pub(crate) use self::crabusb_service::runtime_diag as crabusb_runtime_diag;
