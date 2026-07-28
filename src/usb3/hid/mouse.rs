//! Extracted USB HID support retained through the compatibility module path.

use super::{
    HidRuntime, TrueosHidCursorEvent, clamp01, push_cursor_event, sync_runtime_cursor_snapshot,
};
const HID_MOUSE_RING_CAP: usize = crate::allcaps::input::HID_MOUSE_RING_CAP;

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

#[derive(Copy, Clone, Debug)]
pub(crate) struct MouseRing {
    buf: [TrueosHidMouseSample; HID_MOUSE_RING_CAP],
    r: u32,
    w: u32,
    len: u32,
    pub(crate) dropped: u32,
}

impl MouseRing {
    pub(crate) fn new() -> Self {
        Self {
            buf: [ZERO_MOUSE_SAMPLE; HID_MOUSE_RING_CAP],
            r: 0,
            w: 0,
            len: 0,
            dropped: 0,
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, s: TrueosHidMouseSample) {
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
    pub(crate) fn pop(&mut self) -> Option<TrueosHidMouseSample> {
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

impl Default for MouseRing {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn handle_report(runtime: &mut HidRuntime, data: &[u8], now_ms: u32) {
    if data.len() < 3 {
        return;
    }

    let buttons = data[0];
    let dx = i32::from(i8::from_le_bytes([data[1]]));
    let dy = i32::from(i8::from_le_bytes([data[2]]));
    let wheel = i32::from(data.get(3).copied().map(|value| value as i8).unwrap_or(0));
    let has_wheel = data.len() >= 4;
    handle_decoded_report(runtime, buttons, dx, dy, wheel, has_wheel, now_ms);
}

pub(crate) fn handle_decoded_report(
    runtime: &mut HidRuntime,
    buttons: u8,
    dx: i32,
    dy: i32,
    wheel: i32,
    has_wheel: bool,
    now_ms: u32,
) {
    let legacy_dx = saturating_i8(dx);
    let legacy_dy = saturating_i8(dy);
    let legacy_wheel = saturating_i8(wheel);
    let prev_buttons = runtime.mouse_buttons_down;
    runtime.mouse_buttons_down = u32::from(buttons);

    if dx != 0 || dy != 0 {
        runtime.mouse_x =
            clamp01(runtime.mouse_x + f64::from(dx) * super::HID_MOUSE_NORM_PER_DELTA);
        runtime.mouse_y =
            clamp01(runtime.mouse_y + f64::from(dy) * super::HID_MOUSE_NORM_PER_DELTA);
    }

    runtime.mouse_ring.push(TrueosHidMouseSample {
        t_ms: now_ms,
        seq: runtime.seq as u32,
        slot_id: runtime.slot_id,
        buttons,
        dx: legacy_dx,
        dy: legacy_dy,
        wheel: legacy_wheel,
        flags: 1 << 0,
    });

    let mut flags = 0u32;
    if dx != 0 || dy != 0 {
        flags |= 1 << 0;
    }
    if wheel != 0 {
        flags |= 1 << 1;
    }
    if runtime.mouse_buttons_down != prev_buttons {
        flags |= 1 << 2;
    }

    if runtime.mouse_buttons_down != prev_buttons {
        crate::log!(
            "mouse-click: ctrl={} slot={} ep={} seq={} flags=0x{:02X} buttons=0x{:02X} prev=0x{:02X}\n",
            runtime.controller_id as u32,
            runtime.slot_id,
            runtime.ep_target,
            runtime.seq as u32,
            flags,
            buttons,
            prev_buttons as u8,
        );
    }

    push_cursor_event(TrueosHidCursorEvent {
        t_ms: now_ms,
        seq: runtime.seq as u32,
        controller_id: runtime.controller_id as u32,
        slot_id: runtime.slot_id,
        ep_target: runtime.ep_target,
        hid_kind: runtime.hid_kind,
        reserved0: 0,
        reserved1: 0,
        buttons_down: runtime.mouse_buttons_down,
        wheel: saturating_i16(wheel),
        reserved2: 0,
        x: runtime.mouse_x,
        y: runtime.mouse_y,
        flags,
    });
    super::input::push_event(super::input::InputEvent::Mouse(super::input::MouseEvent {
        slot_id: runtime.slot_id,
        buttons,
        dx: legacy_dx,
        dy: legacy_dy,
        wheel: legacy_wheel,
        has_wheel,
    }));
    super::input::qjs_mouse_offer(super::input::MouseEvent {
        slot_id: runtime.slot_id,
        buttons,
        dx: legacy_dx,
        dy: legacy_dy,
        wheel: legacy_wheel,
        has_wheel,
    });
    sync_runtime_cursor_snapshot(runtime);
    super::hut::upsert_mouse_state(
        runtime.controller_id as u32,
        runtime.slot_id,
        runtime.ep_target,
        runtime.mouse_x,
        runtime.mouse_y,
        runtime.mouse_buttons_down,
        super::hut::HidSourceKind::Human,
        "human",
        false,
    );
}

#[inline]
fn saturating_i8(value: i32) -> i8 {
    value.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8
}

#[inline]
fn saturating_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}
