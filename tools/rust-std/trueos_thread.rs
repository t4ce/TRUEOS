// Reference source for `library/std/src/sys/thread/trueos.rs`.
//
// TRUEOS is a concurrent target, but POSIX/OS thread lifecycle is not a native
// execution primitive. Keep `target_has_threads` truthful for atomics and
// memory concurrency while making `std::thread::spawn` fail explicitly.

use crate::ffi::CStr;
use crate::io;
use crate::num::NonZero;
use crate::thread::ThreadInit;
use crate::time::Duration;

unsafe extern "C" {
    fn trueos_cabi_poll_once();
    fn trueos_cabi_sleep_ms(ms: u64);
}

/// Native std thread handle.
///
/// This is deliberately uninhabited: TRUEOS does not manufacture a native
/// std/OS thread object. `Thread::new` always returns `UNSUPPORTED_PLATFORM`.
pub struct Thread(!);

// Keep the common ThreadInit entry point referenced on this unsupported
// lifecycle target, just as std's generic unsupported backend does.
#[expect(dead_code)]
fn dummy_init_call(init: Box<ThreadInit>) {
    drop(init.init());
}

pub const DEFAULT_MIN_STACK_SIZE: usize = 64 * 1024;

impl Thread {
    // SAFETY: see `std::thread::Builder::spawn_unchecked` for the caller's
    // requirements. TRUEOS never consumes `init` because no native thread is
    // created.
    pub unsafe fn new(_stack: usize, _init: Box<ThreadInit>) -> io::Result<Thread> {
        Err(io::Error::UNSUPPORTED_PLATFORM)
    }

    pub fn join(self) {
        self.0
    }
}

/// Do not infer TRUEOS worker/carrier capacity from `std::thread`.
///
/// Native parallel capacity belongs to the TRUEOS execution layer, not the
/// standard library's OS-thread model. Callers that want a fallback commonly
/// map this error to one logical execution lane.
pub fn available_parallelism() -> io::Result<NonZero<usize>> {
    Err(io::Error::UNKNOWN_THREAD_COUNT)
}

/// TRUEOS has logical execution identities, but no native std/OS-thread ID.
pub fn current_os_id() -> Option<u64> {
    None
}

/// There is no native std thread to name.
pub fn set_name(_name: &CStr) {}

/// Best-effort synchronous yield for code expressed through `std::thread`.
///
/// This does not create or switch an OS thread. It gives the TRUEOS runtime a
/// chance to make platform progress on the current execution lane.
pub fn yield_now() {
    unsafe { trueos_cabi_poll_once() }
}

/// Synchronous sleep remains meaningful even though native thread creation is
/// unsupported. Round up to milliseconds so the call never returns earlier
/// solely because the TRUEOS CABI has millisecond granularity.
pub fn sleep(dur: Duration) {
    let mut millis = dur.as_millis();
    if dur.subsec_nanos() % 1_000_000 != 0 {
        millis += 1;
    }

    while millis != 0 {
        let chunk = crate::cmp::min(millis, u64::MAX as u128) as u64;
        unsafe { trueos_cabi_sleep_ms(chunk) };
        millis -= chunk as u128;
    }
}
