//! Decoder-only no_std WebP support adapted for TRUEOS.

#![no_std]
#![cfg_attr(all(test, feature = "_benchmarks"), feature(test))]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
extern crate alloc;
#[cfg(test)]
extern crate std;

#[allow(missing_docs)]
mod io_ext;
// Enable nightly benchmark functionality if "_benchmarks" feature is enabled.
#[cfg(all(test, feature = "_benchmarks"))]
extern crate test;

pub use self::decoder::{
    DecodingError, LoopCount, UpsamplingMethod, WebPDecodeOptions, WebPDecoder,
};

mod alpha_blending;
mod decoder;
mod extended;
mod huffman;
mod loop_filter;
mod lossless;
mod lossless_transform;
mod transform;
mod vp8_arithmetic_decoder;
mod yuv;

pub mod vp8;
