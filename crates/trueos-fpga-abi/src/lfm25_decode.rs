//! Fixed, ahead-of-time LFM2.5 single-token decode plan.
//!
//! This module describes orchestration only. It is deliberately not a graph format,
//! bytecode, runtime compiler, or numerical fallback. Every [`DecodeOpKind`] names a
//! circuit which must be present in the matching TRUEGA firmware before a token may
//! start. The order mirrors the pinned llama.cpp `src/models/lfm2.cpp` implementation.

use super::lfm25::{self, LayerKind, NativeTensorDescriptor, TensorFormat, TensorRole};

pub const PINNED_LLAMA_CPP_COMMIT: &str = "76f46ad29d61fd8c1401e8221842934bf62a6064";
pub const PINNED_LFM2_SOURCE: &str = "src/models/lfm2.cpp";
pub const OPS_PER_LAYER: usize = 6;
pub const OPS_PER_TOKEN: usize = 1 + lfm25::MODEL_LAYER_COUNT * OPS_PER_LAYER + 2;
pub const SHORTCONV_STATE_COUNT: usize = 10;
pub const KV_CACHE_COUNT: usize = 6;
pub const Q8_ROW_BYTES: u32 =
    (lfm25::MODEL_HIDDEN_SIZE as usize / lfm25::Q8_0_BLOCK_VALUES * lfm25::Q8_0_BLOCK_BYTES) as u32;
pub const TOKEN1_DECODE_TRACE_BYTES: u32 = 670_936;
pub const TOKEN1_DECODE_TRACE_SHA256: [u8; 32] = [
    0x0f, 0x66, 0xcf, 0x36, 0x91, 0x4d, 0x52, 0xdc, 0x56, 0x22, 0x3f, 0x7b, 0x85, 0x2e, 0x13, 0x89,
    0x6e, 0x58, 0x96, 0xe0, 0xe2, 0x2a, 0x5e, 0x22, 0x0d, 0x3f, 0xf3, 0x03, 0x75, 0x34, 0xce, 0x9a,
];
pub const TOKEN1_DECODE_INPUT_TOKEN: u32 = 1;
pub const TOKEN1_DECODE_OUTPUT_TOKEN: u32 = 1;

/// One immutable operation in the generated single-token program.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DecodeOpKind {
    TokenEmbedding = 0,
    OperatorRmsNorm = 1,
    ShortConv = 2,
    Attention = 3,
    OperatorResidual = 4,
    FfnRmsNorm = 5,
    Ffn = 6,
    FfnResidual = 7,
    FinalRmsNorm = 8,
    TiedLmHeadArgmax = 9,
}

impl DecodeOpKind {
    pub const fn capability(self) -> DecodeCapabilities {
        DecodeCapabilities(1u16 << self as u8)
    }
}

/// Firmware circuits accepted by the fixed decode scheduler.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DecodeCapabilities(u16);

impl DecodeCapabilities {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self((1u16 << 10) - 1);

    pub const fn from_bits(bits: u16) -> Self {
        Self(bits & Self::ALL.0)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MissingCapability {
    pub operation: DecodeOpKind,
}

/// State slot owned by a stateful layer operation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LayerStateSlot {
    ShortConv(u8),
    KvCache(u8),
}

/// One step of the 99-operation token plan.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PlannedDecodeOp {
    pub ordinal: u8,
    pub kind: DecodeOpKind,
    pub layer: Option<u8>,
    pub state: Option<LayerStateSlot>,
}

/// Iterator over the exact pinned single-token graph.
#[derive(Clone, Debug)]
pub struct DecodePlan {
    cursor: usize,
}

impl DecodePlan {
    pub const fn new() -> Self {
        Self { cursor: 0 }
    }

    /// Reject the token before embedding or cache mutation if any circuit is absent.
    pub fn require_capabilities(available: DecodeCapabilities) -> Result<(), MissingCapability> {
        let mut plan = Self::new();
        while let Some(operation) = plan.next() {
            if !available.contains(operation.kind.capability()) {
                return Err(MissingCapability {
                    operation: operation.kind,
                });
            }
        }
        Ok(())
    }
}

impl Default for DecodePlan {
    fn default() -> Self {
        Self::new()
    }
}

impl Iterator for DecodePlan {
    type Item = PlannedDecodeOp;

    fn next(&mut self) -> Option<Self::Item> {
        let ordinal = self.cursor;
        if ordinal >= OPS_PER_TOKEN {
            return None;
        }
        self.cursor += 1;

        let (kind, layer, state) = if ordinal == 0 {
            (DecodeOpKind::TokenEmbedding, None, None)
        } else if ordinal <= lfm25::MODEL_LAYER_COUNT * OPS_PER_LAYER {
            let in_layers = ordinal - 1;
            let layer = (in_layers / OPS_PER_LAYER) as u8;
            let phase = in_layers % OPS_PER_LAYER;
            match phase {
                0 => (DecodeOpKind::OperatorRmsNorm, Some(layer), None),
                1 => match lfm25::LAYER_SCHEDULE[layer as usize] {
                    LayerKind::ShortConv => {
                        (DecodeOpKind::ShortConv, Some(layer), state_slot_for_layer(layer))
                    }
                    LayerKind::Attention => {
                        (DecodeOpKind::Attention, Some(layer), state_slot_for_layer(layer))
                    }
                },
                2 => (DecodeOpKind::OperatorResidual, Some(layer), None),
                3 => (DecodeOpKind::FfnRmsNorm, Some(layer), None),
                4 => (DecodeOpKind::Ffn, Some(layer), None),
                _ => (DecodeOpKind::FfnResidual, Some(layer), None),
            }
        } else if ordinal == OPS_PER_TOKEN - 2 {
            (DecodeOpKind::FinalRmsNorm, None, None)
        } else {
            (DecodeOpKind::TiedLmHeadArgmax, None, None)
        };

        Some(PlannedDecodeOp {
            ordinal: ordinal as u8,
            kind,
            layer,
            state,
        })
    }
}

/// Map the generated hybrid schedule onto ten recurrent and six KV state slots.
pub const fn state_slot_for_layer(layer: u8) -> Option<LayerStateSlot> {
    if layer as usize >= lfm25::MODEL_LAYER_COUNT {
        return None;
    }
    let kind = lfm25::LAYER_SCHEDULE[layer as usize];
    let mut slot = 0u8;
    let mut candidate = 0usize;
    while candidate < layer as usize {
        if lfm25::LAYER_SCHEDULE[candidate] as u8 == kind as u8 {
            slot += 1;
        }
        candidate += 1;
    }
    match kind {
        LayerKind::ShortConv => Some(LayerStateSlot::ShortConv(slot)),
        LayerKind::Attention => Some(LayerStateSlot::KvCache(slot)),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StateProgressError {
    Poisoned,
    Position {
        slot: LayerStateSlot,
        expected: u32,
        observed: u32,
    },
    IncompleteToken,
}

/// Testable host mirror of state positions committed by completion callbacks.
///
/// This stores counters only; recurrent vectors and KV values remain FPGA-owned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodeStateProgress {
    position: u32,
    shortconv_next: [u32; SHORTCONV_STATE_COUNT],
    kv_next: [u32; KV_CACHE_COUNT],
    poisoned: bool,
}

impl DecodeStateProgress {
    pub const fn new() -> Self {
        Self {
            position: 0,
            shortconv_next: [0; SHORTCONV_STATE_COUNT],
            kv_next: [0; KV_CACHE_COUNT],
            poisoned: false,
        }
    }

    pub const fn position(&self) -> u32 {
        self.position
    }

    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub fn require_current(&self, slot: LayerStateSlot) -> Result<(), StateProgressError> {
        if self.poisoned {
            return Err(StateProgressError::Poisoned);
        }
        let observed = self.slot_position(slot);
        if observed == self.position {
            Ok(())
        } else {
            Err(StateProgressError::Position {
                slot,
                expected: self.position,
                observed,
            })
        }
    }

    /// Retire exactly the state position reported by the MSI callback.
    pub fn commit_callback(
        &mut self,
        slot: LayerStateSlot,
        completed_position: u32,
    ) -> Result<(), StateProgressError> {
        self.require_current(slot)?;
        if completed_position != self.position {
            self.poisoned = true;
            return Err(StateProgressError::Position {
                slot,
                expected: self.position,
                observed: completed_position,
            });
        }
        let next = self.position + 1;
        match slot {
            LayerStateSlot::ShortConv(slot) => self.shortconv_next[slot as usize] = next,
            LayerStateSlot::KvCache(slot) => self.kv_next[slot as usize] = next,
        }
        Ok(())
    }

    pub fn commit_token(&mut self) -> Result<u32, StateProgressError> {
        let next = self.position + 1;
        if !self.shortconv_next.iter().all(|position| *position == next)
            || !self.kv_next.iter().all(|position| *position == next)
        {
            self.poisoned = true;
            return Err(StateProgressError::IncompleteToken);
        }
        self.position = next;
        Ok(self.position)
    }

    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    pub fn reset_after_hardware(&mut self) {
        *self = Self::new();
    }

    fn slot_position(&self, slot: LayerStateSlot) -> u32 {
        match slot {
            LayerStateSlot::ShortConv(slot) => self.shortconv_next[slot as usize],
            LayerStateSlot::KvCache(slot) => self.kv_next[slot as usize],
        }
    }
}

impl Default for DecodeStateProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// Find an exact generated tensor; global tensors use `layer=None`.
pub fn tensor_descriptor(layer: Option<u8>, role: TensorRole) -> Option<NativeTensorDescriptor> {
    let layer = layer.unwrap_or(0xff);
    lfm25::generated::TENSORS
        .iter()
        .copied()
        .find(|tensor| tensor.layer == layer && tensor.role == role as u8)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PlanError {
    TokenOutOfRange,
    Contract,
    Overflow,
}

/// Exact byte range for one token row in the tied Q8_0 embedding tensor.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct EmbeddingRowPlan {
    pub token: u32,
    pub tensor_id: u16,
    pub native_offset: u32,
    pub native_bytes: u32,
}

impl EmbeddingRowPlan {
    pub fn new(token: u32) -> Result<Self, PlanError> {
        if token >= lfm25::MODEL_VOCABULARY_SIZE {
            return Err(PlanError::TokenOutOfRange);
        }
        let tensor = tied_embedding_tensor()?;
        let row_offset = token.checked_mul(Q8_ROW_BYTES).ok_or(PlanError::Overflow)?;
        let native_offset = tensor
            .native_offset
            .checked_add(row_offset)
            .ok_or(PlanError::Overflow)?;
        if row_offset + Q8_ROW_BYTES > tensor.native_bytes {
            return Err(PlanError::Contract);
        }
        Ok(Self {
            token,
            tensor_id: tensor.tensor_id,
            native_offset,
            native_bytes: Q8_ROW_BYTES,
        })
    }
}

/// Fixed 65,536-row tied output projection. Argmax accumulation belongs to hardware.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TiedLmHeadPlan {
    pub tensor_id: u16,
    pub native_offset: u32,
    pub rows: u32,
    pub row_bytes: u32,
}

impl TiedLmHeadPlan {
    pub fn new() -> Result<Self, PlanError> {
        let tensor = tied_embedding_tensor()?;
        if tensor.native_bytes != Q8_ROW_BYTES * lfm25::MODEL_VOCABULARY_SIZE {
            return Err(PlanError::Contract);
        }
        Ok(Self {
            tensor_id: tensor.tensor_id,
            native_offset: tensor.native_offset,
            rows: lfm25::MODEL_VOCABULARY_SIZE,
            row_bytes: Q8_ROW_BYTES,
        })
    }

    pub fn row_native_offset(self, row: u32) -> Option<u32> {
        if row >= self.rows {
            return None;
        }
        self.native_offset
            .checked_add(row.checked_mul(self.row_bytes)?)
    }
}

fn tied_embedding_tensor() -> Result<NativeTensorDescriptor, PlanError> {
    let tensor = tensor_descriptor(None, TensorRole::TokenEmbedding).ok_or(PlanError::Contract)?;
    if TensorFormat::from_raw(tensor.format) != Some(TensorFormat::Q8_0)
        || tensor.ggml_ne0 != lfm25::MODEL_HIDDEN_SIZE
        || tensor.ggml_ne1 != lfm25::MODEL_VOCABULARY_SIZE
        || tensor.flags & lfm25::TENSOR_FLAG_TIED_OUTPUT == 0
        || lfm25::generated::MODEL_SEAL.flags & lfm25::MODEL_FLAG_TIED_OUTPUT == 0
    {
        return Err(PlanError::Contract);
    }
    Ok(tensor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_99_op_hybrid_plan_and_state_slots() {
        let mut count = 0;
        let mut stateful = 0;
        for operation in DecodePlan::new() {
            if operation.ordinal == 0 {
                assert_eq!(operation.kind, DecodeOpKind::TokenEmbedding);
            } else if operation.ordinal == 97 {
                assert_eq!(operation.kind, DecodeOpKind::FinalRmsNorm);
            } else if operation.ordinal == 98 {
                assert_eq!(operation.kind, DecodeOpKind::TiedLmHeadArgmax);
            }
            if let Some(state) = operation.state {
                match stateful {
                    0 => assert_eq!(
                        (operation.layer, state),
                        (Some(0), LayerStateSlot::ShortConv(0))
                    ),
                    1 => assert_eq!(
                        (operation.layer, state),
                        (Some(1), LayerStateSlot::ShortConv(1))
                    ),
                    2 => {
                        assert_eq!((operation.layer, state), (Some(2), LayerStateSlot::KvCache(0)))
                    }
                    15 => assert_eq!(
                        (operation.layer, state),
                        (Some(15), LayerStateSlot::ShortConv(9))
                    ),
                    _ => {}
                }
                stateful += 1;
            }
            count += 1;
        }
        assert_eq!(count, 99);
        assert_eq!(stateful, lfm25::MODEL_LAYER_COUNT);
    }

    #[test]
    fn all_sixteen_layers_have_the_generated_state_mapping() {
        assert_eq!(
            [
                state_slot_for_layer(0).unwrap(),
                state_slot_for_layer(1).unwrap(),
                state_slot_for_layer(2).unwrap(),
                state_slot_for_layer(3).unwrap(),
                state_slot_for_layer(4).unwrap(),
                state_slot_for_layer(5).unwrap(),
                state_slot_for_layer(6).unwrap(),
                state_slot_for_layer(7).unwrap(),
                state_slot_for_layer(8).unwrap(),
                state_slot_for_layer(9).unwrap(),
                state_slot_for_layer(10).unwrap(),
                state_slot_for_layer(11).unwrap(),
                state_slot_for_layer(12).unwrap(),
                state_slot_for_layer(13).unwrap(),
                state_slot_for_layer(14).unwrap(),
                state_slot_for_layer(15).unwrap(),
            ],
            [
                LayerStateSlot::ShortConv(0),
                LayerStateSlot::ShortConv(1),
                LayerStateSlot::KvCache(0),
                LayerStateSlot::ShortConv(2),
                LayerStateSlot::ShortConv(3),
                LayerStateSlot::KvCache(1),
                LayerStateSlot::ShortConv(4),
                LayerStateSlot::ShortConv(5),
                LayerStateSlot::KvCache(2),
                LayerStateSlot::ShortConv(6),
                LayerStateSlot::KvCache(3),
                LayerStateSlot::ShortConv(7),
                LayerStateSlot::KvCache(4),
                LayerStateSlot::ShortConv(8),
                LayerStateSlot::KvCache(5),
                LayerStateSlot::ShortConv(9),
            ]
        );
        assert_eq!(state_slot_for_layer(16), None);
    }

    #[test]
    fn all_circuits_are_required_before_token_mutation() {
        assert_eq!(
            DecodePlan::require_capabilities(DecodeCapabilities::NONE),
            Err(MissingCapability {
                operation: DecodeOpKind::TokenEmbedding,
            })
        );
        let no_attention = DecodeCapabilities::from_bits(
            DecodeCapabilities::ALL.bits() & !DecodeOpKind::Attention.capability().bits(),
        );
        assert_eq!(
            DecodePlan::require_capabilities(no_attention),
            Err(MissingCapability {
                operation: DecodeOpKind::Attention,
            })
        );
        assert_eq!(DecodePlan::require_capabilities(DecodeCapabilities::ALL), Ok(()));
    }

    #[test]
    fn token_one_embedding_and_complete_tied_head_are_exact() {
        let embedding = EmbeddingRowPlan::new(1).unwrap();
        assert_eq!(embedding.tensor_id, 0);
        assert_eq!(embedding.native_offset, 0x440);
        assert_eq!(embedding.native_bytes, 0x440);
        assert_eq!(EmbeddingRowPlan::new(65_536), Err(PlanError::TokenOutOfRange));

        let head = TiedLmHeadPlan::new().unwrap();
        assert_eq!(head.tensor_id, embedding.tensor_id);
        assert_eq!(head.rows, 65_536);
        assert_eq!(head.row_native_offset(0), Some(0));
        assert_eq!(head.row_native_offset(65_535), Some(0x043f_fbc0));
        assert_eq!(head.row_native_offset(65_536), None);
        assert_eq!(
            head.row_native_offset(65_535).unwrap() + head.row_bytes,
            lfm25::generated::TENSORS[0].native_bytes
        );
        assert_eq!(TOKEN1_DECODE_TRACE_BYTES, 670_936);
        assert_eq!(TOKEN1_DECODE_INPUT_TOKEN, 1);
        assert_eq!(TOKEN1_DECODE_OUTPUT_TOKEN, 1);
    }

    #[test]
    fn every_generated_layer_has_exact_scheduler_tensors() {
        for layer in 0..lfm25::MODEL_LAYER_COUNT as u8 {
            for role in [
                TensorRole::OperatorNorm,
                TensorRole::FfnNorm,
                TensorRole::FfnGate,
                TensorRole::FfnUp,
                TensorRole::FfnDown,
            ] {
                assert!(
                    tensor_descriptor(Some(layer), role).is_some(),
                    "layer={layer} role={role:?}"
                );
            }
            match lfm25::LAYER_SCHEDULE[layer as usize] {
                LayerKind::ShortConv => {
                    for role in [
                        TensorRole::ShortConvKernel,
                        TensorRole::ShortConvInput,
                        TensorRole::ShortConvOutput,
                    ] {
                        assert!(tensor_descriptor(Some(layer), role).is_some());
                    }
                }
                LayerKind::Attention => {
                    for role in [
                        TensorRole::QueryNorm,
                        TensorRole::KeyNorm,
                        TensorRole::Query,
                        TensorRole::Key,
                        TensorRole::Value,
                        TensorRole::AttentionOutput,
                    ] {
                        assert!(tensor_descriptor(Some(layer), role).is_some());
                    }
                }
            }
        }
    }

    #[test]
    fn callbacks_advance_ten_recurrent_and_six_kv_states_transactionally() {
        let mut states = DecodeStateProgress::new();
        for token_position in 0..2 {
            for layer in 0..lfm25::MODEL_LAYER_COUNT as u8 {
                let slot = state_slot_for_layer(layer).unwrap();
                assert_eq!(states.require_current(slot), Ok(()));
                assert_eq!(states.commit_callback(slot, token_position), Ok(()));
            }
            assert_eq!(states.commit_token(), Ok(token_position + 1));
        }
        assert_eq!(states.position(), 2);

        let mut bad = DecodeStateProgress::new();
        assert!(bad.commit_callback(LayerStateSlot::KvCache(0), 1).is_err());
        assert!(bad.is_poisoned());
        assert_eq!(bad.commit_token(), Err(StateProgressError::IncompleteToken));
        bad.reset_after_hardware();
        assert_eq!(bad, DecodeStateProgress::new());
    }
}
