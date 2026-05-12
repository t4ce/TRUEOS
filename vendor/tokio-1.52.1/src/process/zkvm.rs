use super::kill::Kill;
use super::SpawnedChild;
use crate::io::{AsyncRead, AsyncWrite, ReadBuf};

use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io;

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Other, "tokio process is not supported on zkvm")
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExitStatus {
    code: Option<i32>,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    pub fn code(&self) -> Option<i32> {
        self.code
    }
}

#[derive(Debug, Default)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stdio;

impl Stdio {
    pub fn null() -> Self {
        Self
    }

    pub fn inherit() -> Self {
        Self
    }

    pub fn piped() -> Self {
        Self
    }
}

#[derive(Debug, Default)]
pub struct Command;

impl Command {
    pub fn new<S>(_program: S) -> Self
    where
        S: AsRef<core::ffi::OsStr>,
    {
        Self
    }

    pub fn arg<S>(&mut self, _arg: S) -> &mut Self
    where
        S: AsRef<core::ffi::OsStr>,
    {
        self
    }

    pub fn args<I, S>(&mut self, _args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<core::ffi::OsStr>,
    {
        self
    }

    pub fn env<K, V>(&mut self, _key: K, _val: V) -> &mut Self
    where
        K: AsRef<core::ffi::OsStr>,
        V: AsRef<core::ffi::OsStr>,
    {
        self
    }

    pub fn envs<I, K, V>(&mut self, _vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<core::ffi::OsStr>,
        V: AsRef<core::ffi::OsStr>,
    {
        self
    }

    pub fn env_remove<K>(&mut self, _key: K) -> &mut Self
    where
        K: AsRef<core::ffi::OsStr>,
    {
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self
    }

    pub fn current_dir<P>(&mut self, _dir: P) -> &mut Self
    where
        P: AsRef<std::path::Path>,
    {
        self
    }

    pub fn stdin<T>(&mut self, _cfg: T) -> &mut Self
    where
        T: Into<Stdio>,
    {
        self
    }

    pub fn stdout<T>(&mut self, _cfg: T) -> &mut Self
    where
        T: Into<Stdio>,
    {
        self
    }

    pub fn stderr<T>(&mut self, _cfg: T) -> &mut Self
    where
        T: Into<Stdio>,
    {
        self
    }

    pub fn spawn(&mut self) -> io::Result<Child> {
        Err(unsupported())
    }
}

#[must_use = "futures do nothing unless polled"]
pub(crate) struct Child {
    id: u32,
}

impl core::fmt::Debug for Child {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt.debug_struct("Child").field("pid", &self.id()).finish()
    }
}

pub(crate) fn build_child(child: Child) -> io::Result<SpawnedChild> {
    Ok(SpawnedChild {
        child,
        stdin: None,
        stdout: None,
        stderr: None,
    })
}

impl Child {
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(None)
    }
}

impl Kill for Child {
    fn kill(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Future for Child {
    type Output = io::Result<ExitStatus>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(Err(unsupported()))
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
