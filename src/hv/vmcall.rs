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

// ── op codes (u32, written by guest before vmcall) ──────────────────────────
pub const OP_PRESERVE: u32 = 0x01; // snapshot + stop
pub const OP_PING: u32 = 0x02; // response_data = 0xCAFE_BABE
pub const OP_UNIX_TIME: u32 = 0x03; // response_data = unix seconds
pub const OP_NET_TCP_WRITE: u32 = 0x10; // request payload -> net tcp shell tx
pub const OP_NET_TCP_READ: u32 = 0x11; // net tcp shell rx -> response payload

// ── response status codes (u32, written by host) ────────────────────────────
pub const STATUS_OK: u32 = 0;
pub const STATUS_UNKNOWN_OP: u32 = 1;
pub const STATUS_BAD_ARG: u32 = 2;

// ── shared page ─────────────────────────────────────────────────────────────

/// Guest virtual address of the comm page.
/// Sits immediately after the 64 KB guest stack (0x400000 + 0x10000).
pub const COMM_PAGE_GUEST_VA: u64 = 0x0000_0000_0041_0000;
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

/// Static 4 K backing page for CommPage.
#[repr(C, align(4096))]
pub struct CommPageBacking([u8; 4096]);

pub static mut COMM_PAGE: CommPageBacking = CommPageBacking([0u8; 4096]);

#[inline]
fn host_ptr() -> *mut CommPage {
    unsafe { core::ptr::addr_of_mut!(COMM_PAGE.0) as *mut CommPage }
}

pub fn pa() -> Option<u64> {
    let va = unsafe { core::ptr::addr_of!(COMM_PAGE.0) as u64 };
    kernel_va_to_pa(va)
}

// ── transport helpers ────────────────────────────────────────────────────────

fn read_request() -> (u32, u32, u64, u64, u32) {
    let p = host_ptr();
    unsafe {
        (
            core::ptr::read_volatile(&(*p).request_op),
            core::ptr::read_volatile(&(*p).request_seq),
            core::ptr::read_volatile(&(*p).request_arg0),
            core::ptr::read_volatile(&(*p).request_arg1),
            core::ptr::read_volatile(&(*p).request_len),
        )
    }
}

fn write_response(seq: u32, status: u32, data: u64, len: u32) {
    let p = host_ptr();
    unsafe {
        core::ptr::write_volatile(&mut (*p).response_status, status);
        core::ptr::write_volatile(&mut (*p).response_data, data);
        core::ptr::write_volatile(&mut (*p).response_len, len);
        // seq written last — guest may poll this as a completion flag
        core::ptr::write_volatile(&mut (*p).response_seq, seq);
    }
}

// ── exec dispatch ────────────────────────────────────────────────────────────

/// Called from the vmexit loop on every VMCALL exit.
/// Returns `true` if the guest should be resumed, `false` to stop the loop.
pub fn dispatch() -> bool {
    let (op, seq, arg0, _arg1, req_len) = read_request();
    match op {
        OP_PRESERVE => {
            write_response(seq, STATUS_OK, 0, 0);
            false
        }
        OP_PING => {
            write_response(seq, STATUS_OK, 0xCAFE_BABE, 0);
            true
        }
        OP_UNIX_TIME => {
            let t = crate::r::net::ntp::current_unix_seconds().unwrap_or(0);
            write_response(seq, STATUS_OK, t, 0);
            true
        }
        OP_NET_TCP_WRITE => {
            let n = core::cmp::min(req_len as usize, PAYLOAD_CAP);
            let p = host_ptr();
            let bytes = unsafe { &(&(*p).payload)[..n] };
            crate::shell2::backends::net_tcp::net_shell_write_bytes(bytes);
            write_response(seq, STATUS_OK, n as u64, 0);
            true
        }
        OP_NET_TCP_READ => {
            let want = core::cmp::min(arg0 as usize, PAYLOAD_CAP);
            if want == 0 {
                write_response(seq, STATUS_BAD_ARG, 0, 0);
                return true;
            }

            let p = host_ptr();
            let mut got = 0usize;
            while got < want {
                if let Some(b) = crate::shell2::backends::net_tcp::net_shell_read_byte() {
                    unsafe {
                        (*p).payload[got] = b;
                    }
                    got += 1;
                } else {
                    break;
                }
            }
            write_response(seq, STATUS_OK, got as u64, got as u32);
            true
        }
        _ => {
            hvlogf(format_args!("hv: vmcall unknown op=0x{:02X} seq={}", op, seq));
            write_response(seq, STATUS_UNKNOWN_OP, 0, 0);
            true
        }
    }
}
