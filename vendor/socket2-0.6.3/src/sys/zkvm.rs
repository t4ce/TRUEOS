use std::collections::BTreeMap;
use std::io;
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::ptr;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use trueos_sys::vcabi;

use crate::{MsgHdr, MsgHdrMut, RecvFlags, SockAddr, TcpKeepalive};

pub(crate) use std::ffi::c_int;

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
    nonblocking: bool,
    recv_timeout: Option<Duration>,
    send_timeout: Option<Duration>,
    local: Option<SocketAddr>,
    peer: Option<SocketAddr>,
}

fn socket_registry() -> &'static Mutex<BTreeMap<RawSocket, SocketMeta>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<RawSocket, SocketMeta>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn io_error_from_neg_rc(rc: i32) -> io::Error {
    io::Error::from_raw_os_error(-rc)
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
        Err(io::Error::from_raw_os_error((-value) as i32))
    }
}

fn timeout_to_cabi(timeout: Option<Duration>) -> u64 {
    timeout
        .map(|timeout| timeout.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(u64::MAX)
}

fn invalid_socket() -> io::Error {
    io::Error::from_raw_os_error(9)
}

fn with_meta<T>(socket: RawSocket, f: impl FnOnce(&SocketMeta) -> T) -> io::Result<T> {
    let registry = socket_registry().lock().expect("socket registry poisoned");
    let Some(meta) = registry.get(&socket) else {
        return Err(invalid_socket());
    };
    Ok(f(meta))
}

fn with_meta_mut<T>(socket: RawSocket, f: impl FnOnce(&mut SocketMeta) -> T) -> io::Result<T> {
    let mut registry = socket_registry().lock().expect("socket registry poisoned");
    let Some(meta) = registry.get_mut(&socket) else {
        return Err(invalid_socket());
    };
    Ok(f(meta))
}

unsafe fn cast_value<T, U: Copy>(value: U) -> T {
    debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<U>());
    let mut out = MaybeUninit::<T>::uninit();
    ptr::copy_nonoverlapping(
        (&value as *const U).cast::<u8>(),
        out.as_mut_ptr().cast::<u8>(),
        std::mem::size_of::<U>(),
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
    std::mem::forget(socket);
    raw
}

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "socket2 zkvm backend is not wired to TRUEOS net yet",
    )
}

pub(crate) fn socket(domain: c_int, socket_type: c_int, protocol: c_int) -> io::Result<RawSocket> {
    let raw = unsafe { vcabi::trueos_cabi_socket_tcp_open(domain, socket_type, protocol) };
    if raw < 0 {
        return Err(io_error_from_neg_rc(raw));
    }

    socket_registry()
        .lock()
        .expect("socket registry poisoned")
        .insert(
            raw,
            SocketMeta {
                domain,
                socket_type,
                protocol,
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

    match address {
        SocketAddr::V4(address) => {
            rc_to_io(unsafe {
                vcabi::trueos_cabi_socket_tcp_bind_v4(
                    socket as u32,
                    u32::from_be_bytes(address.ip().octets()),
                    address.port().to_be(),
                )
            })?;
        }
        SocketAddr::V6(address) => {
            rc_to_io(unsafe {
                vcabi::trueos_cabi_socket_tcp_bind_v6(
                    socket as u32,
                    address.ip().octets().as_ptr(),
                    address.port().to_be(),
                )
            })?;
        }
    }

    let _ = with_meta_mut(socket, |meta| meta.local = Some(address));
    Ok(())
}

pub(crate) fn connect(socket: RawSocket, address: &SockAddr) -> io::Result<()> {
    let Some(address) = address.as_socket() else {
        return Err(unsupported());
    };
    let nonblocking = with_meta(socket, |meta| meta.nonblocking)?;

    let rc = match address {
        SocketAddr::V4(address) => unsafe {
            vcabi::trueos_cabi_socket_tcp_connect_v4(
                socket as u32,
                u32::from_be_bytes(address.ip().octets()),
                address.port().to_be(),
                nonblocking as u32,
            )
        },
        SocketAddr::V6(address) => unsafe {
            vcabi::trueos_cabi_socket_tcp_connect_v6(
                socket as u32,
                address.ip().octets().as_ptr(),
                address.port().to_be(),
                nonblocking as u32,
            )
        },
    };

    let result = rc_to_io(rc);
    if result.is_ok() || result.as_ref().err().and_then(|err| err.raw_os_error()) == Some(115) {
        let _ = with_meta_mut(socket, |meta| meta.peer = Some(address));
    }
    result
}

pub(crate) fn poll_connect(socket: &crate::Socket, timeout: Duration) -> io::Result<()> {
    rc_to_io(unsafe {
        vcabi::trueos_cabi_socket_tcp_poll_connect(
            socket.as_raw() as u32,
            timeout.as_millis().min(u128::from(u64::MAX)) as u64,
        )
    })
}

pub(crate) fn listen(_: RawSocket, _: c_int) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn accept(_: RawSocket) -> io::Result<(RawSocket, SockAddr)> {
    Err(unsupported())
}

pub(crate) fn getsockname(socket: RawSocket) -> io::Result<SockAddr> {
    let address = with_meta(socket, |meta| {
        meta.local
            .unwrap_or_else(|| default_local_addr(meta.domain))
    })?;
    Ok(SockAddr::from(address))
}

pub(crate) fn getpeername(socket: RawSocket) -> io::Result<SockAddr> {
    let address =
        with_meta(socket, |meta| meta.peer)?.ok_or_else(|| io::Error::from_raw_os_error(107))?;
    Ok(SockAddr::from(address))
}

pub(crate) fn try_clone(_: RawSocket) -> io::Result<RawSocket> {
    Err(unsupported())
}

pub(crate) fn nonblocking(socket: RawSocket) -> io::Result<bool> {
    with_meta(socket, |meta| meta.nonblocking)
}

pub(crate) fn set_nonblocking(socket: RawSocket, nonblocking: bool) -> io::Result<()> {
    rc_to_io(unsafe {
        vcabi::trueos_cabi_socket_tcp_set_nonblocking(socket as u32, nonblocking as u32)
    })?;
    let _ = with_meta_mut(socket, |meta| meta.nonblocking = nonblocking)?;
    Ok(())
}

pub(crate) fn shutdown(socket: RawSocket, how: Shutdown) -> io::Result<()> {
    let how = match how {
        Shutdown::Read => 0,
        Shutdown::Write => 1,
        Shutdown::Both => 2,
    };
    rc_to_io(unsafe { vcabi::trueos_cabi_socket_tcp_shutdown(socket as u32, how) })
}

pub(crate) fn recv(
    socket: RawSocket,
    buf: &mut [MaybeUninit<u8>],
    flags: c_int,
) -> io::Result<usize> {
    let (nonblocking, timeout) = with_meta(socket, |meta| (meta.nonblocking, meta.recv_timeout))?;
    ssize_to_io(unsafe {
        vcabi::trueos_cabi_socket_tcp_recv(
            socket as u32,
            buf.as_mut_ptr().cast::<u8>(),
            buf.len(),
            flags,
            nonblocking as u32,
            timeout_to_cabi(timeout),
        )
    })
}

pub(crate) fn recv_from(
    _: RawSocket,
    _: &mut [MaybeUninit<u8>],
    _: c_int,
) -> io::Result<(usize, SockAddr)> {
    Err(unsupported())
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
    ssize_to_io(unsafe {
        vcabi::trueos_cabi_socket_tcp_send(socket as u32, buf.as_ptr(), buf.len())
    })
}

pub(crate) fn send_vectored(
    socket: RawSocket,
    bufs: &[std::io::IoSlice<'_>],
    flags: c_int,
) -> io::Result<usize> {
    let total = bufs.iter().map(|buf| buf.len()).sum();
    let mut merged = Vec::with_capacity(total);
    for buf in bufs {
        merged.extend_from_slice(buf);
    }
    send(socket, &merged, flags)
}

pub(crate) fn send_to(_: RawSocket, _: &[u8], _: &SockAddr, _: c_int) -> io::Result<usize> {
    Err(unsupported())
}

pub(crate) fn send_to_vectored(
    _: RawSocket,
    _: &[std::io::IoSlice<'_>],
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
            let value = unsafe { vcabi::trueos_cabi_socket_tcp_take_error(socket as u32) };
            if value < 0 {
                Err(io_error_from_neg_rc(value))
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

pub(crate) fn unix_sockaddr(_: &std::path::Path) -> io::Result<SockAddr> {
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
        let _ = unsafe { vcabi::trueos_cabi_socket_tcp_close(self.0 as u32) };
        let _ = socket_registry()
            .lock()
            .expect("socket registry poisoned")
            .remove(&self.0);
    }
}

impl std::fmt::Debug for crate::Domain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Domain").field(&self.0).finish()
    }
}

impl std::fmt::Debug for crate::Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Type").field(&self.0).finish()
    }
}

impl std::fmt::Debug for crate::Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Protocol").field(&self.0).finish()
    }
}

impl std::fmt::Debug for crate::RecvFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RecvFlags").field(&self.0).finish()
    }
}
