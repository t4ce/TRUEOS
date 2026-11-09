#![allow(dead_code, missing_docs)]
#![allow(missing_docs)]
use crate::{Interest, Registry, Token};

use core::net::SocketAddr;
use crate::io;

const STATUS_UNSUPPORTED: i32 = -1;
const STATUS_WOULD_BLOCK: i32 = -2;
const STATUS_NOT_CONNECTED: i32 = -3;
const STATUS_INVALID_INPUT: i32 = -4;
const STATUS_NOT_FOUND: i32 = -5;
const STATUS_IO: i32 = -6;
const STATUS_TIMED_OUT: i32 = -7;
const STATUS_NO_DEVICE: i32 = -8;
const STATUS_RESOURCE_EXHAUSTED: i32 = -9;
const STATUS_BROKEN_PIPE: i32 = -10;

pub(crate) const READY_READABLE: u8 = 0b0000_0001;
pub(crate) const READY_WRITABLE: u8 = 0b0000_0010;
pub(crate) const READY_ERROR: u8 = 0b0000_0100;
pub(crate) const READY_READ_CLOSED: u8 = 0b0000_1000;
pub(crate) const READY_WRITE_CLOSED: u8 = 0b0001_0000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SocketAddrRaw {
    family: u8,
    reserved: u8,
    port: u16,
    addr: [u8; 16],
}

const _: () = assert!(core::mem::size_of::<SocketAddrRaw>() == 20);

unsafe extern "C" {
    fn trueos_mio_tcp_socket_create(family: u32, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_tcp_socket_bind(socket_id: u32, addr: SocketAddrRaw) -> i32;
    fn trueos_mio_tcp_socket_connect(socket_id: u32, addr: SocketAddrRaw) -> i32;
    fn trueos_mio_tcp_socket_listen(socket_id: u32, backlog: u32) -> i32;
    fn trueos_mio_tcp_listener_bind(addr: SocketAddrRaw, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_tcp_stream_connect(addr: SocketAddrRaw, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_udp_socket_bind(addr: SocketAddrRaw, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_socket_close(socket_id: u32) -> i32;
    fn trueos_mio_socket_shutdown(socket_id: u32, how: u32) -> i32;
    fn trueos_mio_socket_option(socket_id: u32, option: u32, value: i32) -> i32;
    fn trueos_mio_socket_local_addr(socket_id: u32, out_addr: *mut SocketAddrRaw) -> i32;
    fn trueos_mio_socket_peer_addr(socket_id: u32, out_addr: *mut SocketAddrRaw) -> i32;
    fn trueos_mio_socket_take_error(socket_id: u32) -> i32;
    fn trueos_mio_tcp_stream_read(socket_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_mio_tcp_stream_peek(socket_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
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
    fn trueos_mio_udp_socket_peek_from(
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
    fn trueos_mio_socket_poll_ready(socket_id: u32, interests: u8) -> i32;
}

#[derive(Debug)]
pub(crate) struct Socket {
    id: u32,
    registration: Option<crate::sys::SocketRegistration>,
    // Retain the original std capability; registration_id is borrowed only.
    // Its Arc also keeps independently held std listener clones alive.
    #[cfg(all(target_os = "trueos", feature = "std-compat"))]
    std_owner: Option<std::net::NativeSocket>,
}

impl Drop for Socket {
    fn drop(&mut self) {
        // Remove local readiness state before releasing the kernel ID. A later
        // socket can therefore never inherit this source's token on ID reuse.
        drop(self.registration.take());
        #[cfg(all(target_os = "trueos", feature = "std-compat"))]
        if self.std_owner.is_some() {
            return; // NativeSocket's final Arc owns the close.
        }
        if self.id != 0 {
            let _ = unsafe { trueos_mio_socket_close(self.id) };
        }
    }
}

impl Socket {
    fn owned(id: u32) -> Self {
        Self { id, registration: None,
            #[cfg(all(target_os = "trueos", feature = "std-compat"))]
            std_owner: None,
        }
    }

    #[cfg(all(target_os = "trueos", feature = "std-compat"))]
    pub(crate) fn from_std_native(owner: std::net::NativeSocket) -> Self {
        Self { id: owner.registration_id(), registration: None, std_owner: Some(owner) }
    }

    #[cfg(all(target_os = "trueos", feature = "std-compat"))]
    pub(crate) fn into_std_native(mut self) -> io::Result<std::net::NativeSocket> {
        // Removing the reactor token must precede returning ownership to std.
        drop(self.registration.take());
        match self.std_owner.take() {
            Some(owner) => {
                self.id = 0; // the returned NativeSocket owns this capability
                Ok(owner)
            }
            None => Err(io::Error::new(io::ErrorKind::Unsupported,
                "native-created Mio sockets have no retained std capability to return")),
        }
    }
    pub(crate) fn tcp_socket(family: u32) -> io::Result<Self> {
        let mut id = 0;
        let status = unsafe { trueos_mio_tcp_socket_create(family, &mut id) };
        status_to_result(status, "native TCP socket creation failed")?;
        Ok(Self::owned(id))
    }

    pub(crate) fn bind(&self, addr: SocketAddr) -> io::Result<()> {
        let status = unsafe { trueos_mio_tcp_socket_bind(self.id, socket_addr_to_raw(addr)?) };
        status_to_result(status, "native TCP endpoint reservation failed")
    }

    pub(crate) fn connect(&self, addr: SocketAddr) -> io::Result<()> {
        let status = unsafe { trueos_mio_tcp_socket_connect(self.id, socket_addr_to_raw(addr)?) };
        status_to_result(status, "native TCP socket connect failed")
    }

    pub(crate) fn listen(&self, backlog: u32) -> io::Result<()> {
        let status = unsafe { trueos_mio_tcp_socket_listen(self.id, backlog) };
        status_to_result(status, "native TCP socket listen failed")
    }

    pub(crate) fn tcp_listener_bind(addr: SocketAddr) -> io::Result<Self> {
        let mut id = 0u32;
        let status = unsafe { trueos_mio_tcp_listener_bind(socket_addr_to_raw(addr)?, &mut id) };
        status_to_result(status, "mio zkvm TCP listener bind failed")?;
        Ok(Self::owned(id))
    }

    pub(crate) fn tcp_stream_connect(addr: SocketAddr) -> io::Result<Self> {
        let mut id = 0u32;
        let status = unsafe { trueos_mio_tcp_stream_connect(socket_addr_to_raw(addr)?, &mut id) };
        status_to_result(status, "mio zkvm TCP stream connect failed")?;
        Ok(Self::owned(id))
    }

    pub(crate) fn udp_bind(addr: SocketAddr) -> io::Result<Self> {
        let mut id = 0u32;
        let status = unsafe { trueos_mio_udp_socket_bind(socket_addr_to_raw(addr)?, &mut id) };
        status_to_result(status, "mio zkvm UDP bind failed")?;
        Ok(Self::owned(id))
    }

    pub(crate) fn accept(&self) -> io::Result<(Self, SocketAddr)> {
        let mut id = 0u32;
        let mut addr = SocketAddrRaw::default();
        let status = unsafe { trueos_mio_tcp_listener_accept(self.id, &mut id, &mut addr) };
        self.rearm_if_blocked(status as isize, READY_READABLE);
        status_to_result(status, "mio zkvm TCP accept failed")?;
        Ok((Self::owned(id), raw_to_socket_addr(addr)?))
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
        if let Some(registration) = &self.registration { registration.rearm(READY_ERROR); }
        if status == 0 {
            Ok(None)
        } else {
            Ok(Some(status_to_error(status, "mio zkvm socket error")))
        }
    }

    pub(crate) fn shutdown(&self, how: u32) -> io::Result<()> {
        // Shutdown changes the stream directions, not its owned socket ID.
        // Keep that ID alive for the remaining direction and close it on Drop.
        let status = unsafe { trueos_mio_socket_shutdown(self.id, how) };
        status_to_result(status, "mio zkvm socket shutdown failed")
    }

    pub(crate) fn option(&self, option: u32, value: Option<u32>) -> io::Result<u32> {
        let value = match value {
            Some(value) => i32::try_from(value).map_err(|_| io::ErrorKind::InvalidInput)?,
            None => -1,
        };
        let status = unsafe { trueos_mio_socket_option(self.id, option, value) };
        if status >= 0 { Ok(status as u32) }
        else { Err(status_to_error(status, "native socket option failed")) }
    }

    pub(crate) fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let status = unsafe { trueos_mio_tcp_stream_read(self.id, buf.as_mut_ptr(), buf.len()) };
        self.rearm_if_blocked(status, READY_READABLE);
        read_write_result(status, "mio zkvm tcp read failed")
    }

    pub(crate) fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        let status = unsafe { trueos_mio_tcp_stream_peek(self.id, buf.as_mut_ptr(), buf.len()) };
        self.rearm_if_blocked(status, READY_READABLE);
        read_write_result(status, "mio zkvm tcp peek failed")
    }

    pub(crate) fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let status = unsafe { trueos_mio_tcp_stream_write(self.id, buf.as_ptr(), buf.len()) };
        self.rearm_if_blocked(status, READY_WRITABLE);
        read_write_result(status, "mio zkvm tcp write failed")
    }

    pub(crate) fn udp_connect(&self, addr: SocketAddr) -> io::Result<()> {
        let status = unsafe { trueos_mio_udp_socket_connect(self.id, socket_addr_to_raw(addr)?) };
        status_to_result(status, "mio zkvm udp connect failed")?;
        // Replacing a peer also replaces the kernel's queued receive set.
        if let Some(registration) = &self.registration { registration.rearm(READY_READABLE); }
        Ok(())
    }

    pub(crate) fn udp_send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        let status = unsafe {
            trueos_mio_udp_socket_send_to(
                self.id,
                socket_addr_to_raw(addr)?,
                buf.as_ptr(),
                buf.len(),
            )
        };
        self.rearm_if_blocked(status, READY_WRITABLE);
        read_write_result(status, "mio zkvm udp send failed")
    }

    pub(crate) fn udp_recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut addr = SocketAddrRaw::default();
        let status = unsafe {
            trueos_mio_udp_socket_recv_from(self.id, &mut addr, buf.as_mut_ptr(), buf.len())
        };
        self.rearm_if_blocked(status, READY_READABLE);
        Ok((read_write_result(status, "mio zkvm udp recv failed")?, raw_to_socket_addr(addr)?))
    }

    pub(crate) fn udp_peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut addr = SocketAddrRaw::default();
        let status = unsafe {
            trueos_mio_udp_socket_peek_from(self.id, &mut addr, buf.as_mut_ptr(), buf.len())
        };
        self.rearm_if_blocked(status, READY_READABLE);
        Ok((read_write_result(status, "native UDP peek failed")?, raw_to_socket_addr(addr)?))
    }

    pub(crate) fn register(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        if self.registration.as_ref().is_some_and(|entry| entry.is_live()) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        self.registration = Some(registry.selector().register_socket(self.id, token, interests)?);
        Ok(())
    }

    pub(crate) fn reregister(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        self.registration.as_mut().ok_or(io::ErrorKind::NotFound)?
            .reregister(registry.selector(), token, interests)
    }

    pub(crate) fn deregister(&mut self, registry: &Registry) -> io::Result<()> {
        self.registration.as_ref().ok_or(io::ErrorKind::NotFound)?.deregister(registry.selector())?;
        drop(self.registration.take());
        Ok(())
    }

    fn rearm_if_blocked(&self, status: isize, direction: u8) {
        if status == STATUS_WOULD_BLOCK as isize {
            if let Some(registration) = &self.registration { registration.rearm(direction); }
        }
    }
}

pub(crate) fn socket_ready(socket_id: u32, interests: Interest) -> io::Result<u8> {
    let status = unsafe { trueos_mio_socket_poll_ready(socket_id, interest_bits(interests)) };
    if status >= 0 {
        Ok(status as u8)
    } else {
        Err(status_to_error(status, "mio native readiness query failed"))
    }
}

fn socket_addr_to_raw(addr: SocketAddr) -> io::Result<SocketAddrRaw> {
    match addr {
        SocketAddr::V4(addr) => {
            let mut raw = SocketAddrRaw {
                family: 4,
                reserved: 0,
                port: addr.port(),
                addr: [0; 16],
            };
            raw.addr[..4].copy_from_slice(&addr.ip().octets());
            Ok(raw)
        }
        // The native address record has no scope or flow fields. Preserve the
        // caller's meaning by refusing values the service cannot represent.
        SocketAddr::V6(addr) if addr.scope_id() != 0 || addr.flowinfo() != 0 => {
            Err(io::ErrorKind::InvalidInput.into())
        }
        SocketAddr::V6(addr) => Ok(SocketAddrRaw {
            family: 6,
            reserved: 0,
            port: addr.port(),
            addr: addr.ip().octets(),
        }),
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

#[inline(never)]
fn status_to_result(status: i32, detail: &'static str) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(status_to_error(status, detail))
    }
}

#[inline(never)]
fn read_write_result(status: isize, detail: &'static str) -> io::Result<usize> {
    if status >= 0 {
        Ok(status as usize)
    } else {
        Err(status_to_error(status as i32, detail))
    }
}

#[inline(never)]
fn status_to_error(status: i32, _detail: &'static str) -> io::Error {
    let kind = match status {
        STATUS_UNSUPPORTED => io::ErrorKind::Other,
        STATUS_WOULD_BLOCK => io::ErrorKind::WouldBlock,
        STATUS_NOT_CONNECTED => io::ErrorKind::NotConnected,
        STATUS_INVALID_INPUT => io::ErrorKind::InvalidInput,
        STATUS_NOT_FOUND => io::ErrorKind::NotFound,
        STATUS_TIMED_OUT => io::ErrorKind::TimedOut,
        STATUS_NO_DEVICE => io::ErrorKind::NotFound,
        STATUS_RESOURCE_EXHAUSTED => io::ErrorKind::Other,
        STATUS_BROKEN_PIPE => io::ErrorKind::BrokenPipe,
        STATUS_IO => io::ErrorKind::Other,
        _ => io::ErrorKind::Other,
    };
    kind.into()
}
