//! Passive, payload-free boundary observations. One pinned continuation writes
//! its record; observers only read atomics. These are elapsed boundaries, not
//! CPU samples. No allocator, lock, formatter or logger is used on entry/exit.
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
#[repr(u64)]
pub(crate) enum AbiOperation {
    None = 0,
    Alloc = 1,
    Dealloc = 2,
    Realloc = 3,
    Mutex = 4,
    Condvar = 5,
    Stat = 6,
    Mkdir = 7,
    Wait = 8,
    Sleep = 9,
    Yield = 10,
}
impl AbiOperation {
    pub(crate) fn name(value: u64) -> &'static str {
        match value {
            0 => "none",
            1 => "alloc",
            2 => "dealloc",
            3 => "realloc",
            4 => "mutex",
            5 => "condvar",
            6 => "stat",
            7 => "mkdir",
            8 => "wait",
            9 => "sleep",
            10 => "yield",
            _ => "unknown",
        }
    }
}

pub(super) struct State {
    sequence: AtomicU64,
    operation: AtomicU64,
    since_ms: AtomicU64,
    depth: AtomicU64,
    entered: AtomicU64,
    exited: AtomicU64,
    last: AtomicU64,
    last_exit_ms: AtomicU64,
}
impl State {
    pub(super) const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            operation: AtomicU64::new(0),
            since_ms: AtomicU64::new(0),
            depth: AtomicU64::new(0),
            entered: AtomicU64::new(0),
            exited: AtomicU64::new(0),
            last: AtomicU64::new(0),
            last_exit_ms: AtomicU64::new(0),
        }
    }
    /// The owning Thread/JobDiagnostic must outlive the guard. It is safe to
    /// discard the complete suspended stack without dropping its guards; never
    /// resume that stack after its owner/record has been retired.
    pub(super) unsafe fn enter(&self, operation: AbiOperation, now: u64) -> Guard {
        // A same-carrier interrupt may allocate while a scalar publication is
        // in progress. Skip that nested observation rather than making an odd
        // publication appear stable; there is no retry or interrupt masking.
        if self.sequence.load(Ordering::Relaxed) & 1 != 0 {
            return Guard::inactive();
        }
        let previous = (
            self.operation.load(Ordering::Relaxed),
            self.since_ms.load(Ordering::Relaxed),
            self.depth.load(Ordering::Relaxed),
        );
        self.sequence.fetch_add(1, Ordering::AcqRel);
        self.operation.store(operation as u64, Ordering::Relaxed);
        self.since_ms.store(now, Ordering::Relaxed);
        self.depth
            .store(previous.2.saturating_add(1), Ordering::Relaxed);
        self.entered.fetch_add(1, Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
        Guard {
            state: self,
            previous,
            operation,
            _pinned: core::marker::PhantomData,
        }
    }
    pub(super) fn snapshot(&self) -> Option<Snapshot> {
        let sequence = self.sequence.load(Ordering::Acquire);
        let result = Snapshot {
            sequence,
            operation: self.operation.load(Ordering::Relaxed),
            since_ms: self.since_ms.load(Ordering::Relaxed),
            depth: self.depth.load(Ordering::Relaxed),
            entered: self.entered.load(Ordering::Relaxed),
            exited: self.exited.load(Ordering::Relaxed),
            last: self.last.load(Ordering::Relaxed),
            last_exit_ms: self.last_exit_ms.load(Ordering::Relaxed),
        };
        core::sync::atomic::fence(Ordering::Acquire);
        (sequence & 1 == 0 && sequence == self.sequence.load(Ordering::Acquire)).then_some(result)
    }
}
pub(super) struct Snapshot {
    pub sequence: u64,
    pub operation: u64,
    pub since_ms: u64,
    pub depth: u64,
    pub entered: u64,
    pub exited: u64,
    pub last: u64,
    pub last_exit_ms: u64,
}

pub(crate) struct Guard {
    state: *const State,
    previous: (u64, u64, u64),
    operation: AbiOperation,
    // Guards may suspend on their owning pinned stack but cannot be sent to a
    // different continuation, whose current-record identity would differ.
    _pinned: core::marker::PhantomData<*mut ()>,
}
impl Guard {
    pub(crate) const fn inactive() -> Self {
        Self {
            state: core::ptr::null(),
            previous: (0, 0, 0),
            operation: AbiOperation::None,
            _pinned: core::marker::PhantomData,
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if self.state.is_null() {
            return;
        }
        let state = unsafe { &*self.state };
        state.sequence.fetch_add(1, Ordering::AcqRel);
        state.operation.store(self.previous.0, Ordering::Relaxed);
        state.since_ms.store(self.previous.1, Ordering::Relaxed);
        state.depth.store(self.previous.2, Ordering::Relaxed);
        state.last.store(self.operation as u64, Ordering::Relaxed);
        state.last_exit_ms.store(super::now_ms(), Ordering::Relaxed);
        state.exited.fetch_add(1, Ordering::Relaxed);
        state.sequence.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_wait_remains_visible_and_restores_original_entry() {
        let state = State::new();
        let outer = unsafe { state.enter(AbiOperation::Condvar, 10) };
        let inner = unsafe { state.enter(AbiOperation::Wait, 25) };
        let s = state.snapshot().unwrap();
        assert_eq!((s.operation, s.since_ms, s.depth, s.entered, s.exited), (8, 25, 2, 2, 0));
        drop(inner);
        let s = state.snapshot().unwrap();
        assert_eq!((s.operation, s.since_ms, s.depth, s.last), (5, 10, 1, 8));
        drop(outer);
        let s = state.snapshot().unwrap();
        assert_eq!((s.operation, s.depth, s.entered, s.exited, s.last), (0, 0, 2, 2, 5));
    }
    #[test]
    fn incomplete_publication_is_never_formatted_as_stable() {
        let state = State::new();
        state.sequence.store(1, Ordering::Release);
        assert!(state.snapshot().is_none());
        let skipped = unsafe { state.enter(AbiOperation::Alloc, 3) };
        drop(skipped);
        assert_eq!(state.sequence.load(Ordering::Relaxed), 1);
        assert_eq!(state.entered.load(Ordering::Relaxed), 0);
        drop(Guard::inactive());
    }
}
