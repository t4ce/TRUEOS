#![no_std]

extern crate alloc;

#[cfg(feature = "legacy-demo")]
pub mod demo;
pub mod guest;
pub mod runtime;
pub mod stream;
pub mod v;
pub mod vmcall;
pub mod vpanic;
