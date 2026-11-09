//! Host aliases for the real native std networking provider, never host fds.
#![feature(core_io_borrowed_buf, read_buf)]
#![allow(dead_code)]
pub use std::{mem, sync, thread, time, vec};
pub mod net { pub use std::net::*; }
pub mod io {
    pub use std::io::*;
    pub fn default_read_vectored<F: FnOnce(&mut [u8]) -> Result<usize>>(f: F, bufs: &mut [IoSliceMut<'_>]) -> Result<usize> {
        for buf in bufs { if !buf.is_empty() { return f(buf); } } f(&mut [])
    }
    pub fn default_write_vectored<F: FnOnce(&[u8]) -> Result<usize>>(f: F, bufs: &[IoSlice<'_>]) -> Result<usize> {
        for buf in bufs { if !buf.is_empty() { return f(buf); } } f(&[])
    }
}
mod connection {
    pub fn each_addr<A: std::net::ToSocketAddrs, F, T>(addr: A, mut f: F) -> std::io::Result<T>
    where F: FnMut(&std::net::SocketAddr) -> std::io::Result<T> {
        let mut failure = None;
        for address in addr.to_socket_addrs()? {
            match f(&address) { Ok(value) => return Ok(value), Err(error) => failure = Some(error) }
        }
        Err(failure.unwrap_or(std::io::ErrorKind::InvalidInput.into()))
    }
    pub mod native { NATIVE_PROVIDER }
}
pub use connection::native::NativeSocket;
#[derive(Debug)] pub struct TcpStream(connection::native::TcpStream);
#[derive(Debug)] pub struct TcpListener(connection::native::TcpListener);
#[derive(Debug)] pub struct UdpSocket(connection::native::UdpSocket);
macro_rules! capability { ($($ty:ident),*) => {$ (impl $ty {
    pub fn from_native_socket(socket: NativeSocket) -> Self { Self(connection::native::$ty::from_native(socket)) }
    pub fn native_socket(&self) -> &NativeSocket { self.0.native() }
    pub fn into_native_socket(self) -> NativeSocket { self.0.into_native() }
    pub fn set_nonblocking(&self, value: bool) -> io::Result<()> { self.0.set_nonblocking(value) }
    pub fn local_addr(&self) -> io::Result<net::SocketAddr> { self.0.socket_addr() }
    pub fn try_clone(&self) -> io::Result<Self> { self.0.duplicate().map(Self) }
    pub fn take_error(&self) -> io::Result<Option<io::Error>> { self.0.take_error() }
})*}; }
capability!(TcpStream, TcpListener, UdpSocket);
impl TcpListener {
    pub fn bind(addr: impl net::ToSocketAddrs) -> io::Result<Self> { connection::native::TcpListener::bind(addr).map(Self) }
    pub fn accept(&self) -> io::Result<(TcpStream, net::SocketAddr)> { self.0.accept().map(|(socket, peer)| (TcpStream(socket), peer)) }
}
impl TcpStream {
    pub fn connect(addr: impl net::ToSocketAddrs) -> io::Result<Self> { connection::native::TcpStream::connect(addr).map(Self) }
    pub fn shutdown(&self, how: net::Shutdown) -> io::Result<()> { self.0.shutdown(how) }
}
impl io::Read for TcpStream { fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> { self.0.read(bytes) } }
impl io::Write for TcpStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { self.0.write(bytes) }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
impl UdpSocket {
    pub fn bind(addr: impl net::ToSocketAddrs) -> io::Result<Self> { connection::native::UdpSocket::bind(addr).map(Self) }
    pub fn send_to(&self, bytes: &[u8], addr: impl net::ToSocketAddrs) -> io::Result<usize> {
        connection::each_addr(addr, |addr| self.0.send_to(bytes, addr))
    }
    pub fn recv_from(&self, bytes: &mut [u8]) -> io::Result<(usize, net::SocketAddr)> { self.0.recv_from(bytes) }
}
