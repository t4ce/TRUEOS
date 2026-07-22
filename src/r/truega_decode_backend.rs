//! Concrete TRUEGA mapping for the fixed LFM2.5 AOT decode scheduler.
//!
//! Numerical work never occurs here. This module validates scheduler order and
//! opaque resident handles, requires an exact TGF2 data-plane receipt for every
//! operation, and maps exactly one fixed request onto one TGD1 command. A resident
//! output handle is minted only after the transport's MSI-driven completion future
//! resolves.

extern crate alloc;

#[cfg(target_os = "trueos")]
use alloc::vec::Vec;
use core::future::Future;

use trueos_fpga_abi::lfm25;
use trueos_fpga_abi::lfm25_decode::{
    DecodeCapabilities, DecodeOpKind, DecodePlan, EmbeddingRowPlan, LayerStateSlot, OPS_PER_TOKEN,
    TiedLmHeadPlan,
};
use trueos_fpga_abi::lfm25_decode_feed::{
    FeedCapability, FeedMode, FeedRequest, capability_is_exact,
};
use trueos_fpga_abi::lfm25_decode_transport::{Command, Completion, NO_RESIDENT_SLOT};

use super::lfm25_decode::{
    AotDecodeBackend, AotDecodeCallback, AotDecodeOutput, AotDecodeRequest, HiddenQ8, HiddenQ30,
    ResidentTensorHandle,
};

/// The logical identity of all resident tensors in one acquired hardware session.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DecodeTensorDomain {
    pub connection_generation: u32,
    pub session_epoch: u32,
}

pub const MAX_FEED_SEQUENCES_PER_OPERATION: usize = 6;

/// Exact data-plane work which must retire before one TGD1 command may be rung.
///
/// The command binds operation/layer/position/input slots/session epoch. `domain` adds
/// the connection generation, `ordinal` binds scheduler order, and `feeds` names every
/// TGF2 sequence needed by that fixed circuit. TokenEmbedding's dynamic native row is
/// retained explicitly as well as in its TGF2 token field.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DecodeDataPlaneRequest {
    pub domain: DecodeTensorDomain,
    pub ordinal: u8,
    pub command: Command,
    pub embedding_row: Option<EmbeddingRowPlan>,
    pub feeds: [Option<FeedRequest>; MAX_FEED_SEQUENCES_PER_OPERATION],
}

impl DecodeDataPlaneRequest {
    fn new(
        domain: DecodeTensorDomain,
        ordinal: u8,
        command: Command,
        embedding_row: Option<EmbeddingRowPlan>,
    ) -> Result<Self, ()> {
        let feed = |mode, layer, token| {
            Some(FeedRequest {
                mode,
                layer,
                position: command.position,
                token,
                session_epoch: command.session_epoch,
            })
        };
        let feeds = match command.operation {
            DecodeOpKind::TokenEmbedding => [
                feed(FeedMode::EmbeddingQ8Row, None, Some(embedding_row.ok_or(())?.token)),
                None,
                None,
                None,
                None,
                None,
            ],
            DecodeOpKind::OperatorRmsNorm => [
                feed(FeedMode::OperatorRmsNormWeights, command.layer, None),
                None,
                None,
                None,
                None,
                None,
            ],
            DecodeOpKind::ShortConv => [
                feed(FeedMode::ShortConvCoefficients, command.layer, None),
                feed(FeedMode::ShortConvInputTripletRows, command.layer, None),
                feed(FeedMode::ShortConvOutputRows, command.layer, None),
                None,
                None,
                None,
            ],
            DecodeOpKind::Attention => [
                feed(FeedMode::AttentionQkNormWeights, command.layer, None),
                feed(FeedMode::AttentionQueryRows, command.layer, None),
                feed(FeedMode::AttentionKeyRows, command.layer, None),
                feed(FeedMode::AttentionValueRows, command.layer, None),
                feed(FeedMode::AttentionFirstTokenCore, command.layer, None),
                feed(FeedMode::AttentionOutputRows, command.layer, None),
            ],
            DecodeOpKind::OperatorResidual | DecodeOpKind::FfnResidual => {
                [None; MAX_FEED_SEQUENCES_PER_OPERATION]
            }
            DecodeOpKind::FfnRmsNorm => [
                feed(FeedMode::FfnRmsNormWeights, command.layer, None),
                None,
                None,
                None,
                None,
                None,
            ],
            DecodeOpKind::Ffn => [
                feed(FeedMode::FfnGateUpRows, command.layer, None),
                feed(FeedMode::FfnDownRows, command.layer, None),
                None,
                None,
                None,
                None,
            ],
            DecodeOpKind::FinalRmsNorm => [
                feed(FeedMode::FinalRmsNormWeights, None, None),
                None,
                None,
                None,
                None,
                None,
            ],
            DecodeOpKind::TiedLmHeadArgmax => [
                feed(FeedMode::TiedLmHeadRows, None, None),
                None,
                None,
                None,
                None,
                None,
            ],
        };
        if command.operation != DecodeOpKind::TokenEmbedding && embedding_row.is_some() {
            return Err(());
        }
        for request in feeds.iter().flatten() {
            request.validate().map_err(|_| ())?;
        }
        Ok(Self {
            domain,
            ordinal,
            command,
            embedding_row,
            feeds,
        })
    }
}

/// Typed proof that every TGF2 sequence in [`DecodeDataPlaneRequest`] completed. The
/// fixed commit counts prevent a partial row/model stream from being acknowledged as
/// ready. The backend compares the complete receipt before touching the TGD1 doorbell.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DecodeDataPlaneReceipt {
    pub request: DecodeDataPlaneRequest,
    pub committed_units: [u32; MAX_FEED_SEQUENCES_PER_OPERATION],
}

impl DecodeDataPlaneReceipt {
    pub fn complete(request: DecodeDataPlaneRequest) -> Self {
        let mut committed_units = [0; MAX_FEED_SEQUENCES_PER_OPERATION];
        let mut index = 0;
        while index < request.feeds.len() {
            if let Some(feed) = request.feeds[index] {
                committed_units[index] = feed.mode.shape().commits();
            }
            index += 1;
        }
        Self {
            request,
            committed_units,
        }
    }
}

/// Fixed command transport. The kernel implementation below is backed by the existing
/// single fpga-offload worker; host tests substitute a deterministic fake.
pub trait DecodeCommandTransport {
    type Session;
    type Error;

    /// Must be true only for exact TGD1 magic plus the exact v1 capability word.
    fn exact_capability_available(&self) -> bool;

    fn acquire(&mut self) -> impl Future<Output = Result<Self::Session, Self::Error>> + '_;

    fn domain(session: &Self::Session) -> DecodeTensorDomain;

    fn execute<'a>(
        &'a mut self,
        session: &'a mut Self::Session,
        command: Command,
    ) -> impl Future<Output = Result<Completion, Self::Error>> + 'a;
}

/// Required model data-plane boundary.
///
/// TGD1's command word deliberately has no token or native-image address field. A
/// production backend therefore remains unavailable until this hook has observed the
/// exact TGF2 capability, has the sealed model payloads ready, and can complete the
/// tagged TGF2 stage/commit sequences for every operation while the matching BAR2
/// session lane is owned.
pub trait DecodeModelDataPlane {
    /// Must be the exact session type used to issue the following TGD1 command.
    /// This prevents a feeder from acquiring a second BAR2 lane or minting an
    /// unrelated epoch.
    type Session;
    type Error;

    /// The capability read from the TGF2 publication registers. The backend performs
    /// exact equality itself; absence, unknown bits, or a different shape tag fail closed.
    fn published_feed_capability(&self) -> Option<FeedCapability>;

    /// TGF2 support alone is not proof that model payloads have been made available.
    fn sealed_model_payloads_ready(&self) -> bool;
    fn max_context_positions(&self) -> u32;

    fn prepare_operation<'a>(
        &'a mut self,
        session: &'a mut Self::Session,
        request: DecodeDataPlaneRequest,
    ) -> impl Future<Output = Result<DecodeDataPlaneReceipt, Self::Error>> + 'a;
}

#[derive(Debug, Eq, PartialEq)]
pub enum TruegaDecodeBackendError<TransportError, DataPlaneError> {
    Unavailable,
    Poisoned,
    Transport(TransportError),
    DataPlane(DataPlaneError),
    Sequence {
        ordinal: u8,
        expected: DecodeOpKind,
        observed: DecodeOpKind,
    },
    RequestShape(DecodeOpKind),
    DataPlaneReceipt,
    ResidentDomain {
        expected: DecodeTensorDomain,
        observed: DecodeTensorDomain,
    },
    ResidentSlot(u16),
    CompletionKind(DecodeOpKind),
    CompletionPosition {
        expected: u32,
        observed: u32,
    },
    CallbackSequenceOverflow,
    PositionOverflow,
    InternalPlan,
}

#[derive(Copy, Clone)]
enum OutputContract {
    HiddenQ30,
    HiddenQ8,
    StatefulQ30(LayerStateSlot),
    Argmax,
}

/// Exact 99-operation backend mapping.
///
/// `Session` is acquired only on the first valid submission. Rejected ordering, shape,
/// or resident-handle requests never ring hardware. A data-plane or transport failure is
/// conservatively sticky because device-side state may have changed.
pub struct TruegaAotDecodeBackend<Transport, DataPlane>
where
    Transport: DecodeCommandTransport,
    DataPlane: DecodeModelDataPlane<Session = Transport::Session>,
{
    transport: Transport,
    data_plane: DataPlane,
    session: Option<Transport::Session>,
    position: u32,
    next_ordinal: u8,
    callback_sequence: u64,
    poisoned: bool,
}

impl<Transport, DataPlane> TruegaAotDecodeBackend<Transport, DataPlane>
where
    Transport: DecodeCommandTransport,
    DataPlane: DecodeModelDataPlane<Session = Transport::Session>,
{
    pub const fn new(transport: Transport, data_plane: DataPlane) -> Self {
        Self {
            transport,
            data_plane,
            session: None,
            position: 0,
            next_ordinal: 0,
            callback_sequence: 0,
            poisoned: false,
        }
    }

    pub const fn position(&self) -> u32 {
        self.position
    }

    pub const fn next_ordinal(&self) -> u8 {
        self.next_ordinal
    }

    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Call only after recurrent/KV/resident state has been reset in hardware. Dropping
    /// the old transport session releases BAR2 and guarantees a newly minted epoch.
    pub fn acknowledge_hardware_state_reset(&mut self) {
        self.session = None;
        self.position = 0;
        self.next_ordinal = 0;
        self.poisoned = false;
        // callback_sequence deliberately remains monotonic for DecodeSession.
    }

    pub fn into_parts(self) -> (Transport, DataPlane, Option<Transport::Session>) {
        (self.transport, self.data_plane, self.session)
    }

    fn resident_slot(
        handle: ResidentTensorHandle,
        domain: DecodeTensorDomain,
    ) -> Result<u8, TruegaDecodeBackendError<Transport::Error, DataPlane::Error>> {
        let observed = DecodeTensorDomain {
            connection_generation: handle.connection_generation(),
            session_epoch: handle.session_epoch(),
        };
        if observed != domain {
            return Err(TruegaDecodeBackendError::ResidentDomain {
                expected: domain,
                observed,
            });
        }
        let slot = handle.storage_slot();
        if slot >= NO_RESIDENT_SLOT as u16 {
            return Err(TruegaDecodeBackendError::ResidentSlot(slot));
        }
        Ok(slot as u8)
    }

    fn request_shape_error(
        operation: DecodeOpKind,
    ) -> TruegaDecodeBackendError<Transport::Error, DataPlane::Error> {
        TruegaDecodeBackendError::RequestShape(operation)
    }

    fn completion_output(
        operation: DecodeOpKind,
        contract: OutputContract,
        completion: Completion,
        domain: DecodeTensorDomain,
        position: u32,
    ) -> Result<AotDecodeOutput, TruegaDecodeBackendError<Transport::Error, DataPlane::Error>> {
        match (contract, completion) {
            (
                OutputContract::HiddenQ30,
                Completion::Resident {
                    storage_slot,
                    position: observed,
                },
            ) => {
                if observed != position {
                    return Err(TruegaDecodeBackendError::CompletionPosition {
                        expected: position,
                        observed,
                    });
                }
                Ok(AotDecodeOutput::HiddenQ30(HiddenQ30::from_resident(ResidentTensorHandle::new(
                    domain.connection_generation,
                    domain.session_epoch,
                    storage_slot as u16,
                ))))
            }
            (
                OutputContract::HiddenQ8,
                Completion::Resident {
                    storage_slot,
                    position: observed,
                },
            ) => {
                if observed != position {
                    return Err(TruegaDecodeBackendError::CompletionPosition {
                        expected: position,
                        observed,
                    });
                }
                Ok(AotDecodeOutput::HiddenQ8(HiddenQ8::from_resident(ResidentTensorHandle::new(
                    domain.connection_generation,
                    domain.session_epoch,
                    storage_slot as u16,
                ))))
            }
            (
                OutputContract::StatefulQ30(state),
                Completion::Resident {
                    storage_slot,
                    position: observed,
                },
            ) => {
                if observed != position {
                    return Err(TruegaDecodeBackendError::CompletionPosition {
                        expected: position,
                        observed,
                    });
                }
                Ok(AotDecodeOutput::StatefulHiddenQ30 {
                    output: HiddenQ30::from_resident(ResidentTensorHandle::new(
                        domain.connection_generation,
                        domain.session_epoch,
                        storage_slot as u16,
                    )),
                    state,
                    position: observed,
                })
            }
            (
                OutputContract::Argmax,
                Completion::Argmax {
                    token,
                    score_q30,
                    rows,
                },
            ) if token < lfm25::MODEL_VOCABULARY_SIZE && rows == lfm25::MODEL_VOCABULARY_SIZE => {
                Ok(AotDecodeOutput::Argmax {
                    token,
                    score_q30,
                    rows,
                })
            }
            _ => Err(TruegaDecodeBackendError::CompletionKind(operation)),
        }
    }
}

impl<Transport, DataPlane> AotDecodeBackend for TruegaAotDecodeBackend<Transport, DataPlane>
where
    Transport: DecodeCommandTransport,
    DataPlane: DecodeModelDataPlane<Session = Transport::Session>,
{
    type Error = TruegaDecodeBackendError<Transport::Error, DataPlane::Error>;

    fn capabilities(&self) -> DecodeCapabilities {
        let exact_feed = match self.data_plane.published_feed_capability() {
            Some(capability) => capability_is_exact(capability),
            None => false,
        };
        if !self.poisoned
            && self.transport.exact_capability_available()
            && exact_feed
            && self.data_plane.sealed_model_payloads_ready()
        {
            DecodeCapabilities::ALL
        } else {
            DecodeCapabilities::NONE
        }
    }

    fn max_context_positions(&self) -> u32 {
        if self.capabilities() == DecodeCapabilities::ALL {
            self.data_plane
                .max_context_positions()
                .min(lfm25::MODEL_INITIAL_CONTEXT)
                // TGF2 publishes CAP_ATTENTION_FIRST_TOKEN, not a later-position
                // attention/KV feed contract.
                .min(1)
        } else {
            0
        }
    }

    async fn submit(
        &mut self,
        request: AotDecodeRequest,
    ) -> Result<AotDecodeCallback, Self::Error> {
        if self.poisoned {
            return Err(TruegaDecodeBackendError::Poisoned);
        }
        if self.capabilities() != DecodeCapabilities::ALL {
            return Err(TruegaDecodeBackendError::Unavailable);
        }

        let operation = request.kind();
        let expected = DecodePlan::new()
            .nth(self.next_ordinal as usize)
            .ok_or(TruegaDecodeBackendError::InternalPlan)?;
        if expected.kind != operation {
            return Err(TruegaDecodeBackendError::Sequence {
                ordinal: self.next_ordinal,
                expected: expected.kind,
                observed: operation,
            });
        }

        if self.session.is_none() {
            let session = self
                .transport
                .acquire()
                .await
                .map_err(TruegaDecodeBackendError::Transport)?;
            self.session = Some(session);
        }
        let domain = Transport::domain(
            self.session
                .as_ref()
                .ok_or(TruegaDecodeBackendError::InternalPlan)?,
        );
        if domain.connection_generation == 0 || domain.session_epoch == 0 {
            return Err(TruegaDecodeBackendError::Unavailable);
        }

        let (command, contract, embedding_row) = match request {
            AotDecodeRequest::TokenEmbedding { row } => {
                if expected.layer.is_some() || EmbeddingRowPlan::new(row.token).ok() != Some(row) {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: None,
                        position: self.position,
                        input_slot: None,
                        residual_slot: None,
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::HiddenQ30,
                    Some(row),
                )
            }
            AotDecodeRequest::OperatorRmsNorm { layer, input } => {
                if expected.layer != Some(layer) {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: Some(layer),
                        position: self.position,
                        input_slot: Some(Self::resident_slot(input.resident(), domain)?),
                        residual_slot: None,
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::HiddenQ8,
                    None,
                )
            }
            AotDecodeRequest::ShortConv {
                layer,
                position,
                state,
                input,
            } => {
                if expected.layer != Some(layer)
                    || expected.state != Some(state)
                    || position != self.position
                {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: Some(layer),
                        position,
                        input_slot: Some(Self::resident_slot(input.resident(), domain)?),
                        residual_slot: None,
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::StatefulQ30(state),
                    None,
                )
            }
            AotDecodeRequest::Attention {
                layer,
                position,
                state,
                input,
            } => {
                if expected.layer != Some(layer)
                    || expected.state != Some(state)
                    || position != self.position
                {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: Some(layer),
                        position,
                        input_slot: Some(Self::resident_slot(input.resident(), domain)?),
                        residual_slot: None,
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::StatefulQ30(state),
                    None,
                )
            }
            AotDecodeRequest::OperatorResidual {
                layer,
                residual,
                branch,
            } => {
                if expected.layer != Some(layer) {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: Some(layer),
                        position: self.position,
                        input_slot: Some(Self::resident_slot(branch.resident(), domain)?),
                        residual_slot: Some(Self::resident_slot(residual.resident(), domain)?),
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::HiddenQ30,
                    None,
                )
            }
            AotDecodeRequest::FfnRmsNorm { layer, input } => {
                if expected.layer != Some(layer) {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: Some(layer),
                        position: self.position,
                        input_slot: Some(Self::resident_slot(input.resident(), domain)?),
                        residual_slot: None,
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::HiddenQ8,
                    None,
                )
            }
            AotDecodeRequest::Ffn { layer, input } => {
                if expected.layer != Some(layer) {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: Some(layer),
                        position: self.position,
                        input_slot: Some(Self::resident_slot(input.resident(), domain)?),
                        residual_slot: None,
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::HiddenQ30,
                    None,
                )
            }
            AotDecodeRequest::FfnResidual {
                layer,
                residual,
                branch,
            } => {
                if expected.layer != Some(layer) {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: Some(layer),
                        position: self.position,
                        input_slot: Some(Self::resident_slot(branch.resident(), domain)?),
                        residual_slot: Some(Self::resident_slot(residual.resident(), domain)?),
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::HiddenQ30,
                    None,
                )
            }
            AotDecodeRequest::FinalRmsNorm { input } => {
                if expected.layer.is_some() {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: None,
                        position: self.position,
                        input_slot: Some(Self::resident_slot(input.resident(), domain)?),
                        residual_slot: None,
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::HiddenQ8,
                    None,
                )
            }
            AotDecodeRequest::TiedLmHeadArgmax { head, input } => {
                if expected.layer.is_some() || TiedLmHeadPlan::new().ok() != Some(head) {
                    return Err(Self::request_shape_error(operation));
                }
                (
                    Command {
                        operation,
                        layer: None,
                        position: self.position,
                        input_slot: Some(Self::resident_slot(input.resident(), domain)?),
                        residual_slot: None,
                        session_epoch: domain.session_epoch,
                    },
                    OutputContract::Argmax,
                    None,
                )
            }
        };

        command
            .validate()
            .map_err(|_| Self::request_shape_error(operation))?;
        let data_plane_request =
            DecodeDataPlaneRequest::new(domain, self.next_ordinal, command, embedding_row)
                .map_err(|_| Self::request_shape_error(operation))?;
        let expected_receipt = DecodeDataPlaneReceipt::complete(data_plane_request);
        // From the first model-feed read through the TGD1 completion callback,
        // cancellation must leave the session unusable. Only the fully checked
        // success path below clears this in-flight poison marker.
        self.poisoned = true;
        let receipt = self
            .data_plane
            .prepare_operation(
                self.session
                    .as_mut()
                    .ok_or(TruegaDecodeBackendError::InternalPlan)?,
                data_plane_request,
            )
            .await
            .map_err(|error| {
                self.poisoned = true;
                TruegaDecodeBackendError::DataPlane(error)
            })?;
        if receipt != expected_receipt {
            self.poisoned = true;
            return Err(TruegaDecodeBackendError::DataPlaneReceipt);
        }
        let completion = match self
            .transport
            .execute(
                self.session
                    .as_mut()
                    .ok_or(TruegaDecodeBackendError::InternalPlan)?,
                command,
            )
            .await
        {
            Ok(completion) => completion,
            Err(error) => {
                self.poisoned = true;
                return Err(TruegaDecodeBackendError::Transport(error));
            }
        };
        let output =
            match Self::completion_output(operation, contract, completion, domain, self.position) {
                Ok(output) => output,
                Err(error) => {
                    self.poisoned = true;
                    return Err(error);
                }
            };

        self.callback_sequence = self
            .callback_sequence
            .checked_add(1)
            .ok_or(TruegaDecodeBackendError::CallbackSequenceOverflow)?;
        if self.next_ordinal as usize + 1 == OPS_PER_TOKEN {
            self.next_ordinal = 0;
            self.position = self
                .position
                .checked_add(1)
                .ok_or(TruegaDecodeBackendError::PositionOverflow)?;
        } else {
            self.next_ordinal += 1;
        }
        self.poisoned = false;

        Ok(AotDecodeCallback {
            operation,
            callback_sequence: self.callback_sequence,
            output,
        })
    }
}

#[cfg(target_os = "trueos")]
pub struct KernelDecodeCommandTransport;

#[cfg(target_os = "trueos")]
impl DecodeCommandTransport for KernelDecodeCommandTransport {
    type Session = super::fpga_offload::Lfm25DecodeTransportSession;
    type Error = super::fpga_offload::Error;

    fn exact_capability_available(&self) -> bool {
        super::fpga_offload::lfm25_decode_transport_available()
    }

    async fn acquire(&mut self) -> Result<Self::Session, Self::Error> {
        super::fpga_offload::acquire_lfm25_decode_transport().await
    }

    fn domain(session: &Self::Session) -> DecodeTensorDomain {
        DecodeTensorDomain {
            connection_generation: session.connection_generation(),
            session_epoch: session.session_epoch(),
        }
    }

    async fn execute<'a>(
        &'a mut self,
        session: &'a mut Self::Session,
        command: Command,
    ) -> Result<Completion, Self::Error> {
        session.execute(command).await
    }
}

#[cfg(target_os = "trueos")]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KernelDecodeDataPlaneError {
    Model(super::lfm25_model::Error),
    Feed(trueos_fpga_abi::lfm25_decode_feed::FeedError),
    Transport(super::fpga_offload::Error),
    Capability,
    SessionDomain,
    BufferUnavailable,
    TensorRange,
    Retirement,
}

#[cfg(target_os = "trueos")]
const MODEL_RANGE_CACHE_BYTES: usize = 4096;

/// One lane-local native-image page. TGF2 consumes paired/triplet tensors in
/// stage-major order, so a single shared cache would thrash between distant
/// tensor ranges. Three fixed pages preserve that ordering while coalescing
/// the underlying TRUEOSFS reads; returned BAR payloads remain exact 34/64-byte
/// validator-issued slices.
#[cfg(target_os = "trueos")]
struct KernelDecodeRangeCache {
    valid: bool,
    page_offset: u64,
    valid_bytes: usize,
    bytes: [u8; MODEL_RANGE_CACHE_BYTES],
}

#[cfg(target_os = "trueos")]
impl KernelDecodeRangeCache {
    const fn new() -> Self {
        Self {
            valid: false,
            page_offset: 0,
            valid_bytes: 0,
            bytes: [0; MODEL_RANGE_CACHE_BYTES],
        }
    }
}

/// One verified, pinned native-image handle feeding the same BAR2 session that
/// executes TGD1. Only one item worth of 34/64-byte stages exists in memory at a
/// time; the complete tensor is never materialized by the kernel.
#[cfg(target_os = "trueos")]
pub struct KernelDecodeModelDataPlane {
    image: super::lfm25_model::NativeImage,
    verified_sha256: [u8; 32],
    range_cache: [KernelDecodeRangeCache; 3],
}

#[cfg(target_os = "trueos")]
impl KernelDecodeModelDataPlane {
    pub async fn open_verified() -> Result<Self, KernelDecodeDataPlaneError> {
        let image = super::lfm25_model::open()
            .await
            .map_err(KernelDecodeDataPlaneError::Model)?;
        let verified_sha256 = super::lfm25_model::verify_with_progress(&image, |_, _| {})
            .await
            .map_err(KernelDecodeDataPlaneError::Model)?;
        if verified_sha256 != super::lfm25_model::NATIVE_IMAGE_SHA256 {
            return Err(KernelDecodeDataPlaneError::Capability);
        }
        Ok(Self {
            image,
            verified_sha256,
            range_cache: [
                KernelDecodeRangeCache::new(),
                KernelDecodeRangeCache::new(),
                KernelDecodeRangeCache::new(),
            ],
        })
    }

    async fn read_stage_payload(
        &mut self,
        bank: trueos_fpga_abi::lfm25_decode_feed::StageBank,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), KernelDecodeDataPlaneError> {
        let cache = &mut self.range_cache[bank as usize];
        let mut source_offset = offset;
        let mut destination_offset = 0usize;
        while destination_offset < out.len() {
            let page_offset = source_offset & !((MODEL_RANGE_CACHE_BYTES as u64) - 1);
            if !cache.valid || cache.page_offset != page_offset {
                cache.valid = false;
                let available = self.image.len().saturating_sub(page_offset);
                let want = core::cmp::min(available, MODEL_RANGE_CACHE_BYTES as u64) as usize;
                if want == 0 {
                    return Err(KernelDecodeDataPlaneError::TensorRange);
                }
                self.image
                    .read_exact_at(page_offset, &mut cache.bytes[..want])
                    .await
                    .map_err(KernelDecodeDataPlaneError::Model)?;
                cache.page_offset = page_offset;
                cache.valid_bytes = want;
                cache.valid = true;
            }

            let in_page = source_offset
                .checked_sub(cache.page_offset)
                .and_then(|offset| usize::try_from(offset).ok())
                .ok_or(KernelDecodeDataPlaneError::TensorRange)?;
            if in_page >= cache.valid_bytes {
                return Err(KernelDecodeDataPlaneError::TensorRange);
            }
            let take = core::cmp::min(cache.valid_bytes - in_page, out.len() - destination_offset);
            out[destination_offset..destination_offset + take]
                .copy_from_slice(&cache.bytes[in_page..in_page + take]);
            destination_offset += take;
            source_offset = source_offset
                .checked_add(take as u64)
                .ok_or(KernelDecodeDataPlaneError::TensorRange)?;
        }
        Ok(())
    }

    fn expected_tensor_bytes(
        tensor: trueos_fpga_abi::lfm25_decode_feed::TensorExpectation,
    ) -> Option<u32> {
        use trueos_fpga_abi::lfm25::TensorFormat;

        match tensor.format {
            TensorFormat::Bf16Le => tensor.ggml_ne0.checked_mul(tensor.ggml_ne1)?.checked_mul(2),
            TensorFormat::Q8_0 => {
                if !tensor
                    .ggml_ne0
                    .is_multiple_of(lfm25::Q8_0_BLOCK_VALUES as u32)
                {
                    return None;
                }
                tensor
                    .ggml_ne0
                    .checked_div(lfm25::Q8_0_BLOCK_VALUES as u32)?
                    .checked_mul(lfm25::Q8_0_BLOCK_BYTES as u32)?
                    .checked_mul(tensor.ggml_ne1)
            }
        }
    }

    async fn feed_sequence(
        &mut self,
        session: &mut super::fpga_offload::Lfm25DecodeTransportSession,
        capability: FeedCapability,
        request: FeedRequest,
    ) -> Result<u32, KernelDecodeDataPlaneError> {
        use trueos_fpga_abi::lfm25_decode::tensor_descriptor;
        use trueos_fpga_abi::lfm25_decode_feed::{FeedSequenceValidator, FeedState};

        let mut validator = FeedSequenceValidator::begin(capability, request)
            .map_err(KernelDecodeDataPlaneError::Feed)?;
        while !validator.is_complete() {
            let payload_count =
                request.mode.shape().stages_per_item as usize * request.mode.shape().lanes as usize;
            let mut stages = Vec::new();
            stages
                .try_reserve_exact(payload_count)
                .map_err(|_| KernelDecodeDataPlaneError::BufferUnavailable)?;

            while !validator.staging_complete() {
                let staged = validator
                    .expected_stage()
                    .map_err(KernelDecodeDataPlaneError::Feed)?;
                let source = validator
                    .expected_source()
                    .map_err(KernelDecodeDataPlaneError::Feed)?;
                if source.payload_bytes != staged.payload_bytes {
                    return Err(KernelDecodeDataPlaneError::TensorRange);
                }
                let tensor = tensor_descriptor(request.layer, source.tensor.role)
                    .ok_or(KernelDecodeDataPlaneError::TensorRange)?;
                source
                    .tensor
                    .validate(tensor, request.layer)
                    .map_err(KernelDecodeDataPlaneError::Feed)?;
                if tensor.native_bytes
                    != Self::expected_tensor_bytes(source.tensor)
                        .ok_or(KernelDecodeDataPlaneError::TensorRange)?
                    || tensor.native_offset as usize % lfm25::MODEL_TENSOR_ALIGNMENT != 0
                {
                    return Err(KernelDecodeDataPlaneError::TensorRange);
                }
                let relative_end = source
                    .relative_byte_offset
                    .checked_add(source.payload_bytes as u32)
                    .ok_or(KernelDecodeDataPlaneError::TensorRange)?;
                let absolute_offset = tensor
                    .native_offset
                    .checked_add(source.relative_byte_offset)
                    .ok_or(KernelDecodeDataPlaneError::TensorRange)?;
                let absolute_end = absolute_offset
                    .checked_add(source.payload_bytes as u32)
                    .ok_or(KernelDecodeDataPlaneError::TensorRange)?;
                if relative_end > tensor.native_bytes
                    || absolute_end as u64 > self.image.len()
                    || source.payload_bytes as usize
                        > super::fpga_offload::LFM25_FEED_MAX_PAYLOAD_BYTES
                {
                    return Err(KernelDecodeDataPlaneError::TensorRange);
                }

                let mut payload = [0u8; super::fpga_offload::LFM25_FEED_MAX_PAYLOAD_BYTES];
                let payload = &mut payload[..source.payload_bytes as usize];
                self.read_stage_payload(staged.bank, absolute_offset as u64, payload)
                    .await?;
                stages.push(
                    super::fpga_offload::Lfm25FeedStage::new(staged, payload)
                        .map_err(KernelDecodeDataPlaneError::Transport)?,
                );
                validator
                    .stage(staged)
                    .map_err(KernelDecodeDataPlaneError::Feed)?;
            }

            let record = validator
                .expected_commit()
                .map_err(KernelDecodeDataPlaneError::Feed)?;
            let status = session
                .commit_feed_item(record, stages)
                .await
                .map_err(KernelDecodeDataPlaneError::Transport)?;
            if status.state != FeedState::Complete
                || status.error_code != 0
                || !status.identity_matches(record)
            {
                return Err(KernelDecodeDataPlaneError::Retirement);
            }
            validator
                .commit(record)
                .map_err(KernelDecodeDataPlaneError::Feed)?;
        }
        Ok(validator.committed_units())
    }
}

#[cfg(target_os = "trueos")]
impl DecodeModelDataPlane for KernelDecodeModelDataPlane {
    type Session = super::fpga_offload::Lfm25DecodeTransportSession;
    type Error = KernelDecodeDataPlaneError;

    fn published_feed_capability(&self) -> Option<FeedCapability> {
        super::fpga_offload::lfm25_feed_capability().ok()
    }

    fn sealed_model_payloads_ready(&self) -> bool {
        self.verified_sha256 == super::lfm25_model::NATIVE_IMAGE_SHA256
            && self.image.len() == super::lfm25_model::NATIVE_IMAGE_BYTES
            && super::fpga_offload::lfm25_feed_transport_available()
    }

    fn max_context_positions(&self) -> u32 {
        lfm25::MODEL_INITIAL_CONTEXT
    }

    async fn prepare_operation<'a>(
        &'a mut self,
        session: &'a mut Self::Session,
        request: DecodeDataPlaneRequest,
    ) -> Result<DecodeDataPlaneReceipt, Self::Error> {
        let observed_domain = DecodeTensorDomain {
            connection_generation: session.connection_generation(),
            session_epoch: session.session_epoch(),
        };
        if observed_domain != request.domain || session.feed_is_poisoned() {
            return Err(KernelDecodeDataPlaneError::SessionDomain);
        }
        let capability = self
            .published_feed_capability()
            .filter(|capability| capability_is_exact(*capability))
            .ok_or(KernelDecodeDataPlaneError::Capability)?;

        let mut committed_units = [0u32; MAX_FEED_SEQUENCES_PER_OPERATION];
        for (index, feed) in request.feeds.iter().copied().enumerate() {
            if let Some(feed) = feed {
                committed_units[index] = self.feed_sequence(session, capability, feed).await?;
            }
        }
        Ok(DecodeDataPlaneReceipt {
            request,
            committed_units,
        })
    }
}

/// Concrete production kernel backend: one verified native-image handle, one
/// TGD1/TGF2 session, one BAR2 lane, and one MSI-driven offload worker.
#[cfg(target_os = "trueos")]
pub type KernelTruegaAotDecodeBackend =
    TruegaAotDecodeBackend<KernelDecodeCommandTransport, KernelDecodeModelDataPlane>;

#[cfg(target_os = "trueos")]
pub async fn open_kernel_backend()
-> Result<KernelTruegaAotDecodeBackend, KernelDecodeDataPlaneError> {
    let data_plane = KernelDecodeModelDataPlane::open_verified().await?;
    Ok(TruegaAotDecodeBackend::new(KernelDecodeCommandTransport, data_plane))
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::vec::Vec;
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use super::super::lfm25_decode::{DecodeSession, HiddenQ30};
    use super::*;
    use trueos_fpga_abi::lfm25_decode_feed::{FeedSequenceValidator, REQUIRED_CAPABILITY};

    const DOMAIN: DecodeTensorDomain = DecodeTensorDomain {
        connection_generation: 7,
        session_epoch: 11,
    };
    const FULL_SCORE: i64 = i64::MIN + 0x1234_5678;

    struct FakeSession {
        commands: Vec<Command>,
        next_slot: u8,
    }

    struct FakeTransport {
        exact: bool,
        acquisitions: u32,
        execute_error: bool,
    }

    impl DecodeCommandTransport for FakeTransport {
        type Session = FakeSession;
        type Error = &'static str;

        fn exact_capability_available(&self) -> bool {
            self.exact
        }

        async fn acquire(&mut self) -> Result<Self::Session, Self::Error> {
            self.acquisitions += 1;
            Ok(FakeSession {
                commands: Vec::new(),
                next_slot: 0,
            })
        }

        fn domain(_session: &Self::Session) -> DecodeTensorDomain {
            DOMAIN
        }

        async fn execute<'a>(
            &'a mut self,
            session: &'a mut Self::Session,
            command: Command,
        ) -> Result<Completion, Self::Error> {
            if self.execute_error {
                return Err("execute");
            }
            session.commands.push(command);
            if command.operation == DecodeOpKind::TiedLmHeadArgmax {
                Ok(Completion::Argmax {
                    token: 9,
                    score_q30: FULL_SCORE,
                    rows: lfm25::MODEL_VOCABULARY_SIZE,
                })
            } else {
                let slot = session.next_slot;
                session.next_slot += 1;
                Ok(Completion::Resident {
                    storage_slot: slot,
                    position: command.position,
                })
            }
        }
    }

    struct FakeDataPlane {
        feed_capability: Option<FeedCapability>,
        model_ready: bool,
        fail: bool,
        bad_receipt: bool,
        prepared: Vec<DecodeDataPlaneRequest>,
        commands_before_prepare: Vec<usize>,
    }

    impl DecodeModelDataPlane for FakeDataPlane {
        type Session = FakeSession;
        type Error = &'static str;

        fn published_feed_capability(&self) -> Option<FeedCapability> {
            self.feed_capability
        }

        fn sealed_model_payloads_ready(&self) -> bool {
            self.model_ready
        }

        fn max_context_positions(&self) -> u32 {
            2
        }

        async fn prepare_operation<'a>(
            &'a mut self,
            session: &'a mut Self::Session,
            request: DecodeDataPlaneRequest,
        ) -> Result<DecodeDataPlaneReceipt, Self::Error> {
            self.commands_before_prepare.push(session.commands.len());
            self.prepared.push(request);
            if self.fail {
                return Err("prepare");
            }
            for feed in request.feeds.iter().flatten() {
                FeedSequenceValidator::begin(REQUIRED_CAPABILITY, *feed)
                    .map_err(|_| "feed-begin")?;
            }
            let mut receipt = DecodeDataPlaneReceipt::complete(request);
            if self.bad_receipt {
                receipt.committed_units[0] ^= 1;
            }
            Ok(receipt)
        }
    }

    fn backend(
        exact_tgd1: bool,
        exact_tgf2: bool,
        model_ready: bool,
    ) -> TruegaAotDecodeBackend<FakeTransport, FakeDataPlane> {
        let feed_capability = if exact_tgf2 {
            Some(REQUIRED_CAPABILITY)
        } else {
            let mut mismatched = REQUIRED_CAPABILITY;
            mismatched.capability_bits ^= 1 << 31;
            Some(mismatched)
        };
        TruegaAotDecodeBackend::new(
            FakeTransport {
                exact: exact_tgd1,
                acquisitions: 0,
                execute_error: false,
            },
            FakeDataPlane {
                feed_capability,
                model_ready,
                fail: false,
                bad_receipt: false,
                prepared: Vec::new(),
                commands_before_prepare: Vec::new(),
            },
        )
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
            Poll::Pending => panic!("immediate fake unexpectedly parked"),
        }
    }

    fn take_error<T, E>(result: Result<T, E>) -> E {
        match result {
            Ok(_) => panic!("expected error"),
            Err(error) => error,
        }
    }

    #[test]
    fn one_position_prepares_and_maps_all_99_ops_and_preserves_i64_argmax() {
        let mut backend = backend(true, true, true);
        let mut scheduler = DecodeSession::new();
        let first = ready(scheduler.decode_token(&mut backend, 1)).unwrap();
        assert_eq!(first.score_q30, FULL_SCORE);
        assert_eq!(first.callback_sequence, OPS_PER_TOKEN as u64);
        assert_eq!((backend.position(), backend.next_ordinal()), (1, 0));
        assert_eq!(backend.max_context_positions(), 1);

        let (transport, data_plane, session) = backend.into_parts();
        assert_eq!(transport.acquisitions, 1);
        assert_eq!(data_plane.prepared.len(), OPS_PER_TOKEN);
        assert_eq!(data_plane.commands_before_prepare, (0..OPS_PER_TOKEN).collect::<Vec<_>>());

        let commands = session.unwrap().commands;
        assert_eq!(commands.len(), OPS_PER_TOKEN);
        let expected: Vec<_> = DecodePlan::new().map(|step| step.kind).collect();
        assert_eq!(
            commands
                .iter()
                .map(|command| command.operation)
                .collect::<Vec<_>>(),
            expected
        );
        for (ordinal, (prepared, command)) in
            data_plane.prepared.iter().zip(commands.iter()).enumerate()
        {
            assert_eq!(prepared.domain, DOMAIN);
            assert_eq!(prepared.ordinal, ordinal as u8);
            assert_eq!(prepared.command, *command);
            assert_eq!(command.position, 0);
            assert_eq!(command.session_epoch, DOMAIN.session_epoch);
            assert!(command.validate().is_ok());

            let modes: Vec<_> = prepared
                .feeds
                .iter()
                .flatten()
                .map(|feed| feed.mode)
                .collect();
            let expected_modes: &[FeedMode] = match command.operation {
                DecodeOpKind::TokenEmbedding => &[FeedMode::EmbeddingQ8Row],
                DecodeOpKind::OperatorRmsNorm => &[FeedMode::OperatorRmsNormWeights],
                DecodeOpKind::ShortConv => &[
                    FeedMode::ShortConvCoefficients,
                    FeedMode::ShortConvInputTripletRows,
                    FeedMode::ShortConvOutputRows,
                ],
                DecodeOpKind::Attention => &[
                    FeedMode::AttentionQkNormWeights,
                    FeedMode::AttentionQueryRows,
                    FeedMode::AttentionKeyRows,
                    FeedMode::AttentionValueRows,
                    FeedMode::AttentionFirstTokenCore,
                    FeedMode::AttentionOutputRows,
                ],
                DecodeOpKind::OperatorResidual | DecodeOpKind::FfnResidual => &[],
                DecodeOpKind::FfnRmsNorm => &[FeedMode::FfnRmsNormWeights],
                DecodeOpKind::Ffn => &[FeedMode::FfnGateUpRows, FeedMode::FfnDownRows],
                DecodeOpKind::FinalRmsNorm => &[FeedMode::FinalRmsNormWeights],
                DecodeOpKind::TiedLmHeadArgmax => &[FeedMode::TiedLmHeadRows],
            };
            assert_eq!(modes, expected_modes);
            assert!(
                prepared
                    .feeds
                    .iter()
                    .flatten()
                    .all(|feed| feed.validate().is_ok())
            );
        }
        assert_eq!(data_plane.prepared[0].embedding_row, Some(EmbeddingRowPlan::new(1).unwrap()));
        assert_eq!(data_plane.prepared[0].feeds[0].unwrap().token, Some(1));
        assert!(commands.iter().any(|command| {
            matches!(command.operation, DecodeOpKind::OperatorResidual | DecodeOpKind::FfnResidual)
                && command.input_slot.is_some()
                && command.residual_slot.is_some()
        }));
    }

    #[test]
    fn exact_tgd1_tgf2_and_model_ready_are_required_before_lazy_acquire() {
        for (exact_tgd1, exact_tgf2, model_ready) in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
            (false, false, false),
        ] {
            let mut backend = backend(exact_tgd1, exact_tgf2, model_ready);
            assert_eq!(backend.capabilities(), DecodeCapabilities::NONE);
            let request = AotDecodeRequest::TokenEmbedding {
                row: EmbeddingRowPlan::new(1).unwrap(),
            };
            let error = take_error(ready(backend.submit(request)));
            assert_eq!(error, TruegaDecodeBackendError::Unavailable);
            assert_eq!(backend.transport.acquisitions, 0);
            assert!(backend.data_plane.prepared.is_empty());
        }
    }

    #[test]
    fn stale_generation_epoch_or_wide_slot_never_reaches_transport() {
        let mut backend = backend(true, true, true);
        let embedding = ready(backend.submit(AotDecodeRequest::TokenEmbedding {
            row: EmbeddingRowPlan::new(1).unwrap(),
        }))
        .unwrap();
        assert!(matches!(embedding.output, AotDecodeOutput::HiddenQ30(_)));

        let stale = HiddenQ30::from_resident(ResidentTensorHandle::new(8, 11, 0));
        let error = take_error(ready(backend.submit(AotDecodeRequest::OperatorRmsNorm {
            layer: 0,
            input: stale,
        })));
        assert!(matches!(error, TruegaDecodeBackendError::ResidentDomain { .. }));
        assert_eq!(backend.session.as_ref().unwrap().commands.len(), 1);
        assert_eq!(backend.data_plane.prepared.len(), 1);

        let wide = HiddenQ30::from_resident(ResidentTensorHandle::new(7, 11, 255));
        let error = take_error(ready(backend.submit(AotDecodeRequest::OperatorRmsNorm {
            layer: 0,
            input: wide,
        })));
        assert_eq!(error, TruegaDecodeBackendError::ResidentSlot(255));
        assert_eq!(backend.session.as_ref().unwrap().commands.len(), 1);
        assert_eq!(backend.data_plane.prepared.len(), 1);
    }

    #[test]
    fn missing_data_plane_receipt_poisoned_without_issuing_a_command() {
        let mut backend = backend(true, true, true);
        backend.data_plane.fail = true;
        let error = take_error(ready(backend.submit(AotDecodeRequest::TokenEmbedding {
            row: EmbeddingRowPlan::new(1).unwrap(),
        })));
        assert_eq!(error, TruegaDecodeBackendError::DataPlane("prepare"));
        assert!(backend.is_poisoned());
        assert!(backend.session.as_ref().unwrap().commands.is_empty());
        assert_eq!(backend.capabilities(), DecodeCapabilities::NONE);
    }

    #[test]
    fn mismatched_data_plane_receipt_poisoned_without_issuing_a_command() {
        let mut backend = backend(true, true, true);
        backend.data_plane.bad_receipt = true;
        let error = take_error(ready(backend.submit(AotDecodeRequest::TokenEmbedding {
            row: EmbeddingRowPlan::new(1).unwrap(),
        })));
        assert_eq!(error, TruegaDecodeBackendError::DataPlaneReceipt);
        assert!(backend.is_poisoned());
        assert!(backend.session.as_ref().unwrap().commands.is_empty());
        assert_eq!(backend.data_plane.prepared.len(), 1);
    }
}
