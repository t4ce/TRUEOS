//! A `Barrier` that provides `wait_timeout`.
//!
//! This implementation mirrors that of the Rust standard library.

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::loom::sync::Condvar;
use crate::loom::sync::Mutex;
use std::fmt;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use core::time::Duration;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use core::time::Duration;
use std::time::Instant;

/// A barrier enables multiple threads to synchronize the beginning
/// of some computation.
///
/// # Examples
///
/// ```
/// # #[cfg(not(target_family = "wasm"))]
/// # {
/// use std::sync::{Arc, Barrier};
/// use std::thread;
///
/// let mut handles = Vec::with_capacity(10);
/// let barrier = Arc::new(Barrier::new(10));
/// for _ in 0..10 {
///     let c = Arc::clone(&barrier);
///     // The same messages will be printed together.
///     // You will NOT see any interleaving.
///     handles.push(thread::spawn(move|| {
///         println!("before wait");
///         c.wait();
///         println!("after wait");
///     }));
/// }
/// // Wait for other threads to finish.
/// for handle in handles {
///     handle.join().unwrap();
/// }
/// # }
/// ```
pub(crate) struct Barrier {
    lock: Mutex<BarrierState>,
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    cvar: Condvar,
    num_threads: usize,
}

// The inner state of a double barrier
struct BarrierState {
    count: usize,
    generation_id: usize,
}

/// A `BarrierWaitResult` is returned by [`Barrier::wait()`] when all threads
/// in the [`Barrier`] have rendezvoused.
///
/// # Examples
///
/// ```
/// use std::sync::Barrier;
///
/// let barrier = Barrier::new(1);
/// let barrier_wait_result = barrier.wait();
/// ```
pub(crate) struct BarrierWaitResult(bool);

impl fmt::Debug for Barrier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Barrier").finish_non_exhaustive()
    }
}

impl Barrier {
    /// Creates a new barrier that can block a given number of threads.
    ///
    /// A barrier will block `n`-1 threads which call [`wait()`] and then wake
    /// up all threads at once when the `n`th thread calls [`wait()`].
    ///
    /// [`wait()`]: Barrier::wait
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Barrier;
    ///
    /// let barrier = Barrier::new(10);
    /// ```
    #[must_use]
    pub(crate) fn new(n: usize) -> Barrier {
        Barrier {
            lock: Mutex::new(BarrierState {
                count: 0,
                generation_id: 0,
            }),
            #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
            cvar: Condvar::new(),
            num_threads: n,
        }
    }

    /// Blocks the current thread until all threads have rendezvoused here.
    ///
    /// Barriers are re-usable after all threads have rendezvoused once, and can
    /// be used continuously.
    ///
    /// A single (arbitrary) thread will receive a [`BarrierWaitResult`] that
    /// returns `true` from [`BarrierWaitResult::is_leader()`] when returning
    /// from this function, and all other threads will receive a result that
    /// will return `false` from [`BarrierWaitResult::is_leader()`].
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(not(target_family = "wasm"))]
    /// # {
    /// use std::sync::{Arc, Barrier};
    /// use std::thread;
    ///
    /// let mut handles = Vec::with_capacity(10);
    /// let barrier = Arc::new(Barrier::new(10));
    /// for _ in 0..10 {
    ///     let c = Arc::clone(&barrier);
    ///     // The same messages will be printed together.
    ///     // You will NOT see any interleaving.
    ///     handles.push(thread::spawn(move|| {
    ///         println!("before wait");
    ///         c.wait();
    ///         println!("after wait");
    ///     }));
    /// }
    /// // Wait for other threads to finish.
    /// for handle in handles {
    ///     handle.join().unwrap();
    /// }
    /// # }
    /// ```
    pub(crate) fn wait(&self) -> BarrierWaitResult {
        let mut lock = self.lock.lock();
        let local_gen = lock.generation_id;
        lock.count += 1;
        if lock.count < self.num_threads {
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            {
                crate::platform::note_semantic_gap(crate::platform::SEMANTIC_GAP_BARRIER_POLL);
                loop {
                    drop(lock);
                    crate::platform::sleep_ms(1);
                    lock = self.lock.lock();
                    if local_gen != lock.generation_id {
                        break;
                    }
                }
            }

            #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
            // We need a while loop to guard against spurious wakeups.
            // https://en.wikipedia.org/wiki/Spurious_wakeup
            while local_gen == lock.generation_id {
                lock = self.cvar.wait(lock).unwrap();
            }
            BarrierWaitResult(false)
        } else {
            lock.count = 0;
            lock.generation_id = lock.generation_id.wrapping_add(1);
            #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
            self.cvar.notify_all();
            BarrierWaitResult(true)
        }
    }

    /// Blocks the current thread until all threads have rendezvoused here for
    /// at most `timeout` duration.
    pub(crate) fn wait_timeout(&self, timeout: Duration) -> Option<BarrierWaitResult> {
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            crate::platform::note_semantic_gap(crate::platform::SEMANTIC_GAP_BARRIER_POLL);
            let deadline =
                crate::platform::monotonic_nanos().saturating_add(duration_to_nanos(timeout));

            let mut lock = loop {
                if let Some(guard) = self.lock.try_lock() {
                    break guard;
                }

                let now = crate::platform::monotonic_nanos();
                if now >= deadline {
                    return None;
                }
                crate::platform::sleep_ms(remaining_sleep_ms(deadline.saturating_sub(now)));
            };

            let local_gen = lock.generation_id;
            lock.count += 1;
            if lock.count < self.num_threads {
                loop {
                    drop(lock);

                    let now = crate::platform::monotonic_nanos();
                    if now >= deadline {
                        return None;
                    }
                    crate::platform::sleep_ms(remaining_sleep_ms(deadline.saturating_sub(now)));

                    lock = self.lock.lock();
                    if local_gen != lock.generation_id {
                        return Some(BarrierWaitResult(false));
                    }
                }
            }

            lock.count = 0;
            lock.generation_id = lock.generation_id.wrapping_add(1);
            return Some(BarrierWaitResult(true));
        }

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            // This implementation mirrors `wait`, but with each blocking operation
            // replaced by a timeout-amenable alternative.

            let deadline = Instant::now() + timeout;

            // Acquire `self.lock` with at most `timeout` duration.
            let mut lock = loop {
                if let Some(guard) = self.lock.try_lock() {
                    break guard;
                } else if Instant::now() > deadline {
                    return None;
                } else {
                    std::thread::yield_now();
                }
            };

            // Shrink the `timeout` to account for the time taken to acquire `lock`.
            let timeout = deadline.saturating_duration_since(Instant::now());

            let local_gen = lock.generation_id;
            lock.count += 1;
            if lock.count < self.num_threads {
                // We need a while loop to guard against spurious wakeups.
                // https://en.wikipedia.org/wiki/Spurious_wakeup
                while local_gen == lock.generation_id {
                    let (guard, timeout_result) = self.cvar.wait_timeout(lock, timeout).unwrap();
                    lock = guard;
                    if timeout_result.timed_out() {
                        return None;
                    }
                }
                Some(BarrierWaitResult(false))
            } else {
                lock.count = 0;
                lock.generation_id = lock.generation_id.wrapping_add(1);
                self.cvar.notify_all();
                Some(BarrierWaitResult(true))
            }
        }
    }
}

impl fmt::Debug for BarrierWaitResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BarrierWaitResult")
            .field("is_leader", &self.is_leader())
            .finish()
    }
}

impl BarrierWaitResult {
    /// Returns `true` if this thread is the "leader thread" for the call to
    /// [`Barrier::wait()`].
    ///
    /// Only one thread will have `true` returned from their result, all other
    /// threads will have `false` returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Barrier;
    ///
    /// let barrier = Barrier::new(1);
    /// let barrier_wait_result = barrier.wait();
    /// println!("{:?}", barrier_wait_result.is_leader());
    /// ```
    #[must_use]
    pub(crate) fn is_leader(&self) -> bool {
        self.0
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn duration_to_nanos(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(duration.subsec_nanos()))
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn remaining_sleep_ms(remaining_nanos: u64) -> u64 {
    (remaining_nanos / 1_000_000).min(1)
}
