//! VMX hypercall protocol — host side.
//!
//! Three roles in one compact module (to be split when the op table grows):
//!   vmx-comm  : shared CommPage layout + op/status codes
//!   vmx-trans : read request / write response helpers
//!   vmx-exec  : dispatch table executed by the host vmexit loop
//!
//! Guest writes request fields then issues `vmcall`.
//! Host reads, executes, writes response, then vmresumes.
//! The vmcall is synchronous from the guest's point of view.

use crate::hv::hvlogf;
use crate::hv::memory::kernel_va_to_pa;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// ── op codes (u32, written by guest before vmcall) ──────────────────────────
pub const OP_PRESERVE: u32 = 0x01; // snapshot + stop
pub const OP_PING: u32 = 0x02; // response_data = 0xCAFE_BABE
pub const OP_UNIX_TIME: u32 = 0x03; // response_data = unix seconds
pub const OP_YIELD: u32 = 0x04; // cooperative host yield point
pub const OP_SLEEP_MS: u32 = 0x05; // cooperative host sleep before resume
pub const OP_RAND_BYTES: u32 = 0x06; // arg0 requested bytes, response payload is random bytes
pub const OP_BP_CPU_COUNT: u32 = 0x07; // response is app-visible CPU/service lane count
pub const OP_MONOTONIC_NANOS: u32 = 0x08; // response_data = host monotonic nanos
pub const OP_BP_UI3_FRAME_CREATE: u32 = 0x82; // arg0=x/y, arg1=w/h -> frame id
pub const OP_BP_UI3_FRAME_CLOSE: u32 = 0x83; // arg0=frame_id -> rc
pub const OP_BP_UI3_FRAME_REQUEST_REPAINT: u32 = 0x84; // arg0=frame_id -> rc
pub const OP_BP_UI3_FRAME_SET_POSITION: u32 = 0x85; // arg0=frame_id, arg1=x/y -> rc
pub const OP_BP_UI3_FRAME_SET_SIZE: u32 = 0x86; // arg0=frame_id, arg1=w/h -> rc
pub const OP_BP_UI3_FRAME_BEGIN: u32 = 0x87; // arg0=frame_id, arg1=clear/flags -> rc
pub const OP_BP_UI3_FRAME_END: u32 = 0x88; // arg0=frame_id -> rc
pub const OP_BP_UI3_FRAME_SET_RENDER_TARGET: u32 = 0x89; // arg0=frame_id, arg1=tex_id -> rc
pub const OP_BP_UI3_FRAME_DRAW_SOLID_BATCH: u32 = 0x8A; // arg0=frame_id -> rc
pub const OP_BP_UI3_FRAME_DRAW_SPRITE_BATCH: u32 = 0x8B; // arg0=frame_id,arg1=tex_id -> rc
pub const OP_BP_UI3_TEXTURE_UPLOAD_BEGIN: u32 = 0x8C; // arg0=tex_id, arg1=w/h, data=total_len -> rc
pub const OP_BP_UI3_TEXTURE_UPLOAD_CHUNK: u32 = 0x8D; // arg0=tex_id, arg1=offset, payload=rgba -> rc
pub const OP_BP_UI3_TEXTURE_UPLOAD_FINISH: u32 = 0x8E; // arg0=tex_id -> rc
pub const OP_BP_UI3_TEXTURE_STATUS: u32 = 0x8F; // arg0=tex_id -> status
pub const OP_BP_UI3_TEXTURE_DIMENSIONS: u32 = 0x90; // arg0=tex_id -> status + packed w/h
pub const OP_BP_RAPL_SNAPSHOT_READ: u32 = 0x91; // arg0 offset, arg1 cap -> latest RAPL snapshot text
pub const OP_BP_RAPL_HISTORY_READ: u32 = 0x92; // arg0 offset, arg1 cap -> capped RAPL history text
pub const OP_BP_PCI_SNAPSHOT_READ: u32 = 0x93; // arg0 offset, arg1 cap -> latest PCI snapshot text
pub const OP_NET_TCP_WRITE: u32 = 0x10; // request payload -> net tcp shell tx
pub const OP_NET_TCP_READ: u32 = 0x11; // net tcp shell rx -> response payload
pub const OP_BP_NET_OPEN: u32 = 0x20; // host-owned blueprint vnet session
pub const OP_BP_NET_SUBMIT: u32 = 0x21; // request payload is wire Command
pub const OP_BP_NET_POLL: u32 = 0x22; // response payload is optional wire Event
pub const OP_BP_FETCH_BYTES_START: u32 = 0x23; // request payload is URL, response is op id
pub const OP_BP_FETCH_BYTES_RESULT_LEN: u32 = 0x24; // arg0 is op id, response is signed len/rc
pub const OP_BP_FETCH_BYTES_READ: u32 = 0x25; // arg0 op id, arg1 offset, response payload bytes
pub const OP_BP_FETCH_BYTES_DISCARD: u32 = 0x26; // arg0 is op id
pub const OP_BP_FETCH_FILE_START: u32 = 0x27; // arg0 url len, payload is URL || cache path
pub const OP_BP_FETCH_FILE_RESULT: u32 = 0x28; // arg0 is op id, response is signed rc/pending
pub const OP_BP_FETCH_FILE_DISCARD: u32 = 0x29; // arg0 is op id
pub const OP_BP_ENV_ARGS_COUNT: u32 = 0x2A; // response is argc
pub const OP_BP_ENV_ARG: u32 = 0x2B; // arg0 is index, response payload is arg bytes
pub const OP_BP_ENV_VAR: u32 = 0x2C; // request payload is key, response payload is value bytes
pub const OP_BP_FS_READ_FILE: u32 = 0x2D; // arg0 offset, arg1 cap; payload path -> payload bytes
pub const OP_BP_FS_WRITE_BEGIN: u32 = 0x2E; // arg0 total len, payload path -> response handle/rc
pub const OP_BP_FS_WRITE_CHUNK: u32 = 0x2F; // arg0 handle, payload chunk -> rc
pub const OP_BP_FS_WRITE_FINISH: u32 = 0x30; // arg0 handle -> rc
pub const OP_BP_FS_WRITE_ABORT: u32 = 0x31; // arg0 handle -> rc
pub const OP_BP_FS_CREATE_DIR_ALL: u32 = 0x32; // payload path -> rc
pub const OP_BP_FS_EXISTS: u32 = 0x33; // payload path -> 0/1/rc
pub const OP_BP_FS_REMOVE: u32 = 0x34; // payload path -> rc
pub const OP_BP_FS_STAT: u32 = 0x60; // payload path -> rc + kind in response_data[63:32], optional payload kind:u32 len:u64
pub const OP_BP_THREAD_CURRENT_ID: u32 = 0x61; // response is current TRUEOS vthread id
pub const OP_BP_SERVICE_LANE_SUBMIT: u32 = 0x62; // arg0/arg1 boxed service-lane job raw parts
pub const OP_BP_TOKIO_BLOCKING_SPAWN: u32 = OP_BP_SERVICE_LANE_SUBMIT; // compatibility alias
pub const OP_BP_INPUT_CURSOR_POS: u32 = 0x68; // arg0 cursor id -> packed x/y
pub const OP_BP_INPUT_CURSOR_BUTTONS: u32 = 0x69; // arg0 cursor id -> buttons
pub const OP_BP_INPUT_CURSOR_EVENTS: u32 = 0x6A; // arg0 read seq, arg1 cap -> payload events
pub const OP_BP_DNS_RESOLVE_IPV4: u32 = 0x6B; // payload host -> response payload IPv4 bytes
pub const OP_BP_SHELL_ATTACHED_WRITE: u32 = 0x6C; // payload bytes -> attached shell
pub const OP_BP_SHELL_ATTACHED_READ_BYTE: u32 = 0x6D; // response is byte or u64::MAX
pub const OP_BP_ENV_ALL: u32 = 0x6E; // response payload is newline-separated key=value text
pub const OP_BP_FS_LIST_TREE: u32 = 0x6F; // payload path -> response payload tree text
pub const OP_BP_FS_LIST_DIR: u32 = 0x81; // arg0 offset, arg1 cap; payload path -> newline children
pub const OP_BP_SOCKET_TCP_OPEN: u32 = 0x35; // arg0 domain/type, arg1 protocol -> socket/rc
pub const OP_BP_SOCKET_TCP_CLOSE: u32 = 0x36; // arg0 socket -> rc
pub const OP_BP_SOCKET_TCP_SET_NONBLOCKING: u32 = 0x37; // arg0 socket, arg1 bool -> rc
pub const OP_BP_SOCKET_TCP_BIND_V4: u32 = 0x38; // arg0 socket, arg1 addr/port -> rc
pub const OP_BP_SOCKET_TCP_BIND_V6: u32 = 0x39; // arg0 socket, arg1 port, payload addr -> rc
pub const OP_BP_SOCKET_TCP_CONNECT_V4: u32 = 0x3A; // arg0 socket, arg1 addr/port/nb -> rc
pub const OP_BP_SOCKET_TCP_CONNECT_V6: u32 = 0x3B; // arg0 socket, arg1 port/nb, payload addr -> rc
pub const OP_BP_SOCKET_TCP_POLL_CONNECT: u32 = 0x3C; // arg0 socket, arg1 timeout -> rc
pub const OP_BP_SOCKET_TCP_SEND: u32 = 0x3D; // arg0 socket, payload data -> signed count/rc
pub const OP_BP_SOCKET_TCP_RECV: u32 = 0x3E; // arg0 socket, arg1 cap, payload recv opts -> data
pub const OP_BP_SOCKET_TCP_SHUTDOWN: u32 = 0x3F; // arg0 socket, arg1 how -> rc
pub const OP_BP_SOCKET_TCP_TAKE_ERROR: u32 = 0x40; // arg0 socket -> rc
pub const OP_BP_SOCKET_TCP_PEER_V4: u32 = 0x41; // arg0 socket -> rc + addr/port payload
pub const OP_BP_SOCKET_TCP_PEER_V6: u32 = 0x42; // arg0 socket -> rc + addr/port payload
pub const OP_BP_MIO_TCP_LISTENER_BIND: u32 = 0x50; // payload addr -> socket id/status
pub const OP_BP_MIO_TCP_STREAM_CONNECT: u32 = 0x51; // payload addr -> socket id/status
pub const OP_BP_MIO_UDP_SOCKET_BIND: u32 = 0x52; // payload addr -> socket id/status
pub const OP_BP_MIO_SOCKET_CLOSE: u32 = 0x53; // arg0 socket -> status
pub const OP_BP_MIO_SOCKET_LOCAL_ADDR: u32 = 0x54; // arg0 socket -> addr/status
pub const OP_BP_MIO_SOCKET_PEER_ADDR: u32 = 0x55; // arg0 socket -> addr/status
pub const OP_BP_MIO_SOCKET_TAKE_ERROR: u32 = 0x56; // arg0 socket -> status
pub const OP_BP_MIO_TCP_STREAM_READ: u32 = 0x57; // arg0 socket, arg1 cap -> bytes/status
pub const OP_BP_MIO_TCP_STREAM_WRITE: u32 = 0x58; // arg0 socket, payload bytes -> signed rc
pub const OP_BP_MIO_UDP_SOCKET_CONNECT: u32 = 0x59; // arg0 socket, payload addr -> status
pub const OP_BP_MIO_UDP_SOCKET_SEND_TO: u32 = 0x5A; // arg0 socket, payload addr+bytes -> rc
pub const OP_BP_MIO_UDP_SOCKET_RECV_FROM: u32 = 0x5B; // arg0 socket, arg1 cap -> addr+bytes
pub const OP_BP_MIO_TCP_LISTENER_ACCEPT: u32 = 0x5C; // arg0 socket -> child+addr/status
pub const OP_BP_MIO_SELECTOR_REGISTER_SOCKET: u32 = 0x5D; // selector/socket/token/interests
pub const OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET: u32 = 0x5E; // selector/socket
pub const OP_BP_MIO_SELECTOR_POLL: u32 = 0x5F; // selector/cap/timeout -> ready events
pub const OP_BP_MIO_SELECTOR_WAKE: u32 = 0x80; // selector -> wake parked pollers

// ── response status codes (u32, written by host) ────────────────────────────
pub const STATUS_OK: u32 = 0;
pub const STATUS_UNKNOWN_OP: u32 = 1;
pub const STATUS_BAD_ARG: u32 = 2;
const MAX_GUEST_SLEEP_MS: u64 = 10_000;
pub const COMM_PAGE_VM_ID_MAGIC: u32 = 0x4856_0000;

// ── shared page ─────────────────────────────────────────────────────────────

/// Guest virtual address of the comm page.
/// Fixed above the maximum supported guest stack span so guest-side code can
/// use a stable address independent of the runtime-selected stack size.
pub fn comm_page_guest_va() -> u64 {
    crate::hv::memory::GUEST_COMM_PAGE_VA
}
pub const PAYLOAD_CAP: usize = 4096 - 56;

/// Layout of the communication page shared between guest and host.
/// Guest writes request_* fields then vmcall.
/// Host writes response_* fields then vmresumes.
#[repr(C)]
pub struct CommPage {
    // guest fills before vmcall
    pub request_op: u32,
    pub request_seq: u32,
    pub request_arg0: u64,
    pub request_arg1: u64,
    pub request_len: u32,
    pub request_pad: u32,
    // host fills before vmresume
    pub response_seq: u32,
    pub response_status: u32,
    pub response_data: u64,
    pub response_len: u32,
    pub response_pad: u32,
    pub payload: [u8; PAYLOAD_CAP],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DispatchOutcome {
    Resume,
    Stop,
    Yield,
    SleepMs(u64),
}

static GUEST_CABI_SEQ: AtomicU32 = AtomicU32::new(1);
static UI3_VMCALL_COUNT: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_BYTES: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_NS: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_MAX_NS: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_DRAW_SPRITE_COUNT: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_DRAW_SPRITE_NS: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_DRAW_SOLID_COUNT: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_DRAW_SOLID_NS: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_UPLOAD_CHUNK_COUNT: AtomicU64 = AtomicU64::new(0);
static UI3_VMCALL_UPLOAD_CHUNK_NS: AtomicU64 = AtomicU64::new(0);

/// Static 4 K backing page for CommPage.
#[repr(C, align(4096))]
pub struct CommPageBacking([u8; 4096]);

pub static mut COMM_PAGES: [CommPageBacking; crate::hv::TRUEOS_VM_ID_LIMIT] =
    [const { CommPageBacking([0u8; 4096]) }; crate::hv::TRUEOS_VM_ID_LIMIT];

#[inline]
fn host_ptr(vm_id: u8) -> Option<*mut CommPage> {
    if (vm_id as usize) >= crate::hv::TRUEOS_VM_ID_LIMIT {
        return None;
    }
    Some(unsafe { core::ptr::addr_of_mut!(COMM_PAGES[vm_id as usize].0) as *mut CommPage })
}

pub fn prepare_for_vm(vm_id: u8) -> bool {
    let Some(p) = host_ptr(vm_id) else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<CommPage>());
        core::ptr::write_volatile(
            &mut (*p).response_pad,
            COMM_PAGE_VM_ID_MAGIC | vm_id.saturating_add(1) as u32,
        );
    }
    true
}

pub(crate) fn guest_comm_page_vm_id_tag() -> Option<u32> {
    let p = comm_page_guest_va() as *const CommPage;
    unsafe {
        let tag = core::ptr::read_volatile(&(*p).response_pad);
        if (tag & 0xFFFF_0000) != COMM_PAGE_VM_ID_MAGIC {
            return None;
        }
        Some(tag & 0xFF)
    }
}

pub fn pa_for_vm(vm_id: u8) -> Option<u64> {
    if (vm_id as usize) >= crate::hv::TRUEOS_VM_ID_LIMIT {
        return None;
    }
    let va = unsafe { core::ptr::addr_of!(COMM_PAGES[vm_id as usize].0) as u64 };
    kernel_va_to_pa(va)
}

// ── transport helpers ────────────────────────────────────────────────────────

fn read_request(vm_id: u8) -> Option<(u32, u32, u64, u64, u32)> {
    let p = host_ptr(vm_id)?;
    unsafe {
        Some((
            core::ptr::read_volatile(&(*p).request_op),
            core::ptr::read_volatile(&(*p).request_seq),
            core::ptr::read_volatile(&(*p).request_arg0),
            core::ptr::read_volatile(&(*p).request_arg1),
            core::ptr::read_volatile(&(*p).request_len),
        ))
    }
}

fn write_response(vm_id: u8, seq: u32, status: u32, data: u64, len: u32) {
    let Some(p) = host_ptr(vm_id) else {
        return;
    };
    unsafe {
        core::ptr::write_volatile(&mut (*p).response_status, status);
        core::ptr::write_volatile(&mut (*p).response_data, data);
        core::ptr::write_volatile(&mut (*p).response_len, len);
        // seq written last — guest may poll this as a completion flag
        core::ptr::write_volatile(&mut (*p).response_seq, seq);
    }
}

fn request_payload(vm_id: u8, req_len: u32) -> Option<&'static [u8]> {
    if req_len as usize > PAYLOAD_CAP {
        return None;
    }
    let p = host_ptr(vm_id)?;
    Some(unsafe { &(&(*p).payload)[..req_len as usize] })
}

fn handle_vlayer_text_read_vmcall(
    vm_id: u8,
    seq: u32,
    offset: u64,
    cap: u64,
    len_fn: fn() -> usize,
    read_fn: fn(usize, &mut [u8]) -> usize,
) {
    if cap == 0 {
        write_response(vm_id, seq, STATUS_OK, len_fn() as u64, 0);
        return;
    }

    let Some(p) = host_ptr(vm_id) else {
        write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
        return;
    };

    let want = core::cmp::min(cap as usize, PAYLOAD_CAP);
    let copied = unsafe { read_fn(offset as usize, &mut (&mut (*p).payload)[..want]) };
    write_response(vm_id, seq, STATUS_OK, copied as u64, copied as u32);
}

pub fn guest_call(op: u32, arg0: u64, arg1: u64) -> (u32, u64) {
    let seq = GUEST_CABI_SEQ.fetch_add(1, Ordering::Relaxed);
    let p = comm_page_guest_va() as *mut CommPage;
    unsafe {
        core::ptr::write_volatile(&mut (*p).request_arg0, arg0);
        core::ptr::write_volatile(&mut (*p).request_arg1, arg1);
        core::ptr::write_volatile(&mut (*p).request_len, 0);
        core::ptr::write_volatile(&mut (*p).request_seq, seq);
        core::ptr::write_volatile(&mut (*p).request_op, op);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));
        let status = core::ptr::read_volatile(&(*p).response_status);
        let data = core::ptr::read_volatile(&(*p).response_data);
        (status, data)
    }
}

pub fn guest_yield() {
    let _ = guest_call(OP_YIELD, 0, 0);
}

pub fn guest_sleep_ms(ms: u64) {
    let _ = guest_call(OP_SLEEP_MS, ms, 0);
}

pub fn guest_cpu_count() -> Option<usize> {
    let (status, count) = guest_call(OP_BP_CPU_COUNT, 0, 0);
    if status == STATUS_OK {
        Some(count.max(1) as usize)
    } else {
        None
    }
}

pub fn guest_monotonic_nanos() -> u64 {
    let (status, nanos) = guest_call(OP_MONOTONIC_NANOS, 0, 0);
    if status == STATUS_OK { nanos } else { 0 }
}

pub fn guest_unix_seconds() -> u64 {
    let (status, seconds) = guest_call(OP_UNIX_TIME, 0, 0);
    if status == STATUS_OK { seconds } else { 0 }
}

#[inline]
pub fn pack_i32_pair(a: i32, b: i32) -> u64 {
    ((a as u32 as u64) << 32) | (b as u32 as u64)
}

#[inline]
pub fn pack_u32_pair(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | (b as u64)
}

#[inline]
fn unpack_i32_pair(raw: u64) -> (i32, i32) {
    ((raw >> 32) as u32 as i32, raw as u32 as i32)
}

#[inline]
fn unpack_u32_pair(raw: u64) -> (u32, u32) {
    ((raw >> 32) as u32, raw as u32)
}

const MIO_ADDR_BYTES: usize = core::mem::size_of::<crate::mio_compat::TrueosMioSocketAddr>();
const MIO_READY_EVENT_BYTES: usize = core::mem::size_of::<crate::mio_compat::TrueosMioReadyEvent>();

fn read_mio_addr(bytes: &[u8]) -> Option<crate::mio_compat::TrueosMioSocketAddr> {
    if bytes.len() < MIO_ADDR_BYTES {
        return None;
    }
    Some(unsafe {
        core::ptr::read_unaligned(bytes.as_ptr() as *const crate::mio_compat::TrueosMioSocketAddr)
    })
}

fn write_mio_addr(out: &mut [u8], addr: crate::mio_compat::TrueosMioSocketAddr) -> bool {
    if out.len() < MIO_ADDR_BYTES {
        return false;
    }
    let bytes =
        unsafe { core::slice::from_raw_parts(&addr as *const _ as *const u8, MIO_ADDR_BYTES) };
    out[..MIO_ADDR_BYTES].copy_from_slice(bytes);
    true
}

// ── exec dispatch ────────────────────────────────────────────────────────────

/// Called from the vmexit loop on every VMCALL exit.
pub fn dispatch(vm_id: u8) -> DispatchOutcome {
    crate::allocators::with_host_alloc_domain(|| {
        crate::r::kernel_task_domain::with(
            crate::r::kernel_task_domain::KernelTaskDomain::VmBroker,
            Some(vm_id),
            || crate::hv::with_guest_broker_context(vm_id, || dispatch_inner(vm_id)),
        )
    })
}

fn dispatch_inner(vm_id: u8) -> DispatchOutcome {
    let Some((op, seq, arg0, arg1, req_len)) = read_request(vm_id) else {
        hvlogf(format_args!("hv: vm{} reporting: vmcall bad vm id", vm_id));
        return DispatchOutcome::Stop;
    };
    let dispatch_start_ns = crate::chronos::monotonic_nanos();
    match op {
        OP_PRESERVE => {
            write_response(vm_id, seq, STATUS_OK, 0, 0);
            DispatchOutcome::Stop
        }
        OP_PING => {
            write_response(vm_id, seq, STATUS_OK, 0xCAFE_BABE, 0);
            DispatchOutcome::Resume
        }
        OP_BP_CPU_COUNT => {
            let count = crate::hv::blueprint_exposed_cpu_count(vm_id);
            write_response(vm_id, seq, STATUS_OK, count as u64, 0);
            DispatchOutcome::Resume
        }
        OP_UNIX_TIME => {
            let t = crate::chronos::best_effort_unix_time_seconds().unwrap_or(0);
            write_response(vm_id, seq, STATUS_OK, t, 0);
            DispatchOutcome::Resume
        }
        OP_MONOTONIC_NANOS => {
            let t = crate::chronos::monotonic_nanos();
            write_response(vm_id, seq, STATUS_OK, t, 0);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_CREATE => {
            let (x, y) = unpack_i32_pair(arg0);
            let (width, height) = unpack_u32_pair(arg1);
            let tex_id = request_payload(vm_id, req_len)
                .and_then(|payload| payload.get(..4))
                .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                .unwrap_or(0);
            let frame_id = crate::ui3::ui3_frame::create_frame(x, y, width, height, tex_id);
            if frame_id == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, frame_id as u64, 0);
            }
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_CLOSE => {
            let ok = crate::ui3::ui3_frame::close_frame(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, if ok { 0 } else { (-1i64) as u64 }, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_REQUEST_REPAINT => {
            let ok = crate::ui3::ui3_frame::request_repaint(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, if ok { 0 } else { (-1i64) as u64 }, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_SET_POSITION => {
            let (x, y) = unpack_i32_pair(arg1);
            let ok = crate::ui3::ui3_frame::set_position(arg0 as u32, x, y);
            write_response(vm_id, seq, STATUS_OK, if ok { 0 } else { (-1i64) as u64 }, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_SET_SIZE => {
            let (width, height) = unpack_u32_pair(arg1);
            let ok = crate::ui3::ui3_frame::set_size(arg0 as u32, width, height);
            write_response(vm_id, seq, STATUS_OK, if ok { 0 } else { (-1i64) as u64 }, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_BEGIN => {
            let clear_rgb = arg1 as u32;
            let flags = (arg1 >> 32) as u32;
            let preserve_contents = (flags & 1) != 0;
            let allow_present = (flags & 2) != 0;
            let rc = crate::ui3::ui3_frame::begin_frame(
                arg0 as u32,
                clear_rgb,
                preserve_contents,
                allow_present,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_END => {
            let rc = crate::ui3::ui3_frame::end_frame(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_SET_RENDER_TARGET => {
            let rc = crate::ui3::ui3_frame::set_render_target(arg0 as u32, arg1 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_DRAW_SOLID_BATCH => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
                return DispatchOutcome::Resume;
            };
            let rc = crate::ui3::ui3_frame::draw_solid_batch(arg0 as u32, payload);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_FRAME_DRAW_SPRITE_BATCH => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
                return DispatchOutcome::Resume;
            };
            let rc = crate::ui3::ui3_frame::draw_sprite_batch(arg0 as u32, arg1 as u32, payload);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_TEXTURE_UPLOAD_BEGIN => {
            let (width, height) = unpack_u32_pair(arg1);
            let total_len = request_payload(vm_id, req_len)
                .and_then(|payload| payload.get(..8))
                .map(|bytes| {
                    u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ]) as usize
                })
                .unwrap_or(0);
            let rc = crate::ui3::ui3_img::begin_rgba_upload(
                vm_id,
                arg0 as u32,
                width,
                height,
                total_len,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_TEXTURE_UPLOAD_CHUNK => {
            let Some(payload) = request_payload(vm_id, req_len) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
                return DispatchOutcome::Resume;
            };
            let rc = crate::ui3::ui3_img::write_rgba_upload_chunk(
                vm_id,
                arg0 as u32,
                arg1 as usize,
                payload,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_TEXTURE_UPLOAD_FINISH => {
            let rc = crate::ui3::ui3_img::finish_rgba_upload(vm_id, arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_TEXTURE_STATUS => {
            let status = crate::ui3::ui3_img::image_status(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (status as i64) as u64, 0);
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_BP_UI3_TEXTURE_DIMENSIONS => {
            if let Some((width, height)) = crate::ui3::ui3_img::image_dimensions(arg0 as u32) {
                write_response(vm_id, seq, STATUS_OK, pack_u32_pair(width, height), 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, 0, 0);
            }
            record_ui3_vmcall_timing(op, req_len, dispatch_start_ns);
            DispatchOutcome::Resume
        }
        OP_YIELD => {
            write_response(vm_id, seq, STATUS_OK, 0, 0);
            DispatchOutcome::Yield
        }
        OP_SLEEP_MS => {
            let sleep_ms = arg0.min(MAX_GUEST_SLEEP_MS);
            write_response(vm_id, seq, STATUS_OK, sleep_ms, 0);
            DispatchOutcome::SleepMs(sleep_ms)
        }
        OP_RAND_BYTES => {
            let want = core::cmp::min(arg0 as usize, PAYLOAD_CAP);
            if want == 0 {
                write_response(vm_id, seq, STATUS_OK, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            if crate::tyche::fill_bytes(out) {
                write_response(vm_id, seq, STATUS_OK, want as u64, want as u32);
            } else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_THREAD_CURRENT_ID => {
            let vtid = 0x8000u32.saturating_add(vm_id as u32);
            write_response(vm_id, seq, STATUS_OK, vtid as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SERVICE_LANE_SUBMIT => {
            let rc = unsafe {
                crate::r::blocking::submit_guest_service_lane_job_from_raw(
                    vm_id,
                    arg0 as usize,
                    arg1 as usize,
                    "vmx-service-lane",
                )
            };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_INPUT_CURSOR_POS => {
            let mut x = 0i32;
            let mut y = 0i32;
            let rc = crate::r::io::cabi::host_input_cursor_pos(arg0 as u32, &mut x, &mut y);
            let packed = ((x as u32 as u64) << 32) | (y as u32 as u64);
            if rc == 0 {
                write_response(vm_id, seq, STATUS_OK, packed, 0);
            } else {
                write_response(vm_id, seq, STATUS_BAD_ARG, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_INPUT_CURSOR_BUTTONS => {
            let mut buttons = 0u32;
            let rc = crate::r::io::cabi::host_input_cursor_buttons(arg0 as u32, &mut buttons);
            write_response(vm_id, seq, STATUS_OK, ((rc as i64 as u64) << 32) | (buttons as u64), 0);
            DispatchOutcome::Resume
        }
        OP_BP_INPUT_CURSOR_EVENTS => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let (wrote, response_len) =
                crate::r::io::cabi::host_input_cursor_events_since(arg0, arg1 as u32, unsafe {
                    &mut (*p).payload
                });
            write_response(vm_id, seq, STATUS_OK, wrote as u64, response_len as u32);
            DispatchOutcome::Resume
        }
        OP_BP_DNS_RESOLVE_IPV4 => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(host) = core::str::from_utf8(bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    crate::r::net::vlayer::dns_resolve_error_code(
                        crate::r::net::vlayer::DnsResolveError::BadName,
                    ),
                    0,
                );
                return DispatchOutcome::Resume;
            };
            match crate::r::net::vlayer::resolve_ipv4_for_sync_abi_host(host) {
                Ok(ip) => {
                    unsafe {
                        (&mut (*p).payload)[..4].copy_from_slice(&ip);
                    }
                    write_response(vm_id, seq, STATUS_OK, 0, 4);
                }
                Err(err) => {
                    write_response(
                        vm_id,
                        seq,
                        STATUS_OK,
                        crate::r::net::vlayer::dns_resolve_error_code(err),
                        0,
                    );
                }
            }
            DispatchOutcome::Resume
        }
        OP_BP_RAPL_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::rapl_snapshot_len_host,
                crate::r::net::vlayer::rapl_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_RAPL_HISTORY_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::rapl_history_len_host,
                crate::r::net::vlayer::rapl_history_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_BP_PCI_SNAPSHOT_READ => {
            handle_vlayer_text_read_vmcall(
                vm_id,
                seq,
                arg0,
                arg1,
                crate::r::net::vlayer::pci_snapshot_len_host,
                crate::r::net::vlayer::pci_snapshot_read_host,
            );
            DispatchOutcome::Resume
        }
        OP_NET_TCP_WRITE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            match crate::hv::vnet::tcp_write(vm_id, bytes) {
                Ok(written) => write_response(vm_id, seq, STATUS_OK, written as u64, 0),
                Err(_) => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        OP_NET_TCP_READ => {
            let want = core::cmp::min(arg0 as usize, PAYLOAD_CAP);
            if want == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }

            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            match crate::hv::vnet::tcp_read(vm_id, out) {
                Ok(got) => write_response(vm_id, seq, STATUS_OK, got as u64, got as u32),
                Err(_) => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        OP_BP_NET_OPEN => {
            match crate::hv::blueprint_net::open_primary() {
                Some(session_id) => write_response(vm_id, seq, STATUS_OK, session_id as u64, 0),
                None => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        OP_BP_NET_SUBMIT => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            match crate::hv::blueprint_net::submit(arg0 as u32, bytes) {
                Ok(()) => write_response(vm_id, seq, STATUS_OK, 0, 0),
                Err(()) => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        OP_BP_NET_POLL => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
            match crate::hv::blueprint_net::poll_event(arg0 as u32, out) {
                Ok(Some(len)) => write_response(vm_id, seq, STATUS_OK, 1, len as u32),
                Ok(None) => write_response(vm_id, seq, STATUS_OK, 0, 0),
                Err(()) => write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_START => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(url) = core::str::from_utf8(bytes) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            const TIMEOUT_MS: u32 = 45_000;
            const MAX_BYTES: usize = 8 * 1024 * 1024;
            let op_id =
                crate::r::net::https::cabi_net_fetch_bytes_start_host(url, TIMEOUT_MS, MAX_BYTES);
            if op_id == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, op_id as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_RESULT_LEN => {
            let rc = crate::r::net::https::cabi_net_fetch_bytes_result_len_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_READ => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
            let rc = crate::r::net::https::cabi_net_fetch_bytes_read_chunk_host(
                arg0 as u32,
                arg1 as usize,
                out,
            );
            if rc < 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, (rc as i64) as u64, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, rc as u64, rc as u32);
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_DISCARD => {
            let rc = crate::r::net::https::cabi_net_fetch_bytes_discard_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_FILE_START => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let url_len = arg0 as usize;
            if url_len == 0 || url_len >= n {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(url) = core::str::from_utf8(&bytes[..url_len]) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Ok(path) = core::str::from_utf8(&bytes[url_len..]) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            const TIMEOUT_MS: u32 = 45_000;
            const MAX_BYTES: usize = 8 * 1024 * 1024;
            let op_id =
                crate::r::net::https::cabi_net_fetch_start_host(url, path, TIMEOUT_MS, MAX_BYTES);
            if op_id == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, op_id as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_FILE_RESULT => {
            let rc = crate::r::net::https::cabi_net_fetch_result_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_FILE_DISCARD => {
            let rc = crate::r::net::https::cabi_net_fetch_discard_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_ARGS_COUNT => {
            let count = crate::hv::blueprint_process_arg_count(vm_id)
                .unwrap_or_else(crate::r::io::env::arg_count);
            write_response(vm_id, seq, STATUS_OK, count as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_ARG => {
            let Some(arg) = crate::hv::blueprint_process_arg(vm_id, arg0 as usize)
                .or_else(|| crate::r::io::env::arg(arg0 as usize))
            else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = arg.as_bytes();
            let n = core::cmp::min(bytes.len(), PAYLOAD_CAP);
            unsafe {
                (&mut (&mut (*p).payload)[..n]).copy_from_slice(&bytes[..n]);
            }
            write_response(vm_id, seq, STATUS_OK, bytes.len() as u64, n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_VAR => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let key_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(key) = core::str::from_utf8(key_bytes) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(value) = crate::hv::blueprint_process_env_var(vm_id, key)
                .or_else(|| crate::r::io::env::var(key))
            else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = value.as_bytes();
            let out_n = core::cmp::min(bytes.len(), PAYLOAD_CAP);
            unsafe {
                (&mut (&mut (*p).payload)[..out_n]).copy_from_slice(&bytes[..out_n]);
            }
            write_response(vm_id, seq, STATUS_OK, bytes.len() as u64, out_n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_ALL => {
            let Some(text) = crate::hv::blueprint_process_env_text(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = text.as_bytes();
            let out_n = core::cmp::min(bytes.len(), PAYLOAD_CAP);
            unsafe {
                (&mut (&mut (*p).payload)[..out_n]).copy_from_slice(&bytes[..out_n]);
            }
            write_response(vm_id, seq, STATUS_OK, bytes.len() as u64, out_n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_ATTACHED_WRITE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let data = unsafe { &(&(*p).payload)[..n] };
            let written = crate::hv::blueprint_console_write(vm_id, data);
            write_response(vm_id, seq, STATUS_OK, written as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SHELL_ATTACHED_READ_BYTE => {
            let byte = crate::hv::blueprint_console_read_byte(vm_id)
                .map(u64::from)
                .unwrap_or(u64::MAX);
            write_response(vm_id, seq, STATUS_OK, byte, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_LIST_TREE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let Some(text) = crate::hv::blueprint_process_file_tree_text(vm_id, path) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = text.as_bytes();
            let out_n = core::cmp::min(bytes.len(), PAYLOAD_CAP);
            unsafe {
                (&mut (&mut (*p).payload)[..out_n]).copy_from_slice(&bytes[..out_n]);
            }
            write_response(vm_id, seq, STATUS_OK, bytes.len() as u64, out_n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_FS_LIST_DIR => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let text = match crate::r::io::cabi::fs_list_dir_host_text(path) {
                Ok(text) => text,
                Err(rc) => {
                    write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
                    return DispatchOutcome::Resume;
                }
            };
            if arg1 == 0 {
                write_response(vm_id, seq, STATUS_OK, text.len() as u64, 0);
                return DispatchOutcome::Resume;
            }
            let bytes = text.as_bytes();
            let offset = core::cmp::min(arg0 as usize, bytes.len());
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP);
            let end = core::cmp::min(offset.saturating_add(want), bytes.len());
            let out_n = end.saturating_sub(offset);
            unsafe {
                (&mut (&mut (*p).payload)[..out_n]).copy_from_slice(&bytes[offset..end]);
            }
            write_response(vm_id, seq, STATUS_OK, out_n as u64, out_n as u32);
            DispatchOutcome::Resume
        }
        OP_BP_FS_READ_FILE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            if arg1 == 0 {
                let rc = crate::r::io::cabi::fs_read_file_len_host(path);
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP);
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            let rc = crate::r::io::cabi::fs_read_file_chunk_host(path, arg0 as usize, out);
            let out_len = if rc > 0 { rc as u32 } else { 0 };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, out_len);
            DispatchOutcome::Resume
        }
        OP_BP_FS_WRITE_BEGIN => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let rc = crate::r::io::cabi::fs_write_begin_host(path, arg0);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_WRITE_CHUNK => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let rc = crate::r::io::cabi::fs_write_chunk_host(arg0 as u32, bytes);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_WRITE_FINISH => {
            let rc = crate::r::io::cabi::fs_write_finish_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_WRITE_ABORT => {
            let rc = crate::r::io::cabi::fs_write_abort_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_CREATE_DIR_ALL => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let rc = crate::r::io::cabi::fs_create_dir_all_host(path);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_EXISTS => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let rc = crate::r::io::cabi::fs_exists_host(path);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FS_STAT => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let mut kind = 0u32;
            let mut len = 0u64;
            let rc = crate::r::io::cabi::fs_stat_host(path, &mut kind, &mut len);
            let data = (rc as u32 as u64) | ((kind as u64) << 32);
            if path.contains("ggml-tiny") {
                crate::log!(
                    "vmcall: bp-fs-stat path={} rc={} kind={} len={}\n",
                    path,
                    rc,
                    kind,
                    len
                );
            }
            let out_len = if rc == 0 {
                let payload = unsafe { &mut (&mut (*p).payload)[..12] };
                payload[..4].copy_from_slice(&kind.to_le_bytes());
                payload[4..12].copy_from_slice(&len.to_le_bytes());
                12
            } else {
                0
            };
            write_response(vm_id, seq, STATUS_OK, data, out_len);
            DispatchOutcome::Resume
        }
        OP_BP_FS_REMOVE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let path_bytes = unsafe { &(&(*p).payload)[..n] };
            let Ok(path) = core::str::from_utf8(path_bytes) else {
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    (crate::r::io::cabi::FS_ERR_BAD_UTF8 as i64) as u64,
                    0,
                );
                return DispatchOutcome::Resume;
            };
            let rc = crate::r::io::cabi::fs_remove_host(path);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_OPEN => {
            let domain = arg0 as u32 as i32;
            let socket_type = (arg0 >> 32) as u32 as i32;
            let protocol = arg1 as u32 as i32;
            let rc =
                crate::r::net::socket_cabi::socket_tcp_open_host(domain, socket_type, protocol);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_CLOSE => {
            let rc = crate::r::net::socket_cabi::socket_tcp_close_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_SET_NONBLOCKING => {
            let rc = crate::r::net::socket_cabi::socket_tcp_set_nonblocking_host(
                arg0 as u32,
                arg1 as u32,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_BIND_V4 => {
            let addr_be = arg1 as u32;
            let port_be = ((arg1 >> 32) & 0xFFFF) as u16;
            let rc =
                crate::r::net::socket_cabi::socket_tcp_bind_v4_host(arg0 as u32, addr_be, port_be);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_BIND_V6 => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n < 16 {
                write_response(vm_id, seq, STATUS_OK, (-22i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut addr = [0u8; 16];
            unsafe {
                addr.copy_from_slice(&(&(*p).payload)[..16]);
            }
            let rc =
                crate::r::net::socket_cabi::socket_tcp_bind_v6_host(arg0 as u32, addr, arg1 as u16);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_CONNECT_V4 => {
            let addr_be = arg1 as u32;
            let port_be = ((arg1 >> 32) & 0xFFFF) as u16;
            let nonblocking = ((arg1 >> 48) & 1) as u32;
            let rc = crate::r::net::socket_cabi::socket_tcp_connect_v4_host(
                arg0 as u32,
                addr_be,
                port_be,
                nonblocking,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_CONNECT_V6 => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n < 16 {
                write_response(vm_id, seq, STATUS_OK, (-22i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut addr = [0u8; 16];
            unsafe {
                addr.copy_from_slice(&(&(*p).payload)[..16]);
            }
            let port_be = arg1 as u16;
            let nonblocking = ((arg1 >> 16) & 1) as u32;
            let rc = crate::r::net::socket_cabi::socket_tcp_connect_v6_host(
                arg0 as u32,
                addr,
                port_be,
                nonblocking,
            );
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_POLL_CONNECT => {
            let rc = crate::r::net::socket_cabi::socket_tcp_poll_connect_host(arg0 as u32, arg1);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_SEND => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let rc = crate::r::net::socket_cabi::socket_tcp_send_host(arg0 as u32, bytes);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_RECV => {
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP);
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n < 16 {
                write_response(vm_id, seq, STATUS_OK, (-22i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let payload = unsafe { &mut (*p).payload };
            let flags = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let nonblocking = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let timeout_ms = u64::from_le_bytes([
                payload[8],
                payload[9],
                payload[10],
                payload[11],
                payload[12],
                payload[13],
                payload[14],
                payload[15],
            ]);
            let rc = crate::r::net::socket_cabi::socket_tcp_recv_host(
                arg0 as u32,
                &mut payload[..want],
                flags,
                nonblocking,
                timeout_ms,
            );
            let out_len = if rc > 0 { rc as u32 } else { 0 };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, out_len);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_SHUTDOWN => {
            let rc = crate::r::net::socket_cabi::socket_tcp_shutdown_host(arg0 as u32, arg1 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_TAKE_ERROR => {
            let rc = crate::r::net::socket_cabi::socket_tcp_take_error_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_PEER_V4 => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            match crate::r::net::socket_cabi::socket_tcp_peer_v4_host(arg0 as u32) {
                Ok((addr, port)) => {
                    unsafe {
                        (&mut (*p).payload)[..4].copy_from_slice(&addr.to_le_bytes());
                        (&mut (*p).payload)[4..6].copy_from_slice(&port.to_le_bytes());
                    }
                    write_response(vm_id, seq, STATUS_OK, 0, 6);
                }
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_SOCKET_TCP_PEER_V6 => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            match crate::r::net::socket_cabi::socket_tcp_peer_v6_host(arg0 as u32) {
                Ok((addr, port)) => {
                    unsafe {
                        (&mut (*p).payload)[..16].copy_from_slice(&addr);
                        (&mut (*p).payload)[16..18].copy_from_slice(&port.to_le_bytes());
                    }
                    write_response(vm_id, seq, STATUS_OK, 0, 18);
                }
                Err(rc) => write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0),
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_TCP_LISTENER_BIND | OP_BP_MIO_TCP_STREAM_CONNECT | OP_BP_MIO_UDP_SOCKET_BIND => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Some(addr) = read_mio_addr(bytes) else {
                write_response(vm_id, seq, STATUS_OK, (-4i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let mut socket_id = 0u32;
            let status = crate::hv::with_guest_broker_context(vm_id, || match op {
                OP_BP_MIO_TCP_LISTENER_BIND => unsafe {
                    crate::mio_compat::mio_tcp_listener_bind_host(addr, &mut socket_id)
                },
                OP_BP_MIO_TCP_STREAM_CONNECT => unsafe {
                    crate::mio_compat::mio_tcp_stream_connect_host(addr, &mut socket_id)
                },
                _ => unsafe { crate::mio_compat::mio_udp_socket_bind_host(addr, &mut socket_id) },
            });
            if status == 0 {
                write_response(vm_id, seq, STATUS_OK, socket_id as u64, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, (status as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SOCKET_CLOSE => {
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_socket_close_host(arg0 as u32)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SOCKET_LOCAL_ADDR | OP_BP_MIO_SOCKET_PEER_ADDR => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut addr = crate::mio_compat::TrueosMioSocketAddr::default();
            let rc = crate::hv::with_guest_broker_context(vm_id, || {
                if op == OP_BP_MIO_SOCKET_LOCAL_ADDR {
                    unsafe { crate::mio_compat::mio_socket_local_addr_host(arg0 as u32, &mut addr) }
                } else {
                    unsafe { crate::mio_compat::mio_socket_peer_addr_host(arg0 as u32, &mut addr) }
                }
            });
            if rc == 0 {
                let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
                let len = if write_mio_addr(out, addr) {
                    MIO_ADDR_BYTES as u32
                } else {
                    0
                };
                write_response(vm_id, seq, STATUS_OK, 0, len);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SOCKET_TAKE_ERROR => {
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_socket_take_error_host(arg0 as u32)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_TCP_STREAM_READ => {
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..want] };
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_tcp_stream_read_host(arg0 as u32, out.as_mut_ptr(), want)
            });
            let len = if rc > 0 { rc as u32 } else { 0 };
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, len);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_TCP_STREAM_WRITE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_tcp_stream_write_host(
                    arg0 as u32,
                    bytes.as_ptr(),
                    bytes.len(),
                )
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_UDP_SOCKET_CONNECT => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Some(addr) = read_mio_addr(bytes) else {
                write_response(vm_id, seq, STATUS_OK, (-4i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_udp_socket_connect_host(arg0 as u32, addr)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_UDP_SOCKET_SEND_TO => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let Some(addr) = read_mio_addr(bytes) else {
                write_response(vm_id, seq, STATUS_OK, (-4i64) as u64, 0);
                return DispatchOutcome::Resume;
            };
            let data_len = core::cmp::min(arg1 as usize, n.saturating_sub(MIO_ADDR_BYTES));
            let data = &bytes[MIO_ADDR_BYTES..MIO_ADDR_BYTES + data_len];
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_udp_socket_send_to_host(
                    arg0 as u32,
                    addr,
                    data.as_ptr(),
                    data.len(),
                )
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_UDP_SOCKET_RECV_FROM => {
            let want = core::cmp::min(arg1 as usize, PAYLOAD_CAP.saturating_sub(MIO_ADDR_BYTES));
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let payload = unsafe { &mut (*p).payload };
            let mut addr = crate::mio_compat::TrueosMioSocketAddr::default();
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_udp_socket_recv_from_host(
                    arg0 as u32,
                    &mut addr,
                    payload[MIO_ADDR_BYTES..].as_mut_ptr(),
                    want,
                )
            });
            if rc > 0 {
                let _ = write_mio_addr(&mut payload[..MIO_ADDR_BYTES], addr);
                write_response(
                    vm_id,
                    seq,
                    STATUS_OK,
                    rc as u64,
                    (MIO_ADDR_BYTES + rc as usize) as u32,
                );
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_TCP_LISTENER_ACCEPT => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let mut socket_id = 0u32;
            let mut addr = crate::mio_compat::TrueosMioSocketAddr::default();
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_tcp_listener_accept_host(
                    arg0 as u32,
                    &mut socket_id,
                    &mut addr,
                )
            });
            if rc == 0 {
                let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
                let len = if write_mio_addr(out, addr) {
                    MIO_ADDR_BYTES as u32
                } else {
                    0
                };
                write_response(vm_id, seq, STATUS_OK, socket_id as u64, len);
            } else {
                write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SELECTOR_REGISTER_SOCKET => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            if n < 8 {
                write_response(vm_id, seq, STATUS_OK, (-4i64) as u64, 0);
                return DispatchOutcome::Resume;
            }
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let bytes = unsafe { &(&(*p).payload)[..n] };
            let token = u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]) as usize;
            let socket_id = arg1 as u32;
            let interests = ((arg1 >> 32) & 0xFF) as u8;
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_selector_register_socket_host(
                    arg0 as usize,
                    socket_id,
                    token,
                    interests,
                )
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET => {
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_selector_deregister_socket_host(arg0 as usize, arg1 as u32)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SELECTOR_WAKE => {
            let rc = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_selector_wake_host(arg0 as usize)
            });
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_MIO_SELECTOR_POLL => {
            let max_events = core::cmp::min(
                arg1 as usize,
                PAYLOAD_CAP / core::cmp::max(MIO_READY_EVENT_BYTES, 1),
            );
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let timeout_nanos = if n >= 8 {
                let Some(p) = host_ptr(vm_id) else {
                    write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                    return DispatchOutcome::Resume;
                };
                let bytes = unsafe { &(&(*p).payload)[..n] };
                u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            } else {
                u64::MAX
            };
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
            let count = crate::hv::with_guest_broker_context(vm_id, || unsafe {
                crate::mio_compat::mio_selector_poll_host(
                    arg0 as usize,
                    out.as_mut_ptr() as *mut crate::mio_compat::TrueosMioReadyEvent,
                    max_events,
                    timeout_nanos,
                )
            });
            let count = core::cmp::min(count, max_events);
            write_response(
                vm_id,
                seq,
                STATUS_OK,
                count as u64,
                (count * MIO_READY_EVENT_BYTES) as u32,
            );
            DispatchOutcome::Resume
        }
        _ => {
            hvlogf(format_args!(
                "hv: vm{} reporting: vmcall unknown op=0x{:02X} seq={}",
                vm_id, op, seq
            ));
            write_response(vm_id, seq, STATUS_UNKNOWN_OP, 0, 0);
            DispatchOutcome::Resume
        }
    }
}

fn record_ui3_vmcall_timing(op: u32, req_len: u32, start_ns: u64) {
    let elapsed_ns = crate::chronos::monotonic_nanos().saturating_sub(start_ns);
    let count = UI3_VMCALL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    UI3_VMCALL_BYTES.fetch_add(req_len as u64, Ordering::Relaxed);
    UI3_VMCALL_NS.fetch_add(elapsed_ns, Ordering::Relaxed);
    UI3_VMCALL_MAX_NS.fetch_max(elapsed_ns, Ordering::Relaxed);

    match op {
        OP_BP_UI3_FRAME_DRAW_SPRITE_BATCH => {
            UI3_VMCALL_DRAW_SPRITE_COUNT.fetch_add(1, Ordering::Relaxed);
            UI3_VMCALL_DRAW_SPRITE_NS.fetch_add(elapsed_ns, Ordering::Relaxed);
        }
        OP_BP_UI3_FRAME_DRAW_SOLID_BATCH => {
            UI3_VMCALL_DRAW_SOLID_COUNT.fetch_add(1, Ordering::Relaxed);
            UI3_VMCALL_DRAW_SOLID_NS.fetch_add(elapsed_ns, Ordering::Relaxed);
        }
        OP_BP_UI3_TEXTURE_UPLOAD_CHUNK => {
            UI3_VMCALL_UPLOAD_CHUNK_COUNT.fetch_add(1, Ordering::Relaxed);
            UI3_VMCALL_UPLOAD_CHUNK_NS.fetch_add(elapsed_ns, Ordering::Relaxed);
        }
        _ => {}
    }

    if count <= 16 || count % 512 == 0 {
        let total_ns = UI3_VMCALL_NS.load(Ordering::Relaxed);
        let bytes = UI3_VMCALL_BYTES.load(Ordering::Relaxed);
        let draw_sprite_count = UI3_VMCALL_DRAW_SPRITE_COUNT.load(Ordering::Relaxed);
        let draw_solid_count = UI3_VMCALL_DRAW_SOLID_COUNT.load(Ordering::Relaxed);
        let upload_count = UI3_VMCALL_UPLOAD_CHUNK_COUNT.load(Ordering::Relaxed);
        crate::log!(
            "ui3/vmcall: calls={} bytes={} total_ms={} avg_us={} max_us={} sprite_calls={} sprite_avg_us={} solid_calls={} solid_avg_us={} upload_chunks={} upload_avg_us={} last_op=0x{:02X} last_len={} last_us={}\n",
            count,
            bytes,
            ns_to_ms(total_ns),
            avg_ns_to_us(total_ns, count),
            ns_to_us(UI3_VMCALL_MAX_NS.load(Ordering::Relaxed)),
            draw_sprite_count,
            avg_ns_to_us(UI3_VMCALL_DRAW_SPRITE_NS.load(Ordering::Relaxed), draw_sprite_count),
            draw_solid_count,
            avg_ns_to_us(UI3_VMCALL_DRAW_SOLID_NS.load(Ordering::Relaxed), draw_solid_count),
            upload_count,
            avg_ns_to_us(UI3_VMCALL_UPLOAD_CHUNK_NS.load(Ordering::Relaxed), upload_count),
            op,
            req_len,
            ns_to_us(elapsed_ns)
        );
    }
}

#[inline]
fn ns_to_us(ns: u64) -> u64 {
    ns / 1_000
}

#[inline]
fn ns_to_ms(ns: u64) -> u64 {
    ns / 1_000_000
}

#[inline]
fn avg_ns_to_us(total_ns: u64, count: u64) -> u64 {
    if count == 0 {
        0
    } else {
        total_ns / count / 1_000
    }
}
