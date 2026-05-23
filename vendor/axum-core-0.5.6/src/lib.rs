//! Core types and traits for [`axum`].
//!
//! Libraries authors that want to provide [`FromRequest`] or [`IntoResponse`] implementations
//! should depend on the [`axum-core`] crate, instead of `axum` if possible.
//!
//! [`FromRequest`]: crate::extract::FromRequest
//! [`IntoResponse`]: crate::response::IntoResponse
//! [`axum`]: https://crates.io/crates/axum
//! [`axum-core`]: http://crates.io/crates/axum-core

#![cfg_attr(test, allow(clippy::float_cmp))]
#![cfg_attr(not(test), warn(clippy::print_stdout, clippy::dbg_macro))]
#![cfg_attr(any(target_os = "trueos", target_os = "zkvm"), no_std)]

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate alloc;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate self as std;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod borrow {
    pub use alloc::borrow::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod boxed {
    pub use alloc::boxed::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod convert {
    pub use core::convert::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod fmt {
    pub use core::fmt::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod future {
    pub use core::future::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod marker {
    pub use core::marker::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod pin {
    pub use core::pin::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod result {
    pub use core::result::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod string {
    pub use alloc::string::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod task {
    pub use core::task::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_docs)]
pub mod vec {
    pub use alloc::vec::*;
}

#[macro_use]
pub(crate) mod macros;
#[doc(hidden)] // macro helpers
pub mod __private {
    #[cfg(feature = "tracing")]
    pub use tracing;
}

mod error;
mod ext_traits;
pub use self::error::Error;

pub mod body;
pub mod extract;
pub mod response;

/// Alias for a type-erased error type.
pub type BoxError = alloc::boxed::Box<dyn core::error::Error + core::marker::Send + core::marker::Sync>;

pub use self::ext_traits::{request::RequestExt, request_parts::RequestPartsExt};

