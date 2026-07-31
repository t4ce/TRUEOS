#![no_std]
#![deny(unsafe_code)]

//! Typed, fail-closed dispatch boundary for the sealed Kokoro AOT stream.
//!
//! Attribute decoding is kept separate from tensor execution: a record that
//! is not part of the model-specific v1 contract is rejected before any
//! kernel can observe tensor memory.

pub mod attributes;
mod cpu;

pub use attributes::{
    ATTRIBUTE_ABI_VERSION, AttributeError, Attributes, ContractReason, decode, record_bytes,
};
pub use cpu::{CpuDispatcher, DispatchError, native_dispatch_supported};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
