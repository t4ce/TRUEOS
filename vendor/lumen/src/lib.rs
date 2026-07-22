// src/lib.rs
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(feature = "truega", feature = "host-runtime"))]
compile_error!("features `truega` and `host-runtime` are mutually exclusive: TRUEGA is async-only");

#[cfg(not(any(feature = "truega", feature = "host-runtime")))]
compile_error!("enable exactly one Lumen execution contract: `truega` or `host-runtime`");

#[cfg(all(feature = "host-runtime", not(feature = "std")))]
extern crate alloc;

#[cfg(all(feature = "host-runtime", not(feature = "std")))]
#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{}};
}

#[cfg(all(feature = "host-runtime", not(feature = "std")))]
#[macro_export]
macro_rules! thread_local {
    ($(static $name:ident: $ty:ty = $init:expr;)*) => {
        $(
            static $name: $crate::std::thread_local::LocalKey<$ty> =
                $crate::std::thread_local::LocalKey::new($init);
        )*
    };
}

#[cfg(all(feature = "host-runtime", not(feature = "std")))]
pub mod std {
    pub use alloc::{boxed, format, rc, string, vec};
    pub use core::{arch, cell, cmp, fmt, mem, ops, ptr};

    pub mod collections {
        pub use alloc::collections::{BTreeMap as HashMap, BTreeSet as HashSet};
    }

    pub mod env {
        use alloc::string::String;

        #[derive(Debug)]
        pub struct VarError;

        pub fn var(_key: &str) -> Result<String, VarError> {
            Err(VarError)
        }
    }

    pub mod io {
        use alloc::string::String;

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum ErrorKind {
            Other,
            InvalidData,
            Unsupported,
        }

        #[derive(Debug)]
        pub struct Error {
            kind: ErrorKind,
        }

        impl Error {
            pub fn new<E>(_kind: ErrorKind, _error: E) -> Self {
                Self { kind: _kind }
            }

            pub fn kind(&self) -> ErrorKind {
                self.kind
            }
        }

        pub type Result<T> = core::result::Result<T, Error>;

        impl From<&str> for Error {
            fn from(_value: &str) -> Self {
                Self {
                    kind: ErrorKind::Other,
                }
            }
        }

        impl From<String> for Error {
            fn from(_value: String) -> Self {
                Self {
                    kind: ErrorKind::Other,
                }
            }
        }
    }

    pub mod sync {
        pub use core::sync::atomic;

        use core::cell::UnsafeCell;
        use core::hint::spin_loop;
        use core::ops::{Deref, DerefMut};
        use core::sync::atomic::{AtomicBool, Ordering};

        pub struct Mutex<T> {
            locked: AtomicBool,
            value: UnsafeCell<T>,
        }

        unsafe impl<T: Send> Sync for Mutex<T> {}
        unsafe impl<T: Send> Send for Mutex<T> {}

        pub struct MutexGuard<'a, T> {
            mutex: &'a Mutex<T>,
        }

        impl<T> Mutex<T> {
            pub const fn new(value: T) -> Self {
                Self {
                    locked: AtomicBool::new(false),
                    value: UnsafeCell::new(value),
                }
            }

            pub fn lock(&self) -> Result<MutexGuard<'_, T>, ()> {
                while self
                    .locked
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_err()
                {
                    spin_loop();
                }
                Ok(MutexGuard { mutex: self })
            }
        }

        impl<T> Drop for MutexGuard<'_, T> {
            fn drop(&mut self) {
                self.mutex.locked.store(false, Ordering::Release);
            }
        }

        impl<T> Deref for MutexGuard<'_, T> {
            type Target = T;

            fn deref(&self) -> &Self::Target {
                unsafe { &*self.mutex.value.get() }
            }
        }

        impl<T> DerefMut for MutexGuard<'_, T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                unsafe { &mut *self.mutex.value.get() }
            }
        }

        pub struct OnceLock<T> {
            initialized: AtomicBool,
            value: UnsafeCell<Option<T>>,
        }

        unsafe impl<T: Send + Sync> Sync for OnceLock<T> {}
        unsafe impl<T: Send> Send for OnceLock<T> {}

        impl<T> OnceLock<T> {
            pub const fn new() -> Self {
                Self {
                    initialized: AtomicBool::new(false),
                    value: UnsafeCell::new(None),
                }
            }

            pub fn get_or_init(&self, init: impl FnOnce() -> T) -> &T {
                if !self.initialized.load(Ordering::Acquire) {
                    unsafe {
                        *self.value.get() = Some(init());
                    }
                    self.initialized.store(true, Ordering::Release);
                }
                unsafe { (*self.value.get()).as_ref().unwrap_unchecked() }
            }
        }
    }

    pub mod thread_local {
        use core::cell::UnsafeCell;

        pub struct LocalKey<T> {
            value: UnsafeCell<T>,
        }

        unsafe impl<T> Sync for LocalKey<T> {}

        impl<T> LocalKey<T> {
            pub const fn new(value: T) -> Self {
                Self {
                    value: UnsafeCell::new(value),
                }
            }

            pub fn with<F, R>(&'static self, f: F) -> R
            where
                F: FnOnce(&T) -> R,
            {
                f(unsafe { &*self.value.get() })
            }
        }
    }

    pub mod prelude {
        pub mod v1 {
            pub use alloc::boxed::Box;
            pub use alloc::format;
            pub use alloc::string::{String, ToString};
            pub use alloc::vec;
            pub use alloc::vec::Vec;
        }
    }
}

#[cfg(feature = "host-runtime")]
pub mod arch;
pub mod async_module;
#[cfg(feature = "host-runtime")]
pub mod autograd;
pub mod backend;
#[cfg(feature = "host-runtime")]
pub mod parallel;
#[cfg(feature = "host-runtime")]
pub mod precision;
#[cfg(feature = "host-runtime")]
#[macro_use]
pub mod module;
#[cfg(feature = "host-runtime")]
pub mod init;
#[cfg(feature = "host-runtime")]
pub mod layers;
#[cfg(all(feature = "host-runtime", feature = "model-io"))]
pub mod loader;
#[cfg(feature = "host-runtime")]
pub mod loss;
#[cfg(feature = "host-runtime")]
pub mod models;
#[cfg(feature = "host-runtime")]
pub mod ops;
#[cfg(feature = "host-runtime")]
pub mod optim;
#[cfg(all(feature = "host-runtime", feature = "cli"))]
pub mod tokenizer;
