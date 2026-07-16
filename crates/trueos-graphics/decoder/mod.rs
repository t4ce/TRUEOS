//! Image decoder front door for TRUEOS graphics.
//!
//! These modules keep the current decoding behavior intact while giving PNG
//! and JPEG a shared graphics home.

pub(crate) mod jpeg;
pub(crate) mod jpeg_layout;
pub(crate) mod png;
pub(crate) mod png_decode_pool;
