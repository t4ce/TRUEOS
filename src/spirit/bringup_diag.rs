//! Transition-only Spirit evidence, separate from producer/display readiness.
use super::*;
use alloc::string::String;
use core::fmt::Write;

#[derive(Clone, Copy, Debug)]
struct Evidence {
    stage: &'static str,
    changed_ms: u64,
    sequence: u64,
    cursor_sequence: u64,
    expected_cursor_gpu: u64,
    submitted: u64,
    released: u64,
    visible: u64,
    producer_polls: u32,
    display_polls: u32,
    ctl: u32,
    base: u32,
    live: u32,
    failure: &'static str,
}

impl Evidence {
    const NEW: Self = Self {
        stage: "not-started",
        changed_ms: 0,
        sequence: 0,
        cursor_sequence: 0,
        expected_cursor_gpu: 0,
        submitted: 0,
        released: 0,
        visible: 0,
        producer_polls: 0,
        display_polls: 0,
        ctl: 0,
        base: 0,
        live: 0,
        failure: "none",
    };
}

static EVIDENCE: [Mutex<Evidence>; SPIRIT_FENCE_COUNT] =
    [const { Mutex::new(Evidence::NEW) }; SPIRIT_FENCE_COUNT];

pub(super) fn stage(id: SpiritFenceId, stage: &'static str) {
    let mut evidence = EVIDENCE[id.index()].lock();
    evidence.stage = stage;
    evidence.changed_ms = Instant::now().as_millis();
}

pub(super) fn submitted(id: SpiritFenceId, sequence: u64) {
    let mut evidence = EVIDENCE[id.index()].lock();
    evidence.stage = "producer-inflight";
    evidence.changed_ms = Instant::now().as_millis();
    evidence.sequence = sequence;
    evidence.submitted = evidence.submitted.saturating_add(1);
    evidence.producer_polls = 0;
}

pub(super) fn producer_pending(id: SpiritFenceId, polls: u32) {
    // Retain evidence of a long wait without adding a lock to every poll.
    if polls == 1 || polls.is_power_of_two() {
        EVIDENCE[id.index()].lock().producer_polls = polls;
    }
}

pub(super) fn released(id: SpiritFenceId) {
    let mut evidence = EVIDENCE[id.index()].lock();
    evidence.stage = "producer-released";
    evidence.changed_ms = Instant::now().as_millis();
    evidence.released = evidence.released.saturating_add(1);
}

pub(super) fn cursor_armed(id: SpiritFenceId, sequence: u64, expected_gpu: u64) {
    let mut evidence = EVIDENCE[id.index()].lock();
    evidence.stage = "cursor-armed";
    evidence.changed_ms = Instant::now().as_millis();
    evidence.cursor_sequence = sequence;
    evidence.expected_cursor_gpu = expected_gpu;
    evidence.display_polls = 0;
}

pub(super) fn display(
    id: SpiritFenceId,
    visible: bool,
    polls: u32,
    ctl: u32,
    base: u32,
    live: u32,
) {
    if !visible && polls != 1 && !polls.is_power_of_two() {
        return;
    }
    let mut evidence = EVIDENCE[id.index()].lock();
    evidence.stage = if visible {
        "cursor-visible"
    } else {
        "cursor-waiting"
    };
    evidence.changed_ms = Instant::now().as_millis();
    evidence.display_polls = polls;
    evidence.ctl = ctl;
    evidence.base = base;
    evidence.live = live;
    if visible {
        evidence.visible = evidence.visible.saturating_add(1);
    }
}

pub(super) fn failed(id: SpiritFenceId, reason: &'static str) {
    let mut evidence = EVIDENCE[id.index()].lock();
    evidence.stage = "stopped";
    evidence.changed_ms = Instant::now().as_millis();
    evidence.failure = reason;
}

pub(crate) fn snapshot_text() -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "capture_policy=retained-transition-evidence-no-gpu-or-display-commands now_ms={} pending_poll_counts=power-of-two-sampled visible_counts=cursor-latches-including-rearm",
        Instant::now().as_millis()
    );
    for (index, slot) in EVIDENCE.iter().enumerate() {
        let evidence = *slot.lock();
        let _ = writeln!(
            out,
            "spirit fence={} completed_sequence={} evidence={:?}",
            index,
            COMPLETED[index].load(Ordering::Acquire),
            evidence
        );
    }
    out
}
