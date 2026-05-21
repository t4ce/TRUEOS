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
pub mod error {
    //! no_std stand-in for `std::error`.
    pub use core::error::*;
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
pub mod pin {
    //! no_std stand-in for `std::pin`.
    pub use core::pin::*;
}

#[cfg(not(feature = "std"))]
pub mod sync {
    //! no_std stand-in for the allocation-backed sync types used here.
    pub use alloc::sync::{Arc, Weak};
}

#[cfg(not(feature = "std"))]
pub mod task {
    //! no_std stand-in for `std::task`.
    pub use core::task::*;
}

#[cfg(feature = "client")]
pub mod client;
mod common;
pub mod rt;
#[cfg(feature = "server")]
pub mod server;
#[cfg(any(feature = "service", feature = "client-legacy"))]
pub mod service;
