extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::{c_char, c_int, c_long, c_void};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use spin::Mutex;

use crate::t::static_map::FixedKeyMap;

static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
static C_ALLOCATIONS: Mutex<FixedKeyMap<usize, AllocationRecord, C_ALLOCATION_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static PTHREAD_CONDS: Mutex<FixedKeyMap<usize, PthreadCondState, PTHREAD_COND_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static PTHREAD_MUTEXES: Mutex<FixedKeyMap<usize, PthreadMutexState, PTHREAD_MUTEX_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static PTHREAD_KEYS: Mutex<FixedKeyMap<usize, usize, PTHREAD_KEY_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static PTHREAD_TLS_VALUES: Mutex<FixedKeyMap<usize, usize, PTHREAD_TLS_VALUE_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static PTHREAD_THREADS: Mutex<FixedKeyMap<usize, PthreadThreadState, PTHREAD_THREAD_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static OPEN_FILES: Mutex<FixedKeyMap<c_int, OpenFile, OPEN_FILE_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static LOGGED_PTHREAD_SYNC: AtomicI32 = AtomicI32::new(0);
static LOGGED_C_ALLOCATION_TRACK_OVERFLOW: AtomicI32 = AtomicI32::new(0);
static PTHREAD_SYNC_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
static PTHREAD_CREATE_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
static PTHREAD_NEXT_THREAD_ID: AtomicUsize = AtomicUsize::new(1);
static NEXT_FILE_FD: AtomicI32 = AtomicI32::new(3);

const PTHREAD_SYNC_TRACE_LIMIT: usize = 48;
const PTHREAD_CREATE_TRACE_LIMIT: usize = 16;
const C_ALLOCATION_CAPACITY: usize = 65536;
const PTHREAD_MUTEX_CAPACITY: usize = 256;
const PTHREAD_COND_CAPACITY: usize = 256;
const PTHREAD_KEY_CAPACITY: usize = 128;
const PTHREAD_TLS_VALUE_CAPACITY: usize = 512;
const PTHREAD_THREAD_CAPACITY: usize = 64;
const OPEN_FILE_CAPACITY: usize = 64;

const TRUEOS_EAGAIN: c_int = 11;
const TRUEOS_EBUSY: c_int = 16;
const TRUEOS_ENOENT: c_int = 2;
const TRUEOS_EINVAL: c_int = 22;
const TRUEOS_ENAMETOOLONG: c_int = 36;
const TRUEOS_ERANGE: c_int = 34;
const TRUEOS_ENOSYS: c_int = 38;
const TRUEOS_EIO: c_int = 5;
const TRUEOS_EBADF: c_int = 9;
const TRUEOS_EPERM: c_int = 1;
const TRUEOS_ESRCH: c_int = 3;
const TRUEOS_ECHILD: c_int = 10;
const TRUEOS_ENOTTY: c_int = 25;
const TRUEOS_EAI_SYSTEM: c_int = 11;
const TRUEOS_EAI_FAMILY: c_int = 5;
const TRUEOS_EAI_MEMORY: c_int = 6;
const TRUEOS_EAI_NONAME: c_int = 8;
const TRUEOS_EAI_SERVICE: c_int = 9;
const TRUEOS_EAI_SOCKTYPE: c_int = 10;
const TRUEOS_ETIMEDOUT: c_int = 110;
const TRUEOS_O_RDONLY: c_int = 0;
const TRUEOS_SC_PAGESIZE: c_int = 30;
const TRUEOS_SC_PAGE_SIZE: c_int = TRUEOS_SC_PAGESIZE;
const TRUEOS_SC_NPROCESSORS_CONF: c_int = 83;
const TRUEOS_SC_NPROCESSORS_ONLN: c_int = 84;
const TRUEOS_AF_UNSPEC: c_int = 0;
const TRUEOS_AF_INET: c_int = 2;
const TRUEOS_SOCK_STREAM: c_int = 1;
const TRUEOS_S_IFDIR: u32 = 0o040000;
const TRUEOS_S_IFREG: u32 = 0o100000;
const TRUEOS_DIR_MODE: u32 = TRUEOS_S_IFDIR | 0o755;
const TRUEOS_FILE_MODE: u32 = TRUEOS_S_IFREG | 0o644;

#[repr(C)]
pub struct Iovec {
    base: *const u8,
    len: usize,
}

#[repr(C)]
pub struct TrueosDir {
    _private: u8,
}

#[repr(C)]
struct TrueosInAddr {
    s_addr: u32,
}

#[repr(C)]
struct TrueosSockAddrIn {
    sin_len: u8,
    sin_family: u8,
    sin_port: u16,
    sin_addr: TrueosInAddr,
    sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrueosAddrInfo {
    ai_flags: c_int,
    ai_family: c_int,
    ai_socktype: c_int,
    ai_protocol: c_int,
    ai_addrlen: u32,
    ai_canonname: *mut c_char,
    ai_addr: *mut c_void,
    ai_next: *mut TrueosAddrInfo,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrueosStat {
    st_dev: u32,
    st_ino: u32,
    st_mode: u32,
    st_nlink: u16,
    st_uid: u32,
    st_gid: u32,
    st_rdev: u32,
    st_size: i64,
    st_atime: i32,
    st_atime_nsec: c_long,
    st_mtime: i32,
    st_mtime_nsec: c_long,
    st_ctime: i32,
    st_ctime_nsec: c_long,
    st_blksize: i32,
    st_blocks: i32,
    st_spare4: [c_long; 2],
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
    depth: usize,
}

struct PthreadThreadState {
    completion: Arc<crate::wait::CompletionCell<usize>>,
    detached: bool,
}

struct OpenFile {
    bytes: Vec<u8>,
    offset: usize,
}

fn uart_write(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    crate::shell2::uart1_com1::write_bytes(bytes);
}

fn write_platform_fd(fd: u32, bytes: &[u8]) {
    match fd {
        1 => crate::r::io::cabi::write_console_bytes(crate::r::io::cabi::ConsoleStream::Out, bytes),
        2 => crate::r::io::cabi::write_console_bytes(crate::r::io::cabi::ConsoleStream::Err, bytes),
        _ => uart_write(bytes),
    }
}

fn posix_rc_i32(rc: c_int) -> c_int {
    if rc < 0 {
        TRUEOS_ERRNO.store(rc.saturating_neg(), Ordering::Relaxed);
        -1
    } else {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        rc
    }
}

fn posix_rc_isize(rc: isize) -> isize {
    if rc < 0 {
        TRUEOS_ERRNO
            .store((rc.saturating_neg()).min(c_int::MAX as isize) as c_int, Ordering::Relaxed);
        -1
    } else {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        rc
    }
}

fn copy_bytes_to_words(out_words: *mut u32, out_nwords: usize, bytes: &[u8]) -> usize {
    if !out_words.is_null() && out_nwords != 0 {
        let cap = out_nwords.saturating_mul(core::mem::size_of::<u32>());
        if cap >= bytes.len() {
            let _ = copy_to_abi_out(out_words.cast::<u8>(), bytes);
        }
    }
    bytes.len()
}

fn copy_vmcall_text_response_to_words(
    status: u32,
    len: u64,
    bytes: &[u8],
    out_words: *mut u32,
    out_nwords: usize,
    missing: usize,
) -> usize {
    if status != trueos_vm::vmcall::STATUS_OK {
        return missing;
    }
    let len = len as usize;
    let n = core::cmp::min(len, bytes.len());
    copy_bytes_to_words(out_words, out_nwords, &bytes[..n])
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

fn pthread_tls_slot(key: usize) -> usize {
    (pthread_current_id() << 32) ^ key
}

fn pthread_sync_probe_log() {
    if LOGGED_PTHREAD_SYNC
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!("std-abi: pthread mutex/cond shim using TRUEOS vthread/spin ownership\n");
    }
}

fn pthread_sync_trace(op: &str, key: usize) {
    let seq = PTHREAD_SYNC_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    if seq < PTHREAD_SYNC_TRACE_LIMIT {
        crate::log!(
            "std-abi: pthread trace seq={} op={} key=0x{:x} owner={}\n",
            seq,
            op,
            key,
            pthread_current_id()
        );
    }
}

fn pthread_create_trace(thread_id: usize, rc: c_int) {
    let seq = PTHREAD_CREATE_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    if seq < PTHREAD_CREATE_TRACE_LIMIT {
        crate::log!(
            "std-abi: pthread_create seq={} thread={} rc={} owner={}\n",
            seq,
            thread_id,
            rc,
            pthread_current_id()
        );
    }
}

fn pthread_mutex_unlock_key(key: usize) -> c_int {
    pthread_sync_trace("mutex.unlock", key);
    let owner = pthread_current_id();
    let mut table = PTHREAD_MUTEXES.lock();
    let Some(state) = table.get_mut(key) else {
        return 0;
    };
    if state.locked && state.owner != owner {
        return TRUEOS_EINVAL;
    }
    if state.depth > 1 {
        state.depth = state.depth.saturating_sub(1);
        return 0;
    }
    state.locked = false;
    state.owner = 0;
    state.depth = 0;
    0
}

fn pthread_mutex_lock_key(key: usize) -> c_int {
    pthread_sync_probe_log();
    pthread_sync_trace("mutex.lock", key);
    let owner = pthread_current_id();
    loop {
        {
            let mut table = PTHREAD_MUTEXES.lock();
            let Some(state) = table.get_or_insert_with(key, || PthreadMutexState {
                locked: false,
                owner: 0,
                depth: 0,
            }) else {
                return TRUEOS_EAGAIN;
            };
            if state.locked && state.owner == owner {
                state.depth = state.depth.saturating_add(1).max(1);
                return 0;
            }
            if !state.locked {
                state.locked = true;
                state.owner = owner;
                state.depth = 1;
                return 0;
            }
        }
        core::hint::spin_loop();
    }
}

fn pthread_mutex_trylock_key(key: usize) -> c_int {
    pthread_sync_probe_log();
    pthread_sync_trace("mutex.trylock", key);
    let owner = pthread_current_id();
    let mut table = PTHREAD_MUTEXES.lock();
    let Some(state) = table.get_or_insert_with(key, || PthreadMutexState {
        locked: false,
        owner: 0,
        depth: 0,
    }) else {
        return TRUEOS_EAGAIN;
    };
    if state.locked && state.owner == owner {
        state.depth = state.depth.saturating_add(1).max(1);
        return 0;
    }
    if state.locked {
        return TRUEOS_EBUSY;
    }
    state.locked = true;
    state.owner = owner;
    state.depth = 1;
    0
}

fn pthread_cond_generation(key: usize) -> u64 {
    let mut table = PTHREAD_CONDS.lock();
    table
        .get_or_insert_with(key, || PthreadCondState { generation: 0 })
        .map(|state| state.generation)
        .unwrap_or(0)
}

fn pthread_cond_notify_key(key: usize) -> c_int {
    let mut table = PTHREAD_CONDS.lock();
    let Some(state) = table.get_or_insert_with(key, || PthreadCondState { generation: 0 }) else {
        return TRUEOS_EAGAIN;
    };
    state.generation = state.generation.wrapping_add(1);
    0
}

fn pthread_thread_finish(thread_id: usize) {
    let mut table = PTHREAD_THREADS.lock();
    let remove = table
        .get(thread_id)
        .map(|state| state.detached)
        .unwrap_or(false);
    if remove {
        let _ = table.remove(thread_id);
    }
}

fn c_allocation_layout(size: usize, align: usize) -> Option<Layout> {
    Layout::from_size_align(size.max(1), align.max(1)).ok()
}

fn c_allocation_insert(ptr: *mut u8, record: AllocationRecord) -> bool {
    C_ALLOCATIONS.lock().insert(ptr as usize, record).is_ok()
}

fn log_c_allocation_track_overflow(ptr: *mut u8, record: AllocationRecord) {
    if LOGGED_C_ALLOCATION_TRACK_OVERFLOW
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!(
            "std-abi: c allocation tracking full; returning untracked ptr=0x{:X} size={} align={} cap={}\n",
            ptr as usize,
            record.size,
            record.align,
            C_ALLOCATION_CAPACITY
        );
    }
}

fn log_posix_memalign_failure(reason: &str, memptr: *mut *mut c_void, size: usize, align: usize) {
    let vm_id = active_abi_guest_vm_id()
        .map(|id| id as usize)
        .unwrap_or(usize::MAX);
    let hull_vm_id = crate::hv::current_hull_guest_context_vm_id()
        .map(|id| id as usize)
        .unwrap_or(usize::MAX);
    crate::log!(
        "std-abi: posix_memalign failed reason={} memptr=0x{:X} size={} align={} active_vm={} hull_vm={}\n",
        reason,
        memptr as usize,
        size,
        align,
        vm_id,
        hull_vm_id,
    );
}

fn c_allocation_remove(ptr: *mut c_void) {
    let _ = C_ALLOCATIONS.lock().remove(ptr as usize);
}

fn c_allocation_get(ptr: *mut c_void) -> Option<AllocationRecord> {
    C_ALLOCATIONS.lock().get(ptr as usize).copied()
}

fn c_malloc_aligned(size: usize, align: usize) -> *mut c_void {
    let Some(layout) = c_allocation_layout(size, align) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return ptr::null_mut();
    };
    let ptr = if let Some(vm_id) = active_abi_alloc_guest_vm_id() {
        crate::allocators::with_hv_guest_alloc_domain(vm_id, || unsafe {
            crate::allocators::alloc_raw(layout)
        })
        .unwrap_or(ptr::null_mut())
    } else {
        unsafe { crate::allocators::alloc_raw(layout) }
    };
    if ptr.is_null() {
        log_posix_memalign_failure("alloc-null", ptr::null_mut(), size, align);
        TRUEOS_ERRNO.store(12, Ordering::Relaxed);
        return ptr::null_mut();
    }
    let record = AllocationRecord {
        size: size.max(1),
        align: align.max(1),
    };
    if !c_allocation_insert(ptr, record) {
        log_c_allocation_track_overflow(ptr, record);
    }
    ptr.cast::<c_void>()
}

fn c_free_ptr(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    c_allocation_remove(ptr);
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
    let Some(old) = c_allocation_get(ptr) else {
        TRUEOS_ERRNO.store(12, Ordering::Relaxed);
        return ptr::null_mut();
    };
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

pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    c_malloc_aligned(size, core::mem::align_of::<usize>())
}

pub unsafe extern "C" fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    let Some(total) = nmemb.checked_mul(size) else {
        TRUEOS_ERRNO.store(12, Ordering::Relaxed);
        return ptr::null_mut();
    };
    if total == 0 {
        return ptr::null_mut();
    }
    let ptr = c_malloc_aligned(total, core::mem::align_of::<usize>());
    if !ptr.is_null() {
        unsafe { ptr::write_bytes(ptr, 0, total) };
    }
    ptr
}

pub unsafe extern "C" fn free(ptr: *mut c_void) {
    c_free_ptr(ptr);
}

pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    c_realloc_ptr(ptr, size)
}

#[repr(C)]
pub struct TrueosCabiHeapStats {
    pub heap_start: usize,
    pub heap_end: usize,
    pub usable_total: usize,
    pub free_bytes: usize,
    pub largest_free_block: usize,
    pub free_blocks: usize,
    pub initialized: u32,
    pub source: u32,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_alloc(size: usize) -> *mut u8 {
    c_malloc_aligned(size, core::mem::align_of::<usize>()).cast::<u8>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_calloc(nmemb: usize, size: usize) -> *mut u8 {
    calloc(nmemb, size).cast::<u8>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_free(ptr: *mut u8) {
    c_free_ptr(ptr.cast::<c_void>());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    c_realloc_ptr(ptr.cast::<c_void>(), size).cast::<u8>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_malloc_usable_size(ptr: *const u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    c_allocation_get(ptr as *mut c_void)
        .map(|record| record.size)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_heap_stats(out: *mut TrueosCabiHeapStats) -> i32 {
    if out.is_null() {
        return -1;
    }
    let stats = crate::allocators::heap_stats();
    let source = match stats.source {
        crate::allocators::HeapSourceKind::Unconfigured => 0,
        crate::allocators::HeapSourceKind::Arena => 1,
    };
    unsafe {
        *out = TrueosCabiHeapStats {
            heap_start: stats.heap_start,
            heap_end: stats.heap_end,
            usable_total: stats.usable_total,
            free_bytes: stats.free_bytes,
            largest_free_block: stats.largest_free_block,
            free_blocks: stats.free_blocks,
            initialized: u32::from(stats.initialized),
            source,
        };
    }
    0
}

unsafe fn cstr_arg(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    abi_cstr_to_string(ptr, 4096)
}

unsafe fn getaddrinfo_service_port(service: *const c_char) -> Result<u16, c_int> {
    let Some(service) = (unsafe { cstr_arg(service) }) else {
        return Ok(0);
    };
    if service.is_empty() {
        return Ok(0);
    }
    let Ok(port) = service.parse::<u16>() else {
        return Err(TRUEOS_EAI_SERVICE);
    };
    Ok(port)
}

fn getaddrinfo_resolve_ipv4(host: &str) -> Result<[u8; 4], c_int> {
    crate::t::net::vlayer::resolve_ipv4_for_sync_abi(host).map_err(dns_resolve_error_to_eai)
}

fn dns_resolve_error_to_eai(err: crate::t::net::vlayer::DnsResolveError) -> c_int {
    match err {
        crate::t::net::vlayer::DnsResolveError::BadName
        | crate::t::net::vlayer::DnsResolveError::NoAnswer => TRUEOS_EAI_NONAME,
        crate::t::net::vlayer::DnsResolveError::Runtime
        | crate::t::net::vlayer::DnsResolveError::NoNic
        | crate::t::net::vlayer::DnsResolveError::Timeout => TRUEOS_EAI_SYSTEM,
    }
}

fn dns_resolve_error_to_cabi_errno(err: crate::t::net::vlayer::DnsResolveError) -> c_int {
    match err {
        crate::t::net::vlayer::DnsResolveError::BadName
        | crate::t::net::vlayer::DnsResolveError::NoAnswer => TRUEOS_EIO,
        crate::t::net::vlayer::DnsResolveError::Runtime
        | crate::t::net::vlayer::DnsResolveError::NoNic
        | crate::t::net::vlayer::DnsResolveError::Timeout => TRUEOS_ETIMEDOUT,
    }
}

fn active_abi_guest_vm_id() -> Option<u8> {
    crate::hv::current_guest_execution_context_vm_id()
        .or_else(crate::hv::current_vm_id_by_lapic_low)
}

fn active_abi_alloc_guest_vm_id() -> Option<u8> {
    crate::hv::current_hull_guest_context_vm_id().or_else(crate::hv::current_vm_id_by_lapic_low)
}

fn active_guest_stack_host_ptr_for_vm(vm_id: u8, ptr: *mut u8, len: usize) -> Option<*mut u8> {
    let guest_va = ptr as usize as u64;
    let offset = guest_va.checked_sub(crate::hv::memory::GUEST_STACK_VA_BASE)? as usize;
    if crate::hv::current_hull_guest_context_vm_id() == Some(vm_id) {
        let stack_bytes = crate::hv::memory::active_guest_stack_bytes_for_vm(vm_id);
        let end = offset.checked_add(len)?;
        if end <= stack_bytes {
            return Some(ptr);
        }
        return None;
    }
    let stack = crate::hv::memory::guest_stack_slice_for_vm(vm_id)?;
    let end = offset.checked_add(len)?;
    if end > stack.len() {
        return None;
    }
    let base = crate::hv::memory::guest_stack_mut_ptr_for_vm(vm_id)?;
    Some(unsafe { base.add(offset) })
}

fn active_guest_heap_host_ptr_for_vm(vm_id: u8, ptr: *mut u8, len: usize) -> Option<*mut u8> {
    let guest_va = ptr as usize;
    let stats = crate::allocators::hv_guest_heap_stats(vm_id);
    if !stats.initialized || stats.heap_end <= stats.heap_start {
        return None;
    }
    let end = guest_va.checked_add(len)?;
    if guest_va >= stats.heap_start && end <= stats.heap_end {
        Some(ptr)
    } else {
        None
    }
}

fn any_guest_host_ptr(ptr: *mut u8, len: usize) -> Option<*mut u8> {
    for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
        let vm_id = vm_id as u8;
        if let Some(host) = active_guest_stack_host_ptr_for_vm(vm_id, ptr, len)
            .or_else(|| active_guest_heap_host_ptr_for_vm(vm_id, ptr, len))
        {
            return Some(host);
        }
    }
    None
}

fn looks_like_low_guest_ptr(ptr: *const u8) -> bool {
    let guest_va = ptr as usize as u64;
    guest_va >= crate::hv::memory::GUEST_STACK_VA_BASE
        && guest_va < crate::hv::memory::GUEST_COMM_PAGE_VA
}

fn abi_host_ptr(ptr: *mut u8, len: usize) -> Option<*mut u8> {
    if ptr.is_null() {
        return None;
    }
    if len == 0 {
        return Some(ptr);
    }
    let Some(vm_id) = active_abi_guest_vm_id() else {
        return any_guest_host_ptr(ptr, len).or_else(|| {
            if looks_like_low_guest_ptr(ptr) {
                None
            } else {
                Some(ptr)
            }
        });
    };
    active_guest_stack_host_ptr_for_vm(vm_id, ptr, len)
        .or_else(|| active_guest_heap_host_ptr_for_vm(vm_id, ptr, len))
        .or_else(|| any_guest_host_ptr(ptr, len))
        .or_else(|| {
            if looks_like_low_guest_ptr(ptr) {
                None
            } else {
                Some(ptr)
            }
        })
}

fn abi_read_bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    let host = abi_host_ptr(ptr.cast_mut(), len)?;
    Some(unsafe { slice::from_raw_parts(host.cast::<u8>(), len) })
}

fn abi_write_bytes<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    if len == 0 {
        return Some(&mut []);
    }
    let host = abi_host_ptr(ptr, len)?;
    Some(unsafe { slice::from_raw_parts_mut(host, len) })
}

fn abi_read_struct<T: Copy>(ptr: *const T) -> Option<T> {
    let bytes = abi_read_bytes(ptr.cast::<u8>(), core::mem::size_of::<T>())?;
    Some(unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}

fn abi_cstr_to_string(ptr: *const c_char, max_len: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut bytes = Vec::new();
    for offset in 0..max_len {
        let byte = *abi_read_bytes(unsafe { ptr.cast::<u8>().add(offset) }, 1)?.first()?;
        if byte == 0 {
            return String::from_utf8(bytes).ok();
        }
        bytes.push(byte);
    }
    None
}

fn copy_to_abi_out(ptr: *mut u8, bytes: &[u8]) -> bool {
    if ptr.is_null() {
        return false;
    }
    let Some(dst) = abi_host_ptr(ptr, bytes.len()) else {
        return false;
    };
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len()) };
    true
}

fn copy_usize_to_abi_out(ptr: *mut usize, value: usize) -> bool {
    copy_to_abi_out(ptr.cast::<u8>(), &value.to_ne_bytes())
}

fn fs_rc_to_errno(rc: i32) -> c_int {
    match rc {
        crate::r::io::cabi::FS_ERR_NOT_FOUND => TRUEOS_ENOENT,
        crate::r::io::cabi::FS_ERR_BAD_PATH
        | crate::r::io::cabi::FS_ERR_BAD_PARAM
        | crate::r::io::cabi::FS_ERR_BAD_UTF8 => TRUEOS_EINVAL,
        crate::r::io::cabi::FS_ERR_TOO_LARGE => TRUEOS_ENAMETOOLONG,
        crate::r::io::cabi::FS_ERR_NO_SPACE => TRUEOS_EIO,
        _ => TRUEOS_EIO,
    }
}

fn read_file_from_cabi(path: &str) -> Result<Vec<u8>, c_int> {
    let len = unsafe {
        crate::r::io::cabi::trueos_cabi_fs_read_file(path.as_ptr(), path.len(), ptr::null_mut(), 0)
    };
    if len < 0 {
        return Err(fs_rc_to_errno(len as i32));
    }

    let mut bytes = Vec::new();
    bytes.resize(len as usize, 0);
    if bytes.is_empty() {
        return Ok(bytes);
    }

    let got = unsafe {
        crate::r::io::cabi::trueos_cabi_fs_read_file(
            path.as_ptr(),
            path.len(),
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    if got < 0 {
        return Err(fs_rc_to_errno(got as i32));
    }
    bytes.truncate(got as usize);
    Ok(bytes)
}

fn next_file_fd() -> c_int {
    NEXT_FILE_FD.fetch_add(1, Ordering::AcqRel)
}

unsafe fn freeaddrinfo_chain(mut res: *mut TrueosAddrInfo) {
    while !res.is_null() {
        let next = unsafe { (*res).ai_next };
        let addr = unsafe { (*res).ai_addr };
        if !addr.is_null() {
            c_free_ptr(addr);
        }
        c_free_ptr(res.cast::<c_void>());
        res = next;
    }
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

    if let Some(vm_id) = active_abi_alloc_guest_vm_id() {
        crate::allocators::with_hv_guest_alloc_domain(vm_id, || unsafe {
            crate::allocators::alloc_raw(layout)
        })
        .unwrap_or(ptr::null_mut())
    } else {
        unsafe { crate::allocators::alloc_raw(layout) }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_rand(recv_buf: *mut u32, words: usize) {
    if recv_buf.is_null() || words == 0 {
        return;
    }

    let byte_len = words.saturating_mul(core::mem::size_of::<u32>());
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let bytes = unsafe { slice::from_raw_parts_mut(recv_buf.cast::<u8>(), byte_len) };
        let mut offset = 0usize;
        let mut chunk = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        while offset < bytes.len() {
            let want = core::cmp::min(chunk.len(), bytes.len() - offset);
            let (status, got) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_RAND_BYTES,
                want as u64,
                0,
                &[],
                &mut chunk[..want],
            );
            if status != trueos_vm::vmcall::STATUS_OK || got as usize != want {
                bytes[offset..].fill(0);
                return;
            }
            bytes[offset..offset + want].copy_from_slice(&chunk[..want]);
            offset += want;
        }
        return;
    }
    let Some(bytes) = abi_write_bytes(recv_buf.cast::<u8>(), byte_len) else {
        return;
    };
    if !crate::tyche::fill_bytes(bytes) {
        bytes.fill(0);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_write(fd: u32, write_buf: *const u8, nbytes: usize) {
    if write_buf.is_null() || nbytes == 0 {
        return;
    }
    let Some(bytes) = abi_read_bytes(write_buf, nbytes) else {
        return;
    };
    write_platform_fd(fd, bytes);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_internal_log_write(bytes: *const u8, len: usize) {
    if bytes.is_null() || len == 0 {
        return;
    }
    let Some(bytes) = abi_read_bytes(bytes, len) else {
        return;
    };
    uart_write(bytes);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_read(_fd: u32, recv_buf: *mut u8, nrequested: usize) -> usize {
    if recv_buf.is_null() || nrequested == 0 {
        return 0;
    }

    let Some(out) = abi_write_bytes(recv_buf, nrequested) else {
        return 0;
    };
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
    let Some(key_bytes) = abi_read_bytes(varname, varname_len) else {
        return usize::MAX;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if key_bytes.len() > trueos_vm::vmcall::PAYLOAD_CAP {
            return usize::MAX;
        }
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, len) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_ENV_VAR,
            0,
            0,
            key_bytes,
            &mut bytes,
        );
        return copy_vmcall_text_response_to_words(
            status,
            len,
            &bytes,
            recv_buf,
            words,
            usize::MAX,
        );
    }
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
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, count) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ENV_ARGS_COUNT, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            count as usize
        } else {
            0
        };
    }
    crate::r::io::env::arg_count()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_argv(
    out_words: *mut u32,
    out_nwords: usize,
    arg_index: usize,
) -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, len) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_ENV_ARG,
            arg_index as u64,
            0,
            &[],
            &mut bytes,
        );
        return copy_vmcall_text_response_to_words(status, len, &bytes, out_words, out_nwords, 0);
    }
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
        let _ = copy_to_abi_out(out_state.cast::<u8>(), &[0; core::mem::size_of::<[u32; 8]>()]);
    } else {
        let Some(state) = abi_read_struct(in_state) else {
            return;
        };
        let bytes = unsafe {
            slice::from_raw_parts(
                (&state as *const [u32; 8]).cast::<u8>(),
                core::mem::size_of::<[u32; 8]>(),
            )
        };
        let _ = copy_to_abi_out(out_state.cast::<u8>(), bytes);
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
    if let Some(bytes) = abi_read_bytes(msg_ptr, len) {
        uart_write(bytes);
    }
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
pub extern "C" fn trueos_cabi_boot_timestamp_secs() -> u64 {
    crate::limine::boot_timestamp_secs().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_current_id() -> usize {
    pthread_current_id()
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ntp_current_unix_seconds() -> u64 {
    crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::r::time::unix_time_seconds)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ntp_kernel_date_day_month_year(
    out_ptr: *mut u8,
    out_cap: usize,
) -> usize {
    let date = crate::r::net::ntp::kernel_date_day_month_year();
    let bytes = date.as_bytes();
    if out_ptr.is_null() || out_cap == 0 {
        return bytes.len();
    }
    let n = core::cmp::min(bytes.len(), out_cap);
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, n);
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_panic(msg_ptr: *const u8, len: usize) -> ! {
    if !msg_ptr.is_null() && len != 0 {
        if let Some(bytes) = abi_read_bytes(msg_ptr, len) {
            uart_write(b"std-trueos panic: ");
            uart_write(bytes);
            uart_write(b"\n");
        }
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
pub unsafe extern "C" fn exit(code: c_int) -> ! {
    let _ = code;
    uart_write(b"std-abi: exit\n");
    unsafe { sys_halt() }
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
pub unsafe extern "C" fn __errno() -> *mut c_int {
    unsafe { errno_location() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strerror_r(errnum: c_int, buf: *mut c_char, buflen: usize) -> c_int {
    if buf.is_null() || buflen == 0 {
        return 0;
    }
    let prefix = b"trueos errno ";
    let Some(out) = abi_write_bytes(buf.cast::<u8>(), buflen) else {
        return TRUEOS_EINVAL;
    };
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
pub unsafe extern "C" fn __xpg_strerror_r(errnum: c_int, buf: *mut c_char, buflen: usize) -> c_int {
    unsafe { strerror_r(errnum, buf, buflen) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_memalign(
    memptr: *mut *mut c_void,
    align: usize,
    size: usize,
) -> c_int {
    if memptr.is_null() {
        log_posix_memalign_failure("null-memptr", memptr, size, align);
        return TRUEOS_EINVAL;
    }
    if !align.is_power_of_two() || align < core::mem::size_of::<usize>() {
        log_posix_memalign_failure("bad-align", memptr, size, align);
        return TRUEOS_EINVAL;
    }
    let ptr = c_malloc_aligned(size, align);
    if ptr.is_null() && size != 0 {
        return 12;
    }
    if !copy_usize_to_abi_out(memptr.cast::<usize>(), ptr as usize) {
        c_free_ptr(ptr);
        log_posix_memalign_failure("copy-out-failed", memptr, size, align);
        return TRUEOS_EINVAL;
    }
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
    let Some(out) = abi_write_bytes(buf.cast::<u8>(), size) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return ptr::null_mut();
    };
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
    if fd == 0 {
        return unsafe { sys_read(fd as u32, buf.cast::<u8>(), count) as isize };
    }

    if fd < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }

    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    let remaining = file.bytes.len().saturating_sub(file.offset);
    let n = core::cmp::min(count, remaining);
    if n != 0 && !copy_to_abi_out(buf.cast::<u8>(), &file.bytes[file.offset..file.offset + n]) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    file.offset = file.offset.saturating_add(n);
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    n as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn socket(domain: c_int, socket_type: c_int, protocol: c_int) -> c_int {
    posix_rc_i32(crate::r::net::socket_cabi::trueos_cabi_socket_tcp_open(
        domain,
        socket_type,
        protocol,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setsockopt(
    socket_id: c_int,
    _level: c_int,
    _optname: c_int,
    optval: *const c_void,
    optlen: u32,
) -> c_int {
    if socket_id < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    if optlen != 0 && optval.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }

    let rc =
        crate::r::net::socket_cabi::trueos_cabi_socket_tcp_set_nonblocking(socket_id as u32, 0);
    posix_rc_i32(if rc < 0 { rc } else { 0 })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn send(
    socket_id: c_int,
    buf: *const c_void,
    len: usize,
    _flags: c_int,
) -> isize {
    if socket_id < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    if len != 0 && buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }

    posix_rc_isize(crate::r::net::socket_cabi::trueos_cabi_socket_tcp_send(
        socket_id as u32,
        buf.cast::<u8>(),
        len,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn recv(
    socket_id: c_int,
    buf: *mut c_void,
    len: usize,
    flags: c_int,
) -> isize {
    if socket_id < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    if len != 0 && buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }

    posix_rc_isize(crate::r::net::socket_cabi::trueos_cabi_socket_tcp_recv(
        socket_id as u32,
        buf.cast::<u8>(),
        len,
        flags,
        0,
        u64::MAX,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readv(fd: c_int, iov: *const Iovec, iovcnt: c_int) -> isize {
    if iov.is_null() || iovcnt < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let Some(entries) = abi_read_bytes(
        iov.cast::<u8>(),
        (iovcnt as usize).saturating_mul(core::mem::size_of::<Iovec>()),
    ) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let mut total = 0usize;
    for chunk in entries.chunks_exact(core::mem::size_of::<Iovec>()) {
        let entry = unsafe { ptr::read_unaligned(chunk.as_ptr().cast::<Iovec>()) };
        if entry.base.is_null() || entry.len == 0 {
            continue;
        }
        let got = unsafe { read(fd, entry.base.cast_mut().cast::<c_void>(), entry.len) };
        if got < 0 {
            return if total == 0 { -1 } else { total as isize };
        }
        let got = got as usize;
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
    let Some(entries) = abi_read_bytes(
        iov.cast::<u8>(),
        (iovcnt as usize).saturating_mul(core::mem::size_of::<Iovec>()),
    ) else {
        return -1;
    };
    let mut written = 0usize;
    for chunk in entries.chunks_exact(core::mem::size_of::<Iovec>()) {
        let entry = unsafe { ptr::read_unaligned(chunk.as_ptr().cast::<Iovec>()) };
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
pub unsafe extern "C" fn readdir_r(
    _dir: *mut c_void,
    _entry: *mut c_void,
    result: *mut *mut c_void,
) -> c_int {
    if !result.is_null() {
        let Some(out) = abi_write_bytes(result.cast::<u8>(), core::mem::size_of::<usize>()) else {
            TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
            return TRUEOS_EINVAL;
        };
        out.copy_from_slice(&0usize.to_ne_bytes());
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn stat(path: *const c_char, buf: *mut c_void) -> c_int {
    if path.is_null() || buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let Some(path) = abi_cstr_to_string(path, 4096) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };

    let mut kind = 0u32;
    let mut len = 0u64;
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_fs_stat(
            path.as_ptr(),
            path.len(),
            &mut kind as *mut u32,
            &mut len as *mut u64,
        )
    };
    if rc != 0 {
        TRUEOS_ERRNO.store(fs_rc_to_errno(rc), Ordering::Relaxed);
        return -1;
    }

    let mode = match kind {
        1 => TRUEOS_FILE_MODE,
        2 => TRUEOS_DIR_MODE,
        _ => {
            TRUEOS_ERRNO.store(TRUEOS_EIO, Ordering::Relaxed);
            return -1;
        }
    };
    let blocks = core::cmp::min(len.saturating_add(511) / 512, i32::MAX as u64) as i32;
    let out = TrueosStat {
        st_dev: 1,
        st_ino: 1,
        st_mode: mode,
        st_nlink: if kind == 2 { 2 } else { 1 },
        st_uid: 0,
        st_gid: 0,
        st_rdev: 0,
        st_size: core::cmp::min(len, i64::MAX as u64) as i64,
        st_atime: 0,
        st_atime_nsec: 0,
        st_mtime: 0,
        st_mtime_nsec: 0,
        st_ctime: 0,
        st_ctime_nsec: 0,
        st_blksize: 1024,
        st_blocks: blocks,
        st_spare4: [0; 2],
    };
    if !copy_to_abi_out(buf.cast::<u8>(), unsafe {
        slice::from_raw_parts(
            (&out as *const TrueosStat).cast::<u8>(),
            core::mem::size_of::<TrueosStat>(),
        )
    }) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lstat(path: *const c_char, buf: *mut c_void) -> c_int {
    unsafe { stat(path, buf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn open(path: *const c_char, flags: c_int, _mode: c_int) -> c_int {
    if path.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if flags & 0x3 != TRUEOS_O_RDONLY {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let Some(path) = abi_cstr_to_string(path, 4096) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };

    let bytes = match read_file_from_cabi(path.as_str()) {
        Ok(bytes) => bytes,
        Err(errno) => {
            TRUEOS_ERRNO.store(errno, Ordering::Relaxed);
            return -1;
        }
    };
    let fd = next_file_fd();
    if OPEN_FILES
        .lock()
        .insert(fd, OpenFile { bytes, offset: 0 })
        .is_err()
    {
        TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    fd
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn close(fd: c_int) -> c_int {
    if (0..=2).contains(&fd) {
        return 0;
    }
    if OPEN_FILES.lock().remove(fd).is_some() {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
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
pub unsafe extern "C" fn fstat(fd: c_int, buf: *mut c_void) -> c_int {
    if buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let table = OPEN_FILES.lock();
    let Some(file) = table.get(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    let len = file.bytes.len() as u64;
    let blocks = core::cmp::min(len.saturating_add(511) / 512, i32::MAX as u64) as i32;
    let out = TrueosStat {
        st_dev: 1,
        st_ino: fd as u32,
        st_mode: TRUEOS_FILE_MODE,
        st_nlink: 1,
        st_uid: 0,
        st_gid: 0,
        st_rdev: 0,
        st_size: core::cmp::min(len, i64::MAX as u64) as i64,
        st_atime: 0,
        st_atime_nsec: 0,
        st_mtime: 0,
        st_mtime_nsec: 0,
        st_ctime: 0,
        st_ctime_nsec: 0,
        st_blksize: 1024,
        st_blocks: blocks,
        st_spare4: [0; 2],
    };
    if !copy_to_abi_out(buf.cast::<u8>(), unsafe {
        slice::from_raw_parts(
            (&out as *const TrueosStat).cast::<u8>(),
            core::mem::size_of::<TrueosStat>(),
        )
    }) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
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
pub unsafe extern "C" fn unlink(_path: *const c_char) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readlink(
    _path: *const c_char,
    _buf: *mut c_char,
    _bufsiz: usize,
) -> isize {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn realpath(
    _path: *const c_char,
    _resolved_path: *mut c_char,
) -> *mut c_char {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getaddrinfo(
    node: *const c_char,
    service: *const c_char,
    hints: *const c_void,
    res: *mut *mut c_void,
) -> c_int {
    if res.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return TRUEOS_EAI_SYSTEM;
    }
    if !copy_usize_to_abi_out(res.cast::<usize>(), 0) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return TRUEOS_EAI_SYSTEM;
    }

    let Some(host) = (unsafe { cstr_arg(node) }) else {
        return TRUEOS_EAI_NONAME;
    };
    if host.trim().is_empty() {
        return TRUEOS_EAI_NONAME;
    }

    let (socktype, protocol) = if hints.is_null() {
        (TRUEOS_SOCK_STREAM, 0)
    } else {
        let Some(hints) = abi_read_struct(hints.cast::<TrueosAddrInfo>()) else {
            TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
            return TRUEOS_EAI_SYSTEM;
        };
        if hints.ai_family != TRUEOS_AF_UNSPEC && hints.ai_family != TRUEOS_AF_INET {
            return TRUEOS_EAI_FAMILY;
        }
        if hints.ai_socktype != 0 && hints.ai_socktype != TRUEOS_SOCK_STREAM {
            return TRUEOS_EAI_SOCKTYPE;
        }
        (hints.ai_socktype.max(TRUEOS_SOCK_STREAM), hints.ai_protocol)
    };

    let port = match unsafe { getaddrinfo_service_port(service) } {
        Ok(port) => port,
        Err(err) => return err,
    };
    let ip = match getaddrinfo_resolve_ipv4(host.as_str()) {
        Ok(ip) => ip,
        Err(err) => return err,
    };

    let addr_ptr = c_malloc_aligned(
        core::mem::size_of::<TrueosSockAddrIn>(),
        core::mem::align_of::<TrueosSockAddrIn>(),
    )
    .cast::<TrueosSockAddrIn>();
    if addr_ptr.is_null() {
        return TRUEOS_EAI_MEMORY;
    }

    let info_ptr = c_malloc_aligned(
        core::mem::size_of::<TrueosAddrInfo>(),
        core::mem::align_of::<TrueosAddrInfo>(),
    )
    .cast::<TrueosAddrInfo>();
    if info_ptr.is_null() {
        c_free_ptr(addr_ptr.cast::<c_void>());
        return TRUEOS_EAI_MEMORY;
    }

    unsafe {
        *addr_ptr = TrueosSockAddrIn {
            sin_len: core::mem::size_of::<TrueosSockAddrIn>() as u8,
            sin_family: TRUEOS_AF_INET as u8,
            sin_port: port.to_be(),
            sin_addr: TrueosInAddr {
                s_addr: u32::from_ne_bytes(ip),
            },
            sin_zero: [0; 8],
        };
        *info_ptr = TrueosAddrInfo {
            ai_flags: 0,
            ai_family: TRUEOS_AF_INET,
            ai_socktype: socktype,
            ai_protocol: protocol,
            ai_addrlen: core::mem::size_of::<TrueosSockAddrIn>() as u32,
            ai_canonname: ptr::null_mut(),
            ai_addr: addr_ptr.cast::<c_void>(),
            ai_next: ptr::null_mut(),
        };
    }
    if !copy_usize_to_abi_out(res.cast::<usize>(), info_ptr.cast::<c_void>() as usize) {
        c_free_ptr(addr_ptr.cast::<c_void>());
        c_free_ptr(info_ptr.cast::<c_void>());
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return TRUEOS_EAI_SYSTEM;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn freeaddrinfo(res: *mut c_void) {
    unsafe { freeaddrinfo_chain(res.cast::<TrueosAddrInfo>()) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_dns_resolve_ipv4(
    host: *const u8,
    host_len: usize,
    out_octets: *mut u8,
) -> c_int {
    if host.is_null() || out_octets.is_null() || host_len == 0 {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return TRUEOS_EINVAL;
    }
    let Some(host_bytes) = abi_read_bytes(host, host_len) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return TRUEOS_EINVAL;
    };
    let Ok(host_name) = core::str::from_utf8(host_bytes) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return TRUEOS_EINVAL;
    };
    match crate::t::net::vlayer::resolve_ipv4_for_sync_abi(host_name) {
        Ok(ip) => {
            if copy_to_abi_out(out_octets, &ip) {
                0
            } else {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                TRUEOS_EINVAL
            }
        }
        Err(err) => {
            let errno = dns_resolve_error_to_cabi_errno(err);
            TRUEOS_ERRNO.store(errno, Ordering::Relaxed);
            errno
        }
    }
}

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
pub unsafe extern "C" fn sched_yield() -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tcgetattr(fd: c_int, _termios_p: *mut c_void) -> c_int {
    if !(0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
    } else {
        TRUEOS_ERRNO.store(TRUEOS_ENOTTY, Ordering::Relaxed);
    }
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tcsetattr(
    fd: c_int,
    _optional_actions: c_int,
    _termios_p: *const c_void,
) -> c_int {
    if !(0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
    } else {
        TRUEOS_ERRNO.store(TRUEOS_ENOTTY, Ordering::Relaxed);
    }
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn signal(_signum: c_int, handler: usize) -> usize {
    handler
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn waitpid(_pid: c_int, _status: *mut c_int, _options: c_int) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ECHILD, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setuid(_uid: u32) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_EPERM, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setgid(_gid: u32) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_EPERM, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setgroups(_size: usize, _list: *const u32) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_EPERM, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setsid() -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setpgid(_pid: c_int, _pgid: c_int) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_key_create(key: *mut u32, _destructor: *const c_void) -> c_int {
    if key.is_null() {
        return TRUEOS_EINVAL;
    }
    static NEXT_PTHREAD_KEY: AtomicUsize = AtomicUsize::new(1);
    let next = NEXT_PTHREAD_KEY.fetch_add(1, Ordering::AcqRel);
    if next > u32::MAX as usize {
        return TRUEOS_EAGAIN;
    }
    if PTHREAD_KEYS.lock().insert(next, 0).is_err() {
        return TRUEOS_EAGAIN;
    }
    let bytes = (next as u32).to_ne_bytes();
    let Some(out) = abi_write_bytes(key.cast::<u8>(), core::mem::size_of::<u32>()) else {
        let _ = PTHREAD_KEYS.lock().remove(next);
        return TRUEOS_EINVAL;
    };
    out.copy_from_slice(&bytes);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_key_delete(key: u32) -> c_int {
    let key = key as usize;
    let _ = PTHREAD_KEYS.lock().remove(key);
    let slot = pthread_tls_slot(key);
    let _ = PTHREAD_TLS_VALUES.lock().remove(slot);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setspecific(key: u32, value: *const c_void) -> c_int {
    let key = key as usize;
    if PTHREAD_KEYS.lock().get(key).is_none() {
        return TRUEOS_EINVAL;
    }
    let slot = pthread_tls_slot(key);
    let value = value as usize;
    let mut values = PTHREAD_TLS_VALUES.lock();
    if value == 0 {
        let _ = values.remove(slot);
        return 0;
    }
    values.insert(slot, value).map(|_| ()).unwrap_or(());
    if values.get(slot).copied() == Some(value) {
        0
    } else {
        TRUEOS_EAGAIN
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_getspecific(key: u32) -> *mut c_void {
    let key = key as usize;
    if PTHREAD_KEYS.lock().get(key).is_none() {
        return ptr::null_mut();
    }
    let slot = pthread_tls_slot(key);
    PTHREAD_TLS_VALUES.lock().get(slot).copied().unwrap_or(0) as *mut c_void
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
    pthread_sync_trace("mutex.init", key);
    let mut table = PTHREAD_MUTEXES.lock();
    let Some(state) = table.get_or_insert_with(key, || PthreadMutexState {
        locked: false,
        owner: 0,
        depth: 0,
    }) else {
        return TRUEOS_EAGAIN;
    };
    *state = PthreadMutexState {
        locked: false,
        owner: 0,
        depth: 0,
    };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_destroy(mutex: *mut c_void) -> c_int {
    if let Some(key) = pthread_key(mutex) {
        pthread_sync_trace("mutex.destroy", key);
        let _ = PTHREAD_MUTEXES.lock().remove(key);
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
    pthread_sync_trace("cond.init", key);
    let mut table = PTHREAD_CONDS.lock();
    let Some(state) = table.get_or_insert_with(key, || PthreadCondState { generation: 0 }) else {
        return TRUEOS_EAGAIN;
    };
    *state = PthreadCondState { generation: 0 };
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
        pthread_sync_trace("cond.destroy", key);
        let _ = PTHREAD_CONDS.lock().remove(key);
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

    pthread_sync_trace("cond.wait", cond_key);
    pthread_sync_trace("cond.wait.mutex", mutex_key);

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

    pthread_sync_trace("cond.timedwait", cond_key);
    pthread_sync_trace("cond.timedwait.mutex", mutex_key);

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
    pthread_sync_trace("cond.signal", key);
    pthread_cond_notify_key(key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_cond_broadcast(cond: *mut c_void) -> c_int {
    let Some(key) = pthread_key(cond) else {
        return TRUEOS_EINVAL;
    };
    pthread_sync_trace("cond.broadcast", key);
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_create(
    thread: *mut usize,
    _attr: *const c_void,
    start_routine: *const c_void,
    arg: *mut c_void,
) -> c_int {
    if thread.is_null() || start_routine.is_null() {
        return TRUEOS_EINVAL;
    }

    let thread_id = PTHREAD_NEXT_THREAD_ID.fetch_add(1, Ordering::AcqRel);
    let permit = match crate::t::app_exec::admit_current_app_work(
        crate::t::app_exec::AppWorkKind::Pthread,
        thread_id,
    ) {
        Ok(permit) => permit,
        Err(_) => return TRUEOS_EAGAIN,
    };
    let completion = Arc::new(crate::wait::CompletionCell::new());
    let state = PthreadThreadState {
        completion: completion.clone(),
        detached: false,
    };

    if PTHREAD_THREADS.lock().insert(thread_id, state).is_err() {
        return TRUEOS_EAGAIN;
    }

    let id_bytes = thread_id.to_ne_bytes();
    let Some(out) = abi_write_bytes(thread.cast::<u8>(), core::mem::size_of::<usize>()) else {
        let _ = PTHREAD_THREADS.lock().remove(thread_id);
        return TRUEOS_EINVAL;
    };
    out.copy_from_slice(&id_bytes);

    let start = start_routine as usize;
    let arg = arg as usize;
    let job = Box::new(move || {
        let _permit = permit;
        let start: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
            unsafe { core::mem::transmute(start) };
        let result = unsafe { start(arg as *mut c_void) } as usize;
        let _ = completion.complete(result);
        pthread_thread_finish(thread_id);
    });

    let rc = crate::t::trueos_tokio_worker::trueos_tokio_spawn_blocking_job(job);
    pthread_create_trace(thread_id, rc);
    if rc == 0 {
        0
    } else {
        let _ = PTHREAD_THREADS.lock().remove(thread_id);
        TRUEOS_EAGAIN
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_join(thread: usize, retval: *mut *mut c_void) -> c_int {
    let completion = {
        let table = PTHREAD_THREADS.lock();
        let Some(state) = table.get(thread) else {
            return TRUEOS_ESRCH;
        };
        if state.detached {
            return TRUEOS_EINVAL;
        }
        state.completion.clone()
    };

    let result = completion.join_blocking();
    let _ = PTHREAD_THREADS.lock().remove(thread);

    if !retval.is_null() {
        let bytes = result.to_ne_bytes();
        let Some(out) = abi_write_bytes(retval.cast::<u8>(), core::mem::size_of::<usize>()) else {
            return TRUEOS_EINVAL;
        };
        out.copy_from_slice(&bytes);
    }

    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_detach(thread: usize) -> c_int {
    let mut table = PTHREAD_THREADS.lock();
    let Some(state) = table.get_mut(thread) else {
        return TRUEOS_ESRCH;
    };
    if state.completion.try_take().is_some() {
        let _ = table.remove(thread);
    } else {
        state.detached = true;
    }
    0
}
