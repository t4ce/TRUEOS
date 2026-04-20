//! Minimal `std` ABI shim for the Tokio probe when the custom target is
//! advertised as `target_os = "zkvm"`.
//!
//! The intent is not to emulate the real zkVM environment. We only provide the
//! narrow symbol surface that Rust `std` expects so it can allocate and do
//! basic stdio through TRUEOS facilities while we probe Tokio's `rt` feature.

use core::alloc::Layout;
use core::ptr;
use core::slice;

fn uart_write(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    crate::shell2::uart1_com1::write_bytes(bytes);
}

#[inline]
unsafe fn alloc_bytes(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }

    let Ok(layout) = Layout::from_size_align(size, align.max(1)) else {
        return ptr::null_mut();
    };

    unsafe { crate::allocators::alloc_raw(layout) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_alloc_words(nwords: usize) -> *mut u32 {
    let bytes = nwords.saturating_mul(core::mem::size_of::<u32>());
    unsafe { alloc_bytes(bytes, core::mem::align_of::<u32>()) as *mut u32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_alloc_aligned(size: usize, align: usize) -> *mut u8 {
    unsafe { alloc_bytes(size, align) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_rand(recv_buf: *mut u32, words: usize) {
    if recv_buf.is_null() || words == 0 {
        return;
    }

    let byte_len = words.saturating_mul(core::mem::size_of::<u32>());
    let bytes = unsafe { slice::from_raw_parts_mut(recv_buf.cast::<u8>(), byte_len) };
    if !crate::rng::fill_bytes(bytes) {
        ptr::write_bytes(recv_buf, 0, words);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_write(_fd: u32, write_buf: *const u8, nbytes: usize) {
    if write_buf.is_null() || nbytes == 0 {
        return;
    }
    let bytes = unsafe { slice::from_raw_parts(write_buf, nbytes) };
    uart_write(bytes);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_read(_fd: u32, recv_buf: *mut u8, nrequested: usize) -> usize {
    if recv_buf.is_null() || nrequested == 0 {
        return 0;
    }

    let out = unsafe { slice::from_raw_parts_mut(recv_buf, nrequested) };
    let mut read = 0usize;
    while read < out.len() {
        let Some(byte) = crate::shell2::uart1_com1::read_byte() else {
            break;
        };
        out[read] = byte;
        read += 1;
    }
    read
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getenv(
    _recv_buf: *mut u32,
    _words: usize,
    _varname: *const u8,
    _varname_len: usize,
) -> usize {
    usize::MAX
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_argc() -> usize {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_argv(
    _out_words: *mut u32,
    _out_nwords: usize,
    _arg_index: usize,
) -> usize {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_output(_output_id: u32, _output_value: u32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_sha_compress(
    out_state: *mut [u32; 8],
    in_state: *const [u32; 8],
    _block1_ptr: *const [u32; 8],
    _block2_ptr: *const [u32; 8],
) {
    if out_state.is_null() {
        return;
    }

    if in_state.is_null() {
        unsafe { (*out_state) = [0; 8] };
    } else {
        unsafe { *out_state = *in_state };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_sha_buffer(
    out_state: *mut [u32; 8],
    in_state: *const [u32; 8],
    _buf: *const u8,
    _count: u32,
) {
    unsafe { sys_sha_compress(out_state, in_state, ptr::null(), ptr::null()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_log(msg_ptr: *const u8, len: usize) {
    if msg_ptr.is_null() || len == 0 {
        return;
    }
    let bytes = unsafe { slice::from_raw_parts(msg_ptr, len) };
    uart_write(bytes);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_cycle_count() -> usize {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_panic(msg_ptr: *const u8, len: usize) -> ! {
    if !msg_ptr.is_null() && len != 0 {
        let bytes = unsafe { slice::from_raw_parts(msg_ptr, len) };
        uart_write(b"std-zkvm panic: ");
        uart_write(bytes);
        uart_write(b"\n");
    }
    unsafe { sys_halt() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
