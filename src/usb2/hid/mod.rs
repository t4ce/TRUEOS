extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;

pub mod boot;
pub mod hut;
pub mod input;
pub mod keyboard;
pub mod leds;
pub mod mediacontrol;
pub mod mouse;
pub mod tablet;

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

pub use self::keyboard::TrueosHidKeyboardSample;
pub use self::mouse::TrueosHidMouseSample;
pub use v::vinput::TrueosHidCursorEvent;

pub(crate) use crate::logflag::HID_DEBUG_REPORT_LOGS;

const HID_MOUSE_NORM_PER_DELTA: f64 = 1.0 / 1024.0;
const HID_KIND_KEYBOARD: u8 = 1;
const HID_KIND_MOUSE: u8 = 2;
const HID_KIND_VIRTUAL_CURSOR: u8 = 0;
const CURSOR_EVENT_RING_CAP: usize = 2048;

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

struct CursorEventRing {
    buf: [TrueosHidCursorEvent; CURSOR_EVENT_RING_CAP],
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

pub(crate) struct HidRuntime {
    pub(crate) controller_id: u32,
    pub(crate) slot_id: u32,
    pub(crate) ep_target: u32,
    pub(crate) hid_kind: u8,
    pub(crate) seq: u64,
    pub(crate) last_nonzero_seq: u64,
    pub(crate) mouse_x: f64,
    pub(crate) mouse_y: f64,
    pub(crate) mouse_buttons_down: u32,
    pub(crate) keyboard_modifiers: u8,
    pub(crate) keyboard_keys: [u8; 6],
    pub(crate) keyboard_ascii: [u8; 6],
    pub(crate) keyboard_ring: keyboard::KeyboardRing,
    pub(crate) mouse_ring: mouse::MouseRing,
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
    if hid_kind == HID_KIND_MOUSE {
        let runtime = &runtimes[idx];
        sync_runtime_cursor_snapshot(runtime);
        self::hut::upsert_mouse_state(
            runtime.controller_id,
            runtime.slot_id,
            runtime.ep_target,
            runtime.mouse_x,
            runtime.mouse_y,
            runtime.mouse_buttons_down,
            self::hut::HidSourceKind::Human,
            "human",
            false,
        );
    }
    &mut runtimes[idx]
}

#[inline]
fn push_cursor_event(event: TrueosHidCursorEvent) {
    let mut ring = CURSOR_EVENT_RING.lock();
    ring.write_seq = ring.write_seq.wrapping_add(1);
    let idx = ((ring.write_seq - 1) as usize) % CURSOR_EVENT_RING_CAP;
    ring.buf[idx] = event;
}

#[inline]
fn sync_runtime_cursor_snapshot(runtime: &HidRuntime) {
    crate::r::cursor::upsert_snapshot(
        runtime.controller_id,
        runtime.slot_id,
        runtime.ep_target,
        runtime.hid_kind,
        runtime.mouse_x,
        runtime.mouse_y,
        runtime.mouse_buttons_down,
    );
}

pub(crate) fn read_cursor_events_since(
    read_seq: u64,
    out: &mut [TrueosHidCursorEvent],
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

pub(crate) fn pop_cursor_event() -> Option<TrueosHidCursorEvent> {
    let mut read_seq = CURSOR_EVENT_POP_SEQ.lock();
    let mut out = [ZERO_CURSOR_EVENT; 1];
    let (next_seq, _dropped, wrote) = read_cursor_events_since(*read_seq, &mut out);
    if wrote == 0 {
        return None;
    }
    *read_seq = next_seq;
    Some(out[0])
}

pub(crate) fn inject_virtual_cursor_event(
    slot_id: u32,
    x: f64,
    y: f64,
    buttons_down: u32,
    wheel: i16,
    flags: u32,
) {
    let event = TrueosHidCursorEvent {
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
    crate::r::cursor::upsert_snapshot(0, slot_id, 0, HID_KIND_VIRTUAL_CURSOR, x, y, buttons_down);
    self::hut::upsert_mouse_state(
        0,
        slot_id,
        0,
        clamp01(x),
        clamp01(y),
        buttons_down,
        self::hut::HidSourceKind::Ai,
        "ai",
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
    let runtime =
        runtime_mut_or_insert(&mut runtimes, controller_id, slot_id, ep_target, HID_KIND_KEYBOARD);
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
    let runtime =
        runtime_mut_or_insert(&mut runtimes, controller_id, slot_id, ep_target, HID_KIND_MOUSE);
    runtime.seq = runtime.seq.wrapping_add(1);
    mouse::handle_report(runtime, data, now_ms);
}

pub(crate) fn remove_hid_slot(controller_id: u32, slot_id: u32) {
    let mut runtimes = HID_RUNTIMES.lock();
    runtimes
        .retain(|runtime| !(runtime.controller_id == controller_id && runtime.slot_id == slot_id));
    let _ = self::hut::remove_slot(controller_id, slot_id);
    let _ = crate::r::cursor::remove_snapshots(controller_id, slot_id);
    let _ = crate::r::keyboard::remove_snapshots(controller_id, slot_id);
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
