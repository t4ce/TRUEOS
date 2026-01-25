#![allow(dead_code)]

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use heapless::{Deque, String, Vec};
use spin::Mutex;

pub const MAX_SLOTS: usize = 8;
pub const MAX_LINES: usize = 64;
pub const TITLE_LEN: usize = 32;
pub const LINE_LEN: usize = 96;
pub const BLOB_CAP: usize = 512;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SlotState {
    Running,
    Done,
    Failed,
    Cancelled,
}

pub struct SlotData {
    pub state: SlotState,
    pub title: String<TITLE_LEN>,
    pub lines: Deque<String<LINE_LEN>, MAX_LINES>,
    pub blob: Vec<u8, BLOB_CAP>,
}

impl SlotData {
    pub const fn empty() -> Self {
        Self {
            state: SlotState::Done,
            title: String::new(),
            lines: Deque::new(),
            blob: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.state = SlotState::Running;
        self.title.clear();
        self.lines.clear();
        self.blob.clear();
    }

    fn free(&mut self) {
        self.state = SlotState::Done;
        self.title.clear();
        self.lines.clear();
        self.blob.clear();
    }
}

pub struct Slot {
    used: AtomicBool,
    data: Mutex<SlotData>,
}

impl Slot {
    pub const fn empty() -> Self {
        Self {
            used: AtomicBool::new(false),
            data: Mutex::new(SlotData::empty()),
        }
    }
}

static SLOTS: [Slot; MAX_SLOTS] = [const { Slot::empty() }; MAX_SLOTS];

#[inline]
fn slot_ref(slot_id: u8) -> Option<&'static Slot> {
    let idx = slot_id as usize;
    if idx >= MAX_SLOTS {
        return None;
    }
    Some(&SLOTS[idx])
}

pub fn alloc_slot(title: &str) -> Option<u8> {
    for (idx, slot) in SLOTS.iter().enumerate() {
        if slot
            .used
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            continue;
        }

        // We own the slot now; initialize it.
        let mut data = slot.data.lock();
        data.reset();
        for ch in title.chars() {
            if data.title.push(ch).is_err() {
                break;
            }
        }
        return Some(idx as u8);
    }
    None
}

pub fn free_slot(slot_id: u8) -> bool {
    let Some(slot) = slot_ref(slot_id) else {
        return false;
    };

    if slot
        .used
        .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }

    // Best-effort cleanup.
    let mut data = slot.data.lock();
    data.free();
    true
}

pub fn push_line(slot_id: u8, line: &str) {
    let Some(slot) = slot_ref(slot_id) else {
        return;
    };
    if !slot.used.load(Ordering::Acquire) {
        return;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return;
    }

    let mut s: String<LINE_LEN> = String::new();
    for ch in line.chars() {
        if ch == '\r' || ch == '\n' {
            continue;
        }
        if s.push(ch).is_err() {
            break;
        }
    }

    if data.lines.is_full() {
        let _ = data.lines.pop_front();
    }
    let _ = data.lines.push_back(s);
}

pub fn clear_lines(slot_id: u8) {
    let Some(slot) = slot_ref(slot_id) else {
        return;
    };
    if !slot.used.load(Ordering::Acquire) {
        return;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return;
    }
    data.lines.clear();
}

/// Stores up to `BLOB_CAP` bytes into the slot, overwriting any previous blob.
///
/// Returns `true` if all bytes fit, `false` if truncated or slot missing.
pub fn set_blob(slot_id: u8, bytes: &[u8]) -> bool {
    let Some(slot) = slot_ref(slot_id) else {
        return false;
    };
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }

    data.blob.clear();
    for &b in bytes.iter() {
        if data.blob.push(b).is_err() {
            return false;
        }
    }
    true
}

pub fn set_state(slot_id: u8, state: SlotState) {
    let Some(slot) = slot_ref(slot_id) else {
        return;
    };
    if !slot.used.load(Ordering::Acquire) {
        return;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return;
    }
    data.state = state;
}

pub fn with_slot<R>(slot_id: u8, f: impl FnOnce(&SlotData) -> R) -> Option<R> {
    let Some(slot) = slot_ref(slot_id) else {
        return None;
    };
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    let data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    Some(f(&data))
}

/// Formats a compact list like "§1 §3" for all allocated slots.
pub fn format_symbols(out: &mut String<64>) {
    out.clear();
    for (idx, slot) in SLOTS.iter().enumerate() {
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        if !out.is_empty() {
            let _ = out.push(' ');
        }
        let _ = write!(out, "§{}", idx + 1);
    }
}

/// Collects all allocated slots as (1-based-id, state) pairs in ascending order.
pub fn collect_symbols(out: &mut Vec<(u8, SlotState), MAX_SLOTS>) {
    out.clear();
    for (idx, slot) in SLOTS.iter().enumerate() {
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        let data = slot.data.lock();
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        let _ = out.push((idx as u8 + 1, data.state));
    }
}

pub fn list_slots(out: &mut String<512>) {
    out.clear();
    for (idx, slot) in SLOTS.iter().enumerate() {
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        let data = slot.data.lock();
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        let _ = write!(out, "#{} {:?} {}\r\n", idx + 1, data.state, data.title.as_str());
    }
    if out.is_empty() {
        let _ = out.push_str("(no async jobs)\r\n");
    }
}

pub fn dump_slot(out: &mut String<1024>, slot_id: u8) -> bool {
    out.clear();
    let idx = slot_id as usize;
    if idx >= MAX_SLOTS {
        return false;
    }
    let slot = &SLOTS[idx];
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    let data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }

    let _ = write!(out, "#{} {:?} {}\r\n", idx + 1, data.state, data.title.as_str());
    for line in data.lines.iter() {
        let _ = write!(out, "{}\r\n", line.as_str());
    }
    true
}
