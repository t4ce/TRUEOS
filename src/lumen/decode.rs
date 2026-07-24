//! Lumen module adapter for the fixed LFM2.5 decode scheduler.
//!
//! The adapter owns exactly one [`DecodeSession`] and one backend. A forward
//! call therefore represents one token on one ordered lane. Production may use
//! either the complete TGD1/TGF2 firmware or the scalar CPU backend with the
//! proven TRUEGA FFN function; neither path interprets a runtime graph.

use core::cell::RefCell;
use core::future::Future;

use ::lumen::async_module::AsyncModule;

use crate::r::lfm25_decode::{AotDecodeBackend, DecodeError, DecodeSession, DecodeTokenOutput};

/// One immutable token request accepted by [`Lfm25Decode`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25DecodeInput {
    token: u32,
}

impl Lfm25DecodeInput {
    pub(crate) const fn new(token: u32) -> Self {
        Self { token }
    }

    pub(crate) const fn token(self) -> u32 {
        self.token
    }
}

/// Module-level errors. Backend and callback validation errors remain exact.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Lfm25DecodeError<BackendError> {
    /// Another future currently owns this single decode lane.
    InFlight,
    Decode(DecodeError<BackendError>),
}

/// Read-only state useful to a kernel service without exposing FPGA tensors.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25DecodeState {
    pub position: u32,
    pub poisoned: bool,
}

/// Vendor-compatible asynchronous Lumen module for one sealed LFM2.5 session.
///
/// `Backend` must complete each request only from its registered worker
/// callback. The generic boundary keeps the vendor Lumen interface independent
/// of the kernel transport while production uses the exact TGD1/TGF2 backend.
pub(crate) struct Lfm25Decode<Backend> {
    session: RefCell<DecodeSession>,
    backend: RefCell<Backend>,
}

impl<Backend> Lfm25Decode<Backend> {
    pub(crate) const fn new(backend: Backend) -> Self {
        Self {
            session: RefCell::new(DecodeSession::new()),
            backend: RefCell::new(backend),
        }
    }

    /// Non-blocking state observation. `None` means a forward future owns the lane.
    pub(crate) fn try_state(&self) -> Option<Lfm25DecodeState> {
        let session = self.session.try_borrow().ok()?;
        Some(Lfm25DecodeState {
            position: session.position(),
            poisoned: session.is_poisoned(),
        })
    }

    /// Call only after `backend_mut()` has reset every hardware state circuit.
    pub(crate) fn acknowledge_hardware_state_reset(&mut self) {
        self.session.get_mut().acknowledge_hardware_state_reset();
    }

    /// Initialization/control access; unavailable while `self` is shared by a future.
    pub(crate) fn backend_mut(&mut self) -> &mut Backend {
        self.backend.get_mut()
    }

    pub(crate) fn into_parts(self) -> (DecodeSession, Backend) {
        (self.session.into_inner(), self.backend.into_inner())
    }
}

/// Open and seal-check the pinned native image once, then bind the production
/// TRUEGA backend to this Lumen async module. Capability/session acquisition
/// remains lazy and fail-closed until the first forward call.
#[cfg(target_os = "trueos")]
pub(crate) async fn open_truega() -> Result<
    Lfm25Decode<crate::r::truega_decode_backend::KernelTruegaAotDecodeBackend>,
    crate::r::truega_decode_backend::KernelDecodeDataPlaneError,
> {
    let backend = crate::r::truega_decode_backend::open_kernel_backend().await?;
    Ok(Lfm25Decode::new(backend))
}

/// Bind the sealed scalar CPU stages and the admitted Intel C++/IGC projection
/// program to the same fixed 99-operation Lumen module.
#[cfg(target_os = "trueos")]
pub(crate) async fn open_intel_igc() -> Result<
    Lfm25Decode<crate::r::lfm25_hybrid_cpu_backend::IntelIgcAotDecodeBackend>,
    crate::r::lfm25_hybrid_cpu_backend::HybridCpuBackendError,
> {
    let backend = crate::r::lfm25_hybrid_cpu_backend::open_intel_igc_backend().await?;
    Ok(Lfm25Decode::new(backend))
}

#[cfg(target_os = "trueos")]
pub(crate) async fn open_hybrid_cpu() -> Result<
    Lfm25Decode<crate::r::lfm25_hybrid_cpu_backend::HybridCpuAotDecodeBackend>,
    crate::r::lfm25_hybrid_cpu_backend::HybridCpuBackendError,
> {
    open_intel_igc().await
}

impl<Backend> AsyncModule for Lfm25Decode<Backend>
where
    Backend: AotDecodeBackend,
{
    type Input = Lfm25DecodeInput;
    type Output = DecodeTokenOutput;
    type Error = Lfm25DecodeError<Backend::Error>;

    fn forward(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        async move {
            let mut session = self
                .session
                .try_borrow_mut()
                .map_err(|_| Lfm25DecodeError::InFlight)?;
            let mut backend = self
                .backend
                .try_borrow_mut()
                .map_err(|_| Lfm25DecodeError::InFlight)?;
            session
                .decode_token(&mut *backend, input.token())
                .await
                .map_err(Lfm25DecodeError::Decode)
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::vec::Vec;
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use super::*;
    use crate::r::lfm25_decode::{
        AotDecodeCallback, AotDecodeOutput, AotDecodeRequest, FailClosedBackend, HiddenQ8,
        HiddenQ30, ResidentTensorHandle,
    };
    use trueos_fpga_abi::lfm25;
    use trueos_fpga_abi::lfm25_decode::{DecodeCapabilities, DecodeOpKind, DecodePlan};

    #[derive(Default)]
    struct FakeAotBackend {
        callback_sequence: u64,
        storage_slot: u16,
        observed: Vec<DecodeOpKind>,
    }

    impl FakeAotBackend {
        fn resident(&mut self) -> ResidentTensorHandle {
            let handle = ResidentTensorHandle::new(7, 11, self.storage_slot);
            self.storage_slot += 1;
            handle
        }
    }

    impl AotDecodeBackend for FakeAotBackend {
        type Error = ();

        fn capabilities(&self) -> DecodeCapabilities {
            DecodeCapabilities::ALL
        }

        fn max_context_positions(&self) -> u32 {
            2
        }

        async fn submit(
            &mut self,
            request: AotDecodeRequest,
        ) -> Result<AotDecodeCallback, Self::Error> {
            let operation = request.kind();
            let output = match request {
                AotDecodeRequest::TokenEmbedding { .. }
                | AotDecodeRequest::OperatorResidual { .. }
                | AotDecodeRequest::Ffn { .. }
                | AotDecodeRequest::FfnResidual { .. } => {
                    AotDecodeOutput::HiddenQ30(HiddenQ30::from_resident(self.resident()))
                }
                AotDecodeRequest::OperatorRmsNorm { .. }
                | AotDecodeRequest::FfnRmsNorm { .. }
                | AotDecodeRequest::FinalRmsNorm { .. } => {
                    AotDecodeOutput::HiddenQ8(HiddenQ8::from_resident(self.resident()))
                }
                AotDecodeRequest::ShortConv {
                    position, state, ..
                }
                | AotDecodeRequest::Attention {
                    position, state, ..
                } => AotDecodeOutput::StatefulHiddenQ30 {
                    output: HiddenQ30::from_resident(self.resident()),
                    state,
                    position,
                },
                AotDecodeRequest::TiedLmHeadArgmax { .. } => AotDecodeOutput::Argmax {
                    token: 7,
                    score_q30: 123,
                    rows: lfm25::MODEL_VOCABULARY_SIZE,
                },
            };
            self.callback_sequence += 1;
            self.observed.push(operation);
            Ok(AotDecodeCallback {
                operation,
                callback_sequence: self.callback_sequence,
                output,
            })
        }
    }

    fn ready<F: Future>(future: F) -> F::Output {
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(core::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
        let mut context = Context::from_waker(&waker);
        let mut future = pin!(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("immediate fake backend unexpectedly parked"),
        }
    }

    #[test]
    fn two_tokens_use_exact_async_plan_and_preserve_session_state() {
        let module = Lfm25Decode::new(FakeAotBackend::default());
        let first =
            ready(::lumen::async_module::forward(&module, Lfm25DecodeInput::new(1))).unwrap();
        assert_eq!((first.token, first.input_position), (7, 0));
        assert_eq!(first.callback_sequence, 99);
        assert_eq!(
            module.try_state(),
            Some(Lfm25DecodeState {
                position: 1,
                poisoned: false,
            })
        );

        let second =
            ready(::lumen::async_module::forward(&module, Lfm25DecodeInput::new(first.token)))
                .unwrap();
        assert_eq!((second.token, second.input_position), (7, 1));
        assert_eq!(second.callback_sequence, 198);

        let (session, backend) = module.into_parts();
        assert_eq!(session.position(), 2);
        assert_eq!(backend.observed.len(), 198);
        let expected: Vec<_> = DecodePlan::new().map(|step| step.kind).collect();
        assert_eq!(&backend.observed[..99], expected.as_slice());
        assert_eq!(&backend.observed[99..], expected.as_slice());
    }

    #[test]
    fn unavailable_hardware_fails_closed_without_a_cpu_path() {
        let module = Lfm25Decode::new(FailClosedBackend);
        let result = ready(::lumen::async_module::forward(&module, Lfm25DecodeInput::new(1)));
        assert_eq!(result, Err(Lfm25DecodeError::Decode(DecodeError::ContextFull)));
        assert_eq!(module.try_state().unwrap().position, 0);
    }
}
