//! The default garbage collector.
//!
//! For each thread, a participant is lazily initialized on its first use, when the current thread
//! is registered in the default collector.  If initialized, the thread's participant will get
//! destructed on thread exit, which in turn unregisters the thread.

use crate::collector::{Collector, LocalHandle};
use crate::guard::Guard;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::primitive::thread_local;
#[cfg(not(crossbeam_loom))]
use crate::sync::once_lock::OnceLock;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::boxed::Box;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use core::ptr;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use core::sync::atomic::{AtomicPtr, Ordering};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe extern "Rust" {
    fn trueos_tokio_tls_current_slot() -> u32;
}

fn collector() -> &'static Collector {
    #[cfg(not(crossbeam_loom))]
    {
        /// The global data for the default garbage collector.
        static COLLECTOR: OnceLock<Collector> = OnceLock::new();
        COLLECTOR.get_or_init(Collector::new)
    }
    // FIXME: loom does not currently provide the equivalent of Lazy:
    // https://github.com/tokio-rs/loom/issues/263
    #[cfg(crossbeam_loom)]
    {
        loom::lazy_static! {
            /// The global data for the default garbage collector.
            static ref COLLECTOR: Collector = Collector::new();
        }
        &COLLECTOR
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
thread_local! {
    /// The per-thread participant for the default garbage collector.
    static HANDLE: LocalHandle = collector().register();
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const TRUEOS_HANDLE_SLOTS: usize = 256;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
static TRUEOS_HANDLES: [AtomicPtr<LocalHandle>; TRUEOS_HANDLE_SLOTS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; TRUEOS_HANDLE_SLOTS];

/// Pins the current thread.
#[inline]
pub fn pin() -> Guard {
    with_handle(|handle| handle.pin())
}

/// Returns `true` if the current thread is pinned.
#[inline]
pub fn is_pinned() -> bool {
    with_handle(|handle| handle.is_pinned())
}

/// Returns the default global collector.
pub fn default_collector() -> &'static Collector {
    collector()
}

#[inline]
fn with_handle<F, R>(mut f: F) -> R
where
    F: FnMut(&LocalHandle) -> R,
{
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let raw_key = unsafe { trueos_tokio_tls_current_slot() } as usize;
        let key = raw_key.min(TRUEOS_HANDLE_SLOTS - 1);
        let slot = &TRUEOS_HANDLES[key];

        let mut ptr = slot.load(Ordering::Acquire);
        if ptr.is_null() {
            let candidate = Box::into_raw(Box::new(collector().register()));
            match slot.compare_exchange(
                ptr::null_mut(),
                candidate,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => ptr = candidate,
                Err(existing) => {
                    unsafe {
                        drop(Box::from_raw(candidate));
                    }
                    ptr = existing;
                }
            }
        }

        return f(unsafe { &*ptr });
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    HANDLE
        .try_with(|h| f(h))
        .unwrap_or_else(|_| f(&collector().register()))
}

#[cfg(all(test, not(crossbeam_loom)))]
mod tests {
    use crossbeam_utils::thread;

    #[test]
    fn pin_while_exiting() {
        struct Foo;

        impl Drop for Foo {
            fn drop(&mut self) {
                // Pin after `HANDLE` has been dropped. This must not panic.
                super::pin();
            }
        }

        thread_local! {
            static FOO: Foo = const { Foo };
        }

        thread::scope(|scope| {
            scope.spawn(|_| {
                // Initialize `FOO` and then `HANDLE`.
                FOO.with(|_| ());
                super::pin();
                // At thread exit, `HANDLE` gets dropped first and `FOO` second.
            });
        })
        .unwrap();
    }
}
