#![allow(missing_docs)]

use ::core::fmt;
use core::net::SocketAddr;
use crate::io;

use crate::zkvm_net::Socket;
use crate::{event, Interest, Registry, Token};

#[derive(Debug)]
pub struct TcpListener {
    inner: Socket,
}

impl TcpListener {
    /// Move an existing native std capability without converting a Unix fd.
    #[cfg(all(target_os = "trueos", feature = "std-compat"))]
    pub fn from_std(socket: std::net::TcpListener) -> Self {
        Self { inner: Socket::from_std_native(socket.into_native_socket()) }
    }

    /// Return a retained std capability after reactor deregistration.
    /// A socket created directly by Mio has no std owner and is Unsupported.
    #[cfg(all(target_os = "trueos", feature = "std-compat"))]
    pub fn into_std(self) -> io::Result<std::net::TcpListener> {
        self.inner.into_std_native().map(std::net::TcpListener::from_native_socket)
    }
    pub(crate) fn from_zkvm_socket(inner: Socket) -> Self {
        Self { inner }
    }

    pub fn bind(addr: SocketAddr) -> io::Result<TcpListener> {
        Ok(TcpListener {
            inner: Socket::tcp_listener_bind(addr)?,
        })
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
            io::ErrorKind::Other,
            "mio zkvm TcpListener::set_ttl is not wired yet",
        ))
    }

    pub fn ttl(&self) -> io::Result<u32> {
        Err(io::Error::new(
            io::ErrorKind::Other,
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

impl fmt::Display for TcpListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TcpListener(..)")
    }
}
