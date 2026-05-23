use crate::{
    io::{interest::Interest, PollEvented},
    process::{
        imp::{orphan::Wait, OrphanQueue},
        kill::Kill,
    },
};

use libc::{syscall, SYS_pidfd_open, ENOSYS, PIDFD_NONBLOCK};
use mio::{event::Source, unix::SourceFd};
use std::{
    fs::File,
    future::Future,
    io,
    marker::Unpin,
    ops::Deref,
    os::unix::io::{AsRawFd, FromRawFd, RawFd},
    pin::Pin,
    process::ExitStatus,
    sync::atomic::{AtomicBool, Ordering::Relaxed},
    task::{Context, Poll},
};

#[derive(Debug)]
struct Pidfd {
    fd: File,
}

impl Pidfd {
    fn open(pid: u32) -> Option<Pidfd> {
        // Store false (0) to reduce executable size
        static NO_PIDFD_SUPPORT: AtomicBool = AtomicBool::new(false);

        if NO_PIDFD_SUPPORT.load(Relaxed) {
            return None;
        }

        // Safety: The following function calls invovkes syscall pidfd_open,
        // which takes two parameter: pidfd_open(fd: c_int, flag: c_int)
        let fd = unsafe { syscall(SYS_pidfd_open, pid, PIDFD_NONBLOCK) };
        if fd == -1 {
            let errno = io::Error::last_os_error().raw_os_error().unwrap();

            if errno == ENOSYS {
                NO_PIDFD_SUPPORT.store(true, Relaxed)
            }

            None
        } else {
            // Safety: pidfd_open returns -1 on error or a valid fd with ownership.
            Some(Pidfd {
                fd: unsafe { File::from_raw_fd(fd as i32) },
            })
        }
    }
}

impl AsRawFd for Pidfd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl Source for Pidfd {
    fn register(
        &mut self,
        registry: &mio::Registry,
        token: mio::Token,
        interest: mio::Interest,
    ) -> io::Result<()> {
        SourceFd(&self.as_raw_fd()).register(registry, token, interest)
    }

    fn reregister(
        &mut self,
        registry: &mio::Registry,
        token: mio::Token,
        interest: mio::Interest,
    ) -> io::Result<()> {
        SourceFd(&self.as_raw_fd()).reregister(registry, token, interest)
    }

    fn deregister(&mut self, registry: &mio::Registry) -> io::Result<()> {
        SourceFd(&self.as_raw_fd()).deregister(registry)
    }
}

#[derive(Debug)]
struct PidfdReaperInner<W>
where
    W: Unpin,
{
    inner: W,
    pidfd: PollEvented<Pidfd>,
}

impl<W> Future for PidfdReaperInner<W>
where
    W: Wait + Unpin,
{
    type Output = io::Result<ExitStatus>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = Pin::into_inner(self);

        match this.pidfd.registration().poll_read_ready(cx) {
            Poll::Ready(Ok(evt)) => {
                if let Some(exit_code) = this.inner.try_wait()? {
                    return Poll::Ready(Ok(exit_code));
                }
                this.pidfd.registration().clear_readiness(evt);
            }
            Poll::Ready(Err(err)) if crate::runtime::is_rt_shutdown_err(&err) => {}
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        };

        this.pidfd.reregister(Interest::READABLE)?;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[derive(Debug)]
pub(crate) struct PidfdReaper<W, Q>
where
    W: Wait + Unpin,
    Q: OrphanQueue<W> + Unpin,
{
    inner: Option<PidfdReaperInner<W>>,
    orphan_queue: Q,
}

impl<W, Q> Deref for PidfdReaper<W, Q>
where
    W: Wait + Unpin,
    Q: OrphanQueue<W> + Unpin,
{
    type Target = W;

    fn deref(&self) -> &Self::Target {
        &self.inner.as_ref().expect("inner has gone away").inner
    }
}

impl<W, Q> PidfdReaper<W, Q>
where
    W: Wait + Unpin,
    Q: OrphanQueue<W> + Unpin,
{
    pub(crate) fn new(inner: W, orphan_queue: Q) -> Result<Self, (Option<io::Error>, W)> {
        if let Some(pidfd) = Pidfd::open(inner.id()) {
            match PollEvented::new_with_interest(pidfd, Interest::READABLE) {
                Ok(pidfd) => Ok(Self {
                    inner: Some(PidfdReaperInner { pidfd, inner }),
                    orphan_queue,
                }),
                Err(io_error) => Err((Some(io_error), inner)),
            }
        } else {
            Err((None, inner))
        }
    }

    pub(crate) fn inner_mut(&mut self) -> &mut W {
        &mut self.inner.as_mut().expect("inner has gone away").inner
    }
}

impl<W, Q> Future for PidfdReaper<W, Q>
where
    W: Wait + Unpin,
    Q: OrphanQueue<W> + Unpin,
{
    type Output = io::Result<ExitStatus>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(
            Pin::into_inner(self)
                .inner
                .as_mut()
                .expect("inner has gone away"),
        )
        .poll(cx)
    }
}

impl<W, Q> Kill for PidfdReaper<W, Q>
where
    W: Wait + Unpin + Kill,
    Q: OrphanQueue<W> + Unpin,
{
    fn kill(&mut self) -> io::Result<()> {
        self.inner_mut().kill()
    }
}

impl<W, Q> Drop for PidfdReaper<W, Q>
where
    W: Wait + Unpin,
    Q: OrphanQueue<W> + Unpin,
{
    fn drop(&mut self) {
        let mut orphan = self.inner.take().expect("inner has gone away").inner;
        if let Ok(Some(_)) = orphan.try_wait() {
            return;
        }

        self.orphan_queue.push_orphan(orphan);
    }
}
