use alloc::collections::VecDeque;
use alloc::string::String as AllocString;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use heapless::{String as HString, Vec as HVec};
use spin::Once;

use super::TranscriptEntry;

pub(crate) const MATRIX_SLOT_ID_MAX: usize = 5;
const DEFAULT_MATRIX_SLOT_LINE_CAP: usize = 512;
pub(crate) const DEFAULT_MATRIX_SLOT_LINE_WIDTH: usize = 180;
pub(crate) const DEFAULT_MATRIX_VIEW_ROWS: usize = 51;
const LIVE_USER_INPUT_CAP: usize = 10;
const MATRIX_SLOT_ATTACHMENT_CAP: usize = 8;

pub(crate) type MatrixSlotId = HString<MATRIX_SLOT_ID_MAX>;

/// Identity for one demanded Matrix page lifetime.
///
/// The name alone is never sufficient: freeing and re-demanding the same
/// `§slot§` produces a different generation, so stale resource links cannot
/// attach to the replacement page (the usual ABA problem).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MatrixSlotLease {
    id: MatrixSlotId,
    lifetime_generation: u64,
}

impl MatrixSlotLease {
    pub(crate) fn name(&self) -> &str {
        self.id.as_str()
    }

    pub(crate) const fn lifetime_generation(&self) -> u64 {
        self.lifetime_generation
    }
}

/// Matrix-owned attachment identity. This is an internal detach key, not a
/// user-visible resource handle; [`MatrixSlotLease`] remains the lifecycle
/// authority presented to kernel services.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MatrixSlotAttachmentId(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MatrixSlotAttachmentError {
    TargetExpired,
    Capacity,
}

/// Kernel resource hook owned by a Matrix slot.
///
/// Implementations must only retire their own kernel-side link. This is not a
/// guest callback and must never contain a Blueprint/VM code pointer.
pub(crate) trait MatrixSlotAttachment: Send + Sync {
    fn on_matrix_slot_freed(&self, lease: &MatrixSlotLease);
}

struct MatrixSlotAttachmentRecord {
    id: MatrixSlotAttachmentId,
    attachment: Arc<dyn MatrixSlotAttachment>,
}

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
    app_label: Option<AllocString>,
    app_sha256: Option<[u8; 32]>,
    attachments: HVec<MatrixSlotAttachmentRecord, MATRIX_SLOT_ATTACHMENT_CAP>,
}

#[derive(Clone)]
pub(crate) struct LiveUserInputEntry {
    pub(crate) text: AllocString,
    pub(crate) count: u64,
}

struct MatrixState {
    slots: Vec<MatrixSlot>,
    active_slot_ids: [MatrixSlotId; super::OUTPUT_SCOPE_COUNT],
    active_view_revisions: [u64; super::OUTPUT_SCOPE_COUNT],
    slot_strip_revision: u64,
    view_line_widths: [usize; super::OUTPUT_SCOPE_COUNT],
    view_terminal_rows: [usize; super::OUTPUT_SCOPE_COUNT],
    live_user_input_record: VecDeque<LiveUserInputEntry>,
    revision: u64,
}

static MATRIX_STATE: Once<spin::Mutex<MatrixState>> = Once::new();
static NEXT_SLOT_LIFETIME_GENERATION: AtomicU64 = AtomicU64::new(1);
static NEXT_SLOT_ATTACHMENT_ID: AtomicU64 = AtomicU64::new(1);

fn next_slot_lifetime_generation() -> u64 {
    NEXT_SLOT_LIFETIME_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .max(1)
}

fn next_slot_attachment_id() -> MatrixSlotAttachmentId {
    MatrixSlotAttachmentId(
        NEXT_SLOT_ATTACHMENT_ID
            .fetch_add(1, Ordering::AcqRel)
            .max(1),
    )
}

fn state() -> &'static spin::Mutex<MatrixState> {
    MATRIX_STATE.call_once(|| {
        let mut initial = MatrixState {
            slots: Vec::new(),
            active_slot_ids: core::array::from_fn(|_| default_slot_id()),
            active_view_revisions: [1; super::OUTPUT_SCOPE_COUNT],
            slot_strip_revision: 1,
            view_line_widths: [DEFAULT_MATRIX_SLOT_LINE_WIDTH; super::OUTPUT_SCOPE_COUNT],
            view_terminal_rows: [DEFAULT_MATRIX_VIEW_ROWS; super::OUTPUT_SCOPE_COUNT],
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
        lifetime_generation: next_slot_lifetime_generation(),
        revision: 1,
        lines: VecDeque::new(),
        activity: MatrixSlotActivity::Idle,
        running_count: 0,
        interrupt_generation: 0,
        vm_id: None,
        vm_input_attached: false,
        vm_launch_reserved: false,
        app_label: None,
        app_sha256: None,
        attachments: HVec::new(),
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

fn output_scope_index(output_mask: super::OutputMask) -> usize {
    if output_mask.count_ones() != 1 {
        return super::OUTPUT_SYSTEM_MASK.trailing_zeros() as usize;
    }
    (output_mask.trailing_zeros() as usize).min(super::OUTPUT_SCOPE_COUNT.saturating_sub(1))
}

fn active_view_revision_ref(state: &MatrixState, output_mask: super::OutputMask) -> &u64 {
    &state.active_view_revisions[output_scope_index(output_mask)]
}

fn active_view_revision_mut(state: &mut MatrixState, output_mask: super::OutputMask) -> &mut u64 {
    &mut state.active_view_revisions[output_scope_index(output_mask)]
}

fn bump_active_view_revision(state: &mut MatrixState, output_mask: super::OutputMask) {
    let revision = active_view_revision_mut(state, output_mask);
    *revision = revision.wrapping_add(1).max(1);
    bump_revision(state);
}

/// A granted Matrix slot changes the left-aligned slot strip for every shell
/// view, even when no view's active page changes.
fn bump_slot_strip_revision(state: &mut MatrixState) {
    state.slot_strip_revision = state.slot_strip_revision.wrapping_add(1).max(1);
    for revision in &mut state.active_view_revisions {
        *revision = revision.wrapping_add(1).max(1);
    }
    bump_revision(state);
}

pub(crate) fn slot_strip_revision() -> u64 {
    state().lock().slot_strip_revision
}

fn active_slot_id_ref(state: &MatrixState, output_mask: super::OutputMask) -> &MatrixSlotId {
    &state.active_slot_ids[output_scope_index(output_mask)]
}

fn active_slot_id_mut(
    state: &mut MatrixState,
    output_mask: super::OutputMask,
) -> &mut MatrixSlotId {
    &mut state.active_slot_ids[output_scope_index(output_mask)]
}

fn push_line(slot: &mut MatrixSlot, text: &str) {
    if slot.lines.len() >= DEFAULT_MATRIX_SLOT_LINE_CAP {
        let _ = slot.lines.pop_front();
    }
    slot.lines.push_back(TranscriptEntry {
        text: AllocString::from(text),
        transient: false,
    });
}

fn push_transient_line(slot: &mut MatrixSlot, text: &str) {
    if slot.lines.len() >= DEFAULT_MATRIX_SLOT_LINE_CAP {
        let _ = slot.lines.pop_front();
    }
    slot.lines.push_back(TranscriptEntry {
        text: AllocString::from(text),
        transient: true,
    });
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

pub(crate) fn active_slot_id(output_mask: super::OutputMask) -> MatrixSlotId {
    active_slot_id_ref(&state().lock(), output_mask).clone()
}

pub(crate) fn active_slot_activity(output_mask: super::OutputMask) -> MatrixSlotActivity {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    visible_activity(&guard.slots[idx])
}

pub(crate) fn switch_active_slot(output_mask: super::OutputMask, requested: &str) -> MatrixSlotId {
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
    slot.vm_id.is_none() && !slot.vm_launch_reserved && slot.app_label.is_none()
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
    for ch in stem
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(MATRIX_SLOT_ID_MAX.saturating_sub(1))
    {
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

fn broad_slot_candidate(mut attempt: u16) -> MatrixSlotId {
    let mut encoded = [0u8; MATRIX_SLOT_ID_MAX];
    for digit in encoded.iter_mut().rev() {
        *digit = (attempt % 36) as u8;
        attempt /= 36;
    }

    let mut out = MatrixSlotId::new();
    for digit in encoded {
        let _ = push_base36(&mut out, digit);
    }
    out
}

pub(crate) fn reserve_available_vm_slot_selected(
    output_mask: super::OutputMask,
    preferred: &str,
) -> MatrixSlotId {
    let preferred_id = normalize_slot_id(preferred);
    let default_id = default_slot_id();
    let mut guard = state().lock();

    if preferred_id != default_id && reserve_vm_slot_id(&mut guard, &preferred_id) {
        *active_slot_id_mut(&mut guard, output_mask) = preferred_id.clone();
        bump_slot_strip_revision(&mut guard);
        return preferred_id;
    }

    for attempt in 1..=35 {
        let candidate = fallback_slot_candidate(&preferred_id, attempt);
        if candidate == default_id {
            continue;
        }
        if reserve_vm_slot_id(&mut guard, &candidate) {
            *active_slot_id_mut(&mut guard, output_mask) = candidate.clone();
            bump_slot_strip_revision(&mut guard);
            return candidate;
        }
    }

    for attempt in 0..(26 * 36 * 36) {
        let candidate = broad_slot_candidate(attempt);
        if reserve_vm_slot_id(&mut guard, &candidate) {
            *active_slot_id_mut(&mut guard, output_mask) = candidate.clone();
            bump_slot_strip_revision(&mut guard);
            return candidate;
        }
    }

    let _ = reserve_vm_slot_id(&mut guard, &preferred_id);
    *active_slot_id_mut(&mut guard, output_mask) = preferred_id.clone();
    bump_slot_strip_revision(&mut guard);
    preferred_id
}

fn claim_app_slot(guard: &mut MatrixState, id: &MatrixSlotId, app_label: &str) -> bool {
    let idx = ensure_slot_index(&mut guard.slots, id);
    let slot = &guard.slots[idx];
    let already_owned = slot.app_label.as_deref() == Some(app_label)
        && slot.vm_id.is_none()
        && !slot.vm_launch_reserved;
    let unoccupied = slot.lines.is_empty()
        && slot.activity == MatrixSlotActivity::Idle
        && slot.running_count == 0
        && slot.vm_id.is_none()
        && !slot.vm_launch_reserved
        && slot.app_label.is_none();
    if !already_owned && !unoccupied {
        return false;
    }

    let slot = &mut guard.slots[idx];
    let next_label = Some(AllocString::from(app_label));
    let changed = slot.app_label != next_label || slot.activity != MatrixSlotActivity::Session;
    slot.app_label = next_label;
    slot.activity = MatrixSlotActivity::Session;
    if changed {
        bump_slot_revision(guard, idx);
    }
    true
}

/// Select one app-owned Matrix slot, reusing its prior claim or applying the
/// same compact base-36 collision fallback used by VM-backed slots.
pub(crate) fn claim_available_app_slot_selected(
    output_mask: super::OutputMask,
    preferred: &str,
    app_label: &str,
) -> MatrixSlotId {
    let preferred_id = normalize_slot_id(preferred);
    let default_id = default_slot_id();
    let mut guard = state().lock();

    if let Some(existing) = guard
        .slots
        .iter()
        .find(|slot| {
            slot.app_label.as_deref() == Some(app_label)
                && slot.vm_id.is_none()
                && !slot.vm_launch_reserved
        })
        .map(|slot| slot.id.clone())
    {
        *active_slot_id_mut(&mut guard, output_mask) = existing.clone();
        bump_slot_strip_revision(&mut guard);
        return existing;
    }

    if preferred_id != default_id && claim_app_slot(&mut guard, &preferred_id, app_label) {
        *active_slot_id_mut(&mut guard, output_mask) = preferred_id.clone();
        bump_slot_strip_revision(&mut guard);
        return preferred_id;
    }

    for attempt in 1..=35 {
        let candidate = fallback_slot_candidate(&preferred_id, attempt);
        if candidate == default_id {
            continue;
        }
        if claim_app_slot(&mut guard, &candidate, app_label) {
            *active_slot_id_mut(&mut guard, output_mask) = candidate.clone();
            bump_slot_strip_revision(&mut guard);
            return candidate;
        }
    }

    for attempt in 0..(26 * 36 * 36) {
        let candidate = broad_slot_candidate(attempt);
        if claim_app_slot(&mut guard, &candidate, app_label) {
            *active_slot_id_mut(&mut guard, output_mask) = candidate.clone();
            bump_slot_strip_revision(&mut guard);
            return candidate;
        }
    }

    let _ = claim_app_slot(&mut guard, &preferred_id, app_label);
    *active_slot_id_mut(&mut guard, output_mask) = preferred_id.clone();
    bump_slot_strip_revision(&mut guard);
    preferred_id
}

pub(crate) fn slot_lease_is_live(lease: &MatrixSlotLease) -> bool {
    state()
        .lock()
        .slots
        .iter()
        .any(|slot| slot.id == lease.id && slot.lifetime_generation == lease.lifetime_generation)
}

/// Attach a kernel-owned resource link to exactly one Matrix slot lifetime.
/// Freeing that `§slot§` synchronously retires the attachment before
/// [`free_slot`] returns.
pub(crate) fn attach_to_live_slot(
    lease: &MatrixSlotLease,
    attachment: Arc<dyn MatrixSlotAttachment>,
) -> Result<MatrixSlotAttachmentId, MatrixSlotAttachmentError> {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == lease.id) else {
        return Err(MatrixSlotAttachmentError::TargetExpired);
    };
    if guard.slots[idx].lifetime_generation != lease.lifetime_generation {
        return Err(MatrixSlotAttachmentError::TargetExpired);
    }
    if guard.slots[idx].attachments.is_full() {
        return Err(MatrixSlotAttachmentError::Capacity);
    }

    let id = next_slot_attachment_id();
    guard.slots[idx]
        .attachments
        .push(MatrixSlotAttachmentRecord { id, attachment })
        .map_err(|_| MatrixSlotAttachmentError::Capacity)?;
    Ok(id)
}

pub(crate) fn detach_from_live_slot(
    lease: &MatrixSlotLease,
    attachment_id: MatrixSlotAttachmentId,
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == lease.id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lease.lifetime_generation {
        return false;
    }
    let Some(attachment_index) = guard.slots[idx]
        .attachments
        .iter()
        .position(|record| record.id == attachment_id)
    else {
        return false;
    };
    let _ = guard.slots[idx].attachments.swap_remove(attachment_index);
    true
}

pub(crate) fn free_slot(requested: &str) -> (MatrixSlotId, Vec<u8>) {
    let freed_id = normalize_slot_id(requested);
    let default_id = default_slot_id();
    let mut guard = state().lock();
    let mut changed = false;
    let mut vm_ids = Vec::new();
    let freed_lease;
    let freed_attachments;

    if freed_id == default_id {
        let idx = ensure_slot_index(&mut guard.slots, &default_id);
        let slot = &mut guard.slots[idx];
        freed_lease = MatrixSlotLease {
            id: slot.id.clone(),
            lifetime_generation: slot.lifetime_generation,
        };
        freed_attachments = core::mem::replace(&mut slot.attachments, HVec::new());
        if let Some(vm_id) = slot.vm_id {
            vm_ids.push(vm_id);
        }
        if !slot.lines.is_empty()
            || slot.activity != MatrixSlotActivity::Idle
            || slot.running_count != 0
            || slot.vm_id.is_some()
            || slot.vm_input_attached
            || slot.vm_launch_reserved
            || slot.app_label.is_some()
            || slot.app_sha256.is_some()
        {
            slot.lines.clear();
            slot.activity = MatrixSlotActivity::Idle;
            slot.running_count = 0;
            slot.interrupt_generation = 0;
            slot.vm_id = None;
            slot.vm_input_attached = false;
            slot.vm_launch_reserved = false;
            slot.app_label = None;
            slot.app_sha256 = None;
        }
        // The default page cannot be removed from Matrix, so rotate its
        // generation in place. Old resource leases still expire exactly as
        // they do for an ordinary removed/re-demanded slot.
        guard.slots[idx].lifetime_generation = next_slot_lifetime_generation();
        // A lifetime transition is observable Matrix state even if the page
        // was visually empty and carried only resource attachments.
        bump_slot_revision(&mut guard, idx);
    } else if let Some(idx) = guard.slots.iter().position(|slot| slot.id == freed_id) {
        let removed = guard.slots.remove(idx);
        freed_lease = MatrixSlotLease {
            id: removed.id.clone(),
            lifetime_generation: removed.lifetime_generation,
        };
        freed_attachments = removed.attachments;
        if let Some(vm_id) = removed.vm_id {
            vm_ids.push(vm_id);
        }
        for scope_index in 0..super::OUTPUT_SCOPE_COUNT {
            if guard.active_slot_ids[scope_index] == freed_id {
                guard.active_slot_ids[scope_index] = default_id.clone();
                bump_active_view_revision(&mut guard, 1 << scope_index);
            }
        }
        changed = true;
    } else {
        // A free of an absent page has no lifetime to retire.
        return (freed_id, vm_ids);
    }

    let _ = ensure_slot_index(&mut guard.slots, &default_id);
    if changed {
        bump_revision(&mut guard);
    }
    drop(guard);

    for record in freed_attachments {
        record.attachment.on_matrix_slot_freed(&freed_lease);
    }
    (freed_id, vm_ids)
}

pub(crate) fn active_lines(output_mask: super::OutputMask) -> VecDeque<TranscriptEntry> {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    guard.slots[idx].lines.clone()
}

pub(crate) fn clear_active_lines(output_mask: super::OutputMask) {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    if !guard.slots[idx].lines.is_empty() {
        guard.slots[idx].lines.clear();
        bump_slot_revision(&mut guard, idx);
    }
}

pub(crate) fn active_slot_app_label(output_mask: super::OutputMask) -> Option<AllocString> {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    guard.slots[idx].app_label.clone()
}

pub(crate) fn active_slot_app_sha256(output_mask: super::OutputMask) -> Option<[u8; 32]> {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    guard.slots[idx].app_sha256
}

pub(crate) fn active_line_width(output_mask: super::OutputMask) -> usize {
    let guard = state().lock();
    guard.view_line_widths[output_scope_index(output_mask)]
}

pub(crate) fn set_active_line_width(output_mask: super::OutputMask, width: usize) {
    let mut guard = state().lock();
    let index = output_scope_index(output_mask);
    if guard.view_line_widths[index] != width {
        guard.view_line_widths[index] = width;
        bump_active_view_revision(&mut guard, output_mask);
    }
}

pub(crate) fn active_terminal_rows(output_mask: super::OutputMask) -> usize {
    let guard = state().lock();
    guard.view_terminal_rows[output_scope_index(output_mask)]
}

pub(crate) fn set_active_terminal_rows(output_mask: super::OutputMask, rows: usize) {
    let mut guard = state().lock();
    let index = output_scope_index(output_mask);
    if guard.view_terminal_rows[index] != rows {
        guard.view_terminal_rows[index] = rows;
        bump_active_view_revision(&mut guard, output_mask);
    }
}

pub(crate) fn record_line_for_output(output_mask: super::OutputMask, text: &str) -> MatrixSlotId {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    push_line(&mut guard.slots[idx], text);
    bump_slot_revision(&mut guard, idx);
    slot_id
}

pub(crate) fn record_line_in_default(text: &str) {
    let default_id = default_slot_id();
    record_line_in_slot(&default_id, text);
}

pub(crate) fn record_line_in_slot(slot_id: &MatrixSlotId, text: &str) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    push_line(&mut guard.slots[idx], text);
    bump_slot_revision(&mut guard, idx);
}

pub(crate) fn record_line_in_live_slot(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
    text: &str,
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }
    guard.slots[idx].lines.retain(|line| !line.transient);
    push_line(&mut guard.slots[idx], text);
    bump_slot_revision(&mut guard, idx);
    true
}

pub(crate) fn record_transient_line_in_live_slot(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
    text: &str,
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }
    if let Some(line) = guard.slots[idx]
        .lines
        .iter_mut()
        .find(|line| line.transient)
    {
        line.text.clear();
        line.text.push_str(text);
    } else {
        push_transient_line(&mut guard.slots[idx], text);
    }
    bump_slot_revision(&mut guard, idx);
    true
}

pub(crate) fn replace_transient_lines_in_live_slot(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
    lines: &[AllocString],
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }

    guard.slots[idx].lines.retain(|line| !line.transient);
    // Transcript entries are presented newest-first. Store the block bottom-up
    // so callers can provide its rows in their natural visual order.
    for line in lines.iter().rev() {
        push_transient_line(&mut guard.slots[idx], line.as_str());
    }
    bump_slot_revision(&mut guard, idx);
    true
}

pub(crate) fn clear_transient_lines_in_live_slot(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }
    let previous_len = guard.slots[idx].lines.len();
    guard.slots[idx].lines.retain(|line| !line.transient);
    if guard.slots[idx].lines.len() != previous_len {
        bump_slot_revision(&mut guard, idx);
    }
    true
}

pub(crate) fn record_user_input(transport_scope: u8, text: &str) {
    crate::user_input_record::capture(transport_scope, text);
    let mut guard = state().lock();
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

pub(crate) fn active_slot_vm_input_id(output_mask: super::OutputMask) -> Option<u8> {
    let mut guard = state().lock();
    let active = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &active);
    if guard.slots[idx].vm_input_attached {
        guard.slots[idx].vm_id
    } else {
        None
    }
}

pub(crate) fn active_slot_vm_id(output_mask: super::OutputMask) -> Option<u8> {
    let mut guard = state().lock();
    let active = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &active);
    guard.slots[idx].vm_id
}

pub(crate) fn bind_live_slot_vm(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
    vm_id: u8,
    input_attached: bool,
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }
    if guard.slots[idx].vm_id != Some(vm_id)
        || guard.slots[idx].vm_input_attached != input_attached
        || guard.slots[idx].vm_launch_reserved
    {
        guard.slots[idx].vm_id = Some(vm_id);
        guard.slots[idx].vm_input_attached = input_attached;
        guard.slots[idx].vm_launch_reserved = false;
        bump_slot_revision(&mut guard, idx);
    }
    true
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn set_slot_app_label(slot_id: &MatrixSlotId, label: &str) {
    let mut guard = state().lock();
    let idx = ensure_slot_index(&mut guard.slots, slot_id);
    let next = if label.trim().is_empty() {
        None
    } else {
        Some(AllocString::from(label.trim()))
    };
    if guard.slots[idx].app_label != next || guard.slots[idx].app_sha256.is_some() {
        guard.slots[idx].app_label = next;
        guard.slots[idx].app_sha256 = None;
        bump_slot_revision(&mut guard, idx);
    }
}

pub(crate) fn set_live_slot_app_identity(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
    label: &str,
    sha256: [u8; 32],
) -> bool {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return false;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return false;
    }
    let next = if label.trim().is_empty() {
        None
    } else {
        Some(AllocString::from(label.trim()))
    };
    if guard.slots[idx].app_label != next || guard.slots[idx].app_sha256 != Some(sha256) {
        guard.slots[idx].app_label = next;
        guard.slots[idx].app_sha256 = Some(sha256);
        bump_slot_revision(&mut guard, idx);
    }
    true
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

pub(crate) fn unbind_live_slot_vm(
    slot_id: &MatrixSlotId,
    lifetime_generation: u64,
    vm_id: u8,
) -> super::MatrixVmUnbindResult {
    let mut guard = state().lock();
    let Some(idx) = guard.slots.iter().position(|slot| slot.id == *slot_id) else {
        return super::MatrixVmUnbindResult::TargetExpired;
    };
    if guard.slots[idx].lifetime_generation != lifetime_generation {
        return super::MatrixVmUnbindResult::TargetExpired;
    }
    if guard.slots[idx].vm_id == Some(vm_id) {
        guard.slots[idx].vm_id = None;
        guard.slots[idx].vm_input_attached = false;
        guard.slots[idx].vm_launch_reserved = false;
        guard.slots[idx].app_label = None;
        guard.slots[idx].app_sha256 = None;
        bump_slot_revision(&mut guard, idx);
        super::MatrixVmUnbindResult::Unbound
    } else if guard.slots[idx].vm_id.is_none() {
        super::MatrixVmUnbindResult::AlreadyAbsent
    } else {
        super::MatrixVmUnbindResult::DifferentOwner
    }
}

pub(crate) fn slot_views(output_mask: super::OutputMask) -> Vec<MatrixSlotView> {
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn revision() -> u64 {
    state().lock().revision
}

pub(crate) fn visible_revision(output_mask: super::OutputMask) -> u64 {
    let mut guard = state().lock();
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    active_view_revision_ref(&guard, output_mask)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(guard.slots[idx].revision)
}

/// Read the revision used by the paint loop without busy-spinning.  Shell2 can
/// yield and retry when a producer is updating Matrix state on another CPU.
pub(crate) fn try_visible_revision(output_mask: super::OutputMask) -> Option<u64> {
    let mut guard = state().try_lock()?;
    let slot_id = active_slot_id_ref(&guard, output_mask).clone();
    let idx = ensure_slot_index(&mut guard.slots, &slot_id);
    Some(
        active_view_revision_ref(&guard, output_mask)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(guard.slots[idx].revision),
    )
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn history_total_lines() -> usize {
    let mut guard = state().lock();
    let default_id = default_slot_id();
    let idx = ensure_slot_index(&mut guard.slots, &default_id);
    guard.slots[idx].lines.len()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_ids_accept_five_characters_and_truncate_at_the_soft_cap() {
        assert_eq!(normalize_slot_id("§12345§").as_str(), "12345");
        assert_eq!(normalize_slot_id("123456").as_str(), "12345");
    }

    #[test]
    fn app_slot_fallback_keeps_up_to_four_stem_characters_and_a_suffix() {
        let stem = normalize_slot_id("cry");
        assert_eq!(fallback_slot_candidate(&stem, 1).as_str(), "cry1");
        assert_eq!(fallback_slot_candidate(&stem, 2).as_str(), "cry2");
        assert_eq!(fallback_slot_candidate(&stem, 10).as_str(), "crya");

        let long_stem = normalize_slot_id("hello");
        assert_eq!(fallback_slot_candidate(&long_stem, 1).as_str(), "hell1");
    }

    #[test]
    fn broad_slot_candidates_fill_the_configured_width() {
        assert_eq!(broad_slot_candidate(0).as_str(), "00000");
        assert_eq!(broad_slot_candidate(35).as_str(), "0000z");
        assert_eq!(broad_slot_candidate(36).as_str(), "00010");
    }

    #[test]
    fn permanent_output_dismisses_the_transient_progress_row() {
        let slot_id = slot_id_from_name("trnst");
        let generation = slot_lifetime_generation(&slot_id);

        assert!(record_line_in_live_slot(&slot_id, generation, "before"));
        assert!(record_transient_line_in_live_slot(&slot_id, generation, "busy"));
        assert_eq!(slot_transcript_text(&slot_id), "before\nbusy");

        assert!(record_line_in_live_slot(&slot_id, generation, "after"));
        assert_eq!(slot_transcript_text(&slot_id), "before\nafter");
    }
}
