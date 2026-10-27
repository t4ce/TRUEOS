//! TrueOS kernel random source, with the getrandom fill API adapter.
use crate::Error;
use core::mem::MaybeUninit;

pub use crate::util::{inner_u32, inner_u64};

#[path = "trueos_bytes.rs"]
mod trueos;

pub fn fill_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    trueos::fill(crate::util::uninit_slice_fill_zero(dest));
    Ok(())
}
