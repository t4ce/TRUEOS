use super::control;
use super::xhci::{
    self, EP_STATE_DISABLED, EP_TYPE_INT_IN, Trb, TrbRing, TrbRingState, XhciContext,
    context_index, endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits, ep_interval_bits,
    ep_max_esit_payload_lo_bits, ep_max_packet_bits, ep_state_bits, ep_type_bits, hi, lo, trb_type,
};
pub mod classreq;
pub mod descripto;
pub mod keyboard;
pub mod mouse;

use self::descripto as usbdesc;
use self::keyboard::KeyboardRing;
use self::mouse::MouseRing;
use crate::pci::dma;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::Duration as EmbassyDuration;
use embassy_time_driver::TICK_HZ;
use heapless::Vec;
use spin::Mutex;

const MAX_REPORT_DESC: usize = 512;
const HID_TABLET_RING_CAP: usize = 512;
const HID_CURSOR_EVENT_CAP: usize = 256;
const HID_TABLET_SAMPLE_PERIOD_MS: u32 = 10;
const HID_TABLET_ABS_MAX: u32 = 0x7FFF;
const HID_MOUSE_NORM_PER_DELTA: f64 = 1.0 / 2000.0;
const HID_KIND_VIRTUAL: u8 = 0;
pub(crate) const HID_DEBUG_REPORT_LOGS: bool = true;

pub use self::keyboard::TrueosHidKeyboardSample;
pub use self::mouse::TrueosHidMouseSample;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidTabletSample {
    pub t_ms: u32,
    pub seq: u32,
    pub slot_id: u32,
    pub buttons: u8,
    pub x: u16,
    pub y: u16,
    pub flags: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidCursorEvent {
    pub t_ms: u32,
    pub seq: u32,
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u8,
    pub reserved0: u8,
    pub reserved1: u16,
    pub buttons_down: u32,
    pub wheel: i16,
    pub reserved2: u16,
    pub x: f64,
    pub y: f64,
    pub flags: u32,
}

const ZERO_CURSOR_EVENT: TrueosHidCursorEvent = TrueosHidCursorEvent {
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

const ZERO_TABLET_SAMPLE: TrueosHidTabletSample = TrueosHidTabletSample {
    t_ms: 0,
    seq: 0,
    slot_id: 0,
    buttons: 0,
    x: 0,
    y: 0,
    flags: 0,
};

#[derive(Copy, Clone, Debug)]
struct TabletRing {
    buf: [TrueosHidTabletSample; HID_TABLET_RING_CAP],
    r: u32,
    w: u32,
    len: u32,
    dropped: u32,
}

impl TabletRing {
    fn new() -> Self {
        Self {
            buf: [ZERO_TABLET_SAMPLE; HID_TABLET_RING_CAP],
            r: 0,
            w: 0,
            len: 0,
            dropped: 0,
        }
    }

    #[inline]
    fn push(&mut self, s: TrueosHidTabletSample) {
        let cap = HID_TABLET_RING_CAP as u32;
        if cap == 0 {
            return;
        }

        if self.len == cap {
            self.r = (self.r + 1) % cap;
            self.dropped = self.dropped.wrapping_add(1);
        } else {
            self.len += 1;
        }

        self.buf[self.w as usize] = s;
        self.w = (self.w + 1) % cap;
    }

    #[inline]
    #[allow(dead_code)]
    fn pop(&mut self) -> Option<TrueosHidTabletSample> {
        if self.len == 0 {
            return None;
        }
        let cap = HID_TABLET_RING_CAP as u32;
        let s = self.buf[self.r as usize];
        self.r = (self.r + 1) % cap;
        self.len -= 1;
        Some(s)
    }
}

#[inline]
fn clamp01(v: f64) -> f64 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

pub fn mouse_cursor_snapshot() -> heapless::Vec<(f64, f64), MAX_HID_DEVICES> {
    let snapshots = crate::v::cursor::mouse_cursor_snapshot();
    let mut out: heapless::Vec<(f64, f64), MAX_HID_DEVICES> = heapless::Vec::new();
    for sample in snapshots {
        let _ = out.push(sample);
    }
    out
}

pub fn mouse_cursor_snapshot_with_buttons() -> heapless::Vec<(f64, f64, u32), MAX_HID_DEVICES> {
    let snapshots = crate::v::cursor::mouse_cursor_snapshot_with_buttons();
    let mut out: heapless::Vec<(f64, f64, u32), MAX_HID_DEVICES> = heapless::Vec::new();
    for sample in snapshots {
        let _ = out.push(sample);
    }
    out
}

pub fn tablet_cursor_snapshot() -> heapless::Vec<(f64, f64), MAX_HID_DEVICES> {
    let snapshots = crate::v::cursor::tablet_cursor_snapshot();
    let mut out: heapless::Vec<(f64, f64), MAX_HID_DEVICES> = heapless::Vec::new();
    for sample in snapshots {
        let _ = out.push(sample);
    }
    out
}

#[inline]
fn hid_uptime_ms() -> u64 {
    let ticks = embassy_time_driver::now() as u128;
    let hz = TICK_HZ as u128;
    if hz == 0 {
        0
    } else {
        ((ticks * 1000u128) / hz) as u64
    }
}

pub struct HidEpInfo {
    pub configuration: u8,
    pub interface: u8,
    pub subclass: u8,
    pub address: u8,
    pub max_packet: u16,
    #[allow(dead_code)]
    pub interval: u8,
    pub protocol: u8,
    pub report_desc_len: u16,
}

pub struct HidRuntime {
    pub controller_id: usize,
    pub ep: HidEpInfo,

    #[allow(dead_code)]
    pub report_desc: Vec<u8, MAX_REPORT_DESC>,
    pub report_phys: u64,
    pub report_virt: *mut u8,
    pub report_len: u32,
    pub hid_kind: u8,
    pub slot_id: u32,
    pub ep0_state: TrbRingState,
    pub ep_target: u32,
    pub ep_ring: TrbRing,
    pub seq: u64,
    pub last_nonzero_seq: u64,

    keyboard_ring: KeyboardRing,
    keyboard_modifiers: u8,
    keyboard_keys: [u8; 6],
    keyboard_ascii: [u8; 6],

    mouse_ring: MouseRing,

    mouse_x: f64,
    mouse_y: f64,
    mouse_buttons_down: u32,

    tablet_ring: TabletRing,
    tablet_x: f64,
    tablet_y: f64,
    tablet_last_sample_ms: u32,
}

unsafe impl Send for HidRuntime {}
unsafe impl Sync for HidRuntime {}

const MAX_HID_DEVICES: usize = 16;
const MAX_HID_INTERFACES: usize = 8;
static HID_RUNTIMES: Mutex<Vec<HidRuntime, MAX_HID_DEVICES>> = Mutex::new(Vec::new());

// Global cursor history ring: multi-producer (multiple active cursors/devices)
// and multi-consumer (each reader tracks its own sequence) without interference.
#[derive(Copy, Clone, Debug)]
struct CursorEventRing {
    buf: [TrueosHidCursorEvent; HID_CURSOR_EVENT_CAP],
    write_seq: u64,
}

impl CursorEventRing {
    const fn new() -> Self {
        Self {
            buf: [ZERO_CURSOR_EVENT; HID_CURSOR_EVENT_CAP],
            write_seq: 0,
        }
    }
}

static CURSOR_EVENT_RING: Mutex<CursorEventRing> = Mutex::new(CursorEventRing::new());
static CURSOR_EVENT_POP_SEQ: Mutex<u64> = Mutex::new(0);

#[inline]
fn sync_runtime_cursor_snapshot(runtime: &HidRuntime) {
    if runtime.hid_kind == 2 {
        crate::v::cursor::upsert_snapshot(
            runtime.controller_id as u32,
            runtime.slot_id,
            runtime.ep_target,
            runtime.hid_kind,
            runtime.mouse_x,
            runtime.mouse_y,
            runtime.mouse_buttons_down,
        );
    } else if runtime.hid_kind == 3 {
        crate::v::cursor::upsert_snapshot(
            runtime.controller_id as u32,
            runtime.slot_id,
            runtime.ep_target,
            runtime.hid_kind,
            runtime.tablet_x,
            runtime.tablet_y,
            0,
        );
    }
}

#[inline]
fn push_cursor_event(mut evt: TrueosHidCursorEvent) {
    let mut ring = CURSOR_EVENT_RING.lock();
    ring.write_seq = ring.write_seq.wrapping_add(1);
    let seq = ring.write_seq;
    evt.seq = seq as u32;
    let idx = ((seq - 1) as usize) % HID_CURSOR_EVENT_CAP;
    ring.buf[idx] = evt;
}

pub fn inject_virtual_cursor_event(
    slot_id: u32,
    x: f64,
    y: f64,
    buttons_down: u32,
    wheel: i16,
    flags: u32,
) {
    if slot_id == 0 {
        return;
    }

    let nx = clamp01(x);
    let ny = clamp01(y);
    crate::v::cursor::upsert_snapshot(0, slot_id, slot_id, HID_KIND_VIRTUAL, nx, ny, buttons_down);
    crate::usb::hut::upsert_mouse_state(
        0,
        slot_id,
        slot_id,
        nx,
        ny,
        buttons_down,
        crate::usb::hut::HidSourceKind::Ai,
        "ai",
        true,
    );
    push_cursor_event(TrueosHidCursorEvent {
        t_ms: hid_uptime_ms() as u32,
        seq: 0,
        controller_id: 0,
        slot_id,
        ep_target: slot_id,
        hid_kind: HID_KIND_VIRTUAL,
        reserved0: 0,
        reserved1: 0,
        buttons_down,
        wheel,
        reserved2: 0,
        x: nx,
        y: ny,
        flags,
    });
}

pub fn read_cursor_events_since(
    read_seq: u64,
    out: &mut [TrueosHidCursorEvent],
) -> (u64, u32, usize) {
    let ring = CURSOR_EVENT_RING.lock();
    if ring.write_seq == 0 || out.is_empty() {
        return (read_seq, 0, 0);
    }

    let cap = HID_CURSOR_EVENT_CAP as u64;
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
        let idx = ((seq - 1) as usize) % HID_CURSOR_EVENT_CAP;
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

pub fn pop_cursor_event() -> Option<TrueosHidCursorEvent> {
    let mut seq = CURSOR_EVENT_POP_SEQ.lock();
    let mut one = [ZERO_CURSOR_EVENT; 1];
    let (next_seq, _dropped, wrote) = read_cursor_events_since(*seq, &mut one);
    if wrote == 0 {
        return None;
    }
    *seq = next_seq;
    Some(one[0])
}

pub(crate) fn ep0_state_for_slot(controller_id: usize, slot_id: u32) -> Option<TrbRingState> {
    let guard = HID_RUNTIMES.lock();
    guard
        .iter()
        .find(|r| r.controller_id == controller_id && r.slot_id == slot_id)
        .map(|r| r.ep0_state)
}

pub(crate) fn set_ep0_state_for_slot(controller_id: usize, slot_id: u32, st: TrbRingState) {
    let mut guard = HID_RUNTIMES.lock();
    for r in guard.iter_mut() {
        if r.controller_id == controller_id && r.slot_id == slot_id {
            r.ep0_state = st;
        }
    }
}

pub fn register_runtime(runtime: HidRuntime) {
    let claimed_flags = match runtime.hid_kind {
        1 => crate::v::readiness::HID_KEYBOARD_CLAIMED,
        _ => 0,
    };
    if claimed_flags != 0 {
        crate::v::readiness::set(claimed_flags);
    }

    let mut guard = HID_RUNTIMES.lock();
    if let Some(existing) = guard.iter_mut().find(|r| {
        r.controller_id == runtime.controller_id
            && r.slot_id == runtime.slot_id
            && r.ep_target == runtime.ep_target
    }) {
        *existing = runtime;
        sync_runtime_cursor_snapshot(existing);
        match existing.hid_kind {
            1 => crate::usb::hut::upsert_keyboard_state(
                existing.controller_id as u32,
                existing.slot_id,
                existing.ep_target,
                0,
                [0; 6],
                [0; 6],
                crate::usb::hut::HidSourceKind::Human,
                "human",
                false,
            ),
            2 => crate::usb::hut::upsert_mouse_state(
                existing.controller_id as u32,
                existing.slot_id,
                existing.ep_target,
                existing.mouse_x,
                existing.mouse_y,
                existing.mouse_buttons_down,
                crate::usb::hut::HidSourceKind::Human,
                "human",
                false,
            ),
            _ => {}
        }
        if existing.hid_kind == 1 {
            crate::v::keyboard::upsert_snapshot(
                existing.controller_id as u32,
                existing.slot_id,
                existing.ep_target,
                existing.keyboard_modifiers,
                existing.keyboard_keys,
                existing.keyboard_ascii,
            );
        }
        return;
    }
    sync_runtime_cursor_snapshot(&runtime);
    match runtime.hid_kind {
        1 => crate::usb::hut::upsert_keyboard_state(
            runtime.controller_id as u32,
            runtime.slot_id,
            runtime.ep_target,
            0,
            [0; 6],
            [0; 6],
            crate::usb::hut::HidSourceKind::Human,
            "human",
            false,
        ),
        2 => crate::usb::hut::upsert_mouse_state(
            runtime.controller_id as u32,
            runtime.slot_id,
            runtime.ep_target,
            runtime.mouse_x,
            runtime.mouse_y,
            runtime.mouse_buttons_down,
            crate::usb::hut::HidSourceKind::Human,
            "human",
            false,
        ),
        _ => {}
    }
    if runtime.hid_kind == 1 {
        crate::v::keyboard::upsert_snapshot(
            runtime.controller_id as u32,
            runtime.slot_id,
            runtime.ep_target,
            runtime.keyboard_modifiers,
            runtime.keyboard_keys,
            runtime.keyboard_ascii,
        );
    }
    let _ = guard.push(runtime);
}

pub fn unregister_runtime(controller_id: usize, slot_id: u32) -> bool {
    let _ = crate::v::cursor::remove_snapshots(controller_id as u32, slot_id);
    let _ = crate::v::keyboard::remove_snapshots(controller_id as u32, slot_id);
    let _ = crate::usb::hut::remove_slot(controller_id as u32, slot_id);
    let mut guard = HID_RUNTIMES.lock();
    let mut removed = false;
    let mut idx = 0usize;
    while idx < guard.len() {
        if guard[idx].controller_id == controller_id && guard[idx].slot_id == slot_id {
            let _ = guard.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }
    removed
}

pub fn with_runtime_mut_by_slot_and_target<F, R>(
    controller_id: usize,
    slot_id: u32,
    ep_target: u32,
    f: F,
) -> Option<R>
where
    F: FnOnce(&mut HidRuntime) -> R,
{
    let mut guard = HID_RUNTIMES.lock();
    guard
        .iter_mut()
        .find(|r| {
            r.controller_id == controller_id && r.slot_id == slot_id && r.ep_target == ep_target
        })
        .map(f)
}

pub fn handle_report(runtime: &mut HidRuntime, completion: u32, data: &[u8], residual: u32) {
    runtime.seq = runtime.seq.wrapping_add(1);

    if runtime.hid_kind == 1 {
        let _ = completion;
        let _ = residual;
        keyboard::handle_report(runtime, data, hid_uptime_ms() as u32);
    } else if runtime.hid_kind == 2 {
        let _ = completion;
        let _ = residual;
        mouse::handle_report(runtime, data, hid_uptime_ms() as u32);
    } else if runtime.hid_kind == 3 {
        let _ = completion;
        let _ = residual;

        let now_ms = hid_uptime_ms() as u32;
        if now_ms.wrapping_sub(runtime.tablet_last_sample_ms) < HID_TABLET_SAMPLE_PERIOD_MS {
            return;
        }
        runtime.tablet_last_sample_ms = now_ms;

        if data.len() >= 5 {
            #[derive(Copy, Clone)]
            struct Cand {
                buttons: u8,
                x: u16,
                y: u16,
            }

            let mk = |buttons: u8, x: u16, y: u16| Cand { buttons, x, y };

            let mut best: Option<(Cand, u8)> = None;

            let score = |c: Cand| -> u8 {
                let mut s = 0u8;
                if (c.buttons & 0xE0) == 0 {
                    s = s.saturating_add(2);
                }
                if c.x != 0 || c.y != 0 {
                    s = s.saturating_add(3);
                }
                if (c.x as u32) <= HID_TABLET_ABS_MAX && (c.y as u32) <= HID_TABLET_ABS_MAX {
                    s = s.saturating_add(1);
                }
                s
            };

            let mut consider = |c: Cand| {
                let s = score(c);
                if best.map(|(_, bs)| s > bs).unwrap_or(true) {
                    best = Some((c, s));
                }
            };

            let c1 = mk(
                data[0],
                u16::from_le_bytes([data[1], data[2]]),
                u16::from_le_bytes([data[3], data[4]]),
            );
            consider(c1);

            if data.len() >= 6 {
                let c2 = mk(
                    data[1],
                    u16::from_le_bytes([data[2], data[3]]),
                    u16::from_le_bytes([data[4], data[5]]),
                );
                consider(c2);

                let c3 = mk(
                    data[5],
                    u16::from_le_bytes([data[1], data[2]]),
                    u16::from_le_bytes([data[3], data[4]]),
                );
                consider(c3);
            }

            let (cand, _cand_score) = best.unwrap_or((c1, 0));
            let buttons = cand.buttons;
            let x = cand.x;
            let y = cand.y;

            if x == 0
                && y == 0
                && runtime.seq <= 4
                && (runtime.tablet_x - 0.5).abs() < 0.0001
                && (runtime.tablet_y - 0.5).abs() < 0.0001
            {
                return;
            }

            let denom = if (x as u32) > HID_TABLET_ABS_MAX || (y as u32) > HID_TABLET_ABS_MAX {
                0xFFFFu32
            } else {
                HID_TABLET_ABS_MAX
            };
            let xf = (x as f64) / (denom as f64);
            let yf = (y as f64) / (denom as f64);
            runtime.tablet_x = clamp01(xf);
            runtime.tablet_y = clamp01(yf);

            runtime.tablet_ring.push(TrueosHidTabletSample {
                t_ms: now_ms,
                seq: runtime.seq as u32,
                slot_id: runtime.slot_id,
                buttons,
                x,
                y,
                flags: 0,
            });

            push_cursor_event(TrueosHidCursorEvent {
                t_ms: now_ms,
                seq: runtime.seq as u32,
                controller_id: runtime.controller_id as u32,
                slot_id: runtime.slot_id,
                ep_target: runtime.ep_target,
                hid_kind: runtime.hid_kind,
                reserved0: 0,
                reserved1: 0,
                buttons_down: u32::from(buttons),
                wheel: 0,
                reserved2: 0,
                x: runtime.tablet_x,
                y: runtime.tablet_y,
                flags: (1 << 0) | (1 << 2),
            });
            sync_runtime_cursor_snapshot(runtime);
        }
    } else {
        let _ = completion;
        let _ = residual;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_mouse_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: *mut TrueosHidMouseSample,
    out_cap: u32,
    out_dropped: *mut u32,
) -> u32 {
    if out.is_null() {
        return 0;
    }

    let mut wrote = 0u32;
    let mut dropped = 0u32;

    let controller_id = controller_id as usize;
    if controller_id >= super::xhci::MAX_XHCI_CONTROLLERS {
        return 0;
    }

    let _ = with_runtime_mut_by_slot_and_target(controller_id, slot_id, ep_target, |rt| {
        dropped = rt.mouse_ring.dropped;
        rt.mouse_ring.dropped = 0;

        while wrote < out_cap {
            let Some(s) = rt.mouse_ring.pop() else {
                break;
            };
            core::ptr::write(out.add(wrote as usize), s);
            wrote += 1;
        }
    });

    if !out_dropped.is_null() {
        *out_dropped = dropped;
    }
    wrote
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_keyboard_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: *mut TrueosHidKeyboardSample,
    out_cap: u32,
    out_dropped: *mut u32,
) -> u32 {
    if out.is_null() {
        return 0;
    }

    let mut wrote = 0u32;
    let mut dropped = 0u32;

    let controller_id = controller_id as usize;
    if controller_id >= super::xhci::MAX_XHCI_CONTROLLERS {
        return 0;
    }

    let _ = with_runtime_mut_by_slot_and_target(controller_id, slot_id, ep_target, |rt| {
        dropped = rt.keyboard_ring.dropped;
        rt.keyboard_ring.dropped = 0;

        while wrote < out_cap {
            let Some(s) = rt.keyboard_ring.pop() else {
                break;
            };
            core::ptr::write(out.add(wrote as usize), s);
            wrote += 1;
        }
    });

    if !out_dropped.is_null() {
        *out_dropped = dropped;
    }
    wrote
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_mouse_pos(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out_x: *mut f64,
    out_y: *mut f64,
) -> i32 {
    if out_x.is_null() || out_y.is_null() {
        return -1;
    }

    let controller_id = controller_id as usize;
    if controller_id >= super::xhci::MAX_XHCI_CONTROLLERS {
        return -2;
    }

    let mut ok = false;
    let _ = with_runtime_mut_by_slot_and_target(controller_id, slot_id, ep_target, |rt| {
        *out_x = rt.mouse_x;
        *out_y = rt.mouse_y;
        ok = true;
    });

    if ok { 0 } else { -3 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_tablet_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: *mut TrueosHidTabletSample,
    out_cap: u32,
    out_dropped: *mut u32,
) -> u32 {
    if out.is_null() {
        return 0;
    }

    let mut wrote = 0u32;
    let mut dropped = 0u32;

    let controller_id = controller_id as usize;
    if controller_id >= super::xhci::MAX_XHCI_CONTROLLERS {
        return 0;
    }

    let _ = with_runtime_mut_by_slot_and_target(controller_id, slot_id, ep_target, |rt| {
        dropped = rt.tablet_ring.dropped;
        rt.tablet_ring.dropped = 0;

        while wrote < out_cap {
            let Some(s) = rt.tablet_ring.pop() else {
                break;
            };
            core::ptr::write(out.add(wrote as usize), s);
            wrote += 1;
        }
    });

    if !out_dropped.is_null() {
        *out_dropped = dropped;
    }
    wrote
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_tablet_pos(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out_x: *mut f64,
    out_y: *mut f64,
) -> i32 {
    if out_x.is_null() || out_y.is_null() {
        return -1;
    }

    let controller_id = controller_id as usize;
    if controller_id >= super::xhci::MAX_XHCI_CONTROLLERS {
        return -2;
    }

    let mut ok = false;
    let _ = with_runtime_mut_by_slot_and_target(controller_id, slot_id, ep_target, |rt| {
        *out_x = rt.tablet_x;
        *out_y = rt.tablet_y;
        ok = true;
    });

    if ok { 0 } else { -3 }
}

pub fn parse_hid_interrupt_in_endpoints(cfg: &[u8]) -> Vec<HidEpInfo, MAX_HID_INTERFACES> {
    let mut report_len_by_iface: [u16; 256] = [0; 256];

    let mut config_value = 1u8;
    let mut current_iface: Option<u8> = None;
    let mut current_alt: u8 = 0;
    let mut current_class: u8 = 0;
    let mut current_subclass: u8 = 0;
    let mut current_proto: u8 = 0;

    let mut endpoints: Vec<HidEpInfo, MAX_HID_INTERFACES> = Vec::new();

    for raw in usbdesc::DescriptorIter::new(cfg) {
        match usbdesc::parse_any_descriptor(raw) {
            usbdesc::ParsedDescriptor::Configuration(cd) => {
                config_value = cd.configuration_value;
            }
            usbdesc::ParsedDescriptor::Interface(id) => {
                current_iface = Some(id.interface_number);
                current_alt = id.alternate_setting;
                current_class = id.interface_class;
                current_subclass = id.interface_subclass;
                current_proto = id.interface_protocol;
            }
            usbdesc::ParsedDescriptor::Hid(hd) => {
                if let Some(iface) = current_iface
                    && current_alt == 0
                    && let Some(len) = hd.report_desc_len
                {
                    report_len_by_iface[iface as usize] = len;
                }
            }
            usbdesc::ParsedDescriptor::Endpoint(ed) => {
                let Some(iface) = current_iface else {
                    continue;
                };
                if current_alt != 0 || current_class != 0x03 {
                    continue;
                }
                let ep_addr = ed.endpoint_address;
                let attrs = ed.attributes;
                if (attrs & 0x3) != 0x3 || (ep_addr & 0x80) == 0 {
                    continue;
                }
                if endpoints
                    .iter()
                    .any(|e| e.interface == iface && e.address == ep_addr)
                {
                    continue;
                }
                let _ = endpoints.push(HidEpInfo {
                    configuration: config_value,
                    interface: iface,
                    subclass: current_subclass,
                    address: ep_addr,
                    max_packet: ed.max_packet_size,
                    interval: ed.interval,
                    protocol: current_proto,
                    report_desc_len: report_len_by_iface[iface as usize],
                });
            }
            _ => {}
        }
    }

    endpoints
}

pub async fn attach_hid_devices(params: BootAttachParams<'_>) -> Result<usize, ()> {
    let BootAttachParams {
        ctx,
        cmd_ring,
        ep0_ring,
        slot_id,
        cfg,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
    } = params;

    if cfg.is_empty() {
        return Err(());
    }

    let endpoints = parse_hid_interrupt_in_endpoints(cfg);
    if endpoints.is_empty() {
        return Err(());
    }

    let config_value = endpoints.first().map(|e| e.configuration).unwrap_or(1);

    let setup_cfg = Trb {
        d0: ((9u32) << 8) | ((config_value as u32) << 16),
        d1: 0,
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    };
    let status_cfg = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5) | (1 << 16),
    };
    if !ep0_ring.push(setup_cfg) || !ep0_ring.push(status_cfg) {
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };
    let Some(set_cfg_evt) = xhci::wait_for_event(
        ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            evt_slot == slot_id
        },
        400,
        EmbassyDuration::from_millis(5),
    )
    .await
    else {
        return Err(());
    };

    let completion = (set_cfg_evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        return Err(());
    }

    let mut attached = 0usize;
    for ep in endpoints.into_iter() {
        let want_desc_len = ep.report_desc_len as usize;
        if want_desc_len == 0 {
            continue;
        }
        let report_desc = match fetch_report_descriptor(
            ctx,
            &mut *ep0_ring,
            slot_id,
            ep.interface,
            want_desc_len,
        )
        .await
        {
            Ok(desc) => {
                if desc.is_empty() {
                    continue;
                }
                desc
            }
            Err(_err) => {
                continue;
            }
        };

        let hid_kind = usbdesc::hid_kind_from_report_desc(ep.subclass, ep.protocol, &report_desc);
        if hid_kind == 0 {
            continue;
        }

        if ep.subclass == 0x01 && (ep.protocol == 0x01 || ep.protocol == 0x02) {
            let _ = classreq::set_protocol(ctx, &mut *ep0_ring, slot_id, ep.interface, 0).await;
        }

        let (ep_ring_phys, ep_ring_virt) = match dma::alloc(32 * size_of::<Trb>(), 64) {
            Some(pair) => pair,
            None => {
                continue;
            }
        };
        unsafe { write_bytes(ep_ring_virt, 0, 32 * size_of::<Trb>()) };
        let mut ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, 32) };

        let (input_cfg_phys, input_cfg_virt) = match dma::alloc(4096, 64) {
            Some(pair) => pair,
            None => {
                continue;
            }
        };
        unsafe { write_bytes(input_cfg_virt, 0, 4096) };

        let ep_target = endpoint_target(ep.address);
        let ep_ctx_index = context_index(ep.address);
        let ep_add_bit = ep_ctx_index - 1;

        unsafe {
            let add_flags_ptr = input_cfg_virt as *mut u32;
            write_volatile(add_flags_ptr.add(1), (1 << 0) | (1 << ep_add_bit));

            let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
            let ep_ctx_off: usize = ctx_stride_bytes * (ep_ctx_index as usize);
            let ep_ctx = input_cfg_virt.add(ep_ctx_off) as *mut u32;

            let dev_slot_ctx = dev_ctx_virt as *const u32;
            for i in 0..ctx_stride_words {
                write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
            }

            let mut dw0 = read_volatile(slot_ctx.add(0));
            dw0 = (dw0 & !(0x1F << 27)) | (ep_add_bit << 27);
            write_volatile(slot_ctx.add(0), dw0);

            let mut dw1 = read_volatile(slot_ctx.add(1));
            dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
            write_volatile(slot_ctx.add(1), dw1);

            let mps = (ep.max_packet as u32) & 0x7FF;
            let interval = if speed_code == 3 { 3 } else { 1 };

            write_volatile(
                ep_ctx.add(0),
                ep_state_bits(EP_STATE_DISABLED) | ep_interval_bits(interval),
            );
            let mut ep_cfg = ep_cerr_bits(3);
            ep_cfg |= ep_type_bits(EP_TYPE_INT_IN);
            ep_cfg |= ep_max_packet_bits(mps);
            write_volatile(ep_ctx.add(1), ep_cfg);
            let dq = ep_ring.dequeue_ptr();
            write_volatile(ep_ctx.add(2), lo(dq));
            write_volatile(ep_ctx.add(3), hi(dq));
            write_volatile(
                ep_ctx.add(4),
                ep_avg_trb_len_bits(mps) | ep_max_esit_payload_lo_bits(mps),
            );
        }

        let cfg_ep_cmd = Trb {
            d0: lo(input_cfg_phys),
            d1: hi(input_cfg_phys),
            d2: 0,
            d3: trb_type(12) | (slot_id << 24),
        };
        if xhci::submit_cmd_and_wait(
            ctx,
            cmd_ring,
            cfg_ep_cmd,
            Some(slot_id),
            "hid-config-ep",
            400,
            EmbassyDuration::from_millis(5),
        )
        .await
        .is_err()
        {
            continue;
        }

        let report_bytes = core::cmp::max(usize::from(ep.max_packet), 8);
        let (rep_phys, rep_virt) = match dma::alloc(report_bytes, 64) {
            Some(pair) => pair,
            None => {
                continue;
            }
        };
        unsafe { write_bytes(rep_virt, 0, report_bytes) };

        let report_len = ep.max_packet as u32;

        let normal = Trb {
            d0: lo(rep_phys),
            d1: hi(rep_phys),
            d2: report_len,
            d3: trb_type(1) | (1 << 5),
        };
        if !ep_ring.push(normal) {
            continue;
        }
        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target) };

        register_runtime(HidRuntime {
            controller_id: ctx.controller_id,
            ep,
            report_desc,
            report_phys: rep_phys,
            report_virt: rep_virt,
            report_len,
            hid_kind,
            slot_id,
            ep0_state: ep0_ring.snapshot(),
            ep_target,
            ep_ring,
            seq: 0,
            last_nonzero_seq: 0,

            keyboard_ring: KeyboardRing::new(),
            keyboard_modifiers: 0,
            keyboard_keys: [0; 6],
            keyboard_ascii: [0; 6],

            mouse_ring: MouseRing::new(),
            mouse_x: 0.5,
            mouse_y: 0.5,
            mouse_buttons_down: 0,

            tablet_ring: TabletRing::new(),
            tablet_x: 0.5,
            tablet_y: 0.5,
            tablet_last_sample_ms: 0,
        });

        attached += 1;
    }

    if attached > 0 { Ok(attached) } else { Err(()) }
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum FetchReportError {
    Alloc,
    RingOverflow,
    Timeout,
    Completion(#[allow(dead_code)] u8),
}

async fn fetch_control_in(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup_d0: u32,
    setup_d1: u32,
    len: usize,
) -> Result<Vec<u8, MAX_REPORT_DESC>, FetchReportError> {
    let want_len = core::cmp::min(len, MAX_REPORT_DESC);
    if want_len == 0 {
        return Ok(Vec::new());
    }

    let (phys, virt) = dma::alloc(want_len, 64).ok_or(FetchReportError::Alloc)?;
    unsafe { write_bytes(virt, 0, want_len) };

    let w_length = want_len as u16;
    let setup_d1 = (setup_d1 & 0xFFFF) | ((w_length as u32) << 16);
    let setup = Trb {
        d0: setup_d0,
        d1: setup_d1,
        d2: 8 | (2u32 << 16),
        d3: trb_type(2) | (1 << 6),
    };

    let transferred = match control::control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        phys,
        w_length,
        "hid-ctrl-in",
        800,
    )
    .await
    {
        Ok((_, xfer)) => xfer as usize,
        Err(()) => {
            dma::dealloc(virt, want_len);
            return Err(FetchReportError::Timeout);
        }
    };

    let mut out = Vec::<u8, MAX_REPORT_DESC>::new();
    let data_slice = unsafe { core::slice::from_raw_parts(virt, want_len) };
    let _ = out.extend_from_slice(&data_slice[..core::cmp::min(transferred, want_len)]);
    dma::dealloc(virt, want_len);
    Ok(out)
}
pub(crate) async fn fetch_report_descriptor(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    len: usize,
) -> Result<Vec<u8, MAX_REPORT_DESC>, FetchReportError> {
    fetch_hid_descriptor(ctx, ep0_ring, slot_id, iface, 0x22, len).await
}

pub(crate) async fn fetch_hid_descriptor(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    desc_type: u8,
    len: usize,
) -> Result<Vec<u8, MAX_REPORT_DESC>, FetchReportError> {
    let w_value = (desc_type as u32) << 8;
    let setup_d0 = (0x81u32) | ((0x06u32) << 8) | (w_value << 16);
    let setup_d1 = iface as u32;
    fetch_control_in(ctx, ep0_ring, slot_id, setup_d0, setup_d1, len).await
}

pub(crate) fn log_report_descriptor(slot_id: u32, iface: u8, desc: &[u8]) {
    let _ = slot_id;
    let _ = iface;
    let _ = desc;
}

pub(crate) fn hid_log_allow_chatter(seq: u64) -> bool {
    let _ = seq;
    false
}

pub struct BootAttachParams<'a> {
    pub ctx: &'a XhciContext,
    pub cmd_ring: &'a mut TrbRing,
    pub ep0_ring: &'a mut TrbRing,
    pub slot_id: u32,
    pub cfg: &'a [u8],
    pub dev_ctx_virt: *mut u8,
    pub ctx_stride_bytes: usize,
    pub ctx_stride_words: usize,
    pub speed_code: u32,
    pub target_port: u8,
}
