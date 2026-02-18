pub mod cdc;
pub mod cdc_acm;
pub mod hid;
pub mod hub;
pub mod input;
pub mod mass;
pub mod bot;
pub mod scsi;
pub mod pen;
pub mod print;
pub mod isoch;
pub mod uac;
pub mod midi;
pub mod leds;
pub mod truekey;
pub mod xhci;
mod scout;
mod enumeration;
mod control;
mod attach;

#[allow(unused_imports)]
pub use scout::{usb_scout_service, port_snapshot, ScoutedPort};
pub(crate) use self::control::{control_in, control_out};
pub(crate) use self::enumeration::{disable_slot, enable_slot, enumerate_port, enumerate_with_params};

use self::xhci::{hi, lo, trb_type, Trb, TrbRing, XhciContext, MAX_XHCI_CONTROLLERS};
use core::ptr::write_volatile;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;
use self::hub::{LOG_PORTS_MAX, MAX_DEVICES};

#[allow(dead_code)]
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

#[allow(dead_code)]
pub(crate) fn list_device_summaries(controller_id: usize) -> Vec<UsbDeviceSummary, MAX_DEVICES> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return Vec::new();
    }

    let guard = DEVICES[controller_id].lock();
    let mut out: Vec<UsbDeviceSummary, MAX_DEVICES> = Vec::new();

    for d in guard.iter() {
        let kind = match d.kind {
            DeviceKind::Hid => "hid",
            DeviceKind::Hub => "hub",
            DeviceKind::Mass => "mass",
            DeviceKind::Printer => "printer",
            DeviceKind::Pen => "pen",
            DeviceKind::Cdc => "cdc",
            DeviceKind::Uac => "uac",
            DeviceKind::Midi => "midi",
            DeviceKind::Leds => "leds",
            DeviceKind::Unknown => "unknown",
        };

        let ident = hub::identity_for_slot(controller_id, d.slot_id);
        let _ = out.push(UsbDeviceSummary {
            slot_id: d.slot_id,
            port: d.port,
            kind,
            vid: ident.map(|i| i.vid),
            pid: ident.map(|i| i.pid),
            class: ident.map(|i| i.class),
            subclass: ident.map(|i| i.subclass),
            protocol: ident.map(|i| i.protocol),
        });
    }

    out
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DeviceKind {
    Hid,
    Hub,
    Mass,
    Printer,
    Pen,
    Cdc,
    Uac,
    Midi,
    Leds,
    Unknown,
}

impl DeviceKind {
    fn claimed_log_label(self) -> &'static str {
        match self {
            DeviceKind::Hid => "Hid",
            DeviceKind::Hub => "Hub",
            DeviceKind::Mass => "Mass",
            DeviceKind::Printer => "Printer",
            DeviceKind::Pen => "Pen",
            DeviceKind::Cdc => "Cdc",
            DeviceKind::Uac => "Uac",
            DeviceKind::Midi => "Midi",
            DeviceKind::Leds => "Leds",
            DeviceKind::Unknown => "Unknown",
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct DeviceResources {
    dev_ctx_virt: usize,
    input_ctx_virt: usize,
    ep0_virt_raw: usize,
    ep0_bytes: usize,
}
#[derive(Copy, Clone, Debug)]
struct DeviceEntry {
    slot_id: u32,
    port: u8,
    kind: DeviceKind,
    resources: Option<DeviceResources>,
}

static DEVICES: [Mutex<Vec<DeviceEntry, MAX_DEVICES>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(Vec::new()) }; MAX_XHCI_CONTROLLERS];
static ENUM_READY: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static NOT_CLAIMED_KEY: [[AtomicU32; LOG_PORTS_MAX]; MAX_XHCI_CONTROLLERS] =
    [const { [const { AtomicU32::new(0) }; LOG_PORTS_MAX] }; MAX_XHCI_CONTROLLERS];
static NOT_CLAIMED_COUNT: [[AtomicU32; LOG_PORTS_MAX]; MAX_XHCI_CONTROLLERS] =
    [const { [const { AtomicU32::new(0) }; LOG_PORTS_MAX] }; MAX_XHCI_CONTROLLERS];

pub(crate) struct UsbControllerState {
    info: xhci::XhcInfo,
    ctx: XhciContext,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    dcbaa_virt: *mut u8,
    cmd_ring: TrbRing,
    _cmd_phys: u64,
    _cmd_virt: *mut u8,
    _evt_phys: u64,
    _evt_virt: *mut u8,
    _erst_phys: u64,
    _erst_virt: *mut u8,
}

unsafe impl Send for UsbControllerState {}
unsafe impl Sync for UsbControllerState {}

pub(crate) const USB_LOG_VERBOSE: bool = false;

macro_rules! usbv {
    ($($tt:tt)*) => {{
        if USB_LOG_VERBOSE {
            crate::log!($($tt)*);
        }
    }};
}


fn register_device_inner(
    controller_id: usize,
    slot_id: u32,
    port: u8,
    kind: DeviceKind,
    resources: Option<DeviceResources>,
) {
    let mut guard = DEVICES[controller_id].lock();
    if let Some(existing) = guard.iter_mut().find(|d| d.slot_id == slot_id) {
        existing.kind = kind;
        existing.port = port;
        // If a device becomes claimed later, its resources become driver-owned.
        existing.resources = if kind == DeviceKind::Unknown {
            resources.or(existing.resources)
        } else {
            None
        };
        return;
    }
    if guard
        .push(DeviceEntry {
            slot_id,
            port,
            kind,
            resources: if kind == DeviceKind::Unknown { resources } else { None },
        })
        .is_err()
    {
        crate::log!("usb: device table full, dropping slot {}\n", slot_id);
    }
    // Signal that at least one device is enumerated so poll_task can start.
    ENUM_READY[controller_id].store(true, Ordering::Release);
    if kind != DeviceKind::Unknown {
        crate::log!(
            "usb: device claimed slot={} port={} kind={}\n",
            slot_id,
            port,
            kind.claimed_log_label()
        );
    }
}

fn register_device(controller_id: usize, slot_id: u32, port: u8, kind: DeviceKind) {
    register_device_inner(controller_id, slot_id, port, kind, None);
}

fn register_unclaimed_device(
    controller_id: usize,
    slot_id: u32,
    port: u8,
    resources: DeviceResources,
) {
    register_device_inner(controller_id, slot_id, port, DeviceKind::Unknown, Some(resources));
}

fn device_kind_for_slot(controller_id: usize, slot_id: u32) -> Option<DeviceKind> {
    DEVICES[controller_id]
        .lock()
        .iter()
        .find(|d| d.slot_id == slot_id)
        .map(|d| d.kind)
}

pub(crate) fn friendly_name_for_vidpid(vid: u16, pid: u16) -> Option<&'static str> {
    match (vid, pid) {
        // Devices commonly used in local QEMU setups.
        (0x303A, 0x1001) => Some("ESP USB Device"),
        (0x0951, 0x16A4) => Some("HyperX Device"),
        (0x1462, 0x7E03) => Some("MSI Mystic Light"),
        (0x07CF, 0x6803) => Some("USB MIDI Device"),
        (0x058F, 0x6387) => Some("USB Flash Drive"),

        // Common emulated HID IDs (best effort).
        (0x0627, 0x0001) => Some("QEMU USB Tablet"),
        (0x0627, 0x0002) => Some("QEMU USB Mouse"),
        (0x0627, 0x0005) => Some("QEMU USB Keyboard"),

        _ => None,
    }
}

fn any_hid_registered(controller_id: usize) -> bool {
    DEVICES[controller_id]
        .lock()
        .iter()
        .any(|d| d.kind == DeviceKind::Hid)
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn poll_task(info: xhci::XhcInfo) {
    async move {
        let ctx = unsafe { XhciContext::new(info) };
        let controller_id = ctx.controller_id;
        let mut heartbeat: u32 = 0;
        let mut idle_timeouts: u32 = 0;

        // Predicate used both for the blocking wait and for draining any buffered backlog.
        let mut want_evt = |evt: &Trb| -> bool {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
                return false;
            }
            let ep_target = (evt.d3 >> 16) & 0x1F;
            if ep_target == 0 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) as u32;
            match device_kind_for_slot(controller_id, evt_slot) {
                Some(DeviceKind::Hid)
                | Some(DeviceKind::Cdc)
                | Some(DeviceKind::Uac)
                | Some(DeviceKind::Midi) => true,
                Some(DeviceKind::Mass) => false,
                Some(DeviceKind::Hub) => hub::interrupt_runtime_exists(controller_id, evt_slot),
                Some(_) => false,
                None => {
                    cdc_acm::runtime_exists(controller_id, evt_slot)
                        || midi::runtime_exists(controller_id, evt_slot)
                        || hub::interrupt_runtime_exists(controller_id, evt_slot)
                }
            }
        };

        loop {
            if !ENUM_READY[controller_id].load(Ordering::Acquire) {
                Timer::after(EmbassyDuration::from_millis(5)).await;
                continue;
            }

            heartbeat = heartbeat.wrapping_add(1);

            // Use a short delay here; high-rate interrupt endpoints can generate 1000Hz events.
            // If we only consume at 200Hz (5ms), we build a backlog that drains long after input.
            let evt_opt = xhci::wait_for_event(&ctx, &mut want_evt, 400, EmbassyDuration::from_millis(1)).await;

            let Some(evt) = evt_opt else {
                idle_timeouts = idle_timeouts.wrapping_add(1);
                continue;
            };

            idle_timeouts = 0;

            // Process the event we waited for, then drain a small batch of additional queued
            // transfer events so we keep up with bursty HID.
            let mut to_process: heapless::Vec<Trb, 32> = heapless::Vec::new();
            let _ = to_process.push(evt);
            while to_process.len() < to_process.capacity() {
                let Some(next) = xhci::try_take_buffered_event(controller_id, &mut want_evt) else {
                    break;
                };
                let _ = to_process.push(next);
            }

            for evt in to_process.into_iter() {
                let evt_slot = (evt.d3 >> 24) as u32;

                match device_kind_for_slot(controller_id, evt_slot) {
                Some(DeviceKind::Hid) => {
                    if !any_hid_registered(controller_id) {
                        continue;
                    }

                    let ep_target = (evt.d3 >> 16) & 0x1F;
                    if ep_target == 0 {
                        continue;
                    }

                    let handled = hid::with_runtime_mut_by_slot_and_target(
                        controller_id,
                        evt_slot,
                        ep_target,
                        |runtime| {
                            let completion = (evt.d2 >> 24) & 0xFF;
                            let residual = evt.d2 & 0x00FF_FFFF;
                            let data_len =
                                runtime.report_len.min(runtime.ep.max_packet as u32) as usize;
                            let requested = data_len as u32;
                            let transferred = requested.saturating_sub(residual).min(requested);
                            let data_full = unsafe {
                                core::slice::from_raw_parts(runtime.report_virt, data_len)
                            };
                            let data = &data_full[..(transferred as usize)];

                            // CC=1 Success and CC=13 Short Packet are both normal for interrupt IN.
                            // Other completions can report stale/undefined residual/length; don't parse.
                            if completion == 1 || completion == 13 {
                                hid::handle_report(runtime, completion, data, residual);
                            } else if USB_LOG_VERBOSE {
                                crate::log!(
                                    "usb: hid int slot={} ep={} cc={} residual={}\n",
                                    runtime.slot_id,
                                    runtime.ep_target,
                                    completion,
                                    residual
                                );
                            }

                            let normal = Trb {
                                d0: lo(runtime.report_phys),
                                d1: hi(runtime.report_phys),
                                d2: runtime.report_len,
                                d3: trb_type(1) | (1 << 5),
                            };

                            let before = runtime.ep_ring.state_snapshot();
                            if !runtime.ep_ring.push(normal) {
                                crate::log!(
                                    "usb: failed to requeue HID interrupt IN transfer\n"
                                );
                            } else {
                                let after = runtime.ep_ring.state_snapshot();
                                if hid::hid_log_allow_chatter(runtime.seq) {
                                    crate::log!(
                                        "[hid] requeue slot={} target={} ring_before=({}, {}) ring_after=({}, {})\n",
                                        runtime.slot_id,
                                        runtime.ep_target,
                                        before.0,
                                        before.1 as u8,
                                        after.0,
                                        after.1 as u8
                                    );
                                }
                                unsafe {
                                    write_volatile(
                                        ctx.doorbell.add(runtime.slot_id as usize),
                                        runtime.ep_target
                                    );
                                }
                                if hid::hid_log_allow_chatter(runtime.seq) {
                                    crate::log!(
                                        "[hid] doorbell slot={} target={} rung\n",
                                        runtime.slot_id,
                                        runtime.ep_target
                                    );
                                }
                            }
                            true
                        },
                    )
                    .unwrap_or(false);

                    if !handled {
                        usbv!(
                            "usb: ignoring transfer event slot={} (no HID runtime)\n",
                            evt_slot
                        );
                    }
                },
                Some(DeviceKind::Mass) => {
                    // Mass storage transfers are driven by the mass driver; nothing to do here yet.
                },
                Some(DeviceKind::Cdc) => {
                    if !cdc_acm::handle_transfer_event(controller_id, &evt) {
                        usbv!(
                            "usb: ignoring transfer event slot={} (no CDC runtime)\n",
                            evt_slot
                        );
                    }
                },
                Some(DeviceKind::Uac) => {
                    if !uac::queue_transfer_event_if_owned(controller_id, &evt) {
                        let _ = uac::handle_transfer_event(controller_id, &evt);
                    }
                },
                Some(DeviceKind::Midi) => {
                    let _ = midi::handle_transfer_event(controller_id, &evt);
                },
                Some(DeviceKind::Hub) => {
                    if !hub::handle_transfer_event(controller_id, &evt) {
                        usbv!(
                            "usb: ignoring transfer event slot={} (no HUB runtime)\n",
                            evt_slot
                        );
                    }
                },
                Some(DeviceKind::Printer) => {},
                Some(DeviceKind::Pen) => {},
                Some(DeviceKind::Leds) => {
                    // LED controller endpoints are configured, but no periodic transfers are driven yet.
                },
                Some(DeviceKind::Unknown) => {
                    // Unclaimed devices keep a slot assigned so we don't thrash.
                    // No transfers should complete for them because no endpoints are configured.
                },
                None => {
                    // A device may complete transfers during attach (while the enum path is still
                    // running) before `register_device()` marks the slot kind. Handle CDC events
                    // opportunistically so TX/RX doesn't stall.
                    let _ = cdc_acm::handle_transfer_event(controller_id, &evt);
                    let _ = midi::handle_transfer_event(controller_id, &evt);
                },
            }
        }
    }
    }.await;
}
