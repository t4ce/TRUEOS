//! TRUEOS blueprint/kernel random source.
use crate::Error;
use core::mem::MaybeUninit;

unsafe extern "C" {
    fn sys_rand(recv_buf: *mut u32, words: usize);
}

const WORD_CHUNK: usize = 64;

pub fn getrandom_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    let out = crate::util::uninit_slice_fill_zero(dest);
    let mut offset = 0usize;
    let mut words = [0u32; WORD_CHUNK];
    while offset < out.len() {
        let want = core::cmp::min(words.len() * core::mem::size_of::<u32>(), out.len() - offset);
        let word_count = (want + core::mem::size_of::<u32>() - 1) / core::mem::size_of::<u32>();
        words[..word_count].fill(0);
        unsafe { sys_rand(words.as_mut_ptr(), word_count) };

        for word in &words[..word_count] {
            let bytes = word.to_le_bytes();
            let n = core::cmp::min(bytes.len(), out.len() - offset);
            out[offset..offset + n].copy_from_slice(&bytes[..n]);
            offset += n;
        }
    }
    Ok(())
}
