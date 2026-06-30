use alloc::collections::VecDeque;
use alloc::string::String as AllocString;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    lifetime_generation: u64,
    revision: u64,
    lines: VecDeque<TranscriptEntry>,
    activity: MatrixSlotActivity,
    running_count: usize,
    interrupt_generation: u64,
    vm_id: Option<u8>,
    vm_input_attached: bool,
    vm_launch_reserved: bool,
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
    ui3_active: MatrixSlotId,
    container_active: MatrixSlotId,
    uart_view_revision: u64,
    net_view_revision: u64,
    ui3_view_revision: u64,
    container_view_revision: u64,
    user_input_record: VecDeque<AllocString>,
    live_user_input_record: VecDeque<LiveUserInputEntry>,
    revision: u64,
}

static MATRIX_STATE: Once<spin::Mutex<MatrixState>> = Once::new();
static NEXT_SLOT_LIFETIME_GENERATION: AtomicU64 = AtomicU64::new(1);

fn state() -> &'static spin::Mutex<MatrixState> {
    MATRIX_STATE.call_once(|| {
        let mut initial = MatrixState {
            slots: Vec::new(),
            uart_active: default_slot_id(),
            net_active: default_slot_id(),
            ui3_active: default_slot_id(),
            container_active: default_slot_id(),
            uart_view_revision: 1,
            net_view_revision: 1,
            ui3_view_revision: 1,
            container_view_revision: 1,
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
        lifetime_generation: NEXT_SLOT_LIFETIME_GENERATION.fetch_add(1, Ordering::AcqRel),
        revision: 1,
        lines: VecDeque::new(),
        activity: MatrixSlotActivity::Idle,
        running_count: 0,
        interrupt_generation: 0,
        vm_id: None,
        vm_input_attached: false,
        vm_launch_reserved: false,
        line_width: DEFAULT_MATRIX_SLOT_LINE_WIDTH,
    });
    slots.len() - 1
}

fn bump_revision(state: &mut MatrixState) {
    state.revision = state.revision.wrapping_add(1);
}

fn bump_slot_revision(state: &mut MatrixState, idx: usize) {
    state.slots[idx].revision = state.slots[idx].revision.wrapping_add(1).max(1);
    bump_revision(state);
}

fn active_view_revision_ref(state: &MatrixState, output_mask: u8) -> &u64 {
    if (output_mask & super::OUTPUT_NET_TCP_MASK) != 0 {
        &state.net_view_revision
    } else if (output_mask & super::OUTPUT_UI3_MASK) != 0 {
        &state.ui3_view_revision
    } else if (output_mask & super::OUTPUT_CONTAINER_MASK) != 0 {
        &state.container_view_revision
    } else {
        &state.uart_view_revision
    }
}

fn active_view_revision_mut(state: &mut MatrixState, output_mask: u8) -> &mut u64 {
    if (output_mask & super::OUTPUT_NET_TCP_MASK) != 0 {
        &mut state.net_view_revision
    } else if (output_mask & super::OUTPUT_UI3_MASK) != 0 {
        &mut state.ui3_view_revision
    } else if (output_mask & super::OUTPUT_CONTAINER_MASK) != 0 {
        &mut state.container_view_revision
    } else {
        &mut state.uart_view_revision
    }
}

fn bump_active_view_revision(state: &mut MatrixState, output_mask: u8) {
    let revision = active_view_revision_mut(state, output_mask);
    *revision = revision.wrapping_add(1).max(1);
    bump_revision(state);
}

fn active_slot_id_ref(state: &MatrixState, output_mask: u8) -> &MatrixSlotId {
    if (output_mask & super::OUTPUT_NET_TCP_MASK) != 0 {
        &state.net_active
    } else if (output_mask & super::OUTPUT_UI3_MASK) != 0 {
        &state.ui3_active
    } else if (output_mask & super::OUTPUT_CONTAINER_MASK) != 0 {
        &state.container_active
    } else {
        &state.uart_active
    }
}

fn active_slot_id_mut(state: &mut MatrixState, output_mask: u8) -> &mut MatrixSlotId {
    if (output_mask & super::OUTPUT_NET_TCP_MASK) != 0 {
        &mut state.net_active
    } else if (output_mask & super::OUTPUT_UI3_MASK) != 0 {
        &mut state.ui3_active
    } else if (output_mask & super::OUTPUT_CONTAINER_MASK) != 0 {
        &mut state.container_active
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
    if *active_slot_id_ref(&guard, output_mask) != next_id {
        *active_slot_id_mut(&mut guard, output_mask) = next_id.clone();
        bump_active_view_revision(&mut guard, output_mask);
    }
    let _ = idx;
    next_id
}

fn slot_available_for_vm(slot: &MatrixSlot) -> bool {
    slot.vm_id.is_none() && !slot.vm_launch_reserved
}

fn reserve_vm_slot_id(guard: &mut MatrixState, id: &MatrixSlotId) -> bool {
    let idx = ensure_slot_index(&mut guard.slots, id);
    if !slot_available_for_vm(&guard.slots[idx]) {
        return false;
    }
    guard.slots[idx].vm_launch_reserved = true;
    true
}

fn push_base36(out: &mut MatrixSlotId, value: u8) -> bool {
    let ch = match value {
        0..=9 => (b'0' + value) as char,
        10..=35 => (b'a' + (value - 10)) as char,
        _ => return false,
    };
    out.push(ch).is_ok()
}

fn fallback_slot_candidate(stem: &MatrixSlotId, attempt: u16) -> MatrixSlotId {
    let mut out = MatrixSlotId::new();
    let mut stem_chars = stem.chars().filter(|ch| ch.is_ascii_alphanumeric());
    if let Some(ch) = stem_chars.next() {
        let _ = out.push(ch);
    }
    if let Some(ch) = stem_chars.next() {
        let _ = out.push(ch);
    }
    if out.is_empty() {
        let _ = out.push('b');
        let _ = out.push('p');
    }
    if out.len() >= MATRIX_SLOT_ID_MAX {
        out.truncate(MATRIX_SLOT_ID_MAX - 1);
    }
    let _ = push_base36(&mut out, (attempt % 36) as u8);
    out
}

fn broad_slot_candidate(attempt: u16) -> MatrixSlotId {
    let mut out = MatrixSlotId::new();
    let first = ((attempt / (36 * 36)) % 26) as u8;
    let second = ((attempt / 36) % 36) as u8;
    let third = (attempt % 36) as u8;
    let _ = out.push((b'a' + first) as char);
    let _ = push_base36(&mut out, second);
    let _ = push_base36(&mut out, third);
    out
}

pub(crate) fn reserve_available_vm_slot_selected(output_mask: u8, preferred: &str) -> MatrixSlotId {
    let preferred_id = normalize_slot_id(preferred);
    let default_id = default_slot_id();
    let mut guard = state().lock();

    if preferred_id != default_id && reserve_vm_slot_id(&mut guard, &preferred_id) {
        *active_slot_id_mut(&mut guard, output_mask) = preferred_id.clone();
        bump_active_view_revision(&mut guard, output_mask);
        return preferred_id;
    }

    for attempt in 1..=35 {
        let candidate = fallback_slot_candidate(&preferred_id, attempt);
        if candidate == default_id {
            continue;
        }
        if reserve_vm_slot_id(&mut guard, &candidate) {
            *active_slot_id_mut(&mut guard, output_mask) = candidate.clone();
            bump_active_view_revision(&mut guard, output_mask);
            return candidate;
        }
    }

    for attempt in 0..(26 * 36 * 36) {
        let candidate = broad_slot_candidate(attempt);
        if reserve_vm_slot_id(&mut guard, &candidate) {
            *active_slot_id_mut(&mut guard, output_mask) = candidate.clone();
            bump_active_view_revision(&mut guard, output_mask);
            return candidate;
        }
    }

    let _ = reserve_vm_slot_id(&mut guard, &preferred_id);
    *active_slot_id_mut(&mut guard, output_mask) = preferred_id.clone();
    bump_active_view_revision(&mut guard, output_mask);
    preferred_id
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
            slot.interrupt_generation = 0;
            slot.vm_id = None;
            slot.vm_launch_reserved = false;
            slot.line_width = DEFAULT_MATRIX_SLOT_LINE_WIDTH;
            bump_slot_revision(&mut guard, idx);
        }
    } else if let Some(idx) = guard.slots.iter().position(|slot| slot.id == freed_id) {
        let _ = guard.slots.remove(idx);
        if guard.uart_active == freed_id {
            guard.uart_active = default_id.clone();
            bump_active_view_revision(&mut guard, super::OUTPUT_UART1_MASK);
        }
        if guard.net_active == freed_id {
            guard.net_active = default_id.clone();
            bump_active_view_revision(&mut guard, super::OUTPUT_NET_TCP_MASK);
        }
        if guard.ui3_active == freed_id {
            guard.ui3_active = default_id.clone();
            bump_active_view_revision(&mut guard, super::OUTPUT_UI3_MASK);
        }
        if guard.container_active == freed_id {
            guard.container_active = default_id.clone();
            bump_active_view_revision(&mut guard, super::OUTPUT_CONTAINER_MASK);
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
        bump_slot_revision(&mut guard, idx);
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
    bump_slot_revision(&mut guard, idx);
    slot_id
}

pub(crate) fn record_line_in_default(source: LineSource, text: &str) {
    let mut guard = state().lock();
    let default_id = default_slot_id();
    let idx = ensure_slot_index(&mut guard.slots, &default_id);
    push_line(&mut guard.slots[idx], source, text);
    bump_slot_revision(&mut guard, idx);
}

pub(crate) fn record_line_in_slot(slot_id: &MatrixSlotId, source: LineSource, text: &str) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    push_line(&mut guard.slots[idx], source, text);
    bump_slot_revision(&mut guard, idx);
}

pub(crate) fn record_line_in_live_slot(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
    source: LineSource,
    text: &str,
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }
    push_line(&mut guard.slots[idx], source, text);
    bump_slot_revision(&mut guard, idx);
    true
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
        bump_slot_revision(&mut guard, idx);
    }
}

pub(crate) fn begin_slot_running(slot_id: &MatrixSlotId) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    let was_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    guard.slots[idx].running_count = guard.slots[idx].running_count.saturating_add(1);
    let is_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    if was_running != is_running {
        bump_slot_revision(&mut guard, idx);
    }
}

pub(crate) fn begin_live_slot_running(slot_id: &MatrixSlotId, lifetime_generation: u64) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }
    let was_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    guard.slots[idx].running_count = guard.slots[idx].running_count.saturating_add(1);
    let is_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    if was_running != is_running {
        bump_slot_revision(&mut guard, idx);
    }
    true
}

pub(crate) fn end_slot_running(slot_id: &MatrixSlotId) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    let was_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    guard.slots[idx].running_count = guard.slots[idx].running_count.saturating_sub(1);
    let is_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    if was_running != is_running {
        bump_slot_revision(&mut guard, idx);
    }
}

pub(crate) fn end_live_slot_running(slot_id: &MatrixSlotId, lifetime_generation: u64) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }
    let was_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    guard.slots[idx].running_count = guard.slots[idx].running_count.saturating_sub(1);
    let is_running = visible_activity(&guard.slots[idx]) == MatrixSlotActivity::Running;
    if was_running != is_running {
        bump_slot_revision(&mut guard, idx);
    }
    true
}

pub(crate) fn slot_lifetime_generation(slot_id: &MatrixSlotId) -> u64 {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    guard.slots[idx].lifetime_generation
}

pub(crate) fn live_slot_interrupt_generation(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
) -> Option<u64> {
    let guard = state().lock();
    let idx = guard.slots.iter().position(|slot| slot.id == *slot_id)?;
    (guard.slots[idx].lifetime_generation == lifetime_generation)
        .then_some(guard.slots[idx].interrupt_generation)
}

pub(crate) fn slot_interrupt_generation(slot_id: &MatrixSlotId) -> u64 {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    guard.slots[idx].interrupt_generation
}

pub(crate) fn request_slot_interrupt(slot_id: &MatrixSlotId) -> (u64, Option<u8>) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    guard.slots[idx].interrupt_generation = guard.slots[idx].interrupt_generation.wrapping_add(1);
    let generation = guard.slots[idx].interrupt_generation;
    let vm_id = guard.slots[idx].vm_id;
    bump_slot_revision(&mut guard, idx);
    (generation, vm_id)
}

pub(crate) fn active_slot_vm_input_id(output_mask: u8) -> Option<u8> {
    let mut guard = state().lock();
    let active = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &active);
    if guard.slots[idx].vm_input_attached {
        guard.slots[idx].vm_id
    } else {
        None
    }
}

pub(crate) fn active_slot_vm_id(output_mask: u8) -> Option<u8> {
    let mut guard = state().lock();
    let active = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &active);
    guard.slots[idx].vm_id
}

pub(crate) fn bind_slot_vm(slot_id: &MatrixSlotId, vm_id: u8, input_attached: bool) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    if guard.slots[idx].vm_id != Some(vm_id)
        || guard.slots[idx].vm_input_attached != input_attached
        || guard.slots[idx].vm_launch_reserved
    {
        guard.slots[idx].vm_id = Some(vm_id);
        guard.slots[idx].vm_input_attached = input_attached;
        guard.slots[idx].vm_launch_reserved = false;
        bump_slot_revision(&mut guard, idx);
    }
}

pub(crate) fn release_vm_slot_reservation(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation
        || !guard.slots[idx].vm_launch_reserved
    {
        return false;
    }
    guard.slots[idx].vm_launch_reserved = false;
    bump_slot_revision(&mut guard, idx);
    true
}

pub(crate) fn unbind_slot_vm(slot_id: &MatrixSlotId, vm_id: u8) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    if guard.slots[idx].vm_id == Some(vm_id) {
        guard.slots[idx].vm_id = None;
        guard.slots[idx].vm_input_attached = false;
        guard.slots[idx].vm_launch_reserved = false;
        bump_slot_revision(&mut guard, idx);
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

pub(crate) fn visible_revision(output_mask: u8) -> u64 {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    active_view_revision_ref(&guard, output_mask)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(guard.slots[idx].revision)
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

pub(crate) fn slot_transcript_text(slot_id: &MatrixSlotId) -> AllocString {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    let slot = &guard.slots[idx];

    let mut out = AllocString::new();
    for (idx, line) in slot.lines.iter().enumerate() {
        if idx != 0 {
            out.push('\n');
        }
        out.push_str(line.text.as_str());
    }
    out
}
