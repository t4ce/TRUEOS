//! Minimal `std` ABI shim for the Tokio probe when the custom target is
//! advertised as `target_os = "trueos"`.
//!
//! The intent is not to emulate a host userspace environment. We only provide the
//! narrow symbol surface that Rust `std` expects so it can allocate and do
//! basic stdio through TRUEOS facilities while we probe Tokio's `rt` feature.

extern crate alloc;

use alloc::collections::BTreeMap;
use core::alloc::Layout;
use core::ffi::{c_char, c_int, c_void};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};
use spin::Mutex;

static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);

const TRUEOS_EAGAIN: c_int = 11;
const TRUEOS_EBUSY: c_int = 16;
const TRUEOS_EINVAL: c_int = 22;
const TRUEOS_ENOSYS: c_int = 38;
const TRUEOS_ETIMEDOUT: c_int = 110;
const TRUEOS_SC_PAGESIZE: c_int = 30;
const TRUEOS_SC_PAGE_SIZE: c_int = TRUEOS_SC_PAGESIZE;
const TRUEOS_SC_NPROCESSORS_ONLN: c_int = 84;
const TRUEOS_SC_NPROCESSORS_CONF: c_int = 83;

#[repr(C)]
struct Iovec {
    base: *const u8,
    len: usize,
}

#[derive(Clone, Copy)]
struct PthreadMutexState {
    locked: bool,
    owner: usize,
}

#[derive(Clone, Copy)]
struct PthreadCondState {
    generation: u64,
}

static PTHREAD_MUTEXES: Mutex<BTreeMap<usize, PthreadMutexState>> = Mutex::new(BTreeMap::new());
static PTHREAD_CONDS: Mutex<BTreeMap<usize, PthreadCondState>> = Mutex::new(BTreeMap::new());
static LOGGED_PTHREAD_SYNC: AtomicI32 = AtomicI32::new(0);

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

fn pthread_key(ptr: *mut c_void) -> Option<usize> {
    let key = ptr as usize;
    if key == 0 { None } else { Some(key) }
}

fn pthread_current_id() -> usize {
    crate::percpu::current_slot().saturating_add(1)
}

fn pthread_sync_probe_log() {
    if LOGGED_PTHREAD_SYNC
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!("std-abi: pthread mutex/cond shim using TRUEOS spin wait states\n");
    }
}

fn pthread_mutex_unlock_key(key: usize) -> c_int {
    let owner = pthread_current_id();
    let mut table = PTHREAD_MUTEXES.lock();
    let state = table.entry(key).or_insert(PthreadMutexState {
        locked: false,
        owner: 0,
    });
    if state.locked && state.owner != owner {
        return TRUEOS_EINVAL;
    }
    state.locked = false;
    state.owner = 0;
    0
}

fn pthread_mutex_lock_key(key: usize) -> c_int {
    pthread_sync_probe_log();
    let owner = pthread_current_id();
    loop {
        {
            let mut table = PTHREAD_MUTEXES.lock();
            let state = table.entry(key).or_insert(PthreadMutexState {
                locked: false,
                owner: 0,
            });
            if !state.locked {
                state.locked = true;
                state.owner = owner;
                return 0;
            }
        }
        core::hint::spin_loop();
    }
}

fn pthread_mutex_trylock_key(key: usize) -> c_int {
    pthread_sync_probe_log();
    let owner = pthread_current_id();
    let mut table = PTHREAD_MUTEXES.lock();
    let state = table.entry(key).or_insert(PthreadMutexState {
        locked: false,
        owner: 0,
    });
    if state.locked {
        TRUEOS_EBUSY
    } else {
        state.locked = true;
        state.owner = owner;
        0
    }
}

fn pthread_cond_generation(key: usize) -> u64 {
    let mut table = PTHREAD_CONDS.lock();
    table
        .entry(key)
        .or_insert(PthreadCondState { generation: 0 })
        .generation
}

fn pthread_cond_notify_key(key: usize) -> c_int {
    let mut table = PTHREAD_CONDS.lock();
    let state = table
        .entry(key)
        .or_insert(PthreadCondState { generation: 0 });
    state.generation = state.generation.wrapping_add(1);
    0
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
    crate::globalog::append_raw(bytes);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_internal_log_write(bytes: *const u8, len: usize) {
    if bytes.is_null() || len == 0 {
        return;
    }

    let bytes = unsafe { slice::from_raw_parts(bytes, len) };
    uart_write(bytes);
    crate::globalog::append_raw(bytes);
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
pub extern "C" fn trueos_time_monotonic_nanos() -> u64 {
    crate::chronos::monotonic_nanos()
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_time_unix_seconds() -> u64 {
    crate::chronos::best_effort_unix_time_seconds().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_time_unix_nanos() -> u64 {
    crate::chronos::best_effort_unix_time_seconds()
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_panic(msg_ptr: *const u8, len: usize) -> ! {
    if !msg_ptr.is_null() && len != 0 {
        let bytes = unsafe { slice::from_raw_parts(msg_ptr, len) };
        uart_write(b"std-trueos panic: ");
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn errno_location() -> *mut c_int {
    (&TRUEOS_ERRNO as *const AtomicI32)
        .cast_mut()
        .cast::<c_int>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __errno_location() -> *mut c_int {
    unsafe { errno_location() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strerror_r(errnum: c_int, buf: *mut c_char, buflen: usize) -> c_int {
    if buf.is_null() || buflen == 0 {
        return 0;
    }

    let prefix = b"trueos errno ";
    let mut pos = 0usize;
    let out = unsafe { slice::from_raw_parts_mut(buf.cast::<u8>(), buflen) };
    for byte in prefix {
        if pos + 1 >= out.len() {
            break;
        }
        out[pos] = *byte;
        pos += 1;
    }

    let mut digits = [0u8; 12];
    let mut n = if errnum < 0 {
        if pos + 1 < out.len() {
            out[pos] = b'-';
            pos += 1;
        }
        errnum.saturating_neg() as u32
    } else {
        errnum as u32
    };
    let mut len = 0usize;
    loop {
        digits[len] = b'0' + (n % 10) as u8;
        len += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    while len != 0 && pos + 1 < out.len() {
        len -= 1;
        out[pos] = digits[len];
        pos += 1;
    }
    out[pos.min(out.len() - 1)] = 0;
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_memalign(
    memptr: *mut *mut c_void,
    align: usize,
    size: usize,
) -> c_int {
    if memptr.is_null() {
        return 22;
    }

    let ptr = unsafe { alloc_bytes(size, align) }.cast::<c_void>();
    if ptr.is_null() && size != 0 {
        return 12;
    }

    unsafe { *memptr = ptr };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getenv(_name: *const c_char) -> *mut c_char {
    ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getcwd(buf: *mut c_char, size: usize) -> *mut c_char {
    if buf.is_null() || size < 2 {
        TRUEOS_ERRNO.store(34, core::sync::atomic::Ordering::Relaxed);
        return ptr::null_mut();
    }
    let out = unsafe { slice::from_raw_parts_mut(buf.cast::<u8>(), size) };
    out[0] = b'/';
    out[1] = 0;
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn write(fd: c_int, buf: *const c_void, count: usize) -> isize {
    if buf.is_null() {
        return -1;
    }
    unsafe { sys_write(fd as u32, buf.cast::<u8>(), count) };
    count as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn writev(fd: c_int, iov: *const Iovec, iovcnt: c_int) -> isize {
    if iov.is_null() || iovcnt < 0 {
        return -1;
    }
    let entries = unsafe { slice::from_raw_parts(iov, iovcnt as usize) };
    let mut written = 0usize;
    for entry in entries {
        if !entry.base.is_null() && entry.len != 0 {
            unsafe { sys_write(fd as u32, entry.base, entry.len) };
            written = written.saturating_add(entry.len);
        }
    }
    written as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn acos(x: f64) -> f64 {
    libm::acos(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn asin(x: f64) -> f64 {
    libm::asin(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn atan(x: f64) -> f64 {
    libm::atan(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

#[unsafe(no_mangle)]
pub extern "C" fn cbrt(x: f64) -> f64 {
    libm::cbrt(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn ceil(x: f64) -> f64 {
    libm::ceil(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn cos(x: f64) -> f64 {
    libm::cos(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn cosh(x: f64) -> f64 {
    libm::cosh(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn exp(x: f64) -> f64 {
    libm::exp(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn expm1(x: f64) -> f64 {
    libm::expm1(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn fabs(x: f64) -> f64 {
    libm::fabs(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn floor(x: f64) -> f64 {
    libm::floor(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
    libm::fmod(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn log(x: f64) -> f64 {
    libm::log(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log1p(x: f64) -> f64 {
    libm::log1p(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log2(x: f64) -> f64 {
    libm::log2(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log10(x: f64) -> f64 {
    libm::log10(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn round(x: f64) -> f64 {
    libm::round(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sin(x: f64) -> f64 {
    libm::sin(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sinh(x: f64) -> f64 {
    libm::sinh(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn tan(x: f64) -> f64 {
    libm::tan(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn trunc(x: f64) -> f64 {
    libm::trunc(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sched_yield() -> c_int {
    core::hint::spin_loop();
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutexattr_init(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutexattr_settype(_attr: *mut c_void, _kind: c_int) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutexattr_destroy(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_init(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_setstacksize(
    _attr: *mut c_void,
    _stack_size: usize,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_setguardsize(
    _attr: *mut c_void,
    _guard_size: usize,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_destroy(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_create(
    _thread: *mut usize,
    _attr: *const c_void,
    _start: *mut c_void,
    _arg: *mut c_void,
) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, core::sync::atomic::Ordering::Relaxed);
    TRUEOS_EAGAIN
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_self() -> usize {
    crate::percpu::current_slot().saturating_add(1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setname_np(_thread: usize, _name: *const c_char) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysconf(name: c_int) -> isize {
    match name {
        TRUEOS_SC_PAGESIZE | TRUEOS_SC_PAGE_SIZE => 4096,
        TRUEOS_SC_NPROCESSORS_ONLN | TRUEOS_SC_NPROCESSORS_CONF => {
            crate::workers::background_worker_slots().len().max(1) as isize
        }
        _ => {
            TRUEOS_ERRNO.store(TRUEOS_ENOSYS, core::sync::atomic::Ordering::Relaxed);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_init(mutex: *mut c_void, _attr: *const c_void) -> c_int {
    let Some(key) = pthread_key(mutex) else {
        return TRUEOS_EINVAL;
    };
    PTHREAD_MUTEXES.lock().insert(
        key,
        PthreadMutexState {
            locked: false,
            owner: 0,
        },
    );
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_destroy(mutex: *mut c_void) -> c_int {
    if let Some(key) = pthread_key(mutex) {
        PTHREAD_MUTEXES.lock().remove(&key);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_lock(mutex: *mut c_void) -> c_int {
    let Some(key) = pthread_key(mutex) else {
        return TRUEOS_EINVAL;
    };
    pthread_mutex_lock_key(key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_trylock(mutex: *mut c_void) -> c_int {
    let Some(key) = pthread_key(mutex) else {
        return TRUEOS_EINVAL;
    };
    pthread_mutex_trylock_key(key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_unlock(mutex: *mut c_void) -> c_int {
    let Some(key) = pthread_key(mutex) else {
        return TRUEOS_EINVAL;
    };
    pthread_mutex_unlock_key(key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_cond_init(cond: *mut c_void, _attr: *const c_void) -> c_int {
    let Some(key) = pthread_key(cond) else {
        return TRUEOS_EINVAL;
    };
    PTHREAD_CONDS
        .lock()
        .insert(key, PthreadCondState { generation: 0 });
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_condattr_init(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_condattr_setclock(_attr: *mut c_void, _clock: c_int) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_condattr_destroy(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_cond_destroy(cond: *mut c_void) -> c_int {
    if let Some(key) = pthread_key(cond) {
        PTHREAD_CONDS.lock().remove(&key);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_cond_wait(cond: *mut c_void, mutex: *mut c_void) -> c_int {
    let Some(cond_key) = pthread_key(cond) else {
        return TRUEOS_EINVAL;
    };
    let Some(mutex_key) = pthread_key(mutex) else {
        return TRUEOS_EINVAL;
    };

    let generation = pthread_cond_generation(cond_key);
    let unlock_rc = pthread_mutex_unlock_key(mutex_key);
    if unlock_rc != 0 {
        return unlock_rc;
    }

    while pthread_cond_generation(cond_key) == generation {
        core::hint::spin_loop();
    }

    pthread_mutex_lock_key(mutex_key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_cond_timedwait(
    cond: *mut c_void,
    mutex: *mut c_void,
    _abstime: *const c_void,
) -> c_int {
    let Some(cond_key) = pthread_key(cond) else {
        return TRUEOS_EINVAL;
    };
    let Some(mutex_key) = pthread_key(mutex) else {
        return TRUEOS_EINVAL;
    };

    let generation = pthread_cond_generation(cond_key);
    let unlock_rc = pthread_mutex_unlock_key(mutex_key);
    if unlock_rc != 0 {
        return unlock_rc;
    }

    for _ in 0..4096 {
        if pthread_cond_generation(cond_key) != generation {
            return pthread_mutex_lock_key(mutex_key);
        }
        core::hint::spin_loop();
    }

    let _ = pthread_mutex_lock_key(mutex_key);
    TRUEOS_ETIMEDOUT
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_cond_signal(cond: *mut c_void) -> c_int {
    let Some(key) = pthread_key(cond) else {
        return TRUEOS_EINVAL;
    };
    pthread_cond_notify_key(key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_cond_broadcast(cond: *mut c_void) -> c_int {
    let Some(key) = pthread_key(cond) else {
        return TRUEOS_EINVAL;
    };
    pthread_cond_notify_key(key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_join(_thread: usize, _retval: *mut *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_detach(_thread: usize) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _Unwind_GetIP(_ctx: *mut c_void) -> usize {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _Unwind_Backtrace(
    _trace: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    _arg: *mut c_void,
) -> c_int {
    0
}
