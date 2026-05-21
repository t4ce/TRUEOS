//! Internal macros

macro_rules! opaque_future {
    ($(#[$m:meta])* pub type $name:ident = $actual:ty;) => {
        opaque_future! {
            $(#[$m])*
            pub type $name<> = $actual;
        }
    };

    ($(#[$m:meta])* pub type $name:ident<$($param:ident),*> = $actual:ty;) => {
        pin_project_lite::pin_project! {
            $(#[$m])*
            pub struct $name<$($param),*> {
                #[pin] future: $actual,
            }
        }

        impl<$($param),*> $name<$($param),*> {
            pub(crate) fn new(future: $actual) -> Self {
                Self { future }
            }
        }

        impl<$($param),*> core::fmt::Debug for $name<$($param),*> {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct(stringify!($name)).finish_non_exhaustive()
            }
        }

        impl<$($param),*> core::future::Future for $name<$($param),*>
        where
            $actual: core::future::Future,
        {
            type Output = <$actual as core::future::Future>::Output;

            #[inline]
            fn poll(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<Self::Output> {
                self.project().future.poll(cx)
            }
        }
    };
}

#[rustfmt::skip]
macro_rules! all_the_tuples {
    ($name:ident) => {
        $name!([], T1);
        $name!([T1], T2);
        $name!([T1, T2], T3);
        $name!([T1, T2, T3], T4);
        $name!([T1, T2, T3, T4], T5);
        $name!([T1, T2, T3, T4, T5], T6);
        $name!([T1, T2, T3, T4, T5, T6], T7);
        $name!([T1, T2, T3, T4, T5, T6, T7], T8);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8], T9);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9], T10);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10], T11);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11], T12);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12], T13);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13], T14);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14], T15);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15], T16);
    };
}

#[cfg(feature = "tracing")]
#[allow(unused_macros)]
macro_rules! trace {
    ($($tt:tt)*) => {
        tracing::trace!($($tt)*)
    }
}

#[cfg(feature = "tracing")]
#[allow(unused_macros)]
macro_rules! error {
    ($($tt:tt)*) => {
        tracing::error!($($tt)*)
    };
}

#[cfg(not(feature = "tracing"))]
#[allow(unused_macros)]
macro_rules! trace {
    ($($tt:tt)*) => {};
}

#[cfg(not(feature = "tracing"))]
#[allow(unused_macros)]
macro_rules! error {
    ($($tt:tt)*) => {};
}
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::prelude::rust_2021::*;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::borrow::ToOwned;
