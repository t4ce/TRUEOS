use crate::io::{AsyncRead, AsyncWrite, ReadBuf};
use crate::process::kill::Kill;
use crate::process::SpawnedChild;

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::process::{Child as StdChild, ExitStatus, Stdio};
use std::task::{Context, Poll};

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "tokio process is not supported on zkvm",
    )
}

#[must_use = "futures do nothing unless polled"]
pub(crate) struct Child {
    child: StdChild,
}

impl core::fmt::Debug for Child {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt.debug_struct("Child").field("pid", &self.id()).finish()
    }
}

pub(crate) fn build_child(child: StdChild) -> io::Result<SpawnedChild> {
    Ok(SpawnedChild {
        child: Child { child },
        stdin: None,
        stdout: None,
        stderr: None,
    })
}

impl Child {
    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
}

impl Kill for Child {
    fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }
}

impl Future for Child {
    type Output = io::Result<ExitStatus>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.child.try_wait()? {
            Some(status) => Poll::Ready(Ok(status)),
            None => Poll::Ready(Err(unsupported())),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ChildStdio;

impl AsyncRead for ChildStdio {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(unsupported()))
    }
}

impl AsyncWrite for ChildStdio {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(unsupported()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(unsupported()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(unsupported()))
    }
}

pub(crate) fn convert_to_stdio(_io: ChildStdio) -> io::Result<Stdio> {
    Err(unsupported())
}

pub(super) fn stdio<T>(_io: T) -> io::Result<ChildStdio> {
    Err(unsupported())
}
