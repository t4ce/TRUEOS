//! Synchronization helpers for synchronous VM-exit handling.

use spin::{Mutex, MutexGuard};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleInterrupted;

/// Acquire a VM-root spin mutex without making lifecycle stop/preserve wait
/// indefinitely behind another holder.
///
/// This is deliberately cooperative: an interrupt/request only publishes the
/// VM lifecycle state. The waiter observes it here and returns through normal
/// Rust control flow, without abandoning a live stack frame or held guard.
pub(crate) fn lock<'a, T: ?Sized>(
    vm_id: u8,
    mutex: &'a Mutex<T>,
) -> Result<MutexGuard<'a, T>, LifecycleInterrupted> {
    loop {
        if crate::hv::lifecycle_request_pending(vm_id) {
            return Err(LifecycleInterrupted);
        }

        if let Some(guard) = mutex.try_lock() {
            if crate::hv::lifecycle_request_pending(vm_id) {
                drop(guard);
                return Err(LifecycleInterrupted);
            }
            return Ok(guard);
        }

        core::hint::spin_loop();
    }
}
