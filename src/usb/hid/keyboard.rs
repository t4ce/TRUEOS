use super::HidRuntime;

const HID_KEYBOARD_RING_CAP: usize = 512;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidKeyboardSample {
    pub t_ms: u32,
    pub seq: u32,
    pub slot_id: u32,
    pub modifiers: u8,
    pub reserved0: u8,
    pub reserved1: u16,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub flags: u32,
}

const ZERO_KEYBOARD_SAMPLE: TrueosHidKeyboardSample = TrueosHidKeyboardSample {
    t_ms: 0,
    seq: 0,
    slot_id: 0,
    modifiers: 0,
    reserved0: 0,
    reserved1: 0,
    keys: [0; 6],
    ascii: [0; 6],
    flags: 0,
};

#[derive(Copy, Clone, Debug)]
pub(crate) struct KeyboardRing {
    buf: [TrueosHidKeyboardSample; HID_KEYBOARD_RING_CAP],
    r: u32,
    w: u32,
    len: u32,
    pub(crate) dropped: u32,
}

impl KeyboardRing {
    pub(crate) fn new() -> Self {
        Self {
            buf: [ZERO_KEYBOARD_SAMPLE; HID_KEYBOARD_RING_CAP],
            r: 0,
            w: 0,
            len: 0,
            dropped: 0,
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, s: TrueosHidKeyboardSample) {
        let cap = HID_KEYBOARD_RING_CAP as u32;
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
    pub(crate) fn pop(&mut self) -> Option<TrueosHidKeyboardSample> {
        if self.len == 0 {
            return None;
        }
        let cap = HID_KEYBOARD_RING_CAP as u32;
        let s = self.buf[self.r as usize];
        self.r = (self.r + 1) % cap;
        self.len -= 1;
        Some(s)
    }
}

impl Default for KeyboardRing {
    fn default() -> Self {
        Self::new()
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

pub(crate) fn handle_report(runtime: &mut HidRuntime, data: &[u8], now_ms: u32) {
    if data.len() < 8 {
        return;
    }

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
            .unwrap_or(0);
    }
    if keys.iter().any(|&k| k != 0) || modifiers != 0 {
        runtime.last_nonzero_seq = runtime.seq;
    }
    let prev_modifiers = runtime.keyboard_modifiers;
    let prev_keys = runtime.keyboard_keys;
    let prev_ascii = runtime.keyboard_ascii;
    runtime.keyboard_modifiers = modifiers;
    runtime.keyboard_keys = keys;
    runtime.keyboard_ascii = ascii;

    let mut flags = 0u32;
    if modifiers != 0 || keys.iter().any(|&k| k != 0) {
        flags |= 1 << 0;
    }
    if prev_modifiers != modifiers {
        flags |= 1 << 1;
    }
    if prev_keys != keys {
        flags |= 1 << 2;
    }
    if prev_ascii != ascii {
        flags |= 1 << 3;
    }

    runtime.keyboard_ring.push(TrueosHidKeyboardSample {
        t_ms: now_ms,
        seq: runtime.seq as u32,
        slot_id: runtime.slot_id,
        modifiers,
        reserved0: 0,
        reserved1: 0,
        keys,
        ascii,
        flags,
    });
    crate::usb::input::push_event(crate::usb::input::InputEvent::Keyboard(
        crate::usb::input::KeyboardEvent {
            slot_id: runtime.slot_id,
            modifiers,
            keys,
            ascii,
        },
    ));
    crate::usb::hut::upsert_keyboard_state(
        runtime.controller_id as u32,
        runtime.slot_id,
        runtime.ep_target,
        modifiers,
        keys,
        ascii,
        crate::usb::hut::HidSourceKind::Human,
        "human",
        false,
    );
    crate::v::keyboard::apply_report(
        runtime.controller_id as u32,
        runtime.slot_id,
        runtime.ep_target,
        now_ms,
        runtime.seq as u32,
        modifiers,
        keys,
        ascii,
    );
}
