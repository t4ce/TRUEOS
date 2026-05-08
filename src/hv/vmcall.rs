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
use core::sync::atomic::{AtomicU32, Ordering};

// ── op codes (u32, written by guest before vmcall) ──────────────────────────
pub const OP_PRESERVE: u32 = 0x01; // snapshot + stop
pub const OP_PING: u32 = 0x02; // response_data = 0xCAFE_BABE
pub const OP_UNIX_TIME: u32 = 0x03; // response_data = unix seconds
pub const OP_YIELD: u32 = 0x04; // cooperative host yield point
pub const OP_SLEEP_MS: u32 = 0x05; // cooperative host sleep before resume
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

// ── response status codes (u32, written by host) ────────────────────────────
pub const STATUS_OK: u32 = 0;
pub const STATUS_UNKNOWN_OP: u32 = 1;
pub const STATUS_BAD_ARG: u32 = 2;
const MAX_GUEST_SLEEP_MS: u64 = 10_000;

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

// ── exec dispatch ────────────────────────────────────────────────────────────

/// Called from the vmexit loop on every VMCALL exit.
pub fn dispatch(vm_id: u8) -> DispatchOutcome {
    let Some((op, seq, arg0, arg1, req_len)) = read_request(vm_id) else {
        hvlogf(format_args!("hv: vm{} reporting: vmcall bad vm id", vm_id));
        return DispatchOutcome::Stop;
    };
    match op {
        OP_PRESERVE => {
            write_response(vm_id, seq, STATUS_OK, 0, 0);
            DispatchOutcome::Stop
        }
        OP_PING => {
            write_response(vm_id, seq, STATUS_OK, 0xCAFE_BABE, 0);
            DispatchOutcome::Resume
        }
        OP_UNIX_TIME => {
            let t = crate::r::net::ntp::current_unix_seconds().unwrap_or(0);
            write_response(vm_id, seq, STATUS_OK, t, 0);
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
                crate::t::net::https::cabi_net_fetch_bytes_start_host(url, TIMEOUT_MS, MAX_BYTES);
            if op_id == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, op_id as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_RESULT_LEN => {
            let rc = crate::t::net::https::cabi_net_fetch_bytes_result_len_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_BYTES_READ => {
            let Some(p) = host_ptr(vm_id) else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let out = unsafe { &mut (&mut (*p).payload)[..PAYLOAD_CAP] };
            let rc = crate::t::net::https::cabi_net_fetch_bytes_read_chunk_host(
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
            let rc = crate::t::net::https::cabi_net_fetch_bytes_discard_host(arg0 as u32);
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
                crate::t::net::https::cabi_net_fetch_start_host(url, path, TIMEOUT_MS, MAX_BYTES);
            if op_id == 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
            } else {
                write_response(vm_id, seq, STATUS_OK, op_id as u64, 0);
            }
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_FILE_RESULT => {
            let rc = crate::t::net::https::cabi_net_fetch_result_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_FETCH_FILE_DISCARD => {
            let rc = crate::t::net::https::cabi_net_fetch_discard_host(arg0 as u32);
            write_response(vm_id, seq, STATUS_OK, (rc as i64) as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_ARGS_COUNT => {
            let count = crate::r::io::env::arg_count();
            write_response(vm_id, seq, STATUS_OK, count as u64, 0);
            DispatchOutcome::Resume
        }
        OP_BP_ENV_ARG => {
            let Some(arg) = crate::r::io::env::arg(arg0 as usize) else {
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
            let Some(value) = crate::r::io::env::var(key) else {
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
