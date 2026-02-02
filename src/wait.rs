extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;
use core::task::Waker;

/// Update a stored waker if it differs from the current one.
#[inline]
pub fn register_waker_slot(slot: &mut Option<Waker>, waker: &Waker) -> bool {
    let should_replace = match slot.as_ref() {
        Some(existing) => !existing.will_wake(waker),
        None => true,
    };
    if should_replace {
        *slot = Some(waker.clone());
    }
    should_replace
}

/// Register a waker into a list if it is not already present.
#[inline]
pub fn register_waker_list(list: &mut Vec<Waker>, waker: &Waker) -> bool {
    if list.iter().any(|existing| existing.will_wake(waker)) {
        return false;
    }
    list.push(waker.clone());
    true
}

/// Take a waker from a slot and wake it.
#[inline]
pub fn take_and_wake(slot: &mut Option<Waker>) -> bool {
    if let Some(waker) = slot.take() {
        waker.wake();
        return true;
    }
    false
}

/// Synchronously wait for an async future using the kernel's current strategy.
#[inline]
pub fn block_on<F: Future>(fut: F) -> F::Output {
    crate::time::block_on(fut)
}
