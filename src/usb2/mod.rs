extern crate alloc;

use alloc::vec::Vec;
use core::ptr::NonNull;
use spin::Mutex;

pub(crate) mod xhci {
    pub const MAX_XHCI_CONTROLLERS: usize = 4;
}

mod keyboard;
mod mouse;

pub(crate) mod hid {
    pub use trueos_v::vinput::TrueosHidCursorEvent;

    pub mod classreq {
        #[repr(u8)]
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub enum HidReportType {
            Input = 1,
            Output = 2,
            Feature = 3,
        }

        #[inline]
        pub fn get_protocol_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _timeout_ms: u64,
        ) -> Option<u8> {
            None
        }

        #[inline]
        pub fn set_protocol_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _protocol: u8,
            _timeout_ms: u64,
        ) -> Option<u32> {
            None
        }

        #[inline]
        pub fn get_idle_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _report_id: u8,
            _timeout_ms: u64,
        ) -> Option<u8> {
            None
        }

        #[inline]
        pub fn set_idle_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _report_id: u8,
            _duration_4ms: u8,
            _timeout_ms: u64,
        ) -> Option<u32> {
            None
        }

        #[inline]
        pub fn get_report_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _report_type: HidReportType,
            _report_id: u8,
            _length: usize,
            _timeout_ms: u64,
        ) -> Option<heapless::Vec<u8, 256>> {
            None
        }

        #[inline]
        pub fn set_report_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _report_type: HidReportType,
            _report_id: u8,
            _data: &[u8],
            _timeout_ms: u64,
        ) -> Option<u32> {
            None
        }
    }

    #[inline]
    pub fn pop_cursor_event() -> Option<TrueosHidCursorEvent> {
        super::pop_cursor_event()
    }

    #[inline]
    pub fn read_cursor_events_since(
        read_seq: u64,
        out: &mut [TrueosHidCursorEvent],
    ) -> (u64, u32, usize) {
        super::read_cursor_events_since(read_seq, out)
    }

    #[inline]
    pub fn inject_virtual_cursor_event(
        slot_id: u32,
        x: f64,
        y: f64,
        buttons_down: u32,
        wheel: i16,
        flags: u32,
    ) {
        super::inject_virtual_cursor_event(slot_id, x, y, buttons_down, wheel, flags)
    }
}

pub(crate) mod api;
mod crabusb_service;
pub(crate) mod hut;
pub(crate) mod input;
mod mass;
pub(crate) use self::hid::TrueosHidCursorEvent;

const HID_MOUSE_NORM_PER_DELTA: f64 = 1.0 / 1024.0;
const HID_KIND_KEYBOARD: u8 = 1;
const HID_KIND_MOUSE: u8 = 2;
const HID_KIND_VIRTUAL_CURSOR: u8 = 0;
const CURSOR_EVENT_RING_CAP: usize = 2048;

const ZERO_CURSOR_EVENT: hid::TrueosHidCursorEvent = hid::TrueosHidCursorEvent {
    t_ms: 0,
    seq: 0,
    controller_id: 0,
    slot_id: 0,
    ep_target: 0,
    hid_kind: 0,
    reserved0: 0,
    reserved1: 0,
    buttons_down: 0,
    wheel: 0,
    reserved2: 0,
    x: 0.0,
    y: 0.0,
    flags: 0,
};

struct CursorEventRing {
    buf: [hid::TrueosHidCursorEvent; CURSOR_EVENT_RING_CAP],
    write_seq: u64,
}

impl CursorEventRing {
    const fn new() -> Self {
        Self {
            buf: [ZERO_CURSOR_EVENT; CURSOR_EVENT_RING_CAP],
            write_seq: 0,
        }
    }
}

struct HidRuntime {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u8,
    seq: u64,
    last_nonzero_seq: u64,
    mouse_x: f64,
    mouse_y: f64,
    mouse_buttons_down: u32,
    keyboard_modifiers: u8,
    keyboard_keys: [u8; 6],
    keyboard_ascii: [u8; 6],
    keyboard_ring: keyboard::KeyboardRing,
    mouse_ring: mouse::MouseRing,
}

impl HidRuntime {
    fn new(controller_id: u32, slot_id: u32, ep_target: u32, hid_kind: u8) -> Self {
        Self {
            controller_id,
            slot_id,
            ep_target,
            hid_kind,
            seq: 0,
            last_nonzero_seq: 0,
            mouse_x: 0.5,
            mouse_y: 0.5,
            mouse_buttons_down: 0,
            keyboard_modifiers: 0,
            keyboard_keys: [0; 6],
            keyboard_ascii: [0; 6],
            keyboard_ring: keyboard::KeyboardRing::new(),
            mouse_ring: mouse::MouseRing::new(),
        }
    }
}

static HID_RUNTIMES: Mutex<Vec<HidRuntime>> = Mutex::new(Vec::new());
static CURSOR_EVENT_RING: Mutex<CursorEventRing> = Mutex::new(CursorEventRing::new());
static CURSOR_EVENT_POP_SEQ: Mutex<u64> = Mutex::new(0);

const HID_DEBUG_REPORT_LOGS: bool = crate::logflag::HID_DEBUG_REPORT_LOGS;

#[inline]
fn clamp01(value: f64) -> f64 {
    if value < 0.0 {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}

#[inline]
fn now_ms_u32() -> u32 {
    let ticks = embassy_time_driver::now() as u128;
    let hz = embassy_time_driver::TICK_HZ as u128;
    if hz == 0 {
        0
    } else {
        ((ticks * 1000u128) / hz) as u32
    }
}

#[inline]
fn runtime_mut_or_insert(
    runtimes: &mut Vec<HidRuntime>,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u8,
) -> &mut HidRuntime {
    if let Some(idx) = runtimes.iter().position(|runtime| {
        runtime.controller_id == controller_id
            && runtime.slot_id == slot_id
            && runtime.ep_target == ep_target
            && runtime.hid_kind == hid_kind
    }) {
        return &mut runtimes[idx];
    }

    runtimes.push(HidRuntime::new(controller_id, slot_id, ep_target, hid_kind));
    let idx = runtimes.len() - 1;
    &mut runtimes[idx]
}

#[inline]
fn push_cursor_event(event: hid::TrueosHidCursorEvent) {
    let mut ring = CURSOR_EVENT_RING.lock();
    ring.write_seq = ring.write_seq.wrapping_add(1);
    let idx = ((ring.write_seq - 1) as usize) % CURSOR_EVENT_RING_CAP;
    ring.buf[idx] = event;
}

fn pop_cursor_event() -> Option<hid::TrueosHidCursorEvent> {
    let mut read_seq = CURSOR_EVENT_POP_SEQ.lock();
    let mut out = [ZERO_CURSOR_EVENT; 1];
    let (next_seq, _dropped, wrote) = read_cursor_events_since(*read_seq, &mut out);
    if wrote == 0 {
        return None;
    }
    *read_seq = next_seq;
    Some(out[0])
}

fn read_cursor_events_since(
    read_seq: u64,
    out: &mut [hid::TrueosHidCursorEvent],
) -> (u64, u32, usize) {
    let ring = CURSOR_EVENT_RING.lock();
    if ring.write_seq == 0 || out.is_empty() {
        return (read_seq, 0, 0);
    }

    let cap = CURSOR_EVENT_RING_CAP as u64;
    let oldest = if ring.write_seq > cap {
        ring.write_seq - cap + 1
    } else {
        1
    };

    let mut start = read_seq.wrapping_add(1);
    let mut dropped = 0u32;
    if start < oldest {
        dropped = core::cmp::min(u32::MAX as u64, oldest - start) as u32;
        start = oldest;
    }
    if start > ring.write_seq {
        return (read_seq, dropped, 0);
    }

    let mut wrote = 0usize;
    let mut seq = start;
    while seq <= ring.write_seq && wrote < out.len() {
        let idx = ((seq - 1) as usize) % CURSOR_EVENT_RING_CAP;
        out[wrote] = ring.buf[idx];
        wrote += 1;
        seq = seq.wrapping_add(1);
    }

    let next_seq = if wrote == 0 {
        read_seq
    } else {
        start + (wrote as u64) - 1
    };
    (next_seq, dropped, wrote)
}

#[inline]
fn sync_runtime_cursor_snapshot(runtime: &HidRuntime) {
    crate::v::cursor::upsert_snapshot(
        runtime.controller_id,
        runtime.slot_id,
        runtime.ep_target,
        runtime.hid_kind,
        runtime.mouse_x,
        runtime.mouse_y,
        runtime.mouse_buttons_down,
    );
}

fn inject_virtual_cursor_event(
    slot_id: u32,
    x: f64,
    y: f64,
    buttons_down: u32,
    wheel: i16,
    flags: u32,
) {
    let event = hid::TrueosHidCursorEvent {
        t_ms: now_ms_u32(),
        seq: 0,
        controller_id: 0,
        slot_id,
        ep_target: 0,
        hid_kind: HID_KIND_VIRTUAL_CURSOR,
        reserved0: 0,
        reserved1: 0,
        buttons_down,
        wheel,
        reserved2: 0,
        x: clamp01(x),
        y: clamp01(y),
        flags,
    };
    push_cursor_event(event);
    crate::v::cursor::upsert_snapshot(0, slot_id, 0, HID_KIND_VIRTUAL_CURSOR, x, y, buttons_down);
    crate::usb2::hut::upsert_mouse_state(
        0,
        slot_id,
        0,
        clamp01(x),
        clamp01(y),
        buttons_down,
        crate::usb2::hut::HidSourceKind::Unknown,
        "virtual",
        true,
    );
}

pub(crate) fn handle_keyboard_boot_report(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    data: &[u8],
) {
    let now_ms = now_ms_u32();
    let mut runtimes = HID_RUNTIMES.lock();
    let runtime = runtime_mut_or_insert(
        &mut runtimes,
        controller_id,
        slot_id,
        ep_target,
        HID_KIND_KEYBOARD,
    );
    runtime.seq = runtime.seq.wrapping_add(1);
    keyboard::handle_report(runtime, data, now_ms);
}

pub(crate) fn handle_mouse_boot_report(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    data: &[u8],
) {
    let now_ms = now_ms_u32();
    let mut runtimes = HID_RUNTIMES.lock();
    let runtime = runtime_mut_or_insert(
        &mut runtimes,
        controller_id,
        slot_id,
        ep_target,
        HID_KIND_MOUSE,
    );
    runtime.seq = runtime.seq.wrapping_add(1);
    mouse::handle_report(runtime, data, now_ms);
}

pub(crate) fn remove_hid_slot(controller_id: u32, slot_id: u32) {
    let mut runtimes = HID_RUNTIMES.lock();
    runtimes.retain(|runtime| {
        !(runtime.controller_id == controller_id && runtime.slot_id == slot_id)
    });
    let _ = crate::usb2::hut::remove_slot(controller_id, slot_id);
    let _ = crate::v::cursor::remove_snapshots(controller_id, slot_id);
    let _ = crate::v::keyboard::remove_snapshots(controller_id, slot_id);
}

fn keyboard_ring_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: &mut [keyboard::TrueosHidKeyboardSample],
) -> (u32, usize) {
    let mut runtimes = HID_RUNTIMES.lock();
    let Some(runtime) = runtimes.iter_mut().find(|runtime| {
        runtime.controller_id == controller_id
            && runtime.slot_id == slot_id
            && runtime.ep_target == ep_target
            && runtime.hid_kind == HID_KIND_KEYBOARD
    }) else {
        return (0, 0);
    };

    let dropped = runtime.keyboard_ring.dropped;
    runtime.keyboard_ring.dropped = 0;

    let mut wrote = 0usize;
    while wrote < out.len() {
        let Some(sample) = runtime.keyboard_ring.pop() else {
            break;
        };
        out[wrote] = sample;
        wrote += 1;
    }
    (dropped, wrote)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_keyboard_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: *mut keyboard::TrueosHidKeyboardSample,
    out_cap: u32,
    out_dropped: *mut u32,
) -> u32 {
    if !out_dropped.is_null() {
        *out_dropped = 0;
    }
    if out_cap == 0 || out.is_null() {
        return 0;
    }

    let out_slice = core::slice::from_raw_parts_mut(out, out_cap as usize);
    let (dropped, wrote) = keyboard_ring_read(controller_id, slot_id, ep_target, out_slice);
    if !out_dropped.is_null() {
        *out_dropped = dropped;
    }
    wrote as u32
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct UsbDeviceSummary {
    pub slot_id: u32,
    pub port: u8,
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
}

#[derive(Clone, Copy)]
pub(crate) struct TlbUsbDevice {
    pub controller_index: usize,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub config_count: usize,
    pub interface_count: usize,
}

pub(crate) struct TlbUsbSnapshot {
    pub controllers: Vec<TlbUsbController>,
    pub devices: Vec<TlbUsbDevice>,
    pub probe_error: Option<&'static str>,
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

    crate::pci::mmio::map_mmio_region(phys_base, map_len).ok()
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
fn controller_by_index(controller_id: usize) -> Option<TlbUsbController> {
    pci_usb_controllers()
        .into_iter()
        .find(|info| info.index == controller_id)
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

        let found = match host.probe_devices().await {
            Ok(found) => found,
            Err(_) => return Vec::new(),
        };

        let mut out = Vec::new();
        for dev_info in found.iter() {
            let desc = dev_info.descriptor();
            let slot_id = match host.open_device(dev_info).await {
                Ok(device) => u32::from(device.slot_id()),
                Err(_) => 0,
            };
            out.push(UsbDeviceSummary {
                slot_id,
                port: 0,
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
    use core::ptr::NonNull;

    use crab_usb::usb_if::descriptor::DescriptorType;
    use crab_usb::usb_if::host::ControlSetup;
    use crab_usb::usb_if::transfer::{Recipient, Request, RequestType};

    const DESC_MAX: usize = 256;

    fn with_device_descriptor_read(
        controller_id: usize,
        slot_id: u32,
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
            let found = host.probe_devices().await.ok()?;

            for dev_info in found.iter() {
                let mut device = host.open_device(dev_info).await.ok()?;
                if u32::from(device.slot_id()) != slot_id {
                    continue;
                }
                let mut buf = vec![0u8; usize::from(length).min(DESC_MAX)];
                let read = device.control_in(setup, buf.as_mut_slice()).await.ok()?;
                buf.truncate(read.min(buf.len()));
                return Some(buf);
            }
            None
        })
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
        with_device_descriptor_read(
            controller_id,
            slot_id,
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

    pub fn control_get_hid_descriptor(
        controller_id: usize,
        slot_id: u32,
        interface_number: u16,
        length: u16,
        _timeout_ms: u64,
    ) -> Option<Vec<u8>> {
        with_device_descriptor_read(
            controller_id,
            slot_id,
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
        with_device_descriptor_read(
            controller_id,
            slot_id,
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
            probe_error: None,
        };
    }

    let probe_ctrl = controllers[0];
    let devices = crate::wait::spawn_and_wait_local(async move {
        let mut out = Vec::new();

        crate::pci::enable_mem_and_bus_master(probe_ctrl.bus, probe_ctrl.slot, probe_ctrl.function);

        let mut host = match crab_usb::USBHost::new_xhci(
            probe_ctrl.mmio_base,
            &self::crabusb_service::CRABUSB_KERNEL,
        ) {
            Ok(host) => host,
            Err(_) => return Err("host-new"),
        };

        if host.init().await.is_err() {
            return Err("host-init");
        }

        match host.probe_devices().await {
            Ok(found) => {
                for dev in found.iter() {
                    let desc = dev.descriptor();
                    out.push(TlbUsbDevice {
                        controller_index: probe_ctrl.index,
                        vendor_id: desc.vendor_id,
                        product_id: desc.product_id,
                        class: desc.class,
                        subclass: desc.subclass,
                        protocol: desc.protocol,
                        config_count: dev.configurations().len(),
                        interface_count: dev.interface_descriptors().count(),
                    });
                }
                Ok(out)
            }
            Err(_) => Err("probe"),
        }
    });

    match devices {
        Ok(devices) => TlbUsbSnapshot {
            controllers,
            devices,
            probe_error: None,
        },
        Err(probe_error) => TlbUsbSnapshot {
            controllers,
            devices: Vec::new(),
            probe_error: Some(probe_error),
        },
    }
}

pub(crate) use self::crabusb_service::{
    audio_task as crabusb_audio_task, bsp_service as crabusb_bsp_service,
    event_pump_task as crabusb_event_pump_task, truekey_task as crabusb_truekey_task,
};
