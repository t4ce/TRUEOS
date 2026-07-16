extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::{c_char, c_double, c_int, c_long, c_void};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use crate::r::static_map::FixedKeyMap;

pub(crate) static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
static C_ALLOCATIONS: Mutex<FixedKeyMap<usize, AllocationRecord, C_ALLOCATION_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static PTHREAD_KEYS: Mutex<FixedKeyMap<usize, usize, PTHREAD_KEY_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static PTHREAD_TLS_VALUES: Mutex<FixedKeyMap<PthreadTlsSlot, usize, PTHREAD_TLS_VALUE_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static PTHREAD_THREADS: Mutex<FixedKeyMap<usize, PthreadThreadState, PTHREAD_THREAD_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
pub(crate) static OPEN_FILES: Mutex<FixedKeyMap<c_int, OpenFile, OPEN_FILE_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
static FD_FLAGS: Mutex<FixedKeyMap<c_int, c_int, FD_FLAG_CAPACITY>> =
    Mutex::new(FixedKeyMap::new());
pub(crate) static STD_FD_FLAGS: [AtomicI32; 3] =
    [AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0)];
static SOCKET_FDS: Mutex<FixedKeyMap<c_int, SocketFd, SOCKET_FD_CAPACITY>> =
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
const PTHREAD_KEY_CAPACITY: usize = 128;
const PTHREAD_TLS_VALUE_CAPACITY: usize = 512;
const PTHREAD_THREAD_CAPACITY: usize = 64;
// Guest Hull BSS and host-carrier BSS intentionally have independent thread
// counters. Tag carrier-issued opaque pthread handles inside the u32 range so
// equal local sequence numbers can never become the same mutex owner.
const PTHREAD_THREAD_CARRIER_TAG: usize = crate::stackkeeper::BLUEPRINT_THREAD_CARRIER_TAG as usize;
const PTHREAD_THREAD_SEQUENCE_MASK: usize = PTHREAD_THREAD_CARRIER_TAG - 1;
const OPEN_FILE_CAPACITY: usize = 64;
const FD_FLAG_CAPACITY: usize = 256;
const SOCKET_FD_CAPACITY: usize = 128;

pub(crate) const TRUEOS_EAGAIN: c_int = 11;
const TRUEOS_EADDRINUSE: c_int = 98;
const TRUEOS_EBUSY: c_int = 16;
const TRUEOS_EDEADLK: c_int = 35;
const TRUEOS_ENOENT: c_int = 2;
pub(crate) const TRUEOS_EINVAL: c_int = 22;
const TRUEOS_ENAMETOOLONG: c_int = 36;
const TRUEOS_ERANGE: c_int = 34;
const TRUEOS_ENOSYS: c_int = 38;
const TRUEOS_EIO: c_int = 5;
pub(crate) const TRUEOS_EBADF: c_int = 9;
const TRUEOS_EPERM: c_int = 1;
const TRUEOS_ESRCH: c_int = 3;
const TRUEOS_ECHILD: c_int = 10;
pub(crate) const TRUEOS_ENOTTY: c_int = 25;
const TRUEOS_EAI_SYSTEM: c_int = 11;
const TRUEOS_EAI_FAMILY: c_int = 5;
const TRUEOS_EAI_MEMORY: c_int = 6;
const TRUEOS_EAI_NONAME: c_int = 8;
const TRUEOS_EAI_SERVICE: c_int = 9;
const TRUEOS_EAI_SOCKTYPE: c_int = 10;
const TRUEOS_ETIMEDOUT: c_int = 110;
const TRUEOS_ENOMEM: c_int = 12;
const TRUEOS_O_ACCMODE: c_int = 0x3;
const TRUEOS_O_RDONLY: c_int = 0;
const TRUEOS_O_WRONLY: c_int = 1;
const TRUEOS_O_RDWR: c_int = 2;
const TRUEOS_O_CREAT: c_int = 0o100;
const TRUEOS_O_TRUNC: c_int = 0o1000;
const TRUEOS_O_NONBLOCK: c_int = 0o4000;
const TRUEOS_F_GETFD: c_int = 1;
const TRUEOS_F_SETFD: c_int = 2;
const TRUEOS_F_GETFL: c_int = 3;
const TRUEOS_F_SETFL: c_int = 4;
const TRUEOS_FD_CLOEXEC: c_int = 1;
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
const TRUEOS_ASYNC_WRITE_BEGIN_RETRIES: usize = 100;
// TRUEOSFS writes can spend real time in placement/index work on cold or busy
// media. A POSIX close/fsync path should fail on actual IO errors, not on a
// short userland-style patience budget.
const TRUEOS_ASYNC_WRITE_TIMEOUT_MS: u64 = 120_000;

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

#[derive(Clone, Copy)]
struct SocketAddrV4 {
    addr: [u8; 4],
    port: u16,
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
    st_dev: u64,
    st_ino: u64,
    st_nlink: u64,
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    __pad0: u32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atime: i64,
    st_atime_nsec: i64,
    st_mtime: i64,
    st_mtime_nsec: i64,
    st_ctime: i64,
    st_ctime_nsec: i64,
    __unused: [i64; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrueosTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrueosTm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

static GAI_STRERROR_SYSTEM: &[u8] = b"trueos getaddrinfo unavailable\0";
static TRUEOS_UTC_TZ: &[u8] = b"UTC\0";

#[derive(Clone, Copy)]
struct AllocationRecord {
    size: usize,
    align: usize,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct PthreadTlsSlot {
    owner: usize,
    key: usize,
}

// The Blueprint target uses the x86_64 Linux pthread ABI: pthread_mutex_t is
// 40 bytes and pthread_cond_t is 48 bytes, both 8-byte aligned. Keep the
// synchronization state in those ABI objects instead of in a Hull-global
// address registry. Blueprint code and its host service-lane carriers already
// share the object's guest-heap backing, while Hull .bss is intentionally
// private to each realm. Inline state therefore gives both realms one source
// of truth and has no fixed registry capacity to exhaust.
#[repr(C)]
struct PthreadMutexStorage {
    owner: AtomicUsize,
    depth: AtomicUsize,
    kind: AtomicI32,
}

#[repr(C)]
struct PthreadCondStorage {
    generation: AtomicU64,
}

const PTHREAD_MUTEX_ABI_BYTES: usize = 40;
const PTHREAD_MUTEXATTR_ABI_BYTES: usize = core::mem::size_of::<c_int>();
// x86_64 Linux pthread ABI values. PTHREAD_MUTEX_DEFAULT aliases NORMAL.
const TRUEOS_PTHREAD_MUTEX_NORMAL: c_int = 0;
const TRUEOS_PTHREAD_MUTEX_RECURSIVE: c_int = 1;
const TRUEOS_PTHREAD_MUTEX_ERRORCHECK: c_int = 2;
const PTHREAD_COND_ABI_BYTES: usize = 48;
const _: () = assert!(core::mem::size_of::<PthreadMutexStorage>() <= PTHREAD_MUTEX_ABI_BYTES);
const _: () = assert!(core::mem::size_of::<PthreadCondStorage>() <= PTHREAD_COND_ABI_BYTES);

struct PthreadThreadState {
    completion: Arc<crate::wait::CompletionCell<usize>>,
}

#[derive(Default)]
pub(crate) struct BytePipe {
    pub(crate) bytes: Vec<u8>,
    pub(crate) read_open: bool,
    pub(crate) write_open: bool,
}

pub(crate) enum OpenFile {
    Regular {
        path: Option<String>,
        bytes: Vec<u8>,
        offset: usize,
        readable: bool,
        writable: bool,
        dirty: bool,
        flags: c_int,
    },
    PipeRead {
        pipe: Arc<Mutex<BytePipe>>,
        flags: c_int,
    },
    PipeWrite {
        pipe: Arc<Mutex<BytePipe>>,
        flags: c_int,
    },
    UnixSocket {
        rx: Arc<Mutex<BytePipe>>,
        tx: Arc<Mutex<BytePipe>>,
        flags: c_int,
    },
}

enum SocketFd {
    Cabi {
        backend: u32,
    },
    PendingListener {
        backend: u32,
        local: Option<SocketAddrV4>,
    },
    MioListener {
        backend: u32,
        local: SocketAddrV4,
    },
    MioStream {
        backend: u32,
    },
}

impl SocketFd {
    fn backend(&self) -> u32 {
        match self {
            Self::Cabi { backend }
            | Self::PendingListener { backend, .. }
            | Self::MioListener { backend, .. }
            | Self::MioStream { backend } => *backend,
        }
    }
}

impl OpenFile {
    fn len(&self) -> usize {
        match self {
            Self::Regular { bytes, .. } => bytes.len(),
            Self::PipeRead { pipe, .. } | Self::PipeWrite { pipe, .. } => pipe.lock().bytes.len(),
            Self::UnixSocket { rx, .. } => rx.lock().bytes.len(),
        }
    }

    fn offset(&self) -> usize {
        match self {
            Self::Regular { offset, .. } => *offset,
            Self::PipeRead { .. } | Self::PipeWrite { .. } | Self::UnixSocket { .. } => 0,
        }
    }

    fn set_offset(&mut self, next: usize) {
        match self {
            Self::Regular { offset, .. } => *offset = next,
            Self::PipeRead { .. } | Self::PipeWrite { .. } | Self::UnixSocket { .. } => {
                let _ = next;
            }
        }
    }

    fn resize(&mut self, next: usize) -> bool {
        match self {
            Self::Regular {
                bytes,
                writable,
                dirty,
                ..
            } => {
                if !*writable {
                    return false;
                }
                bytes.resize(next, 0);
                *dirty = true;
                true
            }
            Self::PipeRead { .. } | Self::PipeWrite { .. } | Self::UnixSocket { .. } => false,
        }
    }

    pub(crate) fn flags(&self) -> c_int {
        match self {
            Self::Regular { flags, .. }
            | Self::PipeRead { flags, .. }
            | Self::PipeWrite { flags, .. }
            | Self::UnixSocket { flags, .. } => *flags,
        }
    }

    pub(crate) fn set_flags(&mut self, next: c_int) {
        match self {
            Self::Regular { flags, .. }
            | Self::PipeRead { flags, .. }
            | Self::PipeWrite { flags, .. }
            | Self::UnixSocket { flags, .. } => *flags = next,
        }
    }

    pub(crate) fn readable_len(&self) -> usize {
        match self {
            Self::Regular { bytes, offset, .. } => bytes.len().saturating_sub(*offset),
            Self::PipeRead { pipe, .. } => pipe.lock().bytes.len(),
            Self::PipeWrite { .. } => 0,
            Self::UnixSocket { rx, .. } => rx.lock().bytes.len(),
        }
    }
}

fn write_platform_fd(fd: u32, bytes: &[u8]) {
    match fd {
        1 => crate::r::io::cabi::write_console_bytes(crate::r::io::cabi::ConsoleStream::Out, bytes),
        2 => crate::r::io::cabi::write_console_bytes(crate::r::io::cabi::ConsoleStream::Err, bytes),
        _ => {}
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

fn mio_status_to_errno(status: i32) -> c_int {
    match status {
        -2 => TRUEOS_EAGAIN,
        -3 => TRUEOS_EIO,
        -4 => TRUEOS_EINVAL,
        -5 => TRUEOS_EBADF,
        -8 => TRUEOS_EIO,
        _ => TRUEOS_EIO,
    }
}

fn posix_mio_i32(status: i32) -> c_int {
    if status < 0 {
        TRUEOS_ERRNO.store(mio_status_to_errno(status), Ordering::Relaxed);
        -1
    } else {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        status
    }
}

fn posix_mio_isize(status: isize) -> isize {
    if status < 0 {
        TRUEOS_ERRNO
            .store(mio_status_to_errno(status.max(i32::MIN as isize) as i32), Ordering::Relaxed);
        -1
    } else {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        status
    }
}

fn parse_sockaddr_v4(addr: *const c_void, addr_len: u32) -> Option<SocketAddrV4> {
    if addr.is_null() || addr_len < core::mem::size_of::<TrueosSockAddrIn>() as u32 {
        return None;
    }
    let bytes = abi_read_bytes(addr.cast::<u8>(), core::mem::size_of::<TrueosSockAddrIn>())?;
    if bytes.get(1).copied()? as c_int != TRUEOS_AF_INET {
        return None;
    }
    Some(SocketAddrV4 {
        port: u16::from_be_bytes([bytes[2], bytes[3]]),
        addr: [bytes[4], bytes[5], bytes[6], bytes[7]],
    })
}

fn write_sockaddr_v4(addr: *mut c_void, addr_len: *mut u32, value: SocketAddrV4) -> bool {
    if addr.is_null() {
        return true;
    }
    let len = if addr_len.is_null() {
        core::mem::size_of::<TrueosSockAddrIn>() as u32
    } else {
        let Some(bytes) = abi_read_bytes(addr_len.cast::<u8>(), core::mem::size_of::<u32>()) else {
            return false;
        };
        u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    };
    if len < core::mem::size_of::<TrueosSockAddrIn>() as u32 {
        return false;
    }

    let mut out = [0u8; core::mem::size_of::<TrueosSockAddrIn>()];
    out[0] = core::mem::size_of::<TrueosSockAddrIn>() as u8;
    out[1] = TRUEOS_AF_INET as u8;
    out[2..4].copy_from_slice(&value.port.to_be_bytes());
    out[4..8].copy_from_slice(&value.addr);
    if !copy_to_abi_out(addr.cast::<u8>(), &out) {
        return false;
    }
    if !addr_len.is_null()
        && !copy_to_abi_out(
            addr_len.cast::<u8>(),
            &(core::mem::size_of::<TrueosSockAddrIn>() as u32).to_ne_bytes(),
        )
    {
        return false;
    }
    true
}

fn socket_v4_to_mio(value: SocketAddrV4) -> crate::mio_compat::TrueosMioSocketAddr {
    let mut addr = crate::mio_compat::TrueosMioSocketAddr {
        family: 4,
        port: value.port,
        addr: [0; 16],
    };
    addr.addr[..4].copy_from_slice(&value.addr);
    addr
}

fn socket_v4_from_mio(value: crate::mio_compat::TrueosMioSocketAddr) -> Option<SocketAddrV4> {
    if value.family != 4 {
        return None;
    }
    Some(SocketAddrV4 {
        addr: [value.addr[0], value.addr[1], value.addr[2], value.addr[3]],
        port: value.port,
    })
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
    if let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() {
        return 0x2_0000usize.saturating_add(vm_id as usize);
    }

    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        if let Some(thread_id) = crate::stackkeeper::current_blueprint_thread_id() {
            return 0x4_0000_0000usize
                .saturating_add((vm_id as usize) << 32)
                .saturating_add(thread_id as usize);
        }
        let worker_id = crate::stackkeeper::current_tokio_worker_id().unwrap_or(0);
        return 0x3_0000usize
            .saturating_add((vm_id as usize).saturating_mul(crate::stackkeeper::TOKIO_LANE_COUNT))
            .saturating_add(worker_id);
    }

    if crate::stackkeeper::tokio_blocking_backing_enabled()
        && let Some(worker_id) = crate::stackkeeper::current_tokio_worker_id()
    {
        return 0x1_0000usize.saturating_add(worker_id);
    }
    crate::percpu::current_slot().saturating_add(1)
}

fn pthread_tls_slot(key: usize) -> PthreadTlsSlot {
    PthreadTlsSlot {
        owner: pthread_current_id(),
        key,
    }
}

fn pthread_sync_probe_log() {
    if LOGGED_PTHREAD_SYNC
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log_os::log_with_area_purpose(
            crate::log_os::flags::LogArea::Blueprint,
            log::Level::Info,
            Some("pthread-realm"),
            format_args!("mutex/cond shim using inline cross-realm object state\n"),
        );
    }
}

fn pthread_sync_trace(op: &str, key: usize) {
    let seq = PTHREAD_SYNC_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    if seq < PTHREAD_SYNC_TRACE_LIMIT {
        crate::log_os::log_with_area_purpose(
            crate::log_os::flags::LogArea::Blueprint,
            log::Level::Info,
            Some("pthread-realm"),
            format_args!(
                "sync seq={} op={} key=0x{:x} owner={}\n",
                seq,
                op,
                key,
                pthread_current_id()
            ),
        );
    }
}

fn pthread_create_trace(thread_id: usize, rc: c_int) {
    let seq = PTHREAD_CREATE_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    if seq < PTHREAD_CREATE_TRACE_LIMIT {
        let origin = if (thread_id & PTHREAD_THREAD_CARRIER_TAG) != 0 {
            "carrier"
        } else {
            "hull"
        };
        crate::log_os::log_with_area_purpose(
            crate::log_os::flags::LogArea::Blueprint,
            log::Level::Info,
            Some("pthread-realm"),
            format_args!(
                "create seq={} thread={} origin={} local_seq={} rc={} owner={}\n",
                seq,
                thread_id,
                origin,
                thread_id & PTHREAD_THREAD_SEQUENCE_MASK,
                rc,
                pthread_current_id()
            ),
        );
    }
}

fn pthread_next_thread_id() -> usize {
    let sequence =
        PTHREAD_NEXT_THREAD_ID.fetch_add(1, Ordering::AcqRel) & PTHREAD_THREAD_SEQUENCE_MASK;
    let sequence = sequence.max(1);
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        sequence
    } else {
        sequence | PTHREAD_THREAD_CARRIER_TAG
    }
}

fn pthread_mutex_unlock_key(key: usize) -> c_int {
    pthread_sync_trace("mutex.unlock", key);
    let owner = pthread_current_id();
    let Some(state) = pthread_mutex_storage(key) else {
        return TRUEOS_EINVAL;
    };
    pthread_mutex_unlock_state(unsafe { state.as_ref() }, owner)
}

fn pthread_mutex_unlock_state(state: &PthreadMutexStorage, owner: usize) -> c_int {
    let held_by = state.owner.load(Ordering::Acquire);
    if held_by == 0 {
        return TRUEOS_EPERM;
    }
    if held_by != owner {
        return TRUEOS_EPERM;
    }
    let depth = state.depth.load(Ordering::Relaxed);
    if depth == 0 {
        return TRUEOS_EINVAL;
    }
    if state.kind.load(Ordering::Relaxed) == TRUEOS_PTHREAD_MUTEX_RECURSIVE && depth > 1 {
        state.depth.store(depth - 1, Ordering::Relaxed);
        return 0;
    }
    state.depth.store(0, Ordering::Relaxed);
    state.owner.store(0, Ordering::Release);
    0
}

fn pthread_mutex_lock_key(key: usize) -> c_int {
    pthread_sync_probe_log();
    pthread_sync_trace("mutex.lock", key);
    let owner = pthread_current_id();
    let Some(state) = pthread_mutex_storage(key) else {
        return TRUEOS_EINVAL;
    };
    pthread_mutex_lock_state(unsafe { state.as_ref() }, owner)
}

fn pthread_mutex_lock_state(state: &PthreadMutexStorage, owner: usize) -> c_int {
    loop {
        let held_by = state.owner.load(Ordering::Acquire);
        if held_by == owner {
            return match state.kind.load(Ordering::Relaxed) {
                TRUEOS_PTHREAD_MUTEX_RECURSIVE => {
                    let depth = state.depth.load(Ordering::Relaxed);
                    if depth == usize::MAX {
                        TRUEOS_EAGAIN
                    } else {
                        state.depth.store(depth + 1, Ordering::Relaxed);
                        0
                    }
                }
                TRUEOS_PTHREAD_MUTEX_ERRORCHECK => TRUEOS_EDEADLK,
                TRUEOS_PTHREAD_MUTEX_NORMAL => {
                    core::hint::spin_loop();
                    continue;
                }
                _ => TRUEOS_EINVAL,
            };
        }
        if held_by == 0
            && state
                .owner
                .compare_exchange(0, owner, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        {
            state.depth.store(1, Ordering::Relaxed);
            return 0;
        }
        core::hint::spin_loop();
    }
}

fn pthread_mutex_trylock_key(key: usize) -> c_int {
    pthread_sync_probe_log();
    pthread_sync_trace("mutex.trylock", key);
    let owner = pthread_current_id();
    let Some(state) = pthread_mutex_storage(key) else {
        return TRUEOS_EINVAL;
    };
    pthread_mutex_trylock_state(unsafe { state.as_ref() }, owner)
}

fn pthread_mutex_trylock_state(state: &PthreadMutexStorage, owner: usize) -> c_int {
    let held_by = state.owner.load(Ordering::Acquire);
    if held_by == owner && state.kind.load(Ordering::Relaxed) == TRUEOS_PTHREAD_MUTEX_RECURSIVE {
        let depth = state.depth.load(Ordering::Relaxed);
        if depth == usize::MAX {
            return TRUEOS_EAGAIN;
        }
        state.depth.store(depth + 1, Ordering::Relaxed);
        return 0;
    }
    if held_by != 0 {
        return TRUEOS_EBUSY;
    }
    match state
        .owner
        .compare_exchange(0, owner, Ordering::Acquire, Ordering::Relaxed)
    {
        Ok(_) => {
            state.depth.store(1, Ordering::Relaxed);
            0
        }
        Err(_) => TRUEOS_EBUSY,
    }
}

fn pthread_mutexattr_kind(attr: *const c_void) -> Option<c_int> {
    let host = pthread_object_host_ptr(attr.cast_mut().cast::<u8>(), PTHREAD_MUTEXATTR_ABI_BYTES)?;
    Some(unsafe { ptr::read_unaligned(host.cast::<c_int>()) })
}

fn pthread_mutexattr_set_kind(attr: *mut c_void, kind: c_int) -> bool {
    let Some(host) = pthread_object_host_ptr(attr.cast::<u8>(), PTHREAD_MUTEXATTR_ABI_BYTES) else {
        return false;
    };
    unsafe { ptr::write_unaligned(host.cast::<c_int>(), kind) };
    true
}

#[cfg(test)]
mod pthread_mutex_tests {
    use super::*;

    fn mutex_state(kind: c_int) -> PthreadMutexStorage {
        PthreadMutexStorage {
            owner: AtomicUsize::new(0),
            depth: AtomicUsize::new(0),
            kind: AtomicI32::new(kind),
        }
    }

    #[test]
    fn recursive_mutex_tracks_depth_until_final_unlock() {
        let state = mutex_state(TRUEOS_PTHREAD_MUTEX_RECURSIVE);
        assert_eq!(pthread_mutex_lock_state(&state, 7), 0);
        assert_eq!(pthread_mutex_lock_state(&state, 7), 0);
        assert_eq!(state.owner.load(Ordering::Relaxed), 7);
        assert_eq!(state.depth.load(Ordering::Relaxed), 2);

        assert_eq!(pthread_mutex_unlock_state(&state, 7), 0);
        assert_eq!(state.owner.load(Ordering::Relaxed), 7);
        assert_eq!(state.depth.load(Ordering::Relaxed), 1);
        assert_eq!(pthread_mutex_unlock_state(&state, 7), 0);
        assert_eq!(state.owner.load(Ordering::Relaxed), 0);
        assert_eq!(state.depth.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn recursive_trylock_by_owner_also_increments_depth() {
        let state = mutex_state(TRUEOS_PTHREAD_MUTEX_RECURSIVE);
        assert_eq!(pthread_mutex_trylock_state(&state, 9), 0);
        assert_eq!(pthread_mutex_trylock_state(&state, 9), 0);
        assert_eq!(state.depth.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn errorcheck_mutex_reports_same_owner_deadlock() {
        let state = mutex_state(TRUEOS_PTHREAD_MUTEX_ERRORCHECK);
        assert_eq!(pthread_mutex_lock_state(&state, 11), 0);
        assert_eq!(pthread_mutex_lock_state(&state, 11), TRUEOS_EDEADLK);
        assert_eq!(pthread_mutex_trylock_state(&state, 11), TRUEOS_EBUSY);
    }

    #[test]
    fn mutex_rejects_unlock_by_non_owner() {
        let state = mutex_state(TRUEOS_PTHREAD_MUTEX_RECURSIVE);
        assert_eq!(pthread_mutex_lock_state(&state, 13), 0);
        assert_eq!(pthread_mutex_unlock_state(&state, 17), TRUEOS_EPERM);
        assert_eq!(state.owner.load(Ordering::Relaxed), 13);
        assert_eq!(state.depth.load(Ordering::Relaxed), 1);
    }
}

fn pthread_mutex_storage(key: usize) -> Option<ptr::NonNull<PthreadMutexStorage>> {
    let host =
        pthread_object_host_ptr(key as *mut u8, core::mem::size_of::<PthreadMutexStorage>())?;
    if !(host as usize).is_multiple_of(core::mem::align_of::<PthreadMutexStorage>()) {
        return None;
    }
    ptr::NonNull::new(host.cast::<PthreadMutexStorage>())
}

fn pthread_cond_storage(key: usize) -> Option<ptr::NonNull<PthreadCondStorage>> {
    let host = pthread_object_host_ptr(key as *mut u8, core::mem::size_of::<PthreadCondStorage>())?;
    if !(host as usize).is_multiple_of(core::mem::align_of::<PthreadCondStorage>()) {
        return None;
    }
    ptr::NonNull::new(host.cast::<PthreadCondStorage>())
}

fn pthread_cond_generation(state: &PthreadCondStorage) -> u64 {
    state.generation.load(Ordering::Acquire)
}

fn pthread_cond_notify_key(key: usize) -> c_int {
    let Some(state) = pthread_cond_storage(key) else {
        return TRUEOS_EINVAL;
    };
    let state = unsafe { state.as_ref() };
    state.generation.fetch_add(1, Ordering::Release);
    0
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
        unsafe { crate::allocators::alloc_raw_hv_guest(vm_id, layout) }
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
    // Portal calls execute in the host, but allocations made for a blueprint are
    // charged to its VM-owned allocator. Report that same live domain here so a
    // guest can make bounded allocation decisions without knowing a static VM
    // RAM size. Non-VM callers continue to receive host allocator statistics.
    let stats = active_abi_alloc_guest_vm_id()
        .and_then(crate::allocators::hv_guest_heap_stats_if_configured)
        .unwrap_or_else(crate::allocators::heap_stats);
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
    crate::r::net::vlayer::resolve_ipv4_for_sync_abi(host).map_err(dns_resolve_error_to_eai)
}

fn dns_resolve_error_to_eai(err: crate::r::net::vlayer::DnsResolveError) -> c_int {
    match err {
        crate::r::net::vlayer::DnsResolveError::BadName
        | crate::r::net::vlayer::DnsResolveError::NoAnswer => TRUEOS_EAI_NONAME,
        crate::r::net::vlayer::DnsResolveError::Runtime
        | crate::r::net::vlayer::DnsResolveError::NoNic
        | crate::r::net::vlayer::DnsResolveError::Timeout => TRUEOS_EAI_SYSTEM,
    }
}

fn dns_resolve_error_to_cabi_errno(err: crate::r::net::vlayer::DnsResolveError) -> c_int {
    match err {
        crate::r::net::vlayer::DnsResolveError::BadName
        | crate::r::net::vlayer::DnsResolveError::NoAnswer => TRUEOS_EIO,
        crate::r::net::vlayer::DnsResolveError::Runtime
        | crate::r::net::vlayer::DnsResolveError::NoNic
        | crate::r::net::vlayer::DnsResolveError::Timeout => TRUEOS_ETIMEDOUT,
    }
}

fn active_abi_guest_vm_id() -> Option<u8> {
    crate::hv::current_guest_execution_context_vm_id()
        .or_else(crate::hv::current_vm_id_by_lapic_low)
}

fn active_abi_alloc_guest_vm_id() -> Option<u8> {
    crate::hv::current_hull_guest_context_vm_id()
        .or_else(crate::r::kernel_task_domain::guest_owned_alloc_vm_id)
        .or_else(crate::hv::current_vm_id_by_lapic_low)
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
    let (heap_start, heap_end) = crate::allocators::hv_guest_heap_bounds(vm_id)?;
    let end = guest_va.checked_add(len)?;
    if guest_va >= heap_start && end <= heap_end {
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

// Synchronization objects must never use abi_host_ptr's cross-VM recovery
// fallback. A mutex address is meaningful only in the currently executing
// realm: Hull stack pointers translate through that VM's stack backing,
// guest-heap pointers are already shared HHDM addresses, and other high
// addresses belong to the current Hull/host mapping. Guessing another VM for
// a lock would merge two unrelated objects.
fn pthread_object_host_ptr(ptr: *mut u8, len: usize) -> Option<*mut u8> {
    if ptr.is_null() {
        return None;
    }
    if len == 0 {
        return Some(ptr);
    }
    let Some(vm_id) = active_abi_guest_vm_id() else {
        return (!looks_like_low_guest_ptr(ptr)).then_some(ptr);
    };
    active_guest_stack_host_ptr_for_vm(vm_id, ptr, len)
        .or_else(|| active_guest_heap_host_ptr_for_vm(vm_id, ptr, len))
        .or_else(|| {
            (crate::hv::current_hull_guest_context_vm_id() != Some(vm_id))
                .then(|| {
                    crate::hv::memory::guest_hull_rw_host_ptr_for_vm(
                        vm_id,
                        ptr as usize as u64,
                        len,
                    )
                })
                .flatten()
        })
        .or_else(|| {
            if looks_like_low_guest_ptr(ptr) {
                return None;
            }
            if crate::hv::current_hull_guest_context_vm_id() != Some(vm_id) {
                let address = ptr as usize as u64;
                let end = address.checked_add(len as u64)?;
                let (image_start, image_end) = crate::hv::guest::hull_image_bounds();
                if address < image_end && end > image_start {
                    // A carrier must never fall through from a failed VM Hull
                    // translation to the host mapping at the same high VA.
                    return None;
                }
            }
            Some(ptr)
        })
}

pub(crate) fn abi_read_bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    let host = abi_host_ptr(ptr.cast_mut(), len)?;
    Some(unsafe { slice::from_raw_parts(host.cast::<u8>(), len) })
}

pub(crate) fn abi_write_bytes<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
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

pub(crate) fn copy_to_abi_out(ptr: *mut u8, bytes: &[u8]) -> bool {
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

fn block_error_to_errno(err: crate::disc::block::Error) -> c_int {
    match err {
        crate::disc::block::Error::InvalidParam | crate::disc::block::Error::OutOfBounds => {
            TRUEOS_EINVAL
        }
        crate::disc::block::Error::NotSupported => TRUEOS_ENOSYS,
        crate::disc::block::Error::NotReady
        | crate::disc::block::Error::DmaUnavailable
        | crate::disc::block::Error::MmioMapFailed
        | crate::disc::block::Error::Timeout
        | crate::disc::block::Error::Io
        | crate::disc::block::Error::Corrupted => TRUEOS_EIO,
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

fn write_file_to_cabi(path: &str, bytes: &[u8]) -> Result<(), c_int> {
    let mut handle = 0u32;
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_fs_write_begin(
            path.as_ptr(),
            path.len(),
            bytes.len() as u64,
            &mut handle as *mut u32,
        )
    };
    if rc != 0 {
        return Err(fs_rc_to_errno(rc));
    }

    if !bytes.is_empty() {
        let rc = unsafe {
            crate::r::io::cabi::trueos_cabi_fs_write_chunk(handle, bytes.as_ptr(), bytes.len())
        };
        if rc != 0 {
            let _ = unsafe { crate::r::io::cabi::trueos_cabi_fs_write_abort(handle) };
            return Err(fs_rc_to_errno(rc));
        }
    }

    let rc = unsafe { crate::r::io::cabi::trueos_cabi_fs_write_finish(handle) };
    if rc != 0 {
        return Err(fs_rc_to_errno(rc));
    }
    Ok(())
}

async fn write_file_to_trueosfs_async(path: &str, bytes: &[u8]) -> Result<(), c_int> {
    crate::log!("std-abi-shim: async-write stage=resolve path={} bytes={}\n", path, bytes.len());
    let Some(path) = crate::r::io::env::resolve_fs_path(path, false) else {
        crate::log!("std-abi-shim: async-write failed stage=resolve errno={}\n", TRUEOS_EINVAL);
        return Err(TRUEOS_EINVAL);
    };
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!(
            "std-abi-shim: async-write failed stage=root path={} errno={}\n",
            path.as_str(),
            TRUEOS_ENOENT
        );
        return Err(TRUEOS_ENOENT);
    };
    crate::log!(
        "std-abi-shim: async-write stage=begin disk={} path={} bytes={}\n",
        disk.id().raw(),
        path.as_str(),
        bytes.len()
    );

    let mut begin_attempt = 0usize;
    let handle = loop {
        let begin = match embassy_time::with_timeout(
            embassy_time::Duration::from_millis(TRUEOS_ASYNC_WRITE_TIMEOUT_MS),
            crate::r::fs::trueosfs::file_write_begin_async(disk, path.as_str(), bytes.len() as u64),
        )
        .await
        {
            Ok(begin) => begin,
            Err(_) => {
                crate::log!(
                    "std-abi-shim: async-write failed stage=begin errno={} reason=timeout\n",
                    TRUEOS_EIO
                );
                return Err(TRUEOS_EIO);
            }
        };
        match begin {
            Ok(Some(handle)) => break handle,
            Ok(None) => return Err(TRUEOS_EIO),
            Err(crate::disc::block::Error::NotReady)
                if begin_attempt < TRUEOS_ASYNC_WRITE_BEGIN_RETRIES =>
            {
                begin_attempt = begin_attempt.saturating_add(1);
                if begin_attempt == 1 || begin_attempt % 10 == 0 {
                    crate::log!(
                        "std-abi-shim: async-write retry stage=begin attempt={} err=NotReady\n",
                        begin_attempt
                    );
                }
                embassy_time::Timer::after(embassy_time::Duration::from_millis(25)).await;
            }
            Err(err) => return Err(block_error_to_errno(err)),
        }
    };
    crate::log!(
        "std-abi-shim: async-write success stage=begin handle={} attempts={}\n",
        handle,
        begin_attempt.saturating_add(1)
    );

    let mut offset = 0usize;
    while offset < bytes.len() {
        let end = core::cmp::min(offset.saturating_add(64 * 1024), bytes.len());
        crate::log!(
            "std-abi-shim: async-write stage=chunk handle={} offset={} len={}\n",
            handle,
            offset,
            end - offset
        );
        let chunk = embassy_time::with_timeout(
            embassy_time::Duration::from_millis(TRUEOS_ASYNC_WRITE_TIMEOUT_MS),
            crate::r::fs::trueosfs::file_write_chunk_async(handle, &bytes[offset..end]),
        )
        .await;
        match chunk {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
                return Err(block_error_to_errno(err));
            }
            Err(_) => {
                let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
                crate::log!(
                    "std-abi-shim: async-write failed stage=chunk handle={} errno={}\n",
                    handle,
                    TRUEOS_EIO
                );
                return Err(TRUEOS_EIO);
            }
        }
        offset = end;
    }

    crate::log!("std-abi-shim: async-write stage=finish handle={}\n", handle);
    let finish = embassy_time::with_timeout(
        embassy_time::Duration::from_millis(TRUEOS_ASYNC_WRITE_TIMEOUT_MS),
        crate::r::fs::trueosfs::file_write_finish_async(handle),
    )
    .await;
    match finish {
        Ok(Ok(())) => {
            crate::log!("std-abi-shim: async-write success stage=finish handle={}\n", handle);
            Ok(())
        }
        Ok(Err(err)) => Err(block_error_to_errno(err)),
        Err(_) => {
            crate::log!(
                "std-abi-shim: async-write failed stage=finish handle={} errno={}\n",
                handle,
                TRUEOS_EIO
            );
            Err(TRUEOS_EIO)
        }
    }
}

pub(crate) fn next_file_fd() -> c_int {
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
        unsafe { crate::allocators::alloc_raw_hv_guest(vm_id, layout) }
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
    write_platform_fd(2, bytes);
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
        let byte = crate::r::io::fs_cabi::trueos_cabi_shell_attached_read_byte();
        if byte < 0 {
            break;
        }
        out[read] = byte as u8;
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
        write_platform_fd(2, bytes);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_cycle_count() -> usize {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_time_monotonic_nanos() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_monotonic_nanos();
    }

    crate::chronos::monotonic_nanos()
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_time_unix_seconds() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_unix_seconds();
    }

    crate::chronos::best_effort_unix_time_seconds().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_time_unix_nanos() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_unix_seconds().saturating_mul(1_000_000_000);
    }

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
            write_platform_fd(2, b"std-trueos panic: ");
            write_platform_fd(2, bytes);
            write_platform_fd(2, b"\n");
        }
    }
    unsafe { sys_halt() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_halt() -> ! {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        // A Rust Blueprint binary terminates through the `exit`/`_exit` ABI, so its
        // entry point never returns to the guest runner's post-invoke cleanup.
        // Release the borrowed terminal before the final VMCALL stops the VM;
        // otherwise shell2 is merely revealed with its input still attached
        // to a guest that is spinning here.
        let _ = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_RETURN_TO_CLI,
            0,
            0,
        );
        trueos_vm::vmcall::preserve();
    }
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn exit(code: c_int) -> ! {
    let _ = code;
    write_platform_fd(2, b"std-abi: exit\n");
    unsafe { sys_halt() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stack_chk_fail() -> ! {
    write_platform_fd(2, b"std-abi: stack check failed\n");
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
pub unsafe extern "C" fn __memcpy_chk(
    dest: *mut c_void,
    src: *const c_void,
    len: usize,
    _dest_len: usize,
) -> *mut c_void {
    unsafe { ptr::copy_nonoverlapping(src.cast::<u8>(), dest.cast::<u8>(), len) };
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __memmove_chk(
    dest: *mut c_void,
    src: *const c_void,
    len: usize,
    _dest_len: usize,
) -> *mut c_void {
    unsafe { ptr::copy(src.cast::<u8>(), dest.cast::<u8>(), len) };
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __memset_chk(
    dest: *mut c_void,
    value: c_int,
    len: usize,
    _dest_len: usize,
) -> *mut c_void {
    unsafe { ptr::write_bytes(dest.cast::<u8>(), value as u8, len) };
    dest
}

pub unsafe extern "C" fn memchr(ptr: *const c_void, value: c_int, len: usize) -> *mut c_void {
    if ptr.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { slice::from_raw_parts(ptr.cast::<u8>(), len) };
    let needle = value as u8;
    bytes
        .iter()
        .position(|byte| *byte == needle)
        .map(|offset| unsafe { ptr.cast::<u8>().add(offset).cast_mut().cast::<c_void>() })
        .unwrap_or(ptr::null_mut())
}

pub unsafe extern "C" fn strcmp(left: *const c_char, right: *const c_char) -> c_int {
    unsafe { strncmp(left, right, usize::MAX) }
}

pub unsafe extern "C" fn strncmp(left: *const c_char, right: *const c_char, max: usize) -> c_int {
    if left.is_null() || right.is_null() {
        return if left == right { 0 } else { -1 };
    }
    let mut index = 0usize;
    while index < max {
        let a = unsafe { *left.cast::<u8>().add(index) };
        let b = unsafe { *right.cast::<u8>().add(index) };
        if a != b || a == 0 || b == 0 {
            return a as c_int - b as c_int;
        }
        index = index.saturating_add(1);
    }
    0
}

pub unsafe extern "C" fn strchr(text: *const c_char, needle: c_int) -> *mut c_char {
    if text.is_null() {
        return ptr::null_mut();
    }
    let needle = needle as u8;
    let mut index = 0usize;
    loop {
        let byte = unsafe { *text.cast::<u8>().add(index) };
        if byte == needle {
            return unsafe { text.add(index).cast_mut() };
        }
        if byte == 0 {
            return ptr::null_mut();
        }
        index = index.saturating_add(1);
    }
}

pub unsafe extern "C" fn strrchr(text: *const c_char, needle: c_int) -> *mut c_char {
    if text.is_null() {
        return ptr::null_mut();
    }
    let needle = needle as u8;
    let mut last = ptr::null_mut();
    let mut index = 0usize;
    loop {
        let byte = unsafe { *text.cast::<u8>().add(index) };
        if byte == needle {
            last = unsafe { text.add(index).cast_mut() };
        }
        if byte == 0 {
            return last;
        }
        index = index.saturating_add(1);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strspn(text: *const c_char, accept: *const c_char) -> usize {
    unsafe { str_span(text, accept, true) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcspn(text: *const c_char, reject: *const c_char) -> usize {
    unsafe { str_span(text, reject, false) }
}

unsafe fn str_span(text: *const c_char, set: *const c_char, accept_mode: bool) -> usize {
    if text.is_null() || set.is_null() {
        return 0;
    }
    let mut len = 0usize;
    loop {
        let byte = unsafe { *text.cast::<u8>().add(len) };
        if byte == 0 {
            return len;
        }
        let contains = unsafe { cstr_contains_byte(set, byte) };
        if contains != accept_mode {
            return len;
        }
        len = len.saturating_add(1);
    }
}

unsafe fn cstr_contains_byte(text: *const c_char, needle: u8) -> bool {
    let mut index = 0usize;
    loop {
        let byte = unsafe { *text.cast::<u8>().add(index) };
        if byte == 0 {
            return false;
        }
        if byte == needle {
            return true;
        }
        index = index.saturating_add(1);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn log(value: c_double) -> c_double {
    libm::log(value)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qsort(
    base: *mut c_void,
    count: usize,
    size: usize,
    compar: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) {
    if base.is_null() || size == 0 || count < 2 {
        return;
    }
    let Some(compare) = compar else {
        return;
    };
    let mut scratch = Vec::new();
    scratch.resize(size, 0);
    let base = base.cast::<u8>();
    for _ in 0..count {
        let mut swapped = false;
        for index in 1..count {
            let left = unsafe { base.add((index - 1).saturating_mul(size)) };
            let right = unsafe { base.add(index.saturating_mul(size)) };
            if unsafe { compare(left.cast::<c_void>(), right.cast::<c_void>()) } > 0 {
                unsafe {
                    ptr::copy_nonoverlapping(left, scratch.as_mut_ptr(), size);
                    ptr::copy(right, left, size);
                    ptr::copy_nonoverlapping(scratch.as_ptr(), right, size);
                }
                swapped = true;
            }
        }
        if !swapped {
            break;
        }
    }
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
    let cwd = crate::r::io::env::current_app_fs_root()
        .map(|root| alloc::format!("/{}", root.trim_matches('/')))
        .unwrap_or_else(|| String::from("/"));
    let cwd = cwd.as_bytes();
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
pub unsafe extern "C" fn getpid() -> c_int {
    crate::hv::current_hull_guest_context_vm_id()
        .map(|vm_id| 1000 + vm_id as c_int)
        .unwrap_or(1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn geteuid() -> u32 {
    0
}

pub unsafe extern "C" fn time(out: *mut i64) -> i64 {
    let now = trueos_time_unix_seconds().min(i64::MAX as u64) as i64;
    if !out.is_null() {
        let _ = copy_to_abi_out(out.cast::<u8>(), &now.to_ne_bytes());
    }
    now
}

pub unsafe extern "C" fn gettimeofday(tv: *mut c_void, _tz: *mut c_void) -> c_int {
    if tv.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let now = trueos_time_unix_seconds().min(i64::MAX as u64) as i64;
    let out = TrueosTimeval {
        tv_sec: now,
        tv_usec: 0,
    };
    if !copy_to_abi_out(tv.cast::<u8>(), unsafe {
        slice::from_raw_parts(
            (&out as *const TrueosTimeval).cast::<u8>(),
            core::mem::size_of::<TrueosTimeval>(),
        )
    }) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

pub unsafe extern "C" fn localtime_r(timep: *const i64, result: *mut c_void) -> *mut c_void {
    if timep.is_null() || result.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return ptr::null_mut();
    }
    let Some(seconds) = abi_read_struct(timep) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return ptr::null_mut();
    };
    let tm = unix_seconds_to_utc_tm(seconds);
    if !copy_to_abi_out(result.cast::<u8>(), unsafe {
        slice::from_raw_parts(
            (&tm as *const TrueosTm).cast::<u8>(),
            core::mem::size_of::<TrueosTm>(),
        )
    }) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return ptr::null_mut();
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    result
}

fn unix_seconds_to_utc_tm(seconds: i64) -> TrueosTm {
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let yday = day_of_year(year, month, day);
    TrueosTm {
        tm_sec: (seconds_of_day % 60) as c_int,
        tm_min: ((seconds_of_day / 60) % 60) as c_int,
        tm_hour: (seconds_of_day / 3600) as c_int,
        tm_mday: day as c_int,
        tm_mon: month as c_int - 1,
        tm_year: year as c_int - 1900,
        tm_wday: ((days + 4).rem_euclid(7)) as c_int,
        tm_yday: yday as c_int,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: TRUEOS_UTC_TZ.as_ptr().cast::<c_char>(),
    }
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let d = doy - (153 * mp + 2).div_euclid(5) + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

fn day_of_year(year: i64, month: i64, day: i64) -> i64 {
    const CUMULATIVE: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut yday = CUMULATIVE[(month - 1).clamp(0, 11) as usize] + day - 1;
    if month > 2 && is_leap_year(year) {
        yday += 1;
    }
    yday
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nanosleep(_req: *const c_void, _rem: *mut c_void) -> c_int {
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn write(fd: c_int, buf: *const c_void, count: usize) -> isize {
    if count != 0 && buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if fd == 1 || fd == 2 {
        unsafe { sys_write(fd as u32, buf.cast::<u8>(), count) };
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        return count as isize;
    }
    if fd < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let Some(input) = abi_read_bytes(buf.cast::<u8>(), count) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };

    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    match file {
        OpenFile::Regular {
            bytes,
            offset,
            writable,
            dirty,
            ..
        } => {
            if !*writable {
                TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
                return -1;
            }
            let end = offset.saturating_add(input.len());
            if bytes.len() < end {
                bytes.resize(end, 0);
            }
            bytes[*offset..end].copy_from_slice(input);
            *offset = end;
            *dirty = true;
        }
        OpenFile::PipeWrite { pipe, .. } => {
            let mut pipe = pipe.lock();
            if !pipe.write_open || !pipe.read_open {
                TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
                return -1;
            }
            pipe.bytes.extend_from_slice(input);
        }
        OpenFile::UnixSocket { tx, .. } => {
            let mut tx = tx.lock();
            if !tx.write_open || !tx.read_open {
                TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
                return -1;
            }
            tx.bytes.extend_from_slice(input);
        }
        OpenFile::PipeRead { .. } => {
            TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
            return -1;
        }
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    input.len() as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize {
    if buf.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if fd == 0 {
        let n = unsafe { sys_read(fd as u32, buf.cast::<u8>(), count) };
        if n == 0 && STD_FD_FLAGS[0].load(Ordering::Relaxed) & TRUEOS_O_NONBLOCK != 0 {
            TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
            return -1;
        }
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        return n as isize;
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
    let copied = match file {
        OpenFile::Regular {
            bytes,
            offset,
            readable,
            ..
        } => {
            if !*readable {
                TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
                return -1;
            }
            let remaining = bytes.len().saturating_sub(*offset);
            let n = core::cmp::min(count, remaining);
            if n != 0 && !copy_to_abi_out(buf.cast::<u8>(), &bytes[*offset..*offset + n]) {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            *offset = offset.saturating_add(n);
            n
        }
        OpenFile::PipeRead { pipe, flags } => {
            let mut pipe = pipe.lock();
            let n = core::cmp::min(count, pipe.bytes.len());
            if n == 0 && *flags & TRUEOS_O_NONBLOCK != 0 && pipe.write_open {
                TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
                return -1;
            }
            if n != 0 && !copy_to_abi_out(buf.cast::<u8>(), &pipe.bytes[..n]) {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            pipe.bytes.drain(..n);
            n
        }
        OpenFile::UnixSocket { rx, flags, .. } => {
            let mut rx = rx.lock();
            let n = core::cmp::min(count, rx.bytes.len());
            if n == 0 && *flags & TRUEOS_O_NONBLOCK != 0 && rx.write_open {
                TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
                return -1;
            }
            if n != 0 && !copy_to_abi_out(buf.cast::<u8>(), &rx.bytes[..n]) {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            rx.bytes.drain(..n);
            n
        }
        OpenFile::PipeWrite { .. } => {
            TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
            return -1;
        }
    };
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    copied as isize
}

pub(crate) fn open_file_read_ready(file: &OpenFile) -> bool {
    match file {
        OpenFile::Regular { bytes, offset, .. } => *offset < bytes.len(),
        OpenFile::PipeRead { pipe, .. } => !pipe.lock().bytes.is_empty(),
        OpenFile::PipeWrite { .. } => false,
        OpenFile::UnixSocket { rx, .. } => !rx.lock().bytes.is_empty(),
    }
}

pub(crate) fn open_file_write_ready(file: &OpenFile) -> bool {
    match file {
        OpenFile::Regular { writable, .. } => *writable,
        OpenFile::PipeRead { .. } => false,
        OpenFile::PipeWrite { pipe, .. } => {
            let pipe = pipe.lock();
            pipe.read_open && pipe.write_open
        }
        OpenFile::UnixSocket { tx, .. } => {
            let tx = tx.lock();
            tx.read_open && tx.write_open
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn socket(domain: c_int, socket_type: c_int, protocol: c_int) -> c_int {
    let fd = posix_rc_i32(crate::r::net::socket_cabi::trueos_cabi_socket_tcp_open(
        domain,
        socket_type,
        protocol,
    ));
    if fd < 0 {
        return fd;
    }
    if SOCKET_FDS
        .lock()
        .insert(
            fd,
            SocketFd::PendingListener {
                backend: fd as u32,
                local: None,
            },
        )
        .is_err()
    {
        let _ = crate::r::net::socket_cabi::trueos_cabi_socket_tcp_close(fd as u32);
        TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
        return -1;
    }
    fd
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

    if matches!(
        SOCKET_FDS.lock().get(socket_id),
        Some(SocketFd::MioListener { .. } | SocketFd::MioStream { .. })
    ) {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        return 0;
    }

    let rc =
        crate::r::net::socket_cabi::trueos_cabi_socket_tcp_set_nonblocking(socket_id as u32, 0);
    posix_rc_i32(if rc < 0 { rc } else { 0 })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bind(socket_id: c_int, addr: *const c_void, addr_len: u32) -> c_int {
    if socket_id < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let Some(local) = parse_sockaddr_v4(addr, addr_len) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let mut sockets = SOCKET_FDS.lock();
    let Some(socket) = sockets.get_mut(socket_id) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    match socket {
        SocketFd::PendingListener { local: slot, .. } => {
            *slot = Some(local);
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            0
        }
        SocketFd::MioListener { local: slot, .. } => {
            *slot = local;
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            0
        }
        SocketFd::Cabi { .. } | SocketFd::MioStream { .. } => {
            TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn listen(socket_id: c_int, _backlog: c_int) -> c_int {
    if socket_id < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let local = {
        let sockets = SOCKET_FDS.lock();
        let Some(socket) = sockets.get(socket_id) else {
            TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
            return -1;
        };
        match socket {
            SocketFd::MioListener { .. } => {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                return 0;
            }
            SocketFd::PendingListener {
                local: Some(local), ..
            } => *local,
            SocketFd::PendingListener { local: None, .. } => SocketAddrV4 {
                addr: [0, 0, 0, 0],
                port: 0,
            },
            SocketFd::Cabi { .. } | SocketFd::MioStream { .. } => {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
        }
    };

    let mut backend = 0u32;
    let rc = unsafe {
        crate::mio_compat::trueos_mio_tcp_listener_bind(
            socket_v4_to_mio(local),
            &mut backend as *mut u32,
        )
    };
    if rc != 0 {
        TRUEOS_ERRNO.store(
            if rc == -6 {
                TRUEOS_EADDRINUSE
            } else {
                mio_status_to_errno(rc)
            },
            Ordering::Relaxed,
        );
        return -1;
    }
    let mut sockets = SOCKET_FDS.lock();
    let _ = sockets.insert(socket_id, SocketFd::MioListener { backend, local });
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn accept(socket_id: c_int, addr: *mut c_void, addr_len: *mut u32) -> c_int {
    unsafe { accept4(socket_id, addr, addr_len, 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn accept4(
    socket_id: c_int,
    addr: *mut c_void,
    addr_len: *mut u32,
    _flags: c_int,
) -> c_int {
    if socket_id < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let backend = {
        let sockets = SOCKET_FDS.lock();
        let Some(SocketFd::MioListener { backend, .. }) = sockets.get(socket_id) else {
            TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
            return -1;
        };
        *backend
    };

    let mut child = 0u32;
    let mut peer = crate::mio_compat::TrueosMioSocketAddr::default();
    let rc = unsafe {
        crate::mio_compat::trueos_mio_tcp_listener_accept(
            backend,
            &mut child as *mut u32,
            &mut peer as *mut crate::mio_compat::TrueosMioSocketAddr,
        )
    };
    if rc != 0 {
        return posix_mio_i32(rc);
    }
    if let Some(peer) = socket_v4_from_mio(peer)
        && !write_sockaddr_v4(addr, addr_len, peer)
    {
        let _ = unsafe { crate::mio_compat::trueos_mio_socket_close(child) };
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if SOCKET_FDS
        .lock()
        .insert(child as c_int, SocketFd::MioStream { backend: child })
        .is_err()
    {
        let _ = unsafe { crate::mio_compat::trueos_mio_socket_close(child) };
        TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    child as c_int
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

    if let Some(SocketFd::MioStream { backend }) = SOCKET_FDS.lock().get(socket_id) {
        return posix_mio_isize(unsafe {
            crate::mio_compat::trueos_mio_tcp_stream_write(*backend, buf.cast::<u8>(), len)
        });
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

    if let Some(SocketFd::MioStream { backend }) = SOCKET_FDS.lock().get(socket_id) {
        return posix_mio_isize(unsafe {
            crate::mio_compat::trueos_mio_tcp_stream_read(*backend, buf.cast::<u8>(), len)
        });
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
            let n = unsafe { write(fd, entry.base.cast::<c_void>(), entry.len) };
            if n < 0 {
                return if written == 0 { -1 } else { written as isize };
            }
            written = written.saturating_add(n as usize);
            if n as usize != entry.len {
                break;
            }
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
    let blocks = core::cmp::min(len.saturating_add(511) / 512, i64::MAX as u64) as i64;
    let out = TrueosStat {
        st_dev: 1,
        st_ino: 1,
        st_nlink: if kind == 2 { 2 } else { 1 },
        st_mode: mode,
        st_uid: 0,
        st_gid: 0,
        __pad0: 0,
        st_rdev: 0,
        st_size: core::cmp::min(len, i64::MAX as u64) as i64,
        st_blksize: 1024,
        st_blocks: blocks,
        st_atime: 0,
        st_atime_nsec: 0,
        st_mtime: 0,
        st_mtime_nsec: 0,
        st_ctime: 0,
        st_ctime_nsec: 0,
        __unused: [0; 3],
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
pub unsafe extern "C" fn stat64(path: *const c_char, buf: *mut c_void) -> c_int {
    unsafe { stat(path, buf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lstat64(path: *const c_char, buf: *mut c_void) -> c_int {
    unsafe { stat(path, buf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn open(path: *const c_char, flags: c_int, _mode: c_int) -> c_int {
    if path.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let Some(path) = abi_cstr_to_string(path, 4096) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };

    let access = flags & TRUEOS_O_ACCMODE;
    let readable = access == TRUEOS_O_RDONLY || access == TRUEOS_O_RDWR;
    let writable = access == TRUEOS_O_WRONLY || access == TRUEOS_O_RDWR;
    let should_truncate = flags & TRUEOS_O_TRUNC != 0;
    let should_create = flags & TRUEOS_O_CREAT != 0;

    let file = match access {
        TRUEOS_O_RDONLY | TRUEOS_O_WRONLY | TRUEOS_O_RDWR => {
            let mut dirty = false;
            let bytes = if should_truncate && writable && should_create {
                dirty = true;
                Vec::new()
            } else {
                let mut bytes = match read_file_from_cabi(path.as_str()) {
                    Ok(bytes) => bytes,
                    Err(TRUEOS_ENOENT) if should_create => {
                        dirty = writable;
                        Vec::new()
                    }
                    Err(errno) => {
                        TRUEOS_ERRNO.store(errno, Ordering::Relaxed);
                        return -1;
                    }
                };
                if should_truncate {
                    bytes.clear();
                    dirty |= writable;
                }
                bytes
            };
            OpenFile::Regular {
                path: writable.then_some(path),
                bytes,
                offset: 0,
                readable,
                writable,
                dirty,
                flags,
            }
        }
        _ => {
            TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
            return -1;
        }
    };
    let fd = next_file_fd();
    if OPEN_FILES.lock().insert(fd, file).is_err() {
        TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    fd
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn open64(path: *const c_char, flags: c_int, mode: c_int) -> c_int {
    unsafe { open(path, flags, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn close(fd: c_int) -> c_int {
    if (0..=2).contains(&fd) {
        return 0;
    }
    let _ = FD_FLAGS.lock().remove(fd);
    if let Some(socket) = SOCKET_FDS.lock().remove(fd) {
        let rc = match socket {
            SocketFd::Cabi { backend } | SocketFd::PendingListener { backend, .. } => {
                crate::r::net::socket_cabi::trueos_cabi_socket_tcp_close(backend)
            }
            SocketFd::MioListener { backend, .. } | SocketFd::MioStream { backend } => unsafe {
                crate::mio_compat::trueos_mio_socket_close(backend)
            },
        };
        return if rc < 0 {
            TRUEOS_ERRNO.store(rc.saturating_neg(), Ordering::Relaxed);
            -1
        } else {
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            0
        };
    }
    let Some(file) = OPEN_FILES.lock().remove(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    match file {
        OpenFile::Regular {
            path,
            bytes,
            writable,
            dirty,
            ..
        } => {
            if writable
                && dirty
                && let Some(path) = path
            {
                if let Err(errno) = write_file_to_cabi(path.as_str(), bytes.as_slice()) {
                    TRUEOS_ERRNO.store(errno, Ordering::Relaxed);
                    return -1;
                }
            }
        }
        OpenFile::PipeRead { pipe, .. } => {
            pipe.lock().read_open = false;
        }
        OpenFile::PipeWrite { pipe, .. } => {
            pipe.lock().write_open = false;
        }
        OpenFile::UnixSocket { rx, tx, .. } => {
            rx.lock().read_open = false;
            tx.lock().write_open = false;
        }
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

pub async fn close_async(fd: c_int) -> c_int {
    if (0..=2).contains(&fd) {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        return 0;
    }
    if SOCKET_FDS.lock().get(fd).is_some() {
        return unsafe { close(fd) };
    }
    let Some(file) = OPEN_FILES.lock().remove(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    match file {
        OpenFile::Regular {
            path,
            bytes,
            writable,
            dirty,
            ..
        } => {
            if writable
                && dirty
                && let Some(path) = path
                && let Err(errno) =
                    write_file_to_trueosfs_async(path.as_str(), bytes.as_slice()).await
            {
                TRUEOS_ERRNO.store(errno, Ordering::Relaxed);
                return -1;
            }
        }
        OpenFile::PipeRead { pipe, .. } => {
            pipe.lock().read_open = false;
        }
        OpenFile::PipeWrite { pipe, .. } => {
            pipe.lock().write_open = false;
        }
        OpenFile::UnixSocket { rx, tx, .. } => {
            rx.lock().read_open = false;
            tx.lock().write_open = false;
        }
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int {
    if fd < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    if (0..=2).contains(&fd) {
        let flags = &STD_FD_FLAGS[fd as usize];
        return match cmd {
            TRUEOS_F_GETFD => {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                0
            }
            TRUEOS_F_SETFD => {
                let _ = arg & TRUEOS_FD_CLOEXEC;
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                0
            }
            TRUEOS_F_GETFL => {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                flags.load(Ordering::Relaxed)
            }
            TRUEOS_F_SETFL => {
                flags.store(arg, Ordering::Relaxed);
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                0
            }
            _ => {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                -1
            }
        };
    }

    if SOCKET_FDS.lock().get(fd).is_some() {
        return match cmd {
            TRUEOS_F_GETFD => {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                FD_FLAGS.lock().get(fd).copied().unwrap_or(0)
            }
            TRUEOS_F_SETFD => {
                let mut fd_flags = FD_FLAGS.lock();
                let next = arg & TRUEOS_FD_CLOEXEC;
                if next == 0 {
                    let _ = fd_flags.remove(fd);
                } else if fd_flags.insert(fd, next).is_err() {
                    TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
                    return -1;
                }
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                0
            }
            TRUEOS_F_GETFL => {
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                0
            }
            TRUEOS_F_SETFL => {
                let _ = arg & TRUEOS_O_NONBLOCK;
                TRUEOS_ERRNO.store(0, Ordering::Relaxed);
                0
            }
            _ => {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                -1
            }
        };
    }

    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    match cmd {
        TRUEOS_F_GETFD => {
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            FD_FLAGS.lock().get(fd).copied().unwrap_or(0)
        }
        TRUEOS_F_SETFD => {
            let mut fd_flags = FD_FLAGS.lock();
            let next = arg & TRUEOS_FD_CLOEXEC;
            if next == 0 {
                let _ = fd_flags.remove(fd);
            } else if fd_flags.insert(fd, next).is_err() {
                TRUEOS_ERRNO.store(TRUEOS_EAGAIN, Ordering::Relaxed);
                return -1;
            }
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            0
        }
        TRUEOS_F_GETFL => {
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            file.flags()
        }
        TRUEOS_F_SETFL => {
            file.set_flags(arg);
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            0
        }
        _ => {
            TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fcntl64(fd: c_int, cmd: c_int, arg: c_int) -> c_int {
    unsafe { fcntl(fd, cmd, arg) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getsockname(
    socket_id: c_int,
    addr: *mut c_void,
    addr_len: *mut u32,
) -> c_int {
    if socket_id < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let local = {
        let sockets = SOCKET_FDS.lock();
        let Some(socket) = sockets.get(socket_id) else {
            TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
            return -1;
        };
        match socket {
            SocketFd::MioListener { local, .. } => Some(*local),
            SocketFd::PendingListener { local, .. } => *local,
            SocketFd::MioStream { backend } => {
                let mut mio_addr = crate::mio_compat::TrueosMioSocketAddr::default();
                let rc = unsafe {
                    crate::mio_compat::trueos_mio_socket_local_addr(*backend, &mut mio_addr)
                };
                if rc == 0 {
                    socket_v4_from_mio(mio_addr)
                } else {
                    None
                }
            }
            SocketFd::Cabi { .. } => None,
        }
    }
    .unwrap_or(SocketAddrV4 {
        addr: [0, 0, 0, 0],
        port: 0,
    });

    if !write_sockaddr_v4(addr, addr_len, local) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpeername(
    socket_id: c_int,
    addr: *mut c_void,
    addr_len: *mut u32,
) -> c_int {
    if socket_id < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let peer = {
        let sockets = SOCKET_FDS.lock();
        let Some(socket) = sockets.get(socket_id) else {
            TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
            return -1;
        };
        let backend = socket.backend();
        let mut mio_addr = crate::mio_compat::TrueosMioSocketAddr::default();
        let rc = unsafe { crate::mio_compat::trueos_mio_socket_peer_addr(backend, &mut mio_addr) };
        if rc == 0 {
            socket_v4_from_mio(mio_addr)
        } else {
            None
        }
    };
    let Some(peer) = peer else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    if !write_sockaddr_v4(addr, addr_len, peer) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lseek(fd: c_int, offset: isize, whence: c_int) -> isize {
    if fd < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    let base = match whence {
        0 => 0isize,
        1 => file.offset().min(isize::MAX as usize) as isize,
        2 => file.len().min(isize::MAX as usize) as isize,
        _ => {
            TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
            return -1;
        }
    };
    let Some(next) = base.checked_add(offset) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    if next < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    file.set_offset(next as usize);
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    next
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pread64(fd: c_int, buf: *mut c_void, count: usize, offset: i64) -> isize {
    if offset < 0 || (count != 0 && buf.is_null()) {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if count == 0 {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        return 0;
    }
    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    match file {
        OpenFile::Regular {
            bytes, readable, ..
        } => {
            if !*readable {
                TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
                return -1;
            }
            let offset = offset as usize;
            let remaining = bytes.len().saturating_sub(offset);
            let n = core::cmp::min(count, remaining);
            if n != 0 && !copy_to_abi_out(buf.cast::<u8>(), &bytes[offset..offset + n]) {
                TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
                return -1;
            }
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            n as isize
        }
        OpenFile::PipeRead { .. } | OpenFile::PipeWrite { .. } | OpenFile::UnixSocket { .. } => {
            TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pread(fd: c_int, buf: *mut c_void, count: usize, offset: i64) -> isize {
    unsafe { pread64(fd, buf, count, offset) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pwrite64(
    fd: c_int,
    buf: *const c_void,
    count: usize,
    offset: i64,
) -> isize {
    if (count != 0 && buf.is_null()) || offset < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    if count == 0 {
        TRUEOS_ERRNO.store(0, Ordering::Relaxed);
        return 0;
    }
    let Some(input) = abi_read_bytes(buf.cast::<u8>(), count) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    match file {
        OpenFile::Regular {
            bytes,
            writable,
            dirty,
            ..
        } => {
            if !*writable {
                TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
                return -1;
            }
            let offset = offset as usize;
            let end = offset.saturating_add(input.len());
            if bytes.len() < end {
                bytes.resize(end, 0);
            }
            bytes[offset..end].copy_from_slice(input);
            *dirty = true;
            TRUEOS_ERRNO.store(0, Ordering::Relaxed);
            input.len() as isize
        }
        OpenFile::PipeRead { .. } | OpenFile::PipeWrite { .. } | OpenFile::UnixSocket { .. } => {
            TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pwrite(fd: c_int, buf: *const c_void, count: usize, offset: i64) -> isize {
    unsafe { pwrite64(fd, buf, count, offset) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fsync(fd: c_int) -> c_int {
    if fd < 0 || OPEN_FILES.lock().get(fd).is_none() {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fdatasync(fd: c_int) -> c_int {
    unsafe { fsync(fd) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ftruncate64(fd: c_int, len: i64) -> c_int {
    if len < 0 {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let mut table = OPEN_FILES.lock();
    let Some(file) = table.get_mut(fd) else {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    };
    if !file.resize(len as usize) {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ftruncate(fd: c_int, len: i64) -> c_int {
    unsafe { ftruncate64(fd, len) }
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
    let len = file.len() as u64;
    let blocks = core::cmp::min(len.saturating_add(511) / 512, i64::MAX as u64) as i64;
    let out = TrueosStat {
        st_dev: 1,
        st_ino: fd as u64,
        st_nlink: 1,
        st_mode: TRUEOS_FILE_MODE,
        st_uid: 0,
        st_gid: 0,
        __pad0: 0,
        st_rdev: 0,
        st_size: core::cmp::min(len, i64::MAX as u64) as i64,
        st_blksize: 1024,
        st_blocks: blocks,
        st_atime: 0,
        st_atime_nsec: 0,
        st_mtime: 0,
        st_mtime_nsec: 0,
        st_ctime: 0,
        st_ctime_nsec: 0,
        __unused: [0; 3],
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
pub unsafe extern "C" fn fstat64(fd: c_int, buf: *mut c_void) -> c_int {
    unsafe { fstat(fd, buf) }
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
pub unsafe extern "C" fn mkdir(path: *const c_char, _mode: c_int) -> c_int {
    if path.is_null() {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    }
    let Some(path) = abi_cstr_to_string(path, 4096) else {
        TRUEOS_ERRNO.store(TRUEOS_EINVAL, Ordering::Relaxed);
        return -1;
    };
    let rc =
        unsafe { crate::r::io::cabi::trueos_cabi_fs_create_dir_all(path.as_ptr(), path.len()) };
    if rc != 0 {
        TRUEOS_ERRNO.store(fs_rc_to_errno(rc), Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn unlink(_path: *const c_char) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmdir(_path: *const c_char) -> c_int {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn access(path: *const c_char, _mode: c_int) -> c_int {
    let mut stat_buf = [0u8; core::mem::size_of::<TrueosStat>()];
    unsafe { stat(path, stat_buf.as_mut_ptr().cast::<c_void>()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fchmod(fd: c_int, _mode: u32) -> c_int {
    if fd < 0 || OPEN_FILES.lock().get(fd).is_none() {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fchown(fd: c_int, _owner: u32, _group: u32) -> c_int {
    if fd < 0 || OPEN_FILES.lock().get(fd).is_none() {
        TRUEOS_ERRNO.store(TRUEOS_EBADF, Ordering::Relaxed);
        return -1;
    }
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn utimes(_path: *const c_char, _times: *const c_void) -> c_int {
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmap64(
    _addr: *mut c_void,
    _len: usize,
    _prot: c_int,
    _flags: c_int,
    _fd: c_int,
    _offset: i64,
) -> *mut c_void {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    usize::MAX as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mremap(
    _old_address: *mut c_void,
    _old_size: usize,
    _new_size: usize,
    _flags: c_int,
) -> *mut c_void {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    usize::MAX as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn munmap(_addr: *mut c_void, _len: usize) -> c_int {
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlopen(_filename: *const c_char, _flags: c_int) -> *mut c_void {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlsym(_handle: *mut c_void, _symbol: *const c_char) -> *mut c_void {
    TRUEOS_ERRNO.store(TRUEOS_ENOSYS, Ordering::Relaxed);
    ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlclose(_handle: *mut c_void) -> c_int {
    TRUEOS_ERRNO.store(0, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlerror() -> *const c_char {
    static DLERROR: &[u8] = b"dynamic loading unavailable\0";
    DLERROR.as_ptr().cast::<c_char>()
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
    match crate::r::net::vlayer::resolve_ipv4_for_sync_abi(host_name) {
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
            if crate::hv::current_hull_guest_context_vm_id().is_some()
                && let Some(count) = crate::hv::vmcall::guest_cpu_count()
            {
                return count as isize;
            }
            crate::workers::app_visible_parallelism() as isize
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
pub unsafe extern "C" fn pthread_mutexattr_init(attr: *mut c_void) -> c_int {
    if pthread_mutexattr_set_kind(attr, TRUEOS_PTHREAD_MUTEX_NORMAL) {
        0
    } else {
        TRUEOS_EINVAL
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutexattr_settype(attr: *mut c_void, kind: c_int) -> c_int {
    if !matches!(
        kind,
        TRUEOS_PTHREAD_MUTEX_NORMAL
            | TRUEOS_PTHREAD_MUTEX_RECURSIVE
            | TRUEOS_PTHREAD_MUTEX_ERRORCHECK
    ) {
        return TRUEOS_EINVAL;
    }
    if pthread_mutexattr_set_kind(attr, kind) {
        0
    } else {
        TRUEOS_EINVAL
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutexattr_destroy(attr: *mut c_void) -> c_int {
    if pthread_mutexattr_set_kind(attr, -1) {
        0
    } else {
        TRUEOS_EINVAL
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_init(mutex: *mut c_void, attr: *const c_void) -> c_int {
    let Some(key) = pthread_key(mutex) else {
        return TRUEOS_EINVAL;
    };
    let kind = if attr.is_null() {
        TRUEOS_PTHREAD_MUTEX_NORMAL
    } else {
        let Some(kind) = pthread_mutexattr_kind(attr) else {
            return TRUEOS_EINVAL;
        };
        if !matches!(
            kind,
            TRUEOS_PTHREAD_MUTEX_NORMAL
                | TRUEOS_PTHREAD_MUTEX_RECURSIVE
                | TRUEOS_PTHREAD_MUTEX_ERRORCHECK
        ) {
            return TRUEOS_EINVAL;
        }
        kind
    };
    pthread_sync_trace("mutex.init", key);
    let Some(state) = pthread_mutex_storage(key) else {
        return TRUEOS_EINVAL;
    };
    unsafe {
        state.as_ptr().write(PthreadMutexStorage {
            owner: AtomicUsize::new(0),
            depth: AtomicUsize::new(0),
            kind: AtomicI32::new(kind),
        });
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_destroy(mutex: *mut c_void) -> c_int {
    if let Some(key) = pthread_key(mutex) {
        pthread_sync_trace("mutex.destroy", key);
        let Some(state) = pthread_mutex_storage(key) else {
            return TRUEOS_EINVAL;
        };
        let state = unsafe { state.as_ref() };
        if state.owner.load(Ordering::Acquire) != 0 {
            return TRUEOS_EBUSY;
        }
        state.depth.store(0, Ordering::Relaxed);
        state.kind.store(-1, Ordering::Relaxed);
        state.owner.store(0, Ordering::Release);
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
    let Some(state) = pthread_cond_storage(key) else {
        return TRUEOS_EINVAL;
    };
    unsafe {
        state.as_ptr().write(PthreadCondStorage {
            generation: AtomicU64::new(0),
        });
    }
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
        let Some(state) = pthread_cond_storage(key) else {
            return TRUEOS_EINVAL;
        };
        let state = unsafe { state.as_ref() };
        state.generation.store(0, Ordering::Release);
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

    let Some(cond_state) = pthread_cond_storage(cond_key) else {
        return TRUEOS_EINVAL;
    };
    let cond_state = unsafe { cond_state.as_ref() };
    let generation = pthread_cond_generation(cond_state);
    let unlock_rc = pthread_mutex_unlock_key(mutex_key);
    if unlock_rc != 0 {
        return unlock_rc;
    }

    while pthread_cond_generation(cond_state) == generation {
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

    let Some(cond_state) = pthread_cond_storage(cond_key) else {
        return TRUEOS_EINVAL;
    };
    let cond_state = unsafe { cond_state.as_ref() };
    let generation = pthread_cond_generation(cond_state);
    let unlock_rc = pthread_mutex_unlock_key(mutex_key);
    if unlock_rc != 0 {
        return unlock_rc;
    }

    for _ in 0..4096 {
        if pthread_cond_generation(cond_state) != generation {
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
pub unsafe extern "C" fn pthread_attr_init(_attr: *mut c_void) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_setstacksize(_attr: *mut c_void, _stacksize: usize) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_attr_destroy(_attr: *mut c_void) -> c_int {
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

    let thread_id = pthread_next_thread_id();
    let completion = Arc::new(crate::wait::CompletionCell::new());
    let state = PthreadThreadState {
        completion: completion.clone(),
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
        let start: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
            unsafe { core::mem::transmute(start) };
        let result = crate::stackkeeper::with_current_blueprint_thread_id(thread_id, || {
            (unsafe { start(arg as *mut c_void) }) as usize
        });
        let _ = completion.complete(result);
    });

    let rc = crate::r::blocking::trueos_service_lane_submit_job(job);
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
        state.completion.clone()
    };

    let result = completion.join_blocking_parked();
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
    let Some(_state) = table.remove(thread) else {
        return TRUEOS_ESRCH;
    };
    // The scheduled closure owns a clone of the completion cell, so dropping
    // the registry entry here is true detach: execution continues without a
    // join resource. This also avoids asking the host carrier to remove an
    // entry from the guest-private Hull table when the thread later exits.
    0
}
