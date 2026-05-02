use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

// V-layer readiness flags.
//
// These are monotonic: once set, they are never cleared.
// Consumers can `await` prerequisites instead of guessing boot ordering.
pub const PIANO_CLAIMED: u32 = 1 << 3;

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
// These bits are about address/config state, not about being able to ping the
// router. Some networks drop ICMP echo; TCP/DNS can still work.
//
// `NET_ANY_CONFIGURED`: at least one IP stack has usable config.
// `NET_CONFIGURED`: both IPv4 and IPv6 have usable config.
pub const NET_ANY_CONFIGURED: u32 = 1 << 12;
pub const NET_V4_CONFIGURED: u32 = 1 << 13;
pub const NET_V6_CONFIGURED: u32 = 1 << 14;
pub const NET_CONFIGURED: u32 = NET_V4_CONFIGURED | NET_V6_CONFIGURED;

// Socket readiness.
//
// `NET_SOCKET_READY` means the VNet/socket surface has proven forward progress:
// an open command was processed, an Opened event returned, and selector
// readiness reached a Mio/Tokio-style socket user. This is stronger than IP
// configuration and is the right gate for Hyper/Hickory/Tokio socket clients.
pub const NET_SOCKET_READY: u32 = 1 << 15;

pub const TRUEOSFS_ROOT_MOUNTED: u32 = 1 << 16;
pub const QJS_ASYNC_FS_READY: u32 = 1 << 17;
pub const INTEL_HDA_READY: u32 = 1 << 18;
pub const GFX_VIRGL_READY: u32 = 1 << 19;
pub const HTTP_TRUEOSFS_LISTENING: u32 = 1 << 20;
pub const TOKIO_RUNTIME_READY: u32 = 1 << 21;
pub const GFX_BACKEND_READY: u32 = 1 << 22;
pub const UI2_READY: u32 = 1 << 23;
pub const APP_VM_READY: u32 = 1 << 24;
pub const GFX_TEXTURE_UPLOAD_SERVICE_READY: u32 = 1 << 25;
pub const BACKGROUND_AP_WORKER_READY: u32 = 1 << 26;
pub const VTHREAD_HW_TAG_READY: u32 = 1 << 27;

const APP_VM_READY_REQUIRED: u32 = NET_ANY_CONFIGURED | TRUEOSFS_ROOT_MOUNTED;

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
    let mut next = flags;
    if flags & APP_VM_READY_REQUIRED == APP_VM_READY_REQUIRED {
        next |= APP_VM_READY;
    }

    let prev = READY.fetch_or(next, Ordering::AcqRel);
    let combined = prev | next;
    if combined & APP_VM_READY_REQUIRED == APP_VM_READY_REQUIRED {
        READY.fetch_or(APP_VM_READY, Ordering::AcqRel);
    }
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
