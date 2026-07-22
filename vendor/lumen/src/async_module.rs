//! Asynchronous module contract for interrupt-driven execution backends.
//!
//! Host-runtime modules retain the synchronous `Module` interface. Device
//! modules such as TRUEGA cannot implement that contract honestly because a
//! forward pass completes only after an interrupt wakes its owning task.

use core::future::Future;

/// A typed module whose forward pass may suspend while a device is running.
pub trait AsyncModule {
    type Input;
    type Output;
    type Error;

    fn forward(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>>;
}

/// Run one asynchronous module forward pass.
#[inline]
pub async fn forward<Module>(
    module: &Module,
    input: Module::Input,
) -> Result<Module::Output, Module::Error>
where
    Module: AsyncModule,
{
    module.forward(input).await
}
