//! Versioned fixed feed contract for the resident LFM2.5 decode engines.
//!
//! This extends the physical three-bank BAR2 staging convention used by the proven
//! layer-0 FFN v1 image. It does not change any v1 offset, magic, or mode. A future
//! endpoint must separately publish the exact [`REQUIRED_CAPABILITY`] before TRUEOS may
//! use this contract.
//!
//! Every [`FeedMode`] names one ahead-of-time circuit and has immutable model shapes.
//! Payload bytes are written to one or more fixed BAR2 banks first; the final dword of a
//! [`FeedCommitRecord`] publishes one complete row/vector/item. This is not bytecode, a graph, a
//! dynamic-shape protocol, or evidence that the current `TGD1` endpoint implements it.

use core::mem::{align_of, size_of};

use super::lfm25::{self, LayerKind, NativeTensorDescriptor, TensorFormat, TensorRole};

pub const FEED_ABI_VERSION: u16 = 2;
pub const FEED_CAPABILITY_MAGIC: u32 = 0x3246_4754; // "TGF2"
pub const FEED_RECORD_MAGIC: u32 = 0x3244_4654; // "TFD2"
pub const FEED_COMMIT_MAGIC: u32 = 0x324D_4346; // "FCM2"
pub const FEED_COMMIT_RECORD_BYTES: u16 = 64;
pub const FEED_COMMIT_BAR2_OFFSET: usize = 0x7_F000;
pub const FEED_STAGING_SLOTS: u16 = 144;
/// Read-only TGF2 publication starts immediately after the existing 0x200..0x27f
/// firmware manifest. BAR2 remains a write-only staging/commit aperture.
pub const BAR0_FEED_CAPABILITY_MAGIC_OFFSET: usize = 0x280;
pub const BAR0_FEED_VERSION_RECORD_BYTES_OFFSET: usize = 0x284;
pub const BAR0_FEED_CAPABILITY_BITS_OFFSET: usize = 0x288;
pub const BAR0_FEED_MODEL_GENERATION_OFFSET: usize = 0x28C;
pub const BAR0_FEED_SHAPE_SET_TAG_OFFSET: usize = 0x290;
pub const BAR0_FEED_CAPABILITY_BYTES: usize = 5 * 4;
pub const BAR0_FEED_CAPABILITY_REQUIRED_BYTES: usize =
    BAR0_FEED_CAPABILITY_MAGIC_OFFSET + BAR0_FEED_CAPABILITY_BYTES;
/// The endpoint accepts a commit only from IDLE, then publishes BUSY. A
/// terminal envelope remains owned until the shared IRQ is acknowledged; the
/// host's following non-posted status read flushes that acknowledgement before
/// another commit can be published.
/// Retirement publication order is identity/error, incremented completion
/// count, terminal state, then the shared IRQ request/pending bit. The host
/// consumes a matching status snapshot before acknowledging that shared IRQ.
pub const BAR0_FEED_STATE_OFFSET: usize = 0x294;
pub const BAR0_FEED_RETIRED_MODE_LAYER_OFFSET: usize = 0x298;
pub const BAR0_FEED_RETIRED_SESSION_EPOCH_OFFSET: usize = 0x29C;
pub const BAR0_FEED_RETIRED_SEQUENCE_OFFSET: usize = 0x2A0;
pub const BAR0_FEED_RETIRED_ITEM_OFFSET: usize = 0x2A4;
pub const BAR0_FEED_ERROR_CODE_OFFSET: usize = 0x2A8;
pub const BAR0_FEED_COMPLETION_COUNT_OFFSET: usize = 0x2AC;
pub const BAR0_FEED_CONTROL_OFFSET: usize = 0x2B0;
pub const BAR0_FEED_REQUIRED_BYTES: usize = BAR0_FEED_CONTROL_OFFSET + 4;
pub const FEED_RESET_MAGIC: u32 = 0x3254_5352; // "RST2"
/// A reset write is synchronous with a subsequent non-posted BAR0 read: before
/// that read completes, status is IDLE, error is zero, and shared IRQ pending is
/// clear. Hosts reject the endpoint if this postcondition is not observed.
pub const FEED_ERROR_NONE: u32 = 0;
/// The fixed frontend rejected staging/commit ordering and requires an explicit reset.
pub const FEED_ERROR_FRONTEND_POISON: u32 = 0xBAD4_0001;
/// TGF2 shares the one physical completion bridge with inline, row-stream, and
/// TGD1 jobs. There is deliberately no second interrupt fabric.
pub const BAR0_FEED_SHARED_IRQ_ACK_OFFSET: usize = super::BAR0_CALL_IRQ_ACK_OFFSET;
pub const BAR0_FEED_SHARED_IRQ_STATE_OFFSET: usize = super::BAR0_CALL_IRQ_STATE_OFFSET;
pub const FEED_SHARED_IRQ_PENDING_BIT: u32 = 1 << 0;
pub const TGA_BAR0_APERTURE_BYTES: usize = 0x400;
pub const FEED_NO_LAYER: u8 = u8::MAX;
pub const FEED_NO_TOKEN: u32 = u32::MAX;
pub const FEED_NO_STAGE_SLOT: u16 = u16::MAX;

pub const CAP_EXPLICIT_STAGE_COMMIT: u32 = 1 << 0;
pub const CAP_TAGGED_SEQUENCE: u32 = 1 << 1;
pub const CAP_EMBEDDING: u32 = 1 << 2;
pub const CAP_RMSNORM: u32 = 1 << 3;
pub const CAP_SHORTCONV: u32 = 1 << 4;
pub const CAP_ATTENTION_FIRST_TOKEN: u32 = 1 << 5;
pub const CAP_FFN: u32 = 1 << 6;
pub const CAP_TIED_LM_HEAD: u32 = 1 << 7;
pub const CAP_SHARED_MSI_RETIREMENT: u32 = 1 << 8;
pub const REQUIRED_CAPABILITY_BITS: u32 = CAP_EXPLICIT_STAGE_COMMIT
    | CAP_TAGGED_SEQUENCE
    | CAP_EMBEDDING
    | CAP_RMSNORM
    | CAP_SHORTCONV
    | CAP_ATTENTION_FIRST_TOKEN
    | CAP_FFN
    | CAP_TIED_LM_HEAD
    | CAP_SHARED_MSI_RETIREMENT;

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FeedState {
    Idle = 0,
    Busy = 1,
    Complete = 2,
    Failed = 3,
    Poisoned = 4,
}

impl FeedState {
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::Busy),
            2 => Some(Self::Complete),
            3 => Some(Self::Failed),
            4 => Some(Self::Poisoned),
            _ => None,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Failed | Self::Poisoned)
    }
}

/// Exact v2 publication. Equality is intentional: unknown bits or shape tags fail closed.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FeedCapability {
    pub magic: u32,
    pub abi_version: u16,
    pub commit_record_bytes: u16,
    pub capability_bits: u32,
    pub model_generation: u32,
    pub shape_set_tag: u32,
}

pub const REQUIRED_CAPABILITY: FeedCapability = FeedCapability {
    magic: FEED_CAPABILITY_MAGIC,
    abi_version: FEED_ABI_VERSION,
    commit_record_bytes: FEED_COMMIT_RECORD_BYTES,
    capability_bits: REQUIRED_CAPABILITY_BITS,
    model_generation: lfm25::MODEL_GENERATION,
    shape_set_tag: fixed_shape_set_tag(),
};

pub const fn capability_is_exact(observed: FeedCapability) -> bool {
    observed.magic == REQUIRED_CAPABILITY.magic
        && observed.abi_version == REQUIRED_CAPABILITY.abi_version
        && observed.commit_record_bytes == REQUIRED_CAPABILITY.commit_record_bytes
        && observed.capability_bits == REQUIRED_CAPABILITY.capability_bits
        && observed.model_generation == REQUIRED_CAPABILITY.model_generation
        && observed.shape_set_tag == REQUIRED_CAPABILITY.shape_set_tag
}

impl FeedCapability {
    /// Five dwords published read-only through BAR0. Version occupies bits 15:0 and
    /// commit-record bytes occupy bits 31:16 of the second word.
    pub const fn bar0_words(self) -> [u32; 5] {
        [
            self.magic,
            self.abi_version as u32 | ((self.commit_record_bytes as u32) << 16),
            self.capability_bits,
            self.model_generation,
            self.shape_set_tag,
        ]
    }

    pub const fn from_bar0_words(words: [u32; 5]) -> Self {
        Self {
            magic: words[0],
            abi_version: words[1] as u16,
            commit_record_bytes: (words[1] >> 16) as u16,
            capability_bits: words[2],
            model_generation: words[3],
            shape_set_tag: words[4],
        }
    }
}

/// The existing BAR2 offsets retain their v1 meaning. v2 treats them as three generic
/// staging lanes selected by each fixed mode.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StageBank {
    Bank0 = 0,
    Bank1 = 1,
    Bank2 = 2,
}

impl StageBank {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Bank0),
            1 => Some(Self::Bank1),
            2 => Some(Self::Bank2),
            _ => None,
        }
    }

    pub const fn bar2_offset(self) -> usize {
        match self {
            Self::Bank0 => super::BAR2_LFM25_STREAM_ACTIVATION_OFFSET,
            Self::Bank1 => super::BAR2_LFM25_STREAM_WEIGHT0_OFFSET,
            Self::Bank2 => super::BAR2_LFM25_STREAM_WEIGHT1_OFFSET,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FeedPayloadFormat {
    None = 0,
    Bf16 = 1,
    Bf16x3 = 2,
    GgmlQ8_0Block = 3,
}

impl FeedPayloadFormat {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::None),
            1 => Some(Self::Bf16),
            2 => Some(Self::Bf16x3),
            3 => Some(Self::GgmlQ8_0Block),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FeedMode {
    EmbeddingQ8Row = 0,
    OperatorRmsNormWeights = 1,
    FfnRmsNormWeights = 2,
    FinalRmsNormWeights = 3,
    ShortConvCoefficients = 4,
    ShortConvInputTripletRows = 5,
    ShortConvOutputRows = 6,
    AttentionQkNormWeights = 7,
    AttentionQueryRows = 8,
    AttentionKeyRows = 9,
    AttentionValueRows = 10,
    AttentionFirstTokenCore = 11,
    AttentionOutputRows = 12,
    FfnGateUpRows = 13,
    FfnDownRows = 14,
    TiedLmHeadRows = 15,
}

pub const ALL_FEED_MODES: [FeedMode; 16] = [
    FeedMode::EmbeddingQ8Row,
    FeedMode::OperatorRmsNormWeights,
    FeedMode::FfnRmsNormWeights,
    FeedMode::FinalRmsNormWeights,
    FeedMode::ShortConvCoefficients,
    FeedMode::ShortConvInputTripletRows,
    FeedMode::ShortConvOutputRows,
    FeedMode::AttentionQkNormWeights,
    FeedMode::AttentionQueryRows,
    FeedMode::AttentionKeyRows,
    FeedMode::AttentionValueRows,
    FeedMode::AttentionFirstTokenCore,
    FeedMode::AttentionOutputRows,
    FeedMode::FfnGateUpRows,
    FeedMode::FfnDownRows,
    FeedMode::TiedLmHeadRows,
];

impl FeedMode {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::EmbeddingQ8Row),
            1 => Some(Self::OperatorRmsNormWeights),
            2 => Some(Self::FfnRmsNormWeights),
            3 => Some(Self::FinalRmsNormWeights),
            4 => Some(Self::ShortConvCoefficients),
            5 => Some(Self::ShortConvInputTripletRows),
            6 => Some(Self::ShortConvOutputRows),
            7 => Some(Self::AttentionQkNormWeights),
            8 => Some(Self::AttentionQueryRows),
            9 => Some(Self::AttentionKeyRows),
            10 => Some(Self::AttentionValueRows),
            11 => Some(Self::AttentionFirstTokenCore),
            12 => Some(Self::AttentionOutputRows),
            13 => Some(Self::FfnGateUpRows),
            14 => Some(Self::FfnDownRows),
            15 => Some(Self::TiedLmHeadRows),
            _ => None,
        }
    }

    pub const fn shape(self) -> FixedFeedShape {
        use FeedPayloadFormat::{Bf16, Bf16x3, GgmlQ8_0Block, None};
        match self {
            Self::EmbeddingQ8Row => FixedFeedShape::new(self, 1, 32, 1, GgmlQ8_0Block, 34),
            Self::OperatorRmsNormWeights | Self::FfnRmsNormWeights | Self::FinalRmsNormWeights => {
                FixedFeedShape::new(self, 1, 32, 1, Bf16, 64)
            }
            Self::ShortConvCoefficients => FixedFeedShape::new(self, 1, 96, 1, Bf16x3, 64),
            Self::ShortConvInputTripletRows => {
                FixedFeedShape::new(self, lfm25::MODEL_HIDDEN_SIZE, 32, 3, GgmlQ8_0Block, 34)
            }
            Self::ShortConvOutputRows => {
                FixedFeedShape::new(self, lfm25::MODEL_HIDDEN_SIZE, 32, 1, GgmlQ8_0Block, 34)
            }
            Self::AttentionQkNormWeights => FixedFeedShape::new(self, 1, 2, 2, Bf16, 64),
            Self::AttentionQueryRows => FixedFeedShape::new(self, 1024, 32, 1, GgmlQ8_0Block, 34),
            Self::AttentionKeyRows | Self::AttentionValueRows => {
                FixedFeedShape::new(self, 512, 32, 1, GgmlQ8_0Block, 34)
            }
            Self::AttentionFirstTokenCore => FixedFeedShape::new(self, 1, 0, 0, None, 0),
            Self::AttentionOutputRows => FixedFeedShape::new(self, 1024, 32, 1, GgmlQ8_0Block, 34),
            Self::FfnGateUpRows => {
                FixedFeedShape::new(self, lfm25::MODEL_FEED_FORWARD_SIZE, 32, 2, GgmlQ8_0Block, 34)
            }
            Self::FfnDownRows => {
                FixedFeedShape::new(self, lfm25::MODEL_HIDDEN_SIZE, 144, 1, GgmlQ8_0Block, 34)
            }
            Self::TiedLmHeadRows => {
                FixedFeedShape::new(self, lfm25::MODEL_VOCABULARY_SIZE, 32, 1, GgmlQ8_0Block, 34)
            }
        }
    }

    const fn domain(self) -> LayerDomain {
        match self {
            Self::EmbeddingQ8Row | Self::FinalRmsNormWeights | Self::TiedLmHeadRows => {
                LayerDomain::Global
            }
            Self::OperatorRmsNormWeights
            | Self::FfnRmsNormWeights
            | Self::FfnGateUpRows
            | Self::FfnDownRows => LayerDomain::Any,
            Self::ShortConvCoefficients
            | Self::ShortConvInputTripletRows
            | Self::ShortConvOutputRows => LayerDomain::ShortConv,
            Self::AttentionQkNormWeights
            | Self::AttentionQueryRows
            | Self::AttentionKeyRows
            | Self::AttentionValueRows
            | Self::AttentionFirstTokenCore
            | Self::AttentionOutputRows => LayerDomain::Attention,
        }
    }

    const fn first_token_only(self) -> bool {
        matches!(
            self,
            Self::AttentionQkNormWeights
                | Self::AttentionQueryRows
                | Self::AttentionKeyRows
                | Self::AttentionValueRows
                | Self::AttentionFirstTokenCore
                | Self::AttentionOutputRows
        )
    }

    /// Exact sealed-model tensor consumed by one lane. The control-only attention core
    /// has no tensor lane.
    pub const fn tensor_expectation(self, lane: u8) -> Option<TensorExpectation> {
        use TensorRole::*;
        let (role, format, ne0, ne1) = match self {
            Self::EmbeddingQ8Row | Self::TiedLmHeadRows if lane == 0 => (
                TokenEmbedding,
                TensorFormat::Q8_0,
                lfm25::MODEL_HIDDEN_SIZE,
                lfm25::MODEL_VOCABULARY_SIZE,
            ),
            Self::OperatorRmsNormWeights if lane == 0 => {
                (OperatorNorm, TensorFormat::Bf16Le, lfm25::MODEL_HIDDEN_SIZE, 1)
            }
            Self::FfnRmsNormWeights if lane == 0 => {
                (FfnNorm, TensorFormat::Bf16Le, lfm25::MODEL_HIDDEN_SIZE, 1)
            }
            Self::FinalRmsNormWeights if lane == 0 => {
                (TokenEmbeddingNorm, TensorFormat::Bf16Le, lfm25::MODEL_HIDDEN_SIZE, 1)
            }
            Self::ShortConvCoefficients if lane == 0 => (
                ShortConvKernel,
                TensorFormat::Bf16Le,
                lfm25::MODEL_SHORTCONV_CACHE as u32,
                lfm25::MODEL_HIDDEN_SIZE,
            ),
            Self::ShortConvInputTripletRows if lane < 3 => (
                ShortConvInput,
                TensorFormat::Q8_0,
                lfm25::MODEL_HIDDEN_SIZE,
                lfm25::MODEL_HIDDEN_SIZE * 3,
            ),
            Self::ShortConvOutputRows if lane == 0 => (
                ShortConvOutput,
                TensorFormat::Q8_0,
                lfm25::MODEL_HIDDEN_SIZE,
                lfm25::MODEL_HIDDEN_SIZE,
            ),
            Self::AttentionQkNormWeights if lane == 0 => {
                (QueryNorm, TensorFormat::Bf16Le, lfm25::MODEL_HEAD_DIMENSION as u32, 1)
            }
            Self::AttentionQkNormWeights if lane == 1 => {
                (KeyNorm, TensorFormat::Bf16Le, lfm25::MODEL_HEAD_DIMENSION as u32, 1)
            }
            Self::AttentionQueryRows if lane == 0 => {
                (Query, TensorFormat::Q8_0, lfm25::MODEL_HIDDEN_SIZE, lfm25::MODEL_HIDDEN_SIZE)
            }
            Self::AttentionKeyRows if lane == 0 => {
                (Key, TensorFormat::Q8_0, lfm25::MODEL_HIDDEN_SIZE, 512)
            }
            Self::AttentionValueRows if lane == 0 => {
                (Value, TensorFormat::Q8_0, lfm25::MODEL_HIDDEN_SIZE, 512)
            }
            Self::AttentionOutputRows if lane == 0 => (
                AttentionOutput,
                TensorFormat::Q8_0,
                lfm25::MODEL_HIDDEN_SIZE,
                lfm25::MODEL_HIDDEN_SIZE,
            ),
            Self::FfnGateUpRows if lane == 0 => (
                FfnGate,
                TensorFormat::Q8_0,
                lfm25::MODEL_HIDDEN_SIZE,
                lfm25::MODEL_FEED_FORWARD_SIZE,
            ),
            Self::FfnGateUpRows if lane == 1 => (
                FfnUp,
                TensorFormat::Q8_0,
                lfm25::MODEL_HIDDEN_SIZE,
                lfm25::MODEL_FEED_FORWARD_SIZE,
            ),
            Self::FfnDownRows if lane == 0 => (
                FfnDown,
                TensorFormat::Q8_0,
                lfm25::MODEL_FEED_FORWARD_SIZE,
                lfm25::MODEL_HIDDEN_SIZE,
            ),
            _ => return None,
        };
        Some(TensorExpectation {
            role,
            format,
            ggml_ne0: ne0,
            ggml_ne1: ne1,
        })
    }

    /// Map one fixed feed item/lane to the corresponding sealed tensor item. For the
    /// shortconv input matrix, B/C/X lanes are the three consecutive 1,024-row regions.
    pub const fn tensor_item(self, item: u32, lane: u8, token: Option<u32>) -> Option<u32> {
        let shape = self.shape();
        if item >= shape.items || lane >= shape.lanes {
            return None;
        }
        match self {
            Self::EmbeddingQ8Row => match token {
                Some(token) if token < lfm25::MODEL_VOCABULARY_SIZE => Some(token),
                _ => None,
            },
            Self::ShortConvInputTripletRows => Some(lane as u32 * lfm25::MODEL_HIDDEN_SIZE + item),
            Self::AttentionFirstTokenCore => None,
            _ => Some(item),
        }
    }
}

/// Exact packed value published at [`BAR0_FEED_RETIRED_MODE_LAYER_OFFSET`].
/// The upper half remains zero so future widening fails closed in old hosts.
pub const fn retired_mode_layer_word(mode: FeedMode, layer: Option<u8>) -> u32 {
    let encoded_layer = match layer {
        Some(layer) => layer,
        None => FEED_NO_LAYER,
    };
    mode as u32 | ((encoded_layer as u32) << 8)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FeedRetirementStatus {
    pub state: FeedState,
    pub retired_mode: FeedMode,
    pub retired_layer: Option<u8>,
    pub retired_session_epoch: u32,
    pub retired_sequence: u32,
    pub retired_item: u32,
    pub error_code: i32,
    pub completion_count: u32,
}

impl FeedRetirementStatus {
    /// Decode the seven read-only TGF2 status dwords in BAR0 offset order.
    pub fn from_bar0_words(words: [u32; 7]) -> Result<Self, FeedError> {
        let state = FeedState::from_raw(words[0]).ok_or(FeedError::InvalidRetirement)?;
        if words[1] & 0xFFFF_0000 != 0 {
            return Err(FeedError::InvalidRetirement);
        }
        let retired_mode =
            FeedMode::from_raw(words[1] as u8).ok_or(FeedError::InvalidRetirement)?;
        let raw_layer = (words[1] >> 8) as u8;
        let retired_layer = if raw_layer == FEED_NO_LAYER {
            None
        } else if raw_layer as usize >= lfm25::MODEL_LAYER_COUNT {
            return Err(FeedError::InvalidRetirement);
        } else {
            Some(raw_layer)
        };
        Ok(Self {
            state,
            retired_mode,
            retired_layer,
            retired_session_epoch: words[2],
            retired_sequence: words[3],
            retired_item: words[4],
            error_code: words[5] as i32,
            completion_count: words[6],
        })
    }

    pub fn identity_matches(self, record: FeedCommitRecord) -> bool {
        self.retired_mode as u8 == record.mode
            && self.retired_layer
                == if record.layer == FEED_NO_LAYER {
                    None
                } else {
                    Some(record.layer)
                }
            && self.retired_session_epoch == record.session_epoch
            && self.retired_sequence == record.sequence
            && self.retired_item == record.item
    }

    pub fn completion_matches(self, record: FeedCommitRecord, previous_count: u32) -> bool {
        self.state == FeedState::Complete
            && self.error_code == 0
            && self.completion_count == previous_count.wrapping_add(1)
            && self.identity_matches(record)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum LayerDomain {
    Global,
    Any,
    ShortConv,
    Attention,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FixedFeedShape {
    pub mode: FeedMode,
    /// Number of row/vector retirements. One commit publishes one complete item.
    pub items: u32,
    /// Number of BAR2 staging slots per lane and item. Zero is the control-only core.
    pub stages_per_item: u16,
    pub lanes: u8,
    pub payload_format: FeedPayloadFormat,
    pub payload_bytes_per_stage: u16,
}

impl FixedFeedShape {
    const fn new(
        mode: FeedMode,
        items: u32,
        stages_per_item: u16,
        lanes: u8,
        payload_format: FeedPayloadFormat,
        payload_bytes_per_stage: u16,
    ) -> Self {
        Self {
            mode,
            items,
            stages_per_item,
            lanes,
            payload_format,
            payload_bytes_per_stage,
        }
    }

    pub const fn commits(self) -> u32 {
        self.items
    }

    pub const fn lane_mask(self) -> u8 {
        if self.lanes == 0 {
            0
        } else {
            (1u8 << self.lanes) - 1
        }
    }

    pub const fn shape_tag(self) -> u32 {
        let mut tag = 0x811C_9DC5u32;
        tag = fnv_word(tag, self.mode as u32);
        tag = fnv_word(tag, self.items);
        tag = fnv_word(tag, self.stages_per_item as u32);
        tag = fnv_word(tag, self.lanes as u32);
        tag = fnv_word(tag, self.payload_format as u32);
        fnv_word(tag, self.payload_bytes_per_stage as u32)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TensorExpectation {
    pub role: TensorRole,
    pub format: TensorFormat,
    pub ggml_ne0: u32,
    pub ggml_ne1: u32,
}

/// Exact sealed tensor byte range for the next staged payload. Q8_0 ranges name their
/// source row and native block. Packed BF16 ranges may cross logical scalar/triple
/// boundaries, so their byte offset is authoritative.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FeedSourceRange {
    pub tensor: TensorExpectation,
    pub row: Option<u32>,
    pub block: Option<u16>,
    pub relative_byte_offset: u32,
    pub payload_bytes: u16,
}

impl TensorExpectation {
    pub fn validate(
        self,
        descriptor: NativeTensorDescriptor,
        expected_layer: Option<u8>,
    ) -> Result<(), FeedError> {
        let layer = expected_layer.unwrap_or(FEED_NO_LAYER);
        if descriptor.layer != layer
            || descriptor.role != self.role as u8
            || TensorFormat::from_raw(descriptor.format) != Some(self.format)
            || descriptor.rank != if self.ggml_ne1 == 1 { 1 } else { 2 }
            || descriptor.ggml_ne0 != self.ggml_ne0
            || descriptor.ggml_ne1 != self.ggml_ne1
        {
            return Err(FeedError::InvalidTensorShape);
        }
        if self.role == TensorRole::TokenEmbedding
            && descriptor.flags & lfm25::TENSOR_FLAG_TIED_OUTPUT == 0
        {
            return Err(FeedError::InvalidTensorShape);
        }
        Ok(())
    }
}

const fn fnv_word(mut hash: u32, value: u32) -> u32 {
    let bytes = value.to_le_bytes();
    let mut index = 0;
    while index < 4 {
        hash ^= bytes[index] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        index += 1;
    }
    hash
}

const fn fixed_shape_set_tag() -> u32 {
    let mut hash = 0x811C_9DC5u32;
    let mut index = 0;
    while index < ALL_FEED_MODES.len() {
        hash = fnv_word(hash, ALL_FEED_MODES[index].shape().shape_tag());
        index += 1;
    }
    hash
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FeedRequest {
    pub mode: FeedMode,
    pub layer: Option<u8>,
    pub position: u32,
    pub token: Option<u32>,
    pub session_epoch: u32,
}

impl FeedRequest {
    pub fn validate(self) -> Result<(), FeedError> {
        if self.session_epoch == 0 {
            return Err(FeedError::InvalidSessionEpoch);
        }
        if self.position >= lfm25::MODEL_INITIAL_CONTEXT {
            return Err(FeedError::InvalidPosition);
        }
        if self.mode.first_token_only() && self.position != 0 {
            return Err(FeedError::InvalidPosition);
        }
        match (self.mode.domain(), self.layer) {
            (LayerDomain::Global, None) => {}
            (LayerDomain::Any, Some(layer)) if (layer as usize) < lfm25::MODEL_LAYER_COUNT => {}
            (LayerDomain::ShortConv, Some(layer))
                if (layer as usize) < lfm25::MODEL_LAYER_COUNT
                    && lfm25::LAYER_SCHEDULE[layer as usize] == LayerKind::ShortConv => {}
            (LayerDomain::Attention, Some(layer))
                if (layer as usize) < lfm25::MODEL_LAYER_COUNT
                    && lfm25::LAYER_SCHEDULE[layer as usize] == LayerKind::Attention => {}
            _ => return Err(FeedError::InvalidLayer),
        }
        match (self.mode, self.token) {
            (FeedMode::EmbeddingQ8Row, Some(token)) if token < lfm25::MODEL_VOCABULARY_SIZE => {}
            (FeedMode::EmbeddingQ8Row, _) => return Err(FeedError::InvalidToken),
            (_, None) => {}
            (_, Some(_)) => return Err(FeedError::InvalidToken),
        }
        Ok(())
    }

    pub const fn encoded_layer(self) -> u8 {
        match self.layer {
            Some(layer) => layer,
            None => FEED_NO_LAYER,
        }
    }

    pub const fn encoded_token(self) -> u32 {
        match self.token {
            Some(token) => token,
            None => FEED_NO_TOKEN,
        }
    }
}

/// One payload already copied into a fixed BAR2 lane. Generation and sequence tags make
/// reusing a physical slot safe only after its preceding commit has retired.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StagedPayload {
    pub sequence: u32,
    pub generation: u32,
    pub bank: StageBank,
    pub slot: u16,
    pub payload_format: FeedPayloadFormat,
    pub payload_bytes: u16,
}

impl StagedPayload {
    pub const fn bar2_offset(self) -> Option<usize> {
        if self.slot >= FEED_STAGING_SLOTS
            || self.payload_bytes as usize > super::BAR2_LFM25_STREAM_BLOCK_STRIDE
        {
            return None;
        }
        Some(self.bank.bar2_offset() + self.slot as usize * super::BAR2_LFM25_STREAM_BLOCK_STRIDE)
    }
}

/// Fixed little-endian record staged at [`FEED_COMMIT_BAR2_OFFSET`]. The host writes
/// bytes 0..59 before publishing [`commit_magic`](Self::commit_magic) as the last dword.
#[repr(C, align(64))]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FeedCommitRecord {
    pub record_magic: u32,
    pub abi_version: u16,
    pub record_bytes: u16,
    pub capability_bits: u32,
    pub mode: u8,
    pub layer: u8,
    pub lane_mask: u8,
    pub payload_format: u8,
    pub session_epoch: u32,
    pub sequence: u32,
    pub position: u32,
    pub token: u32,
    pub item: u32,
    /// Number of staged payloads in each active lane for this complete item.
    pub stages_per_lane: u16,
    /// Final physical staging slot, or [`FEED_NO_STAGE_SLOT`] for a control-only item.
    pub last_stage_slot: u16,
    pub payload_bytes_per_stage: u16,
    pub reserved0: u16,
    pub stage_generation: u32,
    pub shape_tag: u32,
    pub model_generation: u32,
    pub reserved1: [u8; 4],
    pub commit_magic: u32,
}

impl FeedCommitRecord {
    pub fn encode_le(self) -> [u8; FEED_COMMIT_RECORD_BYTES as usize] {
        let mut bytes = [0u8; FEED_COMMIT_RECORD_BYTES as usize];
        put_u32(&mut bytes, 0, self.record_magic);
        put_u16(&mut bytes, 4, self.abi_version);
        put_u16(&mut bytes, 6, self.record_bytes);
        put_u32(&mut bytes, 8, self.capability_bits);
        bytes[12] = self.mode;
        bytes[13] = self.layer;
        bytes[14] = self.lane_mask;
        bytes[15] = self.payload_format;
        put_u32(&mut bytes, 16, self.session_epoch);
        put_u32(&mut bytes, 20, self.sequence);
        put_u32(&mut bytes, 24, self.position);
        put_u32(&mut bytes, 28, self.token);
        put_u32(&mut bytes, 32, self.item);
        put_u16(&mut bytes, 36, self.stages_per_lane);
        put_u16(&mut bytes, 38, self.last_stage_slot);
        put_u16(&mut bytes, 40, self.payload_bytes_per_stage);
        put_u16(&mut bytes, 42, self.reserved0);
        put_u32(&mut bytes, 44, self.stage_generation);
        put_u32(&mut bytes, 48, self.shape_tag);
        put_u32(&mut bytes, 52, self.model_generation);
        bytes[56..60].copy_from_slice(&self.reserved1);
        put_u32(&mut bytes, 60, self.commit_magic);
        bytes
    }

    pub fn decode_le(bytes: &[u8]) -> Result<Self, FeedError> {
        if bytes.len() != FEED_COMMIT_RECORD_BYTES as usize {
            return Err(FeedError::InvalidEncoding);
        }
        let record = Self {
            record_magic: get_u32(bytes, 0),
            abi_version: get_u16(bytes, 4),
            record_bytes: get_u16(bytes, 6),
            capability_bits: get_u32(bytes, 8),
            mode: bytes[12],
            layer: bytes[13],
            lane_mask: bytes[14],
            payload_format: bytes[15],
            session_epoch: get_u32(bytes, 16),
            sequence: get_u32(bytes, 20),
            position: get_u32(bytes, 24),
            token: get_u32(bytes, 28),
            item: get_u32(bytes, 32),
            stages_per_lane: get_u16(bytes, 36),
            last_stage_slot: get_u16(bytes, 38),
            payload_bytes_per_stage: get_u16(bytes, 40),
            reserved0: get_u16(bytes, 42),
            stage_generation: get_u32(bytes, 44),
            shape_tag: get_u32(bytes, 48),
            model_generation: get_u32(bytes, 52),
            reserved1: [bytes[56], bytes[57], bytes[58], bytes[59]],
            commit_magic: get_u32(bytes, 60),
        };
        if !record.header_is_exact() {
            return Err(FeedError::InvalidEncoding);
        }
        Ok(record)
    }

    pub const fn header_is_exact(self) -> bool {
        self.record_magic == FEED_RECORD_MAGIC
            && self.abi_version == FEED_ABI_VERSION
            && self.record_bytes == FEED_COMMIT_RECORD_BYTES
            && self.capability_bits == REQUIRED_CAPABILITY_BITS
            && self.model_generation == lfm25::MODEL_GENERATION
            && self.reserved0 == 0
            && self.reserved1[0] == 0
            && self.reserved1[1] == 0
            && self.reserved1[2] == 0
            && self.reserved1[3] == 0
            && self.commit_magic == FEED_COMMIT_MAGIC
            && FeedMode::from_raw(self.mode).is_some()
            && FeedPayloadFormat::from_raw(self.payload_format).is_some()
    }

    /// Validate every fixed-shape field before the kernel may publish this
    /// record to BAR2. Sequence and item advance together because one commit
    /// retires exactly one complete item in TGF2.
    pub fn validate_for_publish(self) -> Result<(), FeedError> {
        if !self.header_is_exact() {
            return Err(FeedError::InvalidCommit);
        }
        let mode = FeedMode::from_raw(self.mode).ok_or(FeedError::InvalidCommit)?;
        let shape = mode.shape();
        let layer = if self.layer == FEED_NO_LAYER {
            None
        } else {
            Some(self.layer)
        };
        let token = if self.token == FEED_NO_TOKEN {
            None
        } else {
            Some(self.token)
        };
        FeedRequest {
            mode,
            layer,
            position: self.position,
            token,
            session_epoch: self.session_epoch,
        }
        .validate()?;
        let expected_last_stage = if shape.stages_per_item == 0 {
            FEED_NO_STAGE_SLOT
        } else {
            shape.stages_per_item - 1
        };
        let expected_generation = if shape.stages_per_item == 0 {
            0
        } else {
            (self.sequence + 1) * shape.stages_per_item as u32 * shape.lanes as u32
        };
        if self.item >= shape.items
            || self.sequence != self.item
            || self.lane_mask != shape.lane_mask()
            || self.payload_format != shape.payload_format as u8
            || self.stages_per_lane != shape.stages_per_item
            || self.last_stage_slot != expected_last_stage
            || self.payload_bytes_per_stage != shape.payload_bytes_per_stage
            || self.stage_generation != expected_generation
            || self.shape_tag != shape.shape_tag()
        {
            return Err(FeedError::InvalidCommit);
        }
        Ok(())
    }

    pub fn staged_payload_count(self) -> Result<usize, FeedError> {
        self.validate_for_publish()?;
        let mode = FeedMode::from_raw(self.mode).ok_or(FeedError::InvalidCommit)?;
        Ok(mode.shape().stages_per_item as usize * mode.shape().lanes as usize)
    }

    /// Descriptor for one stage-major, lane-minor payload belonging to this
    /// item. This is the same order the endpoint consumes paired/triplet lanes.
    pub fn expected_staged_payload(self, ordinal: usize) -> Result<StagedPayload, FeedError> {
        self.validate_for_publish()?;
        let mode = FeedMode::from_raw(self.mode).ok_or(FeedError::InvalidCommit)?;
        let shape = mode.shape();
        let count = shape.stages_per_item as usize * shape.lanes as usize;
        if ordinal >= count || shape.lanes == 0 {
            return Err(FeedError::UnexpectedStage);
        }
        let stage = ordinal / shape.lanes as usize;
        let lane = ordinal % shape.lanes as usize;
        Ok(StagedPayload {
            sequence: self.sequence,
            generation: self.sequence * count as u32 + ordinal as u32 + 1,
            bank: stage_bank(lane as u8),
            slot: stage as u16,
            payload_format: shape.payload_format,
            payload_bytes: shape.payload_bytes_per_stage,
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FeedError {
    InvalidCapability,
    InvalidSessionEpoch,
    InvalidLayer,
    InvalidPosition,
    InvalidToken,
    InvalidTensorShape,
    InvalidEncoding,
    InvalidRetirement,
    UnexpectedStage,
    MissingStage,
    InvalidCommit,
    Complete,
    Poisoned,
}

/// Host/firmware-model mirror of the strict staged feed sequence. Any protocol error
/// poisons the instance, matching the resident engines' explicit-reset requirement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedSequenceValidator {
    request: FeedRequest,
    item: u32,
    sequence: u32,
    next_stage: u16,
    next_lane: u8,
    poisoned: bool,
    complete: bool,
}

impl FeedSequenceValidator {
    pub fn begin(capability: FeedCapability, request: FeedRequest) -> Result<Self, FeedError> {
        if !capability_is_exact(capability) {
            return Err(FeedError::InvalidCapability);
        }
        request.validate()?;
        Ok(Self {
            request,
            item: 0,
            sequence: 0,
            next_stage: 0,
            next_lane: 0,
            poisoned: false,
            complete: false,
        })
    }

    pub const fn request(&self) -> FeedRequest {
        self.request
    }

    pub const fn committed_units(&self) -> u32 {
        self.sequence
    }

    pub const fn is_complete(&self) -> bool {
        self.complete
    }

    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub const fn expected_item(&self) -> u32 {
        self.item
    }

    /// True only after every lane of every fixed stage for the current item has been
    /// written. Control-only modes are immediately ready to commit.
    pub const fn staging_complete(&self) -> bool {
        self.next_stage == self.request.mode.shape().stages_per_item
    }

    /// The only staged write accepted next. Ordering is stage-major, lane-minor so
    /// paired/triplet feeds remain lock-step while a whole row is assembled.
    pub fn expected_stage(&self) -> Result<StagedPayload, FeedError> {
        self.require_active()?;
        let shape = self.request.mode.shape();
        if self.staging_complete() || shape.lanes == 0 {
            return Err(FeedError::UnexpectedStage);
        }
        Ok(StagedPayload {
            sequence: self.sequence,
            generation: self.expected_stage_generation(),
            bank: stage_bank(self.next_lane),
            slot: self.next_stage,
            payload_format: shape.payload_format,
            payload_bytes: shape.payload_bytes_per_stage,
        })
    }

    pub fn expected_source(&self) -> Result<FeedSourceRange, FeedError> {
        self.require_active()?;
        if self.staging_complete() {
            return Err(FeedError::UnexpectedStage);
        }
        let mode = self.request.mode;
        let shape = mode.shape();
        if shape.lanes == 0 {
            return Err(FeedError::UnexpectedStage);
        }
        let tensor = mode
            .tensor_expectation(self.next_lane)
            .ok_or(FeedError::InvalidTensorShape)?;
        let (row, block, relative_byte_offset) =
            if shape.payload_format == FeedPayloadFormat::GgmlQ8_0Block {
                let row = mode
                    .tensor_item(self.item, self.next_lane, self.request.token)
                    .ok_or(FeedError::InvalidTensorShape)?;
                let blocks_per_row = tensor.ggml_ne0 / 32;
                let row_bytes = blocks_per_row * 34;
                (Some(row), Some(self.next_stage), row * row_bytes + self.next_stage as u32 * 34)
            } else {
                (None, None, self.next_stage as u32 * shape.payload_bytes_per_stage as u32)
            };
        let source = FeedSourceRange {
            tensor,
            row,
            block,
            relative_byte_offset,
            payload_bytes: shape.payload_bytes_per_stage,
        };
        if source.relative_byte_offset + source.payload_bytes as u32 > tensor_encoded_bytes(tensor)
        {
            return Err(FeedError::InvalidTensorShape);
        }
        Ok(source)
    }

    pub fn stage(&mut self, staged: StagedPayload) -> Result<(), FeedError> {
        let result = self.validate_stage(staged);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    pub fn expected_commit(&self) -> Result<FeedCommitRecord, FeedError> {
        self.require_active()?;
        let shape = self.request.mode.shape();
        if !self.staging_complete() {
            return Err(FeedError::MissingStage);
        }
        Ok(FeedCommitRecord {
            record_magic: FEED_RECORD_MAGIC,
            abi_version: FEED_ABI_VERSION,
            record_bytes: FEED_COMMIT_RECORD_BYTES,
            capability_bits: REQUIRED_CAPABILITY_BITS,
            mode: self.request.mode as u8,
            layer: self.request.encoded_layer(),
            lane_mask: shape.lane_mask(),
            payload_format: shape.payload_format as u8,
            session_epoch: self.request.session_epoch,
            sequence: self.sequence,
            position: self.request.position,
            token: self.request.encoded_token(),
            item: self.item,
            stages_per_lane: shape.stages_per_item,
            last_stage_slot: if shape.stages_per_item == 0 {
                FEED_NO_STAGE_SLOT
            } else {
                shape.stages_per_item - 1
            },
            payload_bytes_per_stage: shape.payload_bytes_per_stage,
            reserved0: 0,
            stage_generation: if shape.stages_per_item == 0 {
                0
            } else {
                self.final_stage_generation()
            },
            shape_tag: shape.shape_tag(),
            model_generation: lfm25::MODEL_GENERATION,
            reserved1: [0; 4],
            commit_magic: FEED_COMMIT_MAGIC,
        })
    }

    pub fn commit(&mut self, record: FeedCommitRecord) -> Result<(), FeedError> {
        let result = self.validate_commit(record);
        if result.is_err() {
            self.poisoned = true;
            return result;
        }
        self.advance();
        Ok(())
    }

    fn validate_stage(&mut self, staged: StagedPayload) -> Result<(), FeedError> {
        self.require_active()?;
        let shape = self.request.mode.shape();
        if shape.lanes == 0 {
            return Err(FeedError::UnexpectedStage);
        }
        if staged != self.expected_stage()? {
            return Err(FeedError::UnexpectedStage);
        }
        if staged.bar2_offset().is_none() {
            return Err(FeedError::UnexpectedStage);
        }
        self.next_lane += 1;
        if self.next_lane == shape.lanes {
            self.next_lane = 0;
            self.next_stage += 1;
        }
        Ok(())
    }

    fn validate_commit(&self, record: FeedCommitRecord) -> Result<(), FeedError> {
        let expected = self.expected_commit()?;
        if !record.header_is_exact() || record != expected {
            return Err(FeedError::InvalidCommit);
        }
        Ok(())
    }

    fn require_active(&self) -> Result<(), FeedError> {
        if self.poisoned {
            Err(FeedError::Poisoned)
        } else if self.complete {
            Err(FeedError::Complete)
        } else {
            Ok(())
        }
    }

    const fn stages_per_item_all_lanes(&self) -> u32 {
        let shape = self.request.mode.shape();
        shape.stages_per_item as u32 * shape.lanes as u32
    }

    const fn expected_stage_generation(&self) -> u32 {
        self.sequence * self.stages_per_item_all_lanes()
            + self.next_stage as u32 * self.request.mode.shape().lanes as u32
            + self.next_lane as u32
            + 1
    }

    const fn final_stage_generation(&self) -> u32 {
        (self.sequence + 1) * self.stages_per_item_all_lanes()
    }

    fn advance(&mut self) {
        let shape = self.request.mode.shape();
        self.sequence += 1;
        self.item += 1;
        self.next_stage = 0;
        self.next_lane = 0;
        if self.item == shape.items {
            self.complete = true;
        }
    }
}

const fn tensor_encoded_bytes(tensor: TensorExpectation) -> u32 {
    match tensor.format {
        TensorFormat::Bf16Le => tensor.ggml_ne0 * tensor.ggml_ne1 * 2,
        TensorFormat::Q8_0 => tensor.ggml_ne0 / 32 * 34 * tensor.ggml_ne1,
    }
}

const fn stage_bank(lane: u8) -> StageBank {
    match lane {
        0 => StageBank::Bank0,
        1 => StageBank::Bank1,
        _ => StageBank::Bank2,
    }
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

const _: [(); FEED_COMMIT_RECORD_BYTES as usize] = [(); size_of::<FeedCommitRecord>()];
const _: [(); 64] = [(); align_of::<FeedCommitRecord>()];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lfm25_decode::tensor_descriptor;

    fn request(mode: FeedMode, layer: Option<u8>) -> FeedRequest {
        FeedRequest {
            mode,
            layer,
            position: 0,
            token: if mode == FeedMode::EmbeddingQ8Row {
                Some(1)
            } else {
                None
            },
            session_epoch: 7,
        }
    }

    fn stage_and_commit_one(validator: &mut FeedSequenceValidator) {
        while !validator.staging_complete() {
            let staged = validator.expected_stage().unwrap();
            assert!(staged.bar2_offset().is_some());
            validator.stage(staged).unwrap();
        }
        let record = validator.expected_commit().unwrap();
        let encoded = record.encode_le();
        assert_eq!(&encoded[60..64], &FEED_COMMIT_MAGIC.to_le_bytes());
        validator
            .commit(FeedCommitRecord::decode_le(&encoded).unwrap())
            .unwrap();
    }

    #[test]
    fn v1_ffn_bar2_contract_is_unchanged_and_not_v2_capability() {
        assert_eq!(crate::LFM25_STREAM_CAPABILITY_MAGIC, 0x3252_4754);
        assert_eq!(crate::LFM25_STREAM_MODE_GATE_UP_SILU, 1);
        assert_eq!(crate::LFM25_STREAM_MODE_DOWN, 2);
        assert_eq!(StageBank::Bank0.bar2_offset(), 0x0000);
        assert_eq!(StageBank::Bank1.bar2_offset(), 0x4000);
        assert_eq!(StageBank::Bank2.bar2_offset(), 0x8000);
        assert_eq!(crate::BAR2_LFM25_STREAM_REQUIRED_BYTES, 0xC000);
        assert!(FEED_COMMIT_BAR2_OFFSET >= crate::BAR2_LFM25_STREAM_REQUIRED_BYTES);
        assert!(FEED_COMMIT_BAR2_OFFSET + 64 <= crate::BAR2_LFM25_STREAM_BYTES);

        let mut wrong = REQUIRED_CAPABILITY;
        wrong.magic = crate::lfm25_decode_transport::CAPABILITY_MAGIC;
        assert!(!capability_is_exact(wrong));
    }

    #[test]
    fn capability_match_is_exact_and_fail_closed() {
        assert!(capability_is_exact(REQUIRED_CAPABILITY));
        assert_eq!(
            FeedCapability::from_bar0_words(REQUIRED_CAPABILITY.bar0_words()),
            REQUIRED_CAPABILITY
        );
        assert_eq!(BAR0_FEED_CAPABILITY_MAGIC_OFFSET, crate::BAR0_REQUIRED_BYTES);
        assert_eq!(BAR0_FEED_VERSION_RECORD_BYTES_OFFSET, 0x284);
        assert_eq!(BAR0_FEED_CAPABILITY_BITS_OFFSET, 0x288);
        assert_eq!(BAR0_FEED_MODEL_GENERATION_OFFSET, 0x28c);
        assert_eq!(BAR0_FEED_SHAPE_SET_TAG_OFFSET, 0x290);
        assert_eq!(BAR0_FEED_CAPABILITY_REQUIRED_BYTES, 0x294);
        assert_eq!(BAR0_FEED_STATE_OFFSET, 0x294);
        assert_eq!(BAR0_FEED_RETIRED_MODE_LAYER_OFFSET, 0x298);
        assert_eq!(BAR0_FEED_RETIRED_SESSION_EPOCH_OFFSET, 0x29c);
        assert_eq!(BAR0_FEED_RETIRED_SEQUENCE_OFFSET, 0x2a0);
        assert_eq!(BAR0_FEED_RETIRED_ITEM_OFFSET, 0x2a4);
        assert_eq!(BAR0_FEED_ERROR_CODE_OFFSET, 0x2a8);
        assert_eq!(BAR0_FEED_COMPLETION_COUNT_OFFSET, 0x2ac);
        assert_eq!(BAR0_FEED_CONTROL_OFFSET, 0x2b0);
        assert_eq!(BAR0_FEED_REQUIRED_BYTES, 0x2b4);
        assert_eq!(BAR0_FEED_SHARED_IRQ_ACK_OFFSET, crate::BAR0_CALL_IRQ_ACK_OFFSET);
        assert_eq!(BAR0_FEED_SHARED_IRQ_STATE_OFFSET, crate::BAR0_CALL_IRQ_STATE_OFFSET);
        assert_eq!(FEED_SHARED_IRQ_PENDING_BIT, 1);
        assert_eq!(FEED_ERROR_NONE, 0);
        assert_eq!(FEED_ERROR_FRONTEND_POISON, 0xbad4_0001);
        assert!(BAR0_FEED_REQUIRED_BYTES <= TGA_BAR0_APERTURE_BYTES);
        for mutate in 0..6 {
            let mut observed = REQUIRED_CAPABILITY;
            match mutate {
                0 => observed.magic ^= 1,
                1 => observed.abi_version += 1,
                2 => observed.commit_record_bytes += 4,
                3 => observed.capability_bits ^= 1,
                4 => observed.capability_bits |= 1 << 31,
                _ => observed.shape_set_tag ^= 1,
            }
            assert!(!capability_is_exact(observed));
        }
    }

    #[test]
    fn retirement_status_decodes_and_matches_exact_identity() {
        let mut validator = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::FfnGateUpRows, Some(0)),
        )
        .unwrap();
        while !validator.staging_complete() {
            let staged = validator.expected_stage().unwrap();
            validator.stage(staged).unwrap();
        }
        let record = validator.expected_commit().unwrap();
        let words = [
            FeedState::Complete as u32,
            retired_mode_layer_word(FeedMode::FfnGateUpRows, Some(0)),
            record.session_epoch,
            record.sequence,
            record.item,
            FEED_ERROR_NONE,
            41,
        ];
        let status = FeedRetirementStatus::from_bar0_words(words).unwrap();
        assert!(status.identity_matches(record));
        assert!(status.completion_matches(record, 40));
        assert!(FeedState::Complete.is_terminal());
        assert!(FeedState::Failed.is_terminal());
        assert!(FeedState::Poisoned.is_terminal());
        assert!(!FeedState::Idle.is_terminal());
        assert!(!FeedState::Busy.is_terminal());

        let mut stale = words;
        stale[3] = stale[3].wrapping_add(1);
        assert!(
            !FeedRetirementStatus::from_bar0_words(stale)
                .unwrap()
                .identity_matches(record)
        );
        let mut widened = words;
        widened[1] |= 1 << 31;
        assert_eq!(
            FeedRetirementStatus::from_bar0_words(widened),
            Err(FeedError::InvalidRetirement)
        );
        assert_eq!(FeedState::from_raw(5), None);
    }

    #[test]
    fn every_mode_has_a_unique_fixed_shape() {
        let mut tags = [0u32; ALL_FEED_MODES.len()];
        for (index, mode) in ALL_FEED_MODES.iter().copied().enumerate() {
            let shape = mode.shape();
            assert_eq!(shape.mode, mode);
            assert!(shape.items > 0);
            assert!(shape.lanes <= 3);
            assert!(shape.stages_per_item <= FEED_STAGING_SLOTS);
            assert!(shape.payload_bytes_per_stage <= 64);
            assert!(shape.commits() > 0);
            tags[index] = shape.shape_tag();
            assert_eq!(FeedMode::from_raw(mode as u8), Some(mode));
        }
        for left in 0..tags.len() {
            for right in left + 1..tags.len() {
                assert_ne!(tags[left], tags[right]);
            }
        }
        assert_eq!(FeedMode::TiedLmHeadRows.shape().items, 65_536);
        assert_eq!(FeedMode::TiedLmHeadRows.shape().commits(), 65_536);
        assert_eq!(FeedMode::TiedLmHeadRows.shape().stages_per_item, 32);
        assert_eq!(FeedMode::FfnGateUpRows.shape().commits(), 4_608);
        assert_eq!(FeedMode::FfnDownRows.shape().commits(), 1_024);
        assert_eq!(FeedMode::FfnDownRows.shape().stages_per_item, 144);
        assert_eq!(FeedMode::EmbeddingQ8Row.shape().commits(), 1);
        assert_eq!(FeedMode::OperatorRmsNormWeights.shape().commits(), 1);
        assert_eq!(FeedMode::AttentionFirstTokenCore.shape().commits(), 1);
        assert_eq!(FeedMode::FfnGateUpRows.shape().lanes, 2);
        assert_eq!(FeedMode::ShortConvInputTripletRows.shape().lanes, 3);
        assert_eq!(FeedMode::AttentionFirstTokenCore.shape().lanes, 0);
    }

    #[test]
    fn generated_model_tensors_match_every_payload_lane() {
        for mode in ALL_FEED_MODES {
            let layer = match mode.domain() {
                LayerDomain::Global => None,
                LayerDomain::Attention => Some(2),
                LayerDomain::ShortConv | LayerDomain::Any => Some(0),
            };
            for lane in 0..mode.shape().lanes {
                let expectation = mode.tensor_expectation(lane).unwrap();
                let tensor = tensor_descriptor(layer, expectation.role).unwrap();
                assert_eq!(expectation.validate(tensor, layer), Ok(()), "{mode:?}/{lane}");
            }
        }
    }

    #[test]
    fn every_mode_can_stage_and_retire_exactly_one_complete_item() {
        for mode in ALL_FEED_MODES {
            let layer = match mode.domain() {
                LayerDomain::Global => None,
                LayerDomain::Attention => Some(2),
                LayerDomain::ShortConv | LayerDomain::Any => Some(0),
            };
            let mut validator =
                FeedSequenceValidator::begin(REQUIRED_CAPABILITY, request(mode, layer)).unwrap();
            let shape = mode.shape();
            let mut staged = 0u32;
            while !validator.staging_complete() {
                let expected = validator.expected_stage().unwrap();
                assert!(expected.bar2_offset().is_some(), "{mode:?}");
                validator.stage(expected).unwrap();
                staged += 1;
            }
            assert_eq!(staged, shape.stages_per_item as u32 * shape.lanes as u32);
            let record = validator.expected_commit().unwrap();
            assert_eq!(record.item, 0);
            assert_eq!(record.stages_per_lane, shape.stages_per_item);
            validator.commit(record).unwrap();
            assert_eq!(validator.committed_units(), 1);
        }
    }

    #[test]
    fn fixed_source_indices_cover_embedding_and_shortconv_triplet_layout() {
        let embedding = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::EmbeddingQ8Row, None),
        )
        .unwrap();
        let source = embedding.expected_source().unwrap();
        assert_eq!(source.row, Some(1));
        assert_eq!(source.block, Some(0));
        assert_eq!(source.relative_byte_offset, 32 * 34);

        let mut shortconv = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::ShortConvInputTripletRows, Some(0)),
        )
        .unwrap();
        assert_eq!(shortconv.expected_source().unwrap().row, Some(0));
        shortconv
            .stage(shortconv.expected_stage().unwrap())
            .unwrap();
        assert_eq!(shortconv.expected_source().unwrap().row, Some(1024));
        shortconv
            .stage(shortconv.expected_stage().unwrap())
            .unwrap();
        assert_eq!(shortconv.expected_source().unwrap().row, Some(2048));

        let mut packed = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::ShortConvCoefficients, Some(0)),
        )
        .unwrap();
        assert_eq!(packed.expected_source().unwrap().relative_byte_offset, 0);
        packed.stage(packed.expected_stage().unwrap()).unwrap();
        assert_eq!(packed.expected_source().unwrap().relative_byte_offset, 64);
    }

    #[test]
    fn request_domains_and_first_token_attention_fail_closed() {
        assert_eq!(
            request(FeedMode::ShortConvInputTripletRows, Some(2)).validate(),
            Err(FeedError::InvalidLayer)
        );
        assert_eq!(
            request(FeedMode::AttentionQueryRows, Some(0)).validate(),
            Err(FeedError::InvalidLayer)
        );
        let mut later_attention = request(FeedMode::AttentionFirstTokenCore, Some(2));
        later_attention.position = 1;
        assert_eq!(later_attention.validate(), Err(FeedError::InvalidPosition));
        let mut dynamic_token = request(FeedMode::FfnDownRows, Some(0));
        dynamic_token.token = Some(1);
        assert_eq!(dynamic_token.validate(), Err(FeedError::InvalidToken));
    }

    #[test]
    fn staging_is_explicit_tagged_and_commit_is_last_dword() {
        let mut validator = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::FfnGateUpRows, Some(0)),
        )
        .unwrap();
        assert_eq!(validator.expected_commit(), Err(FeedError::MissingStage));

        let gate = validator.expected_stage().unwrap();
        validator.stage(gate).unwrap();
        let up = validator.expected_stage().unwrap();
        assert_eq!((gate.bank, up.bank), (StageBank::Bank0, StageBank::Bank1));
        assert_eq!(validator.stage(gate), Err(FeedError::UnexpectedStage));
        assert!(validator.is_poisoned());

        let mut clean = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::FfnGateUpRows, Some(0)),
        )
        .unwrap();
        while !clean.staging_complete() {
            clean.stage(clean.expected_stage().unwrap()).unwrap();
        }
        let record = clean.expected_commit().unwrap();
        assert_eq!(record.stages_per_lane, 32);
        assert_eq!(record.last_stage_slot, 31);
        assert_eq!(record.stage_generation, 64);
        assert_eq!(size_of::<FeedCommitRecord>(), 64);
        assert_eq!(align_of::<FeedCommitRecord>(), 64);
        assert_eq!(core::mem::offset_of!(FeedCommitRecord, stages_per_lane), 36);
        assert_eq!(core::mem::offset_of!(FeedCommitRecord, last_stage_slot), 38);
        assert_eq!(core::mem::offset_of!(FeedCommitRecord, payload_bytes_per_stage), 40);
        assert_eq!(core::mem::offset_of!(FeedCommitRecord, commit_magic), 60);
        assert_eq!(FeedCommitRecord::decode_le(&record.encode_le()), Ok(record));
        clean.commit(record).unwrap();
        assert_eq!(clean.expected_item(), 1);
        assert!(!clean.staging_complete());
    }

    #[test]
    fn stale_or_shape_changed_commit_poisons_sequence() {
        let mut validator = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::EmbeddingQ8Row, None),
        )
        .unwrap();
        while !validator.staging_complete() {
            validator
                .stage(validator.expected_stage().unwrap())
                .unwrap();
        }
        let mut record = validator.expected_commit().unwrap();
        record.shape_tag ^= 1;
        assert_eq!(validator.commit(record), Err(FeedError::InvalidCommit));
        assert!(validator.is_poisoned());
        assert_eq!(validator.expected_stage(), Err(FeedError::Poisoned));
    }

    #[test]
    fn first_token_core_is_a_control_commit_without_staged_payload() {
        let mut validator = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::AttentionFirstTokenCore, Some(2)),
        )
        .unwrap();
        assert_eq!(validator.expected_stage(), Err(FeedError::UnexpectedStage));
        let record = validator.expected_commit().unwrap();
        assert_eq!(record.lane_mask, 0);
        assert_eq!(record.stages_per_lane, 0);
        assert_eq!(record.last_stage_slot, FEED_NO_STAGE_SLOT);
        assert_eq!(record.stage_generation, 0);
        validator.commit(record).unwrap();
        assert!(validator.is_complete());
    }

    #[test]
    fn full_production_tied_head_sequence_reaches_exact_final_count() {
        let mut validator = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::TiedLmHeadRows, None),
        )
        .unwrap();
        while !validator.is_complete() {
            stage_and_commit_one(&mut validator);
        }
        assert_eq!(validator.committed_units(), 65_536);
        assert_eq!(validator.expected_stage(), Err(FeedError::Complete));
        assert_eq!(validator.expected_commit(), Err(FeedError::Complete));
    }
}
