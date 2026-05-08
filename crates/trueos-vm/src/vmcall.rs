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
pub const OP_YIELD: u32 = 0x04;
pub const OP_SLEEP_MS: u32 = 0x05;
pub const OP_RAND_BYTES: u32 = 0x06;
pub const OP_NET_TCP_WRITE: u32 = 0x10;
pub const OP_NET_TCP_READ: u32 = 0x11;
pub const OP_BP_NET_OPEN: u32 = 0x20;
pub const OP_BP_NET_SUBMIT: u32 = 0x21;
pub const OP_BP_NET_POLL: u32 = 0x22;
pub const OP_BP_FETCH_BYTES_START: u32 = 0x23;
pub const OP_BP_FETCH_BYTES_RESULT_LEN: u32 = 0x24;
pub const OP_BP_FETCH_BYTES_READ: u32 = 0x25;
pub const OP_BP_FETCH_BYTES_DISCARD: u32 = 0x26;
pub const OP_BP_FETCH_FILE_START: u32 = 0x27;
pub const OP_BP_FETCH_FILE_RESULT: u32 = 0x28;
pub const OP_BP_FETCH_FILE_DISCARD: u32 = 0x29;
pub const OP_BP_ENV_ARGS_COUNT: u32 = 0x2A;
pub const OP_BP_ENV_ARG: u32 = 0x2B;
pub const OP_BP_ENV_VAR: u32 = 0x2C;
pub const OP_BP_FS_READ_FILE: u32 = 0x2D;
pub const OP_BP_FS_WRITE_BEGIN: u32 = 0x2E;
pub const OP_BP_FS_WRITE_CHUNK: u32 = 0x2F;
pub const OP_BP_FS_WRITE_FINISH: u32 = 0x30;
pub const OP_BP_FS_WRITE_ABORT: u32 = 0x31;
pub const OP_BP_FS_CREATE_DIR_ALL: u32 = 0x32;
pub const OP_BP_FS_EXISTS: u32 = 0x33;
pub const OP_BP_FS_REMOVE: u32 = 0x34;
pub const OP_BP_SOCKET_TCP_OPEN: u32 = 0x35;
pub const OP_BP_SOCKET_TCP_CLOSE: u32 = 0x36;
pub const OP_BP_SOCKET_TCP_SET_NONBLOCKING: u32 = 0x37;
pub const OP_BP_SOCKET_TCP_BIND_V4: u32 = 0x38;
pub const OP_BP_SOCKET_TCP_BIND_V6: u32 = 0x39;
pub const OP_BP_SOCKET_TCP_CONNECT_V4: u32 = 0x3A;
pub const OP_BP_SOCKET_TCP_CONNECT_V6: u32 = 0x3B;
pub const OP_BP_SOCKET_TCP_POLL_CONNECT: u32 = 0x3C;
pub const OP_BP_SOCKET_TCP_SEND: u32 = 0x3D;
pub const OP_BP_SOCKET_TCP_RECV: u32 = 0x3E;
pub const OP_BP_SOCKET_TCP_SHUTDOWN: u32 = 0x3F;
pub const OP_BP_SOCKET_TCP_TAKE_ERROR: u32 = 0x40;
pub const OP_BP_SOCKET_TCP_PEER_V4: u32 = 0x41;
pub const OP_BP_SOCKET_TCP_PEER_V6: u32 = 0x42;
pub const OP_BP_MIO_TCP_LISTENER_BIND: u32 = 0x50;
pub const OP_BP_MIO_TCP_STREAM_CONNECT: u32 = 0x51;
pub const OP_BP_MIO_UDP_SOCKET_BIND: u32 = 0x52;
pub const OP_BP_MIO_SOCKET_CLOSE: u32 = 0x53;
pub const OP_BP_MIO_SOCKET_LOCAL_ADDR: u32 = 0x54;
pub const OP_BP_MIO_SOCKET_PEER_ADDR: u32 = 0x55;
pub const OP_BP_MIO_SOCKET_TAKE_ERROR: u32 = 0x56;
pub const OP_BP_MIO_TCP_STREAM_READ: u32 = 0x57;
pub const OP_BP_MIO_TCP_STREAM_WRITE: u32 = 0x58;
pub const OP_BP_MIO_UDP_SOCKET_CONNECT: u32 = 0x59;
pub const OP_BP_MIO_UDP_SOCKET_SEND_TO: u32 = 0x5A;
pub const OP_BP_MIO_UDP_SOCKET_RECV_FROM: u32 = 0x5B;
pub const OP_BP_MIO_TCP_LISTENER_ACCEPT: u32 = 0x5C;
pub const OP_BP_MIO_SELECTOR_REGISTER_SOCKET: u32 = 0x5D;
pub const OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET: u32 = 0x5E;
pub const OP_BP_MIO_SELECTOR_POLL: u32 = 0x5F;

pub const STATUS_OK: u32 = 0;
pub const STATUS_BAD_ARG: u32 = 2;
pub const PAYLOAD_CAP: usize = 4096 - 56;

/// Guest virtual address of the shared comm page.
const COMM_PAGE_VA: u64 = 0x0000_0000_2040_0000;

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

pub fn hull_bss_anchor() -> u64 {
    core::ptr::addr_of!(SEQ) as u64
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

pub fn yield_now() {
    let _ = call(OP_YIELD, 0, 0);
}

pub fn sleep_ms(ms: u64) {
    let _ = call(OP_SLEEP_MS, ms, 0);
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
