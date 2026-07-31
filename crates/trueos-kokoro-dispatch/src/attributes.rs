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
    UnsupportedVersion { found: u16 },
    KindMismatch { expected: OpCode, found: u16 },
    UnsupportedOpcode { opcode: OpCode },
    ByteCountMismatch { header: u32, actual: usize },
    WrongLength { opcode: OpCode, expected: usize, actual: usize },
    Contract { opcode: OpCode, reason: ContractReason },
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
        Add | Mul | Div | Sub | LeakyRelu | Softmax | FastGelu
        | SkipLayerNormalization | Cast | NonZero => Some(16),
        Atan | Cos | Exp | Floor | Round | Sigmoid | Sin | Sqrt | Tanh => Some(8),
        ReduceMean | Gather | Concat | And | Equal | Greater | GreaterOrEqual | Less
        | ConstantOfShape | CumSum | DequantizeLinear | Where | Pow | Range
        | ScatterNd => Some(20),
        LayerNormalization | Shape | Resize => Some(24),
        Split => Some(28),
        Reshape | Squeeze | Unsqueeze | Transpose | Expand | FixedStft20 | MatMul => Some(32),
        BiLstm256 | ResolveDecoderShape | DynamicQuantizedGemm => Some(40),
        Pad => Some(48),
        FloatConv1d | FloatConvTranspose1d => Some(56),
        DynamicQuantizedConv1d => Some(60),
        Slice => Some(64),
        Clip | Conv | ConvInteger | ConvTranspose | DynamicQuantizeLinear | Lstm
        | MatMulInteger | ReduceSum | Stft | AddSoftmax | AlbertAttention
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
        Clip | Conv | ConvInteger | ConvTranspose | DynamicQuantizeLinear | Lstm
        | MatMulInteger | ReduceSum | Stft | AddSoftmax | AlbertAttention
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
    Ok(Attributes::Resize(ResizeAttributes { profile, mode, scale }))
}
