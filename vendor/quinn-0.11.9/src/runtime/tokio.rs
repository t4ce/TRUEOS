use std::{
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
    time::Instant,
};

use tokio::{
    io::Interest,
    time::{Sleep, sleep_until},
};

use super::{AsyncTimer, AsyncUdpSocket, Runtime, UdpPollHelper};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn tokio_error_kind(kind: tokio::io::ErrorKind) -> io::ErrorKind {
    match kind {
        tokio::io::ErrorKind::NotFound => io::ErrorKind::NotFound,
        tokio::io::ErrorKind::PermissionDenied => io::ErrorKind::PermissionDenied,
        tokio::io::ErrorKind::ConnectionRefused => io::ErrorKind::ConnectionRefused,
        tokio::io::ErrorKind::ConnectionReset => io::ErrorKind::ConnectionReset,
        tokio::io::ErrorKind::ConnectionAborted => io::ErrorKind::ConnectionAborted,
        tokio::io::ErrorKind::NotConnected => io::ErrorKind::NotConnected,
        tokio::io::ErrorKind::AddrInUse => io::ErrorKind::AddrInUse,
        tokio::io::ErrorKind::AddrNotAvailable => io::ErrorKind::AddrNotAvailable,
        tokio::io::ErrorKind::BrokenPipe => io::ErrorKind::BrokenPipe,
        tokio::io::ErrorKind::AlreadyExists => io::ErrorKind::AlreadyExists,
        tokio::io::ErrorKind::WouldBlock => io::ErrorKind::WouldBlock,
        tokio::io::ErrorKind::InvalidInput => io::ErrorKind::InvalidInput,
        tokio::io::ErrorKind::InvalidData => io::ErrorKind::InvalidData,
        tokio::io::ErrorKind::TimedOut => io::ErrorKind::TimedOut,
        tokio::io::ErrorKind::WriteZero => io::ErrorKind::WriteZero,
        tokio::io::ErrorKind::Interrupted => io::ErrorKind::Interrupted,
        _ => io::ErrorKind::Other,
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn tokio_to_std_error(error: tokio::io::Error) -> io::Error {
    io::Error::new(tokio_error_kind(error.kind()), "Quinn TRUEOS Tokio I/O error")
}

/// A Quinn runtime for Tokio
#[derive(Debug)]
pub struct TokioRuntime;

impl Runtime for TokioRuntime {
    fn new_timer(&self, t: Instant) -> Pin<Box<dyn AsyncTimer>> {
        Box::pin(sleep_until(t.into()))
    }

    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        tokio::spawn(future);
    }

    fn wrap_udp_socket(&self, sock: std::net::UdpSocket) -> io::Result<Arc<dyn AsyncUdpSocket>> {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            Ok(Arc::new(UdpSocket {
                inner: udp::UdpSocketState::new((&sock).into())?,
                io: tokio::net::UdpSocket::from_std(sock)?,
            }))
        }

        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            Ok(Arc::new(UdpSocket {
                io: tokio::net::UdpSocket::from_std(sock).map_err(tokio_to_std_error)?,
            }))
        }
    }

    fn now(&self) -> Instant {
        tokio::time::Instant::now().into_std()
    }
}

impl AsyncTimer for Sleep {
    fn reset(self: Pin<&mut Self>, t: Instant) {
        Self::reset(self, t.into())
    }
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        Future::poll(self, cx)
    }
}

#[derive(Debug)]
struct UdpSocket {
    io: tokio::net::UdpSocket,
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    inner: udp::UdpSocketState,
}

impl AsyncUdpSocket for UdpSocket {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn super::UdpPoller>> {
        Box::pin(UdpPollHelper::new(move || {
            let socket = self.clone();
            async move {
                #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
                return socket.io.writable().await;

                #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
                socket.io.writable().await.map_err(tokio_to_std_error)
            }
        }))
    }

    fn try_send(&self, transmit: &udp::Transmit) -> io::Result<()> {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            self.io
                .try_io(Interest::WRITABLE, || self.inner.send((&self.io).into(), transmit))
        }

        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            self.io
                .try_send_to(transmit.contents, transmit.destination)
                .map(|_| ())
                .map_err(tokio_to_std_error)
        }
    }

    fn poll_recv(
        &self,
        cx: &mut Context,
        bufs: &mut [std::io::IoSliceMut<'_>],
        meta: &mut [udp::RecvMeta],
    ) -> Poll<io::Result<usize>> {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            loop {
                ready!(self.io.poll_recv_ready(cx))?;
                if let Ok(res) = self
                    .io
                    .try_io(Interest::READABLE, || self.inner.recv((&self.io).into(), bufs, meta))
                {
                    return Poll::Ready(Ok(res));
                }
            }
        }

        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            let Some(buf) = bufs.first_mut() else {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Quinn requires at least one receive buffer",
                )));
            };
            let Some(meta) = meta.first_mut() else {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Quinn requires at least one receive metadata slot",
                )));
            };

            let mut read_buf = tokio::io::ReadBuf::new(&mut **buf);
            match self.io.poll_recv_from(cx, &mut read_buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(error)) => Poll::Ready(Err(tokio_to_std_error(error))),
                Poll::Ready(Ok(addr)) => {
                    let len = read_buf.filled().len();
                    *meta = udp::RecvMeta {
                        addr,
                        len,
                        stride: len,
                        ecn: None,
                        dst_ip: None,
                    };
                    Poll::Ready(Ok(1))
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            self.io.local_addr()
        }

        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            self.io.local_addr().map_err(tokio_to_std_error)
        }
    }

    fn may_fragment(&self) -> bool {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            self.inner.may_fragment()
        }

        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            true
        }
    }

    fn max_transmit_segments(&self) -> usize {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            self.inner.max_gso_segments()
        }

        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            1
        }
    }

    fn max_receive_segments(&self) -> usize {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            self.inner.gro_segments()
        }

        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            1
        }
    }
}
