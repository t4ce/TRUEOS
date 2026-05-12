#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
#![cfg_attr(test, deny(rust_2018_idioms))]
#![cfg_attr(all(test, feature = "full"), deny(unreachable_pub))]
#![cfg_attr(all(test, feature = "full"), deny(warnings))]
#![cfg_attr(all(test, feature = "nightly"), feature(test))]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(any(target_os = "trueos", target_os = "zkvm"), no_std)]

//! # hyper
//!
//! hyper is a **fast** and **correct** HTTP implementation written in and for Rust.
//!
//! ## Features
//!
//! - HTTP/1 and HTTP/2
//! - Asynchronous design
//! - Leading in performance
//! - Tested and **correct**
//! - Extensive production use
//! - [Client](client/index.html) and [Server](server/index.html) APIs
//!
//! If just starting out, **check out the [Guides](https://hyper.rs/guides/1/)
//! first.**
//!
//! ## "Low-level"
//!
//! hyper is a lower-level HTTP library, meant to be a building block
//! for libraries and applications.
//!
//! If looking for just a convenient HTTP client, consider the
//! [reqwest](https://crates.io/crates/reqwest) crate.
//!
//! # Optional Features
//!
//! hyper uses a set of [feature flags] to reduce the amount of compiled code.
//! It is possible to just enable certain features over others. By default,
//! hyper does not enable any features but allows one to enable a subset for
//! their use case. Below is a list of the available feature flags. You may
//! also notice above each function, struct and trait there is listed one or
//! more feature flags that are required for that item to be used.
//!
//! If you are new to hyper it is possible to enable the `full` feature flag
//! which will enable all public APIs. Beware though that this will pull in
//! many extra dependencies that you may not need.
//!
//! The following optional features are available:
//!
//! - `http1`: Enables HTTP/1 support.
//! - `http2`: Enables HTTP/2 support.
//! - `client`: Enables the HTTP `client`.
//! - `server`: Enables the HTTP `server`.
//!
//! [feature flags]: https://doc.rust-lang.org/cargo/reference/manifest.html#the-features-section
//!
//! ## Unstable Features
//!
//! hyper includes a set of unstable optional features that can be enabled through the use of a
//! feature flag and a [configuration flag].
//!
//! The following is a list of feature flags and their corresponding `RUSTFLAG`:
//!
//! - `ffi`: Enables C API for hyper `hyper_unstable_ffi`.
//! - `tracing`: Enables debug logging with `hyper_unstable_tracing`.
//!
//! For example:
//!
//! ```notrust
//! RUSTFLAGS="--cfg hyper_unstable_tracing" cargo build
//! ```
//!
//! [configuration flag]: https://doc.rust-lang.org/reference/conditional-compilation.html
//!
//! # Stability
//!
//! It's worth talking a bit about the stability of hyper. hyper's API follows
//! [SemVer](https://semver.org). Breaking changes will only be introduced in
//! major versions, if ever. New additions to the API, such as new types,
//! methods, or traits will only be added in minor versions.
//!
//! Some parts of hyper are documented as NOT being part of the stable API. The
//! following is a brief list, you can read more about each one in the relevant
//! part of the documentation.
//!
//! - Downcasting error types from `Error::source()` is not considered stable.
//! - Private dependencies use of global variables is not considered stable.
//!   So, if a dependency uses `log` or `tracing`, hyper doesn't promise it
//!   will continue to do so.
//! - Behavior from default options is not stable. hyper reserves the right to
//!   add new options that are enabled by default which might alter the
//!   behavior, for the purposes of protection. It is also possible to _change_
//!   what the default options are set to, also in efforts to protect the
//!   most people possible.

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate alloc;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate self as std;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod any {
    pub use core::any::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod boxed {
    pub use alloc::boxed::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod cmp {
    pub use core::cmp::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod collections {
    pub use alloc::collections::*;
    pub use hashbrown::{HashMap, HashSet};
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
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod io {
    pub use trueos_io::*;
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
pub mod ops {
    pub use core::ops::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod pin {
    pub use core::pin::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod prelude {
    pub mod rust_2021 {
        pub use alloc::{
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
pub mod process {
    pub fn abort() -> ! {
        loop {
            core::hint::spin_loop();
        }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod rc {
    pub use alloc::rc::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod result {
    pub use core::result::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod string {
    pub use alloc::string::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod sync {
    pub use alloc::sync::{Arc, Weak};
    pub use core::sync::atomic;

    use core::{
        cell::UnsafeCell,
        convert::Infallible,
        ops::{Deref, DerefMut},
        sync::atomic::{AtomicBool, Ordering},
    };

    pub struct Mutex<T: ?Sized> {
        locked: AtomicBool,
        value: UnsafeCell<T>,
    }

    pub struct MutexGuard<'a, T: ?Sized> {
        mutex: &'a Mutex<T>,
    }

    unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
    unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

    impl<T> Mutex<T> {
        pub const fn new(value: T) -> Self {
            Self {
                locked: AtomicBool::new(false),
                value: UnsafeCell::new(value),
            }
        }

        pub fn into_inner(self) -> Result<T, Infallible> {
            Ok(self.value.into_inner())
        }
    }

    impl<T: ?Sized> Mutex<T> {
        pub fn lock(&self) -> Result<MutexGuard<'_, T>, Infallible> {
            while self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                core::hint::spin_loop();
            }
            Ok(MutexGuard { mutex: self })
        }
    }

    impl<T: ?Sized> Drop for MutexGuard<'_, T> {
        fn drop(&mut self) {
            self.mutex.locked.store(false, Ordering::Release);
        }
    }

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
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod task {
    pub use core::task::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod thread {
    pub fn panicking() -> bool {
        false
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod time {
    pub use core::time::Duration;
    pub use tokio::time::Instant;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
    pub struct SystemTime(Duration);

    pub const UNIX_EPOCH: SystemTime = SystemTime(Duration::from_secs(0));

    impl SystemTime {
        pub fn now() -> Self {
            UNIX_EPOCH
        }

        pub fn duration_since(self, earlier: SystemTime) -> Result<Duration, Duration> {
            self.0.checked_sub(earlier.0).ok_or_else(|| earlier.0 - self.0)
        }
    }

    impl core::ops::Add<Duration> for SystemTime {
        type Output = SystemTime;

        fn add(self, rhs: Duration) -> SystemTime {
            SystemTime(self.0 + rhs)
        }
    }

    impl core::ops::Sub<Duration> for SystemTime {
        type Output = SystemTime;

        fn sub(self, rhs: Duration) -> SystemTime {
            SystemTime(self.0 - rhs)
        }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod usize {
    pub use core::usize::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod vec {
    pub use alloc::vec::*;
}

#[doc(hidden)]
pub use http;

#[cfg(all(test, feature = "nightly"))]
extern crate test;

#[doc(no_inline)]
pub use http::{header, HeaderMap, Method, Request, Response, StatusCode, Uri, Version};

pub use crate::hyper_error::{Error, Result};

#[macro_use]
mod cfg;

#[macro_use]
mod trace;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod platform;

pub mod body;
mod common;
#[path = "error.rs"]
mod hyper_error;
pub mod ext;
#[cfg(test)]
mod mock;
pub mod rt;
pub mod service;
pub mod upgrade;

#[cfg(feature = "ffi")]
#[cfg_attr(docsrs, doc(cfg(all(feature = "ffi", hyper_unstable_ffi))))]
pub mod ffi;

cfg_proto! {
    mod headers;
    mod proto;
}

cfg_feature! {
    #![feature = "client"]

    pub mod client;
}

cfg_feature! {
    #![feature = "server"]

    pub mod server;
}
