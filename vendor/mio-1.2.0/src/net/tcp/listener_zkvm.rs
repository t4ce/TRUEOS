#![allow(missing_docs)]

use std::{fmt, io, net};
use std::net::SocketAddr;

use crate::zkvm_net::Socket;
use crate::{event, Interest, Registry, Token};

#[derive(Debug)]
pub struct TcpListener {
    inner: Socket,
}

impl TcpListener {
    pub fn bind(addr: SocketAddr) -> io::Result<TcpListener> {
        Ok(TcpListener {
            inner: Socket::tcp_listener_bind(addr)?,
        })
    }

    pub fn from_std(_: net::TcpListener) -> TcpListener {
        panic!("mio zkvm backend cannot wrap std::net::TcpListener yet")
    }

    pub fn accept(&self) -> io::Result<(crate::net::TcpStream, SocketAddr)> {
        let (socket, addr) = self.inner.accept()?;
        Ok((crate::net::TcpStream::from_zkvm_socket(socket), addr))
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    pub fn set_ttl(&self, _: u32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "mio zkvm TcpListener::set_ttl is not wired yet",
        ))
    }

    pub fn ttl(&self) -> io::Result<u32> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "mio zkvm TcpListener::ttl is not wired yet",
        ))
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.inner.take_error()
    }
}

impl event::Source for TcpListener {
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

impl From<TcpListener> for net::TcpListener {
    fn from(_: TcpListener) -> Self {
        panic!("mio zkvm backend cannot convert TcpListener into std yet")
    }
}

impl fmt::Display for TcpListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TcpListener(..)")
    }
}