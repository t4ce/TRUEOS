#![allow(dead_code)]

use heapless::Vec;
use spin::Mutex;

const MAX_KEYBOARD_SNAPSHOTS: usize = 32;
const MAX_KEYBOARD_OUTPUT_EVENTS: usize = 256;
const KEYBOARD_OUTPUT_FLAG_PRESS: u32 = 1 << 0;
pub const KEYBOARD_OUTPUT_FLAG_SYNTHETIC: u32 = 1 << 1;
pub const KEYBOARD_OUTPUT_KIND_TEXT: u8 = 1;
pub const KEYBOARD_OUTPUT_KIND_KEY: u8 = 2;

pub const KEYBOARD_KEY_BACKSPACE: u16 = 1;
pub const KEYBOARD_KEY_TAB: u16 = 2;
pub const KEYBOARD_KEY_ENTER: u16 = 3;
pub const KEYBOARD_KEY_ESCAPE: u16 = 4;
pub const KEYBOARD_KEY_SPACE: u16 = 5;
pub const KEYBOARD_KEY_DELETE: u16 = 6;
pub const KEYBOARD_KEY_INSERT: u16 = 7;
pub const KEYBOARD_KEY_HOME: u16 = 8;
pub const KEYBOARD_KEY_END: u16 = 9;
pub const KEYBOARD_KEY_PAGE_UP: u16 = 10;
pub const KEYBOARD_KEY_PAGE_DOWN: u16 = 11;
pub const KEYBOARD_KEY_ARROW_UP: u16 = 12;
pub const KEYBOARD_KEY_ARROW_DOWN: u16 = 13;
pub const KEYBOARD_KEY_ARROW_LEFT: u16 = 14;
pub const KEYBOARD_KEY_ARROW_RIGHT: u16 = 15;
pub const KEYBOARD_KEY_F1: u16 = 101;
pub const KEYBOARD_KEY_F2: u16 = 102;
pub const KEYBOARD_KEY_F3: u16 = 103;
pub const KEYBOARD_KEY_F4: u16 = 104;
pub const KEYBOARD_KEY_F5: u16 = 105;
pub const KEYBOARD_KEY_F6: u16 = 106;
pub const KEYBOARD_KEY_F7: u16 = 107;
pub const KEYBOARD_KEY_F8: u16 = 108;
pub const KEYBOARD_KEY_F9: u16 = 109;
pub const KEYBOARD_KEY_F10: u16 = 110;
pub const KEYBOARD_KEY_F11: u16 = 111;
pub const KEYBOARD_KEY_F12: u16 = 112;

#[derive(Copy, Clone, Debug, Default)]
struct KeyboardSnapshot {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    modifiers: u8,
    reserved0: u8,
    reserved1: u16,
    keys: [u8; 6],
    ascii: [u8; 6],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosKeyboardState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub modifiers: u8,
    pub reserved0: u8,
    pub reserved1: u16,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
}

impl From<KeyboardSnapshot> for TrueosKeyboardState {
    fn from(value: KeyboardSnapshot) -> Self {
        Self {
            controller_id: value.controller_id,
            slot_id: value.slot_id,
            ep_target: value.ep_target,
            modifiers: value.modifiers,
            reserved0: value.reserved0,
            reserved1: value.reserved1,
            keys: value.keys,
            ascii: value.ascii,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosKeyboardOutputEvent {
    pub t_ms: u32,
    pub seq: u32,
    pub device_seq: u32,
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub modifiers: u8,
    pub kind: u8,
    pub utf8_len: u8,
    pub reserved0: u8,
    pub key_code: u16,
    pub reserved1: u16,
    pub codepoint: u32,
    pub utf8: [u8; 4],
    pub flags: u32,
}

const ZERO_KEYBOARD_OUTPUT_EVENT: TrueosKeyboardOutputEvent = TrueosKeyboardOutputEvent {
    t_ms: 0,
    seq: 0,
    device_seq: 0,
    controller_id: 0,
    slot_id: 0,
    ep_target: 0,
    modifiers: 0,
    kind: 0,
    utf8_len: 0,
    reserved0: 0,
    key_code: 0,
    reserved1: 0,
    codepoint: 0,
    utf8: [0; 4],
    flags: 0,
};

static KEYBOARD_SNAPSHOTS: Mutex<Vec<KeyboardSnapshot, MAX_KEYBOARD_SNAPSHOTS>> =
    Mutex::new(Vec::new());

#[derive(Copy, Clone, Debug)]
struct KeyboardOutputRing {
    buf: [TrueosKeyboardOutputEvent; MAX_KEYBOARD_OUTPUT_EVENTS],
    write_seq: u64,
}

impl KeyboardOutputRing {
    const fn new() -> Self {
        Self {
            buf: [ZERO_KEYBOARD_OUTPUT_EVENT; MAX_KEYBOARD_OUTPUT_EVENTS],
            write_seq: 0,
        }
    }
}

static KEYBOARD_OUTPUT_RING: Mutex<KeyboardOutputRing> = Mutex::new(KeyboardOutputRing::new());
static KEYBOARD_OUTPUT_POP_SEQ: Mutex<u64> = Mutex::new(0);

pub fn upsert_snapshot(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    modifiers: u8,
    keys: [u8; 6],
    ascii: [u8; 6],
) {
    let mut guard = KEYBOARD_SNAPSHOTS.lock();
    if let Some(existing) = guard.iter_mut().find(|snapshot| {
        snapshot.controller_id == controller_id
            && snapshot.slot_id == slot_id
            && snapshot.ep_target == ep_target
    }) {
        existing.modifiers = modifiers;
        existing.keys = keys;
        existing.ascii = ascii;
        return;
    }

    let snapshot = KeyboardSnapshot {
        controller_id,
        slot_id,
        ep_target,
        modifiers,
        reserved0: 0,
        reserved1: 0,
        keys,
        ascii,
    };
    if guard.push(snapshot).is_ok() {
        return;
    }

    if !guard.is_empty() {
        let last = guard.len() - 1;
        guard[last] = snapshot;
    }
}

pub fn remove_snapshots(controller_id: u32, slot_id: u32) -> bool {
    let mut guard = KEYBOARD_SNAPSHOTS.lock();
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

pub fn keyboard_count() -> u32 {
    KEYBOARD_SNAPSHOTS.lock().len() as u32
}

pub fn ordered_keyboard_snapshot() -> Vec<TrueosKeyboardState, MAX_KEYBOARD_SNAPSHOTS> {
    let guard = KEYBOARD_SNAPSHOTS.lock();
    let mut out = Vec::new();
    for snapshot in guard.iter().copied() {
        let _ = out.push(snapshot.into());
    }
    out
}

pub fn keyboard_state(keyboard_id: u32) -> Option<TrueosKeyboardState> {
    if keyboard_id == 0 {
        return None;
    }

    let guard = KEYBOARD_SNAPSHOTS.lock();
    let idx = (keyboard_id - 1) as usize;
    if idx >= guard.len() {
        return None;
    }
    Some(guard[idx].into())
}

pub fn keyboard_modifiers(keyboard_id: u32) -> Option<u8> {
    keyboard_state(keyboard_id).map(|state| state.modifiers)
}

pub fn keyboard_keys(keyboard_id: u32) -> Option<[u8; 6]> {
    keyboard_state(keyboard_id).map(|state| state.keys)
}

pub fn keyboard_ascii(keyboard_id: u32) -> Option<[u8; 6]> {
    keyboard_state(keyboard_id).map(|state| state.ascii)
}

#[inline]
fn key_is_down(keys: &[u8; 6], key: u8) -> bool {
    key != 0 && keys.iter().copied().any(|candidate| candidate == key)
}

#[inline]
fn ascii_was_emitted(emitted: &[u8; 6], ascii: u8) -> bool {
    ascii != 0 && emitted.iter().copied().any(|candidate| candidate == ascii)
}

#[inline]
fn key_code_was_emitted(emitted: &[u16; 6], key_code: u16) -> bool {
    key_code != 0
        && emitted
            .iter()
            .copied()
            .any(|candidate| candidate == key_code)
}

#[inline]
fn hid_boot_keycode_to_named_key(key: u8) -> Option<u16> {
    match key {
        0x28 => Some(KEYBOARD_KEY_ENTER),
        0x29 => Some(KEYBOARD_KEY_ESCAPE),
        0x2A => Some(KEYBOARD_KEY_BACKSPACE),
        0x2B => Some(KEYBOARD_KEY_TAB),
        0x2C => Some(KEYBOARD_KEY_SPACE),
        0x3A => Some(KEYBOARD_KEY_F1),
        0x3B => Some(KEYBOARD_KEY_F2),
        0x3C => Some(KEYBOARD_KEY_F3),
        0x3D => Some(KEYBOARD_KEY_F4),
        0x3E => Some(KEYBOARD_KEY_F5),
        0x3F => Some(KEYBOARD_KEY_F6),
        0x40 => Some(KEYBOARD_KEY_F7),
        0x41 => Some(KEYBOARD_KEY_F8),
        0x42 => Some(KEYBOARD_KEY_F9),
        0x43 => Some(KEYBOARD_KEY_F10),
        0x44 => Some(KEYBOARD_KEY_F11),
        0x45 => Some(KEYBOARD_KEY_F12),
        0x49 => Some(KEYBOARD_KEY_INSERT),
        0x4A => Some(KEYBOARD_KEY_HOME),
        0x4B => Some(KEYBOARD_KEY_PAGE_UP),
        0x4C => Some(KEYBOARD_KEY_DELETE),
        0x4D => Some(KEYBOARD_KEY_END),
        0x4E => Some(KEYBOARD_KEY_PAGE_DOWN),
        0x4F => Some(KEYBOARD_KEY_ARROW_RIGHT),
        0x50 => Some(KEYBOARD_KEY_ARROW_LEFT),
        0x51 => Some(KEYBOARD_KEY_ARROW_DOWN),
        0x52 => Some(KEYBOARD_KEY_ARROW_UP),
        _ => None,
    }
}

#[inline]
fn push_output_event(mut evt: TrueosKeyboardOutputEvent) {
    let mut ring = KEYBOARD_OUTPUT_RING.lock();
    ring.write_seq = ring.write_seq.wrapping_add(1);
    let seq = ring.write_seq;
    evt.seq = seq as u32;
    let idx = ((seq - 1) as usize) % MAX_KEYBOARD_OUTPUT_EVENTS;
    ring.buf[idx] = evt;
}

#[inline]
fn uptime_ms_u32() -> u32 {
    let ticks = embassy_time_driver::now() as u128;
    let hz = embassy_time_driver::TICK_HZ as u128;
    if hz == 0 {
        0
    } else {
        ((ticks * 1000u128) / hz) as u32
    }
}

#[inline]
fn key_code_default_codepoint(key_code: u16) -> Option<char> {
    match key_code {
        KEYBOARD_KEY_BACKSPACE => Some('\u{0008}'),
        KEYBOARD_KEY_TAB => Some('\t'),
        KEYBOARD_KEY_ENTER => Some('\n'),
        KEYBOARD_KEY_ESCAPE => Some('\u{001b}'),
        KEYBOARD_KEY_SPACE => Some(' '),
        _ => None,
    }
}

pub fn push_output_char(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    t_ms: u32,
    device_seq: u32,
    modifiers: u8,
    ch: char,
    flags: u32,
) {
    let mut utf8 = [0u8; 4];
    let utf8_len = ch.encode_utf8(&mut utf8).len() as u8;
    push_output_event(TrueosKeyboardOutputEvent {
        t_ms,
        seq: 0,
        device_seq,
        controller_id,
        slot_id,
        ep_target,
        modifiers,
        kind: KEYBOARD_OUTPUT_KIND_TEXT,
        utf8_len,
        reserved0: 0,
        key_code: 0,
        reserved1: 0,
        codepoint: ch as u32,
        utf8,
        flags,
    });
}

pub fn push_output_key(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    t_ms: u32,
    device_seq: u32,
    modifiers: u8,
    key_code: u16,
    codepoint: u32,
    flags: u32,
) {
    let mut utf8 = [0u8; 4];
    let utf8_len = if let Some(ch) = char::from_u32(codepoint) {
        ch.encode_utf8(&mut utf8).len() as u8
    } else {
        0
    };
    push_output_event(TrueosKeyboardOutputEvent {
        t_ms,
        seq: 0,
        device_seq,
        controller_id,
        slot_id,
        ep_target,
        modifiers,
        kind: KEYBOARD_OUTPUT_KIND_KEY,
        utf8_len,
        reserved0: 0,
        key_code,
        reserved1: 0,
        codepoint,
        utf8,
        flags,
    });
}

pub fn apply_report(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    t_ms: u32,
    device_seq: u32,
    modifiers: u8,
    keys: [u8; 6],
    ascii: [u8; 6],
) {
    let prev_keys = {
        let guard = KEYBOARD_SNAPSHOTS.lock();
        guard
            .iter()
            .find(|snapshot| {
                snapshot.controller_id == controller_id
                    && snapshot.slot_id == slot_id
                    && snapshot.ep_target == ep_target
            })
            .map(|snapshot| snapshot.keys)
            .unwrap_or([0; 6])
    };

    upsert_snapshot(controller_id, slot_id, ep_target, modifiers, keys, ascii);

    let mut emitted_ascii = [0u8; 6];
    let mut emitted_key_codes = [0u16; 6];
    for idx in 0..keys.len() {
        let key = keys[idx];
        let ch = ascii[idx];
        if key != 0 && !key_is_down(&prev_keys, key) {
            if let Some(key_code) = hid_boot_keycode_to_named_key(key) {
                if !key_code_was_emitted(&emitted_key_codes, key_code) {
                    let codepoint = key_code_default_codepoint(key_code)
                        .map(|ch| ch as u32)
                        .unwrap_or(0);
                    push_output_key(
                        controller_id,
                        slot_id,
                        ep_target,
                        t_ms,
                        device_seq,
                        modifiers,
                        key_code,
                        codepoint,
                        KEYBOARD_OUTPUT_FLAG_PRESS,
                    );
                    if key_code == KEYBOARD_KEY_F10 {
                        let seq = crate::aud::request_bassline_toggle();
                        crate::log_trace!("keyboard: F10 bassline toggle request seq={}\n", seq);
                    }
                    emitted_key_codes[idx] = key_code;
                }
            }
        }
        if key == 0
            || ch == 0
            || key_is_down(&prev_keys, key)
            || ascii_was_emitted(&emitted_ascii, ch)
        {
            continue;
        }
        emitted_ascii[idx] = ch;
        push_output_char(
            controller_id,
            slot_id,
            ep_target,
            t_ms,
            device_seq,
            modifiers,
            ch as char,
            KEYBOARD_OUTPUT_FLAG_PRESS,
        );
    }
}

pub fn read_output_events_since(
    read_seq: u64,
    out: &mut [TrueosKeyboardOutputEvent],
) -> (u64, u32, usize) {
    let ring = KEYBOARD_OUTPUT_RING.lock();
    if ring.write_seq == 0 || out.is_empty() {
        return (read_seq, 0, 0);
    }

    let cap = MAX_KEYBOARD_OUTPUT_EVENTS as u64;
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
        let idx = ((seq - 1) as usize) % MAX_KEYBOARD_OUTPUT_EVENTS;
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

pub fn pop_output_event() -> Option<TrueosKeyboardOutputEvent> {
    let mut seq = KEYBOARD_OUTPUT_POP_SEQ.lock();
    let mut one = [ZERO_KEYBOARD_OUTPUT_EVENT; 1];
    let (next_seq, _dropped, wrote) = read_output_events_since(*seq, &mut one);
    if wrote == 0 {
        return None;
    }
    *seq = next_seq;
    Some(one[0])
}

pub fn inject_text(slot_id: u32, text: &str, flags: u32) -> usize {
    if slot_id == 0 || text.is_empty() {
        return 0;
    }

    let t_ms = uptime_ms_u32();
    let mut wrote = 0usize;
    for ch in text.chars() {
        push_output_char(0, slot_id, 0, t_ms, 0, 0, ch, flags | KEYBOARD_OUTPUT_FLAG_PRESS);
        wrote += 1;
    }
    wrote
}

pub fn inject_key(slot_id: u32, codepoint: u32, key_code: u16, modifiers: u8, flags: u32) -> bool {
    if slot_id == 0 {
        return false;
    }
    if codepoint == 0 && key_code == 0 {
        return false;
    }

    let effective_codepoint = if codepoint != 0 {
        codepoint
    } else if modifiers == 0 {
        key_code_default_codepoint(key_code)
            .map(|ch| ch as u32)
            .unwrap_or(0)
    } else {
        0
    };

    push_output_key(
        0,
        slot_id,
        0,
        uptime_ms_u32(),
        0,
        modifiers,
        key_code,
        effective_codepoint,
        flags | KEYBOARD_OUTPUT_FLAG_PRESS,
    );
    true
}
