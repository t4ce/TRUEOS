#![allow(missing_docs)]

use core::fmt;
use core::net::SocketAddr;
use trueos_io::{self as io, IoSlice, IoSliceMut, Read, Write};

use crate::zkvm_net::Socket;
use crate::{event, Interest, Registry, Token};

#[derive(Debug)]
pub struct TcpStream {
    inner: Socket,
}

impl TcpStream {
    pub fn connect(addr: SocketAddr) -> io::Result<TcpStream> {
        Ok(TcpStream {
            inner: Socket::tcp_stream_connect(addr)?,
        })
    }

    pub(crate) fn from_zkvm_socket(inner: Socket) -> TcpStream {
        TcpStream { inner }
    }

    pub fn from_std(stream: std::net::TcpStream) -> TcpStream {
        stream
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.peer_addr()
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    pub fn shutdown(&self, _: crate::net::Shutdown) -> io::Result<()> {
        self.inner.shutdown()
    }

    pub fn set_nodelay(&self, _: bool) -> io::Result<()> {
        Ok(())
    }

    pub fn nodelay(&self) -> io::Result<bool> {
        Ok(true)
    }

    pub fn set_ttl(&self, _: u32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "mio zkvm TcpStream::set_ttl is not wired yet",
        ))
    }

    pub fn ttl(&self) -> io::Result<u32> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "mio zkvm TcpStream::ttl is not wired yet",
        ))
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.inner.take_error()
    }

    pub fn peek(&self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "mio zkvm TcpStream::peek is not wired yet",
        ))
    }

    pub fn try_io<F, T>(&self, f: F) -> io::Result<T>
    where
        F: FnOnce() -> io::Result<T>,
    {
        f()
    }

    pub fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        if let Some(buf) = bufs.iter_mut().find(|buf| !buf.is_empty()) {
            self.inner.read(buf)
        } else {
            Ok(0)
        }
    }

    pub fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        if let Some(buf) = bufs.iter().find(|buf| !buf.is_empty()) {
            self.inner.write(buf)
        } else {
            Ok(0)
        }
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Read for &'_ TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &'_ TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl event::Source for TcpStream {
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

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl From<TcpStream> for std::net::TcpStream {
    fn from(_: TcpStream) -> Self {
        panic!("mio zkvm backend cannot convert TcpStream into std yet")
    }
}

impl fmt::Display for TcpStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TcpStream(..)")
    }
}
