//! Processorless, single-worker LFM2.5 decode control plane.
//!
//! Numerical work is represented only as fixed AOT requests. A backend submission
//! future may resolve only after the FPGA MSI wakes `fpga-offload` and that worker
//! invokes the registered Rust completion callback. The scheduler awaits each callback
//! before issuing the next request, so there is no pool or second device scheduler.

use core::future::Future;

use trueos_fpga_abi::lfm25;
use trueos_fpga_abi::lfm25_decode::{
    DecodeCapabilities, DecodeOpKind, DecodePlan, EmbeddingRowPlan, LayerStateSlot,
    MissingCapability, PlanError, TiedLmHeadPlan,
};

pub const HIDDEN_ELEMENTS: usize = lfm25::MODEL_HIDDEN_SIZE as usize;
pub const HIDDEN_Q8_BLOCKS: usize = HIDDEN_ELEMENTS / lfm25::Q8_0_BLOCK_VALUES;
/// Backend-owned storage identity for one fixed 1024-element tensor.
///
/// This is deliberately not a BAR address or a host pointer. The backend mints a
/// handle only after an MSI callback retires the operation which produced it,
/// and rejects handles from an old connection generation/session epoch.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ResidentTensorHandle {
    connection_generation: u32,
    session_epoch: u32,
    storage_slot: u16,
}

impl ResidentTensorHandle {
    pub(crate) const fn new(
        connection_generation: u32,
        session_epoch: u32,
        storage_slot: u16,
    ) -> Self {
        Self {
            connection_generation,
            session_epoch,
            storage_slot,
        }
    }

    pub const fn connection_generation(self) -> u32 {
        self.connection_generation
    }

    pub const fn session_epoch(self) -> u32 {
        self.session_epoch
    }

    pub const fn storage_slot(self) -> u16 {
        self.storage_slot
    }
}

/// Opaque FPGA-resident Q30[1024]. Rust can route this handle between fixed
/// operations, but there is no host slice and therefore no numerical fallback.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HiddenQ30(ResidentTensorHandle);

impl HiddenQ30 {
    pub(crate) const fn from_resident(handle: ResidentTensorHandle) -> Self {
        Self(handle)
    }

    pub const fn resident(self) -> ResidentTensorHandle {
        self.0
    }
}

/// Opaque FPGA-resident GGML Q8_0[1024] (exactly 32 native blocks).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HiddenQ8(ResidentTensorHandle);

impl HiddenQ8 {
    pub(crate) const fn from_resident(handle: ResidentTensorHandle) -> Self {
        Self(handle)
    }

    pub const fn resident(self) -> ResidentTensorHandle {
        self.0
    }
}

/// Fixed commands understood by an AOT TRUEGA decode firmware.
pub enum AotDecodeRequest {
    TokenEmbedding {
        row: EmbeddingRowPlan,
    },
    OperatorRmsNorm {
        layer: u8,
        input: HiddenQ30,
    },
    ShortConv {
        layer: u8,
        position: u32,
        state: LayerStateSlot,
        input: HiddenQ8,
    },
    Attention {
        layer: u8,
        position: u32,
        state: LayerStateSlot,
        input: HiddenQ8,
    },
    OperatorResidual {
        layer: u8,
        residual: HiddenQ30,
        branch: HiddenQ30,
    },
    FfnRmsNorm {
        layer: u8,
        input: HiddenQ30,
    },
    Ffn {
        layer: u8,
        input: HiddenQ8,
    },
    FfnResidual {
        layer: u8,
        residual: HiddenQ30,
        branch: HiddenQ30,
    },
    FinalRmsNorm {
        input: HiddenQ30,
    },
    /// The FPGA consumes all 65,536 tied embedding rows and retains the argmax.
    TiedLmHeadArgmax {
        head: TiedLmHeadPlan,
        input: HiddenQ8,
    },
}

impl AotDecodeRequest {
    pub const fn kind(&self) -> DecodeOpKind {
        match self {
            Self::TokenEmbedding { .. } => DecodeOpKind::TokenEmbedding,
            Self::OperatorRmsNorm { .. } => DecodeOpKind::OperatorRmsNorm,
            Self::ShortConv { .. } => DecodeOpKind::ShortConv,
            Self::Attention { .. } => DecodeOpKind::Attention,
            Self::OperatorResidual { .. } => DecodeOpKind::OperatorResidual,
            Self::FfnRmsNorm { .. } => DecodeOpKind::FfnRmsNorm,
            Self::Ffn { .. } => DecodeOpKind::Ffn,
            Self::FfnResidual { .. } => DecodeOpKind::FfnResidual,
            Self::FinalRmsNorm { .. } => DecodeOpKind::FinalRmsNorm,
            Self::TiedLmHeadArgmax { .. } => DecodeOpKind::TiedLmHeadArgmax,
        }
    }

    const fn is_stateful(&self) -> bool {
        matches!(self, Self::ShortConv { .. } | Self::Attention { .. })
    }
}

pub enum AotDecodeOutput {
    HiddenQ30(HiddenQ30),
    HiddenQ8(HiddenQ8),
    StatefulHiddenQ30 {
        output: HiddenQ30,
        state: LayerStateSlot,
        /// Position committed by the FPGA state circuit.
        position: u32,
    },
    Argmax {
        token: u32,
        score_q30: i64,
        rows: u32,
    },
}

/// Completion data delivered by the registered single-worker Rust callback.
pub struct AotDecodeCallback {
    pub operation: DecodeOpKind,
    /// Monotonic worker completion sequence, not a polled hardware counter.
    pub callback_sequence: u64,
    pub output: AotDecodeOutput,
}

/// Adapter boundary for generated TRUEGA operations.
///
/// `submit` registers the callback before ringing the device doorbell. Its future must
/// stay pending until the ISR wakes the single worker and the worker invokes that callback.
pub trait AotDecodeBackend {
    type Error;

    fn capabilities(&self) -> DecodeCapabilities;

    /// Number of token positions physically backed by the fused cache design.
    /// A token-1 BRAM image may report a small value; the future DDR image may
    /// report the complete sealed context. The scheduler never assumes DDR.
    fn max_context_positions(&self) -> u32;

    fn submit(
        &mut self,
        request: AotDecodeRequest,
    ) -> impl Future<Output = Result<AotDecodeCallback, Self::Error>> + '_;
}

#[derive(Debug, Eq, PartialEq)]
pub enum DecodeError<BackendError> {
    MissingCapability(MissingCapability),
    Plan(PlanError),
    Backend(BackendError),
    Busy,
    StatePoisoned,
    ContextFull,
    CallbackOperation {
        expected: DecodeOpKind,
        observed: DecodeOpKind,
    },
    CallbackSequence {
        previous: u64,
        observed: u64,
    },
    TensorDomain {
        expected_generation: u32,
        expected_epoch: u32,
        observed_generation: u32,
        observed_epoch: u32,
    },
    CallbackPayload(DecodeOpKind),
    StateCommit {
        expected_slot: LayerStateSlot,
        observed_slot: LayerStateSlot,
        expected_position: u32,
        observed_position: u32,
    },
}

impl<BackendError> From<PlanError> for DecodeError<BackendError> {
    fn from(value: PlanError) -> Self {
        Self::Plan(value)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DecodeTokenOutput {
    pub token: u32,
    pub score_q30: i64,
    pub input_position: u32,
    pub callback_sequence: u64,
}

/// Host mirror of the ten shortconv states and six KV caches held by FPGA circuits.
pub struct DecodeSession {
    position: u32,
    shortconv_next: [u32; 10],
    kv_next: [u32; 6],
    last_callback_sequence: u64,
    tensor_domain: Option<(u32, u32)>,
    in_flight: bool,
    in_flight_state_mutated: bool,
    poisoned: bool,
}

impl DecodeSession {
    pub const fn new() -> Self {
        Self {
            position: 0,
            shortconv_next: [0; 10],
            kv_next: [0; 6],
            last_callback_sequence: 0,
            tensor_domain: None,
            in_flight: false,
            in_flight_state_mutated: false,
            poisoned: false,
        }
    }

    pub const fn position(&self) -> u32 {
        self.position
    }

    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Use only after the backend has reset all FPGA recurrent/KV state circuits.
    pub fn acknowledge_hardware_state_reset(&mut self) {
        self.position = 0;
        self.shortconv_next = [0; 10];
        self.kv_next = [0; 6];
        self.tensor_domain = None;
        self.in_flight = false;
        self.in_flight_state_mutated = false;
        self.poisoned = false;
    }

    pub async fn decode_token<Backend: AotDecodeBackend>(
        &mut self,
        backend: &mut Backend,
        input_token: u32,
    ) -> Result<DecodeTokenOutput, DecodeError<Backend::Error>> {
        if self.in_flight {
            return Err(DecodeError::Busy);
        }
        if self.poisoned {
            return Err(DecodeError::StatePoisoned);
        }
        if self.position >= lfm25::MODEL_INITIAL_CONTEXT
            || self.position >= backend.max_context_positions()
        {
            return Err(DecodeError::ContextFull);
        }
        DecodePlan::require_capabilities(backend.capabilities())
            .map_err(DecodeError::MissingCapability)?;
        let embedding = EmbeddingRowPlan::new(input_token)?;
        let head = TiedLmHeadPlan::new()?;

        self.in_flight = true;
        self.in_flight_state_mutated = false;
        let result = self.run_token(backend, embedding, head).await;
        if result.is_err() && self.in_flight_state_mutated {
            self.poisoned = true;
        }
        self.in_flight = false;
        self.in_flight_state_mutated = false;
        result
    }

    async fn run_token<Backend: AotDecodeBackend>(
        &mut self,
        backend: &mut Backend,
        embedding: EmbeddingRowPlan,
        head: TiedLmHeadPlan,
    ) -> Result<DecodeTokenOutput, DecodeError<Backend::Error>> {
        let input_position = self.position;
        let callback = self
            .call(backend, AotDecodeRequest::TokenEmbedding { row: embedding })
            .await?;
        let mut hidden = self.expect_q30(DecodeOpKind::TokenEmbedding, callback.output)?;

        for layer in 0..lfm25::MODEL_LAYER_COUNT as u8 {
            let operator_residual = hidden.clone();
            let callback = self
                .call(
                    backend,
                    AotDecodeRequest::OperatorRmsNorm {
                        layer,
                        input: hidden,
                    },
                )
                .await?;
            let normalized = self.expect_q8(DecodeOpKind::OperatorRmsNorm, callback.output)?;

            let state = trueos_fpga_abi::lfm25_decode::state_slot_for_layer(layer)
                .ok_or(DecodeError::CallbackPayload(DecodeOpKind::ShortConv))?;
            self.require_state_position(state)?;
            let request = match lfm25::LAYER_SCHEDULE[layer as usize] {
                lfm25::LayerKind::ShortConv => AotDecodeRequest::ShortConv {
                    layer,
                    position: self.position,
                    state,
                    input: normalized,
                },
                lfm25::LayerKind::Attention => AotDecodeRequest::Attention {
                    layer,
                    position: self.position,
                    state,
                    input: normalized,
                },
            };
            let mixer_kind = request.kind();
            let callback = self.call(backend, request).await?;
            let branch = self.expect_stateful_q30(mixer_kind, state, callback.output)?;
            self.commit_state(state);

            let callback = self
                .call(
                    backend,
                    AotDecodeRequest::OperatorResidual {
                        layer,
                        residual: operator_residual,
                        branch,
                    },
                )
                .await?;
            hidden = self.expect_q30(DecodeOpKind::OperatorResidual, callback.output)?;

            let ffn_residual = hidden.clone();
            let callback = self
                .call(
                    backend,
                    AotDecodeRequest::FfnRmsNorm {
                        layer,
                        input: hidden,
                    },
                )
                .await?;
            let ffn_input = self.expect_q8(DecodeOpKind::FfnRmsNorm, callback.output)?;
            let callback = self
                .call(
                    backend,
                    AotDecodeRequest::Ffn {
                        layer,
                        input: ffn_input,
                    },
                )
                .await?;
            let ffn_output = self.expect_q30(DecodeOpKind::Ffn, callback.output)?;
            let callback = self
                .call(
                    backend,
                    AotDecodeRequest::FfnResidual {
                        layer,
                        residual: ffn_residual,
                        branch: ffn_output,
                    },
                )
                .await?;
            hidden = self.expect_q30(DecodeOpKind::FfnResidual, callback.output)?;
        }

        let callback = self
            .call(backend, AotDecodeRequest::FinalRmsNorm { input: hidden })
            .await?;
        let normalized = self.expect_q8(DecodeOpKind::FinalRmsNorm, callback.output)?;
        let callback = self
            .call(
                backend,
                AotDecodeRequest::TiedLmHeadArgmax {
                    head,
                    input: normalized,
                },
            )
            .await?;
        let (token, score_q30) = match callback.output {
            AotDecodeOutput::Argmax {
                token,
                score_q30,
                rows,
            } if token < lfm25::MODEL_VOCABULARY_SIZE && rows == lfm25::MODEL_VOCABULARY_SIZE => {
                (token, score_q30)
            }
            _ => return self.payload_error(DecodeOpKind::TiedLmHeadArgmax),
        };

        self.require_all_states_at(self.position + 1)?;
        self.position += 1;
        Ok(DecodeTokenOutput {
            token,
            score_q30,
            input_position,
            callback_sequence: self.last_callback_sequence,
        })
    }

    async fn call<Backend: AotDecodeBackend>(
        &mut self,
        backend: &mut Backend,
        request: AotDecodeRequest,
    ) -> Result<AotDecodeCallback, DecodeError<Backend::Error>> {
        let expected = request.kind();
        if request.is_stateful() {
            // Once rung, a missing/malformed callback leaves device state uncertain.
            self.in_flight_state_mutated = true;
        }
        let callback = backend
            .submit(request)
            .await
            .map_err(DecodeError::Backend)?;
        if callback.operation != expected {
            return Err(DecodeError::CallbackOperation {
                expected,
                observed: callback.operation,
            });
        }
        if callback.callback_sequence <= self.last_callback_sequence {
            return Err(DecodeError::CallbackSequence {
                previous: self.last_callback_sequence,
                observed: callback.callback_sequence,
            });
        }
        self.last_callback_sequence = callback.callback_sequence;
        Ok(callback)
    }

    fn expect_q30<BackendError>(
        &mut self,
        kind: DecodeOpKind,
        output: AotDecodeOutput,
    ) -> Result<HiddenQ30, DecodeError<BackendError>> {
        match output {
            AotDecodeOutput::HiddenQ30(output) => {
                self.admit_tensor_domain(output.resident())?;
                Ok(output)
            }
            _ => self.payload_error(kind),
        }
    }

    fn expect_q8<BackendError>(
        &mut self,
        kind: DecodeOpKind,
        output: AotDecodeOutput,
    ) -> Result<HiddenQ8, DecodeError<BackendError>> {
        match output {
            AotDecodeOutput::HiddenQ8(output) => {
                self.admit_tensor_domain(output.resident())?;
                Ok(output)
            }
            _ => self.payload_error(kind),
        }
    }

    fn expect_stateful_q30<BackendError>(
        &mut self,
        kind: DecodeOpKind,
        expected_state: LayerStateSlot,
        output: AotDecodeOutput,
    ) -> Result<HiddenQ30, DecodeError<BackendError>> {
        match output {
            AotDecodeOutput::StatefulHiddenQ30 {
                output,
                state,
                position,
            } if state == expected_state && position == self.position => {
                self.admit_tensor_domain(output.resident())?;
                Ok(output)
            }
            AotDecodeOutput::StatefulHiddenQ30 {
                state, position, ..
            } => Err(DecodeError::StateCommit {
                expected_slot: expected_state,
                observed_slot: state,
                expected_position: self.position,
                observed_position: position,
            }),
            _ => self.payload_error(kind),
        }
    }

    fn payload_error<T, BackendError>(
        &mut self,
        kind: DecodeOpKind,
    ) -> Result<T, DecodeError<BackendError>> {
        Err(DecodeError::CallbackPayload(kind))
    }

    fn admit_tensor_domain<BackendError>(
        &mut self,
        handle: ResidentTensorHandle,
    ) -> Result<(), DecodeError<BackendError>> {
        let observed = (handle.connection_generation(), handle.session_epoch());
        match self.tensor_domain {
            None => {
                self.tensor_domain = Some(observed);
                Ok(())
            }
            Some(expected) if expected == observed => Ok(()),
            Some((expected_generation, expected_epoch)) => {
                self.poisoned = true;
                Err(DecodeError::TensorDomain {
                    expected_generation,
                    expected_epoch,
                    observed_generation: observed.0,
                    observed_epoch: observed.1,
                })
            }
        }
    }

    fn require_state_position<BackendError>(
        &mut self,
        state: LayerStateSlot,
    ) -> Result<(), DecodeError<BackendError>> {
        let observed = match state {
            LayerStateSlot::ShortConv(slot) => self.shortconv_next[slot as usize],
            LayerStateSlot::KvCache(slot) => self.kv_next[slot as usize],
        };
        if observed == self.position {
            Ok(())
        } else {
            self.poisoned = true;
            Err(DecodeError::StatePoisoned)
        }
    }

    fn commit_state(&mut self, state: LayerStateSlot) {
        match state {
            LayerStateSlot::ShortConv(slot) => {
                self.shortconv_next[slot as usize] = self.position + 1
            }
            LayerStateSlot::KvCache(slot) => self.kv_next[slot as usize] = self.position + 1,
        }
    }

    fn require_all_states_at<BackendError>(
        &mut self,
        expected: u32,
    ) -> Result<(), DecodeError<BackendError>> {
        if self
            .shortconv_next
            .iter()
            .all(|position| *position == expected)
            && self.kv_next.iter().all(|position| *position == expected)
        {
            Ok(())
        } else {
            self.poisoned = true;
            Err(DecodeError::StatePoisoned)
        }
    }
}

impl Default for DecodeSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Explicit production placeholder used until all required RTL capability magics land.
/// It cannot accidentally execute a CPU implementation.
pub struct FailClosedBackend;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HardwareDecodeUnavailable;

impl AotDecodeBackend for FailClosedBackend {
    type Error = HardwareDecodeUnavailable;

    fn capabilities(&self) -> DecodeCapabilities {
        DecodeCapabilities::NONE
    }

    fn max_context_positions(&self) -> u32 {
        0
    }

    async fn submit(
        &mut self,
        _request: AotDecodeRequest,
    ) -> Result<AotDecodeCallback, Self::Error> {
        Err(HardwareDecodeUnavailable)
    }
}
