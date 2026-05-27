use super::{
    HID_DEBUG_REPORT_LOGS, HidRuntime, TrueosHidCursorEvent, clamp01, push_cursor_event,
    sync_runtime_cursor_snapshot,
};

const HID_TABLET_RING_CAP: usize = crate::allcaps::input::HID_TABLET_RING_CAP;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidTabletSample {
    pub t_ms: u32,
    pub seq: u32,
    pub slot_id: u32,
    pub buttons: u8,
    pub report_id: u8,
    pub flags: u8,
    pub reserved0: u8,
    pub x_raw: u16,
    pub y_raw: u16,
    pub x_norm_q15: u16,
    pub y_norm_q15: u16,
}

const ZERO_TABLET_SAMPLE: TrueosHidTabletSample = TrueosHidTabletSample {
    t_ms: 0,
    seq: 0,
    slot_id: 0,
    buttons: 0,
    report_id: 0,
    flags: 0,
    reserved0: 0,
    x_raw: 0,
    y_raw: 0,
    x_norm_q15: 0,
    y_norm_q15: 0,
};

#[derive(Copy, Clone, Debug)]
pub(crate) struct TabletRing {
    buf: [TrueosHidTabletSample; HID_TABLET_RING_CAP],
    r: u32,
    w: u32,
    len: u32,
    pub(crate) dropped: u32,
}

impl TabletRing {
    pub(crate) fn new() -> Self {
        Self {
            buf: [ZERO_TABLET_SAMPLE; HID_TABLET_RING_CAP],
            r: 0,
            w: 0,
            len: 0,
            dropped: 0,
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, s: TrueosHidTabletSample) {
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
    pub(crate) fn pop(&mut self) -> Option<TrueosHidTabletSample> {
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

impl Default for TabletRing {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone, Debug)]
struct DecodedTabletPacket {
    report_id: u8,
    buttons: u8,
    x_raw: u16,
    y_raw: u16,
}

#[inline]
pub(crate) fn matches_interface(class: u8, subclass: u8, protocol: u8) -> bool {
    class == 0x03 && subclass == 0x00 && protocol == 0x00
}

#[inline]
pub(crate) fn report_len(max_packet_size: u16) -> usize {
    usize::from(max_packet_size.max(8))
}

#[inline]
fn decode_packet(sample: &[u8]) -> Option<DecodedTabletPacket> {
    if sample.len() < 5 {
        return None;
    }

    if sample.len() >= 6 && sample[0] <= 0x0F && sample[1] <= 0x1F {
        return Some(DecodedTabletPacket {
            report_id: sample[0],
            buttons: sample[1],
            x_raw: u16::from_le_bytes([sample[2], sample[3]]),
            y_raw: u16::from_le_bytes([sample[4], sample[5]]),
        });
    }

    Some(DecodedTabletPacket {
        report_id: 0,
        buttons: sample[0],
        x_raw: u16::from_le_bytes([sample[1], sample[2]]),
        y_raw: u16::from_le_bytes([sample[3], sample[4]]),
    })
}

#[inline]
fn q15_from_norm(v: f64) -> u16 {
    let scaled = (clamp01(v) * 32767.0) + 0.5;
    scaled.clamp(0.0, 32767.0) as u16
}

pub(crate) fn handle_report(runtime: &mut HidRuntime, data: &[u8], now_ms: u32) {
    let Some(decoded) = decode_packet(data) else {
        if HID_DEBUG_REPORT_LOGS {
            crate::log!(
                "tablet-report: ctrl={} slot={} ep={} seq={} decode=unsupported len={}\n",
                runtime.controller_id,
                runtime.slot_id,
                runtime.ep_target,
                runtime.seq as u32,
                data.len(),
            );
        }
        return;
    };

    runtime.last_nonzero_seq = runtime.seq;
    let prev_buttons = runtime.mouse_buttons_down;
    let prev_x = runtime.mouse_x;
    let prev_y = runtime.mouse_y;

    let x = f64::from(decoded.x_raw) / 65535.0;
    let y = f64::from(decoded.y_raw) / 65535.0;
    runtime.mouse_x = clamp01(x);
    runtime.mouse_y = clamp01(y);
    runtime.mouse_buttons_down = u32::from(decoded.buttons);

    let mut flags = 0u32;
    if runtime.mouse_x != prev_x || runtime.mouse_y != prev_y {
        flags |= 1 << 0;
    }
    if runtime.mouse_buttons_down != prev_buttons {
        flags |= 1 << 2;
    }

    runtime.tablet_ring.push(TrueosHidTabletSample {
        t_ms: now_ms,
        seq: runtime.seq as u32,
        slot_id: runtime.slot_id,
        buttons: decoded.buttons,
        report_id: decoded.report_id,
        flags: flags as u8,
        reserved0: 0,
        x_raw: decoded.x_raw,
        y_raw: decoded.y_raw,
        x_norm_q15: q15_from_norm(runtime.mouse_x),
        y_norm_q15: q15_from_norm(runtime.mouse_y),
    });

    if HID_DEBUG_REPORT_LOGS && runtime.mouse_buttons_down != prev_buttons {
        crate::log!(
            "tablet-report: ctrl={} slot={} ep={} seq={} flags=0x{:02X} buttons=0x{:02X} prev=0x{:02X} x={} y={} rid={}\n",
            runtime.controller_id,
            runtime.slot_id,
            runtime.ep_target,
            runtime.seq as u32,
            flags,
            decoded.buttons,
            prev_buttons as u8,
            decoded.x_raw,
            decoded.y_raw,
            decoded.report_id,
        );
    }

    push_cursor_event(TrueosHidCursorEvent {
        t_ms: now_ms,
        seq: runtime.seq as u32,
        controller_id: runtime.controller_id,
        slot_id: runtime.slot_id,
        ep_target: runtime.ep_target,
        hid_kind: runtime.hid_kind,
        reserved0: 0,
        reserved1: 0,
        buttons_down: runtime.mouse_buttons_down,
        wheel: 0,
        reserved2: 0,
        x: runtime.mouse_x,
        y: runtime.mouse_y,
        flags,
    });
    super::input::push_event(super::input::InputEvent::Tablet(super::input::TabletEvent {
        slot_id: runtime.slot_id,
        buttons: decoded.buttons,
        report_id: decoded.report_id,
        x_raw: decoded.x_raw,
        y_raw: decoded.y_raw,
        x_norm_q15: q15_from_norm(runtime.mouse_x),
        y_norm_q15: q15_from_norm(runtime.mouse_y),
        flags: flags as u8,
    }));
    sync_runtime_cursor_snapshot(runtime);
    super::hut::upsert_tablet_state(
        runtime.controller_id,
        runtime.slot_id,
        runtime.ep_target,
        runtime.mouse_x,
        runtime.mouse_y,
        decoded.x_raw,
        decoded.y_raw,
        runtime.mouse_buttons_down,
        decoded.report_id,
        super::hut::HidSourceKind::Human,
        "tablet",
        false,
    );
}
