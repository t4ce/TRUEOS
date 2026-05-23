use crate::loom::sync::{Mutex, MutexGuard};
use crate::runtime::signal::Handle as SignalHandle;
use crate::signal::unix::{signal_with_handle, SignalKind};
use crate::sync::watch;
use crate::io;
use std::process::ExitStatus;

/// An interface for waiting on a process to exit.
pub(crate) trait Wait {
    /// Get the identifier for this process or diagnostics.
    #[allow(dead_code)]
    fn id(&self) -> u32;
    /// Try waiting for a process to exit in a non-blocking manner.
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
}

impl<T: Wait> Wait for &mut T {
    fn id(&self) -> u32 {
        (**self).id()
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        (**self).try_wait()
    }
}

/// An interface for queueing up an orphaned process so that it can be reaped.
pub(crate) trait OrphanQueue<T> {
    /// Adds an orphan to the queue.
    fn push_orphan(&self, orphan: T);
}

impl<T, O: OrphanQueue<T>> OrphanQueue<T> for &O {
    fn push_orphan(&self, orphan: T) {
        (**self).push_orphan(orphan);
    }
}

/// An implementation of `OrphanQueue`.
#[derive(Debug)]
pub(crate) struct OrphanQueueImpl<T> {
    sigchild: Mutex<Option<watch::Receiver<()>>>,
    queue: Mutex<Vec<T>>,
}

impl<T> OrphanQueueImpl<T> {
    cfg_not_has_const_mutex_new! {
        pub(crate) fn new() -> Self {
            Self {
                sigchild: Mutex::new(None),
                queue: Mutex::new(Vec::new()),
            }
        }
    }

    cfg_has_const_mutex_new! {
        pub(crate) const fn new() -> Self {
            Self {
                sigchild: Mutex::const_new(None),
                queue: Mutex::const_new(Vec::new()),
            }
        }
    }


    pub(crate) fn push_orphan(&self, orphan: T)
    where
        T: Wait,
    {
        self.queue.lock().push(orphan);
    }

    /// Attempts to reap every process in the queue, ignoring any errors and
    /// enqueueing any orphans which have not yet exited.
    pub(crate) fn reap_orphans(&self, handle: &SignalHandle)
    where
        T: Wait,
    {
        // If someone else is holding the lock, they will be responsible for draining
        // the queue as necessary, so we can safely bail if that happens
        if let Some(mut sigchild_guard) = self.sigchild.try_lock() {
            match &mut *sigchild_guard {
                Some(sigchild) => {
                    if sigchild.try_has_changed().and_then(Result::ok).is_some() {
                        drain_orphan_queue(self.queue.lock());
                    }
                }
                None => {
                    let queue = self.queue.lock();

                    // Be lazy and only initialize the SIGCHLD listener if there
                    // are any orphaned processes in the queue.
                    if !queue.is_empty() {
                        // An errors shouldn't really happen here, but if it does it
                        // means that the signal driver isn't running, in
                        // which case there isn't anything we can
                        // register/initialize here, so we can try again later
                        if let Ok(sigchild) = signal_with_handle(SignalKind::child(), handle) {
                            *sigchild_guard = Some(sigchild);
                            drain_orphan_queue(queue);
                        }
                    }
                }
            }
        }
    }
}

fn drain_orphan_queue<T>(mut queue: MutexGuard<'_, Vec<T>>)
where
    T: Wait,
{
    for i in (0..queue.len()).rev() {
        match queue[i].try_wait() {
            Ok(None) => {}
            Ok(Some(_)) | Err(_) => {
                // The stdlib handles interruption errors (EINTR) when polling a child process.
                // All other errors represent invalid inputs or pids that have already been
                // reaped, so we can drop the orphan in case an error is raised.
                queue.swap_remove(i);
            }
        }
    }

    drop(queue);
}
