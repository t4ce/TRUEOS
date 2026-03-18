use alloc::collections::VecDeque;
use alloc::string::String as AllocString;
use alloc::vec::Vec;

use heapless::String as HString;
use spin::Once;

use super::{LineSource, TranscriptEntry};

pub(crate) const MATRIX_SLOT_ID_MAX: usize = 3;
const DEFAULT_MATRIX_SLOT_LINE_CAP: usize = 512;

pub(crate) type MatrixSlotId = HString<MATRIX_SLOT_ID_MAX>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatrixSlotActivity {
    Idle,
    Running,
}

#[derive(Clone)]
pub(crate) struct MatrixSlotView {
    pub(crate) id: MatrixSlotId,
    pub(crate) selected: bool,
    pub(crate) activity: MatrixSlotActivity,
}

#[derive(Clone)]
struct MatrixSlot {
    id: MatrixSlotId,
    lines: VecDeque<TranscriptEntry>,
    activity: MatrixSlotActivity,
}

struct MatrixState {
    slots: Vec<MatrixSlot>,
    uart_active: MatrixSlotId,
    net_active: MatrixSlotId,
    revision: u64,
}

static MATRIX_STATE: Once<spin::Mutex<MatrixState>> = Once::new();

fn state() -> &'static spin::Mutex<MatrixState> {
    MATRIX_STATE.call_once(|| {
        let mut initial = MatrixState {
            slots: Vec::new(),
            uart_active: default_slot_id(),
            net_active: default_slot_id(),
            revision: 1,
        };
        let default_id = default_slot_id();
        let _ = ensure_slot_index(&mut initial.slots, &default_id);
        spin::Mutex::new(initial)
    })
}

fn default_slot_id() -> MatrixSlotId {
    MatrixSlotId::new()
}

fn normalize_slot_id(requested: &str) -> MatrixSlotId {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return default_slot_id();
    }

    let mut id = MatrixSlotId::new();
    for ch in trimmed.chars() {
        if id.push(ch).is_err() {
            break;
        }
    }
    id
}

fn ensure_slot_index(slots: &mut Vec<MatrixSlot>, id: &MatrixSlotId) -> usize {
    if let Some(idx) = slots.iter().position(|slot| slot.id == *id) {
        return idx;
    }

    slots.push(MatrixSlot {
        id: id.clone(),
        lines: VecDeque::new(),
        activity: MatrixSlotActivity::Idle,
    });
    slots.len() - 1
}

fn bump_revision(state: &mut MatrixState) {
    state.revision = state.revision.wrapping_add(1);
}

fn is_default_slot_id(id: &MatrixSlotId) -> bool {
    id.is_empty()
}

fn active_slot_id_ref(state: &MatrixState, output_mask: u8) -> &MatrixSlotId {
    if (output_mask & super::OUTPUT_NET_TCP_MASK) != 0 {
        &state.net_active
    } else {
        &state.uart_active
    }
}

fn active_slot_id_mut(state: &mut MatrixState, output_mask: u8) -> &mut MatrixSlotId {
    if (output_mask & super::OUTPUT_NET_TCP_MASK) != 0 {
        &mut state.net_active
    } else {
        &mut state.uart_active
    }
}

fn push_line(slot: &mut MatrixSlot, source: LineSource, text: &str) {
    if slot.lines.len() >= DEFAULT_MATRIX_SLOT_LINE_CAP {
        let _ = slot.lines.pop_front();
    }
    slot.lines.push_back(TranscriptEntry {
        source,
        text: AllocString::from(text),
    });
}

pub(crate) fn active_slot_id(output_mask: u8) -> MatrixSlotId {
    active_slot_id_ref(&state().lock(), output_mask).clone()
}

pub(crate) fn switch_active_slot(output_mask: u8, requested: &str) -> MatrixSlotId {
    let next_id = normalize_slot_id(requested);
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, &next_id);
    *active_slot_id_mut(&mut guard, output_mask) = next_id.clone();
    let _ = idx;
    bump_revision(&mut guard);
    next_id
}

pub(crate) fn active_lines(output_mask: u8) -> VecDeque<TranscriptEntry> {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    guard.slots[idx].lines.clone()
}

pub(crate) fn record_line_for_output(
    output_mask: u8,
    source: LineSource,
    text: &str,
) -> MatrixSlotId {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    push_line(&mut guard.slots[idx], source, text);
    bump_revision(&mut guard);
    slot_id
}

pub(crate) fn record_line_in_default(source: LineSource, text: &str) {
    let mut guard = state().lock();
    let default_id = default_slot_id();
    let idx = ensure_slot_index(&mut guard.slots, &default_id);
    push_line(&mut guard.slots[idx], source, text);
    bump_revision(&mut guard);
}

pub(crate) fn record_line_in_slot(slot_id: &MatrixSlotId, source: LineSource, text: &str) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    push_line(&mut guard.slots[idx], source, text);
    bump_revision(&mut guard);
}

pub(crate) fn set_slot_activity(slot_id: &MatrixSlotId, activity: MatrixSlotActivity) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    if guard.slots[idx].activity != activity {
        guard.slots[idx].activity = activity;
        bump_revision(&mut guard);
    }
}

pub(crate) fn slot_views(output_mask: u8) -> Vec<MatrixSlotView> {
    let mut guard = state().lock();
    let selected = active_slot_id_ref(&guard, output_mask).clone();
    let _ = ensure_slot_index(&mut guard.slots, &selected);

    let mut out = Vec::new();
    for slot in &guard.slots {
        if is_default_slot_id(&slot.id) {
            continue;
        }
        out.push(MatrixSlotView {
            id: slot.id.clone(),
            selected: slot.id == selected,
            activity: slot.activity,
        });
    }
    out
}

pub(crate) fn revision() -> u64 {
    state().lock().revision
}

pub(crate) fn history_total_lines() -> usize {
    let mut guard = state().lock();
    let default_id = default_slot_id();
    let idx = ensure_slot_index(&mut guard.slots, &default_id);
    guard.slots[idx].lines.len()
}

pub(crate) fn history_lines_text(start_line: usize, max_lines: usize) -> AllocString {
    if max_lines == 0 {
        return AllocString::new();
    }

    let mut guard = state().lock();
    let default_id = default_slot_id();
    let idx = ensure_slot_index(&mut guard.slots, &default_id);
    let slot = &guard.slots[idx];
    if start_line >= slot.lines.len() {
        return AllocString::new();
    }

    let mut out = AllocString::new();
    for (idx, line) in slot.lines.iter().skip(start_line).take(max_lines).enumerate() {
        if idx != 0 {
            out.push('\n');
        }
        out.push_str(line.text.as_str());
    }
    out
}
