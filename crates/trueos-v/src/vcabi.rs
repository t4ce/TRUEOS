//! Compatibility name for the original broad TRUEOS C ABI surface.
//!
//! New code should use `bp_abi` for blueprint/kernel service imports, or
//! `qjs_abi` for the QuickJS-facing overlay.

pub use crate::bp_abi::*;
