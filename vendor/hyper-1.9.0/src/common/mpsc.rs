use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use futures_core::Stream;

use crate::sync::Mutex;

pub(crate) fn unbounded<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let inner = Arc::new(Inner {
        queue: Mutex::new(VecDeque::new()),
        closed: AtomicBool::new(false),
        senders: AtomicUsize::new(1),
        recv_waker: AtomicWaker::new(),
    });

    (
        UnboundedSender {
            inner: Some(inner.clone()),
        },
        UnboundedReceiver { inner: Some(inner) },
    )
}

pub(crate) struct TrySendError<T> {
    value: T,
}

impl<T> TrySendError<T> {
    pub(crate) fn into_inner(self) -> T {
        self.value
    }
}

pub(crate) struct UnboundedSender<T> {
    inner: Option<Arc<Inner<T>>>,
}

pub(crate) struct UnboundedReceiver<T> {
    inner: Option<Arc<Inner<T>>>,
}

struct Inner<T> {
    queue: Mutex<VecDeque<T>>,
    closed: AtomicBool,
    senders: AtomicUsize,
    recv_waker: AtomicWaker,
}

impl<T> Clone for UnboundedSender<T> {
    fn clone(&self) -> Self {
        if let Some(inner) = &self.inner {
            inner.senders.fetch_add(1, Ordering::Relaxed);
            Self {
                inner: Some(inner.clone()),
            }
        } else {
            Self { inner: None }
        }
    }
}

impl<T> Drop for UnboundedSender<T> {
    fn drop(&mut self) {
        if let Some(inner) = &self.inner {
            if inner.senders.fetch_sub(1, Ordering::AcqRel) == 1 {
                inner.closed.store(true, Ordering::Release);
                inner.recv_waker.wake();
            }
        }
    }
}

impl<T> UnboundedSender<T> {
    pub(crate) fn unbounded_send(&self, value: T) -> Result<(), TrySendError<T>> {
        let Some(inner) = &self.inner else {
            return Err(TrySendError { value });
        };
        if inner.closed.load(Ordering::Acquire) {
            return Err(TrySendError { value });
        }

        inner.queue.lock().unwrap().push_back(value);
        inner.recv_waker.wake();
        Ok(())
    }

    #[cfg(feature = "http2")]
    pub(crate) fn is_closed(&self) -> bool {
        self.inner
            .as_ref()
            .map(|inner| inner.closed.load(Ordering::Acquire))
            .unwrap_or(true)
    }
}

impl<T> UnboundedReceiver<T> {
    pub(crate) fn is_empty(&self) -> bool {
        self.inner
            .as_ref()
            .map(|inner| inner.queue.lock().unwrap().is_empty())
            .unwrap_or(true)
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.inner
            .as_ref()
            .map(|inner| inner.closed.load(Ordering::Acquire) && self.is_empty())
            .unwrap_or(true)
    }

    pub(crate) fn close(&mut self) {
        if let Some(inner) = &self.inner {
            inner.closed.store(true, Ordering::Release);
            inner.recv_waker.wake();
        }
    }

    pub(crate) fn try_recv(&mut self) -> Option<T> {
        self.inner
            .as_ref()
            .and_then(|inner| inner.queue.lock().unwrap().pop_front())
    }
}

impl<T> Drop for UnboundedReceiver<T> {
    fn drop(&mut self) {
        self.close();
    }
}

impl<T> Stream for UnboundedReceiver<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        let Some(inner) = &this.inner else {
            return Poll::Ready(None);
        };

        if let Some(value) = inner.queue.lock().unwrap().pop_front() {
            return Poll::Ready(Some(value));
        }
        if inner.closed.load(Ordering::Acquire) {
            return Poll::Ready(None);
        }

        inner.recv_waker.register(cx.waker());

        if let Some(value) = inner.queue.lock().unwrap().pop_front() {
            Poll::Ready(Some(value))
        } else if inner.closed.load(Ordering::Acquire) {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}
