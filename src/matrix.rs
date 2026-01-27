#![allow(dead_code)]

use alloc::vec::Vec as AVec;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use heapless::{Deque, String, Vec as HVec};
use spin::Mutex;

pub const MAX_SLOTS: usize = 8;
pub const MAX_LINES: usize = 64;
pub const TITLE_LEN: usize = 32;
pub const LINE_LEN: usize = 96;

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
    pub blob: AVec<u8>,
}

impl SlotData {
    pub const fn empty() -> Self {
        Self {
            state: SlotState::Done,
            title: String::new(),
            lines: Deque::new(),
            blob: AVec::new(),
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

#[inline]
fn push_line_into_lines(lines: &mut Deque<String<LINE_LEN>, MAX_LINES>, line: &str) {
    let mut s: String<LINE_LEN> = String::new();
    for ch in line.chars() {
        if ch == '\r' || ch == '\n' {
            continue;
        }
        if s.push(ch).is_err() {
            break;
        }
    }

    if lines.is_full() {
        let _ = lines.pop_front();
    }
    let _ = lines.push_back(s);
}

#[inline]
fn refresh_preview_locked(data: &mut SlotData) {
    data.lines.clear();
    if data.blob.is_empty() {
        return;
    }

    let blob = data.blob.as_slice();
    let lines = &mut data.lines;

    if let Ok(text) = core::str::from_utf8(blob) {
        for line in text.split('\n') {
            push_line_into_lines(lines, line.trim_end_matches('\r'));
        }
        return;
    }

    // Lossy UTF-8 decode:
    // - preserves as much readable content as possible
    // - replaces invalid sequences with U+FFFD
    // - keeps the same newline splitting behavior as the UTF-8 fast path
    let mut cur: String<LINE_LEN> = String::new();
    let mut i: usize = 0;
    while i < blob.len() {
        match core::str::from_utf8(&blob[i..]) {
            Ok(s) => {
                for ch in s.chars() {
                    match ch {
                        '\r' => {}
                        '\n' => {
                            if lines.is_full() {
                                let _ = lines.pop_front();
                            }
                            let _ = lines.push_back(cur);
                            cur = String::new();
                        }
                        _ => {
                            let _ = cur.push(ch);
                        }
                    }
                }
                break;
            }
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                if valid_up_to != 0 {
                    if let Ok(s) = core::str::from_utf8(&blob[i..i + valid_up_to]) {
                        for ch in s.chars() {
                            match ch {
                                '\r' => {}
                                '\n' => {
                                    if lines.is_full() {
                                        let _ = lines.pop_front();
                                    }
                                    let _ = lines.push_back(cur);
                                    cur = String::new();
                                }
                                _ => {
                                    let _ = cur.push(ch);
                                }
                            }
                        }
                    }
                    i += valid_up_to;
                }

                // Skip the invalid byte sequence and insert a replacement char.
                let skip = e.error_len().unwrap_or(1).max(1);
                i = i.saturating_add(skip);
                let _ = cur.push('\u{FFFD}');
            }
        }
    }

    if !cur.is_empty() {
        if lines.is_full() {
            let _ = lines.pop_front();
        }
        let _ = lines.push_back(cur);
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

    push_line_into_lines(&mut data.lines, line);
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

/// Overwrites the slot blob with `bytes` (no size cap).
///
/// Returns `false` only if the slot is missing.
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
    data.blob.extend_from_slice(bytes);
    true
}

/// Moves an owned blob into the slot (no copy). Returns false if slot missing.
pub fn set_blob_owned(slot_id: u8, blob: AVec<u8>) -> bool {
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
    data.blob = blob;
    true
}

/// Moves an owned blob into the slot and updates preview lines from it.
pub fn set_blob_owned_with_preview(slot_id: u8, blob: AVec<u8>) -> bool {
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

    data.blob = blob;
    refresh_preview_locked(&mut data);
    true
}

/// Takes ownership of the slot blob, leaving it empty.
pub fn take_blob(slot_id: u8) -> Option<AVec<u8>> {
    let Some(slot) = slot_ref(slot_id) else {
        return None;
    };
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    Some(core::mem::take(&mut data.blob))
}

/// Clones the slot blob.
pub fn blob_snapshot(slot_id: u8) -> Option<AVec<u8>> {
    with_slot(slot_id, |s| s.blob.clone())
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
pub fn collect_symbols(out: &mut HVec<(u8, SlotState), MAX_SLOTS>) {
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

/// Rebuild preview lines from the current slot blob.
pub fn refresh_preview(slot_id: u8) -> bool {
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
    refresh_preview_locked(&mut data);
    true
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
