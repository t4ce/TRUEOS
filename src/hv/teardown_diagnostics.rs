//! Lifecycle boundaries only; no VM-exit or application hot-loop tracing.
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

#[repr(u8)]
pub(super) enum Stage {
    Offline, Preparing, Hull, NativeDrain, Archive, Gridpaper, Media, Ui4,
    Input, Vgpu, Net, State, WaitQueues, CrashReport, CarrierRelease,
}
const LABELS: [&str; 15] = [
    "offline", "preparing", "hull", "native-drain", "archive-release",
    "gridpaper-release", "media-release", "ui4-release", "input-release",
    "vgpu-release", "net-release", "state-release", "wait-queue-release",
    "crash-report", "carrier-release",
];
struct Progress { stage: AtomicU8, since_ms: AtomicU64 }
static PROGRESS: [Progress; super::TRUEOS_VM_ID_LIMIT] =
    [const { Progress { stage: AtomicU8::new(0), since_ms: AtomicU64::new(0) } }; super::TRUEOS_VM_ID_LIMIT];

fn now_ms() -> u64 {
    embassy_time_driver::now().saturating_mul(1000) / embassy_time_driver::TICK_HZ.max(1)
}

pub(super) fn set(vm_id: u8, stage: Stage) {
    if let Some(progress) = PROGRESS.get(vm_id as usize) {
        progress.since_ms.store(now_ms(), Ordering::Relaxed);
        progress.stage.store(stage as u8, Ordering::Release);
    }
}

pub(super) fn describe(vm_id: u8) -> (&'static str, u64) {
    PROGRESS.get(vm_id as usize).map_or(("invalid", 0), |progress| (
        LABELS[progress.stage.load(Ordering::Acquire) as usize],
        now_ms().saturating_sub(progress.since_ms.load(Ordering::Relaxed)),
    ))
}
