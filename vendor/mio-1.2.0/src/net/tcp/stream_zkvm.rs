#![allow(missing_docs)]

use std::fmt;
use std::io::{self, IoSlice, IoSliceMut, Read, Write};
use std::net::{self, Shutdown, SocketAddr};

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

    pub fn from_std(_: net::TcpStream) -> TcpStream {
        panic!("mio zkvm backend cannot wrap std::net::TcpStream yet")
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.peer_addr()
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    pub fn shutdown(&self, _: Shutdown) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "mio zkvm TcpStream::shutdown is not wired yet",
        ))
    }

    pub fn set_nodelay(&self, _: bool) -> io::Result<()> {
        Ok(())
    }

    pub fn nodelay(&self) -> io::Result<bool> {
        Ok(true)
    }

    pub fn set_ttl(&self, _: u32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "mio zkvm TcpStream::set_ttl is not wired yet",
        ))
    }

    pub fn ttl(&self) -> io::Result<u32> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "mio zkvm TcpStream::ttl is not wired yet",
        ))
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.inner.take_error()
    }

    pub fn peek(&self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "mio zkvm TcpStream::peek is not wired yet",
        ))
    }

    pub fn try_io<F, T>(&self, f: F) -> io::Result<T>
    where
        F: FnOnce() -> io::Result<T>,
    {
        f()
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        for buf in bufs {
            if !buf.is_empty() {
                return self.inner.read(buf);
            }
        }
        Ok(0)
    }
}

impl Read for &'_ TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        for buf in bufs {
            if !buf.is_empty() {
                return self.inner.read(buf);
            }
        }
        Ok(0)
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        for buf in bufs {
            if !buf.is_empty() {
                return self.inner.write(buf);
            }
        }
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &'_ TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        for buf in bufs {
            if !buf.is_empty() {
                return self.inner.write(buf);
            }
        }
        Ok(0)
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

impl From<TcpStream> for net::TcpStream {
    fn from(_: TcpStream) -> Self {
        panic!("mio zkvm backend cannot convert TcpStream into std yet")
    }
}

impl fmt::Display for TcpStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TcpStream(..)")
    }
}