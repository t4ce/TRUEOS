//! Native synchronous network capabilities, shared with the Mio transport.
//! Native socket IDs are never Unix descriptors. Clones share ownership/options.
use crate::io::{self, BorrowedCursor, IoSlice, IoSliceMut};
use crate::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, SocketAddrV6, ToSocketAddrs};
use crate::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use crate::time::{Duration, Instant};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Address {
    family: u8,
    reserved: u8,
    port: u16,
    addr: [u8; 16],
}
const _: () = assert!(crate::mem::size_of::<Address>() == 20);

unsafe extern "C" {
    fn trueos_mio_tcp_socket_create(family: u32, out: *mut u32) -> i32;
    fn trueos_mio_tcp_socket_bind(id: u32, addr: Address) -> i32;
    fn trueos_mio_tcp_socket_connect(id: u32, addr: Address) -> i32;
    fn trueos_mio_tcp_socket_listen(id: u32, backlog: u32) -> i32;
    fn trueos_mio_tcp_listener_bind(addr: Address, out: *mut u32) -> i32;
    fn trueos_mio_tcp_stream_connect(addr: Address, out: *mut u32) -> i32;
    fn trueos_mio_udp_socket_bind(addr: Address, out: *mut u32) -> i32;
    fn trueos_mio_socket_close(id: u32) -> i32;
    fn trueos_mio_socket_shutdown(id: u32, how: u32) -> i32;
    fn trueos_mio_socket_option(id: u32, option: u32, value: i32) -> i32;
    fn trueos_mio_socket_local_addr(id: u32, out: *mut Address) -> i32;
    fn trueos_mio_socket_peer_addr(id: u32, out: *mut Address) -> i32;
    fn trueos_mio_socket_take_error(id: u32) -> i32;
    fn trueos_mio_socket_poll_ready(id: u32, interests: u8) -> i32;
    fn trueos_mio_tcp_stream_read(id: u32, out: *mut u8, len: usize) -> isize;
    fn trueos_mio_tcp_stream_peek(id: u32, out: *mut u8, len: usize) -> isize;
    fn trueos_mio_tcp_stream_write(id: u32, data: *const u8, len: usize) -> isize;
    fn trueos_mio_tcp_listener_accept(id: u32, out: *mut u32, addr: *mut Address) -> i32;
    fn trueos_mio_udp_socket_connect(id: u32, addr: Address) -> i32;
    fn trueos_mio_udp_socket_send_to(id: u32, addr: Address, data: *const u8, len: usize) -> isize;
    fn trueos_mio_udp_socket_recv_from(
        id: u32,
        addr: *mut Address,
        out: *mut u8,
        len: usize,
    ) -> isize;
    fn trueos_mio_udp_socket_peek_from(
        id: u32,
        addr: *mut Address,
        out: *mut u8,
        len: usize,
    ) -> isize;
    fn trueos_cabi_dns_resolve_ipv4(host: *const u8, len: usize, out: *mut u8) -> i32;
}
fn error(status: i32) -> io::Error {
    io::Error::new(
        match status {
            -1 => io::ErrorKind::Unsupported,
            -2 => io::ErrorKind::WouldBlock,
            -3 => io::ErrorKind::NotConnected,
            -4 => io::ErrorKind::InvalidInput,
            -5 => io::ErrorKind::NotFound,
            -7 => io::ErrorKind::TimedOut,
            -8 => io::ErrorKind::NotConnected,
            -9 => io::ErrorKind::OutOfMemory,
            -10 => io::ErrorKind::BrokenPipe,
            -11 => io::ErrorKind::PermissionDenied,
            _ => io::ErrorKind::Other,
        },
        "native network operation failed",
    )
}
fn status(value: i32) -> io::Result<()> {
    if value < 0 { Err(error(value)) } else { Ok(()) }
}
fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native socket option or transport is unavailable",
    ))
}
fn raw(addr: SocketAddr) -> io::Result<Address> {
    let mut out = Address {
        port: addr.port(),
        ..Address::default()
    };
    match addr {
        SocketAddr::V4(a) => {
            out.family = 4;
            out.addr[..4].copy_from_slice(&a.ip().octets());
        }
        SocketAddr::V6(a) => {
            if a.scope_id() != 0 || a.flowinfo() != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "native socket address does not support IPv6 flowinfo or scope",
                ));
            }
            out.family = 6;
            out.addr = a.ip().octets();
        }
    }
    Ok(out)
}
fn address(addr: Address) -> io::Result<SocketAddr> {
    match addr.family {
        4 => Ok(SocketAddr::from((
            [addr.addr[0], addr.addr[1], addr.addr[2], addr.addr[3]],
            addr.port,
        ))),
        6 => Ok(SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(addr.addr),
            addr.port,
            0,
            0,
        ))),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid native network address",
        )),
    }
}
#[derive(Debug)]
struct Inner {
    id: u32,
    nonblocking: AtomicBool,
    read_timeout: AtomicU64,
    write_timeout: AtomicU64,
}
impl Drop for Inner {
    fn drop(&mut self) {
        let _ = unsafe { trueos_mio_socket_close(self.id) };
    }
}
/// An owned native socket capability. This type has no Unix descriptor conversion.
#[derive(Clone, Debug)]
pub struct NativeSocket(Arc<Inner>);
impl NativeSocket {
    /// Borrow the native capability identity for readiness registration.
    ///
    /// Keep this socket or a clone alive for the entire registration. The
    /// returned value is not a Unix descriptor and transfers no ownership;
    /// consumers must not close it independently of this capability.
    pub fn registration_id(&self) -> u32 {
        self.0.id
    }

    fn owned(id: u32) -> io::Result<Self> {
        if id == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native socket ID is zero",
            ));
        }
        Ok(Self(Arc::new(Inner {
            id,
            nonblocking: AtomicBool::new(false),
            read_timeout: AtomicU64::new(0),
            write_timeout: AtomicU64::new(0),
        })))
    }
    pub fn new_tcp(family: u32) -> io::Result<Self> {
        let mut id = 0;
        status(unsafe { trueos_mio_tcp_socket_create(family, &mut id) })?;
        Self::owned(id)
    }
    pub fn bind(&self, addr: SocketAddr) -> io::Result<()> {
        status(unsafe { trueos_mio_tcp_socket_bind(self.0.id, raw(addr)?) })
    }
    pub fn connect(&self, addr: SocketAddr) -> io::Result<()> {
        status(unsafe { trueos_mio_tcp_socket_connect(self.0.id, raw(addr)?) })?;
        if self.nonblocking() {
            Ok(())
        } else {
            self.connected(None)
        }
    }
    pub fn listen(&self, backlog: u32) -> io::Result<()> {
        status(unsafe { trueos_mio_tcp_socket_listen(self.0.id, backlog) })
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        let mut out = Address::default();
        status(unsafe { trueos_mio_socket_local_addr(self.0.id, &mut out) })?;
        address(out)
    }
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        let mut out = Address::default();
        status(unsafe { trueos_mio_socket_peer_addr(self.0.id, &mut out) })?;
        address(out)
    }
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        let value = unsafe { trueos_mio_socket_take_error(self.0.id) };
        if value == 0 {
            Ok(None)
        } else {
            Ok(Some(error(value)))
        }
    }
    pub fn set_nonblocking(&self, value: bool) -> io::Result<()> {
        self.0.nonblocking.store(value, Ordering::Release);
        Ok(())
    }
    pub fn nonblocking(&self) -> bool {
        self.0.nonblocking.load(Ordering::Acquire)
    }
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        status(unsafe {
            trueos_mio_socket_shutdown(
                self.0.id,
                match how {
                    Shutdown::Read => 0,
                    Shutdown::Write => 1,
                    Shutdown::Both => 2,
                },
            )
        })
    }
    pub fn option(&self, option: u32, value: Option<u32>) -> io::Result<u32> {
        let value = match value {
            None => -1,
            Some(v) => i32::try_from(v).map_err(|_| io::ErrorKind::InvalidInput)?,
        };
        let result = unsafe { trueos_mio_socket_option(self.0.id, option, value) };
        status(result)?;
        Ok(result as u32)
    }
    fn set_timeout(slot: &AtomicU64, value: Option<Duration>) -> io::Result<()> {
        let value = match value {
            None => 0,
            Some(v) if v.is_zero() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "zero network timeout",
                ));
            }
            Some(v) => u64::try_from(v.as_millis()).unwrap_or(u64::MAX).max(1),
        };
        slot.store(value, Ordering::Release);
        Ok(())
    }
    fn timeout(slot: &AtomicU64) -> Option<Duration> {
        let value = slot.load(Ordering::Acquire);
        (value != 0).then(|| Duration::from_millis(value))
    }
    pub fn set_read_timeout(&self, value: Option<Duration>) -> io::Result<()> {
        Self::set_timeout(&self.0.read_timeout, value)
    }
    pub fn set_write_timeout(&self, value: Option<Duration>) -> io::Result<()> {
        Self::set_timeout(&self.0.write_timeout, value)
    }
    pub fn read_timeout(&self) -> Option<Duration> {
        Self::timeout(&self.0.read_timeout)
    }
    pub fn write_timeout(&self) -> Option<Duration> {
        Self::timeout(&self.0.write_timeout)
    }
    fn retry<T>(
        &self,
        timeout: Option<Duration>,
        mut f: impl FnMut() -> io::Result<T>,
    ) -> io::Result<T> {
        let start = Instant::now();
        loop {
            match f() {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && !self.nonblocking() => {
                    if timeout.is_some_and(|limit| start.elapsed() >= limit) {
                        return Err(error(-7));
                    }
                    crate::thread::sleep(Duration::from_millis(1));
                }
                result => return result,
            }
        }
    }
    fn connected(&self, timeout: Option<Duration>) -> io::Result<()> {
        let start = Instant::now();
        loop {
            // peer_addr is available as soon as connection is submitted. Only the native
            // writable state certifies completion; querying it also pumps pending events.
            let ready = unsafe { trueos_mio_socket_poll_ready(self.0.id, 2) };
            if ready < 0 {
                return Err(error(ready));
            }
            if let Some(err) = self.take_error()? {
                return Err(err);
            }
            if ready & 2 != 0 {
                return Ok(());
            }
            if ready & 28 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "native connection closed before becoming writable",
                ));
            }
            if timeout.is_some_and(|limit| start.elapsed() >= limit) {
                return Err(error(-7));
            }
            crate::thread::sleep(Duration::from_millis(1));
        }
    }
    pub fn wait_connected(&self, timeout: Duration) -> io::Result<()> {
        if timeout.is_zero() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.connected(Some(timeout))
    }
    fn count(value: isize, cap: usize) -> io::Result<usize> {
        if value < 0 {
            Err(error(value as i32))
        } else if value as usize > cap {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native network byte count exceeds buffer",
            ))
        } else {
            Ok(value as usize)
        }
    }
    pub fn read(&self, buf: &mut [u8], peek: bool) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.retry(self.read_timeout(), || {
            Self::count(
                unsafe {
                    if peek {
                        trueos_mio_tcp_stream_peek(self.0.id, buf.as_mut_ptr(), buf.len())
                    } else {
                        trueos_mio_tcp_stream_read(self.0.id, buf.as_mut_ptr(), buf.len())
                    }
                },
                buf.len(),
            )
        })
    }
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.retry(self.write_timeout(), || {
            Self::count(
                unsafe { trueos_mio_tcp_stream_write(self.0.id, buf.as_ptr(), buf.len()) },
                buf.len(),
            )
        })
    }
    pub fn accept(&self) -> io::Result<(Self, SocketAddr)> {
        self.retry(None, || {
            let (mut id, mut addr) = (0, Address::default());
            status(unsafe { trueos_mio_tcp_listener_accept(self.0.id, &mut id, &mut addr) })?;
            let socket = Self::owned(id)?;
            Ok((socket, address(addr)?))
        })
    }
    pub fn udp_recv(&self, buf: &mut [u8], peek: bool) -> io::Result<(usize, SocketAddr)> {
        self.retry(self.read_timeout(), || {
            let mut addr = Address::default();
            let result = unsafe {
                if peek {
                    trueos_mio_udp_socket_peek_from(
                        self.0.id,
                        &mut addr,
                        buf.as_mut_ptr(),
                        buf.len(),
                    )
                } else {
                    trueos_mio_udp_socket_recv_from(
                        self.0.id,
                        &mut addr,
                        buf.as_mut_ptr(),
                        buf.len(),
                    )
                }
            };
            Ok((Self::count(result, buf.len())?, address(addr)?))
        })
    }
    pub fn udp_send(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        let addr = raw(addr)?;
        self.retry(self.write_timeout(), || {
            Self::count(
                unsafe { trueos_mio_udp_socket_send_to(self.0.id, addr, buf.as_ptr(), buf.len()) },
                buf.len(),
            )
        })
    }
    pub fn udp_connect(&self, addr: SocketAddr) -> io::Result<()> {
        status(unsafe { trueos_mio_udp_socket_connect(self.0.id, raw(addr)?) })
    }
}
#[derive(Debug)]
pub struct TcpStream(NativeSocket);
#[derive(Debug)]
pub struct TcpListener(NativeSocket);
#[derive(Debug)]
pub struct UdpSocket(NativeSocket);
impl TcpStream {
    pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        super::each_addr(addr, |addr| Self::connect_inner(addr, None))
    }
    fn connect_inner(addr: &SocketAddr, timeout: Option<Duration>) -> io::Result<Self> {
        let mut id = 0;
        status(unsafe { trueos_mio_tcp_stream_connect(raw(*addr)?, &mut id) })?;
        let socket = NativeSocket::owned(id)?;
        socket.connected(timeout)?;
        Ok(Self(socket))
    }
    pub fn connect_timeout(addr: &SocketAddr, timeout: Duration) -> io::Result<Self> {
        if timeout.is_zero() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Self::connect_inner(addr, Some(timeout))
    }
    pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf, true)
    }
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf, false)
    }
    pub fn read_buf(&self, mut buf: BorrowedCursor<'_, u8>) -> io::Result<()> {
        let uninit = unsafe { buf.as_mut() };
        let mut temp = crate::vec![0;uninit.len().min(64*1024)];
        let count = self.read(&mut temp)?;
        buf.append(&temp[..count]);
        Ok(())
    }
    pub fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        io::default_read_vectored(|buf| self.read(buf), bufs)
    }
    pub fn is_read_vectored(&self) -> bool {
        false
    }
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    pub fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        io::default_write_vectored(|buf| self.write(buf), bufs)
    }
    pub fn is_write_vectored(&self) -> bool {
        false
    }
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.peer_addr()
    }
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.0.shutdown(how)
    }
    pub fn set_linger(&self, _: Option<Duration>) -> io::Result<()> {
        unsupported()
    }
    pub fn linger(&self) -> io::Result<Option<Duration>> {
        unsupported()
    }
    pub fn set_keepalive(&self, _: bool) -> io::Result<()> {
        unsupported()
    }
    pub fn keepalive(&self) -> io::Result<bool> {
        unsupported()
    }
    pub fn set_nodelay(&self, value: bool) -> io::Result<()> {
        self.0.option(1, Some(u32::from(value))).map(|_| ())
    }
    pub fn nodelay(&self) -> io::Result<bool> {
        self.0.option(1, None).map(|v| v != 0)
    }
}
impl TcpListener {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        super::each_addr(addr, |addr| {
            let mut id = 0;
            status(unsafe { trueos_mio_tcp_listener_bind(raw(*addr)?, &mut id) })?;
            Ok(Self(NativeSocket::owned(id)?))
        })
    }
    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        self.0.accept().map(|(s, a)| (TcpStream(s), a))
    }
    pub fn set_only_v6(&self, _: bool) -> io::Result<()> {
        unsupported()
    }
    pub fn only_v6(&self) -> io::Result<bool> {
        unsupported()
    }
}
impl UdpSocket {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        super::each_addr(addr, |addr| {
            let mut id = 0;
            status(unsafe { trueos_mio_udp_socket_bind(raw(*addr)?, &mut id) })?;
            Ok(Self(NativeSocket::owned(id)?))
        })
    }
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.peer_addr()
    }
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.0.udp_recv(buf, false)
    }
    pub fn peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.0.udp_recv(buf, true)
    }
    pub fn send_to(&self, buf: &[u8], addr: &SocketAddr) -> io::Result<usize> {
        self.0.udp_send(buf, *addr)
    }
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.recv_from(buf).map(|(n, _)| n)
    }
    pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.peek_from(buf).map(|(n, _)| n)
    }
    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.udp_send(buf, self.0.peer_addr()?)
    }
    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> io::Result<()> {
        super::each_addr(addr, |addr| self.0.udp_connect(*addr))
    }
}
macro_rules! common {($($ty:ident),*)=>{$(impl $ty{
 pub fn from_native(socket:NativeSocket)->Self{Self(socket)}
 pub fn native(&self)->&NativeSocket{&self.0}
 pub fn into_native(self)->NativeSocket{self.0}
 pub fn socket_addr(&self)->io::Result<SocketAddr>{self.0.local_addr()}
 pub fn duplicate(&self)->io::Result<Self>{Ok(Self(self.0.clone()))}
 pub fn set_nonblocking(&self,value:bool)->io::Result<()>{self.0.set_nonblocking(value)}
 pub fn take_error(&self)->io::Result<Option<io::Error>>{self.0.take_error()}
 pub fn set_ttl(&self,value:u32)->io::Result<()>{self.0.option(2,Some(value)).map(|_|())}
 pub fn ttl(&self)->io::Result<u32>{self.0.option(2,None)}
})*};}
common!(TcpStream, TcpListener, UdpSocket);
macro_rules! timeouts {($($ty:ident),*)=>{$(impl $ty{
 pub fn set_read_timeout(&self,v:Option<Duration>)->io::Result<()>{self.0.set_read_timeout(v)}
 pub fn set_write_timeout(&self,v:Option<Duration>)->io::Result<()>{self.0.set_write_timeout(v)}
 pub fn read_timeout(&self)->io::Result<Option<Duration>>{Ok(self.0.read_timeout())}
 pub fn write_timeout(&self)->io::Result<Option<Duration>>{Ok(self.0.write_timeout())}
})*};}
timeouts!(TcpStream, UdpSocket);
macro_rules! udp_unsupported {($($name:ident($($arg:ident:$ty:ty),*)->$result:ty),*$(,)?)=>{impl UdpSocket{$(pub fn $name(&self,$($arg:$ty),*)->io::Result<$result>{$(let _=$arg;)*unsupported()})*}};}
udp_unsupported!(set_broadcast(v:bool)->(),broadcast()->bool,set_multicast_loop_v4(v:bool)->(),multicast_loop_v4()->bool,set_multicast_ttl_v4(v:u32)->(),multicast_ttl_v4()->u32,set_multicast_loop_v6(v:bool)->(),multicast_loop_v6()->bool,join_multicast_v4(a:&Ipv4Addr,b:&Ipv4Addr)->(),join_multicast_v6(a:&Ipv6Addr,b:u32)->(),leave_multicast_v4(a:&Ipv4Addr,b:&Ipv4Addr)->(),leave_multicast_v6(a:&Ipv6Addr,b:u32)->());
pub type LookupHost = crate::vec::IntoIter<SocketAddr>;
pub fn lookup_host(host: &str, port: u16) -> io::Result<LookupHost> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(crate::vec![SocketAddr::new(ip, port)].into_iter());
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(crate::vec![SocketAddr::from(([127, 0, 0, 1], port))].into_iter());
    }
    let mut octets = [0; 4];
    // DNS uses the existing zero-success, positive-error CABI convention,
    // unlike the Mio socket status domain above.
    let code = unsafe { trueos_cabi_dns_resolve_ipv4(host.as_ptr(), host.len(), octets.as_mut_ptr()) };
    if code != 0 {
        return Err(io::Error::new(match code {
            22 => io::ErrorKind::InvalidInput,
            110 => io::ErrorKind::TimedOut,
            _ => io::ErrorKind::Other,
        }, "native DNS resolution failed"));
    }
    Ok(crate::vec![SocketAddr::from((octets, port))].into_iter())
}
