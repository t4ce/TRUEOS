//! Link ABI surface needed by Rust `std` and hosted async crates on TRUEOS.
//!
//! This intentionally stays separate from the blueprint `sys_*` import ABI.

extern crate alloc;

use alloc::collections::BTreeMap;
use core::alloc::Layout;
use core::ffi::{c_char, c_int, c_void};
use core::ptr;
use core::slice;
use core::sync::atomic::AtomicI32;
use spin::Mutex;

static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);

const TRUEOS_EAGAIN: c_int = 11;
const TRUEOS_EBUSY: c_int = 16;
const TRUEOS_EINVAL: c_int = 22;
const TRUEOS_ERANGE: c_int = 34;
const TRUEOS_ETIMEDOUT: c_int = 110;
const TRUEOS_SC_PAGESIZE: c_int = 30;
const TRUEOS_SC_NPROCESSORS_CONF: c_int = 83;
const TRUEOS_SC_NPROCESSORS_ONLN: c_int = 84;

#[derive(Clone, Copy)]
struct PthreadMutexState {
    locked: bool,
    owner: usize,
}

#[derive(Clone, Copy)]
struct PthreadCondState {
    generation: u64,
}

#[repr(C)]
pub(crate) struct Iovec {
    base: *const u8,
    len: usize,
}

static PTHREAD_MUTEXES: Mutex<BTreeMap<usize, PthreadMutexState>> = Mutex::new(BTreeMap::new());
static PTHREAD_CONDS: Mutex<BTreeMap<usize, PthreadCondState>> = Mutex::new(BTreeMap::new());

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

fn uart_write(bytes: &[u8]) {
    if !bytes.is_empty() {
        crate::shell2::uart1_com1::write_bytes(bytes);
    }
}

fn pthread_key(ptr: *mut c_void) -> Option<usize> {
    let key = ptr as usize;
    if key == 0 { None } else { Some(key) }
}

fn pthread_current_id() -> usize {
    if crate::th::vthread::tokio_blocking_backing_enabled()
        && let Some(vtid) = crate::th::vthread::current_id()
    {
        return 0x1_0000usize.saturating_add(vtid as usize);
    }
    crate::percpu::current_slot().saturating_add(1)
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
pub unsafe extern "C" fn getenv(_name: *const c_char) -> *mut c_char {
    ptr::null_mut()
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
        return TRUEOS_EINVAL;
    }

    let ptr = unsafe { alloc_bytes(size, align) }.cast::<c_void>();
    if ptr.is_null() && size != 0 {
        return 12;
    }

    unsafe { *memptr = ptr };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getcwd(buf: *mut c_char, size: usize) -> *mut c_char {
    if buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_ERANGE, core::sync::atomic::Ordering::Relaxed);
        return ptr::null_mut();
    }

    let cwd = b"/";
    let need = cwd.len().saturating_add(1);
    if size < need {
        TRUEOS_ERRNO.store(TRUEOS_ERANGE, core::sync::atomic::Ordering::Relaxed);
        return ptr::null_mut();
    }

    let out = unsafe { slice::from_raw_parts_mut(buf.cast::<u8>(), size) };
    out[..cwd.len()].copy_from_slice(cwd);
    out[cwd.len()] = 0;
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_rand(recv_buf: *mut u32, words: usize) {
    if recv_buf.is_null() || words == 0 {
        return;
    }

    let byte_len = words.saturating_mul(core::mem::size_of::<u32>());
    let bytes = unsafe { slice::from_raw_parts_mut(recv_buf.cast::<u8>(), byte_len) };
    if !crate::Tyche::fill_bytes(bytes) {
        unsafe { ptr::write_bytes(recv_buf, 0, words) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_panic(msg_ptr: *const u8, len: usize) -> ! {
    if !msg_ptr.is_null() && len != 0 {
        let bytes = unsafe { slice::from_raw_parts(msg_ptr, len) };
        uart_write(b"std panic: ");
        uart_write(bytes);
        uart_write(b"\n");
    }
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn write(_fd: c_int, buf: *const c_void, count: usize) -> isize {
    if buf.is_null() {
        return -1;
    }
    let bytes = unsafe { slice::from_raw_parts(buf.cast::<u8>(), count) };
    uart_write(bytes);
    crate::globalog::append_raw(bytes);
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
            let rc = unsafe { write(fd, entry.base.cast::<c_void>(), entry.len) };
            if rc < 0 {
                return rc;
            }
            written = written.saturating_add(entry.len);
        }
    }
    written as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_atfork(
    _prepare: Option<unsafe extern "C" fn()>,
    _parent: Option<unsafe extern "C" fn()>,
    _child: Option<unsafe extern "C" fn()>,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn powf(x: f32, y: f32) -> f32 {
    libm::powf(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn acos(x: f64) -> f64 {
    libm::acos(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn acosf(x: f32) -> f32 {
    libm::acosf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn asin(x: f64) -> f64 {
    libm::asin(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn asinf(x: f32) -> f32 {
    libm::asinf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn atan(x: f64) -> f64 {
    libm::atan(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn atanf(x: f32) -> f32 {
    libm::atanf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

#[unsafe(no_mangle)]
pub extern "C" fn atan2f(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

#[unsafe(no_mangle)]
pub extern "C" fn cos(x: f64) -> f64 {
    libm::cos(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn cosf(x: f32) -> f32 {
    libm::cosf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn exp(x: f64) -> f64 {
    libm::exp(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn expf(x: f32) -> f32 {
    libm::expf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn expm1(x: f64) -> f64 {
    libm::expm1(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn expm1f(x: f32) -> f32 {
    libm::expm1f(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn hypotf(x: f32, y: f32) -> f32 {
    libm::hypotf(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn log(x: f64) -> f64 {
    libm::log(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn logf(x: f32) -> f32 {
    libm::logf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log2(x: f64) -> f64 {
    libm::log2(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log2f(x: f32) -> f32 {
    libm::log2f(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log10(x: f64) -> f64 {
    libm::log10(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log10f(x: f32) -> f32 {
    libm::log10f(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log1p(x: f64) -> f64 {
    libm::log1p(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log1pf(x: f32) -> f32 {
    libm::log1pf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sin(x: f64) -> f64 {
    libm::sin(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sinf(x: f32) -> f32 {
    libm::sinf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sinh(x: f64) -> f64 {
    libm::sinh(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sinhf(x: f32) -> f32 {
    libm::sinhf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn cosh(x: f64) -> f64 {
    libm::cosh(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn coshf(x: f32) -> f32 {
    libm::coshf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn tan(x: f64) -> f64 {
    libm::tan(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn tanf(x: f32) -> f32 {
    libm::tanf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn tanhf(x: f32) -> f32 {
    libm::tanhf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn sched_yield() -> c_int {
    core::hint::spin_loop();
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysconf(name: c_int) -> isize {
    match name {
        TRUEOS_SC_PAGESIZE => 4096,
        TRUEOS_SC_NPROCESSORS_ONLN | TRUEOS_SC_NPROCESSORS_CONF => {
            crate::workers::background_worker_slots().len().max(1) as isize
        }
        _ => -1,
    }
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
    TRUEOS_EAGAIN
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_self() -> usize {
    pthread_current_id()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setname_np(_thread: usize, _name: *const c_char) -> c_int {
    0
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
