//! On-disk ABI constants and dependency-free SHA-256.

use core::convert::TryFrom;

pub const MAGIC: [u8; 8] = *b"KKAOTV1\0";
pub const VERSION: u16 = 1;
pub const LITTLE_ENDIAN_TAG: u16 = 0x4c45;
pub const HEADER_BYTES: usize = 352;
pub const SECTION_COUNT: usize = 6;
pub const PHASE_COUNT: usize = 2;
pub const SECTION_ENTRY_BYTES: usize = 32;
pub const SECTION_DIRECTORY_OFFSET: usize = 160;

pub const ARTIFACT_SHA256_OFFSET: usize = 64;
pub const MODEL_SHA256_OFFSET: usize = 96;
pub const VOICES_SHA256_OFFSET: usize = 128;
pub const SHA256_BYTES: usize = 32;

pub const TENSOR_RECORD_BYTES: usize = 128;
pub const SLOT_RECORD_BYTES: usize = 64;
pub const OP_RECORD_BYTES: usize = 40;
pub const PHASE_RECORD_BYTES: usize = 48;
pub const BINDING_RECORD_BYTES: usize = 4;
pub const ARENA_ALIGNMENT: u32 = 64;

pub const NO_TENSOR: u32 = u32::MAX;
pub const NO_SLOT: u32 = u32::MAX;
pub const STATIC_DIM: u8 = u8::MAX;
pub const UNRESOLVED_SLOT_BASE: u64 = u64::MAX;

#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SectionKind {
    Tensors = 1,
    Slots = 2,
    Ops = 3,
    Bindings = 4,
    Phases = 5,
    Data = 6,
}

impl SectionKind {
    pub const ALL: [Self; SECTION_COUNT] = [
        Self::Tensors,
        Self::Slots,
        Self::Ops,
        Self::Bindings,
        Self::Phases,
        Self::Data,
    ];

    pub const fn alignment(self) -> u32 {
        match self {
            Self::Tensors | Self::Slots | Self::Data => 16,
            Self::Ops | Self::Bindings | Self::Phases => 8,
        }
    }

    pub const fn stride(self) -> u32 {
        match self {
            Self::Tensors => TENSOR_RECORD_BYTES as u32,
            Self::Slots => SLOT_RECORD_BYTES as u32,
            Self::Ops => OP_RECORD_BYTES as u32,
            Self::Bindings => BINDING_RECORD_BYTES as u32,
            Self::Phases => PHASE_RECORD_BYTES as u32,
            Self::Data => 1,
        }
    }
}

impl TryFrom<u16> for SectionKind {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Tensors),
            2 => Ok(Self::Slots),
            3 => Ok(Self::Ops),
            4 => Ok(Self::Bindings),
            5 => Ok(Self::Phases),
            6 => Ok(Self::Data),
            _ => Err(()),
        }
    }
}

/// Every operation accepted by the v1 sealed-program parser.
///
/// Recognition here is a format contract only. This crate does not implement
/// any of these operations.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpCode {
    ResolveDecoderShape = 0x0001,

    Add = 0x0100,
    And = 0x0101,
    Atan = 0x0102,
    Cast = 0x0103,
    Clip = 0x0104,
    Concat = 0x0105,
    ConstantOfShape = 0x0106,
    Conv = 0x0107,
    ConvInteger = 0x0108,
    ConvTranspose = 0x0109,
    Cos = 0x010a,
    CumSum = 0x010b,
    DequantizeLinear = 0x010c,
    Div = 0x010d,
    DynamicQuantizeLinear = 0x010e,
    Equal = 0x010f,
    Exp = 0x0110,
    Expand = 0x0111,
    Floor = 0x0112,
    Gather = 0x0113,
    Greater = 0x0114,
    GreaterOrEqual = 0x0115,
    Lstm = 0x0116,
    LayerNormalization = 0x0117,
    LeakyRelu = 0x0118,
    Less = 0x0119,
    MatMul = 0x011a,
    MatMulInteger = 0x011b,
    Mul = 0x011c,
    NonZero = 0x011d,
    Pad = 0x011e,
    Pow = 0x011f,
    Range = 0x0120,
    ReduceMean = 0x0121,
    ReduceSum = 0x0122,
    Reshape = 0x0123,
    Resize = 0x0124,
    Round = 0x0125,
    Stft = 0x0126,
    ScatterNd = 0x0127,
    Shape = 0x0128,
    Sigmoid = 0x0129,
    Sin = 0x012a,
    Slice = 0x012b,
    Softmax = 0x012c,
    Split = 0x012d,
    Sqrt = 0x012e,
    Squeeze = 0x012f,
    Sub = 0x0130,
    Tanh = 0x0131,
    Transpose = 0x0132,
    Unsqueeze = 0x0133,
    Where = 0x0134,

    FastGelu = 0x0200,
    SkipLayerNormalization = 0x0201,

    DynamicQuantizedGemm = 0x0300,
    DynamicQuantizedConv1d = 0x0301,
    AddSoftmax = 0x0302,
    BiLstm256 = 0x0303,
    AlbertAttention = 0x0304,
    FloatConv1d = 0x0305,
    FloatConvTranspose1d = 0x0306,
    FixedStft20 = 0x0307,
    ElementwiseFusion = 0x0308,
}

impl TryFrom<u16> for OpCode {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        use OpCode::*;
        let opcode = match value {
            0x0001 => ResolveDecoderShape,
            0x0100 => Add,
            0x0101 => And,
            0x0102 => Atan,
            0x0103 => Cast,
            0x0104 => Clip,
            0x0105 => Concat,
            0x0106 => ConstantOfShape,
            0x0107 => Conv,
            0x0108 => ConvInteger,
            0x0109 => ConvTranspose,
            0x010a => Cos,
            0x010b => CumSum,
            0x010c => DequantizeLinear,
            0x010d => Div,
            0x010e => DynamicQuantizeLinear,
            0x010f => Equal,
            0x0110 => Exp,
            0x0111 => Expand,
            0x0112 => Floor,
            0x0113 => Gather,
            0x0114 => Greater,
            0x0115 => GreaterOrEqual,
            0x0116 => Lstm,
            0x0117 => LayerNormalization,
            0x0118 => LeakyRelu,
            0x0119 => Less,
            0x011a => MatMul,
            0x011b => MatMulInteger,
            0x011c => Mul,
            0x011d => NonZero,
            0x011e => Pad,
            0x011f => Pow,
            0x0120 => Range,
            0x0121 => ReduceMean,
            0x0122 => ReduceSum,
            0x0123 => Reshape,
            0x0124 => Resize,
            0x0125 => Round,
            0x0126 => Stft,
            0x0127 => ScatterNd,
            0x0128 => Shape,
            0x0129 => Sigmoid,
            0x012a => Sin,
            0x012b => Slice,
            0x012c => Softmax,
            0x012d => Split,
            0x012e => Sqrt,
            0x012f => Squeeze,
            0x0130 => Sub,
            0x0131 => Tanh,
            0x0132 => Transpose,
            0x0133 => Unsqueeze,
            0x0134 => Where,
            0x0200 => FastGelu,
            0x0201 => SkipLayerNormalization,
            0x0300 => DynamicQuantizedGemm,
            0x0301 => DynamicQuantizedConv1d,
            0x0302 => AddSoftmax,
            0x0303 => BiLstm256,
            0x0304 => AlbertAttention,
            0x0305 => FloatConv1d,
            0x0306 => FloatConvTranspose1d,
            0x0307 => FixedStft20,
            0x0308 => ElementwiseFusion,
            _ => return Err(()),
        };
        Ok(opcode)
    }
}

pub const OP_FLAG_IN_PLACE: u16 = 1;
pub const PHASE_FLAG_RUNTIME_SIZED: u8 = 1;

#[inline]
pub(crate) fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let value: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(value))
}

#[inline]
pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let value: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(value))
}

#[inline]
pub(crate) fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let value: [u8; 8] = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(value))
}

#[inline]
pub(crate) fn read_i64(bytes: &[u8], offset: usize) -> Option<i64> {
    let value: [u8; 8] = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(i64::from_le_bytes(value))
}

#[inline]
pub(crate) fn is_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

#[inline]
pub(crate) fn hash_eq(lhs: &[u8; 32], rhs: &[u8; 32]) -> bool {
    let mut difference = 0u8;
    for index in 0..32 {
        difference |= lhs[index] ^ rhs[index];
    }
    difference == 0
}

#[inline]
pub(crate) fn checked_align_up(value: u64, alignment: u32) -> Option<u64> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return None;
    }
    let mask = u64::from(alignment - 1);
    value.checked_add(mask).map(|aligned| aligned & !mask)
}

/// Compute SHA-256 without allocation or external dependencies.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finish()
}

/// Compute the v1 artifact seal.
///
/// Every byte is authenticated except the seal field itself, which is treated
/// as 32 zero bytes. This binds the provenance hashes and section directory as
/// well as the record payload.
pub fn artifact_sha256(artifact: &[u8]) -> Option<[u8; 32]> {
    if artifact.len() < ARTIFACT_SHA256_OFFSET + SHA256_BYTES {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(&artifact[..ARTIFACT_SHA256_OFFSET]);
    hasher.update(&[0; SHA256_BYTES]);
    hasher.update(&artifact[ARTIFACT_SHA256_OFFSET + SHA256_BYTES..]);
    Some(hasher.finish())
}

struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    block_len: usize,
    total_len: u64,
}

impl Sha256 {
    const fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            block: [0; 64],
            block_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.total_len = self.total_len.wrapping_add(input.len() as u64);
        if self.block_len != 0 {
            let take = core::cmp::min(64 - self.block_len, input.len());
            self.block[self.block_len..self.block_len + take].copy_from_slice(&input[..take]);
            self.block_len += take;
            input = &input[take..];
            if self.block_len == 64 {
                let block = self.block;
                self.compress(&block);
                self.block_len = 0;
            }
        }
        while input.len() >= 64 {
            let block: &[u8; 64] = input[..64].try_into().expect("fixed block");
            self.compress(block);
            input = &input[64..];
        }
        self.block[..input.len()].copy_from_slice(input);
        self.block_len = input.len();
    }

    fn finish(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);
        self.block[self.block_len] = 0x80;
        self.block_len += 1;
        if self.block_len > 56 {
            self.block[self.block_len..].fill(0);
            let block = self.block;
            self.compress(&block);
            self.block_len = 0;
        }
        self.block[self.block_len..56].fill(0);
        self.block[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.block;
        self.compress(&block);

        let mut output = [0u8; 32];
        for (chunk, word) in output.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        output
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];

        let mut schedule = [0u32; 64];
        for (index, chunk) in block.chunks_exact(4).enumerate() {
            schedule[index] = u32::from_be_bytes(chunk.try_into().expect("word"));
        }
        for index in 16..64 {
            let x = schedule[index - 15];
            let y = schedule[index - 2];
            let s0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let s1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}
