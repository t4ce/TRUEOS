//! Fail-closed decoder for `trueos.kokoro-op-attributes.v1`.

use core::convert::TryFrom;

use trueos_kokoro_aot::OpCode;

pub const ATTRIBUTE_ABI_VERSION: u16 = 1;

const CHECKED_LAYOUT: u8 = 1;
const BINARY_FLAGS: u8 = 0b11;
const ROLE_CONTROL: u8 = 0b1_0000;
const ROLE_SCALE_ZERO: u8 = 0b110;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AttributeDType {
    Float = 1,
    Uint8 = 2,
    Int8 = 3,
    Int32 = 6,
    Int64 = 7,
    Bool = 9,
}

impl TryFrom<u8> for AttributeDType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Float),
            2 => Ok(Self::Uint8),
            3 => Ok(Self::Int8),
            6 => Ok(Self::Int32),
            7 => Ok(Self::Int64),
            9 => Ok(Self::Bool),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ControlMode {
    Absent = 0,
    Initializer = 1,
    Dynamic = 2,
}

impl TryFrom<u8> for ControlMode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Absent),
            1 => Ok(Self::Initializer),
            2 => Ok(Self::Dynamic),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ResizeMode {
    Nearest = 1,
    Linear = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BiasMode {
    None = 0,
    Float = 1,
    QuantizedInt32 = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractReason {
    Reserved,
    Rank,
    DType,
    Layout,
    Flags,
    Roles,
    Axis,
    Profile,
    Geometry,
    Parameters,
    Control,
    Provenance,
    FloatBits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeError {
    Truncated,
    UnalignedLength,
    UnsupportedVersion {
        found: u16,
    },
    KindMismatch {
        expected: OpCode,
        found: u16,
    },
    UnsupportedOpcode {
        opcode: OpCode,
    },
    ByteCountMismatch {
        header: u32,
        actual: usize,
    },
    WrongLength {
        opcode: OpCode,
        expected: usize,
        actual: usize,
    },
    Contract {
        opcode: OpCode,
        reason: ContractReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BinaryAttributes {
    pub opcode: OpCode,
    pub lhs_rank: u8,
    pub rhs_rank: u8,
    pub output_rank: u8,
    pub dtype: AttributeDType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComparisonAttributes {
    pub opcode: OpCode,
    pub lhs_rank: u8,
    pub rhs_rank: u8,
    pub output_rank: u8,
    pub input_dtype: AttributeDType,
    pub constant_roles: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CastAttributes {
    pub rank: u8,
    pub input_dtype: AttributeDType,
    pub output_dtype: AttributeDType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConstantOfShapeAttributes {
    pub fill_bits: u32,
    pub output_rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CumSumAttributes {
    pub axis: i32,
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DequantizeAttributes {
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WhereAttributes {
    pub condition_rank: u8,
    pub true_rank: u8,
    pub false_rank: u8,
    pub output_rank: u8,
    pub dtype: AttributeDType,
    pub constant_roles: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatMulAttributes {
    pub profile: u8,
    pub lhs_rank: u8,
    pub rhs_rank: u8,
    pub output_rank: u8,
    pub constant_roles: u8,
    pub k: u32,
    pub n: u32,
    pub lane: u32,
    pub frame_axis: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowAttributes {
    pub exponent_bits: u32,
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizeAttributes {
    pub profile: u8,
    pub mode: ResizeMode,
    pub scale: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BiLstmAttributes {
    pub profile: u16,
    pub constant_input_mask: u8,
    pub input_width: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FloatConvAttributes {
    pub opcode: OpCode,
    pub profile: u16,
    pub has_bias: bool,
    pub input_channels: u32,
    pub output_channels: u32,
    pub kernel: u32,
    pub stride: u32,
    pub dilation: u32,
    pub pad_left: u32,
    pub pad_right: u32,
    pub output_padding: u32,
    pub groups: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedStftAttributes {
    pub frame_length: u32,
    pub frame_step: u32,
    pub bins: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolveDecoderShapeAttributes {
    pub bins: u32,
    pub max_tokens: u32,
    pub source_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuantGemmAttributes {
    pub profile: u16,
    pub activation_rank: u8,
    pub bias_mode: BiasMode,
    pub k: u32,
    pub n: u32,
    pub source_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuantConvAttributes {
    pub profile: u16,
    pub bias_mode: BiasMode,
    pub input_channels: u32,
    pub output_channels: u32,
    pub kernel: u32,
    pub stride: u32,
    pub dilation: u32,
    pub pad_left: u32,
    pub pad_right: u32,
    pub groups: u32,
    pub weight_zero: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnaryAttributes {
    pub opcode: OpCode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeakyReluAttributes {
    pub alpha_bits: u32,
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReduceMeanAttributes {
    pub axis: i32,
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayerNormAttributes {
    pub axis: i32,
    pub epsilon_bits: u32,
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftmaxAttributes {
    pub axis: i32,
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FastGeluAttributes {
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkipLayerNormAttributes {
    pub epsilon_bits: u32,
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransposeAttributes {
    pub rank: u8,
    pub dtype: AttributeDType,
    pub permutation: [i32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GatherAttributes {
    pub axis: i32,
    pub data_rank: u8,
    pub indices_rank: u8,
    pub output_rank: u8,
    pub dtype: AttributeDType,
    pub control_mode: ControlMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConcatAttributes {
    pub axis: i32,
    pub rank: u8,
    pub dtype: AttributeDType,
    pub input_count: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitAttributes {
    pub axis: i32,
    pub rank: u8,
    pub first_axis_len: u32,
    pub second_axis_len: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpandAttributes {
    pub input_rank: u8,
    pub output_rank: u8,
    pub dtype: AttributeDType,
    pub control_mode: ControlMode,
    pub producer_opcode: u16,
    pub target_dims: [i32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShapeAttributes {
    pub input_rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SliceAttributes {
    pub rank: u8,
    pub dtype: AttributeDType,
    pub flags: u8,
    pub control_modes: [ControlMode; 4],
    pub control_values: [i64; 4],
    pub producer_opcodes: [u16; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PadAttributes {
    pub rank: u8,
    pub pads: [u32; 8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScatterNdAttributes;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewAttributes {
    pub opcode: OpCode,
    pub input_rank: u8,
    pub output_rank: u8,
    pub dtype: AttributeDType,
    pub static_control: bool,
    pub count: u8,
    pub parameters: [i32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Attributes {
    Binary(BinaryAttributes),
    Comparison(ComparisonAttributes),
    Cast(CastAttributes),
    ConstantOfShape(ConstantOfShapeAttributes),
    CumSum(CumSumAttributes),
    DequantizeLinear(DequantizeAttributes),
    Where(WhereAttributes),
    MatMul(MatMulAttributes),
    Pow(PowAttributes),
    Range,
    Resize(ResizeAttributes),
    BiLstm256(BiLstmAttributes),
    FloatConv(FloatConvAttributes),
    FixedStft20(FixedStftAttributes),
    ResolveDecoderShape(ResolveDecoderShapeAttributes),
    DynamicQuantizedGemm(QuantGemmAttributes),
    DynamicQuantizedConv1d(QuantConvAttributes),
    Unary(UnaryAttributes),
    LeakyRelu(LeakyReluAttributes),
    ReduceMean(ReduceMeanAttributes),
    LayerNormalization(LayerNormAttributes),
    Softmax(SoftmaxAttributes),
    FastGelu(FastGeluAttributes),
    SkipLayerNormalization(SkipLayerNormAttributes),
    Transpose(TransposeAttributes),
    Gather(GatherAttributes),
    Concat(ConcatAttributes),
    Split(SplitAttributes),
    Expand(ExpandAttributes),
    Shape(ShapeAttributes),
    Slice(SliceAttributes),
    Pad(PadAttributes),
    NonZero,
    ScatterNd(ScatterNdAttributes),
    View(ViewAttributes),
}

impl Attributes {
    pub const fn opcode(self) -> OpCode {
        match self {
            Self::Binary(value) => value.opcode,
            Self::Comparison(value) => value.opcode,
            Self::Cast(_) => OpCode::Cast,
            Self::ConstantOfShape(_) => OpCode::ConstantOfShape,
            Self::CumSum(_) => OpCode::CumSum,
            Self::DequantizeLinear(_) => OpCode::DequantizeLinear,
            Self::Where(_) => OpCode::Where,
            Self::MatMul(_) => OpCode::MatMul,
            Self::Pow(_) => OpCode::Pow,
            Self::Range => OpCode::Range,
            Self::Resize(_) => OpCode::Resize,
            Self::BiLstm256(_) => OpCode::BiLstm256,
            Self::FloatConv(value) => value.opcode,
            Self::FixedStft20(_) => OpCode::FixedStft20,
            Self::ResolveDecoderShape(_) => OpCode::ResolveDecoderShape,
            Self::DynamicQuantizedGemm(_) => OpCode::DynamicQuantizedGemm,
            Self::DynamicQuantizedConv1d(_) => OpCode::DynamicQuantizedConv1d,
            Self::Unary(value) => value.opcode,
            Self::LeakyRelu(_) => OpCode::LeakyRelu,
            Self::ReduceMean(_) => OpCode::ReduceMean,
            Self::LayerNormalization(_) => OpCode::LayerNormalization,
            Self::Softmax(_) => OpCode::Softmax,
            Self::FastGelu(_) => OpCode::FastGelu,
            Self::SkipLayerNormalization(_) => OpCode::SkipLayerNormalization,
            Self::Transpose(_) => OpCode::Transpose,
            Self::Gather(_) => OpCode::Gather,
            Self::Concat(_) => OpCode::Concat,
            Self::Split(_) => OpCode::Split,
            Self::Expand(_) => OpCode::Expand,
            Self::Shape(_) => OpCode::Shape,
            Self::Slice(_) => OpCode::Slice,
            Self::Pad(_) => OpCode::Pad,
            Self::NonZero => OpCode::NonZero,
            Self::ScatterNd(_) => OpCode::ScatterNd,
            Self::View(value) => value.opcode,
        }
    }
}

/// Decode one fixed-size v1 record and bind it to the operation descriptor.
pub fn decode(record: &[u8], expected_opcode: OpCode) -> Result<Attributes, AttributeError> {
    if record.len() < 8 {
        return Err(AttributeError::Truncated);
    }
    if !record.len().is_multiple_of(4) {
        return Err(AttributeError::UnalignedLength);
    }
    let version = u16_at(record, 0);
    if version != ATTRIBUTE_ABI_VERSION {
        return Err(AttributeError::UnsupportedVersion { found: version });
    }
    let kind = u16_at(record, 2);
    if kind != expected_opcode as u16 {
        return Err(AttributeError::KindMismatch {
            expected: expected_opcode,
            found: kind,
        });
    }
    let header_bytes = u32_at(record, 4);
    if usize::try_from(header_bytes).ok() != Some(record.len()) {
        return Err(AttributeError::ByteCountMismatch {
            header: header_bytes,
            actual: record.len(),
        });
    }
    let expected_len = record_bytes(expected_opcode).ok_or(AttributeError::UnsupportedOpcode {
        opcode: expected_opcode,
    })?;
    if record.len() != expected_len {
        return Err(AttributeError::WrongLength {
            opcode: expected_opcode,
            expected: expected_len,
            actual: record.len(),
        });
    }

    decode_body(record, expected_opcode)
}

pub const fn record_bytes(opcode: OpCode) -> Option<usize> {
    use OpCode::*;
    match opcode {
        Add
        | Mul
        | Div
        | Sub
        | LeakyRelu
        | Softmax
        | FastGelu
        | SkipLayerNormalization
        | Cast
        | NonZero => Some(16),
        Atan | Cos | Exp | Floor | Round | Sigmoid | Sin | Sqrt | Tanh => Some(8),
        ReduceMean | Gather | Concat | And | Equal | Greater | GreaterOrEqual | Less
        | ConstantOfShape | CumSum | DequantizeLinear | Where | Pow | Range | ScatterNd => Some(20),
        LayerNormalization | Shape | Resize => Some(24),
        Split => Some(28),
        Reshape | Squeeze | Unsqueeze | Transpose | Expand | FixedStft20 | MatMul => Some(32),
        BiLstm256 | ResolveDecoderShape | DynamicQuantizedGemm => Some(40),
        Pad => Some(48),
        FloatConv1d | FloatConvTranspose1d => Some(56),
        DynamicQuantizedConv1d => Some(60),
        Slice => Some(64),
        Clip
        | Conv
        | ConvInteger
        | ConvTranspose
        | DynamicQuantizeLinear
        | Lstm
        | MatMulInteger
        | ReduceSum
        | Stft
        | AddSoftmax
        | AlbertAttention
        | ElementwiseFusion => None,
    }
}

fn fail(opcode: OpCode, reason: ContractReason) -> AttributeError {
    AttributeError::Contract { opcode, reason }
}

fn dtype(value: u8, opcode: OpCode) -> Result<AttributeDType, AttributeError> {
    AttributeDType::try_from(value).map_err(|_| fail(opcode, ContractReason::DType))
}

fn decode_body(record: &[u8], opcode: OpCode) -> Result<Attributes, AttributeError> {
    use OpCode::*;
    match opcode {
        Add | Mul | Div | Sub => decode_binary(record, opcode),
        And | Equal | Greater | GreaterOrEqual | Less => decode_comparison(record, opcode),
        Cast => decode_cast(record),
        ConstantOfShape => decode_constant_of_shape(record),
        CumSum => decode_cumsum(record),
        DequantizeLinear => decode_dequantize(record),
        Where => decode_where(record),
        MatMul => decode_matmul(record),
        Pow => decode_pow(record),
        Range => decode_range(record),
        Resize => decode_resize(record),
        BiLstm256 => decode_bilstm(record),
        FloatConv1d | FloatConvTranspose1d => decode_float_conv(record, opcode),
        FixedStft20 => decode_fixed_stft(record),
        ResolveDecoderShape => decode_resolve(record),
        DynamicQuantizedGemm => decode_quant_gemm(record),
        DynamicQuantizedConv1d => decode_quant_conv(record),
        Atan | Cos | Exp | Floor | Round | Sigmoid | Sin | Sqrt | Tanh => {
            Ok(Attributes::Unary(UnaryAttributes { opcode }))
        }
        LeakyRelu => decode_leaky_relu(record),
        ReduceMean => decode_reduce_mean(record),
        LayerNormalization => decode_layer_norm(record),
        Softmax => decode_softmax(record),
        FastGelu => decode_fast_gelu(record),
        SkipLayerNormalization => decode_skip_layer_norm(record),
        Transpose => decode_transpose(record),
        Gather => decode_gather(record),
        Concat => decode_concat(record),
        Split => decode_split(record),
        Expand => decode_expand(record),
        Shape => decode_shape(record),
        Slice => decode_slice(record),
        Pad => decode_pad(record),
        NonZero => decode_nonzero(record),
        ScatterNd => decode_scatter(record),
        Reshape | Squeeze | Unsqueeze => decode_view(record, opcode),
        Clip
        | Conv
        | ConvInteger
        | ConvTranspose
        | DynamicQuantizeLinear
        | Lstm
        | MatMulInteger
        | ReduceSum
        | Stft
        | AddSoftmax
        | AlbertAttention
        | ElementwiseFusion => Err(AttributeError::UnsupportedOpcode { opcode }),
    }
}

fn decode_binary(record: &[u8], opcode: OpCode) -> Result<Attributes, AttributeError> {
    let lhs_rank = record[8];
    let rhs_rank = record[9];
    let output_rank = record[10];
    if record[15] != 0 {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if lhs_rank > 4 || rhs_rank > 4 || output_rank != lhs_rank.max(rhs_rank) {
        return Err(fail(opcode, ContractReason::Rank));
    }
    let value_dtype = dtype(record[14], opcode)?;
    if !matches!(value_dtype, AttributeDType::Float | AttributeDType::Int64)
        || (opcode != OpCode::Add && value_dtype != AttributeDType::Float)
    {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[11] != CHECKED_LAYOUT || record[12] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[13] != BINARY_FLAGS {
        return Err(fail(opcode, ContractReason::Flags));
    }
    Ok(Attributes::Binary(BinaryAttributes {
        opcode,
        lhs_rank,
        rhs_rank,
        output_rank,
        dtype: value_dtype,
    }))
}

fn decode_comparison(record: &[u8], opcode: OpCode) -> Result<Attributes, AttributeError> {
    if !bytes_zero(record, 16, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    let lhs_rank = record[8];
    let rhs_rank = record[9];
    let output_rank = record[10];
    if lhs_rank > 4 || rhs_rank > 4 || output_rank != lhs_rank.max(rhs_rank) {
        return Err(fail(opcode, ContractReason::Rank));
    }
    let input_dtype = dtype(record[11], opcode)?;
    let output_dtype = dtype(record[12], opcode)?;
    let accepted_input = if opcode == OpCode::And {
        input_dtype == AttributeDType::Bool
    } else {
        matches!(input_dtype, AttributeDType::Float | AttributeDType::Int64)
    };
    if !accepted_input || output_dtype != AttributeDType::Bool {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[13] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[14] != BINARY_FLAGS {
        return Err(fail(opcode, ContractReason::Flags));
    }
    if record[15] & !0b11 != 0 {
        return Err(fail(opcode, ContractReason::Roles));
    }
    Ok(Attributes::Comparison(ComparisonAttributes {
        opcode,
        lhs_rank,
        rhs_rank,
        output_rank,
        input_dtype,
        constant_roles: record[15],
    }))
}

fn decode_cast(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Cast;
    if !bytes_zero(record, 14, 16) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    let rank = record[8];
    if rank > 4 || record[9] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    let input_dtype = dtype(record[10], opcode)?;
    let output_dtype = dtype(record[11], opcode)?;
    if !matches!(
        (input_dtype, output_dtype),
        (AttributeDType::Float, AttributeDType::Bool)
            | (AttributeDType::Int64, AttributeDType::Float)
            | (AttributeDType::Bool, AttributeDType::Float)
    ) {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[12] != 1 {
        return Err(fail(opcode, ContractReason::Flags));
    }
    if record[13] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    Ok(Attributes::Cast(CastAttributes {
        rank,
        input_dtype,
        output_dtype,
    }))
}

fn decode_constant_of_shape(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::ConstantOfShape;
    if !bytes_zero(record, 17, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    let fill_bits = u32_at(record, 8);
    if fill_bits != 0 && fill_bits != 1.0_f32.to_bits() {
        return Err(fail(opcode, ContractReason::FloatBits));
    }
    if record[12] != 1 || !matches!(record[13], 2 | 3) {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[14] != AttributeDType::Float as u8 {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[15] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[16] != ROLE_CONTROL {
        return Err(fail(opcode, ContractReason::Roles));
    }
    Ok(Attributes::ConstantOfShape(ConstantOfShapeAttributes {
        fill_bits,
        output_rank: record[13],
    }))
}

fn decode_cumsum(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::CumSum;
    if record[19] != 0 {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if i32_at(record, 8) != 1 {
        return Err(fail(opcode, ContractReason::Axis));
    }
    if record[12] != 3 || record[13] != 3 {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[14] != 1 {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[15] != 0 || record[16] != 0 {
        return Err(fail(opcode, ContractReason::Flags));
    }
    if record[17] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[18] != 0b10 {
        return Err(fail(opcode, ContractReason::Roles));
    }
    Ok(Attributes::CumSum(CumSumAttributes { axis: 1, rank: 3 }))
}

fn decode_dequantize(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::DequantizeLinear;
    if !bytes_zero(record, 16, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if record[8] != 3 || record[9] != 3 || record[12] != 0 || record[13] != 0 {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[10] != 3 || record[11] != 1 {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[14] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[15] != ROLE_SCALE_ZERO {
        return Err(fail(opcode, ContractReason::Roles));
    }
    Ok(Attributes::DequantizeLinear(DequantizeAttributes { rank: 3 }))
}

fn decode_where(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Where;
    if !bytes_zero(record, 18, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    let condition_rank = record[8];
    let true_rank = record[9];
    let false_rank = record[10];
    let output_rank = record[11];
    if condition_rank > 4
        || true_rank > 4
        || false_rank > 4
        || output_rank != condition_rank.max(true_rank).max(false_rank)
    {
        return Err(fail(opcode, ContractReason::Rank));
    }
    let value_dtype = dtype(record[13], opcode)?;
    if record[12] != AttributeDType::Bool as u8
        || !matches!(value_dtype, AttributeDType::Float | AttributeDType::Int64)
        || record[14] != value_dtype as u8
    {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[15] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[16] != BINARY_FLAGS {
        return Err(fail(opcode, ContractReason::Flags));
    }
    if record[17] & !0b11 != 0 {
        return Err(fail(opcode, ContractReason::Roles));
    }
    Ok(Attributes::Where(WhereAttributes {
        condition_rank,
        true_rank,
        false_rank,
        output_rank,
        dtype: value_dtype,
        constant_roles: record[17],
    }))
}

fn decode_matmul(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::MatMul;
    let profile = record[8];
    let lhs_rank = record[9];
    let rhs_rank = record[10];
    let output_rank = record[11];
    let roles = record[14];
    let k = u32_at(record, 16);
    let n = u32_at(record, 20);
    let lane = u32_at(record, 24);
    let frame_axis = u32_at(record, 28);
    if record[15] != 0 {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if record[12] != 1 {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[13] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    let expected = match profile {
        1 => (4, 4, 4, 0, 64, 0, 64, 2),
        2 => (4, 4, 4, 0, 0, 64, 64, 2),
        3 => (3, 2, 3, 0, 640, 512, 1, 1),
        4 => (3, 2, 3, 0, 512, 512, 1, 1),
        5 => (3, 2, 3, 2, 9, 1, 1, 1),
        _ => return Err(fail(opcode, ContractReason::Profile)),
    };
    if (lhs_rank, rhs_rank, output_rank, roles, k, n, lane, frame_axis) != expected {
        return Err(fail(opcode, ContractReason::Geometry));
    }
    Ok(Attributes::MatMul(MatMulAttributes {
        profile,
        lhs_rank,
        rhs_rank,
        output_rank,
        constant_roles: roles,
        k,
        n,
        lane,
        frame_axis,
    }))
}

fn decode_pow(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Pow;
    if !bytes_zero(record, 18, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    let exponent_bits = u32_at(record, 8);
    if exponent_bits != 2.0_f32.to_bits() {
        return Err(fail(opcode, ContractReason::FloatBits));
    }
    if record[12] != 3 || record[13] != 3 || record[15] != 0 {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[14] != 1 {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[16] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[17] != 0b10 {
        return Err(fail(opcode, ContractReason::Roles));
    }
    Ok(Attributes::Pow(PowAttributes {
        exponent_bits,
        rank: 3,
    }))
}

fn decode_range(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Range;
    if record[15] != 0 || !bytes_zero(record, 16, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if record[8..12] != [0, 0, 0, 1] {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[12] != 7 {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[13] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[14] != 0b101 {
        return Err(fail(opcode, ContractReason::Roles));
    }
    Ok(Attributes::Range)
}

fn decode_resize(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Resize;
    let profile = record[8];
    if record[15] != 0 || u32_at(record, 20) != 0 {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if record[9] != 3 || record[10] != 3 {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[11] != 1 {
        return Err(fail(opcode, ContractReason::DType));
    }
    let mode = match record[12] {
        1 => ResizeMode::Nearest,
        2 => ResizeMode::Linear,
        _ => return Err(fail(opcode, ContractReason::Parameters)),
    };
    if record[13] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[14] != 0b10 {
        return Err(fail(opcode, ContractReason::Roles));
    }
    let scale = u32_at(record, 16);
    let expected = match profile {
        1 => (ResizeMode::Nearest, 2),
        2 => (ResizeMode::Nearest, 300),
        3 | 4 => (ResizeMode::Linear, 300),
        _ => return Err(fail(opcode, ContractReason::Profile)),
    };
    if (mode, scale) != expected {
        return Err(fail(opcode, ContractReason::Profile));
    }
    Ok(Attributes::Resize(ResizeAttributes {
        profile,
        mode,
        scale,
    }))
}

fn decode_bilstm(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::BiLstm256;
    let profile = u16_at(record, 8);
    let constant_input_mask = record[18];
    let input_width = u32_at(record, 24);
    if !(1..=6).contains(&profile) {
        return Err(fail(opcode, ContractReason::Profile));
    }
    if record[10..18] != [3, 4, 3, 1, 1, 2, 6, 3]
        || constant_input_mask != if profile == 1 { 0b111110 } else { 0b001110 }
        || record[19] != CHECKED_LAYOUT
    {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    if u32_at(record, 20) != 256
        || !matches!(input_width, 512 | 640)
        || u32_at(record, 28) != 4
        || u32_at(record, 32) != 0
        || u32_at(record, 36) != 0
    {
        return Err(fail(opcode, ContractReason::Geometry));
    }
    Ok(Attributes::BiLstm256(BiLstmAttributes {
        profile,
        constant_input_mask,
        input_width,
    }))
}

fn decode_float_conv(record: &[u8], opcode: OpCode) -> Result<Attributes, AttributeError> {
    let profile = u16_at(record, 8);
    let expected_kind = if opcode == OpCode::FloatConv1d { 1 } else { 2 };
    let has_bias = record[16];
    if !(1..=6).contains(&profile) {
        return Err(fail(opcode, ContractReason::Profile));
    }
    if record[10] != expected_kind
        || record[11] != 3
        || record[12] != 3
        || record[13] != has_bias
        || record[14] != 3
        || record[15] != AttributeDType::Float as u8
        || !matches!(has_bias, 0 | 1)
        || record[17] != CHECKED_LAYOUT
        || record[18] != if has_bias == 1 { 0b110 } else { 0b010 }
        || record[19] != 0
    {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    let input_channels = u32_at(record, 20);
    let output_channels = u32_at(record, 24);
    let kernel = u32_at(record, 28);
    let stride = u32_at(record, 32);
    let dilation = u32_at(record, 36);
    let pad_left = u32_at(record, 40);
    let pad_right = u32_at(record, 44);
    let output_padding = u32_at(record, 48);
    let groups = u32_at(record, 52);
    if input_channels == 0
        || output_channels == 0
        || kernel == 0
        || stride == 0
        || dilation == 0
        || groups == 0
    {
        return Err(fail(opcode, ContractReason::Geometry));
    }
    Ok(Attributes::FloatConv(FloatConvAttributes {
        opcode,
        profile,
        has_bias: has_bias == 1,
        input_channels,
        output_channels,
        kernel,
        stride,
        dilation,
        pad_left,
        pad_right,
        output_padding,
        groups,
    }))
}

fn decode_fixed_stft(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::FixedStft20;
    if u16_at(record, 8) != 1
        || record[10..20] != [2, 4, 1, 1, 1, CHECKED_LAYOUT, 0b1110, 4, 0, 0]
        || u32_at(record, 20) != 20
        || u32_at(record, 24) != 5
        || u32_at(record, 28) != 11
    {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::FixedStft20(FixedStftAttributes {
        frame_length: 20,
        frame_step: 5,
        bins: 11,
    }))
}

fn decode_resolve(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::ResolveDecoderShape;
    if u16_at(record, 8) != 1
        || record[10..20] != [3, 1, 1, 0, 1, 7, CHECKED_LAYOUT, 2, 2, 0]
        || u32_at(record, 20) != 50
        || u32_at(record, 24) != 512
        || u32_at(record, 28) != 0x1ff
        || u32_at(record, 32) != 1.0_f32.to_bits()
        || u32_at(record, 36) != 9
    {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::ResolveDecoderShape(ResolveDecoderShapeAttributes {
        bins: 50,
        max_tokens: 512,
        source_count: 9,
    }))
}

fn decode_quant_gemm(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::DynamicQuantizedGemm;
    let profile = u16_at(record, 8);
    let activation_rank = record[10];
    let bias_mode = match record[18] {
        0 => BiasMode::None,
        1 => BiasMode::Float,
        _ => return Err(fail(opcode, ContractReason::Parameters)),
    };
    let expected_roles = if bias_mode == BiasMode::None {
        0b01110
    } else {
        0b11110
    };
    let k = u32_at(record, 20);
    let n = u32_at(record, 24);
    let source_count = u32_at(record, 28);
    if profile == 0
        || profile > 148
        || !matches!(activation_rank, 2 | 3)
        || record[11] != 2
        || record[12] != activation_rank
        || record[13..18] != [1, 3, 1, 1, 1]
        || record[19] != expected_roles
        || source_count != if bias_mode == BiasMode::None { 5 } else { 6 }
        || k == 0
        || n == 0
        || u32_at(record, 32) != 0
        || u32_at(record, 36) != if bias_mode == BiasMode::None { 0 } else { 1 }
    {
        return Err(fail(opcode, ContractReason::Geometry));
    }
    Ok(Attributes::DynamicQuantizedGemm(QuantGemmAttributes {
        profile,
        activation_rank,
        bias_mode,
        k,
        n,
        source_count,
    }))
}

fn decode_quant_conv(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::DynamicQuantizedConv1d;
    let profile = u16_at(record, 8);
    let bias_mode = match record[19] {
        0 => BiasMode::None,
        2 => BiasMode::QuantizedInt32,
        _ => return Err(fail(opcode, ContractReason::Parameters)),
    };
    let expected_bias_rank = if bias_mode == BiasMode::None { 0 } else { 1 };
    let expected_roles = if bias_mode == BiasMode::None {
        0b01110
    } else {
        0b11110
    };
    let expected_sources = if bias_mode == BiasMode::None { 5 } else { 10 };
    if profile == 0
        || profile > 87
        || record[10..13] != [3, 3, 3]
        || record[13..15] != [0, 0]
        || record[15] != expected_bias_rank
        || record[16..19] != [1, 2, 1]
        || record[20] != CHECKED_LAYOUT
        || record[21] != expected_roles
        || record[22] != expected_sources
        || record[23] != 0
    {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    let input_channels = u32_at(record, 24);
    let output_channels = u32_at(record, 28);
    let kernel = u32_at(record, 32);
    let stride = u32_at(record, 36);
    let dilation = u32_at(record, 40);
    let pad_left = u32_at(record, 44);
    let pad_right = u32_at(record, 48);
    let groups = u32_at(record, 52);
    let weight_zero = u32_at(record, 56);
    if input_channels == 0
        || output_channels == 0
        || kernel == 0
        || stride == 0
        || dilation == 0
        || groups == 0
        || weight_zero > u8::MAX as u32
    {
        return Err(fail(opcode, ContractReason::Geometry));
    }
    Ok(Attributes::DynamicQuantizedConv1d(QuantConvAttributes {
        profile,
        bias_mode,
        input_channels,
        output_channels,
        kernel,
        stride,
        dilation,
        pad_left,
        pad_right,
        groups,
        weight_zero,
    }))
}

fn decode_leaky_relu(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::LeakyRelu;
    let alpha_bits = u32_at(record, 8);
    let alpha = f32::from_bits(alpha_bits);
    let rank = record[12];
    if !(alpha > 0.0 && alpha.is_finite()) {
        return Err(fail(opcode, ContractReason::FloatBits));
    }
    if rank > 4 || record[13] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[14] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[15] != 0 {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    Ok(Attributes::LeakyRelu(LeakyReluAttributes { alpha_bits, rank }))
}

fn decode_reduce_mean(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::ReduceMean;
    let axis = i32_at(record, 8);
    let rank = record[14];
    if !bytes_zero(record, 17, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if rank == 0 || rank > 4 || record[15] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if axis < -i32::from(rank) || axis >= i32::from(rank) {
        return Err(fail(opcode, ContractReason::Axis));
    }
    if record[12] != 1 || record[13] != 0 || record[16] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Flags));
    }
    Ok(Attributes::ReduceMean(ReduceMeanAttributes { axis, rank }))
}

fn decode_layer_norm(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::LayerNormalization;
    let axis = i32_at(record, 8);
    let epsilon_bits = u32_at(record, 12);
    let epsilon = f32::from_bits(epsilon_bits);
    let rank = record[20];
    if rank == 0 || rank > 4 || record[21] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if axis < -i32::from(rank) || axis >= i32::from(rank) {
        return Err(fail(opcode, ContractReason::Axis));
    }
    if !(epsilon > 0.0 && epsilon.is_finite()) {
        return Err(fail(opcode, ContractReason::FloatBits));
    }
    if u32_at(record, 16) != 1 || record[22] != 1 || record[23] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::LayerNormalization(LayerNormAttributes {
        axis,
        epsilon_bits,
        rank,
    }))
}

fn decode_softmax(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Softmax;
    let axis = i32_at(record, 8);
    let rank = record[12];
    if rank == 0 || rank > 4 || record[13] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if axis < -i32::from(rank) || axis >= i32::from(rank) {
        return Err(fail(opcode, ContractReason::Axis));
    }
    if record[14] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    if record[15] != 0 {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    Ok(Attributes::Softmax(SoftmaxAttributes { axis, rank }))
}

fn decode_fast_gelu(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::FastGelu;
    let rank = record[9];
    if !bytes_zero(record, 13, 16) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if rank == 0 || rank > 4 || record[10] != rank || record[11] != 1 {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[8] != 1 || record[12] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::FastGelu(FastGeluAttributes { rank }))
}

fn decode_skip_layer_norm(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::SkipLayerNormalization;
    let epsilon_bits = u32_at(record, 8);
    let epsilon = f32::from_bits(epsilon_bits);
    let rank = record[12];
    if !(epsilon > 0.0 && epsilon.is_finite()) {
        return Err(fail(opcode, ContractReason::FloatBits));
    }
    if rank == 0 || rank > 4 || record[13] != rank || record[14] != 1 {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[15] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    Ok(Attributes::SkipLayerNormalization(SkipLayerNormAttributes { epsilon_bits, rank }))
}

fn decode_transpose(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Transpose;
    let rank = record[8];
    let value_dtype = dtype(record[10], opcode)?;
    if !bytes_zero(record, 13, 16) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if rank == 0 || rank > 4 || record[9] != rank || record[12] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if !matches!(value_dtype, AttributeDType::Float | AttributeDType::Int64) {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[11] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    let permutation = [
        i32_at(record, 16),
        i32_at(record, 20),
        i32_at(record, 24),
        i32_at(record, 28),
    ];
    for index in 0..usize::from(rank) {
        let axis = permutation[index];
        if axis < 0 || axis >= i32::from(rank) {
            return Err(fail(opcode, ContractReason::Parameters));
        }
        if permutation[..index].contains(&axis) {
            return Err(fail(opcode, ContractReason::Parameters));
        }
    }
    if permutation[usize::from(rank)..]
        .iter()
        .any(|&axis| axis != 0)
    {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    Ok(Attributes::Transpose(TransposeAttributes {
        rank,
        dtype: value_dtype,
        permutation,
    }))
}

fn decode_gather(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Gather;
    let axis = i32_at(record, 8);
    let data_rank = record[12];
    let indices_rank = record[13];
    let output_rank = record[14];
    let value_dtype = dtype(record[15], opcode)?;
    let control_mode =
        ControlMode::try_from(record[16]).map_err(|_| fail(opcode, ContractReason::Control))?;
    if !bytes_zero(record, 18, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if data_rank == 0
        || data_rank > 4
        || indices_rank > 4
        || output_rank != data_rank - 1 + indices_rank
        || output_rank > 4
    {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if axis < -i32::from(data_rank) || axis >= i32::from(data_rank) {
        return Err(fail(opcode, ContractReason::Axis));
    }
    if !matches!(value_dtype, AttributeDType::Float | AttributeDType::Int8 | AttributeDType::Int64)
    {
        return Err(fail(opcode, ContractReason::DType));
    }
    if !matches!(control_mode, ControlMode::Initializer | ControlMode::Dynamic) {
        return Err(fail(opcode, ContractReason::Control));
    }
    if record[17] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    Ok(Attributes::Gather(GatherAttributes {
        axis,
        data_rank,
        indices_rank,
        output_rank,
        dtype: value_dtype,
        control_mode,
    }))
}

fn decode_concat(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Concat;
    let axis = i32_at(record, 8);
    let rank = record[12];
    let value_dtype = dtype(record[14], opcode)?;
    let input_count = record[15];
    if !bytes_zero(record, 17, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if rank == 0 || rank > 4 || record[13] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if axis < -i32::from(rank) || axis >= i32::from(rank) {
        return Err(fail(opcode, ContractReason::Axis));
    }
    if !matches!(value_dtype, AttributeDType::Float | AttributeDType::Int64) {
        return Err(fail(opcode, ContractReason::DType));
    }
    if !(2..=4).contains(&input_count) || record[16] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::Concat(ConcatAttributes {
        axis,
        rank,
        dtype: value_dtype,
        input_count,
    }))
}

fn decode_split(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Split;
    let axis = i32_at(record, 8);
    let first_axis_len = u32_at(record, 12);
    let second_axis_len = u32_at(record, 16);
    let rank = record[20];
    if !bytes_zero(record, 25, 28) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if rank == 0 || rank > 4 || record[21] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if axis < -i32::from(rank) || axis >= i32::from(rank) {
        return Err(fail(opcode, ContractReason::Axis));
    }
    if first_axis_len == 0
        || second_axis_len == 0
        || record[22] != AttributeDType::Float as u8
        || record[23] != 2
        || record[24] != CHECKED_LAYOUT
    {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::Split(SplitAttributes {
        axis,
        rank,
        first_axis_len,
        second_axis_len,
    }))
}

fn decode_expand(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Expand;
    let input_rank = record[8];
    let output_rank = record[9];
    let value_dtype = dtype(record[10], opcode)?;
    let control_mode =
        ControlMode::try_from(record[11]).map_err(|_| fail(opcode, ContractReason::Control))?;
    let producer_opcode = u16_at(record, 14);
    let target_dims = [
        i32_at(record, 16),
        i32_at(record, 20),
        i32_at(record, 24),
        i32_at(record, 28),
    ];
    if input_rank > 4 || output_rank < input_rank || output_rank > 4 || record[12] != 1 {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if !matches!(value_dtype, AttributeDType::Float | AttributeDType::Int64) {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[13] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    match control_mode {
        ControlMode::Initializer => {
            if producer_opcode != 0
                || target_dims[..usize::from(output_rank)]
                    .iter()
                    .any(|&dimension| dimension <= 0)
                || target_dims[usize::from(output_rank)..]
                    .iter()
                    .any(|&dimension| dimension != 0)
            {
                return Err(fail(opcode, ContractReason::Control));
            }
        }
        ControlMode::Dynamic => {
            if OpCode::try_from(producer_opcode).is_err()
                || target_dims.iter().any(|&dimension| dimension != 0)
            {
                return Err(fail(opcode, ContractReason::Provenance));
            }
        }
        ControlMode::Absent => return Err(fail(opcode, ContractReason::Control)),
    }
    Ok(Attributes::Expand(ExpandAttributes {
        input_rank,
        output_rank,
        dtype: value_dtype,
        control_mode,
        producer_opcode,
        target_dims,
    }))
}

fn decode_shape(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Shape;
    let input_rank = record[16];
    if !bytes_zero(record, 20, 24) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if i32_at(record, 8) != 0
        || i32_at(record, 12) != 0
        || input_rank == 0
        || input_rank > 4
        || record[17] != 1
        || record[18] != 0
        || record[19] != CHECKED_LAYOUT
    {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::Shape(ShapeAttributes { input_rank }))
}

fn decode_slice(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Slice;
    let rank = record[8];
    let value_dtype = dtype(record[10], opcode)?;
    let control_count = record[11];
    let flags = record[12];
    if u16_at(record, 14) != 0 || !bytes_zero(record, 20, 24) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    let axes_present = flags & 1 != 0;
    let steps_present = flags & 2 != 0;
    if rank == 0
        || rank > 4
        || record[9] != rank
        || control_count != 2 + u8::from(axes_present) + u8::from(steps_present)
        || flags & !0b11 != 0
    {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if !matches!(value_dtype, AttributeDType::Float | AttributeDType::Int64) {
        return Err(fail(opcode, ContractReason::DType));
    }
    if record[13] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Layout));
    }
    let mut control_modes = [ControlMode::Absent; 4];
    let mut control_values = [0_i64; 4];
    let mut producer_opcodes = [0_u16; 4];
    for slot in 0..4 {
        control_modes[slot] = ControlMode::try_from(record[16 + slot])
            .map_err(|_| fail(opcode, ContractReason::Control))?;
        control_values[slot] = i64_at(record, 24 + slot * 8);
        producer_opcodes[slot] = u16_at(record, 56 + slot * 2);
        let present = slot < 2 || (slot == 2 && axes_present) || (slot == 3 && steps_present);
        match (present, control_modes[slot]) {
            (false, ControlMode::Absent)
                if control_values[slot] == 0 && producer_opcodes[slot] == 0 => {}
            (true, ControlMode::Initializer) if producer_opcodes[slot] == 0 => {}
            (true, ControlMode::Dynamic)
                if control_values[slot] == 0
                    && OpCode::try_from(producer_opcodes[slot]).is_ok() => {}
            _ => return Err(fail(opcode, ContractReason::Provenance)),
        }
    }
    if control_modes[0] != ControlMode::Initializer {
        return Err(fail(opcode, ContractReason::Control));
    }
    if axes_present
        && (control_modes[2] != ControlMode::Initializer
            || control_values[2] < -i64::from(rank)
            || control_values[2] >= i64::from(rank))
    {
        return Err(fail(opcode, ContractReason::Axis));
    }
    if steps_present && (control_modes[3] != ControlMode::Initializer || control_values[3] != 1) {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::Slice(SliceAttributes {
        rank,
        dtype: value_dtype,
        flags,
        control_modes,
        control_values,
        producer_opcodes,
    }))
}

fn decode_pad(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::Pad;
    let rank = record[8];
    let count = record[12];
    if u16_at(record, 14) != 0 {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if rank == 0 || rank > 4 || record[9] != rank {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if record[10] != AttributeDType::Float as u8
        || record[11] != 1
        || count != rank * 2
        || record[13] != CHECKED_LAYOUT
    {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    let mut pads = [0_u32; 8];
    for (index, pad) in pads.iter_mut().enumerate() {
        *pad = u32_at(record, 16 + index * 4);
    }
    if pads[usize::from(count)..].iter().any(|&pad| pad != 0) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    Ok(Attributes::Pad(PadAttributes { rank, pads }))
}

fn decode_nonzero(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::NonZero;
    if !bytes_zero(record, 14, 16) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if record[8..14] != [1, 2, 9, 7, CHECKED_LAYOUT, 1] {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::NonZero)
}

fn decode_scatter(record: &[u8]) -> Result<Attributes, AttributeError> {
    let opcode = OpCode::ScatterNd;
    if !bytes_zero(record, 17, 20) {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if record[8..17] != [2, 3, 2, 2, 1, 2, 0, 1, CHECKED_LAYOUT] {
        return Err(fail(opcode, ContractReason::Parameters));
    }
    Ok(Attributes::ScatterNd(ScatterNdAttributes))
}

fn decode_view(record: &[u8], opcode: OpCode) -> Result<Attributes, AttributeError> {
    let input_rank = record[8];
    let output_rank = record[9];
    let value_dtype = dtype(record[10], opcode)?;
    let flags = record[11];
    let count = record[13];
    let parameters = [
        i32_at(record, 16),
        i32_at(record, 20),
        i32_at(record, 24),
        i32_at(record, 28),
    ];
    if record[15] != 0
        || parameters[usize::from(count.min(4))..]
            .iter()
            .any(|&v| v != 0)
    {
        return Err(fail(opcode, ContractReason::Reserved));
    }
    if input_rank > 4 || output_rank > 4 || count > 4 {
        return Err(fail(opcode, ContractReason::Rank));
    }
    if !matches!(value_dtype, AttributeDType::Float | AttributeDType::Int32 | AttributeDType::Int64)
    {
        return Err(fail(opcode, ContractReason::DType));
    }
    if flags & !0b11 != 0 || flags & 1 == 0 || record[12] != 0 || record[14] != CHECKED_LAYOUT {
        return Err(fail(opcode, ContractReason::Flags));
    }
    let static_control = flags & 2 != 0;
    match opcode {
        OpCode::Reshape => {
            if output_rank == 0
                || (static_control && count != output_rank)
                || (!static_control && count != 0)
            {
                return Err(fail(opcode, ContractReason::Control));
            }
            if static_control {
                let values = &parameters[..usize::from(count)];
                if values.iter().filter(|&&value| value == -1).count() > 1
                    || values.iter().any(|&value| value < -1)
                    || values
                        .iter()
                        .enumerate()
                        .any(|(axis, &value)| value == 0 && axis >= usize::from(input_rank))
                {
                    return Err(fail(opcode, ContractReason::Parameters));
                }
            }
        }
        OpCode::Unsqueeze => {
            if !static_control || count == 0 || output_rank != input_rank + count {
                return Err(fail(opcode, ContractReason::Control));
            }
            validate_unique_axes(opcode, &parameters[..usize::from(count)], output_rank)?;
        }
        OpCode::Squeeze => {
            if !static_control || count == 0 || output_rank + count != input_rank {
                return Err(fail(opcode, ContractReason::Control));
            }
            validate_unique_axes(opcode, &parameters[..usize::from(count)], input_rank)?;
        }
        _ => return Err(AttributeError::UnsupportedOpcode { opcode }),
    }
    Ok(Attributes::View(ViewAttributes {
        opcode,
        input_rank,
        output_rank,
        dtype: value_dtype,
        static_control,
        count,
        parameters,
    }))
}

fn validate_unique_axes(
    opcode: OpCode,
    parameters: &[i32],
    rank: u8,
) -> Result<(), AttributeError> {
    let rank = i32::from(rank);
    for (index, &axis) in parameters.iter().enumerate() {
        let normalized = if axis < 0 { axis + rank } else { axis };
        if normalized < 0 || normalized >= rank {
            return Err(fail(opcode, ContractReason::Axis));
        }
        for &previous in &parameters[..index] {
            let previous = if previous < 0 {
                previous + rank
            } else {
                previous
            };
            if previous == normalized {
                return Err(fail(opcode, ContractReason::Axis));
            }
        }
    }
    Ok(())
}

fn bytes_zero(bytes: &[u8], start: usize, end: usize) -> bool {
    bytes[start..end].iter().all(|&byte| byte == 0)
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn i64_at(bytes: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}
