extern crate alloc;

use alloc::collections::BTreeMap;
use core::alloc::Layout;
use core::ffi::{c_char, c_int, c_void};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};
use spin::Mutex;

static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
static C_ALLOCATIONS: Mutex<BTreeMap<usize, AllocationRecord>> = Mutex::new(BTreeMap::new());
static PTHREAD_CONDS: Mutex<BTreeMap<usize, PthreadCondState>> = Mutex::new(BTreeMap::new());
static PTHREAD_MUTEXES: Mutex<BTreeMap<usize, PthreadMutexState>> = Mutex::new(BTreeMap::new());
static LOGGED_PTHREAD_SYNC: AtomicI32 = AtomicI32::new(0);

const TRUEOS_EAGAIN: c_int = 11;
const TRUEOS_EBUSY: c_int = 16;
const TRUEOS_EINVAL: c_int = 22;
const TRUEOS_ERANGE: c_int = 34;
const TRUEOS_ENOSYS: c_int = 38;
const TRUEOS_EIO: c_int = 5;
const TRUEOS_EBADF: c_int = 9;
const TRUEOS_EAI_SYSTEM: c_int = 11;
const TRUEOS_ETIMEDOUT: c_int = 110;
const TRUEOS_O_RDONLY: c_int = 0;
const TRUEOS_SC_PAGESIZE: c_int = 30;
const TRUEOS_SC_PAGE_SIZE: c_int = TRUEOS_SC_PAGESIZE;
const TRUEOS_SC_NPROCESSORS_CONF: c_int = 83;
const TRUEOS_SC_NPROCESSORS_ONLN: c_int = 84;

#[repr(C)]
pub struct Iovec {
    base: *const u8,
    len: usize,
}

#[repr(C)]
pub struct TrueosDir {
    _private: u8,
}

static GAI_STRERROR_SYSTEM: &[u8] = b"trueos getaddrinfo unavailable\0";

#[derive(Clone, Copy)]
struct AllocationRecord {
    size: usize,
    align: usize,
}

#[derive(Clone, Copy)]
struct PthreadCondState {
    generation: u64,
}

#[derive(Clone, Copy)]
struct PthreadMutexState {
    locked: bool,
    owner: usize,
}

fn uart_write(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    crate::shell2::uart1_com1::write_bytes(bytes);
}

fn copy_bytes_to_words(out_words: *mut u32, out_nwords: usize, bytes: &[u8]) -> usize {
    if !out_words.is_null() && out_nwords != 0 {
        let cap = out_nwords.saturating_mul(core::mem::size_of::<u32>());
        if cap >= bytes.len() {
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), out_words.cast::<u8>(), bytes.len());
            }
        }
    }
    bytes.len()
}

fn pthread_key(ptr: *mut c_void) -> Option<usize> {
    let key = ptr as usize;
    if key == 0 { None } else { Some(key) }
}

fn pthread_current_id() -> usize {
    if crate::t::th::vthread::tokio_blocking_backing_enabled()
        && let Some(vtid) = crate::t::th::vthread::current_id()
    {
        return 0x1_0000usize.saturating_add(vtid as usize);
    }
    crate::percpu::current_slot().saturating_add(1)
}

fn pthread_sync_probe_log() {
    if LOGGED_PTHREAD_SYNC
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!("std-abi: pthread mutex/cond shim using TRUEOS vthread/spin ownership\n");
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
        return TRUEOS_EBUSY;
    }
    state.locked = true;
    state.owner = owner;
    0
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

fn c_allocation_layout(size: usize, align: usize) -> Option<Layout> {
    Layout::from_size_align(size.max(1), align.max(1)).ok()
}

fn c_malloc_aligned(size: usize, align: usize) -> *mut c_void {
    let Some(layout) = c_allocation_layout(size, align) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return ptr::null_mut();
    };
    let ptr = unsafe { crate::allocators::alloc_raw(layout) };
    if ptr.is_null() {
        TRUEOS_ERRNO.store(12, Ordering::Relaxed);
        return ptr::null_mut();
    }
    C_ALLOCATIONS.lock().insert(
        ptr as usize,
        AllocationRecord {
            size: size.max(1),
            align: align.max(1),
        },
    );
    ptr.cast::<c_void>()
}

fn c_free_ptr(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    C_ALLOCATIONS.lock().remove(&(ptr as usize));
    unsafe { crate::allocators::dealloc_raw(ptr.cast::<u8>()) };
}

fn c_realloc_ptr(ptr: *mut c_void, size: usize) -> *mut c_void {
    if ptr.is_null() {
        return c_malloc_aligned(size, core::mem::align_of::<usize>());
    }
    if size == 0 {
        c_free_ptr(ptr);
        return ptr::null_mut();
    }
    let old = C_ALLOCATIONS
        .lock()
        .get(&(ptr as usize))
        .copied()
        .unwrap_or(AllocationRecord {
            size: 1,
            align: core::mem::align_of::<usize>(),
        });
    let new_ptr = c_malloc_aligned(size, old.align);
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(
            ptr.cast::<u8>(),
            new_ptr.cast::<u8>(),
            core::cmp::min(old.size, size),
        );
    }
    c_free_ptr(ptr);
    new_ptr
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_alloc_words(nwords: usize) -> *mut u32 {
    let bytes = nwords.saturating_mul(core::mem::size_of::<u32>());
    unsafe { sys_alloc_aligned(bytes, core::mem::align_of::<u32>()) as *mut u32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_alloc_aligned(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }

    let Ok(layout) = Layout::from_size_align(size, align.max(1)) else {
        return ptr::null_mut();
    };

    unsafe { crate::allocators::alloc_raw(layout) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_rand(recv_buf: *mut u32, words: usize) {
    if recv_buf.is_null() || words == 0 {
        return;
    }

    let byte_len = words.saturating_mul(core::mem::size_of::<u32>());
    let bytes = unsafe { slice::from_raw_parts_mut(recv_buf.cast::<u8>(), byte_len) };
    if !crate::tyche::fill_bytes(bytes) {
        unsafe { ptr::write_bytes(recv_buf, 0, words) };
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
    recv_buf: *mut u32,
    words: usize,
    varname: *const u8,
    varname_len: usize,
) -> usize {
    if varname.is_null() {
        return usize::MAX;
    }
    let key_bytes = unsafe { slice::from_raw_parts(varname, varname_len) };
    let Ok(key) = core::str::from_utf8(key_bytes) else {
        return usize::MAX;
    };
    let Some(value) = crate::r::io::env::var(key) else {
        return usize::MAX;
    };
    copy_bytes_to_words(recv_buf, words, value.as_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_argc() -> usize {
    crate::r::io::env::arg_count()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_argv(
    out_words: *mut u32,
    out_nwords: usize,
    arg_index: usize,
) -> usize {
    let Some(arg) = crate::r::io::env::arg(arg_index) else {
        return 0;
    };
    copy_bytes_to_words(out_words, out_nwords, arg.as_bytes())
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
    (&TRUEOS_ERRNO as *const AtomicI32).cast_mut().cast::<c_int>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __errno_location() -> *mut c_int {
    unsafe { errno_location() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __errno() -> *mut c_int {
    unsafe { errno_location() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strerror_r(errnum: c_int, buf: *mut c_char, buflen: usize) -> c_int {
    if buf.is_null() || buflen == 0 {
        return 0;
    }
    let prefix = b"trueos errno ";
    let out = unsafe { slice::from_raw_parts_mut(buf.cast::<u8>(), buflen) };
    let mut pos = 0usize;
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
pub unsafe extern "C" fn __xpg_strerror_r(
    errnum: c_int,
    buf: *mut c_char,
    buflen: usize,
) -> c_int {
    unsafe { strerror_r(errnum, buf, buflen) }
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
    if !align.is_power_of_two() || align < core::mem::size_of::<usize>() {
        return TRUEOS_EINVAL;
    }
    let ptr = c_malloc_aligned(size, align);
    if ptr.is_null() && size != 0 {
        return 12;
    }
    unsafe { *memptr = ptr };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getcwd(buf: *mut c_char, size: usize) -> *mut c_char {
    if buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_ERANGE, Ordering::Relaxed);
        return ptr::null_mut();
    }
    let cwd = b"/";
    if size < cwd.len() + 1 {
        TRUEOS_ERRNO.store(TRUEOS_ERANGE, Ordering::Relaxed);
        return ptr::null_mut();
    }
    let out = unsafe { slice::from_raw_parts_mut(buf.cast::<u8>(), size) };
    out[..cwd.len()].copy_from_slice(cwd);
    out[cwd.len()] = 0;
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
pub unsafe extern "C" fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize {
    if buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if fd != 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    unsafe { sys_read(fd as u32, buf.cast::<u8>(), count) as isize }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readv(fd: c_int, iov: *const Iovec, iovcnt: c_int) -> isize {
    if iov.is_null() || iovcnt < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if fd != 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let entries = unsafe { slice::from_raw_parts(iov, iovcnt as usize) };
    let mut total = 0usize;
    for entry in entries {
        if entry.base.is_null() || entry.len == 0 {
            continue;
        }
        let got = unsafe { sys_read(fd as u32, entry.base.cast_mut(), entry.len) };
        total = total.saturating_add(got);
        if got < entry.len {
            break;
        }
    }
    total as isize
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
pub unsafe extern "C" fn readdir(_dir: *mut c_void) -> *mut c_void {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn stat(_path: *const c_char, _buf: *mut c_void) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn open(_path: *const c_char, flags: c_int, _mode: c_int) -> c_int {
    if flags == TRUEOS_O_RDONLY {
        TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    } else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
    }
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn close(fd: c_int) -> c_int {
    if (0..=2).contains(&fd) {
        return 0;
    }
    TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lseek(_fd: c_int, _offset: isize, _whence: c_int) -> isize {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fstat(_fd: c_int, _buf: *mut c_void) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn opendir(_path: *const c_char) -> *mut TrueosDir {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn closedir(_dir: *mut TrueosDir) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dirfd(_dir: *mut TrueosDir) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getaddrinfo(
    _node: *const c_char,
    _service: *const c_char,
    _hints: *const c_void,
    _res: *mut *mut c_void,
) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    TRUEOS_EAI_SYSTEM
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn freeaddrinfo(_res: *mut c_void) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gai_strerror(_ecode: c_int) -> *const c_char {
    GAI_STRERROR_SYSTEM.as_ptr().cast::<c_char>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysconf(name: c_int) -> isize {
    match name {
        TRUEOS_SC_PAGESIZE => 4096,
        TRUEOS_SC_NPROCESSORS_ONLN | TRUEOS_SC_NPROCESSORS_CONF => {
            crate::workers::background_worker_slots().len().max(1) as isize
        }
        _ => {
            TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
            -1
        }
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
pub unsafe extern "C" fn pthread_self() -> usize {
    pthread_current_id()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setname_np(_thread: usize, _name: *const c_char) -> c_int {
    0
}