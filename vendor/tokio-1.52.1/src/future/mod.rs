#![cfg_attr(not(feature = "macros"), allow(unreachable_pub))]

//! Asynchronous values.

#[cfg(target_os = "zkvm")]
pub use core::future::{poll_fn, Future, IntoFuture};
#[cfg(target_os = "zkvm")]
use core::pin::Pin;
#[cfg(target_os = "zkvm")]
use core::task::{Context, Poll};

#[cfg(target_os = "zkvm")]
#[derive(Debug, Clone)]
pub struct Ready<T>(Option<T>);

#[cfg(target_os = "zkvm")]
pub fn ready<T>(value: T) -> Ready<T> {
    Ready(Some(value))
}

#[cfg(target_os = "zkvm")]
impl<T> Unpin for Ready<T> {}

#[cfg(target_os = "zkvm")]
impl<T> Future for Ready<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
        Poll::Ready(self.get_mut().0.take().expect("Ready polled after completion"))
    }
}

#[cfg(any(feature = "macros", feature = "process"))]
pub(crate) mod maybe_done;

cfg_process! {
    mod try_join;
    pub(crate) use try_join::try_join3;
}

cfg_sync! {
    mod block_on;
    pub(crate) use block_on::block_on;
}

cfg_trace! {
    mod trace;
    #[allow(unused_imports)]
    pub(crate) use trace::InstrumentedFuture as Future;
}

cfg_not_trace! {
    cfg_rt! {
        #[cfg(not(target_os = "zkvm"))]
        pub(crate) use core::future::Future;
    }
}
