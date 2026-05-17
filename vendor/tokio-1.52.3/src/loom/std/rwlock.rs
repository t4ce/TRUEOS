#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use core::{
    cell::UnsafeCell,
    fmt,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicIsize, Ordering},
};

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::sync::{self, RwLockReadGuard, RwLockWriteGuard, TryLockError};

/// Adapter for `std::sync::RwLock` that removes the poisoning aspects
/// from its api.
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
#[derive(Debug)]
pub(crate) struct RwLock<T: ?Sized>(sync::RwLock<T>);

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub(crate) struct RwLock<T: ?Sized> {
    state: AtomicIsize,
    value: UnsafeCell<T>,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub(crate) struct RwLockReadGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub(crate) struct RwLockWriteGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockReadGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockWriteGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLock<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RwLock").finish_non_exhaustive()
    }
}

#[allow(dead_code)]
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl<T> RwLock<T> {
    #[inline]
    pub(crate) fn new(t: T) -> Self {
        Self(sync::RwLock::new(t))
    }

    #[inline]
    pub(crate) fn read(&self) -> RwLockReadGuard<'_, T> {
        match self.0.read() {
            Ok(guard) => guard,
            Err(p_err) => p_err.into_inner(),
        }
    }

    #[inline]
    pub(crate) fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        match self.0.try_read() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(p_err)) => Some(p_err.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    #[inline]
    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, T> {
        match self.0.write() {
            Ok(guard) => guard,
            Err(p_err) => p_err.into_inner(),
        }
    }

    #[inline]
    pub(crate) fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        match self.0.try_write() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(p_err)) => Some(p_err.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }
}

#[allow(dead_code)]
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T> RwLock<T> {
    #[inline]
    pub(crate) fn new(t: T) -> Self {
        Self {
            state: AtomicIsize::new(0),
            value: UnsafeCell::new(t),
        }
    }
}

#[allow(dead_code)]
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> RwLock<T> {
    #[inline]
    pub(crate) fn read(&self) -> RwLockReadGuard<'_, T> {
        loop {
            if let Some(guard) = self.try_read() {
                return guard;
            }
            crate::platform::note_semantic_gap(crate::platform::SEMANTIC_GAP_MUTEX_SPIN);
            core::hint::spin_loop();
        }
    }

    #[inline]
    pub(crate) fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state < 0 {
                return None;
            }

            match self.state.compare_exchange_weak(
                state,
                state + 1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(RwLockReadGuard { lock: self }),
                Err(next) => state = next,
            }
        }
    }

    #[inline]
    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, T> {
        loop {
            if let Some(guard) = self.try_write() {
                return guard;
            }
            crate::platform::note_semantic_gap(crate::platform::SEMANTIC_GAP_MUTEX_SPIN);
            core::hint::spin_loop();
        }
    }

    #[inline]
    pub(crate) fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        self.state
            .compare_exchange(0, -1, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| RwLockWriteGuard { lock: self })
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.fetch_sub(1, Ordering::Release);
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.store(0, Ordering::Release);
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.value.get() }
    }
}
