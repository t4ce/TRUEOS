#![allow(dead_code)]

use core::fmt::Write;

use heapless::{Deque, String, Vec};
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

pub struct Slot {
    used: bool,
    pub state: SlotState,
    pub title: String<TITLE_LEN>,
    pub lines: Deque<String<LINE_LEN>, MAX_LINES>,
}

impl Slot {
    pub const fn empty() -> Self {
        Self {
            used: false,
            state: SlotState::Done,
            title: String::new(),
            lines: Deque::new(),
        }
    }

    fn reset(&mut self) {
        self.used = true;
        self.state = SlotState::Running;
        self.title.clear();
        self.lines.clear();
    }

    fn free(&mut self) {
        self.used = false;
        self.state = SlotState::Done;
        self.title.clear();
        self.lines.clear();
    }
}

pub struct Matrix {
    slots: [Slot; MAX_SLOTS],
}

impl Matrix {
    pub const fn new() -> Self {
        Self {
            slots: [const { Slot::empty() }; MAX_SLOTS],
        }
    }

    fn alloc_slot(&mut self, title: &str) -> Option<u8> {
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if !slot.used {
                slot.reset();
                for ch in title.chars() {
                    if slot.title.push(ch).is_err() {
                        break;
                    }
                }
                return Some(idx as u8);
            }
        }
        None
    }

    fn free_slot(&mut self, slot_id: u8) -> bool {
        let idx = slot_id as usize;
        if idx >= MAX_SLOTS {
            return false;
        }
        if !self.slots[idx].used {
            return false;
        }
        self.slots[idx].free();
        true
    }

    fn push_line(&mut self, slot_id: u8, line: &str) {
        let idx = slot_id as usize;
        if idx >= MAX_SLOTS {
            return;
        }
        if !self.slots[idx].used {
            return;
        }
        let slot = &mut self.slots[idx];

        let mut s: String<LINE_LEN> = String::new();
        for ch in line.chars() {
            if ch == '\r' || ch == '\n' {
                continue;
            }
            if s.push(ch).is_err() {
                break;
            }
        }

        if slot.lines.is_full() {
            let _ = slot.lines.pop_front();
        }
        let _ = slot.lines.push_back(s);
    }

    fn set_state(&mut self, slot_id: u8, state: SlotState) {
        let idx = slot_id as usize;
        if idx >= MAX_SLOTS {
            return;
        }
        if !self.slots[idx].used {
            return;
        }
        self.slots[idx].state = state;
    }
}

static MATRIX: Mutex<Matrix> = Mutex::new(Matrix::new());

pub fn alloc_slot(title: &str) -> Option<u8> {
    MATRIX.lock().alloc_slot(title)
}

pub fn free_slot(slot_id: u8) -> bool {
    MATRIX.lock().free_slot(slot_id)
}

pub fn push_line(slot_id: u8, line: &str) {
    MATRIX.lock().push_line(slot_id, line)
}

pub fn set_state(slot_id: u8, state: SlotState) {
    MATRIX.lock().set_state(slot_id, state)
}

pub fn with_slot<R>(slot_id: u8, f: impl FnOnce(&Slot) -> R) -> Option<R> {
    let guard = MATRIX.lock();
    let idx = slot_id as usize;
    if idx >= MAX_SLOTS {
        return None;
    }
    let slot = &guard.slots[idx];
    if !slot.used {
        return None;
    }
    Some(f(slot))
}

/// Formats a compact list like "§1 §3" for all allocated slots.
pub fn format_symbols(out: &mut String<64>) {
    out.clear();
    let guard = MATRIX.lock();
    for (idx, slot) in guard.slots.iter().enumerate() {
        if !slot.used {
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
    let guard = MATRIX.lock();
    for (idx, slot) in guard.slots.iter().enumerate() {
        if !slot.used {
            continue;
        }
        let _ = out.push((idx as u8 + 1, slot.state));
    }
}

pub fn list_slots(out: &mut String<512>) {
    out.clear();
    let guard = MATRIX.lock();
    for (idx, slot) in guard.slots.iter().enumerate() {
        if !slot.used {
            continue;
        }
        let _ = write!(
            out,
            "#{} {:?} {}\r\n",
            idx + 1,
            slot.state,
            slot.title.as_str()
        );
    }
    if out.is_empty() {
        let _ = out.push_str("(no async jobs)\r\n");
    }
}

pub fn dump_slot(out: &mut String<1024>, slot_id: u8) -> bool {
    out.clear();
    let guard = MATRIX.lock();
    let idx = slot_id as usize;
    if idx >= MAX_SLOTS {
        return false;
    }
    let slot = &guard.slots[idx];
    if !slot.used {
        return false;
    }

    let _ = write!(
        out,
        "#{} {:?} {}\r\n",
        idx + 1,
        slot.state,
        slot.title.as_str()
    );
    for line in slot.lines.iter() {
        let _ = write!(out, "{}\r\n", line.as_str());
    }
    true
}
