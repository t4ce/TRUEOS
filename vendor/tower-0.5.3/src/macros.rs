#[cfg(any(
    feature = "util",
    feature = "spawn-ready",
    feature = "filter",
    feature = "make"
))]
macro_rules! opaque_future {
    ($(#[$m:meta])* pub type $name:ident<$($param:ident),+> = $actual:ty;) => {
        pin_project_lite::pin_project! {
            $(#[$m])*
            pub struct $name<$($param),+> {
                #[pin]
                inner: $actual
            }
        }

        impl<$($param),+> $name<$($param),+> {
            pub(crate) fn new(inner: $actual) -> Self {
                Self {
                    inner
                }
            }
        }

        impl<$($param),+> core::fmt::Debug for $name<$($param),+> {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_tuple(stringify!($name)).field(&format_args!("...")).finish()
            }
        }

        impl<$($param),+> core::future::Future for $name<$($param),+>
        where
            $actual: core::future::Future,
        {
            type Output = <$actual as core::future::Future>::Output;
            #[inline]
            fn poll(self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
                self.project().inner.poll(cx)
            }
        }
    }
}
