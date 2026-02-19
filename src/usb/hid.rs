use super::xhci::{
    self, context_index, endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits, ep_interval_bits,
    ep_max_esit_payload_lo_bits, ep_max_packet_bits, ep_state_bits, ep_type_bits, hi, lo, trb_type,
    Trb, TrbRing, XhciContext, EP_STATE_DISABLED, EP_TYPE_INT_IN,
};
use crate::pci::dma;
use crate::usb::input;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::{Duration as EmbassyDuration, Timer};
use embassy_time_driver::TICK_HZ;
use heapless::Vec;
use spin::Mutex;

const MAX_REPORT_DESC: usize = 512;
const HID_MOUSE_RING_CAP: usize = 2048;
const HID_TABLET_RING_CAP: usize = 512;
const HID_TABLET_SAMPLE_PERIOD_MS: u32 = 10;
const HID_TABLET_ABS_MAX: u32 = 0x7FFF;
const HID_MOUSE_NORM_PER_DELTA: f64 = 1.0 / 2000.0;
const HID_QEMU_TABLET_REPORT_DESC_FALLBACK_LEN: usize = 256;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidMouseSample {
    pub t_ms: u32,
    pub seq: u32,
    pub slot_id: u32,
    pub buttons: u8,
    pub dx: i8,
    pub dy: i8,
    pub wheel: i8,
    pub flags: u8,
}

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

const ZERO_MOUSE_SAMPLE: TrueosHidMouseSample = TrueosHidMouseSample {
    t_ms: 0,
    seq: 0,
    slot_id: 0,
    buttons: 0,
    dx: 0,
    dy: 0,
    wheel: 0,
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
struct MouseRing {
    buf: [TrueosHidMouseSample; HID_MOUSE_RING_CAP],
    r: u32,
    w: u32,
    len: u32,
    dropped: u32,
}

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

impl MouseRing {
    fn new() -> Self {
        Self {
            buf: [ZERO_MOUSE_SAMPLE; HID_MOUSE_RING_CAP],
            r: 0,
            w: 0,
            len: 0,
            dropped: 0,
        }
    }

    #[inline]
    fn push(&mut self, s: TrueosHidMouseSample) {
        let cap = HID_MOUSE_RING_CAP as u32;
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
    fn pop(&mut self) -> Option<TrueosHidMouseSample> {
        if self.len == 0 {
            return None;
        }
        let cap = HID_MOUSE_RING_CAP as u32;
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
    let guard = HID_RUNTIMES.lock();
    let mut out: heapless::Vec<(f64, f64), MAX_HID_DEVICES> = heapless::Vec::new();
    for rt in guard.iter() {
        if rt.hid_kind != 2 {
            continue;
        }
        let _ = out.push((rt.mouse_x, rt.mouse_y));
    }
    out
}

pub fn for_each_tablet_cursor(mut f: impl FnMut(f64, f64)) {
    let guard = HID_RUNTIMES.lock();
    for rt in guard.iter() {
        if rt.hid_kind != 3 {
            continue;
        }
        f(rt.tablet_x, rt.tablet_y);
    }
}

pub fn tablet_cursor_snapshot() -> heapless::Vec<(f64, f64), MAX_HID_DEVICES> {
    let guard = HID_RUNTIMES.lock();
    let mut out: heapless::Vec<(f64, f64), MAX_HID_DEVICES> = heapless::Vec::new();
    for rt in guard.iter() {
        if rt.hid_kind != 3 {
            continue;
        }
        let _ = out.push((rt.tablet_x, rt.tablet_y));
    }
    out
}

impl Default for MouseRing {
    fn default() -> Self {
        Self::new()
    }
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

#[inline]
fn hid_kbd_shift(modifiers: u8) -> bool {
    (modifiers & ((1 << 1) | (1 << 5))) != 0
}

#[inline]
fn hid_boot_keycode_to_ascii(key: u8, shift: bool) -> Option<char> {
    match key {
        0x04..=0x1D => {
            let base = (key - 0x04) + b'a';
            let ch = base as char;
            Some(if shift { ch.to_ascii_uppercase() } else { ch })
        }

        0x1E => Some(if shift { '!' } else { '1' }),
        0x1F => Some(if shift { '@' } else { '2' }),
        0x20 => Some(if shift { '#' } else { '3' }),
        0x21 => Some(if shift { '$' } else { '4' }),
        0x22 => Some(if shift { '%' } else { '5' }),
        0x23 => Some(if shift { '^' } else { '6' }),
        0x24 => Some(if shift { '&' } else { '7' }),
        0x25 => Some(if shift { '*' } else { '8' }),
        0x26 => Some(if shift { '(' } else { '9' }),
        0x27 => Some(if shift { ')' } else { '0' }),

        0x2C => Some(' '),

        0x2D => Some(if shift { '_' } else { '-' }),
        0x2E => Some(if shift { '+' } else { '=' }),
        0x2F => Some(if shift { '{' } else { '[' }),
        0x30 => Some(if shift { '}' } else { ']' }),
        0x31 => Some(if shift { '|' } else { '\\' }),
        0x33 => Some(if shift { ':' } else { ';' }),
        0x34 => Some(if shift { '"' } else { '\'' }),
        0x35 => Some(if shift { '~' } else { '`' }),
        0x36 => Some(if shift { '<' } else { ',' }),
        0x37 => Some(if shift { '>' } else { '.' }),
        0x38 => Some(if shift { '?' } else { '/' }),

        _ => None,
    }
}

pub struct HidEpInfo {
    pub configuration: u8,
    pub interface: u8,
    pub address: u8,
    pub max_packet: u16,
    pub interval: u8,
    pub protocol: u8,
    pub report_desc_len: u16,
}

pub struct HidRuntime {
    pub controller_id: usize,
    pub ep: HidEpInfo,

    pub report_desc: Vec<u8, MAX_REPORT_DESC>,
    pub report_phys: u64,
    pub report_virt: *mut u8,
    pub report_len: u32,
    pub hid_kind: u8,
    pub slot_id: u32,
    pub ep_target: u32,
    pub ep_ring: TrbRing,
    pub seq: u64,
    pub last_nonzero_seq: u64,

    mouse_ring: MouseRing,

    mouse_x: f64,
    mouse_y: f64,

    tablet_ring: TabletRing,
    tablet_x: f64,
    tablet_y: f64,
    tablet_last_sample_ms: u32,
}

unsafe impl Send for HidRuntime {}
unsafe impl Sync for HidRuntime {}

const MAX_HID_DEVICES: usize = 16;
const MAX_BOOT_INTERFACES: usize = 8;
static HID_RUNTIMES: Mutex<Vec<HidRuntime, MAX_HID_DEVICES>> = Mutex::new(Vec::new());

pub fn hid_kind_from_protocol(protocol: u8) -> u8 {
    match protocol {
        1 => 1,
        2 => 2,
        _ => protocol,
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
        return;
    }
    let _ = guard.push(runtime);
}

pub fn unregister_runtime(controller_id: usize, slot_id: u32) -> bool {
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
        if data.len() >= 8 {
            let modifiers = data[0];
            let mut keys = [0u8; 6];
            keys.copy_from_slice(&data[2..8]);

            let shift = hid_kbd_shift(modifiers);
            let mut ascii = [0u8; 6];
            for (dst, &k) in ascii.iter_mut().zip(keys.iter()) {
                if k == 0 {
                    *dst = 0;
                    continue;
                }
                *dst = hid_boot_keycode_to_ascii(k, shift)
                    .and_then(|ch| if ch.is_ascii() { Some(ch as u8) } else { None })
                    .unwrap_or(b'?');
            }
            if keys.iter().any(|&k| k != 0) || modifiers != 0 {
                runtime.last_nonzero_seq = runtime.seq;
            }
            input::push_event(input::InputEvent::Keyboard(input::KeyboardEvent {
                slot_id: runtime.slot_id,
                modifiers,
                keys,
                ascii,
            }));
        }
    } else if runtime.hid_kind == 2 {
        let _ = completion;
        let _ = residual;
        if data.len() >= 4 {
            let buttons = data[0];
            let dx = i8::from_le_bytes([data[1]]);
            let dy = i8::from_le_bytes([data[2]]);
            let wheel = i8::from_le_bytes([data[3]]);

            if dx != 0 || dy != 0 {
                runtime.mouse_x = clamp01(runtime.mouse_x + (dx as f64) * HID_MOUSE_NORM_PER_DELTA);
                runtime.mouse_y = clamp01(runtime.mouse_y + (dy as f64) * HID_MOUSE_NORM_PER_DELTA);
            }

            runtime.mouse_ring.push(TrueosHidMouseSample {
                t_ms: hid_uptime_ms() as u32,
                seq: runtime.seq as u32,
                slot_id: runtime.slot_id,
                buttons,
                dx,
                dy,
                wheel,
                flags: 1 << 0,
            });
        }
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
        }
    } else {
        let _ = completion;
        let _ = residual;
    }
}

#[no_mangle]
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

#[no_mangle]
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

    if ok {
        0
    } else {
        -3
    }
}

#[no_mangle]
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

#[no_mangle]
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

    if ok {
        0
    } else {
        -3
    }
}

#[embassy_executor::task]
pub(crate) async fn input_logger() {
    loop {
        while input::pop_event().is_some() {}
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }
}

pub fn parse_boot_endpoints(cfg: &[u8]) -> Vec<HidEpInfo, MAX_BOOT_INTERFACES> {
    let mut idx = 0usize;
    let mut config_value = 1u8;
    let mut current_iface: Option<u8> = None;
    let mut current_alt: u8 = 0;
    let mut current_proto: u8 = 0;
    let mut current_subclass: u8 = 0;
    let mut current_report_len: u16 = 0;
    let mut endpoints: Vec<HidEpInfo, MAX_BOOT_INTERFACES> = Vec::new();

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        let ty = cfg[idx + 1];
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        match ty {
            2 => {
                if len >= 6 {
                    config_value = cfg[idx + 5];
                }
            }
            4 => {
                if len >= 9 {
                    current_iface = Some(cfg[idx + 2]);
                    current_alt = cfg[idx + 3];
                    current_subclass = cfg[idx + 6];
                    current_proto = cfg[idx + 7];
                    current_report_len = 0;
                } else {
                    current_iface = None;
                }
            }
            0x21 => {
                if len >= 9 {
                    current_report_len = u16::from_le_bytes([cfg[idx + 7], cfg[idx + 8]]);
                }
            }
            5 => {
                if let Some(iface) = current_iface {
                    let subclass = current_subclass;
                    let proto = current_proto;
                    if current_alt == 0 && subclass == 0x01 && proto == 0x01 {
                        if len >= 7 {
                            let ep_addr = cfg[idx + 2];
                            let attrs = cfg[idx + 3];
                            let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                            let interval = cfg[idx + 6];
                            if (attrs & 0x3) == 0x3 && (ep_addr & 0x80) != 0 {
                                if endpoints
                                    .iter()
                                    .any(|e| e.interface == iface && e.address == ep_addr)
                                {
                                } else if endpoints
                                    .push(HidEpInfo {
                                        configuration: config_value,
                                        interface: iface,
                                        address: ep_addr,
                                        max_packet,
                                        interval,
                                        protocol: proto,
                                        report_desc_len: current_report_len,
                                    })
                                    .is_err()
                                {
                                } else {
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        idx += len;
    }

    endpoints
}

pub fn parse_hid_interrupt_in_endpoints(cfg: &[u8]) -> Vec<HidEpInfo, MAX_BOOT_INTERFACES> {
    let mut idx = 0usize;
    let mut config_value = 1u8;
    let mut current_iface: Option<u8> = None;
    let mut current_alt: u8 = 0;
    let mut current_class: u8 = 0;
    let mut current_proto: u8 = 0;
    let mut current_report_len: u16 = 0;
    let mut endpoints: Vec<HidEpInfo, MAX_BOOT_INTERFACES> = Vec::new();

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        let ty = cfg[idx + 1];
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        match ty {
            2 => {
                if len >= 6 {
                    config_value = cfg[idx + 5];
                }
            }
            4 => {
                if len >= 9 {
                    current_iface = Some(cfg[idx + 2]);
                    current_alt = cfg[idx + 3];
                    current_class = cfg[idx + 5];
                    current_proto = cfg[idx + 7];
                    current_report_len = 0;
                } else {
                    current_iface = None;
                }
            }
            0x21 => {
                if len >= 9 {
                    current_report_len = u16::from_le_bytes([cfg[idx + 7], cfg[idx + 8]]);
                }
            }
            5 => {
                if let Some(iface) = current_iface {
                    if current_alt == 0 && current_class == 0x03 {
                        if len >= 7 {
                            let ep_addr = cfg[idx + 2];
                            let attrs = cfg[idx + 3];
                            let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                            let interval = cfg[idx + 6];

                            if (attrs & 0x3) == 0x3 && (ep_addr & 0x80) != 0 {
                                if endpoints
                                    .iter()
                                    .any(|e| e.interface == iface && e.address == ep_addr)
                                {
                                } else if endpoints
                                    .push(HidEpInfo {
                                        configuration: config_value,
                                        interface: iface,
                                        address: ep_addr,
                                        max_packet,
                                        interval,
                                        protocol: current_proto,
                                        report_desc_len: current_report_len,
                                    })
                                    .is_err()
                                {
                                } else {
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        idx += len;
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
        d0: 0x0000 | ((9u32) << 8) | ((config_value as u32) << 16),
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

    let is_qemu_tablet = xhci::get_port_vidpid(ctx.controller_id, target_port)
        .map(|(vid, pid)| vid == 0x0627 && pid == 0x0001)
        .unwrap_or(false);

    let mut attached = 0usize;
    for ep in endpoints.into_iter() {
        let mut hid_kind = hid_kind_from_protocol(ep.protocol);
        if is_qemu_tablet {
            hid_kind = if ep.protocol == 1 { 1 } else { 3 };
        }

        let want_desc_len = if ep.report_desc_len != 0 {
            ep.report_desc_len as usize
        } else {
            HID_QEMU_TABLET_REPORT_DESC_FALLBACK_LEN
        };
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
        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target as u32) };

        register_runtime(HidRuntime {
            controller_id: ctx.controller_id,
            ep,
            report_desc,
            report_phys: rep_phys,
            report_virt: rep_virt,
            report_len,
            hid_kind,
            slot_id,
            ep_target: ep_target as u32,
            ep_ring,
            seq: 0,
            last_nonzero_seq: 0,

            mouse_ring: MouseRing::new(),
            mouse_x: 0.5,
            mouse_y: 0.5,

            tablet_ring: TabletRing::new(),
            tablet_x: 0.5,
            tablet_y: 0.5,
            tablet_last_sample_ms: 0,
        });

        attached += 1;
    }

    if attached > 0 {
        Ok(attached)
    } else {
        Err(())
    }
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
    let (phys, virt) = dma::alloc(want_len, 64).ok_or(FetchReportError::Alloc)?;
    unsafe { write_bytes(virt, 0, want_len) };

    let w_length = want_len as u16;
    let setup_d1 = (setup_d1 & 0xFFFF) | ((w_length as u32) << 16);

    let setup = Trb {
        d0: setup_d0,
        d1: setup_d1,
        d2: 8 | (2 << 16),
        d3: trb_type(2) | (1 << 6),
    };

    let data = Trb {
        d0: lo(phys),
        d1: hi(phys),
        d2: want_len as u32,
        d3: trb_type(3) | (1 << 16),
    };

    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,

        d3: trb_type(4) | (1 << 5),
    };

    let Some(setup_trb_phys) = ep0_ring.push_with_phys(setup) else {
        dma::dealloc(virt, want_len);
        return Err(FetchReportError::RingOverflow);
    };
    let Some(data_trb_phys) = ep0_ring.push_with_phys(data) else {
        dma::dealloc(virt, want_len);
        return Err(FetchReportError::RingOverflow);
    };
    let Some(status_trb_phys) = ep0_ring.push_with_phys(status) else {
        dma::dealloc(virt, want_len);
        return Err(FetchReportError::RingOverflow);
    };

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let Some(evt) = xhci::wait_for_event(
        ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            if evt_slot != slot_id {
                return false;
            }

            let evt_target = (evt.d3 >> 16) & 0x1F;
            if evt_target != 1 && evt_target != 2 {
                return false;
            }

            let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
            let evt_ptr = evt_ptr & !0xFu64;
            evt_ptr == (setup_trb_phys & !0xFu64)
                || evt_ptr == (data_trb_phys & !0xFu64)
                || evt_ptr == (status_trb_phys & !0xFu64)
        },
        1,
        EmbassyDuration::from_millis(5),
    )
    .await
    else {
        dma::dealloc(virt, want_len);
        return Err(FetchReportError::Timeout);
    };

    let completion = ((evt.d2 >> 24) & 0xFF) as u8;

    if completion != 1 && completion != 13 {
        dma::dealloc(virt, want_len);
        return Err(FetchReportError::Completion(completion));
    }

    let remaining = (evt.d2 & 0x00FF_FFFF) as u32;
    let requested = want_len as u32;
    let transferred = requested.saturating_sub(remaining).min(requested) as usize;

    let mut out = Vec::<u8, MAX_REPORT_DESC>::new();
    let data_slice = unsafe { core::slice::from_raw_parts(virt, want_len) };
    let _ = out.extend_from_slice(&data_slice[..transferred]);
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
    let setup_d0 = (0x81u32) | ((0x06u32) << 8) | ((0x22u32) << 16);
    let setup_d1 = iface as u32;
    fetch_control_in(ctx, ep0_ring, slot_id, setup_d0, setup_d1, len).await
}

pub(crate) async fn fetch_report_descriptor_device(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    len: usize,
) -> Result<Vec<u8, MAX_REPORT_DESC>, FetchReportError> {
    let setup_d0 = (0x80u32) | ((0x06u32) << 8) | ((0x22u32) << 16);
    let setup_d1 = 0;
    fetch_control_in(ctx, ep0_ring, slot_id, setup_d0, setup_d1, len).await
}

pub(crate) async fn fetch_hid_get_report(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    report_type: u8,
    report_id: u8,
    len: usize,
) -> Result<Vec<u8, MAX_REPORT_DESC>, FetchReportError> {
    let setup_d0 =
        (0xA1u32) | ((0x01u32) << 8) | (((report_type as u32) << 8) | (report_id as u32)) << 16;
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

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct HidOutputReportFormat {
    pub report_id: u8,
    pub total_len_bytes: u16,
}

#[derive(Copy, Clone, Debug, Default)]
struct HidGlobalState {
    report_size_bits: u32,
    report_count: u32,
    report_id: u8,
}

pub(crate) fn parse_output_report_format(desc: &[u8]) -> Option<HidOutputReportFormat> {
    let mut idx: usize = 0;
    let mut state = HidGlobalState {
        report_size_bits: 0,
        report_count: 0,
        report_id: 0,
    };
    let mut stack: [HidGlobalState; 4] = [HidGlobalState::default(); 4];
    let mut sp: usize = 0;

    let mut best_id: u8 = 0;
    let mut best_payload_bytes: u16 = 0;

    while idx < desc.len() {
        let b = desc[idx];
        idx += 1;

        if b == 0xFE {
            if idx + 2 > desc.len() {
                break;
            }
            let data_size = desc[idx] as usize;
            idx += 2;
            idx = idx.saturating_add(data_size);
            continue;
        }

        let size_code = (b & 0x03) as usize;
        let data_size = match size_code {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 4,
            _ => 0,
        };
        let item_type = (b >> 2) & 0x03;
        let tag = (b >> 4) & 0x0F;

        if idx + data_size > desc.len() {
            break;
        }

        let mut value_u32: u32 = 0;
        for i in 0..data_size {
            value_u32 |= (desc[idx + i] as u32) << (8 * i);
        }
        idx += data_size;

        match (item_type, tag) {
            (1, 7) => {
                state.report_size_bits = value_u32;
            }
            (1, 9) => {
                state.report_count = value_u32;
            }
            (1, 8) => {
                state.report_id = (value_u32 & 0xFF) as u8;
            }
            (1, 10) => {
                if sp < stack.len() {
                    stack[sp] = state;
                    sp += 1;
                }
            }
            (1, 11) => {
                if sp > 0 {
                    sp -= 1;
                    state = stack[sp];
                }
            }

            (0, 9) => {
                let bits = state.report_size_bits.saturating_mul(state.report_count);
                if bits == 0 {
                    continue;
                }
                let payload_bytes = ((bits + 7) / 8) as u16;
                if payload_bytes >= best_payload_bytes {
                    best_payload_bytes = payload_bytes;
                    best_id = state.report_id;
                }
            }

            _ => {}
        }
    }

    if best_payload_bytes == 0 {
        return None;
    }

    let total_len_bytes = best_payload_bytes.saturating_add((best_id != 0) as u16);
    Some(HidOutputReportFormat {
        report_id: best_id,
        total_len_bytes,
    })
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

pub async fn attach_boot_devices(params: BootAttachParams<'_>) -> Result<usize, ()> {
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

    let endpoints = parse_boot_endpoints(cfg);
    if endpoints.is_empty() {
        return Err(());
    }

    let config_value = endpoints.first().map(|e| e.configuration).unwrap_or(1);

    let setup_cfg = Trb {
        d0: 0x0000 | ((9u32) << 8) | ((config_value as u32) << 16),
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

    let is_qemu_tablet = xhci::get_port_vidpid(ctx.controller_id, target_port)
        .map(|(vid, pid)| vid == 0x0627 && pid == 0x0001)
        .unwrap_or(false);

    for ep in endpoints.iter() {
        let _ =
            class_request_nodata(ctx, &mut *ep0_ring, slot_id, 0x0B, 0, ep.interface as u16).await;
        let _ =
            class_request_nodata(ctx, &mut *ep0_ring, slot_id, 0x0A, 0, ep.interface as u16).await;
    }

    let mut attached = 0usize;

    for ep in endpoints.into_iter() {
        let mut hid_kind = hid_kind_from_protocol(ep.protocol);
        if is_qemu_tablet {
            hid_kind = if ep.protocol == 1 { 1 } else { 3 };
        }

        let want_desc_len = if ep.report_desc_len != 0 {
            ep.report_desc_len as usize
        } else {
            HID_QEMU_TABLET_REPORT_DESC_FALLBACK_LEN
        };
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

            let avg_trb_len = mps;
            let max_esit_payload = mps;
            write_volatile(
                ep_ctx.add(4),
                ep_avg_trb_len_bits(avg_trb_len) | ep_max_esit_payload_lo_bits(max_esit_payload),
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

        let _ = is_qemu_tablet;
        let _ = hid_kind;

        let normal = Trb {
            d0: lo(rep_phys),
            d1: hi(rep_phys),
            d2: report_len,
            d3: trb_type(1) | (1 << 5),
        };
        if !ep_ring.push(normal) {
            continue;
        }
        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target as u32) };

        register_runtime(HidRuntime {
            controller_id: ctx.controller_id,
            ep,
            report_desc,
            report_phys: rep_phys,
            report_virt: rep_virt,
            report_len,
            hid_kind,
            slot_id,
            ep_target: ep_target as u32,
            ep_ring,
            seq: 0,
            last_nonzero_seq: 0,
            mouse_ring: MouseRing::new(),
            mouse_x: 0.5,
            mouse_y: 0.5,
            tablet_ring: TabletRing::new(),
            tablet_x: 0.5,
            tablet_y: 0.5,
            tablet_last_sample_ms: 0,
        });

        attached += 1;
    }

    if attached > 0 {
        Ok(attached)
    } else {
        Err(())
    }
}

pub async fn class_request_nodata(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    request: u8,
    value: u16,
    index: u16,
) -> Result<(), ()> {
    let setup = Trb {
        d0: (0x21u32) | ((request as u32) << 8) | ((value as u32) << 16),
        d1: index as u32,

        d2: 8,
        d3: trb_type(2) | (1 << 6),
    };
    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,

        d3: trb_type(4) | (1 << 5) | (1 << 16),
    };
    if !ep0_ring.push(setup) || !ep0_ring.push(status) {
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let Some(evt) = xhci::wait_for_event(
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

    let completion = (evt.d2 >> 24) & 0xFF;
    if completion == 1 {
        Ok(())
    } else {
        Err(())
    }
}
