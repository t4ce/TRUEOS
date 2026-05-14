use crate::{util::uninit_slice_fill_zero, Error};
use core::mem::MaybeUninit;

unsafe extern "C" {
    fn sys_rand(recv_buf: *mut u32, words: usize);
}

pub fn getrandom_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    let dest = uninit_slice_fill_zero(dest);

    for chunk in dest.chunks_mut(core::mem::size_of::<u32>()) {
        let mut word = 0u32;
        unsafe { sys_rand(&mut word, 1) };
        let bytes = word.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }

    Ok(())
}