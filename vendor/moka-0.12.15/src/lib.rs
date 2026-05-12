#![warn(clippy::all)]
#![warn(rust_2018_idioms)]
// Temporary disable this lint as the MSRV (1.51) require an older lint name:
// #![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(any(target_os = "trueos", target_os = "zkvm"), no_std)]

//! Moka is a fast, concurrent cache library for Rust. Moka is inspired by the
//! [Caffeine][caffeine-git] library for Java.
//!
//! Moka provides in-memory concurrent cache implementations on top of hash maps.
//! They support full concurrency of retrievals and a high expected concurrency for
//! updates. They utilize a lock-free concurrent hash table as the central key-value
//! storage.
//!
//! All cache implementations perform a best-effort bounding of the map using an
//! entry replacement algorithm to determine which entries to evict when the capacity
//! is exceeded.
//!
//! [caffeine-git]: https://github.com/ben-manes/caffeine
//!
//! **NOTE**:
//! If you have any questions about Moka's APIs or internal design, you can ask the
//! AI chatbot at DeepWiki in a natural language:
//! <https://deepwiki.com/moka-rs/moka>
//!
//! # Features
//!
//! - Thread-safe, highly concurrent in-memory cache implementations:
//!     - Synchronous caches that can be shared across OS threads.
//!     - An asynchronous (futures aware) cache.
//! - A cache can be bounded by one of the followings:
//!     - The maximum number of entries.
//!     - The total weighted size of entries. (Size aware eviction)
//! - Maintains near optimal hit ratio by using an entry replacement algorithms
//!   inspired by Caffeine:
//!     - Admission to a cache is controlled by the Least Frequently Used (LFU)
//!       policy.
//!     - Eviction from a cache is controlled by the Least Recently Used (LRU)
//!       policy.
//!     - [More details and some benchmark results are available here][tiny-lfu].
//! - Supports expiration policies:
//!     - Time to live.
//!     - Time to idle.
//!     - Per-entry variable expiration.
//! - Supports eviction listener, a callback function that will be called when an
//!   entry is removed from the cache.
//!
//! [tiny-lfu]: https://github.com/moka-rs/moka/wiki#admission-and-eviction-policies
//!
//! ## Cache Policies
//!
//! When a cache is full, it has to select and evict existing entries to make some
//! room. A cache policy is a strategy to determine which entry to evict.
//!
//! The choice of the cache policy may have a significant impact on the performance
//! of the cache. Because the time for cache misses is usually much greater than the
//! time for cache hits, the miss rate (number of misses per second) has a
//! significant impact on the performance.
//!
//! Moka provides the following policies:
//!
//! - TinyLFU
//! - LRU
//!
//! ### TinyLFU
//!
//! TinyLFU is the default policy of the cache, and will be suitable for most
//! workloads.
//!
//! TinyLFU is a combination of the LRU eviction policy and the LFU admission policy.
//! LRU stands for Least Recently Used, which is very popular in many cache systems.
//! LFU stands for Least Frequently Used.
//!
//! ![The lifecycle of cached entries with TinyLFU][tiny-lfu-image]
//!
//! [tiny-lfu-image]:
//!     https://github.com/moka-rs/moka/wiki/images/benchmarks/moka-tiny-lfu.png
//!
//! With TinyLFU policy, the cache will admit a new entry based on its popularity. If
//! the key of the entry is popular, it will be admitted to the cache. Otherwise, it
//! will be rejected.
//!
//! The popularity of the key is estimated by the historic popularity estimator
//! called LFU filter. It is a modified Count-Min Sketch, and it can estimate the
//! frequency of keys with a very low memory footprint (thus the name “tiny”). Note
//! that it tracks not only the keys currently in the cache, but all hit and missed
//! keys.
//!
//! Once the entry is admitted to the cache, it will be evicted based on the LRU
//! policy. It evicts the least recently used entry from the cache.
//!
//! TinyLFU will be suitable for most workloads, such as database, search, and
//! analytics.
//!
//! ### LRU
//!
//! LRU stands for Least Recently Used.
//!
//! With LRU policy, the cache will evict the least recently used entry. It is a
//! simple policy and has been used in many cache systems.
//!
//! LRU will be suitable for recency-biased workloads, such as job queues and event
//! streams.
//!
//! # Examples
//!
//! See the following document:
//!
//! - Thread-safe, synchronous caches:
//!     - [`sync::Cache`][sync-cache-struct]
//!     - [`sync::SegmentedCache`][sync-seg-cache-struct]
//! - An asynchronous (futures aware) cache:
//!     - [`future::Cache`][future-cache-struct] (Requires "future" feature)
//!
//! [future-cache-struct]: ./future/struct.Cache.html
//! [sync-cache-struct]: ./sync/struct.Cache.html
//! [sync-seg-cache-struct]: ./sync/struct.SegmentedCache.html
//!
//! **NOTE:** The following caches have been moved to a separate crate called
//! "[mini-moka][mini-moka-crate]".
//!
//! - Non concurrent cache for single threaded applications:
//!     - `moka::unsync::Cache` → [`mini_moka::unsync::Cache`][unsync-cache-struct]
//! - A simple, thread-safe, synchronous cache:
//!     - `moka::dash::Cache` → [`mini_moka::sync::Cache`][dash-cache-struct]
//!
//! [mini-moka-crate]: https://crates.io/crates/mini-moka
//! [unsync-cache-struct]:
//!     https://docs.rs/mini-moka/latest/mini_moka/unsync/struct.Cache.html
//! [dash-cache-struct]:
//!     https://docs.rs/mini-moka/latest/mini_moka/sync/struct.Cache.html
//!
//! # Minimum Supported Rust Versions
//!
//! This crate's minimum supported Rust versions (MSRV) are the followings:
//!
//! | Feature  | MSRV                       |
//! |:---------|:-----------------------------:|
//! | `future` | Rust 1.71.1 (August 3, 2023) |
//! | `sync`   | Rust 1.71.1 (August 3, 2023) |
//!
//! It will keep a rolling MSRV policy of at least 6 months. If the default features
//! with a mandatory features (`future` or `sync`) are enabled, MSRV will be updated
//! conservatively. When using other features, MSRV might be updated more frequently,
//! up to the latest stable.
//!
//! In both cases, increasing MSRV is _not_ considered a semver-breaking change.
//!
//! # Implementation Details
//!
//! ## Concurrency
//!
//! The entry replacement algorithms are kept eventually consistent with the
//! concurrent hash table. While updates to the cache are immediately applied to the
//! hash table, recording of reads and writes may not be immediately reflected on the
//! cache policy's data structures.
//!
//! These cache policy structures are guarded by a lock and operations are applied in
//! batches to avoid lock contention.
//!
//! Recap:
//!
//! - The concurrent hash table in the cache is _strong consistent_:
//!     - It is a lock-free data structure and immediately applies updates.
//!     - It is guaranteed that the inserted entry will become visible immediately to
//!       all threads.
//! - The cache policy's data structures are _eventually consistent_:
//!     - They are guarded by a lock and operations are applied in batches.
//!     - An example of eventual consistency: the `entry_count` method may return an
//!       outdated value.
//!
//! ### Bounded Channels
//!
//! In order to hold the recordings of reads and writes until they are applied to the
//! cache policy's data structures, the cache uses two bounded channels, one for
//! reads and the other for writes. Bounded means that a channel have a maximum
//! number of elements that can be stored.
//!
//! These channels are drained when one of the following conditions is met:
//!
//! - The numbers of read or write recordings reach to the configured amounts.
//!     - It is currently hard-coded to 64.
//! - Or, the certain time past from the last draining.
//!     - It is currently hard-coded to 300 milliseconds.
//!
//! Cache does not have a dedicated thread for draining. Instead, it is done by a
//! user thread. When user code calls certain cache methods, such as `get`,
//! `get_with`, `insert`, and `run_pending_tasks`, the cache checks if the above
//! condition is met, and if so, it will start draining as a part of the method call
//! and apply the recordings to the cache policy's data structures. See [the
//! Maintenance Tasks section](#maintenance-tasks) for more details of applying the
//! recordings.
//!
//! ### When a Bounded Channels is Full
//!
//! Under heavy concurrent operations from clients, draining may not be able to catch
//! up and the bounded channels can become full. In this case, the cache will do one
//! of the followings:
//!
//! - For the read channel, recordings of new reads will be discarded, so that
//!   retrievals will never be blocked. This behavior may have some impact to the hit
//!   rate of the cache.
//! - For the write channel, updates from clients to the cache will be blocked until
//!   the draining task catches up.
//!
//! ## Maintenance Tasks
//!
//! When draining the read and write recordings from the channels, the cache will do
//! the following maintenance tasks:
//!
//! 1. Determine whether to admit an entry to the cache or not, based on its
//!    popularity.
//!    - If not, the entry is removed from the internal concurrent hash table.
//! 2. Apply the recording of cache reads and writes to the internal data structures
//!    for the cache policies, such as the LFU filter, LRU queues, and hierarchical
//!    timer wheels.
//!    - The hierarchical timer wheels are used for the per-entry expiration policy.
//! 3. When cache's max capacity is exceeded, remove least recently used (LRU)
//!    entries.
//! 4. Remove expired entries.
//! 5. Find and remove the entries that have been invalidated by the `invalidate_all`
//!    or `invalidate_entries_if` methods.
//! 6. Deliver removal notifications to the eviction listener. (Call the eviction
//!    listener closure with the information about the evicted entry)
//!
//! The following cache method calls may trigger the maintenance tasks:
//!
//! - All cache write methods: `insert`, `get_with`, `invalidate`, etc., except for
//!   `invalidate_all` and `invalidate_entries_if`.
//! - Some of the cache read methods: `get`
//! - `run_pending_tasks` method, which executes the pending maintenance tasks
//!   explicitly.
//!
//! Except `run_pending_tasks` method, the maintenance tasks are executed lazily
//! when one of the conditions in the [Bounded Channels](#bounded-channels) section
//! is met.

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate alloc;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate self as std;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod any {
    pub use core::any::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod borrow {
    pub use alloc::borrow::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod boxed {
    pub use alloc::boxed::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod cell {
    pub use core::cell::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod cmp {
    pub use core::cmp::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod collections {
    pub mod hash_map {
        use core::hash::{BuildHasher, Hasher};

        #[derive(Clone, Copy, Debug, Default)]
        pub struct RandomState;

        #[derive(Clone, Copy, Debug, Default)]
        pub struct DefaultHasher(u64);

        impl BuildHasher for RandomState {
            type Hasher = DefaultHasher;

            fn build_hasher(&self) -> Self::Hasher {
                DefaultHasher(0xcbf2_9ce4_8422_2325)
            }
        }

        impl Hasher for DefaultHasher {
            fn finish(&self) -> u64 {
                self.0
            }

            fn write(&mut self, bytes: &[u8]) {
                for byte in bytes {
                    self.0 ^= u64::from(*byte);
                    self.0 = self.0.wrapping_mul(0x100_0000_01b3);
                }
            }
        }
    }

    pub use alloc::collections::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod convert {
    pub use core::convert::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod default {
    pub use core::default::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod error {
    pub use core::error::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod fmt {
    pub use core::fmt::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod future {
    pub use core::future::*;
    pub use core::future::{ready, Ready};
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod hash {
    pub use core::hash::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod iter {
    pub use core::iter::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod marker {
    pub use core::marker::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod mem {
    pub use core::mem::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod panic {
    pub use core::panic::{AssertUnwindSafe, Location, RefUnwindSafe, UnwindSafe};

    pub fn catch_unwind<F: FnOnce() -> R, R>(f: F) -> Result<R, alloc::boxed::Box<dyn core::any::Any + Send>> {
        Ok(f())
    }

    pub fn resume_unwind(_: alloc::boxed::Box<dyn core::any::Any + Send>) -> ! {
        panic!("panic resume is not available on TRUEOS")
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod prelude {
    pub mod rust_2021 {
        pub use alloc::{
            borrow::ToOwned,
            boxed::Box,
            format,
            string::{String, ToString},
            vec,
            vec::Vec,
        };
        pub use core::prelude::rust_2021::*;
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod ptr {
    pub use core::ptr::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod result {
    pub use core::result::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod pin {
    pub use core::pin::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod process {
    pub fn abort() -> ! {
        panic!("abort")
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod rc {
    pub use alloc::rc::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod string {
    pub use alloc::string::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod thread {
    pub fn sleep(_: core::time::Duration) {}
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod time {
    pub use core::time::Duration;

    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
    pub struct Instant(parking_lot::time::Instant);

    impl Instant {
        pub fn now() -> Self {
            Self(parking_lot::time::Instant::now())
        }

        pub fn elapsed(&self) -> Duration {
            self.0.elapsed().into()
        }

        pub fn saturating_duration_since(&self, earlier: Self) -> Duration {
            self.0.saturating_duration_since(earlier.0).into()
        }
    }

    impl core::ops::Add<Duration> for Instant {
        type Output = Instant;

        fn add(self, duration: Duration) -> Self::Output {
            let nanos = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
            Self(parking_lot::time::Instant::from_nanos(
                self.0.as_nanos().saturating_add(nanos),
            ))
        }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod vec {
    pub use alloc::vec::*;
}

#[cfg(not(any(feature = "sync", feature = "future")))]
compile_error!(
    "At least one of the crate features `sync` or `future` must be enabled for \
    `moka` crate. Please update your dependencies in Cargo.toml"
);

// Reexport(s)
pub use equivalent::Equivalent;

#[cfg(feature = "future")]
#[cfg_attr(docsrs, doc(cfg(feature = "future")))]
pub mod future;

#[cfg(feature = "sync")]
#[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
pub mod sync;

#[cfg(any(feature = "sync", feature = "future"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "sync", feature = "future"))))]
pub mod notification;

#[cfg(any(feature = "sync", feature = "future"))]
pub(crate) mod cht;

#[cfg(any(feature = "sync", feature = "future"))]
pub(crate) mod common;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub(crate) mod platform;

#[cfg(any(feature = "sync", feature = "future"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "sync", feature = "future"))))]
pub mod ops;

#[cfg(any(feature = "sync", feature = "future"))]
pub mod policy;

#[cfg(any(feature = "sync", feature = "future"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "sync", feature = "future"))))]
pub use common::error::PredicateError;

#[cfg(any(feature = "sync", feature = "future"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "sync", feature = "future"))))]
pub use common::entry::Entry;

#[cfg(any(feature = "sync", feature = "future"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "sync", feature = "future"))))]
pub use policy::{Expiry, Policy};

#[cfg(feature = "unstable-debug-counters")]
#[cfg_attr(docsrs, doc(cfg(feature = "unstable-debug-counters")))]
pub use common::concurrent::debug_counters::GlobalDebugCounters;

#[cfg(test)]
mod tests {
    #[cfg(trybuild)]
    #[test]
    fn trybuild_default() {
        let t = trybuild::TestCases::new();
        t.compile_fail("tests/compile_tests/default/clone/*.rs");
    }

    #[cfg(all(trybuild, feature = "future"))]
    #[test]
    fn trybuild_future() {
        let t = trybuild::TestCases::new();
        t.compile_fail("tests/compile_tests/future/clone/*.rs");
    }
}

#[cfg(all(doctest, feature = "sync"))]
mod doctests {
    // https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html#include-items-only-when-collecting-doctests
    #[doc = include_str!("../README.md")]
    struct ReadMeDoctests;
}
