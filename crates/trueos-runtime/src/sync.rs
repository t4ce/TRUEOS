//! Blocking synchronization over execution-aware generation waits.
//!
//! Native panic-abort has no recoverable poisoned state. LockResult is therefore
//! infallible; it exists to make that property explicit at adaptation boundaries.
use crate::{abi, time};
pub use alloc::sync::{Arc, Weak};
use core::{
    cell::UnsafeCell,
    convert::Infallible,
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};
pub type LockResult<T> = Result<T, Infallible>;

pub struct Mutex<T: ?Sized> {
    state: AtomicU32,
    value: UnsafeCell<T>,
}
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            state: AtomicU32::new(0),
            value: UnsafeCell::new(value),
        }
    }
    pub fn into_inner(self) -> LockResult<T> {
        Ok(self.value.into_inner())
    }
}
impl<T: ?Sized> Mutex<T> {
    fn key(&self) -> u64 {
        &self.state as *const _ as u64
    }
    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            loop {
                // Observe before testing the protected state. Unlock publishes
                // zero before advancing the same generation; a wake cannot slip
                // between the last check and parking.
                let observed = unsafe { abi::trueos_tokio_platform_wait_observe(self.key()) };
                if self.state.swap(2, Ordering::Acquire) == 0 {
                    break;
                }
                unsafe {
                    abi::trueos_tokio_platform_wait_after(self.key(), observed, u64::MAX);
                }
            }
        }
        Ok(MutexGuard {
            mutex: self,
            _not_send: PhantomData,
        })
    }
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| MutexGuard {
                mutex: self,
                _not_send: PhantomData,
            })
    }
    pub fn try_lock_for(&self, duration: Duration) -> Option<MutexGuard<'_, T>> {
        if let Some(guard) = self.try_lock() {
            return Some(guard);
        }
        if duration.is_zero() {
            return None;
        }
        let start = time::Instant::now();
        loop {
            let observed = unsafe { abi::trueos_tokio_platform_wait_observe(self.key()) };
            if self.state.swap(2, Ordering::Acquire) == 0 {
                return Some(MutexGuard {
                    mutex: self,
                    _not_send: PhantomData,
                });
            }
            let remaining = duration.checked_sub(start.elapsed())?;
            if remaining.is_zero() {
                return None;
            }
            unsafe {
                abi::trueos_tokio_platform_wait_after(
                    self.key(),
                    observed,
                    time::timeout_ms(remaining),
                );
            }
        }
    }
    pub fn get_mut(&mut self) -> LockResult<&mut T> {
        Ok(self.value.get_mut())
    }
}
impl<T: ?Sized + fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.try_lock() {
            Some(v) => f.debug_struct("Mutex").field("value", &&*v).finish(),
            None => f.write_str("Mutex { <locked> }"),
        }
    }
}
impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
pub struct MutexGuard<'a, T: ?Sized> {
    mutex: &'a Mutex<T>,
    _not_send: PhantomData<*mut ()>,
}
unsafe impl<T: ?Sized + Sync> Sync for MutexGuard<'_, T> {}
impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.value.get() }
    }
}
impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.value.get() }
    }
}
impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        if self.mutex.state.swap(0, Ordering::Release) == 2 {
            unsafe {
                abi::trueos_tokio_platform_wake_one(self.mutex.key());
            }
        }
    }
}
impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

#[derive(Debug, Default)]
pub struct Condvar {
    identity: AtomicU32,
}
#[derive(Debug, Clone, Copy)]
pub struct WaitTimeoutResult(bool);
impl WaitTimeoutResult {
    pub fn timed_out(&self) -> bool {
        self.0
    }
}
impl Condvar {
    pub const fn new() -> Self {
        Self {
            identity: AtomicU32::new(0),
        }
    }
    fn key(&self) -> u64 {
        &self.identity as *const _ as u64
    }
    pub fn notify_one(&self) {
        unsafe {
            abi::trueos_tokio_platform_wake_one(self.key());
        }
    }
    pub fn notify_all(&self) {
        unsafe {
            abi::trueos_tokio_platform_wake_all(self.key());
        }
    }
    pub fn wait<'a, T: ?Sized>(&self, guard: MutexGuard<'a, T>) -> LockResult<MutexGuard<'a, T>> {
        self.wait_inner(guard, u64::MAX).map(|v| v.0)
    }
    pub fn wait_timeout<'a, T: ?Sized>(
        &self,
        guard: MutexGuard<'a, T>,
        duration: Duration,
    ) -> LockResult<(MutexGuard<'a, T>, WaitTimeoutResult)> {
        self.wait_inner(guard, time::timeout_ms(duration))
    }
    fn wait_inner<'a, T: ?Sized>(
        &self,
        guard: MutexGuard<'a, T>,
        ms: u64,
    ) -> LockResult<(MutexGuard<'a, T>, WaitTimeoutResult)> {
        let mutex = guard.mutex;
        let observed = unsafe { abi::trueos_tokio_platform_wait_observe(self.key()) };
        drop(guard);
        let notified = unsafe { abi::trueos_tokio_platform_wait_after(self.key(), observed, ms) };
        Ok((mutex.lock().unwrap(), WaitTimeoutResult(!notified)))
    }
}

#[derive(Debug)]
struct RwState {
    readers: usize,
    writer: bool,
    waiting_writers: usize,
}
pub struct RwLock<T: ?Sized> {
    state: Mutex<RwState>,
    changed: Condvar,
    value: UnsafeCell<T>,
}
unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}
impl<T> RwLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            state: Mutex::new(RwState {
                readers: 0,
                writer: false,
                waiting_writers: 0,
            }),
            changed: Condvar::new(),
            value: UnsafeCell::new(value),
        }
    }
}
impl<T: ?Sized> RwLock<T> {
    pub fn read(&self) -> LockResult<RwLockReadGuard<'_, T>> {
        let mut s = self.state.lock().unwrap();
        while s.writer || s.waiting_writers != 0 {
            s = self.changed.wait(s).unwrap();
        }
        s.readers = s.readers.checked_add(1).expect("reader count overflow");
        Ok(RwLockReadGuard {
            lock: self,
            _not_send: PhantomData,
        })
    }
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        let mut s = self.state.try_lock()?;
        if s.writer || s.waiting_writers != 0 {
            return None;
        }
        s.readers = s.readers.checked_add(1).expect("reader count overflow");
        Some(RwLockReadGuard {
            lock: self,
            _not_send: PhantomData,
        })
    }
    pub fn write(&self) -> LockResult<RwLockWriteGuard<'_, T>> {
        let mut s = self.state.lock().unwrap();
        s.waiting_writers += 1;
        while s.writer || s.readers != 0 {
            s = self.changed.wait(s).unwrap();
        }
        s.waiting_writers -= 1;
        s.writer = true;
        Ok(RwLockWriteGuard {
            lock: self,
            _not_send: PhantomData,
        })
    }
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        let mut s = self.state.try_lock()?;
        if s.writer || s.readers != 0 {
            return None;
        }
        s.writer = true;
        Some(RwLockWriteGuard {
            lock: self,
            _not_send: PhantomData,
        })
    }
}
impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLock<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.try_read() {
            Some(v) => f.debug_struct("RwLock").field("value", &&*v).finish(),
            None => f.write_str("RwLock { <locked> }"),
        }
    }
}
pub struct RwLockReadGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
    _not_send: PhantomData<*mut ()>,
}
pub struct RwLockWriteGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
    _not_send: PhantomData<*mut ()>,
}
impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}
impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}
impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.value.get() }
    }
}
impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        let mut s = self.lock.state.lock().unwrap();
        s.readers -= 1;
        let notify = s.readers == 0;
        drop(s);
        if notify {
            self.lock.changed.notify_all();
        }
    }
}
impl<T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.lock().unwrap().writer = false;
        self.lock.changed.notify_all();
    }
}

/// One-time publication. A native panic aborts the process rather than leaving
/// a recoverable poisoned initializer behind.
#[derive(Debug)]
pub struct Once {
    state: AtomicU32,
}
impl Once {
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(0),
        }
    }
    pub fn is_completed(&self) -> bool {
        self.state.load(Ordering::Acquire) == 2
    }
    pub fn call_once(&self, f: impl FnOnce()) {
        if self.is_completed() {
            return;
        }
        let key = &self.state as *const _ as u64;
        loop {
            let observed = unsafe { abi::trueos_tokio_platform_wait_observe(key) };
            match self
                .state
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Acquire)
            {
                Ok(_) => {
                    f();
                    self.state.store(2, Ordering::Release);
                    unsafe {
                        abi::trueos_tokio_platform_wake_all(key);
                    }
                    return;
                }
                Err(2) => return,
                Err(_) => unsafe {
                    abi::trueos_tokio_platform_wait_after(key, observed, u64::MAX);
                },
            }
        }
    }
}
impl Default for Once {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockReadGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockWriteGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
unsafe impl<T: ?Sized + Sync> Sync for RwLockReadGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for RwLockWriteGuard<'_, T> {}
