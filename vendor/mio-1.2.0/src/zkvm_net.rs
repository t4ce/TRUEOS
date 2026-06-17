#![allow(dead_code, missing_docs)]
#![allow(missing_docs)]
use crate::{Interest, Registry, Token};

use alloc::{vec, vec::Vec};
use core::net::SocketAddr;
use trueos_io as io;

const STATUS_UNSUPPORTED: i32 = -1;
const STATUS_WOULD_BLOCK: i32 = -2;
const STATUS_NOT_CONNECTED: i32 = -3;
const STATUS_INVALID_INPUT: i32 = -4;
const STATUS_NOT_FOUND: i32 = -5;
const STATUS_IO: i32 = -6;
const STATUS_TIMED_OUT: i32 = -7;
const STATUS_NO_DEVICE: i32 = -8;

pub(crate) const READY_READABLE: u8 = 0b0000_0001;
pub(crate) const READY_WRITABLE: u8 = 0b0000_0010;
pub(crate) const READY_ERROR: u8 = 0b0000_0100;
pub(crate) const READY_READ_CLOSED: u8 = 0b0000_1000;
pub(crate) const READY_WRITE_CLOSED: u8 = 0b0001_0000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct ReadyEventRaw {
    pub token: usize,
    pub readiness: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SocketAddrRaw {
    family: u8,
    port: u16,
    addr: [u8; 16],
}

unsafe extern "C" {
    fn trueos_mio_tcp_listener_bind(addr: SocketAddrRaw, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_tcp_stream_connect(addr: SocketAddrRaw, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_udp_socket_bind(addr: SocketAddrRaw, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_socket_close(socket_id: u32) -> i32;
    fn trueos_mio_socket_local_addr(socket_id: u32, out_addr: *mut SocketAddrRaw) -> i32;
    fn trueos_mio_socket_peer_addr(socket_id: u32, out_addr: *mut SocketAddrRaw) -> i32;
    fn trueos_mio_socket_take_error(socket_id: u32) -> i32;
    fn trueos_mio_tcp_stream_read(socket_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_mio_tcp_stream_write(socket_id: u32, data_ptr: *const u8, data_len: usize) -> isize;
    fn trueos_mio_udp_socket_connect(socket_id: u32, addr: SocketAddrRaw) -> i32;
    fn trueos_mio_udp_socket_send_to(
        socket_id: u32,
        addr: SocketAddrRaw,
        data_ptr: *const u8,
        data_len: usize,
    ) -> isize;
    fn trueos_mio_udp_socket_recv_from(
        socket_id: u32,
        out_addr: *mut SocketAddrRaw,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_mio_tcp_listener_accept(
        socket_id: u32,
        out_socket_id: *mut u32,
        out_addr: *mut SocketAddrRaw,
    ) -> i32;
    fn trueos_mio_selector_register_socket(
        selector_id: usize,
        socket_id: u32,
        token: usize,
        interests: u8,
    ) -> i32;
    fn trueos_mio_selector_deregister_socket(selector_id: usize, socket_id: u32) -> i32;
    fn trueos_mio_selector_poll(
        selector_id: usize,
        out_events: *mut ReadyEventRaw,
        out_cap: usize,
        timeout_nanos: u64,
    ) -> usize;
    fn trueos_mio_selector_wake(selector_id: usize) -> i32;
}

#[derive(Debug)]
pub(crate) struct Socket {
    id: u32,
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = unsafe { trueos_mio_socket_close(self.id) };
    }
}

impl Socket {
    pub(crate) fn tcp_listener_bind(addr: SocketAddr) -> io::Result<Self> {
        let mut id = 0u32;
        let status = unsafe { trueos_mio_tcp_listener_bind(socket_addr_to_raw(addr), &mut id) };
        status_to_result(status, "mio zkvm TCP listener bind failed")?;
        Ok(Self { id })
    }

    pub(crate) fn tcp_stream_connect(addr: SocketAddr) -> io::Result<Self> {
        let mut id = 0u32;
        let status = unsafe { trueos_mio_tcp_stream_connect(socket_addr_to_raw(addr), &mut id) };
        status_to_result(status, "mio zkvm TCP stream connect failed")?;
        Ok(Self { id })
    }

    pub(crate) fn udp_bind(addr: SocketAddr) -> io::Result<Self> {
        let mut id = 0u32;
        let status = unsafe { trueos_mio_udp_socket_bind(socket_addr_to_raw(addr), &mut id) };
        status_to_result(status, "mio zkvm UDP bind failed")?;
        Ok(Self { id })
    }

    pub(crate) fn accept(&self) -> io::Result<(Self, SocketAddr)> {
        let mut id = 0u32;
        let mut addr = SocketAddrRaw::default();
        let status = unsafe { trueos_mio_tcp_listener_accept(self.id, &mut id, &mut addr) };
        status_to_result(status, "mio zkvm TCP accept failed")?;
        Ok((Self { id }, raw_to_socket_addr(addr)?))
    }

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        let mut addr = SocketAddrRaw::default();
        let status = unsafe { trueos_mio_socket_local_addr(self.id, &mut addr) };
        status_to_result(status, "mio zkvm local_addr failed")?;
        raw_to_socket_addr(addr)
    }

    pub(crate) fn peer_addr(&self) -> io::Result<SocketAddr> {
        let mut addr = SocketAddrRaw::default();
        let status = unsafe { trueos_mio_socket_peer_addr(self.id, &mut addr) };
        status_to_result(status, "mio zkvm peer_addr failed")?;
        raw_to_socket_addr(addr)
    }

    pub(crate) fn take_error(&self) -> io::Result<Option<io::Error>> {
        let status = unsafe { trueos_mio_socket_take_error(self.id) };
        if status == 0 {
            Ok(None)
        } else {
            Ok(Some(status_to_error(status, "mio zkvm socket error")))
        }
    }

    pub(crate) fn shutdown(&self) -> io::Result<()> {
        let status = unsafe { trueos_mio_socket_close(self.id) };
        status_to_result(status, "mio zkvm socket shutdown failed")
    }

    pub(crate) fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let status = unsafe { trueos_mio_tcp_stream_read(self.id, buf.as_mut_ptr(), buf.len()) };
        read_write_result(status, "mio zkvm tcp read failed")
    }

    pub(crate) fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let status = unsafe { trueos_mio_tcp_stream_write(self.id, buf.as_ptr(), buf.len()) };
        read_write_result(status, "mio zkvm tcp write failed")
    }

    pub(crate) fn udp_connect(&self, addr: SocketAddr) -> io::Result<()> {
        let status = unsafe { trueos_mio_udp_socket_connect(self.id, socket_addr_to_raw(addr)) };
        status_to_result(status, "mio zkvm udp connect failed")
    }

    pub(crate) fn udp_send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        let status = unsafe {
            trueos_mio_udp_socket_send_to(
                self.id,
                socket_addr_to_raw(addr),
                buf.as_ptr(),
                buf.len(),
            )
        };
        read_write_result(status, "mio zkvm udp send failed")
    }

    pub(crate) fn udp_recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut addr = SocketAddrRaw::default();
        let status = unsafe {
            trueos_mio_udp_socket_recv_from(self.id, &mut addr, buf.as_mut_ptr(), buf.len())
        };
        Ok((read_write_result(status, "mio zkvm udp recv failed")?, raw_to_socket_addr(addr)?))
    }

    pub(crate) fn register(
        &self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        let status = unsafe {
            trueos_mio_selector_register_socket(
                registry.selector().id(),
                self.id,
                token.0,
                interest_bits(interests),
            )
        };
        status_to_result(status, "mio zkvm socket registration failed")
    }

    pub(crate) fn reregister(
        &self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        self.register(registry, token, interests)
    }

    pub(crate) fn deregister(&self, registry: &Registry) -> io::Result<()> {
        let status =
            unsafe { trueos_mio_selector_deregister_socket(registry.selector().id(), self.id) };
        status_to_result(status, "mio zkvm socket deregistration failed")
    }
}

pub(crate) fn selector_poll(
    selector_id: usize,
    out_events: &mut Vec<ReadyEventRaw>,
    timeout: Option<core::time::Duration>,
) {
    let cap = out_events.capacity();
    out_events.clear();
    if cap == 0 {
        return;
    }
    let mut raw = vec![ReadyEventRaw::default(); cap];
    let timeout_nanos = timeout
        .map(crate::zkvm_compat::duration_to_nanos)
        .unwrap_or(u64::MAX);
    let len = unsafe {
        trueos_mio_selector_poll(selector_id, raw.as_mut_ptr(), raw.len(), timeout_nanos)
    };
    out_events.extend(raw.into_iter().take(len));
}

pub(crate) fn selector_wake(selector_id: usize) {
    let _ = unsafe { trueos_mio_selector_wake(selector_id) };
}

fn socket_addr_to_raw(addr: SocketAddr) -> SocketAddrRaw {
    match addr {
        SocketAddr::V4(addr) => {
            let mut raw = SocketAddrRaw {
                family: 4,
                port: addr.port(),
                addr: [0; 16],
            };
            raw.addr[..4].copy_from_slice(&addr.ip().octets());
            raw
        }
        SocketAddr::V6(addr) => SocketAddrRaw {
            family: 6,
            port: addr.port(),
            addr: addr.ip().octets(),
        },
    }
}

fn raw_to_socket_addr(raw: SocketAddrRaw) -> io::Result<SocketAddr> {
    match raw.family {
        4 => Ok(SocketAddr::from(([raw.addr[0], raw.addr[1], raw.addr[2], raw.addr[3]], raw.port))),
        6 => Ok(SocketAddr::from((raw.addr, raw.port))),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mio zkvm invalid socket address family",
        )),
    }
}

fn interest_bits(interests: Interest) -> u8 {
    let mut bits = 0u8;
    if interests.is_readable() {
        bits |= READY_READABLE;
    }
    if interests.is_writable() {
        bits |= READY_WRITABLE;
    }
    bits
}

fn status_to_result(status: i32, detail: &'static str) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(status_to_error(status, detail))
    }
}

fn read_write_result(status: isize, detail: &'static str) -> io::Result<usize> {
    if status >= 0 {
        Ok(status as usize)
    } else {
        Err(status_to_error(status as i32, detail))
    }
}

fn status_to_error(status: i32, detail: &'static str) -> io::Error {
    let kind = match status {
        STATUS_UNSUPPORTED => io::ErrorKind::Other,
        STATUS_WOULD_BLOCK => io::ErrorKind::WouldBlock,
        STATUS_NOT_CONNECTED => io::ErrorKind::NotConnected,
        STATUS_INVALID_INPUT => io::ErrorKind::InvalidInput,
        STATUS_NOT_FOUND => io::ErrorKind::NotFound,
        STATUS_TIMED_OUT => io::ErrorKind::TimedOut,
        STATUS_NO_DEVICE => io::ErrorKind::NotFound,
        STATUS_IO => io::ErrorKind::Other,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, detail)
}
