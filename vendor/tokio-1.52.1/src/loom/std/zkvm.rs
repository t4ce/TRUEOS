use std::cell::{Cell, UnsafeCell};
use std::fmt;
use std::hint::spin_loop;
use std::ops::{Deref, DerefMut};
use std::sync::LockResult;
use std::time::Duration;

#[derive(Debug, Copy, Clone)]
pub(crate) struct WaitTimeoutResult(bool);

impl WaitTimeoutResult {
    #[inline]
    pub(crate) fn timed_out(self) -> bool {
        self.0
    }
}

#[derive(Debug)]
pub(crate) struct Mutex<T: ?Sized> {
    locked: Cell<bool>,
    value: UnsafeCell<T>,
}

#[derive(Debug)]
pub(crate) struct MutexGuard<'a, T: ?Sized> {
    mutex: &'a Mutex<T>,
}

#[derive(Debug)]
pub(crate) struct RwLock<T: ?Sized> {
    readers: Cell<usize>,
    writer: Cell<bool>,
    value: UnsafeCell<T>,
}

#[derive(Debug)]
pub(crate) struct RwLockReadGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
}

#[derive(Debug)]
pub(crate) struct RwLockWriteGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
}

#[derive(Debug)]
pub(crate) struct Condvar {
    seq: Cell<u64>,
}

unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}
unsafe impl<T: ?Sized + Send + Sync> Send for RwLock<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}
unsafe impl Send for Condvar {}
unsafe impl Sync for Condvar {}

impl<T> Mutex<T> {
    #[inline]
    pub(crate) fn new(t: T) -> Mutex<T> {
        Mutex {
            locked: Cell::new(false),
            value: UnsafeCell::new(t),
        }
    }

    #[inline]
    #[cfg(not(all(loom, test)))]
    pub(crate) const fn const_new(t: T) -> Mutex<T> {
        Mutex {
            locked: Cell::new(false),
            value: UnsafeCell::new(t),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    #[inline]
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        while self.locked.replace(true) {
            spin_loop();
        }

        MutexGuard { mutex: self }
    }

    #[inline]
    pub(crate) fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        if self.locked.replace(true) {
            self.locked.set(true);
            None
        } else {
            Some(MutexGuard { mutex: self })
        }
    }

    #[inline]
    pub(crate) fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    #[inline]
    fn unlock(&self) {
        self.locked.set(false);
    }
}

impl<'a, T: ?Sized> Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}

impl<T> RwLock<T> {
    #[inline]
    pub(crate) fn new(t: T) -> RwLock<T> {
        RwLock {
            readers: Cell::new(0),
            writer: Cell::new(false),
            value: UnsafeCell::new(t),
        }
    }
}

impl<T: ?Sized> RwLock<T> {
    #[inline]
    pub(crate) fn read(&self) -> RwLockReadGuard<'_, T> {
        while self.writer.get() {
            spin_loop();
        }
        self.readers.set(self.readers.get().saturating_add(1));
        RwLockReadGuard { lock: self }
    }

    #[inline]
    pub(crate) fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        if self.writer.get() {
            None
        } else {
            self.readers.set(self.readers.get().saturating_add(1));
            Some(RwLockReadGuard { lock: self })
        }
    }

    #[inline]
    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, T> {
        while self.writer.get() || self.readers.get() != 0 {
            spin_loop();
        }
        self.writer.set(true);
        RwLockWriteGuard { lock: self }
    }

    #[inline]
    pub(crate) fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        if self.writer.get() || self.readers.get() != 0 {
            None
        } else {
            self.writer.set(true);
            Some(RwLockWriteGuard { lock: self })
        }
    }
}

impl<'a, T: ?Sized> Deref for RwLockReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}

impl<'a, T: ?Sized> Drop for RwLockReadGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.readers.set(self.lock.readers.get().saturating_sub(1));
    }
}

impl<'a, T: ?Sized> Deref for RwLockWriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for RwLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<'a, T: ?Sized> Drop for RwLockWriteGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.writer.set(false);
    }
}

impl Condvar {
    #[inline]
    pub(crate) fn new() -> Condvar {
        Condvar { seq: Cell::new(0) }
    }

    #[inline]
    pub(crate) fn notify_one(&self) {
        self.seq.set(self.seq.get().wrapping_add(1));
    }

    #[inline]
    pub(crate) fn notify_all(&self) {
        self.notify_one();
    }

    #[inline]
    pub(crate) fn wait<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
    ) -> LockResult<MutexGuard<'a, T>> {
        let observed = self.seq.get();
        let mutex = guard.mutex;
        drop(guard);

        while self.seq.get() == observed {
            spin_loop();
        }

        Ok(mutex.lock())
    }

    #[inline]
    pub(crate) fn wait_timeout<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout: Duration,
    ) -> LockResult<(MutexGuard<'a, T>, WaitTimeoutResult)> {
        let observed = self.seq.get();
        let deadline = deadline_after(timeout);
        let mutex = guard.mutex;
        drop(guard);

        while self.seq.get() == observed {
            if deadline_reached(deadline) {
                return Ok((mutex.lock(), WaitTimeoutResult(true)));
            }
            spin_loop();
        }

        Ok((mutex.lock(), WaitTimeoutResult(false)))
    }
}

impl<'a, T: ?Sized + fmt::Display> fmt::Display for MutexGuard<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'a, T: ?Sized + fmt::Display> fmt::Display for RwLockReadGuard<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'a, T: ?Sized + fmt::Display> fmt::Display for RwLockWriteGuard<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

#[inline]
fn deadline_after(timeout: Duration) -> u64 {
    now_nanos().saturating_add(duration_as_nanos(timeout))
}

#[inline]
fn deadline_reached(deadline: u64) -> bool {
    now_nanos() >= deadline
}

#[inline]
fn duration_as_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[inline]
fn now_nanos() -> u64 {
    unsafe { trueos_tokio_time_now_nanos() }
}

unsafe extern "C" {
    fn trueos_tokio_time_now_nanos() -> u64;
}