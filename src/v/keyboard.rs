use heapless::Vec;
use spin::Mutex;

const MAX_KEYBOARD_SNAPSHOTS: usize = 32;
const MAX_KEYBOARD_OUTPUT_EVENTS: usize = 256;
const KEYBOARD_OUTPUT_FLAG_PRESS: u32 = 1 << 0;

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
    pub utf8_len: u8,
    pub reserved0: u16,
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
    utf8_len: 0,
    reserved0: 0,
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
fn push_output_event(mut evt: TrueosKeyboardOutputEvent) {
    let mut ring = KEYBOARD_OUTPUT_RING.lock();
    ring.write_seq = ring.write_seq.wrapping_add(1);
    let seq = ring.write_seq;
    evt.seq = seq as u32;
    let idx = ((seq - 1) as usize) % MAX_KEYBOARD_OUTPUT_EVENTS;
    ring.buf[idx] = evt;
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
        utf8_len,
        reserved0: 0,
        codepoint: ch as u32,
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
    for idx in 0..keys.len() {
        let key = keys[idx];
        let ch = ascii[idx];
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
