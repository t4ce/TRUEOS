#![cfg_attr(not(feature = "macros"), allow(unreachable_pub))]

//! Asynchronous values.

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub use core::future::{poll_fn, Future, IntoFuture};

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
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        pub(crate) use std::future::Future;
    }
}
