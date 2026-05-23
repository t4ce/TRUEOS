use alloc::{collections::BTreeMap, vec::Vec};
use core::mem::MaybeUninit;
use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use core::ptr;
use core::time::Duration;
use spin::{Mutex, Once};
use trueos_io as io;

use crate::{MsgHdr, MsgHdrMut, RecvFlags, SockAddr, TcpKeepalive};

pub(crate) use core::ffi::c_int;

pub(crate) const AF_UNIX: c_int = 1;
pub(crate) const AF_INET: c_int = 2;
pub(crate) const AF_INET6: c_int = 10;

pub(crate) const SOCK_STREAM: c_int = 1;
pub(crate) const SOCK_DGRAM: c_int = 2;
pub(crate) const SOCK_RAW: c_int = 3;
pub(crate) const SOCK_SEQPACKET: c_int = 5;

pub(crate) const IPPROTO_ICMP: c_int = 1;
pub(crate) const IPPROTO_TCP: c_int = 6;
pub(crate) const IPPROTO_UDP: c_int = 17;
pub(crate) const IPPROTO_IPV6: c_int = 41;
pub(crate) const IPPROTO_ICMPV6: c_int = 58;
pub(crate) const IPPROTO_IP: c_int = 0;

pub(crate) const SOL_SOCKET: c_int = 1;
pub(crate) const SO_BROADCAST: c_int = 6;
pub(crate) const SO_ERROR: c_int = 4;
pub(crate) const SO_KEEPALIVE: c_int = 9;
pub(crate) const SO_LINGER: c_int = 13;
pub(crate) const SO_OOBINLINE: c_int = 10;
pub(crate) const SO_RCVBUF: c_int = 8;
pub(crate) const SO_RCVTIMEO: c_int = 20;
pub(crate) const SO_REUSEADDR: c_int = 2;
pub(crate) const SO_SNDBUF: c_int = 7;
pub(crate) const SO_SNDTIMEO: c_int = 21;
pub(crate) const SO_TYPE: c_int = 3;

pub(crate) const IP_ADD_MEMBERSHIP: c_int = 35;
pub(crate) const IP_DROP_MEMBERSHIP: c_int = 36;
pub(crate) const IP_MULTICAST_IF: c_int = 32;
pub(crate) const IP_MULTICAST_LOOP: c_int = 34;
pub(crate) const IP_MULTICAST_TTL: c_int = 33;
pub(crate) const IP_TOS: c_int = 1;
pub(crate) const IP_TTL: c_int = 2;
pub(crate) const IP_HDRINCL: c_int = 3;
pub(crate) const IP_RECVTOS: c_int = 13;
pub(crate) const IP_ADD_SOURCE_MEMBERSHIP: c_int = 39;
pub(crate) const IP_DROP_SOURCE_MEMBERSHIP: c_int = 40;
pub(crate) const IPV6_ADD_MEMBERSHIP: c_int = 20;
pub(crate) const IPV6_DROP_MEMBERSHIP: c_int = 21;
pub(crate) const IPV6_MULTICAST_HOPS: c_int = 18;
pub(crate) const IPV6_MULTICAST_IF: c_int = 17;
pub(crate) const IPV6_MULTICAST_LOOP: c_int = 19;
pub(crate) const IPV6_UNICAST_HOPS: c_int = 16;
pub(crate) const IPV6_V6ONLY: c_int = 26;
pub(crate) const IPV6_RECVTCLASS: c_int = 66;
pub(crate) const IPV6_RECVHOPLIMIT: c_int = 51;

pub(crate) const TCP_NODELAY: c_int = 1;
pub(crate) const TCP_KEEPINTVL: c_int = 5;
pub(crate) const TCP_KEEPCNT: c_int = 6;

pub(crate) const MSG_OOB: c_int = 1;
pub(crate) const MSG_PEEK: c_int = 2;
pub(crate) const MSG_TRUNC: c_int = 32;

pub(crate) type Bool = c_int;
pub(crate) type RawSocket = c_int;
#[allow(non_camel_case_types)]
pub(crate) type sa_family_t = u16;
#[allow(non_camel_case_types)]
pub(crate) type socklen_t = u32;

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct in_addr {
    pub s_addr: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct in6_addr {
    pub s6_addr: [u8; 16],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr {
    pub sa_family: sa_family_t,
    pub sa_data: [u8; 14],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr_in {
    pub sin_family: sa_family_t,
    pub sin_port: u16,
    pub sin_addr: in_addr,
    pub sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr_in6 {
    pub sin6_family: sa_family_t,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: in6_addr,
    pub sin6_scope_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr_storage {
    pub ss_family: sa_family_t,
    pub __storage: [u8; 126],
}

#[repr(C)]
#[derive(Copy, Clone)]
#[allow(non_camel_case_types)]
pub(crate) struct linger {
    pub l_onoff: c_int,
    pub l_linger: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct IpMreq {
    pub imr_multiaddr: in_addr,
    pub imr_interface: in_addr,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct IpMreqSource {
    pub imr_multiaddr: in_addr,
    pub imr_interface: in_addr,
    pub imr_sourceaddr: in_addr,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct Ipv6Mreq {
    pub ipv6mr_multiaddr: in6_addr,
    pub ipv6mr_interface: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct msghdr {
    pub msg_name: *mut core::ffi::c_void,
    pub msg_namelen: socklen_t,
    pub msg_iov: *mut core::ffi::c_void,
    pub msg_iovlen: usize,
    pub msg_control: *mut core::ffi::c_void,
    pub msg_controllen: usize,
    pub msg_flags: c_int,
}

pub(crate) struct MaybeUninitSlice<'a>(&'a mut [MaybeUninit<u8>]);

impl<'a> MaybeUninitSlice<'a> {
    pub(crate) fn new(buf: &'a mut [MaybeUninit<u8>]) -> MaybeUninitSlice<'a> {
        MaybeUninitSlice(buf)
    }

    pub(crate) fn as_slice(&self) -> &[MaybeUninit<u8>] {
        self.0
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [MaybeUninit<u8>] {
        self.0
    }
}

#[derive(Debug)]
pub(crate) struct Socket(RawSocket);

#[derive(Clone)]
struct SocketMeta {
    domain: c_int,
    socket_type: c_int,
    protocol: c_int,
    backend: Option<RawSocket>,
    nonblocking: bool,
    recv_timeout: Option<Duration>,
    send_timeout: Option<Duration>,
    local: Option<SocketAddr>,
    peer: Option<SocketAddr>,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TrueosMioSocketAddr {
    family: u8,
    port: u16,
    addr: [u8; 16],
}

unsafe extern "C" {
    fn trueos_mio_tcp_listener_bind(addr: TrueosMioSocketAddr, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_tcp_stream_connect(addr: TrueosMioSocketAddr, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_udp_socket_bind(addr: TrueosMioSocketAddr, out_socket_id: *mut u32) -> i32;
    fn trueos_mio_socket_close(socket_id: u32) -> i32;
    fn trueos_mio_socket_local_addr(socket_id: u32, out_addr: *mut TrueosMioSocketAddr) -> i32;
    fn trueos_mio_socket_peer_addr(socket_id: u32, out_addr: *mut TrueosMioSocketAddr) -> i32;
    fn trueos_mio_socket_take_error(socket_id: u32) -> i32;
    fn trueos_mio_tcp_stream_read(socket_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_mio_tcp_stream_write(socket_id: u32, data_ptr: *const u8, data_len: usize) -> isize;
    fn trueos_mio_udp_socket_connect(socket_id: u32, addr: TrueosMioSocketAddr) -> i32;
    fn trueos_mio_udp_socket_send_to(
        socket_id: u32,
        addr: TrueosMioSocketAddr,
        data_ptr: *const u8,
        data_len: usize,
    ) -> isize;
    fn trueos_mio_udp_socket_recv_from(
        socket_id: u32,
        out_addr: *mut TrueosMioSocketAddr,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_mio_tcp_listener_accept(
        socket_id: u32,
        out_socket_id: *mut u32,
        out_addr: *mut TrueosMioSocketAddr,
    ) -> i32;
}

fn next_socket_id() -> RawSocket {
    static NEXT: Once<Mutex<RawSocket>> = Once::new();
    let mut next = NEXT.call_once(|| Mutex::new(1)).lock();
    let id = *next;
    *next = next.saturating_add(1).max(1);
    id
}

fn socket_registry() -> &'static Mutex<BTreeMap<RawSocket, SocketMeta>> {
    static REGISTRY: Once<Mutex<BTreeMap<RawSocket, SocketMeta>>> = Once::new();
    REGISTRY.call_once(|| Mutex::new(BTreeMap::new()))
}

fn io_error_from_neg_rc(rc: i32) -> io::Error {
    let kind = match -rc {
        9 => io::ErrorKind::NotFound,
        11 => io::ErrorKind::WouldBlock,
        22 => io::ErrorKind::InvalidInput,
        32 => io::ErrorKind::BrokenPipe,
        101 => io::ErrorKind::NotFound,
        104 => io::ErrorKind::ConnectionReset,
        107 => io::ErrorKind::NotConnected,
        110 => io::ErrorKind::TimedOut,
        111 => io::ErrorKind::ConnectionRefused,
        115 => io::ErrorKind::WouldBlock,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, "socket2 zkvm socket error")
}

fn rc_to_io(rc: i32) -> io::Result<()> {
    if rc >= 0 {
        Ok(())
    } else {
        Err(io_error_from_neg_rc(rc))
    }
}

fn ssize_to_io(value: isize) -> io::Result<usize> {
    if value >= 0 {
        Ok(value as usize)
    } else {
        Err(io_error_from_neg_rc(value as i32))
    }
}

fn mio_status_to_error(status: i32, detail: &'static str) -> io::Error {
    let kind = match status {
        -1 => io::ErrorKind::Other,
        -2 => io::ErrorKind::WouldBlock,
        -3 => io::ErrorKind::NotConnected,
        -4 => io::ErrorKind::InvalidInput,
        -5 => io::ErrorKind::NotFound,
        -7 => io::ErrorKind::TimedOut,
        -8 => io::ErrorKind::NotFound,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, detail)
}

fn mio_status_to_io(status: i32, detail: &'static str) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(mio_status_to_error(status, detail))
    }
}

fn mio_ssize_to_io(value: isize, detail: &'static str) -> io::Result<usize> {
    if value >= 0 {
        Ok(value as usize)
    } else {
        Err(mio_status_to_error(value as i32, detail))
    }
}

fn socket_addr_to_mio(addr: SocketAddr) -> TrueosMioSocketAddr {
    match addr {
        SocketAddr::V4(addr) => {
            let mut raw = TrueosMioSocketAddr {
                family: 4,
                port: addr.port(),
                addr: [0; 16],
            };
            raw.addr[..4].copy_from_slice(&addr.ip().octets());
            raw
        }
        SocketAddr::V6(addr) => TrueosMioSocketAddr {
            family: 6,
            port: addr.port(),
            addr: addr.ip().octets(),
        },
    }
}

fn mio_to_socket_addr(raw: TrueosMioSocketAddr) -> io::Result<SocketAddr> {
    match raw.family {
        4 => Ok(SocketAddr::from(([raw.addr[0], raw.addr[1], raw.addr[2], raw.addr[3]], raw.port))),
        6 => Ok(SocketAddr::from((raw.addr, raw.port))),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket2 zkvm invalid TRUEOS mio socket address family",
        )),
    }
}

fn timeout_to_cabi(timeout: Option<Duration>) -> u64 {
    timeout
        .map(|timeout| timeout.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(u64::MAX)
}

fn invalid_socket() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "socket2 zkvm invalid socket")
}

fn with_meta<T>(socket: RawSocket, f: impl FnOnce(&SocketMeta) -> T) -> io::Result<T> {
    let registry = socket_registry().lock();
    let Some(meta) = registry.get(&socket) else {
        return Err(invalid_socket());
    };
    Ok(f(meta))
}

fn with_meta_mut<T>(socket: RawSocket, f: impl FnOnce(&mut SocketMeta) -> T) -> io::Result<T> {
    let mut registry = socket_registry().lock();
    let Some(meta) = registry.get_mut(&socket) else {
        return Err(invalid_socket());
    };
    Ok(f(meta))
}

fn with_backend<T>(
    socket: RawSocket,
    detail: &'static str,
    f: impl FnOnce(RawSocket) -> io::Result<T>,
) -> io::Result<T> {
    let backend = with_meta(socket, |meta| meta.backend)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, detail))?;
    f(backend)
}

unsafe fn cast_value<T, U: Copy>(value: U) -> T {
    debug_assert_eq!(core::mem::size_of::<T>(), core::mem::size_of::<U>());
    let mut out = MaybeUninit::<T>::uninit();
    ptr::copy_nonoverlapping(
        (&value as *const U).cast::<u8>(),
        out.as_mut_ptr().cast::<u8>(),
        core::mem::size_of::<U>(),
    );
    out.assume_init()
}

fn default_local_addr(domain: c_int) -> SocketAddr {
    match domain {
        AF_INET6 => SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0)),
        _ => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
    }
}

pub(crate) unsafe fn socket_from_raw(socket: RawSocket) -> Socket {
    Socket(socket)
}

pub(crate) fn socket_as_raw(socket: &Socket) -> RawSocket {
    socket.0
}

pub(crate) fn socket_into_raw(socket: Socket) -> RawSocket {
    let raw = socket.0;
    core::mem::forget(socket);
    raw
}

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Other, "socket2 zkvm backend is not wired to TRUEOS net yet")
}

pub(crate) fn socket(domain: c_int, socket_type: c_int, protocol: c_int) -> io::Result<RawSocket> {
    if !matches!(socket_type, SOCK_STREAM | SOCK_DGRAM) {
        return Err(unsupported());
    }

    let raw = next_socket_id();

    socket_registry().lock().insert(
        raw,
        SocketMeta {
            domain,
            socket_type,
            protocol,
            backend: None,
            nonblocking: false,
            recv_timeout: None,
            send_timeout: None,
            local: None,
            peer: None,
        },
    );
    Ok(raw)
}

pub(crate) fn socketpair(_: c_int, _: c_int, _: c_int) -> io::Result<[RawSocket; 2]> {
    Err(unsupported())
}

pub(crate) fn bind(socket: RawSocket, address: &SockAddr) -> io::Result<()> {
    let Some(address) = address.as_socket() else {
        return Err(unsupported());
    };

    let socket_type = with_meta(socket, |meta| meta.socket_type)?;
    if socket_type == SOCK_DGRAM {
        let mut backend = 0u32;
        let status =
            unsafe { trueos_mio_udp_socket_bind(socket_addr_to_mio(address), &mut backend) };
        mio_status_to_io(status, "socket2 zkvm UDP bind failed")?;
        let _ = with_meta_mut(socket, |meta| {
            meta.backend = Some(backend as RawSocket);
            meta.local = Some(address);
        })?;
        return Ok(());
    }

    let _ = with_meta_mut(socket, |meta| meta.local = Some(address));
    Ok(())
}

pub(crate) fn connect(socket: RawSocket, address: &SockAddr) -> io::Result<()> {
    let Some(address) = address.as_socket() else {
        return Err(unsupported());
    };
    let socket_type = with_meta(socket, |meta| meta.socket_type)?;
    if socket_type == SOCK_DGRAM {
        with_backend(socket, "socket2 zkvm UDP connect before bind", |backend| {
            let status = unsafe {
                trueos_mio_udp_socket_connect(backend as u32, socket_addr_to_mio(address))
            };
            mio_status_to_io(status, "socket2 zkvm UDP connect failed")
        })?;
        let _ = with_meta_mut(socket, |meta| meta.peer = Some(address));
        return Ok(());
    }

    let mut backend = 0u32;
    let status =
        unsafe { trueos_mio_tcp_stream_connect(socket_addr_to_mio(address), &mut backend) };
    mio_status_to_io(status, "socket2 zkvm TCP connect failed")?;
    let _ = with_meta_mut(socket, |meta| {
        meta.backend = Some(backend as RawSocket);
        meta.peer = Some(address);
    })?;
    Ok(())
}

pub(crate) fn poll_connect(socket: &crate::Socket, timeout: Duration) -> io::Result<()> {
    let _ = timeout;
    with_backend(socket.as_raw(), "socket2 zkvm TCP connect not submitted", |backend| {
        let status = unsafe { trueos_mio_socket_take_error(backend as u32) };
        if status == 0 {
            Ok(())
        } else {
            Err(mio_status_to_error(status, "socket2 zkvm TCP connect failed"))
        }
    })
}

pub(crate) fn listen(socket: RawSocket, _: c_int) -> io::Result<()> {
    let address = with_meta(socket, |meta| {
        meta.local
            .unwrap_or_else(|| default_local_addr(meta.domain))
    })?;
    let mut backend = 0u32;
    let status = unsafe { trueos_mio_tcp_listener_bind(socket_addr_to_mio(address), &mut backend) };
    mio_status_to_io(status, "socket2 zkvm TCP listen failed")?;
    let _ = with_meta_mut(socket, |meta| {
        meta.backend = Some(backend as RawSocket);
        meta.local = Some(address);
    })?;
    Ok(())
}

pub(crate) fn accept(socket: RawSocket) -> io::Result<(RawSocket, SockAddr)> {
    with_backend(socket, "socket2 zkvm accept before listen", |backend| {
        let mut child = 0u32;
        let mut addr = TrueosMioSocketAddr::default();
        let status =
            unsafe { trueos_mio_tcp_listener_accept(backend as u32, &mut child, &mut addr) };
        mio_status_to_io(status, "socket2 zkvm TCP accept failed")?;
        let peer = mio_to_socket_addr(addr)?;
        let child_socket = next_socket_id();
        let parent_meta = with_meta(socket, Clone::clone)?;
        socket_registry().lock().insert(
            child_socket,
            SocketMeta {
                domain: parent_meta.domain,
                socket_type: SOCK_STREAM,
                protocol: IPPROTO_TCP,
                backend: Some(child as RawSocket),
                nonblocking: parent_meta.nonblocking,
                recv_timeout: parent_meta.recv_timeout,
                send_timeout: parent_meta.send_timeout,
                local: parent_meta.local,
                peer: Some(peer),
            },
        );
        Ok((child_socket, SockAddr::from(peer)))
    })
}

pub(crate) fn getsockname(socket: RawSocket) -> io::Result<SockAddr> {
    let address = if let Some(backend) = with_meta(socket, |meta| meta.backend)? {
        let mut addr = TrueosMioSocketAddr::default();
        let status = unsafe { trueos_mio_socket_local_addr(backend as u32, &mut addr) };
        if status == 0 {
            mio_to_socket_addr(addr)?
        } else {
            with_meta(socket, |meta| {
                meta.local
                    .unwrap_or_else(|| default_local_addr(meta.domain))
            })?
        }
    } else {
        with_meta(socket, |meta| {
            meta.local
                .unwrap_or_else(|| default_local_addr(meta.domain))
        })?
    };
    Ok(SockAddr::from(address))
}

pub(crate) fn getpeername(socket: RawSocket) -> io::Result<SockAddr> {
    let address = if let Some(backend) = with_meta(socket, |meta| meta.backend)? {
        let mut addr = TrueosMioSocketAddr::default();
        let status = unsafe { trueos_mio_socket_peer_addr(backend as u32, &mut addr) };
        if status == 0 {
            mio_to_socket_addr(addr)?
        } else {
            with_meta(socket, |meta| meta.peer)?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotConnected, "socket2 zkvm peer not connected")
            })?
        }
    } else {
        with_meta(socket, |meta| meta.peer)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "socket2 zkvm peer not connected")
        })?
    };
    Ok(SockAddr::from(address))
}

pub(crate) fn try_clone(_: RawSocket) -> io::Result<RawSocket> {
    Err(unsupported())
}

pub(crate) fn nonblocking(socket: RawSocket) -> io::Result<bool> {
    with_meta(socket, |meta| meta.nonblocking)
}

pub(crate) fn set_nonblocking(socket: RawSocket, nonblocking: bool) -> io::Result<()> {
    let _ = with_meta_mut(socket, |meta| meta.nonblocking = nonblocking)?;
    Ok(())
}

pub(crate) fn shutdown(socket: RawSocket, how: std::net::Shutdown) -> io::Result<()> {
    let how = match how {
        std::net::Shutdown::Read => 0,
        std::net::Shutdown::Write => 1,
        std::net::Shutdown::Both => 2,
    };
    let _ = how;
    with_backend(socket, "socket2 zkvm shutdown before connect", |backend| {
        mio_status_to_io(
            unsafe { trueos_mio_socket_close(backend as u32) },
            "socket2 zkvm socket shutdown failed",
        )
    })
}

pub(crate) fn recv(
    socket: RawSocket,
    buf: &mut [MaybeUninit<u8>],
    flags: c_int,
) -> io::Result<usize> {
    let _ = flags;
    with_backend(socket, "socket2 zkvm recv before connect", |backend| {
        mio_ssize_to_io(
            unsafe {
                trueos_mio_tcp_stream_read(backend as u32, buf.as_mut_ptr().cast::<u8>(), buf.len())
            },
            "socket2 zkvm TCP recv failed",
        )
    })
}

pub(crate) fn recv_from(
    socket: RawSocket,
    buf: &mut [MaybeUninit<u8>],
    flags: c_int,
) -> io::Result<(usize, SockAddr)> {
    let _ = flags;
    with_backend(socket, "socket2 zkvm UDP recv_from before bind", |backend| {
        let mut addr = TrueosMioSocketAddr::default();
        let len = mio_ssize_to_io(
            unsafe {
                trueos_mio_udp_socket_recv_from(
                    backend as u32,
                    &mut addr,
                    buf.as_mut_ptr().cast::<u8>(),
                    buf.len(),
                )
            },
            "socket2 zkvm UDP recv_from failed",
        )?;
        Ok((len, SockAddr::from(mio_to_socket_addr(addr)?)))
    })
}

pub(crate) fn peek_sender(_: RawSocket) -> io::Result<SockAddr> {
    Err(unsupported())
}

pub(crate) fn recv_vectored(
    _: RawSocket,
    _: &mut [crate::MaybeUninitSlice<'_>],
    _: c_int,
) -> io::Result<(usize, RecvFlags)> {
    Err(unsupported())
}

pub(crate) fn recv_from_vectored(
    _: RawSocket,
    _: &mut [crate::MaybeUninitSlice<'_>],
    _: c_int,
) -> io::Result<(usize, RecvFlags, SockAddr)> {
    Err(unsupported())
}

pub(crate) fn recvmsg(_: RawSocket, _: &mut MsgHdrMut<'_, '_, '_>, _: c_int) -> io::Result<usize> {
    Err(unsupported())
}

pub(crate) fn send(socket: RawSocket, buf: &[u8], _: c_int) -> io::Result<usize> {
    with_backend(socket, "socket2 zkvm send before connect", |backend| {
        mio_ssize_to_io(
            unsafe { trueos_mio_tcp_stream_write(backend as u32, buf.as_ptr(), buf.len()) },
            "socket2 zkvm TCP send failed",
        )
    })
}

pub(crate) fn send_vectored(
    socket: RawSocket,
    bufs: &[crate::io::IoSlice<'_>],
    flags: c_int,
) -> io::Result<usize> {
    let total = bufs.iter().map(|buf| buf.len()).sum();
    let mut merged = Vec::with_capacity(total);
    for buf in bufs {
        merged.extend_from_slice(buf);
    }
    send(socket, &merged, flags)
}

pub(crate) fn send_to(
    socket: RawSocket,
    buf: &[u8],
    address: &SockAddr,
    _: c_int,
) -> io::Result<usize> {
    let Some(address) = address.as_socket() else {
        return Err(unsupported());
    };
    with_backend(socket, "socket2 zkvm UDP send_to before bind", |backend| {
        mio_ssize_to_io(
            unsafe {
                trueos_mio_udp_socket_send_to(
                    backend as u32,
                    socket_addr_to_mio(address),
                    buf.as_ptr(),
                    buf.len(),
                )
            },
            "socket2 zkvm UDP send_to failed",
        )
    })
}

pub(crate) fn send_to_vectored(
    _: RawSocket,
    _: &[crate::io::IoSlice<'_>],
    _: &SockAddr,
    _: c_int,
) -> io::Result<usize> {
    Err(unsupported())
}

pub(crate) fn sendmsg(_: RawSocket, _: &MsgHdr<'_, '_, '_>, _: c_int) -> io::Result<usize> {
    Err(unsupported())
}

pub(crate) fn timeout_opt(
    socket: RawSocket,
    _: c_int,
    name: c_int,
) -> io::Result<Option<Duration>> {
    with_meta(socket, |meta| match name {
        SO_RCVTIMEO => meta.recv_timeout,
        SO_SNDTIMEO => meta.send_timeout,
        _ => None,
    })
}

pub(crate) fn set_timeout_opt(
    socket: RawSocket,
    _: c_int,
    name: c_int,
    timeout: Option<Duration>,
) -> io::Result<()> {
    with_meta_mut(socket, |meta| match name {
        SO_RCVTIMEO => meta.recv_timeout = timeout,
        SO_SNDTIMEO => meta.send_timeout = timeout,
        _ => {}
    })?;
    Ok(())
}

pub(crate) fn tcp_keepalive_time(_: RawSocket) -> io::Result<Duration> {
    Err(unsupported())
}

pub(crate) fn set_tcp_keepalive(_: RawSocket, _: &TcpKeepalive) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) unsafe fn getsockopt<T>(socket: RawSocket, level: c_int, name: c_int) -> io::Result<T> {
    match (level, name) {
        (SOL_SOCKET, SO_TYPE) => {
            let socket_type = with_meta(socket, |meta| meta.socket_type)?;
            Ok(cast_value(socket_type))
        }
        (SOL_SOCKET, SO_ERROR) => {
            let value = with_meta(socket, |meta| meta.backend)?
                .map_or(0, |backend| unsafe { trueos_mio_socket_take_error(backend as u32) });
            if value < 0 {
                Err(mio_status_to_error(value, "socket2 zkvm socket error"))
            } else {
                Ok(cast_value(value as c_int))
            }
        }
        _ => Err(unsupported()),
    }
}

pub(crate) unsafe fn setsockopt<T>(
    _: RawSocket,
    level: c_int,
    name: c_int,
    _: T,
) -> io::Result<()> {
    match (level, name) {
        (SOL_SOCKET, SO_KEEPALIVE | SO_REUSEADDR | SO_BROADCAST) => Ok(()),
        (IPPROTO_TCP, TCP_NODELAY) => Ok(()),
        _ => Err(unsupported()),
    }
}

pub(crate) const fn to_in_addr(addr: &Ipv4Addr) -> in_addr {
    in_addr {
        s_addr: u32::from_ne_bytes(addr.octets()),
    }
}

pub(crate) fn from_in_addr(addr: in_addr) -> Ipv4Addr {
    Ipv4Addr::from(addr.s_addr.to_ne_bytes())
}

pub(crate) const fn to_in6_addr(addr: &Ipv6Addr) -> in6_addr {
    in6_addr {
        s6_addr: addr.octets(),
    }
}

pub(crate) fn from_in6_addr(addr: in6_addr) -> Ipv6Addr {
    Ipv6Addr::from(addr.s6_addr)
}

pub(crate) const fn to_mreqn(
    multiaddr: &Ipv4Addr,
    interface: &crate::socket::InterfaceIndexOrAddress,
) -> IpMreq {
    let interface = match interface {
        crate::socket::InterfaceIndexOrAddress::Index(_) => Ipv4Addr::UNSPECIFIED,
        crate::socket::InterfaceIndexOrAddress::Address(addr) => *addr,
    };
    IpMreq {
        imr_multiaddr: to_in_addr(multiaddr),
        imr_interface: to_in_addr(&interface),
    }
}

pub(crate) fn original_dst_v4(_: RawSocket) -> io::Result<SockAddr> {
    Err(unsupported())
}

pub(crate) fn original_dst_v6(_: RawSocket) -> io::Result<SockAddr> {
    Err(unsupported())
}

pub(crate) fn set_tcp_ack_frequency(_: RawSocket, _: u32) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn unix_sockaddr(_: &crate::path::Path) -> io::Result<SockAddr> {
    Err(unsupported())
}

pub(crate) fn set_msghdr_name(msg: &mut msghdr, name: &SockAddr) {
    msg.msg_name = name.as_ptr().cast_mut().cast();
    msg.msg_namelen = name.len();
}

pub(crate) fn set_msghdr_iov(msg: &mut msghdr, ptr: *mut core::ffi::c_void, len: usize) {
    msg.msg_iov = ptr;
    msg.msg_iovlen = len;
}

pub(crate) fn set_msghdr_control(msg: &mut msghdr, ptr: *mut core::ffi::c_void, len: usize) {
    msg.msg_control = ptr;
    msg.msg_controllen = len;
}

pub(crate) fn set_msghdr_flags(msg: &mut msghdr, flags: c_int) {
    msg.msg_flags = flags;
}

pub(crate) fn msghdr_flags(msg: &msghdr) -> RecvFlags {
    RecvFlags(msg.msg_flags)
}

pub(crate) fn msghdr_control_len(msg: &msghdr) -> usize {
    msg.msg_controllen
}

impl Drop for Socket {
    fn drop(&mut self) {
        let meta = socket_registry().lock().remove(&self.0);
        if let Some(meta) = meta {
            if let Some(backend) = meta.backend {
                let _ = unsafe { trueos_mio_socket_close(backend as u32) };
            }
        }
    }
}

impl ::core::fmt::Debug for crate::Domain {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.debug_tuple("Domain").field(&self.0).finish()
    }
}

impl ::core::fmt::Debug for crate::Type {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.debug_tuple("Type").field(&self.0).finish()
    }
}

impl ::core::fmt::Debug for crate::Protocol {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.debug_tuple("Protocol").field(&self.0).finish()
    }
}

impl ::core::fmt::Debug for crate::RecvFlags {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.debug_tuple("RecvFlags").field(&self.0).finish()
    }
}
