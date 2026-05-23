//! Lightweight, flexible WebSockets for Rust.
#![deny(
#![allow(missing_docs)]
    missing_docs,
    missing_copy_implementations,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unstable_features,
    unused_must_use,
    unused_mut,
    unused_imports,
    unused_import_braces
)]
#![cfg_attr(any(target_os = "trueos", target_os = "zkvm"), no_std)]
// This can be removed when `error::Error::Http`, `handshake::HandshakeError::Interrupted` and
// `handshake::server::ErrorResponse` are boxed.
#![allow(clippy::result_large_err)]

extern crate alloc;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate self as std;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod prelude {
    //! TRUEOS no_std compatibility prelude for upstream std-shaped source.
    pub mod rust_2021 {
        //! TRUEOS no_std compatibility prelude for upstream std-shaped source.
        pub use alloc::{
            boxed::Box,
            format,
            string::{String, ToString},
            vec::Vec,
        };
        pub use core::prelude::rust_2021::*;
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod any {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::any::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod borrow {
    //! TRUEOS no_std compatibility re-exports.
    pub use alloc::borrow::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod boxed {
    //! TRUEOS no_std compatibility re-exports.
    pub use alloc::boxed::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod cmp {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::cmp::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod convert {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::convert::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod default {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::default::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod fmt {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::fmt::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod hash {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::hash::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod io {
    //! TRUEOS no_std IO compatibility.
    pub use trueos_io::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod iter {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::iter::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod marker {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::marker::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod mem {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::mem::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod net {
    //! TRUEOS no_std network compatibility placeholders.
    use crate::io;

    /// Placeholder TCP stream type for APIs that are unavailable on TRUEOS kernel builds.
    #[derive(Clone, Copy, Debug)]
    pub struct TcpStream;

    impl TcpStream {
        /// TCP_NODELAY is a no-op for the placeholder stream.
        pub fn set_nodelay(&self, _nodelay: bool) -> io::Result<()> {
            Ok(())
        }
    }

    impl io::Read for TcpStream {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::Other.into())
        }
    }

    impl io::Write for TcpStream {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::Other.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod ops {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::ops::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod option {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::option::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod result {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::result::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod str {
    //! TRUEOS no_std compatibility re-exports.
    pub use core::str::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod string {
    //! TRUEOS no_std compatibility re-exports.
    pub use alloc::format;
    pub use alloc::string::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod sync {
    //! TRUEOS no_std compatibility re-exports.
    pub use alloc::sync::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod vec {
    //! TRUEOS no_std compatibility re-exports.
    pub use alloc::vec;
    pub use alloc::vec::*;
}

#[cfg(feature = "handshake")]
pub use http;

pub mod buffer;
#[cfg(feature = "handshake")]
pub mod client;
pub mod error;
#[cfg(feature = "handshake")]
pub mod handshake;
pub mod protocol;
#[cfg(feature = "handshake")]
mod server;
pub mod stream;
#[cfg(all(any(feature = "native-tls", feature = "__rustls-tls"), feature = "handshake"))]
mod tls;
mod utf8;
pub mod util;

const READ_BUFFER_CHUNK_SIZE: usize = 4096;
type ReadBuffer = buffer::ReadBuffer<READ_BUFFER_CHUNK_SIZE>;

pub use crate::{
    error::{Error, Result},
    protocol::{frame::Utf8Bytes, Message, WebSocket},
};
// re-export bytes since used in `Message` API.
pub use bytes::Bytes;

#[cfg(feature = "handshake")]
pub use crate::{
    client::{client, connect, ClientRequestBuilder},
    handshake::{client::ClientHandshake, server::ServerHandshake, HandshakeError},
    server::{accept, accept_hdr, accept_hdr_with_config, accept_with_config},
};

#[cfg(all(any(feature = "native-tls", feature = "__rustls-tls"), feature = "handshake"))]
pub use tls::{client_tls, client_tls_with_config, Connector};
