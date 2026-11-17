//! Passive presentation breadcrumbs readable even when the owning AP stalls.
//! No reader acquires Shell3, terminal, frame-pool, or GPU locks.
use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

struct Probe {
    frontend: AtomicU64,
    window: AtomicU64,
    started_ms: AtomicU64,
    stage_ms: AtomicU64,
    stage: AtomicU64,
    passes: AtomicU64,
    completed: AtomicU64,
    snapshots: AtomicU64,
}
impl Probe {
    const fn new() -> Self {
        Self {
            frontend: AtomicU64::new(0), window: AtomicU64::new(0),
            started_ms: AtomicU64::new(0), stage_ms: AtomicU64::new(0),
            stage: AtomicU64::new(0), passes: AtomicU64::new(0),
            completed: AtomicU64::new(0), snapshots: AtomicU64::new(0),
        }
    }
}
static PROBES: [Probe; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { Probe::new() }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

fn current() -> Option<&'static Probe> { PROBES.get(crate::percpu::current_slot()) }

pub(super) struct Pass;
impl Pass {
    pub(super) fn begin(frontend: u64, window: u32) -> Self {
        if let Some(probe) = current() {
            probe.frontend.store(frontend, Ordering::Relaxed);
            probe.window.store(window as u64, Ordering::Relaxed);
            probe.started_ms.store(crate::r::input_timing::now_ms(), Ordering::Relaxed);
            probe.snapshots.store(0, Ordering::Relaxed);
            probe.passes.fetch_add(1, Ordering::Relaxed);
        }
        stage(1);
        Self
    }
}
impl Drop for Pass {
    fn drop(&mut self) {
        if let Some(probe) = current() {
            probe.completed.fetch_add(1, Ordering::Relaxed);
        }
        stage(0);
    }
}

pub(super) fn stage(stage: u64) {
    if let Some(probe) = current() {
        // Show is also used directly for resize outside Shell3::present.
        if stage != 0 && probe.stage.load(Ordering::Relaxed) == 0 && stage != 1 { return; }
        probe.stage_ms.store(crate::r::input_timing::now_ms(), Ordering::Relaxed);
        probe.stage.store(stage, Ordering::Release);
    }
}
pub(super) fn snapshot() {
    if let Some(probe) = current() { probe.snapshots.fetch_add(1, Ordering::Relaxed); }
}
pub(super) fn window(window: u32) {
    if let Some(probe) = current() { probe.window.store(window as u64, Ordering::Relaxed); }
}

fn stage_name(stage: u64) -> &'static str {
    match stage {
        0 => "idle", 1 => "reconcile", 2 => "font-scale", 3 => "resize",
        4 => "capture", 5 => "show", 6 => "bind-window", 7 => "compare-snapshot",
        8 => "startup", 10 => "font-prepare", 11 => "create-surface",
        12 => "copy-acquire", 13 => "pan-prepare", 14 => "strip-prepare",
        15 => "glyph-paint", 16 => "strip-copy", 17 => "pan-copy",
        18 => "frame-publish", 19 => "window-publish", _ => "unknown",
    }
}

/// Call from a surviving BSP reporter at a bounded cadence (e.g. five seconds).
/// Idle records retain counters so completed work can be distinguished from a
/// presentation that never began; active records show the exact current wait.
pub(crate) fn lines(now_ms: u64) -> Vec<String> {
    PROBES.iter().enumerate().filter_map(|(slot, probe)| {
        let passes = probe.passes.load(Ordering::Relaxed);
        if passes == 0 { return None; }
        let stage = probe.stage.load(Ordering::Acquire);
        Some(format!("shell3-present-probe slot={} frontend={} window={} stage={} pass_age_ms={} stage_age_ms={} passes={} completed={} snapshots={}",
            slot, probe.frontend.load(Ordering::Relaxed), probe.window.load(Ordering::Relaxed),
            stage_name(stage), if stage == 0 { 0 } else { now_ms.saturating_sub(probe.started_ms.load(Ordering::Relaxed)) },
            if stage == 0 { 0 } else { now_ms.saturating_sub(probe.stage_ms.load(Ordering::Relaxed)) },
            passes, probe.completed.load(Ordering::Relaxed), probe.snapshots.load(Ordering::Relaxed)))
    }).collect()
}
