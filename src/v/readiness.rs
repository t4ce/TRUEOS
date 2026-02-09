use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

// V-layer readiness flags.
//
// These are monotonic: once set, they are never cleared.
// Consumers can `await` prerequisites instead of guessing boot ordering.
pub const HID_MOUSE_CLAIMED: u32 = 1 << 1;
pub const HID_KEYBOARD_CLAIMED: u32 = 1 << 2;
pub const PIANO_CLAIMED: u32 = 1 << 3;
pub const UAC_SINE_DONE: u32 = 1 << 4;
pub const UAC_ATTACHED: u32 = 1 << 5;

pub const NET_GATEWAY_REACHABLE: u32 = 1 << 8;

pub const TRUEOSFS_ROOT_MOUNTED: u32 = 1 << 16;

static READY: AtomicU32 = AtomicU32::new(0);

#[inline]
pub fn mask() -> u32 {
    READY.load(Ordering::Acquire)
}

#[inline]
pub fn is_set(required: u32) -> bool {
    mask() & required == required
}

/// Mark one or more readiness flags as set.
#[inline]
pub fn set(flags: u32) {
    READY.fetch_or(flags, Ordering::AcqRel);
}

/// Wait until all required flags are set.
///
/// This is a simple polling waiter to avoid additional dependencies.
pub async fn wait_for(required: u32) {
    loop {
        if is_set(required) {
            return;
        }
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}

/// Wait until all required flags are set or a timeout elapses.
///
/// Returns `true` if ready, `false` on timeout.
pub async fn wait_for_timeout(required: u32, timeout: EmbassyDuration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if is_set(required) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}
