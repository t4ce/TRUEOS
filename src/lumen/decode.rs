//! Lumen module adapter for the fixed LFM2.5 decode scheduler.
//!
//! The adapter owns exactly one [`DecodeSession`] and one backend. A decode
//! call therefore represents one token on one ordered lane. Production uses
//! scalar CPU state stages and the admitted native-row AVX-VNNI projection
//! kernel; no runtime graph is interpreted.

extern crate alloc;

use alloc::vec::Vec;
use core::cell::RefCell;

use crate::ai::lfm25_decode::{
    AotDecodeBackend, DecodeError, DecodePrefillOutput, DecodeSession, DecodeTokenOutput,
};

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

/// Read-only state useful to the Shell2 session without exposing model tensors.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25DecodeState {
    pub position: u32,
    pub callback_sequence: u64,
    pub poisoned: bool,
}

/// Asynchronous Lumen module for one sealed LFM2.5 session.
///
/// `Backend` must complete each request only from its registered worker
/// callback. The generic boundary keeps the scheduler independent of the
/// projection transport.
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

    pub(crate) const fn from_parts(session: DecodeSession, backend: Backend) -> Self {
        Self {
            session: RefCell::new(session),
            backend: RefCell::new(backend),
        }
    }

    /// Non-blocking state observation. `None` means a forward future owns the lane.
    pub(crate) fn try_state(&self) -> Option<Lfm25DecodeState> {
        let session = self.session.try_borrow().ok()?;
        Some(Lfm25DecodeState {
            position: session.position(),
            callback_sequence: session.callback_sequence(),
            poisoned: session.is_poisoned(),
        })
    }

    /// Call only after `backend_mut()` has reset all recurrent and KV state.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn acknowledge_hardware_state_reset(&mut self) {
        self.session.get_mut().acknowledge_hardware_state_reset();
    }

    /// Initialization/control access; unavailable while `self` is shared by a future.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn backend_mut(&mut self) -> &mut Backend {
        self.backend.get_mut()
    }

    /// Advance a non-final prompt token without computing output logits.
    pub(crate) async fn prefill_token(
        &self,
        input: Lfm25DecodeInput,
    ) -> Result<DecodePrefillOutput, Lfm25DecodeError<Backend::Error>>
    where
        Backend: AotDecodeBackend,
    {
        let mut session = self
            .session
            .try_borrow_mut()
            .map_err(|_| Lfm25DecodeError::InFlight)?;
        let mut backend = self
            .backend
            .try_borrow_mut()
            .map_err(|_| Lfm25DecodeError::InFlight)?;
        session
            .prefill_token(&mut *backend, input.token())
            .await
            .map_err(Lfm25DecodeError::Decode)
    }

    /// Advance one token and compute the next-token result.
    pub(crate) async fn decode_token(
        &self,
        input: Lfm25DecodeInput,
    ) -> Result<DecodeTokenOutput, Lfm25DecodeError<Backend::Error>>
    where
        Backend: AotDecodeBackend,
    {
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

    pub(crate) fn into_parts(self) -> (DecodeSession, Backend) {
        (self.session.into_inner(), self.backend.into_inner())
    }
}

/// Bind the sealed scalar CPU stages and native-row AVX-VNNI projection
/// kernel to the same fixed 99-operation Lumen module.
#[cfg(target_os = "trueos")]
pub(crate) async fn open_cpu_vnni() -> Result<
    Lfm25Decode<crate::ai::lfm25_hybrid_cpu_backend::CpuVnniAotDecodeBackend>,
    crate::ai::lfm25_hybrid_cpu_backend::HybridCpuBackendError,
> {
    let backend = crate::ai::lfm25_hybrid_cpu_backend::open_cpu_vnni_backend().await?;
    Ok(Lfm25Decode::new(backend))
}

#[cfg(target_os = "trueos")]
pub(crate) fn checkpoint_cpu_vnni(
    module: Lfm25Decode<crate::ai::lfm25_hybrid_cpu_backend::CpuVnniAotDecodeBackend>,
) -> Result<Vec<u8>, crate::ai::lfm25_hybrid_cpu_backend::HybridCpuBackendError> {
    let (session, backend) = module.into_parts();
    if session.is_poisoned() {
        return Err(crate::ai::lfm25_hybrid_cpu_backend::HybridCpuBackendError::SessionImage);
    }
    backend.checkpoint_state(session.position(), session.callback_sequence())
}

#[cfg(target_os = "trueos")]
pub(crate) async fn restore_cpu_vnni(
    image: &[u8],
) -> Result<
    Lfm25Decode<crate::ai::lfm25_hybrid_cpu_backend::CpuVnniAotDecodeBackend>,
    crate::ai::lfm25_hybrid_cpu_backend::HybridCpuBackendError,
> {
    let mut backend = crate::ai::lfm25_hybrid_cpu_backend::open_cpu_vnni_backend().await?;
    let checkpoint = backend.restore_state(image)?;
    let session = DecodeSession::from_checkpoint(checkpoint.position, checkpoint.callback_sequence)
        .ok_or(crate::ai::lfm25_hybrid_cpu_backend::HybridCpuBackendError::SessionImage)?;
    Ok(Lfm25Decode::from_parts(session, backend))
}

#[cfg(target_os = "trueos")]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) async fn open_hybrid_cpu() -> Result<
    Lfm25Decode<crate::ai::lfm25_hybrid_cpu_backend::HybridCpuAotDecodeBackend>,
    crate::ai::lfm25_hybrid_cpu_backend::HybridCpuBackendError,
> {
    open_cpu_vnni().await
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::vec::Vec;
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use super::*;
    use crate::ai::lfm25_decode::{
        AotDecodeCallback, AotDecodeOutput, AotDecodeRequest, FailClosedBackend, HiddenQ8,
        HiddenQ30, ResidentTensorHandle,
    };
    use trueos_lfm25_model::lfm25;
    use trueos_lfm25_model::lfm25_decode::{DecodeCapabilities, DecodeOpKind, DecodePlan};

    #[derive(Default)]
    struct FakeAotBackend {
        callback_sequence: u64,
        storage_slot: u16,
        prefill_finishes: usize,
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

        fn finish_prefill_token(&mut self, _output: HiddenQ30) -> Result<(), Self::Error> {
            self.prefill_finishes += 1;
            Ok(())
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
        let first = ready(module.decode_token(Lfm25DecodeInput::new(1))).unwrap();
        assert_eq!((first.token, first.input_position), (7, 0));
        assert_eq!(first.callback_sequence, 99);
        assert_eq!(
            module.try_state(),
            Some(Lfm25DecodeState {
                position: 1,
                callback_sequence: 99,
                poisoned: false,
            })
        );

        let second = ready(module.decode_token(Lfm25DecodeInput::new(first.token))).unwrap();
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
    fn non_final_prefill_skips_output_ops_and_preserves_next_full_decode() {
        let module = Lfm25Decode::new(FakeAotBackend::default());
        let prefill = ready(module.prefill_token(Lfm25DecodeInput::new(1))).unwrap();
        assert_eq!((prefill.input_position, prefill.callback_sequence), (0, 97));
        assert_eq!(
            module.try_state(),
            Some(Lfm25DecodeState {
                position: 1,
                callback_sequence: 97,
                poisoned: false,
            })
        );

        let output = ready(module.decode_token(Lfm25DecodeInput::new(2))).unwrap();
        assert_eq!((output.token, output.input_position), (7, 1));
        assert_eq!(output.callback_sequence, 196);

        let (session, backend) = module.into_parts();
        assert_eq!(session.position(), 2);
        assert_eq!(backend.prefill_finishes, 1);
        let expected: Vec<_> = DecodePlan::new().map(|step| step.kind).collect();
        assert_eq!(backend.observed.len(), 196);
        assert_eq!(&backend.observed[..97], &expected[..97]);
        assert_eq!(&backend.observed[97..], expected.as_slice());
    }

    #[test]
    fn unavailable_hardware_fails_closed_without_a_cpu_path() {
        let module = Lfm25Decode::new(FailClosedBackend);
        let result = ready(module.decode_token(Lfm25DecodeInput::new(1)));
        assert_eq!(result, Err(Lfm25DecodeError::Decode(DecodeError::ContextFull)));
        assert_eq!(module.try_state().unwrap().position, 0);
    }
}
