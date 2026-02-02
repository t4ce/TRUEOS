extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;
use core::task::Waker;
use embassy_time_driver::{now, TICK_HZ};

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

/// Return whether the current context may block.
#[inline]
pub fn can_block() -> bool {
    true
}

/// Single spin step that can be swapped for parking later.
#[inline]
pub fn spin_step() {
    core::hint::spin_loop();
}

/// Spin until `condition` is true.
#[inline]
pub fn spin_until<F: FnMut() -> bool>(mut condition: F) {
    while !condition() {
        spin_step();
    }
}

/// Spin until `condition` is true or the timeout expires.
#[inline]
pub fn spin_until_timeout<F: FnMut() -> bool>(timeout_ms: u64, mut condition: F) -> bool {
    let hz = TICK_HZ as u64;
    let ticks = if hz == 0 {
        0
    } else {
        ((timeout_ms.saturating_mul(hz) + 999) / 1000).max(1)
    };
    let deadline = now().saturating_add(ticks);

    loop {
        if condition() {
            return true;
        }
        if now() >= deadline {
            return false;
        }
        spin_step();
    }
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
