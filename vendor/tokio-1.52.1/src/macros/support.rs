cfg_macros! {
    pub use crate::future::maybe_done::maybe_done;

    pub use core::future::poll_fn;

    pub use crate::macros::join::{BiasedRotator, Rotator, RotatorSelect, SelectNormal, SelectBiased};

    #[doc(hidden)]
    pub fn thread_rng_n(n: u32) -> u32 {
        crate::runtime::context::thread_rng_n(n)
    }

    cfg_coop! {
        #[doc(hidden)]
        #[inline]
        pub fn poll_budget_available(cx: &mut Context<'_>) -> Poll<()> {
            crate::task::coop::poll_budget_available(cx)
        }
    }

    cfg_not_coop! {
        #[doc(hidden)]
        #[inline]
        pub fn poll_budget_available(_: &mut Context<'_>) -> Poll<()> {
            Poll::Ready(())
        }
    }
}

pub use core::future::{Future, IntoFuture};
pub use core::pin::Pin;
pub use std::result::Result;
pub use core::task::{ready, Context, Poll};
