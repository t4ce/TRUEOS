//! TRUEOS kernel random source.
use crate::Error;
use core::mem::MaybeUninit;

pub use crate::util::{inner_u32, inner_u64};

unsafe extern "C" {
    fn sys_rand(recv_buf: *mut u32, words: usize);
}

pub fn fill_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    let dest = crate::util::uninit_slice_fill_zero(dest);

    for chunk in dest.chunks_mut(core::mem::size_of::<u32>()) {
        let mut word = 0u32;
        unsafe { sys_rand(&mut word, 1) };
        let bytes = word.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }

    Ok(())
}