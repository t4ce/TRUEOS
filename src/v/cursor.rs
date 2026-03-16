#![allow(dead_code)]

use heapless::Vec;
use spin::Mutex;

const MAX_CURSOR_SNAPSHOTS: usize = 32;
const HID_KIND_MOUSE: u8 = 2;
const HID_KIND_TABLET: u8 = 3;

#[derive(Copy, Clone, Debug)]
struct CursorSnapshot {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u8,
    x: f64,
    y: f64,
    buttons_down: u32,
}

static CURSOR_SNAPSHOTS: Mutex<Vec<CursorSnapshot, MAX_CURSOR_SNAPSHOTS>> = Mutex::new(Vec::new());

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

#[inline]
fn snapshot_order_match(snapshot: &CursorSnapshot, phase: u8) -> bool {
    match phase {
        0 => snapshot.hid_kind == HID_KIND_MOUSE,
        1 => snapshot.hid_kind == HID_KIND_TABLET,
        _ => snapshot.hid_kind != HID_KIND_MOUSE && snapshot.hid_kind != HID_KIND_TABLET,
    }
}

pub fn upsert_snapshot(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u8,
    x: f64,
    y: f64,
    buttons_down: u32,
) {
    let mut guard = CURSOR_SNAPSHOTS.lock();
    if let Some(existing) = guard.iter_mut().find(|snapshot| {
        snapshot.controller_id == controller_id
            && snapshot.slot_id == slot_id
            && snapshot.ep_target == ep_target
            && snapshot.hid_kind == hid_kind
    }) {
        existing.x = clamp01(x);
        existing.y = clamp01(y);
        existing.buttons_down = buttons_down;
        return;
    }

    let snapshot = CursorSnapshot {
        controller_id,
        slot_id,
        ep_target,
        hid_kind,
        x: clamp01(x),
        y: clamp01(y),
        buttons_down,
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
    let mut guard = CURSOR_SNAPSHOTS.lock();
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

pub fn mouse_cursor_snapshot() -> Vec<(f64, f64), MAX_CURSOR_SNAPSHOTS> {
    let guard = CURSOR_SNAPSHOTS.lock();
    let mut out = Vec::new();
    for snapshot in guard.iter() {
        if snapshot.hid_kind != HID_KIND_MOUSE {
            continue;
        }
        let _ = out.push((snapshot.x, snapshot.y));
    }
    out
}

pub fn mouse_cursor_snapshot_with_buttons() -> Vec<(f64, f64, u32), MAX_CURSOR_SNAPSHOTS> {
    let guard = CURSOR_SNAPSHOTS.lock();
    let mut out = Vec::new();
    for snapshot in guard.iter() {
        if snapshot.hid_kind != HID_KIND_MOUSE {
            continue;
        }
        let _ = out.push((snapshot.x, snapshot.y, snapshot.buttons_down));
    }
    out
}

pub fn tablet_cursor_snapshot() -> Vec<(f64, f64), MAX_CURSOR_SNAPSHOTS> {
    let guard = CURSOR_SNAPSHOTS.lock();
    let mut out = Vec::new();
    for snapshot in guard.iter() {
        if snapshot.hid_kind != HID_KIND_TABLET {
            continue;
        }
        let _ = out.push((snapshot.x, snapshot.y));
    }
    out
}

pub fn ordered_cursor_snapshot() -> Vec<(f64, f64), MAX_CURSOR_SNAPSHOTS> {
    let guard = CURSOR_SNAPSHOTS.lock();
    let mut out = Vec::new();
    for phase in 0..=2u8 {
        for snapshot in guard.iter() {
            if !snapshot_order_match(snapshot, phase) {
                continue;
            }
            let _ = out.push((snapshot.x, snapshot.y));
        }
    }
    out
}

pub fn cursor_pos(cursor_id: u32) -> Option<(f64, f64)> {
    if cursor_id == 0 {
        return None;
    }

    let guard = CURSOR_SNAPSHOTS.lock();
    let mut remaining = (cursor_id - 1) as usize;
    for phase in 0..=2u8 {
        for snapshot in guard.iter() {
            if !snapshot_order_match(snapshot, phase) {
                continue;
            }
            if remaining == 0 {
                return Some((snapshot.x, snapshot.y));
            }
            remaining -= 1;
        }
    }
    None
}

pub fn cursor_buttons(cursor_id: u32) -> Option<u32> {
    if cursor_id == 0 {
        return None;
    }

    let guard = CURSOR_SNAPSHOTS.lock();
    let mut remaining = (cursor_id - 1) as usize;
    for phase in 0..=2u8 {
        for snapshot in guard.iter() {
            if !snapshot_order_match(snapshot, phase) {
                continue;
            }
            if remaining == 0 {
                return Some(snapshot.buttons_down);
            }
            remaining -= 1;
        }
    }
    None
}
