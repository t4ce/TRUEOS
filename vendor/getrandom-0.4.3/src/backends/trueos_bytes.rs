// Shared byte adapter for the TrueOS sys_rand word ABI.
// Keep upstream getrandom APIs and errors in their versioned crates.

unsafe extern "C" {
    fn sys_rand(recv_buf: *mut u32, words: usize);
}

pub(super) fn fill(out: &mut [u8]) {
    // Use aligned storage: callers may supply an unaligned byte slice.
    let mut words = [0u32; 64];
    for chunk in out.chunks_mut(words.len() * core::mem::size_of::<u32>()) {
        let word_count = (chunk.len() + 3) / 4;
        words[..word_count].fill(0);
        // SAFETY: words contains word_count writable, aligned u32 values.
        // sys_rand fills whole words synchronously and does not retain the pointer.
        unsafe { sys_rand(words.as_mut_ptr(), word_count) };
        for (bytes, word) in chunk.chunks_mut(4).zip(&words[..word_count]) {
            bytes.copy_from_slice(&word.to_le_bytes()[..bytes.len()]);
        }
    }
    // sys_rand has no error result. Preserve the existing backend contract:
    // failures must be handled by the kernel; this adapter cannot detect them.
}
