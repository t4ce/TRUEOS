use core::future::Future;

#[cfg(feature = "truega")]
use core::marker::PhantomData;

#[cfg(feature = "truega")]
pub use trueos_fpga_abi::{
    AotCodecError, AotCompletionKind, AotFirmwareCapability, AotFixedShape, AotLane,
    AotLaneOwnership, AotOpDescriptor, AotScalarFormat, AotStateOwnership, AotTensorDescriptor,
    AotTransportKind, TruegaCustomOp,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LumenBackend {
    #[cfg(feature = "host-runtime")]
    Cpu,
    #[cfg(all(feature = "host-runtime", feature = "cuda"))]
    Cuda,
    #[cfg(feature = "truega")]
    Truega,
}

impl LumenBackend {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "host-runtime")]
            Self::Cpu => "cpu",
            #[cfg(all(feature = "host-runtime", feature = "cuda"))]
            Self::Cuda => "cuda",
            #[cfg(feature = "truega")]
            Self::Truega => "truega",
        }
    }

    #[cfg(feature = "truega")]
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
    #[cfg(feature = "truega")]
    return LumenBackend::Truega;

    #[cfg(all(feature = "host-runtime", feature = "cuda"))]
    return LumenBackend::Cuda;

    #[cfg(all(feature = "host-runtime", not(feature = "cuda")))]
    return LumenBackend::Cpu;
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

/// Typed invocation of one operation generated beside the TRUEGA bitstream.
///
/// `Op` selects immutable AOT metadata and fixed codecs at compile time. Only the typed
/// input is carried at runtime; there is no operation registry, graph, bytecode, or compiler.
#[cfg(feature = "truega")]
pub struct AotInvocation<Op>
where
    Op: TruegaCustomOp,
{
    pub input: Op::Input,
    marker: PhantomData<fn() -> Op>,
}

#[cfg(feature = "truega")]
impl<Op> AotInvocation<Op>
where
    Op: TruegaCustomOp,
{
    #[inline]
    pub const fn new(input: Op::Input) -> Self {
        Self {
            input,
            marker: PhantomData,
        }
    }

    #[inline]
    pub const fn descriptor(&self) -> &'static AotOpDescriptor {
        &Op::DESCRIPTOR
    }

    #[inline]
    pub fn into_input(self) -> Op::Input {
        self.input
    }
}

/// One kernel-owned executor for every generated TRUEGA operation.
///
/// A TRUEOS backend implements this once. The blanket [`AsyncBackend`] implementation
/// below preserves each generated operation's concrete output type while the executor
/// selects only its compile-time transport descriptor.
#[cfg(feature = "truega")]
pub trait TruegaAotBackend {
    type Error;

    fn execute_aot<Op>(
        &self,
        operation: AotInvocation<Op>,
    ) -> impl Future<Output = Result<Op::Output, Self::Error>>
    where
        Op: TruegaCustomOp;
}

#[cfg(feature = "truega")]
impl<Backend, Op> AsyncBackend<AotInvocation<Op>> for Backend
where
    Backend: TruegaAotBackend,
    Op: TruegaCustomOp,
{
    type Output = Op::Output;
    type Error = Backend::Error;

    #[inline]
    fn execute(
        &self,
        operation: AotInvocation<Op>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        self.execute_aot(operation)
    }
}

/// Dispatch one typed operation through its asynchronous backend.
#[inline]
pub async fn execute<B, Operation>(backend: &B, operation: Operation) -> Result<B::Output, B::Error>
where
    B: AsyncBackend<Operation>,
{
    backend.execute(operation).await
}
