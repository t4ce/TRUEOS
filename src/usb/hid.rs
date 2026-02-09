use super::xhci::{
    self, context_index, endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits, ep_interval_bits,
    ep_max_esit_payload_lo_bits, ep_max_packet_bits, ep_state_bits, ep_type_bits, hi, lo,
    trb_type, Trb, TrbRing, XhciContext, EP_STATE_DISABLED, EP_TYPE_INT_IN,
};
use crate::pci::dma;
use crate::usb::input;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::{String as HString, Vec};
use spin::Mutex;

const MAX_REPORT_DESC: usize = 512;

// Local switch to silence noisy HID debug output.
pub(crate) const HID_LOGS: bool = false;

macro_rules! hidlog {
    ($($arg:tt)*) => {{
        if HID_LOGS {
            crate::log!($($arg)*);
        }
    }};
}

#[inline]
fn hid_kbd_shift(modifiers: u8) -> bool {
    // HID boot keyboard modifier bits:
    // 0 LCtrl, 1 LShift, 2 LAlt, 3 LGUI, 4 RCtrl, 5 RShift, 6 RAlt, 7 RGUI
    (modifiers & ((1 << 1) | (1 << 5))) != 0
}

#[inline]
fn hid_boot_keycode_to_ascii(key: u8, shift: bool) -> Option<char> {
    // Minimal US layout mapping for HID Usage Page 0x07 (Keyboard/Keypad).
    // This is *not* a Unicode keyboard decoder; it just produces ASCII for common keys.
    match key {
        // a-z
        0x04..=0x1D => {
            let base = (key - 0x04) + b'a';
            let ch = base as char;
            Some(if shift { ch.to_ascii_uppercase() } else { ch })
        }

        // 1-0
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

        // space
        0x2C => Some(' '),

        // punctuation
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

fn kbd_debug_ascii(keys: &[u8; 6], modifiers: u8) -> HString<16> {
    let shift = hid_kbd_shift(modifiers);
    let mut out: HString<16> = HString::new();
    for &k in keys.iter() {
        if k == 0 {
            continue;
        }
        if let Some(ch) = hid_boot_keycode_to_ascii(k, shift) {
            // Keep logs one-line: don't emit control characters.
            if ch.is_ascii_graphic() || ch == ' ' {
                let _ = out.push(ch);
            }
        }
    }
    out
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
    pub report_phys: u64,
    pub report_virt: *mut u8,
    pub report_len: u32,
    pub hid_kind: u8,
    pub slot_id: u32,
    pub ep_target: u32,
    pub ep_ring: TrbRing,
    pub seq: u64,
    pub last_nonzero_seq: u64,
}

unsafe impl Send for HidRuntime {}
unsafe impl Sync for HidRuntime {}

const MAX_HID_DEVICES: usize = 8;
const MAX_BOOT_INTERFACES: usize = 8;
static HID_RUNTIMES: Mutex<Vec<HidRuntime, MAX_HID_DEVICES>> = Mutex::new(Vec::new());
pub fn hid_kind_from_protocol(protocol: u8) -> u8 {
    // Placeholder: higher-level input stack not wired in yet.
    protocol
}

pub fn register_runtime(runtime: HidRuntime) {
    // Signal v-layer readiness once we have a boot keyboard/mouse runtime.
    // These flags are monotonic (set-only) by design.
    let claimed_flags = match runtime.hid_kind {
        1 => crate::v::readiness::HID_KEYBOARD_CLAIMED,
        2 => crate::v::readiness::HID_MOUSE_CLAIMED,
        _ => 0,
    };
    if claimed_flags != 0 {
        crate::v::readiness::set(claimed_flags);
    }

    let mut guard = HID_RUNTIMES.lock();
    if let Some(existing) = guard
        .iter_mut()
        .find(|r| {
            r.controller_id == runtime.controller_id
                && r.slot_id == runtime.slot_id
                && r.ep_target == runtime.ep_target
        })
    {
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

    hidlog!(
        "[hid] interrupt IN slot={} cc={} rem={} len={} ep=0x{:02X} proto={} seq={} phys=0x{:08X} data={:02X?}\n",
        runtime.slot_id,
        completion,
        residual,
        data.len(),
        runtime.ep.address,
        runtime.hid_kind,
        runtime.seq,
        lo(runtime.report_phys),
        data
    );

    if runtime.hid_kind == 2 {
        // Boot mouse: buttons, dx, dy, wheel (optional).
        if data.len() >= 3 {
            let buttons = data[0];
            let dx = data[1] as i8;
            let dy = data[2] as i8;
            let wheel = if data.len() > 3 { data[3] as i8 } else { 0 };
            if buttons != 0 || dx != 0 || dy != 0 || wheel != 0 {
                runtime.last_nonzero_seq = runtime.seq;
                hidlog!(
                    "[mouse] buttons=0x{:02X} dx={} dy={} wheel={} (slot={} seq={})\n",
                    buttons,
                    dx,
                    dy,
                    wheel,
                    runtime.slot_id,
                    runtime.seq
                );
            }
            input::push_event(input::InputEvent::Mouse(input::MouseEvent {
                slot_id: runtime.slot_id,
                buttons,
                dx,
                dy,
                wheel,
            }));
        }
    } else if runtime.hid_kind == 1 {
        // Boot keyboard: modifiers + 6 keycodes
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
                let chars = kbd_debug_ascii(&keys, modifiers);
                hidlog!(
                    "[kbd] mods=0x{:02X} keys={:02X} {:02X} {:02X} {:02X} {:02X} {:02X} chars='{}'\n",
                    modifiers,
                    keys[0],
                    keys[1],
                    keys[2],
                    keys[3],
                    keys[4],
                    keys[5],
                    chars
                );
            }
            input::push_event(input::InputEvent::Keyboard(input::KeyboardEvent {
                slot_id: runtime.slot_id,
                modifiers,
                keys,
                ascii,
            }));
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn input_logger() {
    async move {
        loop {
            if let Some(evt) = input::pop_event() {
                match evt {
                    input::InputEvent::Keyboard(kbd) => {
                        if kbd.modifiers != 0 || kbd.keys.iter().any(|&c| c != 0) {
                            let show = |b: u8| -> char {
                                if b == 0 {
                                    '.'
                                } else if (b as char).is_ascii_graphic() || b == b' ' {
                                    b as char
                                } else {
                                    '?'
                                }
                            };
                            crate::log!(
                                "[keybd] [{}] mods=0x{:02X} [{}][{}][{}][{}][{}][{}]\n",
                                kbd.slot_id,
                                kbd.modifiers,
                                show(kbd.ascii[0]),
                                show(kbd.ascii[1]),
                                show(kbd.ascii[2]),
                                show(kbd.ascii[3]),
                                show(kbd.ascii[4]),
                                show(kbd.ascii[5])
                            );
                        }
                    }
                    input::InputEvent::Mouse(mouse) => {
                        if mouse.buttons != 0 || mouse.dx != 0 || mouse.dy != 0 || mouse.wheel != 0 {
                            crate::log!(
                                "[mouse] [{}] [move] {:+}/{:+} [click] {:08b} [wheel] {:+}\n",
                                mouse.slot_id,
                                mouse.dx,
                                mouse.dy,
                                mouse.buttons,
                                mouse.wheel
                            );
                        }
                    }
                }
            } else {
                Timer::after(EmbassyDuration::from_millis(5)).await;
            }
        }
    }.await;
}

// NOTE: No synthetic HID injections; reports now only reflect real device data.

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
                // HID descriptor: extract report descriptor length for the current interface.
                if len >= 9 {
                    current_report_len = u16::from_le_bytes([cfg[idx + 7], cfg[idx + 8]]);
                }
            }
            5 => {
                if let Some(iface) = current_iface {
                    let subclass = current_subclass;
                    let proto = current_proto;
                    if current_alt == 0 && subclass == 0x01 && (proto == 0x01 || proto == 0x02) {
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
                                    hidlog!(
                                        "[hid] skipping duplicate HID ep iface={} addr=0x{:02X}\n",
                                        iface,
                                        ep_addr
                                    );
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
                                    hidlog!(
                                        "[hid] HID endpoint list full, dropping iface={} addr=0x{:02X}\n",
                                        iface,
                                        ep_addr
                                    );
                                } else {
                                    hidlog!(
                                        "[hid] parse ep iface={} addr=0x{:02X} mps={} interval={} cfg={} subclass={} proto={}\n",
                                        iface,
                                        ep_addr,
                                        max_packet,
                                        interval,
                                        config_value,
                                        subclass,
                                        proto
                                    );
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
                // HID descriptor: extract report descriptor length for the current interface.
                if len >= 9 {
                    current_report_len = u16::from_le_bytes([cfg[idx + 7], cfg[idx + 8]]);
                }
            }
            5 => {
                if let Some(iface) = current_iface {
                    // Only claim HID interfaces.
                    if current_alt == 0 && current_class == 0x03 {
                        if len >= 7 {
                            let ep_addr = cfg[idx + 2];
                            let attrs = cfg[idx + 3];
                            let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                            let interval = cfg[idx + 6];
                            // Interrupt IN
                            if (attrs & 0x3) == 0x3 && (ep_addr & 0x80) != 0 {
                                if endpoints
                                    .iter()
                                    .any(|e| e.interface == iface && e.address == ep_addr)
                                {
                                    hidlog!(
                                        "[hid] skipping duplicate HID ep iface={} addr=0x{:02X}\n",
                                        iface,
                                        ep_addr
                                    );
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
                                    hidlog!(
                                        "[hid] HID endpoint list full, dropping iface={} addr=0x{:02X}\n",
                                        iface,
                                        ep_addr
                                    );
                                } else {
                                    hidlog!(
                                        "[hid] parse hid ep iface={} addr=0x{:02X} mps={} interval={} cfg={} proto={} rep_len={}\n",
                                        iface,
                                        ep_addr,
                                        max_packet,
                                        interval,
                                        config_value,
                                        current_proto,
                                        current_report_len
                                    );
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
        hidlog!("[hid] empty configuration descriptor\n");
        return Err(());
    }

    let endpoints = parse_hid_interrupt_in_endpoints(cfg);
    if endpoints.is_empty() {
        hidlog!("[hid] no HID interrupt IN endpoints found\n");
        return Err(());
    }

    let config_value = endpoints.first().map(|e| e.configuration).unwrap_or(1);

    // SET_CONFIGURATION
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
        hidlog!("usb: ep0 ring overflow for set_configuration\n");
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
        hidlog!("usb: timeout waiting for set-configuration\n");
        return Err(());
    };

    let completion = (set_cfg_evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        return Err(());
    }

    // Best-effort: idle/protocol setup (works for many HID devices; errors ignored)
    for ep in endpoints.iter() {
        let _ = class_request_nodata(ctx, &mut *ep0_ring, slot_id, 0x0B, 0, ep.interface as u16).await;
        let _ = class_request_nodata(ctx, &mut *ep0_ring, slot_id, 0x0A, 0, ep.interface as u16).await;
    }

    let mut attached = 0usize;
    for ep in endpoints.into_iter() {
        hidlog!(
            "usb: hid(generic) ep addr=0x{:02X} maxpkt={} interval={} iface={} cfg={} proto={}\n",
            ep.address,
            ep.max_packet,
            ep.interval,
            ep.interface,
            ep.configuration,
            ep.protocol
        );

        let hid_kind = hid_kind_from_protocol(ep.protocol);

        let (ep_ring_phys, ep_ring_virt) = match dma::alloc(32 * size_of::<Trb>(), 64) {
            Some(pair) => pair,
            None => {
                hidlog!("usb: failed to alloc ep ring\n");
                continue;
            }
        };
        unsafe { write_bytes(ep_ring_virt, 0, 32 * size_of::<Trb>()) };
        let mut ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, 32) };

        let (input_cfg_phys, input_cfg_virt) = match dma::alloc(4096, 64) {
            Some(pair) => pair,
            None => {
                hidlog!("usb: failed to alloc input ctx for cfg-ep\n");
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
            let interval = if speed_code == 3 {
                core::cmp::min(15u32, ep.interval.saturating_sub(1) as u32)
            } else {
                ep.interval as u32
            };

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
                hidlog!("usb: failed to alloc report buffer\n");
                continue;
            }
        };
        unsafe { write_bytes(rep_virt, 0, report_bytes) };

        let report_len = ep.max_packet as u32;

        if ep.report_desc_len > 0 {
            let _ = fetch_report_descriptor(
                ctx,
                &mut *ep0_ring,
                slot_id,
                ep.interface,
                ep.report_desc_len as usize,
            )
            .await;
        }

        let normal = Trb {
            d0: lo(rep_phys),
            d1: hi(rep_phys),
            d2: report_len,
            d3: trb_type(1) | (1 << 5),
        };
        if !ep_ring.push(normal) {
            hidlog!("usb: ep ring full before interrupt IN\n");
            continue;
        }
        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target as u32) };

        register_runtime(HidRuntime {
            controller_id: ctx.controller_id,
            ep,
            report_phys: rep_phys,
            report_virt: rep_virt,
            report_len,
            hid_kind,
            slot_id,
            ep_target: ep_target as u32,
            ep_ring,
            seq: 0,
            last_nonzero_seq: 0,
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
    Completion(u8),
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

    // xHCI Setup Stage TRB encodes an 8-byte USB setup packet in d0/d1.
    // d0: bmRequestType | (bRequest<<8) | (wValue<<16)
    // d1: wIndex | (wLength<<16)
    // d2: transfer length (8) and TRT bits (IN=2)
    let w_length = want_len as u16;
    let setup_d1 = (setup_d1 & 0xFFFF) | ((w_length as u32) << 16);

    let setup = Trb {
        d0: setup_d0,
        d1: setup_d1,
        d2: 8 | (2 << 16), // 8-byte setup, TRT=IN
        d3: trb_type(2) | (1 << 6),
    };

    let data = Trb {
        d0: lo(phys),
        d1: hi(phys),
        d2: want_len as u32,
        d3: trb_type(3) | (1 << 16), // IN data stage
    };

    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        // Status stage for IN data is OUT (DIR=0)
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

            // EP0 completions may show up on endpoint ID 1 (EP0 OUT) or 2 (EP0 IN)
            // depending on controller behavior.
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
        400,
        EmbassyDuration::from_millis(5),
    )
    .await
    else {
        dma::dealloc(virt, want_len);
        return Err(FetchReportError::Timeout);
    };

    let completion = ((evt.d2 >> 24) & 0xFF) as u8;
    // CC=13 is normal Short Packet (common for descriptor reads).
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
    // bmRequestType=IN|Standard|Interface, bRequest=GET_DESCRIPTOR,
    // wValue=(REPORT<<8)|0, wIndex=interface.
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
    // bmRequestType=IN|Standard|Device, bRequest=GET_DESCRIPTOR,
    // wValue=(REPORT<<8)|0, wIndex=0.
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
    // bmRequestType=IN|Class|Interface, bRequest=GET_REPORT (0x01),
    // wValue=(report_type<<8)|report_id, wIndex=interface.
    let setup_d0 = (0xA1u32) | ((0x01u32) << 8) | (((report_type as u32) << 8) | (report_id as u32)) << 16;
    let setup_d1 = iface as u32;
    fetch_control_in(ctx, ep0_ring, slot_id, setup_d0, setup_d1, len).await
}

pub(crate) fn log_report_descriptor(slot_id: u32, iface: u8, desc: &[u8]) {
    crate::log!(
        "usb: hid report descriptor slot={} iface={} len={}\n",
        slot_id,
        iface,
        desc.len()
    );

    let mut idx: usize = 0;
    while idx < desc.len() {
        let end = core::cmp::min(idx + 16, desc.len());
        crate::log!(
            "usb: hid report desc {:03}..{:03} {:02X?}\n",
            idx,
            end.saturating_sub(1),
            &desc[idx..end]
        );
        idx = end;
    }
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

/// Best-effort parse of a HID report descriptor to find an Output report format.
///
/// This intentionally handles only the subset needed for simple vendor LED devices:
/// Report Size, Report Count, Report ID, and Main Output items.
pub(crate) fn parse_output_report_format(desc: &[u8]) -> Option<HidOutputReportFormat> {
    let mut idx: usize = 0;
    let mut state = HidGlobalState {
        report_size_bits: 0,
        report_count: 0,
        report_id: 0,
    };
    let mut stack: [HidGlobalState; 4] = [HidGlobalState::default(); 4];
    let mut sp: usize = 0;

    // Track the largest Output report payload size per report ID.
    let mut best_id: u8 = 0;
    let mut best_payload_bytes: u16 = 0;

    while idx < desc.len() {
        let b = desc[idx];
        idx += 1;

        if b == 0xFE {
            // Long item: [0xFE, data_size, long_tag, data...]
            if idx + 2 > desc.len() {
                break;
            }
            let data_size = desc[idx] as usize;
            idx += 2; // skip size + long_tag
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
            // Global items.
            (1, 7) => {
                // Report Size (bits)
                state.report_size_bits = value_u32;
            }
            (1, 9) => {
                // Report Count
                state.report_count = value_u32;
            }
            (1, 8) => {
                // Report ID
                state.report_id = (value_u32 & 0xFF) as u8;
            }
            (1, 10) => {
                // Push
                if sp < stack.len() {
                    stack[sp] = state;
                    sp += 1;
                }
            }
            (1, 11) => {
                // Pop
                if sp > 0 {
                    sp -= 1;
                    state = stack[sp];
                }
            }

            // Main items.
            (0, 9) => {
                // Output
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

fn analyze_keyboard_report_descriptor(desc: &[u8]) -> Option<(Option<u8>, u16)> {
    // Heuristic: look for Input items on Usage Page 0x07 with report_size=1 and report_count>=8.
    let mut idx = 0usize;
    let mut usage_page: u16 = 0;
    let mut report_size: u16 = 0;
    let mut report_count: u16 = 0;
    let mut report_id: Option<u8> = None;
    let mut best_bits: u16 = 0;

    while idx < desc.len() {
        let b = desc[idx];
        let size = (b & 0x03) as usize;
        let kind = (b >> 2) & 0x03;
        let tag = b & 0xFC;
        let mut val: u32 = 0;
        for i in 0..size {
            if idx + 1 + i < desc.len() {
                val |= (desc[idx + 1 + i] as u32) << (8 * i);
            }
        }

        if kind == 1 {
            // Global items
            match tag {
                0x04 => usage_page = val as u16,
                0x07 => report_size = val as u16,
                0x09 => report_count = val as u16,
                0x08 => report_id = Some(val as u8),
                _ => {}
            }
        } else if kind == 0 {
            // Main items
            if tag == 0x80 {
                if usage_page == 0x07 && report_size == 1 && report_count >= 8 {
                    let bits = report_size.saturating_mul(report_count);
                    if bits > best_bits {
                        best_bits = bits;
                    }
                }
            }
        }

        idx += 1 + size;
    }

    if best_bits > 0 {
        Some((report_id, best_bits))
    } else {
        None
    }
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
        hidlog!("[hid] empty configuration descriptor\n");
        return Err(());
    }

    let endpoints = parse_boot_endpoints(cfg);
    if endpoints.is_empty() {
        hidlog!("[hid] no HID boot interrupt IN endpoints found\n");
        return Err(());
    }

    let config_value = endpoints.first().map(|e| e.configuration).unwrap_or(1);

    let setup_cfg = Trb {
        d0: 0x0000 | ((9u32) << 8) | ((config_value as u32) << 16),
        d1: 0,
        // Setup Stage TRB: TRB Transfer Length=8, TRT=0 (no data stage)
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    };
    let status_cfg = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        // Status Stage TRB: DIR=1 (IN) for no-data control transfers
        d3: trb_type(4) | (1 << 5) | (1 << 16),
    };
    if !ep0_ring.push(setup_cfg) || !ep0_ring.push(status_cfg) {
        hidlog!("usb: ep0 ring overflow for set_configuration\n");
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
        hidlog!("usb: timeout waiting for set-configuration\n");
        return Err(());
    };

    let completion = (set_cfg_evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        return Err(());
    }

    for ep in endpoints.iter() {
        let _ =
            class_request_nodata(ctx, &mut *ep0_ring, slot_id, 0x0B, 0, ep.interface as u16).await;
        let _ =
            class_request_nodata(ctx, &mut *ep0_ring, slot_id, 0x0A, 0, ep.interface as u16).await;
    }

    let mut attached = 0usize;

    for ep in endpoints.into_iter() {
        hidlog!(
            "usb: hid ep addr=0x{:02X} maxpkt={} interval={} iface={} cfg={} proto={}\n",
            ep.address,
            ep.max_packet,
            ep.interval,
            ep.interface,
            ep.configuration,
            ep.protocol
        );
        let hid_kind = hid_kind_from_protocol(ep.protocol);

        let (ep_ring_phys, ep_ring_virt) = match dma::alloc(32 * size_of::<Trb>(), 64) {
            Some(pair) => pair,
            None => {
                hidlog!("usb: failed to alloc ep ring\n");
                continue;
            }
        };
        unsafe { write_bytes(ep_ring_virt, 0, 32 * size_of::<Trb>()) };
        let mut ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, 32) };

        let (input_cfg_phys, input_cfg_virt) = match dma::alloc(4096, 64) {
            Some(pair) => pair,
            None => {
                hidlog!("usb: failed to alloc input ctx for cfg-ep\n");
                continue;
            }
        };
        unsafe { write_bytes(input_cfg_virt, 0, 4096) };

        let ep_target = endpoint_target(ep.address);
        // Input context array index (slot=1, ep0=2, ep1out=3, ep1in=4, ...)
        let ep_ctx_index = context_index(ep.address);
        // Add Context Flags bit index (slot=0, ep0=1, ep1out=2, ep1in=3, ...)
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
            // Context Entries = highest valid endpoint context index in *device* context
            // (slot=0, ep0=1, ep1out=2, ep1in=3, ...), which corresponds to (ep_ctx_index - 1).
            dw0 = (dw0 & !(0x1F << 27)) | (ep_add_bit << 27);
            write_volatile(slot_ctx.add(0), dw0);

            let mut dw1 = read_volatile(slot_ctx.add(1));
            dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
            write_volatile(slot_ctx.add(1), dw1);

            let mps = (ep.max_packet as u32) & 0x7FF;
            let interval = if speed_code == 3 {
                core::cmp::min(15u32, ep.interval.saturating_sub(1) as u32)
            } else {
                ep.interval as u32
            };

            write_volatile(
                ep_ctx.add(0),
                ep_state_bits(EP_STATE_DISABLED) | ep_interval_bits(interval),
            );
            let mut ep_cfg = ep_cerr_bits(3);
            ep_cfg |= ep_type_bits(EP_TYPE_INT_IN);
            ep_cfg |= ep_max_packet_bits(mps);
            write_volatile(ep_ctx.add(1), ep_cfg);
            // Set dequeue pointer with the current ring cycle bit (DCS) set.
            // Using the raw phys address would leave DCS cleared and the host would
            // ignore our queued transfer ring.
            let dq = ep_ring.dequeue_ptr();
            write_volatile(ep_ctx.add(2), lo(dq));
            write_volatile(ep_ctx.add(3), hi(dq));

            // Use the endpoint's packet size consistently for scheduling hints.
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
                hidlog!("usb: failed to alloc report buffer\n");
                continue;
            }
        };
        unsafe { write_bytes(rep_virt, 0, report_bytes) };

        let report_len = ep.max_packet as u32;

        if hid_kind == 1 && ep.report_desc_len > 0 {
            match fetch_report_descriptor(
                ctx,
                &mut *ep0_ring,
                slot_id,
                ep.interface,
                ep.report_desc_len as usize,
            )
            .await
            {
                Ok(desc) => {
                    if let Some((rid, bits)) = analyze_keyboard_report_descriptor(&desc) {
                        hidlog!(
                            "[hid] iface={} slot={} keyboard report descriptor len={} nkro_bits={} report_id={:?}\n",
                            ep.interface,
                            slot_id,
                            desc.len(),
                            bits,
                            rid
                        );
                    } else {
                        hidlog!(
                            "[hid] iface={} slot={} keyboard report descriptor len={} no bitmap nkro found\n",
                            ep.interface,
                            slot_id,
                            desc.len()
                        );
                    }
                }
                Err(err) => {
                    hidlog!(
                        "[hid] report descriptor fetch failed iface={} slot={} err={:?}\n",
                        ep.interface,
                        slot_id,
                        err
                    );
                }
            }
        }

        let normal = Trb {
            d0: lo(rep_phys),
            d1: hi(rep_phys),
            d2: report_len,
            d3: trb_type(1) | (1 << 5),
        };
        if !ep_ring.push(normal) {
            hidlog!("usb: ep ring full before interrupt IN\n");
            continue;
        }
        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target as u32) };

        register_runtime(HidRuntime {
            controller_id: ctx.controller_id,
            ep,
            report_phys: rep_phys,
            report_virt: rep_virt,
            report_len,
            hid_kind,
            slot_id,
            ep_target: ep_target as u32,
            ep_ring,
            seq: 0,
            last_nonzero_seq: 0,
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
        // Setup Stage TRB: TRB Transfer Length=8, TRT=0 (no data stage)
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    };
    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        // Status Stage TRB: DIR=1 (IN) for no-data control transfers
        d3: trb_type(4) | (1 << 5) | (1 << 16),
    };
    if !ep0_ring.push(setup) || !ep0_ring.push(status) {
        hidlog!("[hid] ep0 ring overflow for class request\n");
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
        hidlog!("[hid] timeout waiting for class request {}\n", request);
        return Err(());
    };

    let completion = (evt.d2 >> 24) & 0xFF;
    hidlog!(
        "[hid] class req {} cc={} value=0x{:04X}\n",
        request,
        completion,
        value
    );
    if completion == 1 {
        Ok(())
    } else {
        Err(())
    }
}
