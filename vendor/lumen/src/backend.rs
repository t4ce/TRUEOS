use core::future::Future;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LumenBackend {
    Cpu,
    Cuda,
    Truega,
}

impl LumenBackend {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
            Self::Truega => "truega",
        }
    }

    #[inline]
    pub const fn is_truega(self) -> bool {
        matches!(self, Self::Truega)
    }
}

#[inline]
pub const fn truega_compiled() -> bool {
    cfg!(feature = "truega")
}

#[inline]
pub const fn default_backend() -> LumenBackend {
    if truega_compiled() {
        LumenBackend::Truega
    } else if cfg!(feature = "cuda") {
        LumenBackend::Cuda
    } else {
        LumenBackend::Cpu
    }
}

#[inline]
pub const fn default_backend_name() -> &'static str {
    default_backend().as_str()
}

/// Asynchronous ownership boundary for host- or device-backed Lumen operations.
///
/// The operation type defines the typed contract. Backends remain responsible
/// for scheduling and completion, allowing no-std kernels to use interrupt-
/// driven devices without blocking a synchronous tensor callback.
pub trait AsyncBackend<Operation> {
    type Output;
    type Error;

    fn execute(
        &self,
        operation: Operation,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>>;
}

/// Dispatch one typed operation through its asynchronous backend.
#[inline]
pub async fn execute<B, Operation>(backend: &B, operation: Operation) -> Result<B::Output, B::Error>
where
    B: AsyncBackend<Operation>,
{
    backend.execute(operation).await
}
