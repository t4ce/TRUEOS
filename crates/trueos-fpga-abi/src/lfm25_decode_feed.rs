//! Versioned fixed feed contract for the resident LFM2.5 decode engines.
//!
//! This extends the physical three-bank BAR2 staging convention used by the proven
//! layer-0 FFN v1 image. It does not change any v1 offset, magic, or mode. A future
//! endpoint must separately publish the exact [`REQUIRED_CAPABILITY`] before TRUEOS may
//! use this contract.
//!
//! Every [`FeedMode`] names one ahead-of-time circuit and has immutable model shapes.
//! Payload bytes are written to one or more fixed BAR2 banks first; the final dword of a
//! [`FeedCommitRecord`] publishes that staged unit. This is not bytecode, a graph, a
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
pub const FEED_NO_LAYER: u8 = u8::MAX;
pub const FEED_NO_TOKEN: u32 = u32::MAX;
pub const FEED_NO_BLOCK: u16 = u16::MAX;
pub const FEED_NO_STAGE_SLOT: u16 = u16::MAX;

pub const CAP_EXPLICIT_STAGE_COMMIT: u32 = 1 << 0;
pub const CAP_TAGGED_SEQUENCE: u32 = 1 << 1;
pub const CAP_EMBEDDING: u32 = 1 << 2;
pub const CAP_RMSNORM: u32 = 1 << 3;
pub const CAP_SHORTCONV: u32 = 1 << 4;
pub const CAP_ATTENTION_FIRST_TOKEN: u32 = 1 << 5;
pub const CAP_FFN: u32 = 1 << 6;
pub const CAP_TIED_LM_HEAD: u32 = 1 << 7;
pub const REQUIRED_CAPABILITY_BITS: u32 = CAP_EXPLICIT_STAGE_COMMIT
    | CAP_TAGGED_SEQUENCE
    | CAP_EMBEDDING
    | CAP_RMSNORM
    | CAP_SHORTCONV
    | CAP_ATTENTION_FIRST_TOKEN
    | CAP_FFN
    | CAP_TIED_LM_HEAD;

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
                FixedFeedShape::new(self, lfm25::MODEL_HIDDEN_SIZE, 1, 1, Bf16, 2)
            }
            Self::ShortConvCoefficients => {
                FixedFeedShape::new(self, lfm25::MODEL_HIDDEN_SIZE, 1, 1, Bf16x3, 6)
            }
            Self::ShortConvInputTripletRows => {
                FixedFeedShape::new(self, lfm25::MODEL_HIDDEN_SIZE, 32, 3, GgmlQ8_0Block, 34)
            }
            Self::ShortConvOutputRows => {
                FixedFeedShape::new(self, lfm25::MODEL_HIDDEN_SIZE, 32, 1, GgmlQ8_0Block, 34)
            }
            Self::AttentionQkNormWeights => {
                FixedFeedShape::new(self, lfm25::MODEL_HEAD_DIMENSION as u32, 1, 2, Bf16, 2)
            }
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
    pub items: u32,
    /// Zero is reserved for the single control-only attention-core commit.
    pub blocks_per_item: u16,
    pub lanes: u8,
    pub payload_format: FeedPayloadFormat,
    pub payload_bytes_per_lane: u16,
}

impl FixedFeedShape {
    const fn new(
        mode: FeedMode,
        items: u32,
        blocks_per_item: u16,
        lanes: u8,
        payload_format: FeedPayloadFormat,
        payload_bytes_per_lane: u16,
    ) -> Self {
        Self {
            mode,
            items,
            blocks_per_item,
            lanes,
            payload_format,
            payload_bytes_per_lane,
        }
    }

    pub const fn commits(self) -> u32 {
        self.items
            * if self.blocks_per_item == 0 {
                1
            } else {
                self.blocks_per_item as u32
            }
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
        tag = fnv_word(tag, self.blocks_per_item as u32);
        tag = fnv_word(tag, self.lanes as u32);
        tag = fnv_word(tag, self.payload_format as u32);
        fnv_word(tag, self.payload_bytes_per_lane as u32)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TensorExpectation {
    pub role: TensorRole,
    pub format: TensorFormat,
    pub ggml_ne0: u32,
    pub ggml_ne1: u32,
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
    pub block: u16,
    pub stage_slot: u16,
    pub payload_bytes_per_lane: u16,
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
        put_u16(&mut bytes, 36, self.block);
        put_u16(&mut bytes, 38, self.stage_slot);
        put_u16(&mut bytes, 40, self.payload_bytes_per_lane);
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
            block: get_u16(bytes, 36),
            stage_slot: get_u16(bytes, 38),
            payload_bytes_per_lane: get_u16(bytes, 40),
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
    UnexpectedStage,
    DuplicateStage,
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
    block: u16,
    sequence: u32,
    staged_mask: u8,
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
            block: 0,
            sequence: 0,
            staged_mask: 0,
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

    pub const fn expected_block(&self) -> u16 {
        if self.request.mode.shape().blocks_per_item == 0 {
            FEED_NO_BLOCK
        } else {
            self.block
        }
    }

    pub fn expected_stage(&self, lane: u8) -> Result<StagedPayload, FeedError> {
        self.require_active()?;
        let shape = self.request.mode.shape();
        if lane >= shape.lanes {
            return Err(FeedError::UnexpectedStage);
        }
        Ok(StagedPayload {
            sequence: self.sequence,
            generation: self.sequence + 1,
            bank: stage_bank(lane),
            slot: self.expected_stage_slot(),
            payload_format: shape.payload_format,
            payload_bytes: shape.payload_bytes_per_lane,
        })
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
        if self.staged_mask != shape.lane_mask() {
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
            block: self.expected_block(),
            stage_slot: if shape.lanes == 0 {
                FEED_NO_STAGE_SLOT
            } else {
                self.expected_stage_slot()
            },
            payload_bytes_per_lane: shape.payload_bytes_per_lane,
            reserved0: 0,
            stage_generation: if shape.lanes == 0 {
                0
            } else {
                self.sequence + 1
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
        let lane = staged.bank as u8;
        if lane >= shape.lanes || staged != self.expected_stage(lane)? {
            return Err(FeedError::UnexpectedStage);
        }
        let lane_bit = 1u8 << lane;
        if self.staged_mask & lane_bit != 0 {
            return Err(FeedError::DuplicateStage);
        }
        if staged.bar2_offset().is_none() {
            return Err(FeedError::UnexpectedStage);
        }
        self.staged_mask |= lane_bit;
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

    const fn expected_stage_slot(&self) -> u16 {
        let shape = self.request.mode.shape();
        if shape.blocks_per_item > 1 {
            self.block
        } else {
            (self.item % FEED_STAGING_SLOTS as u32) as u16
        }
    }

    fn advance(&mut self) {
        let shape = self.request.mode.shape();
        self.staged_mask = 0;
        self.sequence += 1;
        if shape.blocks_per_item > 0 && self.block + 1 < shape.blocks_per_item {
            self.block += 1;
            return;
        }
        self.block = 0;
        self.item += 1;
        if self.item == shape.items {
            self.complete = true;
        }
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
        let lanes = validator.request().mode.shape().lanes;
        for lane in 0..lanes {
            let staged = validator.expected_stage(lane).unwrap();
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
    fn every_mode_has_a_unique_fixed_shape() {
        let mut tags = [0u32; ALL_FEED_MODES.len()];
        for (index, mode) in ALL_FEED_MODES.iter().copied().enumerate() {
            let shape = mode.shape();
            assert_eq!(shape.mode, mode);
            assert!(shape.items > 0);
            assert!(shape.lanes <= 3);
            assert!(shape.payload_bytes_per_lane <= 34);
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
        assert_eq!(FeedMode::TiedLmHeadRows.shape().commits(), 2_097_152);
        assert_eq!(FeedMode::FfnDownRows.shape().blocks_per_item, 144);
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

        let gate = validator.expected_stage(0).unwrap();
        let up = validator.expected_stage(1).unwrap();
        assert_eq!((gate.bank, up.bank), (StageBank::Bank0, StageBank::Bank1));
        validator.stage(gate).unwrap();
        assert_eq!(validator.stage(gate), Err(FeedError::DuplicateStage));
        assert!(validator.is_poisoned());

        let mut clean = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::FfnGateUpRows, Some(0)),
        )
        .unwrap();
        clean.stage(clean.expected_stage(0).unwrap()).unwrap();
        clean.stage(clean.expected_stage(1).unwrap()).unwrap();
        let record = clean.expected_commit().unwrap();
        assert_eq!(size_of::<FeedCommitRecord>(), 64);
        assert_eq!(align_of::<FeedCommitRecord>(), 64);
        assert_eq!(core::mem::offset_of!(FeedCommitRecord, commit_magic), 60);
        assert_eq!(FeedCommitRecord::decode_le(&record.encode_le()), Ok(record));
        clean.commit(record).unwrap();
        assert_eq!((clean.expected_item(), clean.expected_block()), (0, 1));
    }

    #[test]
    fn stale_or_shape_changed_commit_poisons_sequence() {
        let mut validator = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::EmbeddingQ8Row, None),
        )
        .unwrap();
        validator
            .stage(validator.expected_stage(0).unwrap())
            .unwrap();
        let mut record = validator.expected_commit().unwrap();
        record.shape_tag ^= 1;
        assert_eq!(validator.commit(record), Err(FeedError::InvalidCommit));
        assert!(validator.is_poisoned());
        assert_eq!(validator.expected_stage(0), Err(FeedError::Poisoned));
    }

    #[test]
    fn first_token_core_is_a_control_commit_without_staged_payload() {
        let mut validator = FeedSequenceValidator::begin(
            REQUIRED_CAPABILITY,
            request(FeedMode::AttentionFirstTokenCore, Some(2)),
        )
        .unwrap();
        assert_eq!(validator.expected_stage(0), Err(FeedError::UnexpectedStage));
        let record = validator.expected_commit().unwrap();
        assert_eq!(record.lane_mask, 0);
        assert_eq!(record.block, FEED_NO_BLOCK);
        assert_eq!(record.stage_slot, FEED_NO_STAGE_SLOT);
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
        assert_eq!(validator.committed_units(), 65_536 * 32);
        assert_eq!(validator.expected_stage(0), Err(FeedError::Complete));
        assert_eq!(validator.expected_commit(), Err(FeedError::Complete));
    }
}
