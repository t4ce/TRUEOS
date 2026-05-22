#![deny(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! Utilities for working with hyper.
//!
//! This crate is less-stable than [`hyper`](https://docs.rs/hyper). However,
//! does respect Rust's semantic version regarding breaking changes.

extern crate alloc;

#[cfg(not(feature = "std"))]
extern crate self as std;

#[cfg(not(feature = "std"))]
pub mod borrow {
    //! no_std stand-in for `std::borrow`.
    pub use alloc::borrow::*;
}

#[cfg(not(feature = "std"))]
pub mod boxed {
    //! no_std stand-in for `std::boxed`.
    pub use alloc::boxed::*;
}

#[cfg(not(feature = "std"))]
pub mod collections {
    //! no_std stand-in for `std::collections`.
    pub use alloc::collections::*;
    pub use hashbrown::{HashMap, HashSet};
}

#[cfg(not(feature = "std"))]
#[allow(missing_docs)]
pub mod env {
    //! no_std stand-in for `std::env`.
    use crate::string::String;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum VarError {
        NotPresent,
        NotUnicode(String),
    }

    pub fn var<K>(_key: K) -> Result<String, VarError> {
        Err(VarError::NotPresent)
    }

    pub fn var_os<K>(_key: K) -> Option<String> {
        None
    }
}

#[cfg(not(feature = "std"))]
pub mod error {
    //! no_std stand-in for `std::error`.
    pub use core::error::*;
}

#[cfg(not(feature = "std"))]
pub mod fmt {
    //! no_std stand-in for `std::fmt`.
    pub use core::fmt::*;
}

#[cfg(not(feature = "std"))]
pub mod future {
    //! no_std stand-in for `std::future`.
    pub use core::future::*;
}

#[cfg(not(feature = "std"))]
pub mod io {
    //! no_std stand-in for the small `std::io` surface used here.
    pub use tokio::io::{Error, ErrorKind, IoSlice, Result};
}

#[cfg(not(feature = "std"))]
pub mod marker {
    //! no_std stand-in for `std::marker`.
    pub use core::marker::*;
}

#[cfg(not(feature = "std"))]
#[allow(missing_docs)]
pub mod net {
    //! no_std stand-in for the `std::net` address vocabulary used here.
    pub use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

    use crate::io;
    use crate::vec;
    use alloc::string::String;

    pub trait ToSocketAddrs {
        type Iter: Iterator<Item = SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter>;
    }

    impl ToSocketAddrs for SocketAddr {
        type Iter = vec::IntoIter<SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
            Ok(alloc::vec![*self].into_iter())
        }
    }

    impl ToSocketAddrs for SocketAddrV4 {
        type Iter = vec::IntoIter<SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
            Ok(alloc::vec![SocketAddr::V4(*self)].into_iter())
        }
    }

    impl ToSocketAddrs for SocketAddrV6 {
        type Iter = vec::IntoIter<SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
            Ok(alloc::vec![SocketAddr::V6(*self)].into_iter())
        }
    }

    impl ToSocketAddrs for (IpAddr, u16) {
        type Iter = vec::IntoIter<SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
            Ok(alloc::vec![SocketAddr::new(self.0, self.1)].into_iter())
        }
    }

    impl ToSocketAddrs for (Ipv4Addr, u16) {
        type Iter = vec::IntoIter<SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
            Ok(alloc::vec![SocketAddr::V4(SocketAddrV4::new(self.0, self.1))].into_iter())
        }
    }

    impl ToSocketAddrs for (Ipv6Addr, u16) {
        type Iter = vec::IntoIter<SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
            Ok(alloc::vec![SocketAddr::V6(SocketAddrV6::new(self.0, self.1, 0, 0))].into_iter())
        }
    }

    impl ToSocketAddrs for (String, u16) {
        type Iter = vec::IntoIter<SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
            parse_host_port(self.0.as_str(), self.1)
        }
    }

    impl ToSocketAddrs for (&str, u16) {
        type Iter = vec::IntoIter<SocketAddr>;

        fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
            parse_host_port(self.0, self.1)
        }
    }

    fn parse_host_port(host: &str, port: u16) -> io::Result<vec::IntoIter<SocketAddr>> {
        if let Ok(addr) = host.parse::<Ipv4Addr>() {
            return Ok(alloc::vec![SocketAddr::V4(SocketAddrV4::new(addr, port))].into_iter());
        }
        if let Ok(addr) = host.parse::<Ipv6Addr>() {
            return Ok(alloc::vec![SocketAddr::V6(SocketAddrV6::new(addr, port, 0, 0))].into_iter());
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "hostname resolution unavailable in hyper-util std shim",
        ))
    }
}

#[cfg(not(feature = "std"))]
pub mod ops {
    //! no_std stand-in for `std::ops`.
    pub use core::ops::*;
}

#[cfg(not(feature = "std"))]
pub mod pin {
    //! no_std stand-in for `std::pin`.
    pub use core::pin::*;
}

#[cfg(not(feature = "std"))]
pub mod prelude {
    //! no_std stand-in for `std::prelude`.
    #[allow(missing_docs)]
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

#[cfg(not(feature = "std"))]
pub mod string {
    //! no_std stand-in for `std::string`.
    pub use alloc::string::*;
}

#[cfg(not(feature = "std"))]
#[allow(missing_docs)]
pub mod sync {
    //! no_std stand-in for the allocation-backed sync types used here.
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

#[cfg(not(feature = "std"))]
pub mod task {
    //! no_std stand-in for `std::task`.
    pub use core::task::*;
}

#[cfg(not(feature = "std"))]
pub mod vec {
    //! no_std stand-in for `std::vec`.
    pub use alloc::vec::*;
}

#[cfg(feature = "client")]
pub mod client;
mod common;
pub mod rt;
#[cfg(feature = "server")]
pub mod server;
#[cfg(any(feature = "service", feature = "client-legacy"))]
pub mod service;
