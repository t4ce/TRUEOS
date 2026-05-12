//! Provides thread-safe, concurrent cache implementations.

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use core::fmt;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use parking_lot::MutexGuard;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub use alloc::sync::{Arc, Weak};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod atomic {
    pub use core::sync::atomic::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub struct Mutex<T>(parking_lot::Mutex<T>);

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T> Mutex<T> {
    pub fn new(value: T) -> Self {
        Self(parking_lot::Mutex::new(value))
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, T>, PoisonError<MutexGuard<'_, T>>> {
        Ok(self.0.lock())
    }

    pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, TryLockError<MutexGuard<'_, T>>> {
        self.0.try_lock().ok_or(TryLockError::WouldBlock)
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub struct PoisonError<T>(pub T);

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub enum TryLockError<T> {
    Poisoned(PoisonError<T>),
    WouldBlock,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T> fmt::Debug for TryLockError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Poisoned(_) => f.write_str("Poisoned"),
            Self::WouldBlock => f.write_str("WouldBlock"),
        }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::string::String;

mod base_cache;
mod builder;
mod cache;
mod entry_selector;
mod invalidator;
mod key_lock;
mod segment;
mod value_initializer;

/// The type of the unique ID to identify a predicate used by
/// [`Cache::invalidate_entries_if`][invalidate-if] method.
///
/// A `PredicateId` is a `String` of UUID (version 4).
///
/// [invalidate-if]: ./struct.Cache.html#method.invalidate_entries_if
pub type PredicateId = String;

pub(crate) type PredicateIdStr<'a> = &'a str;

pub use crate::common::iter::Iter;
pub use {
    builder::CacheBuilder,
    cache::Cache,
    entry_selector::{OwnedKeyEntrySelector, RefKeyEntrySelector},
    segment::SegmentedCache,
};

/// Provides extra methods that will be useful for testing.
pub trait ConcurrentCacheExt<K, V> {
    /// Performs any pending maintenance operations needed by the cache.
    fn sync(&self);
}

// Empty struct to be used in `InitResult::InitErr` to represent the Option None.
pub(crate) struct OptionallyNone;

// Empty struct to be used in `InitResult::InitErr`` to represent the Compute None.
pub(crate) struct ComputeNone;
