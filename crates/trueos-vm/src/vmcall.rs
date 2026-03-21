//! Guest-side vmcall protocol.
//!
//! Mirrors CommPage layout from src/hv/vmcall.rs.
//! Guest writes request fields, issues vmcall (synchronous — host handles
//! and vmresumes before this call returns), then reads back the response.

use core::sync::atomic::{AtomicU32, Ordering};

// ── op codes ─────────────────────────────────────────────────────────────────
pub const OP_PRESERVE: u32 = 0x01;
pub const OP_PING: u32 = 0x02;
pub const OP_UNIX_TIME: u32 = 0x03;
pub const OP_NET_TCP_WRITE: u32 = 0x10;
pub const OP_NET_TCP_READ: u32 = 0x11;

pub const STATUS_OK: u32 = 0;
pub const STATUS_BAD_ARG: u32 = 2;
pub const PAYLOAD_CAP: usize = 4096 - 56;

/// Guest virtual address of the shared comm page.
const COMM_PAGE_VA: u64 = 0x0000_0000_0041_0000;

#[repr(C)]
struct CommPage {
    request_op: u32,
    request_seq: u32,
    request_arg0: u64,
    request_arg1: u64,
    request_len: u32,
    request_pad: u32,
    response_seq: u32,
    response_status: u32,
    response_data: u64,
    response_len: u32,
    response_pad: u32,
    payload: [u8; PAYLOAD_CAP],
}

static SEQ: AtomicU32 = AtomicU32::new(1);

#[inline(always)]
fn page() -> *mut CommPage {
    COMM_PAGE_VA as *mut CommPage
}

/// Issue a vmcall and return (response_status, response_data).
/// Synchronous: host writes response before vmresume, so data is ready
/// on return.
pub fn call(op: u32, arg0: u64, arg1: u64) -> (u32, u64) {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = page();
    unsafe {
        core::ptr::write_volatile(&mut (*p).request_arg0, arg0);
        core::ptr::write_volatile(&mut (*p).request_arg1, arg1);
        core::ptr::write_volatile(&mut (*p).request_len, 0);
        core::ptr::write_volatile(&mut (*p).request_seq, seq);
        // op written last — host treats this as the trigger
        core::ptr::write_volatile(&mut (*p).request_op, op);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));
        // vmcall is synchronous; response is ready on return
        let status = core::ptr::read_volatile(&(*p).response_status);
        let data = core::ptr::read_volatile(&(*p).response_data);
        (status, data)
    }
}

pub fn call_with_payload(op: u32, arg0: u64, arg1: u64, req: &[u8], out: &mut [u8]) -> (u32, u64) {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = page();
    unsafe {
        let req_n = core::cmp::min(req.len(), PAYLOAD_CAP);
        if req_n != 0 {
            (&mut (&mut (*p).payload)[..req_n]).copy_from_slice(&req[..req_n]);
        }

        core::ptr::write_volatile(&mut (*p).request_arg0, arg0);
        core::ptr::write_volatile(&mut (*p).request_arg1, arg1);
        core::ptr::write_volatile(&mut (*p).request_len, req_n as u32);
        core::ptr::write_volatile(&mut (*p).request_seq, seq);
        core::ptr::write_volatile(&mut (*p).request_op, op);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));

        let status = core::ptr::read_volatile(&(*p).response_status);
        let data = core::ptr::read_volatile(&(*p).response_data);
        let resp_n = core::cmp::min(
            core::ptr::read_volatile(&(*p).response_len) as usize,
            core::cmp::min(out.len(), PAYLOAD_CAP),
        );
        if resp_n != 0 {
            out[..resp_n].copy_from_slice(&(&(*p).payload)[..resp_n]);
        }
        (status, data)
    }
}

pub fn ping() -> bool {
    let (s, d) = call(OP_PING, 0, 0);
    s == STATUS_OK && d == 0xCAFE_BABE
}

pub fn unix_time() -> u64 {
    let (_s, d) = call(OP_UNIX_TIME, 0, 0);
    d
}

pub fn net_tcp_write(bytes: &[u8]) -> usize {
    let mut total = 0usize;
    let mut out = [0u8; 1];
    while total < bytes.len() {
        let end = core::cmp::min(total + PAYLOAD_CAP, bytes.len());
        let (s, d) = call_with_payload(OP_NET_TCP_WRITE, 0, 0, &bytes[total..end], &mut out);
        if s != STATUS_OK {
            break;
        }
        let wrote = d as usize;
        if wrote == 0 {
            break;
        }
        total += wrote;
    }
    total
}

pub fn net_tcp_read(out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let want = core::cmp::min(out.len(), PAYLOAD_CAP);
    let (s, d) = call_with_payload(OP_NET_TCP_READ, want as u64, 0, &[], &mut out[..want]);
    if s == STATUS_BAD_ARG {
        return 0;
    }
    if s != STATUS_OK {
        return 0;
    }
    let got = core::cmp::min(d as usize, want);
    got
}

/// Signal host to snapshot and stop executing the guest.
/// This is the final call; the guest halts after this.
#[inline(never)]
pub fn preserve() {
    let p = page();
    unsafe {
        core::ptr::write_volatile(&mut (*p).request_len, 0);
        core::ptr::write_volatile(&mut (*p).request_seq, 0xFFFF_FFFF);
        core::ptr::write_volatile(&mut (*p).request_op, OP_PRESERVE);
        core::arch::asm!("vmcall", options(nostack, preserves_flags));
    }
}
