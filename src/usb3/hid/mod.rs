extern crate alloc;
use alloc::vec::Vec;
use spin::Mutex;
pub mod boot;
pub mod eyetracker;
pub mod hut;
pub mod input;
pub mod keyboard;
pub mod leds;
pub mod mediacontrol;
pub mod midi;
pub mod mouse;
pub mod tablet;
pub(crate) use crate::logflag::HID_DEBUG_REPORT_LOGS;
pub use v::vinput::TrueosHidCursorEvent;

const HID_MOUSE_NORM_PER_DELTA: f64 = 1.0 / 1024.0;
const HID_KIND_KEYBOARD: u8 = 1;
const HID_KIND_MOUSE: u8 = crate::r::cursor::HID_KIND_MOUSE;
const HID_KIND_TABLET: u8 = crate::r::cursor::HID_KIND_TABLET;
const HID_KIND_VIRTUAL_CURSOR: u8 = crate::r::cursor::HID_KIND_VIRTUAL_CURSOR;
const CURSOR_EVENT_RING_CAP: usize = crate::allcaps::input::HID_CURSOR_EVENT_RING_CAP;

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
    pub(crate) tablet_ring: tablet::TabletRing,
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
            tablet_ring: tablet::TabletRing::new(),
        }
    }
}

static HID_RUNTIMES: Mutex<Vec<HidRuntime>> = Mutex::new(Vec::new());
static CURSOR_EVENT_RING: Mutex<CursorEventRing> = Mutex::new(CursorEventRing::new());
static CURSOR_EVENT_POP_SEQ: Mutex<u64> = Mutex::new(0);

pub(crate) const HID_UDP_CONTROLLER_ID: u32 = 0x5544_5048; // "UDPH"
const HID_UDP_SLOT_BASE: u32 = 0x5500_0000;

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
    } else if hid_kind == HID_KIND_TABLET {
        let runtime = &runtimes[idx];
        sync_runtime_cursor_snapshot(runtime);
        self::hut::upsert_tablet_state(
            runtime.controller_id,
            runtime.slot_id,
            runtime.ep_target,
            runtime.mouse_x,
            runtime.mouse_y,
            0,
            0,
            runtime.mouse_buttons_down,
            0,
            self::hut::HidSourceKind::Human,
            "tablet",
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

#[inline]
pub(crate) const fn hid_udp_slot_id(udp_device_id: u16) -> u32 {
    HID_UDP_SLOT_BASE | (udp_device_id as u32)
}

pub(crate) fn inject_udp_mouse_boot_report(
    udp_device_id: u16,
    buttons: u8,
    dx: i8,
    dy: i8,
    wheel: i8,
) {
    let slot_id = hid_udp_slot_id(udp_device_id);
    let report = [buttons, dx as u8, dy as u8, wheel as u8];
    handle_mouse_boot_report(HID_UDP_CONTROLLER_ID, slot_id, 0, &report);
    if let Some((x, y)) =
        crate::r::cursor::cursor_source_pos(HID_UDP_CONTROLLER_ID, slot_id, 0, HID_KIND_MOUSE)
    {
        self::hut::upsert_mouse_state(
            HID_UDP_CONTROLLER_ID,
            slot_id,
            0,
            x,
            y,
            buttons as u32,
            self::hut::HidSourceKind::Ai,
            "udp",
            true,
        );
    }
}

pub(crate) fn inject_udp_keyboard_boot_report(udp_device_id: u16, modifiers: u8, keys: [u8; 6]) {
    let slot_id = hid_udp_slot_id(udp_device_id);
    let report = [
        modifiers, 0, keys[0], keys[1], keys[2], keys[3], keys[4], keys[5],
    ];
    handle_keyboard_boot_report(HID_UDP_CONTROLLER_ID, slot_id, 0, &report);
    self::hut::upsert_keyboard_state(
        HID_UDP_CONTROLLER_ID,
        slot_id,
        0,
        modifiers,
        keys,
        self::keyboard::boot_ascii_for_keys(modifiers, keys),
        self::hut::HidSourceKind::Ai,
        "udp",
        true,
    );
}

pub(crate) fn inject_udp_tablet_absolute_event(
    udp_device_id: u16,
    x: f64,
    y: f64,
    buttons_down: u32,
    wheel: i16,
    flags: u32,
) {
    let slot_id = hid_udp_slot_id(udp_device_id);
    let x = clamp01(x);
    let y = clamp01(y);
    let x_raw = ((x * 65535.0) + 0.5).clamp(0.0, 65535.0) as u16;
    let y_raw = ((y * 65535.0) + 0.5).clamp(0.0, 65535.0) as u16;
    let q15_x = ((x * 32767.0) + 0.5).clamp(0.0, 32767.0) as u16;
    let q15_y = ((y * 32767.0) + 0.5).clamp(0.0, 32767.0) as u16;
    let buttons = buttons_down.min(u8::MAX as u32) as u8;
    let now_ms = now_ms_u32();

    let seq = {
        let mut runtimes = HID_RUNTIMES.lock();
        let runtime = runtime_mut_or_insert(
            &mut runtimes,
            HID_UDP_CONTROLLER_ID,
            slot_id,
            0,
            HID_KIND_TABLET,
        );
        runtime.seq = runtime.seq.wrapping_add(1);
        runtime.mouse_x = x;
        runtime.mouse_y = y;
        runtime.mouse_buttons_down = buttons_down;
        runtime
            .tablet_ring
            .push(self::tablet::TrueosHidTabletSample {
                t_ms: now_ms,
                seq: runtime.seq as u32,
                slot_id,
                buttons,
                report_id: 0,
                flags: flags.min(u8::MAX as u32) as u8,
                reserved0: 0,
                x_raw,
                y_raw,
                x_norm_q15: q15_x,
                y_norm_q15: q15_y,
            });
        runtime.seq as u32
    };

    push_cursor_event(TrueosHidCursorEvent {
        t_ms: now_ms,
        seq,
        controller_id: HID_UDP_CONTROLLER_ID,
        slot_id,
        ep_target: 0,
        hid_kind: HID_KIND_TABLET,
        reserved0: 0,
        reserved1: 0,
        buttons_down,
        wheel,
        reserved2: 0,
        x,
        y,
        flags,
    });
    self::input::push_event(self::input::InputEvent::Tablet(self::input::TabletEvent {
        slot_id,
        buttons,
        report_id: 0,
        x_raw,
        y_raw,
        x_norm_q15: q15_x,
        y_norm_q15: q15_y,
        flags: flags.min(u8::MAX as u32) as u8,
    }));
    crate::r::cursor::upsert_snapshot(
        HID_UDP_CONTROLLER_ID,
        slot_id,
        0,
        HID_KIND_TABLET,
        x,
        y,
        buttons_down,
    );
    self::hut::upsert_tablet_state(
        HID_UDP_CONTROLLER_ID,
        slot_id,
        0,
        x,
        y,
        x_raw,
        y_raw,
        buttons_down,
        0,
        self::hut::HidSourceKind::Ai,
        "udp",
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

pub(crate) fn inject_usb3_mouse_relative_event(
    slot_id: u32,
    ep_target: u32,
    dx: i8,
    dy: i8,
    buttons_down: u32,
    wheel: i16,
    _flags: u32,
) {
    let report = [buttons_down as u8, dx as u8, dy as u8, wheel as u8];
    handle_mouse_boot_report(super::CRABUSB_CONTROLLER_ID, slot_id, ep_target, &report);
}

pub(crate) fn handle_tablet_boot_report(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    data: &[u8],
) {
    let now_ms = now_ms_u32();
    let mut runtimes = HID_RUNTIMES.lock();
    let runtime =
        runtime_mut_or_insert(&mut runtimes, controller_id, slot_id, ep_target, HID_KIND_TABLET);
    runtime.seq = runtime.seq.wrapping_add(1);
    tablet::handle_report(runtime, data, now_ms);
}

pub(crate) fn remove_hid_slot(controller_id: u32, slot_id: u32) {
    let mut runtimes = HID_RUNTIMES.lock();
    runtimes
        .retain(|runtime| !(runtime.controller_id == controller_id && runtime.slot_id == slot_id));
    let _ = self::hut::remove_slot(controller_id, slot_id);
    let _ = crate::r::cursor::remove_snapshots(controller_id, slot_id);
    let _ = crate::r::keyboard::remove_snapshots(controller_id, slot_id);
}

#[inline]
fn cabi_hut_source_kind(value: u8) -> hut::HidSourceKind {
    match value {
        1 => hut::HidSourceKind::Human,
        2 => hut::HidSourceKind::Ai,
        _ => hut::HidSourceKind::Unknown,
    }
}

#[inline]
fn cabi_utf8<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if len == 0 {
        return Some("");
    }
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    core::str::from_utf8(bytes).ok()
}

fn mouse_ring_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: &mut [mouse::TrueosHidMouseSample],
) -> (u32, usize) {
    let mut runtimes = HID_RUNTIMES.lock();
    let Some(runtime) = runtimes.iter_mut().find(|runtime| {
        runtime.controller_id == controller_id
            && runtime.slot_id == slot_id
            && runtime.ep_target == ep_target
            && runtime.hid_kind == HID_KIND_MOUSE
    }) else {
        return (0, 0);
    };

    let dropped = runtime.mouse_ring.dropped;
    runtime.mouse_ring.dropped = 0;

    let mut wrote = 0usize;
    while wrote < out.len() {
        let Some(sample) = runtime.mouse_ring.pop() else {
            break;
        };
        out[wrote] = sample;
        wrote += 1;
    }
    (dropped, wrote)
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

fn tablet_ring_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: &mut [tablet::TrueosHidTabletSample],
) -> (u32, usize) {
    let mut runtimes = HID_RUNTIMES.lock();
    let Some(runtime) = runtimes.iter_mut().find(|runtime| {
        runtime.controller_id == controller_id
            && runtime.slot_id == slot_id
            && runtime.ep_target == ep_target
            && runtime.hid_kind == HID_KIND_TABLET
    }) else {
        return (0, 0);
    };

    let dropped = runtime.tablet_ring.dropped;
    runtime.tablet_ring.dropped = 0;

    let mut wrote = 0usize;
    while wrote < out.len() {
        let Some(sample) = runtime.tablet_ring.pop() else {
            break;
        };
        out[wrote] = sample;
        wrote += 1;
    }
    (dropped, wrote)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_mouse_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: *mut mouse::TrueosHidMouseSample,
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
    let (dropped, wrote) = mouse_ring_read(controller_id, slot_id, ep_target, out_slice);
    if !out_dropped.is_null() {
        *out_dropped = dropped;
    }
    wrote as u32
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_tablet_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out: *mut tablet::TrueosHidTabletSample,
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
    let (dropped, wrote) = tablet_ring_read(controller_id, slot_id, ep_target, out_slice);
    if !out_dropped.is_null() {
        *out_dropped = dropped;
    }
    wrote as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_hut_upsert_combo(
    combo_id: u32,
    source_kind: u8,
    source_tag_ptr: *const u8,
    source_tag_len: usize,
) -> i32 {
    let Some(source_tag) = cabi_utf8(source_tag_ptr, source_tag_len) else {
        return -1;
    };
    if hut::upsert_combo(combo_id, cabi_hut_source_kind(source_kind), source_tag) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_hid_hut_bind_combo_mouse(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> i32 {
    if hut::bind_combo_mouse(combo_id, controller_id, slot_id, ep_target) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_hid_hut_bind_combo_keyboard(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> i32 {
    if hut::bind_combo_keyboard(combo_id, controller_id, slot_id, ep_target) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_hid_hut_bind_combo_tablet(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> i32 {
    if hut::bind_combo_tablet(combo_id, controller_id, slot_id, ep_target) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_hut_read_mice(
    out: *mut hut::TrueosHidHutMouseState,
    out_cap: u32,
) -> u32 {
    if out_cap == 0 {
        return hut::mice_snapshot().len() as u32;
    }
    if out.is_null() {
        return 0;
    }
    let out_slice = core::slice::from_raw_parts_mut(out, out_cap as usize);
    hut::read_mice_snapshot(out_slice) as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_hut_read_tablets(
    out: *mut hut::TrueosHidHutTabletState,
    out_cap: u32,
) -> u32 {
    if out_cap == 0 {
        return hut::tablets_snapshot().len() as u32;
    }
    if out.is_null() {
        return 0;
    }
    let out_slice = core::slice::from_raw_parts_mut(out, out_cap as usize);
    hut::read_tablets_snapshot(out_slice) as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_hut_read_keyboards(
    out: *mut hut::TrueosHidHutKeyboardState,
    out_cap: u32,
) -> u32 {
    if out_cap == 0 {
        return hut::keyboards_snapshot().len() as u32;
    }
    if out.is_null() {
        return 0;
    }
    let out_slice = core::slice::from_raw_parts_mut(out, out_cap as usize);
    hut::read_keyboards_snapshot(out_slice) as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_hid_hut_read_combos(
    out: *mut hut::TrueosHidHutCombo,
    out_cap: u32,
) -> u32 {
    if out_cap == 0 {
        return hut::combos_snapshot().len() as u32;
    }
    if out.is_null() {
        return 0;
    }
    let out_slice = core::slice::from_raw_parts_mut(out, out_cap as usize);
    hut::read_combos_snapshot(out_slice) as u32
}
