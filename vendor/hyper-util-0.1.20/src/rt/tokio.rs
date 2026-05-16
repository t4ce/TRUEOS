//! [`tokio`] runtime components integration for [`hyper`].
//!
//! [`hyper::rt`] exposes a set of traits to allow hyper to be agnostic to
//! its underlying asynchronous runtime. This submodule provides glue for
//! [`tokio`] users to bridge those types to [`hyper`]'s interfaces.
//!
//! # IO
//!
//! [`hyper`] abstracts over asynchronous readers and writers using [`Read`]
//! and [`Write`], while [`tokio`] abstracts over this using [`AsyncRead`]
//! and [`AsyncWrite`]. This submodule provides a collection of IO adaptors
//! to bridge these two IO ecosystems together: [`TokioIo<I>`],
//! [`WithHyperIo<I>`], and [`WithTokioIo<I>`].
//!
//! To compare and constrast these IO adaptors and to help explain which
//! is the proper choice for your needs, here is a table showing which IO
//! traits these implement, given two types `T` and `H` which implement
//! Tokio's and Hyper's corresponding IO traits:
//!
//! |                    | [`AsyncRead`]    | [`AsyncWrite`]    | [`Read`]     | [`Write`]    |
//! |--------------------|------------------|-------------------|--------------|--------------|
//! | `T`                | ✅ **true**      | ✅ **true**       | ❌ **false** | ❌ **false** |
//! | `H`                | ❌ **false**     | ❌ **false**      | ✅ **true**  | ✅ **true**  |
//! | [`TokioIo<T>`]     | ❌ **false**     | ❌ **false**      | ✅ **true**  | ✅ **true**  |
//! | [`TokioIo<H>`]     | ✅ **true**      | ✅ **true**       | ❌ **false** | ❌ **false** |
//! | [`WithHyperIo<T>`] | ✅ **true**      | ✅ **true**       | ✅ **true**  | ✅ **true**  |
//! | [`WithHyperIo<H>`] | ❌ **false**     | ❌ **false**      | ❌ **false** | ❌ **false** |
//! | [`WithTokioIo<T>`] | ❌ **false**     | ❌ **false**      | ❌ **false** | ❌ **false** |
//! | [`WithTokioIo<H>`] | ✅ **true**      | ✅ **true**       | ✅ **true**  | ✅ **true**  |
//!
//! For most situations, [`TokioIo<I>`] is the proper choice. This should be
//! constructed, wrapping some underlying [`hyper`] or [`tokio`] IO, at the
//! call-site of a function like [`hyper::client::conn::http1::handshake`].
//!
//! [`TokioIo<I>`] switches across these ecosystems, but notably does not
//! preserve the existing IO trait implementations of its underlying IO. If
//! one wishes to _extend_ IO with additional implementations,
//! [`WithHyperIo<I>`] and [`WithTokioIo<I>`] are the correct choice.
//!
//! For example, a Tokio reader/writer can be wrapped in [`WithHyperIo<I>`].
//! That will implement _both_ sets of IO traits. Conversely,
//! [`WithTokioIo<I>`] will implement both sets of IO traits given a
//! reader/writer that implements Hyper's [`Read`] and [`Write`].
//!
//! See [`tokio::io`] and ["_Asynchronous IO_"][tokio-async-docs] for more
//! information.
//!
//! [`AsyncRead`]: tokio::io::AsyncRead
//! [`AsyncWrite`]: tokio::io::AsyncWrite
//! [`Read`]: hyper::rt::Read
//! [`Write`]: hyper::rt::Write
//! [tokio-async-docs]: https://docs.rs/tokio/latest/tokio/#asynchronous-io

use std::{
    future::Future,
    io as tokio_io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use hyper::{io as hyper_io, time::Instant};
use hyper::rt::{Executor, Sleep, Timer};
use pin_project_lite::pin_project;

#[cfg(feature = "tracing")]
use tracing::instrument::Instrument;

pub use self::{with_hyper_io::WithHyperIo, with_tokio_io::WithTokioIo};

mod with_hyper_io;
mod with_tokio_io;

fn tokio_to_hyper_kind(kind: tokio_io::ErrorKind) -> hyper_io::ErrorKind {
    match kind {
        tokio_io::ErrorKind::NotFound => hyper_io::ErrorKind::NotFound,
        tokio_io::ErrorKind::PermissionDenied => hyper_io::ErrorKind::PermissionDenied,
        tokio_io::ErrorKind::ConnectionRefused => hyper_io::ErrorKind::ConnectionRefused,
        tokio_io::ErrorKind::ConnectionReset => hyper_io::ErrorKind::ConnectionReset,
        tokio_io::ErrorKind::ConnectionAborted => hyper_io::ErrorKind::ConnectionAborted,
        tokio_io::ErrorKind::NotConnected => hyper_io::ErrorKind::NotConnected,
        tokio_io::ErrorKind::AddrInUse => hyper_io::ErrorKind::AddrInUse,
        tokio_io::ErrorKind::AddrNotAvailable => hyper_io::ErrorKind::AddrNotAvailable,
        tokio_io::ErrorKind::BrokenPipe => hyper_io::ErrorKind::BrokenPipe,
        tokio_io::ErrorKind::AlreadyExists => hyper_io::ErrorKind::AlreadyExists,
        tokio_io::ErrorKind::WouldBlock => hyper_io::ErrorKind::WouldBlock,
        tokio_io::ErrorKind::InvalidInput => hyper_io::ErrorKind::InvalidInput,
        tokio_io::ErrorKind::InvalidData => hyper_io::ErrorKind::InvalidData,
        tokio_io::ErrorKind::TimedOut => hyper_io::ErrorKind::TimedOut,
        tokio_io::ErrorKind::WriteZero => hyper_io::ErrorKind::WriteZero,
        tokio_io::ErrorKind::Interrupted => hyper_io::ErrorKind::Interrupted,
        tokio_io::ErrorKind::UnexpectedEof => hyper_io::ErrorKind::UnexpectedEof,
        _ => hyper_io::ErrorKind::Other,
    }
}

fn hyper_to_tokio_kind(kind: hyper_io::ErrorKind) -> tokio_io::ErrorKind {
    match kind {
        hyper_io::ErrorKind::NotFound => tokio_io::ErrorKind::NotFound,
        hyper_io::ErrorKind::PermissionDenied => tokio_io::ErrorKind::PermissionDenied,
        hyper_io::ErrorKind::ConnectionRefused => tokio_io::ErrorKind::ConnectionRefused,
        hyper_io::ErrorKind::ConnectionReset => tokio_io::ErrorKind::ConnectionReset,
        hyper_io::ErrorKind::ConnectionAborted => tokio_io::ErrorKind::ConnectionAborted,
        hyper_io::ErrorKind::NotConnected => tokio_io::ErrorKind::NotConnected,
        hyper_io::ErrorKind::AddrInUse => tokio_io::ErrorKind::AddrInUse,
        hyper_io::ErrorKind::AddrNotAvailable => tokio_io::ErrorKind::AddrNotAvailable,
        hyper_io::ErrorKind::BrokenPipe => tokio_io::ErrorKind::BrokenPipe,
        hyper_io::ErrorKind::AlreadyExists => tokio_io::ErrorKind::AlreadyExists,
        hyper_io::ErrorKind::WouldBlock => tokio_io::ErrorKind::WouldBlock,
        hyper_io::ErrorKind::InvalidInput => tokio_io::ErrorKind::InvalidInput,
        hyper_io::ErrorKind::InvalidData => tokio_io::ErrorKind::InvalidData,
        hyper_io::ErrorKind::TimedOut => tokio_io::ErrorKind::TimedOut,
        hyper_io::ErrorKind::WriteZero => tokio_io::ErrorKind::WriteZero,
        hyper_io::ErrorKind::Interrupted => tokio_io::ErrorKind::Interrupted,
        hyper_io::ErrorKind::UnexpectedEof => tokio_io::ErrorKind::UnexpectedEof,
        _ => tokio_io::ErrorKind::Other,
    }
}

fn tokio_to_hyper_error(err: tokio_io::Error) -> hyper_io::Error {
    hyper_io::Error::new(tokio_to_hyper_kind(err.kind()), "tokio io error")
}

fn hyper_to_tokio_error(err: hyper_io::Error) -> tokio_io::Error {
    tokio_io::Error::new(hyper_to_tokio_kind(err.kind()), "hyper io error")
}

fn tokio_to_hyper_slices<'buf>(
    bufs: &'buf [tokio_io::IoSlice<'buf>],
) -> Vec<hyper_io::IoSlice<'buf>> {
    bufs.iter().map(|buf| hyper_io::IoSlice::new(&**buf)).collect()
}

fn hyper_to_tokio_slices<'buf>(
    bufs: &'buf [hyper_io::IoSlice<'buf>],
) -> Vec<tokio_io::IoSlice<'buf>> {
    bufs.iter().map(|buf| tokio_io::IoSlice::new(&**buf)).collect()
}

/// Future executor that utilises `tokio` threads.
#[non_exhaustive]
#[derive(Default, Debug, Clone)]
pub struct TokioExecutor {}

pin_project! {
    /// A wrapper that implements Tokio's IO traits for an inner type that
    /// implements hyper's IO traits, or vice versa (implements hyper's IO
    /// traits for a type that implements Tokio's IO traits).
    #[derive(Debug)]
    pub struct TokioIo<T> {
        #[pin]
        inner: T,
    }
}

/// A Timer that uses the tokio runtime.
#[non_exhaustive]
#[derive(Default, Clone, Debug)]
pub struct TokioTimer;

// Use TokioSleep to get tokio::time::Sleep to implement Unpin.
// see https://docs.rs/tokio/latest/tokio/time/struct.Sleep.html
pin_project! {
    #[derive(Debug)]
    struct TokioSleep {
        #[pin]
        inner: tokio::time::Sleep,
    }
}

// ===== impl TokioExecutor =====

impl<Fut> Executor<Fut> for TokioExecutor
where
    Fut: Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    fn execute(&self, fut: Fut) {
        #[cfg(feature = "tracing")]
        tokio::spawn(fut.in_current_span());

        #[cfg(not(feature = "tracing"))]
        tokio::spawn(fut);
    }
}

impl TokioExecutor {
    /// Create new executor that relies on [`tokio::spawn`] to execute futures.
    pub fn new() -> Self {
        Self {}
    }
}

// ==== impl TokioIo =====

impl<T> TokioIo<T> {
    /// Wrap a type implementing Tokio's or hyper's IO traits.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Borrow the inner type.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Mut borrow the inner type.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Consume this wrapper and get the inner type.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> hyper::rt::Read for TokioIo<T>
where
    T: tokio::io::AsyncRead,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), hyper_io::Error>> {
        let n = unsafe {
            let mut tbuf = tokio::io::ReadBuf::uninit(buf.as_mut());
            match tokio::io::AsyncRead::poll_read(self.project().inner, cx, &mut tbuf) {
                Poll::Ready(Ok(())) => tbuf.filled().len(),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(tokio_to_hyper_error(err))),
                Poll::Pending => return Poll::Pending,
            }
        };

        unsafe {
            buf.advance(n);
        }
        Poll::Ready(Ok(()))
    }
}

impl<T> hyper::rt::Write for TokioIo<T>
where
    T: tokio::io::AsyncWrite,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, hyper_io::Error>> {
        tokio::io::AsyncWrite::poll_write(self.project().inner, cx, buf)
            .map_err(tokio_to_hyper_error)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), hyper_io::Error>> {
        tokio::io::AsyncWrite::poll_flush(self.project().inner, cx).map_err(tokio_to_hyper_error)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), hyper_io::Error>> {
        tokio::io::AsyncWrite::poll_shutdown(self.project().inner, cx)
            .map_err(tokio_to_hyper_error)
    }

    fn is_write_vectored(&self) -> bool {
        tokio::io::AsyncWrite::is_write_vectored(&self.inner)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[hyper_io::IoSlice<'_>],
    ) -> Poll<Result<usize, hyper_io::Error>> {
        let bufs = hyper_to_tokio_slices(bufs);
        tokio::io::AsyncWrite::poll_write_vectored(self.project().inner, cx, &bufs)
            .map_err(tokio_to_hyper_error)
    }
}

impl<T> tokio::io::AsyncRead for TokioIo<T>
where
    T: hyper::rt::Read,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        tbuf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<Result<(), tokio_io::Error>> {
        //let init = tbuf.initialized().len();
        let filled = tbuf.filled().len();
        let sub_filled = unsafe {
            let mut buf = hyper::rt::ReadBuf::uninit(tbuf.unfilled_mut());

            match hyper::rt::Read::poll_read(self.project().inner, cx, buf.unfilled()) {
                Poll::Ready(Ok(())) => buf.filled().len(),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(hyper_to_tokio_error(err))),
                Poll::Pending => return Poll::Pending,
            }
        };

        let n_filled = filled + sub_filled;
        // At least sub_filled bytes had to have been initialized.
        let n_init = sub_filled;
        unsafe {
            tbuf.assume_init(n_init);
            tbuf.set_filled(n_filled);
        }

        Poll::Ready(Ok(()))
    }
}

impl<T> tokio::io::AsyncWrite for TokioIo<T>
where
    T: hyper::rt::Write,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, tokio_io::Error>> {
        hyper::rt::Write::poll_write(self.project().inner, cx, buf).map_err(hyper_to_tokio_error)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), tokio_io::Error>> {
        hyper::rt::Write::poll_flush(self.project().inner, cx).map_err(hyper_to_tokio_error)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), tokio_io::Error>> {
        hyper::rt::Write::poll_shutdown(self.project().inner, cx).map_err(hyper_to_tokio_error)
    }

    fn is_write_vectored(&self) -> bool {
        hyper::rt::Write::is_write_vectored(&self.inner)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[tokio_io::IoSlice<'_>],
    ) -> Poll<Result<usize, tokio_io::Error>> {
        let bufs = tokio_to_hyper_slices(bufs);
        hyper::rt::Write::poll_write_vectored(self.project().inner, cx, &bufs)
            .map_err(hyper_to_tokio_error)
    }
}

// ==== impl TokioTimer =====

impl Timer for TokioTimer {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Sleep>> {
        Box::pin(TokioSleep {
            inner: tokio::time::sleep(duration),
        })
    }

    fn sleep_until(&self, deadline: Instant) -> Pin<Box<dyn Sleep>> {
        Box::pin(TokioSleep {
            inner: tokio::time::sleep_until(deadline.into()),
        })
    }

    fn reset(&self, sleep: &mut Pin<Box<dyn Sleep>>, new_deadline: Instant) {
        if let Some(sleep) = sleep.as_mut().downcast_mut_pin::<TokioSleep>() {
            sleep.reset(new_deadline)
        }
    }

    fn now(&self) -> Instant {
        tokio::time::Instant::now().into()
    }
}

impl TokioTimer {
    /// Create a new TokioTimer
    pub fn new() -> Self {
        Self {}
    }
}

impl Future for TokioSleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

impl Sleep for TokioSleep {}

impl TokioSleep {
    fn reset(self: Pin<&mut Self>, deadline: Instant) {
        self.project().inner.as_mut().reset(deadline.into());
    }
}

#[cfg(test)]
mod tests {
    use crate::rt::TokioExecutor;
    use hyper::rt::Executor;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn simple_execute() -> Result<(), Box<dyn core::error::Error>> {
        let (tx, rx) = oneshot::channel();
        let executor = TokioExecutor::new();
        executor.execute(async move {
            tx.send(()).unwrap();
        });
        rx.await.map_err(Into::into)
    }
}
