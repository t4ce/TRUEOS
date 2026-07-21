//! Sealed LFM2.5-350M model contract shared by the Ubuntu packer and FPGA RTL.
//!
//! GGUF is deliberately absent from this ABI. Ubuntu converts the pinned model into a
//! flat native tensor image; TRUEOS and future RTL consume only this fixed seal/table.

use core::mem::{align_of, size_of};

pub const MODEL_CONTRACT_MAGIC: [u8; 8] = *b"TGALFM25";
pub const MODEL_LAYOUT_VERSION: u16 = 1;
pub const MODEL_SEAL_BYTES: usize = 192;
pub const MODEL_TENSOR_DESCRIPTOR_BYTES: usize = 24;
pub const MODEL_TENSOR_ALIGNMENT: usize = 256;

pub const MODEL_TENSOR_COUNT: usize = 148;
pub const MODEL_LAYER_COUNT: usize = 16;
pub const MODEL_HIDDEN_SIZE: u32 = 1024;
pub const MODEL_FEED_FORWARD_SIZE: u32 = 4608;
pub const MODEL_VOCABULARY_SIZE: u32 = 65_536;
pub const MODEL_SOURCE_CONTEXT: u32 = 128_000;
pub const MODEL_INITIAL_CONTEXT: u32 = 16_384;
pub const MODEL_ATTENTION_HEADS: u16 = 16;
pub const MODEL_KV_HEADS: u16 = 8;
pub const MODEL_HEAD_DIMENSION: u16 = 64;
pub const MODEL_SHORTCONV_CACHE: u16 = 3;
pub const MODEL_ATTENTION_MASK: u16 = 0x5524;
pub const MODEL_GENERATION: u32 = 1;

pub const Q8_0_BLOCK_VALUES: usize = 32;
pub const Q8_0_BLOCK_BYTES: usize = 34;
pub const TENSOR_FLAG_TIED_OUTPUT: u16 = 1 << 0;
pub const MODEL_FLAG_TIED_OUTPUT: u32 = 1 << 0;

pub const PINNED_GGUF_BYTES: u32 = 379_217_632;
pub const PINNED_GGUF_SHA256: [u8; 32] = [
    0xbe, 0x03, 0x6a, 0x75, 0x72, 0x95, 0xe5, 0x50, 0x09, 0x8b, 0x85, 0xe1, 0x3f, 0x6a, 0xf2, 0x73,
    0x5d, 0x0f, 0xa7, 0x3b, 0x41, 0xe1, 0x15, 0x6a, 0x40, 0xc7, 0xd8, 0xe8, 0xe3, 0x2a, 0x57, 0x66,
];

pub const PINNED_NATIVE_IMAGE_BYTES: u32 = 0x1674_0400;
pub const PINNED_NATIVE_IMAGE_SHA256: [u8; 32] = [
    0x05, 0x1c, 0x60, 0x85, 0x67, 0x86, 0xde, 0x2a, 0xc7, 0x08, 0x91, 0x09, 0x35, 0x42, 0x59, 0xfa,
    0x29, 0xfc, 0xd5, 0x7e, 0x83, 0xd5, 0x85, 0xef, 0xc8, 0x6a, 0xfa, 0x0f, 0xb6, 0x05, 0xbb, 0x86,
];

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LayerKind {
    ShortConv = 0,
    Attention = 1,
}

impl LayerKind {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::ShortConv),
            1 => Some(Self::Attention),
            _ => None,
        }
    }
}

pub const LAYER_SCHEDULE: [LayerKind; MODEL_LAYER_COUNT] = [
    LayerKind::ShortConv,
    LayerKind::ShortConv,
    LayerKind::Attention,
    LayerKind::ShortConv,
    LayerKind::ShortConv,
    LayerKind::Attention,
    LayerKind::ShortConv,
    LayerKind::ShortConv,
    LayerKind::Attention,
    LayerKind::ShortConv,
    LayerKind::Attention,
    LayerKind::ShortConv,
    LayerKind::Attention,
    LayerKind::ShortConv,
    LayerKind::Attention,
    LayerKind::ShortConv,
];

pub const LAYER_SCHEDULE_BYTES: [u8; MODEL_LAYER_COUNT] =
    [0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0];

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TensorFormat {
    /// IEEE-754 BF16, little-endian, round-to-nearest-even from source F32.
    Bf16Le = 1,
    /// GGUF Q8_0: one little-endian FP16 scale followed by 32 signed quants.
    Q8_0 = 2,
}

impl TensorFormat {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::Bf16Le),
            2 => Some(Self::Q8_0),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TensorRole {
    TokenEmbedding = 0,
    TokenEmbeddingNorm = 1,
    FfnNorm = 2,
    FfnGate = 3,
    FfnDown = 4,
    FfnUp = 5,
    OperatorNorm = 6,
    ShortConvKernel = 7,
    ShortConvInput = 8,
    ShortConvOutput = 9,
    QueryNorm = 10,
    KeyNorm = 11,
    Query = 12,
    Key = 13,
    Value = 14,
    AttentionOutput = 15,
}

impl TensorRole {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::TokenEmbedding),
            1 => Some(Self::TokenEmbeddingNorm),
            2 => Some(Self::FfnNorm),
            3 => Some(Self::FfnGate),
            4 => Some(Self::FfnDown),
            5 => Some(Self::FfnUp),
            6 => Some(Self::OperatorNorm),
            7 => Some(Self::ShortConvKernel),
            8 => Some(Self::ShortConvInput),
            9 => Some(Self::ShortConvOutput),
            10 => Some(Self::QueryNorm),
            11 => Some(Self::KeyNorm),
            12 => Some(Self::Query),
            13 => Some(Self::Key),
            14 => Some(Self::Value),
            15 => Some(Self::AttentionOutput),
            _ => None,
        }
    }
}

/// Canonical 192-byte model seal at the start of the separate contract artifact.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ModelSeal {
    pub magic: [u8; 8],
    pub layout_version: u16,
    pub seal_bytes: u16,
    pub descriptor_bytes: u16,
    pub tensor_count: u16,
    pub flags: u32,
    pub model_generation: u32,
    pub tensor_alignment: u32,
    pub source_context: u32,
    pub initial_context: u32,
    pub hidden_size: u32,
    pub feed_forward_size: u32,
    pub vocabulary_size: u32,
    pub layer_count: u16,
    pub attention_heads: u16,
    pub kv_heads: u16,
    pub head_dimension: u16,
    pub shortconv_cache: u16,
    pub attention_mask: u16,
    pub source_gguf_bytes: u32,
    pub native_image_bytes: u32,
    pub source_gguf_sha256: [u8; 32],
    pub native_image_sha256: [u8; 32],
    pub tensor_table_sha256: [u8; 32],
    pub layer_schedule: [u8; MODEL_LAYER_COUNT],
    pub reserved: [u8; 12],
}

/// Compact FPGA sequencer record. Tensor names remain ordinary generated Rust metadata.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NativeTensorDescriptor {
    pub tensor_id: u16,
    /// `0xff` for the two global tensors, otherwise the transformer layer.
    pub layer: u8,
    pub role: u8,
    pub format: u8,
    pub rank: u8,
    pub flags: u16,
    /// GGML dimension zero: contiguous reduction width.
    pub ggml_ne0: u32,
    /// GGML dimension one, or one for vectors.
    pub ggml_ne1: u32,
    pub native_offset: u32,
    pub native_bytes: u32,
}

/// Exact generated tensor IDs, names, offsets, shapes, and model seal.
pub mod generated {
    include!("lfm25_generated.rs");
}

const _: [(); MODEL_SEAL_BYTES] = [(); size_of::<ModelSeal>()];
const _: [(); 4] = [(); align_of::<ModelSeal>()];
const _: [(); MODEL_TENSOR_DESCRIPTOR_BYTES] = [(); size_of::<NativeTensorDescriptor>()];
const _: [(); 4] = [(); align_of::<NativeTensorDescriptor>()];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_contract_layout() {
        assert_eq!(size_of::<ModelSeal>(), 192);
        assert_eq!(size_of::<NativeTensorDescriptor>(), 24);
        assert_eq!(core::mem::offset_of!(ModelSeal, source_gguf_bytes), 60);
        assert_eq!(core::mem::offset_of!(ModelSeal, source_gguf_sha256), 68);
        assert_eq!(core::mem::offset_of!(ModelSeal, layer_schedule), 164);
        assert_eq!(core::mem::offset_of!(NativeTensorDescriptor, tensor_id), 0);
        assert_eq!(core::mem::offset_of!(NativeTensorDescriptor, ggml_ne0), 8);
        assert_eq!(core::mem::offset_of!(NativeTensorDescriptor, native_offset), 16);
        assert_eq!(MODEL_SEAL_BYTES + MODEL_TENSOR_COUNT * MODEL_TENSOR_DESCRIPTOR_BYTES, 3744);
    }

    #[test]
    fn exact_hybrid_schedule() {
        assert_eq!(LAYER_SCHEDULE_BYTES, [0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0]);
        assert_eq!(MODEL_ATTENTION_MASK, 0x5524);
        assert_eq!(
            LAYER_SCHEDULE
                .iter()
                .filter(|kind| **kind == LayerKind::ShortConv)
                .count(),
            10
        );
        assert_eq!(
            LAYER_SCHEDULE
                .iter()
                .filter(|kind| **kind == LayerKind::Attention)
                .count(),
            6
        );
    }

    #[test]
    fn generated_model_is_the_exact_native_contract() {
        let seal = generated::MODEL_SEAL;
        assert_eq!(seal.magic, MODEL_CONTRACT_MAGIC);
        assert_eq!(seal.layout_version, MODEL_LAYOUT_VERSION);
        assert_eq!(seal.tensor_count as usize, MODEL_TENSOR_COUNT);
        assert_eq!(seal.attention_mask, MODEL_ATTENTION_MASK);
        assert_eq!(seal.layer_schedule, LAYER_SCHEDULE_BYTES);
        assert_eq!(seal.source_gguf_sha256, PINNED_GGUF_SHA256);
        assert_eq!(seal.native_image_sha256, PINNED_NATIVE_IMAGE_SHA256);
        assert_eq!(seal.native_image_bytes, PINNED_NATIVE_IMAGE_BYTES);

        let mut q8 = 0;
        let mut bf16 = 0;
        for (index, tensor) in generated::TENSORS.iter().enumerate() {
            assert_eq!(tensor.tensor_id as usize, index);
            assert_eq!(tensor.native_offset as usize % MODEL_TENSOR_ALIGNMENT, 0);
            if let Some(next) = generated::TENSORS.get(index + 1) {
                assert!(tensor.native_offset + tensor.native_bytes <= next.native_offset);
            }
            match TensorFormat::from_raw(tensor.format) {
                Some(TensorFormat::Q8_0) => {
                    q8 += 1;
                    assert_eq!(tensor.ggml_ne0 as usize % Q8_0_BLOCK_VALUES, 0);
                }
                Some(TensorFormat::Bf16Le) => bf16 += 1,
                None => panic!("invalid generated tensor format"),
            }
        }
        assert_eq!((q8, bf16), (93, 55));
        let last = generated::TENSORS[MODEL_TENSOR_COUNT - 1];
        let aligned_end = (last.native_offset + last.native_bytes + 255) & !255;
        assert_eq!(aligned_end, PINNED_NATIVE_IMAGE_BYTES);
        assert_eq!(generated::TENSOR_NAMES[0], "token_embd.weight");
        assert_eq!(
            generated::TENSOR_NAMES[MODEL_TENSOR_COUNT - 1],
            "blk.15.shortconv.out_proj.weight"
        );
    }
}
