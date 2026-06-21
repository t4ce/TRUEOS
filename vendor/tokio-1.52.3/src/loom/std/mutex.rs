#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use core::{
    cell::UnsafeCell,
    fmt,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::sync::{self, MutexGuard, TryLockError};

/// Adapter for `std::Mutex` that removes the poisoning aspects
/// from its API.
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
#[derive(Debug)]
pub(crate) struct Mutex<T: ?Sized>(sync::Mutex<T>);

// TRUEOS/zkvm deliberately do not use `std::sync::Mutex` here. This is a
// minimal Core/Platform bridge for Tokio internals: no poisoning, no parking,
// no fairness, and no blocking wait queue. That matches Tokio's loom adapter
// shape well enough for short internal critical sections, but long waits should
// grow a real kernel RawMutex or wait-aware lock instead of becoming pthreads.
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub(crate) struct Mutex<T: ?Sized> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub(crate) struct MutexGuard<'a, T: ?Sized> {
    mutex: &'a Mutex<T>,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized + fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mutex").finish_non_exhaustive()
    }
}

#[allow(dead_code)]
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl<T> Mutex<T> {
    #[inline]
    pub(crate) fn new(t: T) -> Mutex<T> {
        Mutex(sync::Mutex::new(t))
    }

    #[inline]
    pub(crate) const fn const_new(t: T) -> Mutex<T> {
        Mutex(sync::Mutex::new(t))
    }

    #[inline]
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        match self.0.lock() {
            Ok(guard) => guard,
            Err(p_err) => p_err.into_inner(),
        }
    }

    #[inline]
    pub(crate) fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.0.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(p_err)) => Some(p_err.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }
}

#[allow(dead_code)]
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T> Mutex<T> {
    #[inline]
    pub(crate) fn new(t: T) -> Mutex<T> {
        Self::const_new(t)
    }

    #[inline]
    pub(crate) const fn const_new(t: T) -> Mutex<T> {
        Mutex {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(t),
        }
    }

    #[inline]
    pub(crate) fn into_inner(self) -> Result<T, core::convert::Infallible> {
        Ok(self.value.into_inner())
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[allow(dead_code)]
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> Mutex<T> {
    #[inline]
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            let key = self as *const Mutex<T> as *const () as usize as u64;
            let observed = crate::platform::wait_observe(key);
            if self
                .locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return MutexGuard { mutex: self };
            }
            let _ = crate::platform::wait_after(key, observed, 0);
        }

        MutexGuard { mutex: self }
    }

    #[inline]
    pub(crate) fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        let guard = self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| MutexGuard { mutex: self });
        if guard.is_none() {
            crate::platform::note_semantic_gap(crate::platform::SEMANTIC_GAP_MUTEX_SPIN);
        }
        guard
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<'a, T: ?Sized> MutexGuard<'a, T> {
    #[inline]
    pub(crate) fn unwrap(self) -> Self {
        self
    }

    #[inline]
    pub(crate) fn expect(self, _: &str) -> Self {
        self
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.store(false, Ordering::Release);
        let key = self.mutex as *const Mutex<T> as *const () as usize as u64;
        let _ = crate::platform::wake_one(key);
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.value.get() }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.value.get() }
    }
}
