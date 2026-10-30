//! Adapter for the TRUEOS `sys_rand` ABI, which is backed by Tyche.
//!
//! `sys_rand` has no error result, so this adapter cannot report entropy
//! failures. Kernel builds select getrandom's fallible custom backend instead.
use crate::Error;
use core::mem::MaybeUninit;

pub use crate::util::{inner_u32, inner_u64};

#[path = "trueos_bytes.rs"]
mod bytes;

pub fn fill_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    bytes::fill(crate::util::uninit_slice_fill_zero(dest));
    Ok(())
}
