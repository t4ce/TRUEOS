use alloc::collections::VecDeque;
use alloc::string::String as AllocString;
use alloc::vec::Vec;

use heapless::String as HString;
use spin::Once;

use super::{LineSource, TranscriptEntry};

pub(crate) const MATRIX_SLOT_ID_MAX: usize = 3;
const DEFAULT_MATRIX_SLOT_LINE_CAP: usize = 512;
pub(crate) const DEFAULT_MATRIX_SLOT_LINE_WIDTH: usize = 180;
const USER_INPUT_RECORD_CAP: usize = 256;
const LIVE_USER_INPUT_CAP: usize = 10;

pub(crate) type MatrixSlotId = HString<MATRIX_SLOT_ID_MAX>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatrixSlotActivity {
    Idle,
    Session,
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
    running_count: usize,
    line_width: usize,
}

#[derive(Clone)]
pub(crate) struct LiveUserInputEntry {
    pub(crate) text: AllocString,
    pub(crate) count: u64,
}

struct MatrixState {
    slots: Vec<MatrixSlot>,
    uart_active: MatrixSlotId,
    net_active: MatrixSlotId,
    ui2_active: MatrixSlotId,
    user_input_record: VecDeque<AllocString>,
    live_user_input_record: VecDeque<LiveUserInputEntry>,
    revision: u64,
}

static MATRIX_STATE: Once<spin::Mutex<MatrixState>> = Once::new();

fn state() -> &'static spin::Mutex<MatrixState> {
    MATRIX_STATE.call_once(|| {
        let mut initial = MatrixState {
            slots: Vec::new(),
            uart_active: default_slot_id(),
            net_active: default_slot_id(),
            ui2_active: default_slot_id(),
            user_input_record: VecDeque::new(),
            live_user_input_record: VecDeque::new(),
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
    let trimmed = trimmed.strip_prefix('§').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('§').unwrap_or(trimmed);
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

pub(crate) fn slot_id_from_name(requested: &str) -> MatrixSlotId {
    normalize_slot_id(requested)
}

fn ensure_slot_index(slots: &mut Vec<MatrixSlot>, id: &MatrixSlotId) -> usize {
    if let Some(idx) = slots.iter().position(|slot| slot.id == *id) {
        return idx;
    }

    slots.push(MatrixSlot {
        id: id.clone(),
        lines: VecDeque::new(),
        activity: MatrixSlotActivity::Idle,
        running_count: 0,
        line_width: DEFAULT_MATRIX_SLOT_LINE_WIDTH,
    });
    slots.len() - 1
}

fn bump_revision(state: &mut MatrixState) {
    state.revision = state.revision.wrapping_add(1);
}

fn active_slot_id_ref(state: &MatrixState, output_mask: u8) -> &MatrixSlotId {
    if (output_mask & super::OUTPUT_NET_TCP_MASK) != 0 {
        &state.net_active
    } else if (output_mask & super::OUTPUT_UI2_MASK) != 0 {
        &state.ui2_active
    } else {
        &state.uart_active
    }
}

fn active_slot_id_mut(state: &mut MatrixState, output_mask: u8) -> &mut MatrixSlotId {
    if (output_mask & super::OUTPUT_NET_TCP_MASK) != 0 {
        &mut state.net_active
    } else if (output_mask & super::OUTPUT_UI2_MASK) != 0 {
        &mut state.ui2_active
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

fn push_user_input_record(state: &mut MatrixState, text: &str) {
    if state.user_input_record.len() >= USER_INPUT_RECORD_CAP {
        let _ = state.user_input_record.pop_front();
    }
    state.user_input_record.push_back(AllocString::from(text));
}

fn push_live_user_input_record(state: &mut MatrixState, text: &str) {
    if let Some(existing) = state
        .live_user_input_record
        .iter_mut()
        .find(|entry| entry.text.as_str() == text)
    {
        existing.count = existing.count.saturating_add(1);
        return;
    }

    if state.live_user_input_record.len() >= LIVE_USER_INPUT_CAP {
        let _ = state.live_user_input_record.pop_front();
    }
    state.live_user_input_record.push_back(LiveUserInputEntry {
        text: AllocString::from(text),
        count: 1,
    });
}

fn visible_activity(slot: &MatrixSlot) -> MatrixSlotActivity {
    if slot.running_count > 0 {
        MatrixSlotActivity::Running
    } else {
        slot.activity
    }
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

pub(crate) fn free_slot(requested: &str) -> MatrixSlotId {
    let freed_id = normalize_slot_id(requested);
    let default_id = default_slot_id();
    let mut guard = state().lock();
    let mut changed = false;

    if freed_id == default_id {
        let idx = ensure_slot_index(&mut guard.slots, &default_id);
        let slot = &mut guard.slots[idx];
        if !slot.lines.is_empty()
            || slot.activity != MatrixSlotActivity::Idle
            || slot.running_count != 0
            || slot.line_width != DEFAULT_MATRIX_SLOT_LINE_WIDTH
        {
            slot.lines.clear();
            slot.activity = MatrixSlotActivity::Idle;
            slot.running_count = 0;
            slot.line_width = DEFAULT_MATRIX_SLOT_LINE_WIDTH;
            changed = true;
        }
    } else if let Some(idx) = guard.slots.iter().position(|slot| slot.id == freed_id) {
        let _ = guard.slots.remove(idx);
        if guard.uart_active == freed_id {
            guard.uart_active = default_id.clone();
        }
        if guard.net_active == freed_id {
            guard.net_active = default_id.clone();
        }
        if guard.ui2_active == freed_id {
            guard.ui2_active = default_id.clone();
        }
        changed = true;
    }

    let _ = ensure_slot_index(&mut guard.slots, &default_id);
    if changed {
        bump_revision(&mut guard);
    }
    freed_id
}

pub(crate) fn active_lines(output_mask: u8) -> VecDeque<TranscriptEntry> {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    guard.slots[idx].lines.clone()
}

pub(crate) fn active_line_width(output_mask: u8) -> usize {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    guard.slots[idx].line_width
}

pub(crate) fn set_active_line_width(output_mask: u8, width: usize) {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    if guard.slots[idx].line_width != width {
        guard.slots[idx].line_width = width;
        bump_revision(&mut guard);
    }
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

pub(crate) fn record_user_input(text: &str) {
    let mut guard = state().lock();
    push_user_input_record(&mut guard, text);
    push_live_user_input_record(&mut guard, text);
}

pub(crate) fn live_user_input_record() -> Vec<LiveUserInputEntry> {
    state()
        .lock()
        .live_user_input_record
        .iter()
        .cloned()
        .collect()
}

pub(crate) fn take_user_input_record() -> Vec<AllocString> {
    let mut guard = state().lock();
    guard.user_input_record.drain(..).collect()
}

pub(crate) fn restore_user_input_record(entries: Vec<AllocString>) {
    if entries.is_empty() {
        return;
    }

    let mut guard = state().lock();
    for entry in entries.into_iter().rev() {
        guard.user_input_record.push_front(entry);
    }
    while guard.user_input_record.len() > USER_INPUT_RECORD_CAP {
        let _ = guard.user_input_record.pop_front();
    }
}

pub(crate) fn set_slot_activity(slot_id: &MatrixSlotId, activity: MatrixSlotActivity) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    let next = match activity {
        MatrixSlotActivity::Running => MatrixSlotActivity::Idle,
        other => other,
    };
    if guard.slots[idx].activity != next {
        guard.slots[idx].activity = next;
        bump_revision(&mut guard);
    }
}

pub(crate) fn begin_slot_running(slot_id: &MatrixSlotId) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    let was_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    guard.slots[idx].running_count = guard.slots[idx].running_count.saturating_add(1);
    let is_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    if was_running != is_running {
        bump_revision(&mut guard);
    }
}

pub(crate) fn end_slot_running(slot_id: &MatrixSlotId) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    let was_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    guard.slots[idx].running_count = guard.slots[idx].running_count.saturating_sub(1);
    let is_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    if was_running != is_running {
        bump_revision(&mut guard);
    }
}

pub(crate) fn slot_views(output_mask: u8) -> Vec<MatrixSlotView> {
    let mut guard = state().lock();
    let selected = active_slot_id_ref(&guard, output_mask).clone();
    let _ = ensure_slot_index(&mut guard.slots, &selected);

    let mut out = Vec::new();
    for slot in &guard.slots {
        out.push(MatrixSlotView {
            id: slot.id.clone(),
            selected: slot.id == selected,
            activity: visible_activity(slot),
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
    for (idx, line) in slot
        .lines
        .iter()
        .skip(start_line)
        .take(max_lines)
        .enumerate()
    {
        if idx != 0 {
            out.push('\n');
        }
        out.push_str(line.text.as_str());
    }
    out
}
