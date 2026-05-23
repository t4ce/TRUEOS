use crate::process::imp::orphan::{OrphanQueue, Wait};
use crate::process::kill::Kill;
use crate::signal::unix::InternalStream;

use core::future::Future;
use std::io;
use core::ops::Deref;
use core::pin::Pin;
use std::process::ExitStatus;
use core::task::Context;
use core::task::Poll;

/// Orchestrates between registering interest for receiving signals when a
/// child process has exited, and attempting to poll for process completion.
#[derive(Debug)]
pub(crate) struct Reaper<W, Q, S>
where
    W: Wait,
    Q: OrphanQueue<W>,
{
    inner: Option<W>,
    orphan_queue: Q,
    signal: S,
}

impl<W, Q, S> Deref for Reaper<W, Q, S>
where
    W: Wait,
    Q: OrphanQueue<W>,
{
    type Target = W;

    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

impl<W, Q, S> Reaper<W, Q, S>
where
    W: Wait,
    Q: OrphanQueue<W>,
{
    pub(crate) fn new(inner: W, orphan_queue: Q, signal: S) -> Self {
        Self {
            inner: Some(inner),
            orphan_queue,
            signal,
        }
    }

    fn inner(&self) -> &W {
        self.inner.as_ref().expect("inner has gone away")
    }

    pub(crate) fn inner_mut(&mut self) -> &mut W {
        self.inner.as_mut().expect("inner has gone away")
    }
}

impl<W, Q, S> Future for Reaper<W, Q, S>
where
    W: Wait + Unpin,
    Q: OrphanQueue<W> + Unpin,
    S: InternalStream + Unpin,
{
    type Output = io::Result<ExitStatus>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            // If the child hasn't exited yet, then it's our responsibility to
            // ensure the current task gets notified when it might be able to
            // make progress. We can use the delivery of a SIGCHLD signal as a
            // sign that we can potentially make progress.
            //
            // However, we will register for a notification on the next signal
            // BEFORE we poll the child. Otherwise it is possible that the child
            // can exit and the signal can arrive after we last polled the child,
            // but before we've registered for a notification on the next signal
            // (this can cause a deadlock if there are no more spawned children
            // which can generate a different signal for us). A side effect of
            // pre-registering for signal notifications is that when the child
            // exits, we will have already registered for an additional
            // notification we don't need to consume. If another signal arrives,
            // this future's task will be notified/woken up again. Since the
            // futures model allows for spurious wake ups this extra wakeup
            // should not cause significant issues with parent futures.
            let registered_interest = self.signal.poll_recv(cx).is_pending();

            if let Some(status) = self.inner_mut().try_wait()? {
                return Poll::Ready(Ok(status));
            }

            // If our attempt to poll for the next signal was not ready, then
            // we've arranged for our task to get notified and we can bail out.
            if registered_interest {
                return Poll::Pending;
            } else {
                // Otherwise, if the signal stream delivered a signal to us, we
                // won't get notified at the next signal, so we'll loop and try
                // again.
                continue;
            }
        }
    }
}

impl<W, Q, S> Kill for Reaper<W, Q, S>
where
    W: Kill + Wait,
    Q: OrphanQueue<W>,
{
    fn kill(&mut self) -> io::Result<()> {
        self.inner_mut().kill()
    }
}

impl<W, Q, S> Drop for Reaper<W, Q, S>
where
    W: Wait,
    Q: OrphanQueue<W>,
{
    fn drop(&mut self) {
        if let Ok(Some(_)) = self.inner_mut().try_wait() {
            return;
        }

        let orphan = self.inner.take().unwrap();
        self.orphan_queue.push_orphan(orphan);
    }
}
