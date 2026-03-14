use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

// V-layer readiness flags.
//
// These are monotonic: once set, they are never cleared.
// Consumers can `await` prerequisites instead of guessing boot ordering.
pub const HID_KEYBOARD_CLAIMED: u32 = 1 << 2;
pub const PIANO_CLAIMED: u32 = 1 << 3;
pub const UAC_ATTACHED: u32 = 1 << 5;

// Network readiness.
//
// `NET_GATEWAY_REACHABLE` is kept for backward compatibility ("any network").
// Prefer the per-protocol bits for new code.
pub const NET_GATEWAY_REACHABLE: u32 = 1 << 8;
pub const TLS_SOCKET_SERVICE_READY: u32 = 1 << 9;

pub const NET_V4_GATEWAY_REACHABLE: u32 = 1 << 10;
pub const NET_V6_GATEWAY_REACHABLE: u32 = 1 << 11;

// Network configuration readiness.
//
// These bits are about "we have an address configured" (DHCPv4, SLAAC/DHCPv6), not about
// being able to ping the router. Some networks drop ICMP echo; TCP/DNS can still work.
pub const NET_CONFIGURED: u32 = 1 << 12;
pub const NET_V4_CONFIGURED: u32 = 1 << 13;
pub const NET_V6_CONFIGURED: u32 = 1 << 14;

pub const TRUEOSFS_ROOT_MOUNTED: u32 = 1 << 16;
pub const QJS_ASYNC_FS_READY: u32 = 1 << 17;
pub const GFX_VIRGL_READY: u32 = 1 << 19;
pub const LOADSCREEN_END: u32 = 1 << 20;
pub const GFX_INTEL_CLAIMED: u32 = 1 << 21;
pub const GFX_BACKEND_READY: u32 = 1 << 22;

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

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_signal_loadscreen_end() {
    set(LOADSCREEN_END);
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

/// Wait until all required flags are set, or until `timeout` elapses.
///
/// Returns `true` if the flags became ready, `false` on timeout.
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
