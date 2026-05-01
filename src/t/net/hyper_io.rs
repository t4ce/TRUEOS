//! Small Hyper/Tokio IO adapters shared by TRUEOS networking code.

extern crate std;

use core::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};
use hyper::{
    body::{Body, Bytes, Frame, SizeHint},
    rt::{Read as HyperRead, ReadBufCursor, Write as HyperWrite},
};
use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

pub struct HyperEmptyBody;

impl Body for HyperEmptyBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(None)
    }

    fn is_end_stream(&self) -> bool {
        true
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(0)
    }
}

pub struct HyperTokioIo<T> {
    inner: T,
}

impl<T> HyperTokioIo<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T> HyperRead for HyperTokioIo<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let limit = buf.remaining().min(2048);
        if limit == 0 {
            return Poll::Ready(Ok(()));
        }

        let mut scratch = [0u8; 2048];
        let mut tokio_buf = tokio::io::ReadBuf::new(&mut scratch[..limit]);
        match Pin::new(&mut self.inner).poll_read(cx, &mut tokio_buf) {
            Poll::Ready(Ok(())) => {
                buf.put_slice(tokio_buf.filled());
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> HyperWrite for HyperTokioIo<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
