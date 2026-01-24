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
pub mod truekey;
pub mod xhci;
mod scout;
mod enumeration;
mod control;
mod attach;

pub use scout::{usb_scout, usb_scout_service};
pub(crate) use self::control::{control_in, control_out};
pub(crate) use self::enumeration::{disable_slot, enable_slot, enumerate_port, enumerate_with_params};

use self::xhci::{hi, lo, trb_type, Trb, TrbRing, XhciContext, MAX_XHCI_CONTROLLERS};
use core::ptr::write_volatile;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;
use self::hub::{LOG_PORTS_MAX, MAX_DEVICES};
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DeviceKind {
    Hid,
    Hub,
    Mass,
    Printer,
    Pen,
    Cdc,
    Uac,
}
#[derive(Copy, Clone, Debug)]
struct DeviceEntry {
    slot_id: u32,
    port: u8,
    kind: DeviceKind,
}

static DEVICES: [Mutex<Vec<DeviceEntry, MAX_DEVICES>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(Vec::new()) }; MAX_XHCI_CONTROLLERS];
static ENUM_READY: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static NOT_CLAIMED_KEY: [[AtomicU32; LOG_PORTS_MAX]; MAX_XHCI_CONTROLLERS] =
    [const { [const { AtomicU32::new(0) }; LOG_PORTS_MAX] }; MAX_XHCI_CONTROLLERS];
static NOT_CLAIMED_COUNT: [[AtomicU32; LOG_PORTS_MAX]; MAX_XHCI_CONTROLLERS] =
    [const { [const { AtomicU32::new(0) }; LOG_PORTS_MAX] }; MAX_XHCI_CONTROLLERS];

struct UsbControllerState {
    info: xhci::XhcInfo,
    ctx: XhciContext,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    dcbaa_phys: u64,
    dcbaa_virt: *mut u8,
    scratchpad_array_phys: u64,
    scratchpad_array_virt: *mut u8,
    scratchpad_count: u32,
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


fn register_device(controller_id: usize, slot_id: u32, port: u8, kind: DeviceKind) {
    let mut guard = DEVICES[controller_id].lock();
    if let Some(existing) = guard.iter_mut().find(|d| d.slot_id == slot_id) {
        existing.kind = kind;
        existing.port = port;
        return;
    }
    if guard
        .push(DeviceEntry {
            slot_id,
            port,
            kind,
        })
        .is_err()
    {
        crate::log!("usb: device table full, dropping slot {}\n", slot_id);
    }
    // Signal that at least one device is enumerated so poll_task can start.
    ENUM_READY[controller_id].store(true, Ordering::Release);
    crate::log!(
        "usb: device claimed slot={} port={} kind={:?}\n",
        slot_id,
        port,
        kind
    );
}

fn device_kind_for_slot(controller_id: usize, slot_id: u32) -> Option<DeviceKind> {
    DEVICES[controller_id]
        .lock()
        .iter()
        .find(|d| d.slot_id == slot_id)
        .map(|d| d.kind)
}

fn any_hid_registered(controller_id: usize) -> bool {
    DEVICES[controller_id]
        .lock()
        .iter()
        .any(|d| d.kind == DeviceKind::Hid)
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn poll_task(info: xhci::XhcInfo) {
    let ctx = unsafe { XhciContext::new(info) };
    let controller_id = ctx.controller_id;
    let mut heartbeat: u32 = 0;
    let mut idle_timeouts: u32 = 0;

    loop {
        if !ENUM_READY[controller_id].load(Ordering::Acquire) {
            Timer::after(EmbassyDuration::from_millis(5)).await;
            continue;
        }

        heartbeat = heartbeat.wrapping_add(1);

        let evt_opt = xhci::wait_for_event(
            &ctx,
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type != 32 {
                    return false;
                }
                let ep_target = (evt.d3 >> 16) & 0x1F;
                if ep_target == 0 {
                    return false;
                }
                let evt_slot = (evt.d3 >> 24) as u32;
                device_kind_for_slot(controller_id, evt_slot).is_some()
            },
            400,
            EmbassyDuration::from_millis(5),
        )
        .await;

        let Some(evt) = evt_opt else {
            idle_timeouts = idle_timeouts.wrapping_add(1);
            continue;
        };

        idle_timeouts = 0;

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
                        let data = unsafe {
                            core::slice::from_raw_parts(runtime.report_virt, data_len)
                        };
                        hid::handle_report(runtime, completion, data, residual);

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
                            if hid::HID_LOGS {
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
                            if hid::HID_LOGS {
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
            }
            Some(DeviceKind::Mass) => {
                // Mass storage transfers are driven by the mass driver; nothing to do here yet.
            }
            Some(DeviceKind::Cdc) => {
                if !cdc_acm::handle_transfer_event(controller_id, &evt) {
                    usbv!(
                        "usb: ignoring transfer event slot={} (no CDC runtime)\n",
                        evt_slot
                    );
                }
            }
            Some(DeviceKind::Uac) => {
                let _ = uac::handle_transfer_event(controller_id, &evt);
            }
            Some(DeviceKind::Hub) => {
                // Hub class driver not implemented yet.
            }
            Some(DeviceKind::Printer) => {}
            Some(DeviceKind::Pen) => {}
            None => {
                // A device may complete transfers during attach (while the enum path is still
                // running) before `register_device()` marks the slot kind. Handle CDC events
                // opportunistically so TX/RX doesn't stall.
                let _ = cdc_acm::handle_transfer_event(controller_id, &evt);
            }
        }
    }
}
