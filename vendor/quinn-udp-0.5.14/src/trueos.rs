//! TRUEOS UDP fallback.
//!
//! The Tokio runtime drives the platform socket directly. This state object
//! preserves Quinn's portable UDP metadata API without pretending that TRUEOS
//! socket identifiers are Unix file descriptors.

use std::io::{self, IoSliceMut};

use super::{RecvMeta, Transmit, UdpSockRef};

pub const BATCH_SIZE: usize = 1;

#[derive(Debug, Default)]
pub struct UdpSocketState;

impl UdpSocketState {
    pub fn new(_socket: UdpSockRef<'_>) -> io::Result<Self> {
        Ok(Self)
    }

    pub fn send(&self, _socket: UdpSockRef<'_>, _transmit: &Transmit<'_>) -> io::Result<()> {
        Err(unsupported())
    }

    pub fn try_send(&self, _socket: UdpSockRef<'_>, _transmit: &Transmit<'_>) -> io::Result<()> {
        Err(unsupported())
    }

    pub fn recv(
        &self,
        _socket: UdpSockRef<'_>,
        _bufs: &mut [IoSliceMut<'_>],
        _meta: &mut [RecvMeta],
    ) -> io::Result<usize> {
        Err(unsupported())
    }

    pub fn max_gso_segments(&self) -> usize {
        1
    }

    pub fn gro_segments(&self) -> usize {
        1
    }

    pub fn set_send_buffer_size(&self, _socket: UdpSockRef<'_>, _bytes: usize) -> io::Result<()> {
        Ok(())
    }

    pub fn set_recv_buffer_size(&self, _socket: UdpSockRef<'_>, _bytes: usize) -> io::Result<()> {
        Ok(())
    }

    pub fn send_buffer_size(&self, _socket: UdpSockRef<'_>) -> io::Result<usize> {
        Ok(0)
    }

    pub fn recv_buffer_size(&self, _socket: UdpSockRef<'_>) -> io::Result<usize> {
        Ok(0)
    }

    pub fn may_fragment(&self) -> bool {
        true
    }
}

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "use Quinn's TRUEOS Tokio UDP runtime")
}
