//! Initially unconnected TCP socket owned by the native network service.
use core::net::SocketAddr;
use trueos_io as io;

use crate::zkvm_net::Socket;
use crate::net::{TcpListener, TcpStream};

/// A native TCP socket with a stable service identity from creation to close.
///
/// Binding reserves the endpoint in the network service immediately. Connect
/// and listen consume this owner and preserve that reservation and its options.
#[derive(Debug)]
pub struct TcpSocket {
    inner: Socket,
}

impl TcpSocket {
    /// Create an IPv4 TCP socket.
    pub fn new_v4() -> io::Result<Self> {
        Ok(Self { inner: Socket::tcp_socket(4)? })
    }

    /// Create an IPv6 TCP socket if supported by the network service.
    pub fn new_v6() -> io::Result<Self> {
        Ok(Self { inner: Socket::tcp_socket(6)? })
    }

    /// Reserve a local endpoint before connecting or listening.
    pub fn bind(&self, addr: SocketAddr) -> io::Result<()> {
        self.inner.bind(addr)
    }

    /// Return the endpoint reserved by this socket.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    /// Take a pending error from the native socket.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.inner.take_error()
    }

    /// Submit a connection while retaining this socket's identity and options.
    pub fn connect(self, addr: SocketAddr) -> io::Result<TcpStream> {
        self.inner.connect(addr)?;
        Ok(TcpStream::from_zkvm_socket(self.inner))
    }

    /// Listen using the reserved endpoint and requested pending queue capacity.
    ///
    /// The current network service supports up to 32 pending connections. Zero
    /// requests one slot; larger capacities return an error.
    pub fn listen(self, backlog: u32) -> io::Result<TcpListener> {
        self.inner.listen(backlog)?;
        Ok(TcpListener::from_zkvm_socket(self.inner))
    }

    /// Set whether TCP sends without waiting for Nagle coalescing.
    pub fn set_nodelay(&self, value: bool) -> io::Result<()> {
        self.inner.option(1, Some(u32::from(value))).map(|_| ())
    }

    /// Return the socket's Nagle policy.
    pub fn nodelay(&self) -> io::Result<bool> {
        self.inner.option(1, None).map(|value| value != 0)
    }
}
