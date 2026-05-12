#[cfg(feature = "sync")]
pub(crate) mod channel {
    use alloc::{collections::VecDeque, sync::Arc};
    use core::fmt;
    use parking_lot::Mutex;

    pub(crate) fn bounded<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
        let inner = Arc::new(Mutex::new(Inner {
            cap,
            queue: VecDeque::new(),
        }));
        (
            Sender {
                inner: Arc::clone(&inner),
            },
            Receiver { inner },
        )
    }

    struct Inner<T> {
        cap: usize,
        queue: VecDeque<T>,
    }

    pub(crate) struct Sender<T> {
        inner: Arc<Mutex<Inner<T>>>,
    }

    pub(crate) struct Receiver<T> {
        inner: Arc<Mutex<Inner<T>>>,
    }

    impl<T> Clone for Sender<T> {
        fn clone(&self) -> Self {
            Self {
                inner: Arc::clone(&self.inner),
            }
        }
    }

    impl<T> Clone for Receiver<T> {
        fn clone(&self) -> Self {
            Self {
                inner: Arc::clone(&self.inner),
            }
        }
    }

    impl<T> Sender<T> {
        pub(crate) fn len(&self) -> usize {
            self.inner.lock().queue.len()
        }

        pub(crate) fn try_send(&self, msg: T) -> Result<(), TrySendError<T>> {
            let mut inner = self.inner.lock();
            if inner.queue.len() >= inner.cap {
                Err(TrySendError::Full(msg))
            } else {
                inner.queue.push_back(msg);
                Ok(())
            }
        }

        pub(crate) fn send(&self, msg: T) -> Result<(), SendError<T>> {
            self.try_send(msg).map_err(|err| match err {
                TrySendError::Full(msg) | TrySendError::Disconnected(msg) => SendError(msg),
            })
        }
    }

    impl<T> Receiver<T> {
        pub(crate) fn len(&self) -> usize {
            self.inner.lock().queue.len()
        }

        pub(crate) fn try_recv(&self) -> Result<T, TryRecvError> {
            self.inner.lock().queue.pop_front().ok_or(TryRecvError::Empty)
        }
    }

    pub(crate) struct SendError<T>(pub(crate) T);

    impl<T> fmt::Debug for SendError<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("SendError").finish()
        }
    }

    pub(crate) enum TryRecvError {
        Empty,
        Disconnected,
    }

    pub(crate) enum TrySendError<T> {
        Full(T),
        Disconnected(T),
    }

    impl fmt::Debug for TryRecvError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Empty => f.write_str("Empty"),
                Self::Disconnected => f.write_str("Disconnected"),
            }
        }
    }

    impl<T> fmt::Debug for TrySendError<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Full(_) => f.write_str("Full"),
                Self::Disconnected(_) => f.write_str("Disconnected"),
            }
        }
    }
}

pub(crate) mod epoch {
    use alloc::boxed::Box;
    use core::{
        ptr,
        sync::atomic::{AtomicPtr, Ordering},
    };
    use crossbeam_epoch::{Collector, Guard, LocalHandle};

    static COLLECTOR: AtomicPtr<Collector> = AtomicPtr::new(ptr::null_mut());
    static HANDLE: AtomicPtr<LocalHandle> = AtomicPtr::new(ptr::null_mut());

    pub(crate) fn pin() -> Guard {
        handle().pin()
    }

    fn collector() -> &'static Collector {
        let mut ptr = COLLECTOR.load(Ordering::Acquire);
        if ptr.is_null() {
            let new = Box::into_raw(Box::new(Collector::new()));
            match COLLECTOR.compare_exchange(ptr, new, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => ptr = new,
                Err(existing) => {
                    unsafe {
                        drop(Box::from_raw(new));
                    }
                    ptr = existing;
                }
            }
        }
        unsafe { &*ptr }
    }

    fn handle() -> &'static LocalHandle {
        let mut ptr = HANDLE.load(Ordering::Acquire);
        if ptr.is_null() {
            let new = Box::into_raw(Box::new(collector().register()));
            match HANDLE.compare_exchange(ptr, new, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => ptr = new,
                Err(existing) => {
                    unsafe {
                        drop(Box::from_raw(new));
                    }
                    ptr = existing;
                }
            }
        }
        unsafe { &*ptr }
    }
}
