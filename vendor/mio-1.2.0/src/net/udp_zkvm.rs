#![allow(missing_docs)]

use ::core::fmt;
use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use crate::io;

use crate::zkvm_net::Socket;
use crate::{event, Interest, Registry, Token};

#[derive(Debug)]
pub struct UdpSocket {
    inner: Socket,
}

impl UdpSocket {
    /// Move an existing native std capability without converting a Unix fd.
    #[cfg(all(target_os = "trueos", feature = "std-compat"))]
    pub fn from_std(socket: std::net::UdpSocket) -> Self {
        Self { inner: Socket::from_std_native(socket.into_native_socket()) }
    }

    /// Return a retained std capability after reactor deregistration.
    /// A socket created directly by Mio has no std owner and is Unsupported.
    #[cfg(all(target_os = "trueos", feature = "std-compat"))]
    pub fn into_std(self) -> io::Result<std::net::UdpSocket> {
        self.inner.into_std_native().map(std::net::UdpSocket::from_native_socket)
    }
    pub fn bind(addr: SocketAddr) -> io::Result<UdpSocket> {
        Ok(UdpSocket {
            inner: Socket::udp_bind(addr)?,
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.peer_addr()
    }

    pub fn send_to(&self, buf: &[u8], target: SocketAddr) -> io::Result<usize> {
        self.inner.udp_send_to(buf, target)
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.inner.udp_recv_from(buf)
    }

    pub fn peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.inner.udp_peek_from(buf)
    }

    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        let peer = self.peer_addr()?;
        self.inner.udp_send_to(buf, peer)
    }

    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.udp_recv_from(buf).map(|(len, _)| len)
    }

    pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.udp_peek_from(buf).map(|(len, _)| len)
    }

    pub fn connect(&self, addr: SocketAddr) -> io::Result<()> {
        self.inner.udp_connect(addr)
    }

    pub fn set_broadcast(&self, _: bool) -> io::Result<()> {
        Err(unsupported_option())
    }

    pub fn broadcast(&self) -> io::Result<bool> {
        Err(unsupported_option())
    }

    pub fn set_multicast_loop_v4(&self, _: bool) -> io::Result<()> {
        Err(unsupported_option())
    }

    pub fn multicast_loop_v4(&self) -> io::Result<bool> {
        Err(unsupported_option())
    }

    pub fn set_multicast_ttl_v4(&self, _: u32) -> io::Result<()> {
        Err(unsupported_option())
    }

    pub fn multicast_ttl_v4(&self) -> io::Result<u32> {
        Err(unsupported_option())
    }

    pub fn set_multicast_loop_v6(&self, _: bool) -> io::Result<()> {
        Err(unsupported_option())
    }

    pub fn multicast_loop_v6(&self) -> io::Result<bool> {
        Err(unsupported_option())
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.inner.option(2, Some(ttl)).map(|_| ())
    }

    pub fn ttl(&self) -> io::Result<u32> {
        self.inner.option(2, None)
    }

    pub fn join_multicast_v4(&self, _: &Ipv4Addr, _: &Ipv4Addr) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "mio zkvm UdpSocket::join_multicast_v4 is not wired yet",
        ))
    }

    pub fn join_multicast_v6(&self, _: &Ipv6Addr, _: u32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "mio zkvm UdpSocket::join_multicast_v6 is not wired yet",
        ))
    }

    pub fn leave_multicast_v4(&self, _: &Ipv4Addr, _: &Ipv4Addr) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "mio zkvm UdpSocket::leave_multicast_v4 is not wired yet",
        ))
    }

    pub fn leave_multicast_v6(&self, _: &Ipv6Addr, _: u32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "mio zkvm UdpSocket::leave_multicast_v6 is not wired yet",
        ))
    }

    pub fn only_v6(&self) -> io::Result<bool> {
        Ok(false)
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.inner.take_error()
    }

    pub fn try_io<F, T>(&self, f: F) -> io::Result<T>
    where
        F: FnOnce() -> io::Result<T>,
    {
        f()
    }
}

impl event::Source for UdpSocket {
    fn register(&mut self, registry: &Registry, token: Token, interests: Interest) -> io::Result<()> {
        self.inner.register(registry, token, interests)
    }

    fn reregister(&mut self, registry: &Registry, token: Token, interests: Interest) -> io::Result<()> {
        self.inner.reregister(registry, token, interests)
    }

    fn deregister(&mut self, registry: &Registry) -> io::Result<()> {
        self.inner.deregister(registry)
    }
}

impl fmt::Display for UdpSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UdpSocket(..)")
    }
}

fn unsupported_option() -> io::Error {
    io::Error::new(io::ErrorKind::Other, "native transport does not support this socket option")
}
