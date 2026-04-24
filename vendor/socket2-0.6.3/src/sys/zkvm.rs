use std::io;
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown};
use std::ptr;
use std::time::Duration;

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

pub(crate) unsafe fn socket_from_raw(socket: RawSocket) -> Socket {
    Socket(socket)
}

pub(crate) fn socket_as_raw(socket: &Socket) -> RawSocket {
    socket.0
}

pub(crate) fn socket_into_raw(socket: Socket) -> RawSocket {
    socket.0
}

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "socket2 zkvm backend is not wired to TRUEOS net yet",
    )
}

pub(crate) fn socket(_: c_int, _: c_int, _: c_int) -> io::Result<RawSocket> {
    Err(unsupported())
}

pub(crate) fn socketpair(_: c_int, _: c_int, _: c_int) -> io::Result<[RawSocket; 2]> {
    Err(unsupported())
}

pub(crate) fn bind(_: RawSocket, _: &SockAddr) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn connect(_: RawSocket, _: &SockAddr) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn poll_connect(_: &crate::Socket, _: Duration) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn listen(_: RawSocket, _: c_int) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn accept(_: RawSocket) -> io::Result<(RawSocket, SockAddr)> {
    Err(unsupported())
}

pub(crate) fn getsockname(_: RawSocket) -> io::Result<SockAddr> {
    Err(unsupported())
}

pub(crate) fn getpeername(_: RawSocket) -> io::Result<SockAddr> {
    Err(unsupported())
}

pub(crate) fn try_clone(_: RawSocket) -> io::Result<RawSocket> {
    Err(unsupported())
}

pub(crate) fn nonblocking(_: RawSocket) -> io::Result<bool> {
    Err(unsupported())
}

pub(crate) fn set_nonblocking(_: RawSocket, _: bool) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn shutdown(_: RawSocket, _: Shutdown) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn recv(_: RawSocket, _: &mut [MaybeUninit<u8>], _: c_int) -> io::Result<usize> {
    Err(unsupported())
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

pub(crate) fn send(_: RawSocket, _: &[u8], _: c_int) -> io::Result<usize> {
    Err(unsupported())
}

pub(crate) fn send_vectored(_: RawSocket, _: &[std::io::IoSlice<'_>], _: c_int) -> io::Result<usize> {
    Err(unsupported())
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

pub(crate) fn timeout_opt(_: RawSocket, _: c_int, _: c_int) -> io::Result<Option<Duration>> {
    Err(unsupported())
}

pub(crate) fn set_timeout_opt(_: RawSocket, _: c_int, _: c_int, _: Option<Duration>) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) fn tcp_keepalive_time(_: RawSocket) -> io::Result<Duration> {
    Err(unsupported())
}

pub(crate) fn set_tcp_keepalive(_: RawSocket, _: &TcpKeepalive) -> io::Result<()> {
    Err(unsupported())
}

pub(crate) unsafe fn getsockopt<T>(_: RawSocket, _: c_int, _: c_int) -> io::Result<T> {
    Err(unsupported())
}

pub(crate) unsafe fn setsockopt<T>(_: RawSocket, _: c_int, _: c_int, _: T) -> io::Result<()> {
    Err(unsupported())
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
        let _ = ptr::addr_of!(self.0);
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
