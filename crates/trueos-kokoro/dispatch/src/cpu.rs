//! Native CPU implementation for the operation families whose scheduler
//! mapping and tensor semantics are already sealed.

use core::convert::TryFrom;

use trueos_kokoro_aot::{DType, OpCode, Phase, Program, StorageKind, WorkSlice};
use trueos_kokoro_exec::{DispatchResult, Dispatcher as ExecDispatcher, RuntimeShape};
use trueos_kokoro_memory::{MemoryError, OpAccess, TensorElement, TensorMemory};

use crate::attributes::{
    AttributeDType, AttributeError, Attributes, BiLstmAttributes, BiasMode, BinaryAttributes,
    CastAttributes, ComparisonAttributes, ConcatAttributes, ControlMode, ExpandAttributes,
    FloatConvAttributes, GatherAttributes, MatMulAttributes, PadAttributes, QuantConvAttributes,
    QuantGemmAttributes, ResizeAttributes, ResizeMode, SliceAttributes, SplitAttributes,
    TransposeAttributes, UnaryAttributes, ViewAttributes,
};
use crate::decode;

/// Return whether the concrete CPU dispatcher has a real execution adapter
/// for this decoded attribute contract. This is the single warm-audit source
/// of truth.
pub const fn native_dispatch_supported(attributes: Attributes) -> bool {
    match attributes {
        Attributes::Binary(_)
        | Attributes::Comparison(_)
        | Attributes::Cast(_)
        | Attributes::ConstantOfShape(_)
        | Attributes::CumSum(_)
        | Attributes::DequantizeLinear(_)
        | Attributes::Where(_)
        | Attributes::MatMul(_)
        | Attributes::Pow(_)
        | Attributes::Range
        | Attributes::Resize(_)
        | Attributes::BiLstm256(_)
        | Attributes::FloatConv(_)
        | Attributes::FixedStft20(_)
        | Attributes::ResolveDecoderShape(_)
        | Attributes::DynamicQuantizedGemm(_)
        | Attributes::DynamicQuantizedConv1d(_)
        | Attributes::Unary(_)
        | Attributes::LeakyRelu(_)
        | Attributes::ReduceMean(_)
        | Attributes::LayerNormalization(_)
        | Attributes::Softmax(_)
        | Attributes::FastGelu(_)
        | Attributes::SkipLayerNormalization(_)
        | Attributes::Transpose(_)
        | Attributes::Gather(_)
        | Attributes::Concat(_)
        | Attributes::Split(_)
        | Attributes::Expand(_)
        | Attributes::Shape(_)
        | Attributes::Slice(_)
        | Attributes::Pad(_)
        | Attributes::NonZero
        | Attributes::ScatterNd(_)
        | Attributes::View(_) => true,
    }
}

/// Return whether an adapter needs the bounded caller-owned native workspace.
pub const fn native_dispatch_requires_workspace(attributes: Attributes) -> bool {
    matches!(
        attributes,
        Attributes::BiLstm256(_)
            | Attributes::DynamicQuantizedGemm(_)
            | Attributes::DynamicQuantizedConv1d(_)
    )
}

/// Largest scratch slices required by the pinned 2,227-operation artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuWorkspaceRequirements {
    pub quant_u8: usize,
    pub packed_i8: usize,
    pub accum_i32: usize,
    pub row_sums_i32: usize,
    pub bias_i32: usize,
    pub lstm_gates_f32: usize,
}

pub const KOKORO_CPU_WORKSPACE_REQUIREMENTS: CpuWorkspaceRequirements = CpuWorkspaceRequirements {
    quant_u8: 13_080,
    packed_i8: 3_348_480,
    accum_i32: 2_180,
    row_sums_i32: 2_180,
    bias_i32: 1_024,
    lstm_gates_f32: 1_024,
};

// The pinned 2,227-operation Kokoro graph lowers `atan2(imaginary, real)` to
// Div -> Atan -> quadrant correction. Only this edge is allowed to carry the
// signed infinity produced when `real` is exactly zero; all other arithmetic
// keeps the dispatcher's fail-closed finite-value policy.
const KOKORO_STFT_PHASE_DIV_OP_INDEX: u32 = 1_324;
const KOKORO_STFT_PHASE_ATAN_OP_INDEX: u32 = 1_330;

/// Exact workspace slice rejected by [`CpuWorkspace::new`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceError {
    QuantU8TooSmall,
    PackedI8TooSmall,
    AccumI32TooSmall,
    RowSumsI32TooSmall,
    BiasI32TooSmall,
    LstmGatesF32TooSmall,
}

/// Caller-owned, reusable scratch for quantized and recurrent adapters.
///
/// The largest member is the signed runtime weight pack (3,348,480 bytes).
/// Activations and full i32 outputs are deliberately never materialized.
pub struct CpuWorkspace<'a> {
    quant_u8: &'a mut [u8],
    packed_i8: &'a mut [i8],
    accum_i32: &'a mut [i32],
    row_sums_i32: &'a mut [i32],
    bias_i32: &'a mut [i32],
    lstm_gates_f32: &'a mut [f32],
}

impl<'a> CpuWorkspace<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        quant_u8: &'a mut [u8],
        packed_i8: &'a mut [i8],
        accum_i32: &'a mut [i32],
        row_sums_i32: &'a mut [i32],
        bias_i32: &'a mut [i32],
        lstm_gates_f32: &'a mut [f32],
    ) -> Result<Self, WorkspaceError> {
        let required = KOKORO_CPU_WORKSPACE_REQUIREMENTS;
        if quant_u8.len() < required.quant_u8 {
            return Err(WorkspaceError::QuantU8TooSmall);
        }
        if packed_i8.len() < required.packed_i8 {
            return Err(WorkspaceError::PackedI8TooSmall);
        }
        if accum_i32.len() < required.accum_i32 {
            return Err(WorkspaceError::AccumI32TooSmall);
        }
        if row_sums_i32.len() < required.row_sums_i32 {
            return Err(WorkspaceError::RowSumsI32TooSmall);
        }
        if bias_i32.len() < required.bias_i32 {
            return Err(WorkspaceError::BiasI32TooSmall);
        }
        if lstm_gates_f32.len() < required.lstm_gates_f32 {
            return Err(WorkspaceError::LstmGatesF32TooSmall);
        }
        Ok(Self {
            quant_u8,
            packed_i8,
            accum_i32,
            row_sums_i32,
            bias_i32,
            lstm_gates_f32,
        })
    }
}

/// Exact native-dispatch failures. Unsupported work never commits its AOT
/// cursor: there is deliberately no success fallback for a missing kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchError {
    MissingAttributes,
    Attribute(AttributeError),
    Memory(MemoryError),
    F32(trueos_kokoro_f32::Error),
    Scalar(trueos_kokoro_scalar::Error),
    Layout(trueos_kokoro_layout::Error),
    Resize(trueos_kokoro_resize::Error),
    Duration(trueos_kokoro_duration::ResolveError),
    Conv(trueos_kokoro_conv::Error),
    Gemm(trueos_kokoro_gemm::Error),
    Quant(trueos_ttstt_cpu::Error),
    Quantization(trueos_ttstt_cpu::QuantizationError),
    StftContract(trueos_kokoro_stft::ContractError),
    StftAdvance(trueos_kokoro_stft::AdvanceError),
    LstmContract(trueos_kokoro_lstm::ContractError),
    LstmDense(trueos_kokoro_lstm::DenseError),
    Workspace(WorkspaceError),
    WorkspaceRequired { opcode: OpCode },
    ShapeConversion,
    InvalidArity,
    InvalidControlTensor,
    InvalidWorkContract { opcode: OpCode },
    UnsupportedAttributeProfile { opcode: OpCode },
    UnsupportedOpcode { opcode: OpCode },
}

impl From<AttributeError> for DispatchError {
    fn from(value: AttributeError) -> Self {
        Self::Attribute(value)
    }
}

impl From<MemoryError> for DispatchError {
    fn from(value: MemoryError) -> Self {
        Self::Memory(value)
    }
}

impl From<trueos_kokoro_f32::Error> for DispatchError {
    fn from(value: trueos_kokoro_f32::Error) -> Self {
        Self::F32(value)
    }
}

impl From<trueos_kokoro_scalar::Error> for DispatchError {
    fn from(value: trueos_kokoro_scalar::Error) -> Self {
        Self::Scalar(value)
    }
}

impl From<trueos_kokoro_layout::Error> for DispatchError {
    fn from(value: trueos_kokoro_layout::Error) -> Self {
        Self::Layout(value)
    }
}

impl From<trueos_kokoro_resize::Error> for DispatchError {
    fn from(value: trueos_kokoro_resize::Error) -> Self {
        Self::Resize(value)
    }
}

impl From<trueos_kokoro_duration::ResolveError> for DispatchError {
    fn from(value: trueos_kokoro_duration::ResolveError) -> Self {
        Self::Duration(value)
    }
}

impl From<trueos_kokoro_conv::Error> for DispatchError {
    fn from(value: trueos_kokoro_conv::Error) -> Self {
        Self::Conv(value)
    }
}

impl From<trueos_kokoro_gemm::Error> for DispatchError {
    fn from(value: trueos_kokoro_gemm::Error) -> Self {
        Self::Gemm(value)
    }
}

impl From<trueos_ttstt_cpu::Error> for DispatchError {
    fn from(value: trueos_ttstt_cpu::Error) -> Self {
        Self::Quant(value)
    }
}

impl From<trueos_ttstt_cpu::QuantizationError> for DispatchError {
    fn from(value: trueos_ttstt_cpu::QuantizationError) -> Self {
        Self::Quantization(value)
    }
}

impl From<trueos_kokoro_lstm::ContractError> for DispatchError {
    fn from(value: trueos_kokoro_lstm::ContractError) -> Self {
        Self::LstmContract(value)
    }
}

impl From<trueos_kokoro_lstm::DenseError> for DispatchError {
    fn from(value: trueos_kokoro_lstm::DenseError) -> Self {
        Self::LstmDense(value)
    }
}

impl From<WorkspaceError> for DispatchError {
    fn from(value: WorkspaceError) -> Self {
        Self::Workspace(value)
    }
}

impl From<trueos_kokoro_stft::ContractError> for DispatchError {
    fn from(value: trueos_kokoro_stft::ContractError) -> Self {
        Self::StftContract(value)
    }
}

impl From<trueos_kokoro_stft::AdvanceError> for DispatchError {
    fn from(value: trueos_kokoro_stft::AdvanceError) -> Self {
        Self::StftAdvance(value)
    }
}

/// Dispatcher over one already-admitted phase's validated tensor memory.
///
/// Runtime input shapes must already be present in the executor's shape table.
/// The dispatcher derives and transactionally declares each operation's output
/// shapes before resolving its typed bindings. A backend creates a new instance
/// when phase one is admitted. This type owns no buffers and performs no
/// allocation.
pub struct CpuDispatcher<
    'dispatch,
    'memory,
    'artifact,
    'buffers,
    'workspace,
    const SHAPES: usize,
    const EXTERNALS: usize,
    const BINDINGS: usize,
> {
    memory: &'dispatch mut TensorMemory<'memory, 'artifact, 'buffers, SHAPES, EXTERNALS, BINDINGS>,
    workspace: Option<&'dispatch mut CpuWorkspace<'workspace>>,
    conv: trueos_kokoro_conv::Dispatcher,
    gemm: trueos_kokoro_gemm::Dispatcher,
    quant: trueos_ttstt_cpu::Dispatcher,
}

/// Reusable host-kernel selection for cooperatively sliced CPU execution.
///
/// This value owns no tensor or workspace borrows, so an inference job can
/// retain it safely while rebuilding only the short-lived validated memory
/// view required by each scheduler slice.
#[derive(Clone, Copy)]
pub struct CpuDispatchPlan {
    conv: trueos_kokoro_conv::Dispatcher,
    gemm: trueos_kokoro_gemm::Dispatcher,
    quant: trueos_ttstt_cpu::Dispatcher,
}

impl CpuDispatchPlan {
    /// Detect the best sealed kernel lanes once for the current CPU.
    pub fn detect() -> Self {
        Self {
            conv: trueos_kokoro_conv::Dispatcher::detect(),
            gemm: trueos_kokoro_gemm::Dispatcher::detect(),
            quant: trueos_ttstt_cpu::Dispatcher::detect(),
        }
    }
}

impl<
    'dispatch,
    'memory,
    'artifact,
    'buffers,
    'workspace,
    const SHAPES: usize,
    const EXTERNALS: usize,
    const BINDINGS: usize,
> CpuDispatcher<'dispatch, 'memory, 'artifact, 'buffers, 'workspace, SHAPES, EXTERNALS, BINDINGS>
{
    pub fn new(
        memory: &'dispatch mut TensorMemory<
            'memory,
            'artifact,
            'buffers,
            SHAPES,
            EXTERNALS,
            BINDINGS,
        >,
    ) -> Self {
        Self::new_with_plan(memory, CpuDispatchPlan::detect())
    }

    /// Construct a dispatcher from a previously detected CPU kernel plan.
    pub fn new_with_plan(
        memory: &'dispatch mut TensorMemory<
            'memory,
            'artifact,
            'buffers,
            SHAPES,
            EXTERNALS,
            BINDINGS,
        >,
        plan: CpuDispatchPlan,
    ) -> Self {
        Self {
            memory,
            workspace: None,
            conv: plan.conv,
            gemm: plan.gemm,
            quant: plan.quant,
        }
    }

    pub fn new_with_workspace(
        memory: &'dispatch mut TensorMemory<
            'memory,
            'artifact,
            'buffers,
            SHAPES,
            EXTERNALS,
            BINDINGS,
        >,
        workspace: &'dispatch mut CpuWorkspace<'workspace>,
    ) -> Self {
        Self::new_with_workspace_and_plan(memory, workspace, CpuDispatchPlan::detect())
    }

    /// Construct a workspace-backed dispatcher from a retained kernel plan.
    pub fn new_with_workspace_and_plan(
        memory: &'dispatch mut TensorMemory<
            'memory,
            'artifact,
            'buffers,
            SHAPES,
            EXTERNALS,
            BINDINGS,
        >,
        workspace: &'dispatch mut CpuWorkspace<'workspace>,
        plan: CpuDispatchPlan,
    ) -> Self {
        Self {
            memory,
            workspace: Some(workspace),
            conv: plan.conv,
            gemm: plan.gemm,
            quant: plan.quant,
        }
    }

    pub fn memory(
        &self,
    ) -> &TensorMemory<'memory, 'artifact, 'buffers, SHAPES, EXTERNALS, BINDINGS> {
        self.memory
    }
}

#[derive(Clone, Copy)]
struct OutputShapes {
    values: [RuntimeShape; 3],
    len: usize,
}

impl OutputShapes {
    const fn one(shape: RuntimeShape) -> Self {
        Self {
            values: [shape, RuntimeShape::scalar(), RuntimeShape::scalar()],
            len: 1,
        }
    }

    const fn two(first: RuntimeShape, second: RuntimeShape) -> Self {
        Self {
            values: [first, second, RuntimeShape::scalar()],
            len: 2,
        }
    }

    const fn three(first: RuntimeShape, second: RuntimeShape, third: RuntimeShape) -> Self {
        Self {
            values: [first, second, third],
            len: 3,
        }
    }

    fn as_slice(&self) -> &[RuntimeShape] {
        &self.values[..self.len]
    }
}

fn infer_output_shapes<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: Attributes,
) -> Result<OutputShapes, DispatchError> {
    let result = match attributes {
        Attributes::Binary(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 2, 1)?;
            let lhs = input_shape(memory, program, work, 0)?;
            let rhs = input_shape(memory, program, work, 1)?;
            require_rank(lhs, value.lhs_rank)?;
            require_rank(rhs, value.rhs_rank)?;
            let output = broadcast_shape(lhs, rhs)?;
            require_rank(output, value.output_rank)?;
            OutputShapes::one(output)
        }
        Attributes::Comparison(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 2, 1)?;
            let lhs = input_shape(memory, program, work, 0)?;
            let rhs = input_shape(memory, program, work, 1)?;
            require_rank(lhs, value.lhs_rank)?;
            require_rank(rhs, value.rhs_rank)?;
            let output = broadcast_shape(lhs, rhs)?;
            require_rank(output, value.output_rank)?;
            OutputShapes::one(output)
        }
        Attributes::Cast(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 1, 1)?;
            let output = input_shape(memory, program, work, 0)?;
            require_rank(output, value.rank)?;
            OutputShapes::one(output)
        }
        Attributes::ConstantOfShape(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 1, 1)?;
            let dimensions = read_i64_control::<8, _, _, _>(memory, program, work, 0)?;
            if dimensions.len != usize::from(value.output_rank) {
                return Err(DispatchError::InvalidControlTensor);
            }
            OutputShapes::one(runtime_shape_i64(&dimensions.values[..dimensions.len])?)
        }
        Attributes::CumSum(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 2, 1)?;
            let output = input_shape(memory, program, work, 0)?;
            require_rank(output, value.rank)?;
            let axis = read_i32_scalar(memory, program, work, 1)?;
            if axis != value.axis {
                return Err(DispatchError::InvalidControlTensor);
            }
            OutputShapes::one(output)
        }
        Attributes::DequantizeLinear(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 3, 1)?;
            let output = input_shape(memory, program, work, 0)?;
            require_rank(output, value.rank)?;
            OutputShapes::one(output)
        }
        Attributes::Where(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 3, 1)?;
            let condition = input_shape(memory, program, work, 0)?;
            let when_true = input_shape(memory, program, work, 1)?;
            let when_false = input_shape(memory, program, work, 2)?;
            require_rank(condition, value.condition_rank)?;
            require_rank(when_true, value.true_rank)?;
            require_rank(when_false, value.false_rank)?;
            let output = broadcast_shape(condition, broadcast_shape(when_true, when_false)?)?;
            require_rank(output, value.output_rank)?;
            OutputShapes::one(output)
        }
        Attributes::MatMul(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 2, 1)?;
            let lhs = input_shape(memory, program, work, 0)?;
            let rhs = input_shape(memory, program, work, 1)?;
            let (_, output) = matmul_profile(value, lhs, rhs)?;
            OutputShapes::one(output)
        }
        Attributes::DynamicQuantizedGemm(value) => infer_quant_gemm(memory, program, work, value)?,
        Attributes::DynamicQuantizedConv1d(value) => {
            infer_quant_conv(memory, program, work, value)?
        }
        Attributes::BiLstm256(value) => infer_bilstm(memory, program, work, value)?,
        Attributes::Pow(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 2, 1)?;
            let output = input_shape(memory, program, work, 0)?;
            require_rank(output, value.rank)?;
            let exponent = read_f32_scalar(memory, program, work, 1)?;
            if exponent.to_bits() != value.exponent_bits {
                return Err(DispatchError::InvalidControlTensor);
            }
            OutputShapes::one(output)
        }
        Attributes::Range => {
            require_whole_unit(work)?;
            require_op_arity(work, 3, 1)?;
            let start = read_i64_scalar(memory, program, work, 0)?;
            let limit = read_i64_scalar(memory, program, work, 1)?;
            let delta = read_i64_scalar(memory, program, work, 2)?;
            let count = trueos_kokoro_scalar::range_count(start, limit, delta)?;
            let count = u32::try_from(count).map_err(|_| DispatchError::ShapeConversion)?;
            OutputShapes::one(
                RuntimeShape::new(&[count]).map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        Attributes::Resize(value) => {
            require_op_arity(work, 2, 1)?;
            let input = input_shape(memory, program, work, 0)?;
            let dims = input.dims();
            if dims.len() != 3 {
                return Err(DispatchError::ShapeConversion);
            }
            let scales = read_f32_control::<4, _, _, _>(memory, program, work, 1)?;
            let (mode, scale, expected_scale) = resize_profile(value)?;
            if scales.len != 3
                || scales.values[0].to_bits() != 1.0_f32.to_bits()
                || scales.values[1].to_bits() != 1.0_f32.to_bits()
                || scales.values[2].to_bits() != expected_scale.to_bits()
            {
                return Err(DispatchError::InvalidControlTensor);
            }
            let plan = trueos_kokoro_resize::ResizePlan::new(
                dims[0] as usize,
                dims[1] as usize,
                dims[2] as usize,
                mode,
                scale,
            )?;
            require_cooperative_work(work, plan.output_elements())?;
            let width =
                u32::try_from(plan.output_len()).map_err(|_| DispatchError::ShapeConversion)?;
            OutputShapes::one(
                RuntimeShape::new(&[dims[0], dims[1], width])
                    .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        Attributes::FloatConv(value) => {
            require_op_arity(work, if value.has_bias { 3 } else { 2 }, 1)?;
            let input = input_shape(memory, program, work, 0)?;
            let dims = input.dims();
            if dims.len() != 3 || dims[0] != 1 || dims[1] != value.input_channels {
                return Err(DispatchError::ShapeConversion);
            }
            let profile = conv_profile(value, dims[2] as usize)?;
            validate_conv_attributes(value, profile)?;
            let dimensions = profile.dimensions()?;
            require_cooperative_work(work, dimensions.output_elements()?)?;
            let width = u32::try_from(dimensions.output_width)
                .map_err(|_| DispatchError::ShapeConversion)?;
            OutputShapes::one(
                RuntimeShape::new(&[1, value.output_channels, width])
                    .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        Attributes::Unary(_)
        | Attributes::LeakyRelu(_)
        | Attributes::LayerNormalization(_)
        | Attributes::Softmax(_)
        | Attributes::FastGelu(_)
        | Attributes::SkipLayerNormalization(_) => {
            require_whole_unit(work)?;
            let expected_inputs = match attributes {
                Attributes::LayerNormalization(_) => 3,
                Attributes::FastGelu(_) => 2,
                Attributes::SkipLayerNormalization(_) => 4,
                _ => 1,
            };
            require_op_arity(work, expected_inputs, 1)?;
            OutputShapes::one(input_shape(memory, program, work, 0)?)
        }
        Attributes::ReduceMean(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 2, 1)?;
            let input = input_shape(memory, program, work, 0)?;
            require_rank(input, value.rank)?;
            let axis_control = read_i64_scalar(memory, program, work, 1)?;
            if axis_control != i64::from(value.axis) {
                return Err(DispatchError::InvalidControlTensor);
            }
            let axis = normalize_axis(value.axis, input.rank())?;
            let mut dims = [1_u32; 4];
            dims[..input.dims().len()].copy_from_slice(input.dims());
            dims[axis] = 1;
            OutputShapes::one(
                RuntimeShape::new(&dims[..input.dims().len()])
                    .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        Attributes::ResolveDecoderShape(_) => {
            require_whole_unit(work)?;
            require_op_arity(work, 2, 2)?;
            let logits = input_shape(memory, program, work, 0)?;
            let dims = logits.dims();
            if dims.len() != 3
                || dims[0] != 1
                || dims[2] != trueos_kokoro_duration::KOKORO_DURATION_BINS as u32
            {
                return Err(DispatchError::InvalidControlTensor);
            }
            OutputShapes::two(
                RuntimeShape::new(&[dims[1]]).map_err(|_| DispatchError::ShapeConversion)?,
                RuntimeShape::scalar(),
            )
        }
        Attributes::Transpose(value) => infer_transpose(memory, program, work, value)?,
        Attributes::Gather(value) => infer_gather(memory, program, work, value)?,
        Attributes::Concat(value) => infer_concat(memory, program, work, value)?,
        Attributes::Split(value) => infer_split(memory, program, work, value)?,
        Attributes::Expand(value) => infer_expand(memory, program, work, value)?,
        Attributes::Shape(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 1, 1)?;
            let input = input_shape(memory, program, work, 0)?;
            require_rank(input, value.input_rank)?;
            OutputShapes::one(
                RuntimeShape::new(&[u32::from(value.input_rank)])
                    .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        Attributes::Slice(value) => infer_slice(memory, program, work, value)?,
        Attributes::Pad(value) => infer_pad(memory, program, work, value)?,
        Attributes::NonZero => {
            require_whole_unit(work)?;
            require_op_arity(work, 1, 1)?;
            let input_id = op_input(program, work, 0)?;
            let (count, shape) = memory.with_read::<bool, _, _>(input_id, |values, shape| {
                (values.iter().filter(|&&item| item).count(), shape)
            })?;
            if shape.rank() != 1 {
                return Err(DispatchError::ShapeConversion);
            }
            let count = u32::try_from(count).map_err(|_| DispatchError::ShapeConversion)?;
            OutputShapes::one(
                RuntimeShape::new(&[1, count]).map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        Attributes::ScatterNd(_) => {
            require_whole_unit(work)?;
            require_op_arity(work, 3, 1)?;
            OutputShapes::one(input_shape(memory, program, work, 0)?)
        }
        Attributes::View(value) => infer_view(memory, program, work, value)?,
        Attributes::FixedStft20(value) => {
            require_whole_unit(work)?;
            require_op_arity(work, 4, 1)?;
            validate_stft_controls(memory, program, work)?;
            if value.frame_length != trueos_kokoro_stft::FRAME_LENGTH as u32
                || value.frame_step != trueos_kokoro_stft::FRAME_STEP as u32
                || value.bins != trueos_kokoro_stft::OUTPUT_BINS as u32
            {
                return Err(DispatchError::UnsupportedAttributeProfile {
                    opcode: OpCode::FixedStft20,
                });
            }
            let input = input_shape(memory, program, work, 0)?;
            let dims = input.dims();
            if dims.len() != 2 || dims[0] == 0 || dims[1] < value.frame_length {
                return Err(DispatchError::ShapeConversion);
            }
            let frames = dims[1]
                .checked_sub(value.frame_length)
                .and_then(|tail| tail.checked_div(value.frame_step))
                .and_then(|count| count.checked_add(1))
                .ok_or(DispatchError::ShapeConversion)?;
            OutputShapes::one(
                RuntimeShape::new(&[dims[0], frames, value.bins, 2])
                    .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
    };
    Ok(result)
}

fn require_op_arity(work: WorkSlice, inputs: u16, outputs: u16) -> Result<(), DispatchError> {
    if work.op().input_count == inputs && work.op().output_count == outputs {
        Ok(())
    } else {
        Err(DispatchError::InvalidArity)
    }
}

fn require_cooperative_work(work: WorkSlice, output_elements: usize) -> Result<(), DispatchError> {
    // Runtime-sized phase-one tensors use the artifact's sealed maximum-F
    // coordinate space. A shorter utterance consumes only the live prefix;
    // scheduler slices in the remaining suffix are intentional no-ops.
    if usize::try_from(work.op().work_units).is_ok_and(|units| units >= output_elements)
        && work.unit_count() != 0
        && work.unit_end() <= work.op().work_units
    {
        Ok(())
    } else {
        Err(DispatchError::InvalidWorkContract {
            opcode: work.op().opcode,
        })
    }
}

fn op_input(program: &Program<'_>, work: WorkSlice, input: u16) -> Result<u32, DispatchError> {
    program
        .op_input(work.op(), input)
        .ok_or(DispatchError::InvalidArity)
}

fn input_shape<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    input: u16,
) -> Result<RuntimeShape, DispatchError> {
    Ok(memory.tensor_shape(op_input(program, work, input)?)?)
}

fn require_rank(shape: RuntimeShape, rank: u8) -> Result<(), DispatchError> {
    if shape.rank() == rank {
        Ok(())
    } else {
        Err(DispatchError::ShapeConversion)
    }
}

fn normalize_axis(axis: i32, rank: u8) -> Result<usize, DispatchError> {
    let rank = i32::from(rank);
    let normalized = if axis < 0 { axis + rank } else { axis };
    if normalized >= 0 && normalized < rank {
        Ok(normalized as usize)
    } else {
        Err(DispatchError::ShapeConversion)
    }
}

fn broadcast_shape(lhs: RuntimeShape, rhs: RuntimeShape) -> Result<RuntimeShape, DispatchError> {
    let rank = usize::from(lhs.rank().max(rhs.rank()));
    let mut dims = [1_u32; 4];
    for reverse in 0..rank {
        let lhs_dim = lhs
            .dims()
            .len()
            .checked_sub(reverse + 1)
            .map_or(1, |axis| lhs.dims()[axis]);
        let rhs_dim = rhs
            .dims()
            .len()
            .checked_sub(reverse + 1)
            .map_or(1, |axis| rhs.dims()[axis]);
        let output = if lhs_dim == rhs_dim {
            lhs_dim
        } else if lhs_dim == 1 {
            rhs_dim
        } else if rhs_dim == 1 {
            lhs_dim
        } else {
            return Err(DispatchError::ShapeConversion);
        };
        dims[rank - reverse - 1] = output;
    }
    RuntimeShape::new(&dims[..rank]).map_err(|_| DispatchError::ShapeConversion)
}

fn infer_quant_gemm<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: QuantGemmAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    let has_bias = attributes.bias_mode == BiasMode::Float;
    require_op_arity(work, if has_bias { 5 } else { 4 }, 1)?;
    let activation = input_shape(memory, program, work, 0)?;
    require_rank(activation, attributes.activation_rank)?;
    let activation_dims = activation.dims();
    let rank = activation_dims.len();
    if !matches!(rank, 2 | 3)
        || activation_dims[rank - 1] != attributes.k
        || input_shape(memory, program, work, 1)?.dims() != [attributes.k, attributes.n]
        || input_shape(memory, program, work, 2)?.dims() != [attributes.n]
        || input_shape(memory, program, work, 3)?.dims() != [attributes.n]
        || (has_bias && input_shape(memory, program, work, 4)?.dims() != [attributes.n])
    {
        return Err(DispatchError::ShapeConversion);
    }
    let mut output_dims = [1_u32; 4];
    output_dims[..rank].copy_from_slice(activation_dims);
    output_dims[rank - 1] = attributes.n;
    Ok(OutputShapes::one(
        RuntimeShape::new(&output_dims[..rank]).map_err(|_| DispatchError::ShapeConversion)?,
    ))
}

fn infer_quant_conv<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: QuantConvAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    let has_bias = attributes.bias_mode == BiasMode::QuantizedInt32;
    require_op_arity(work, if has_bias { 5 } else { 4 }, 1)?;
    if attributes.groups != 1 {
        return Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::DynamicQuantizedConv1d,
        });
    }
    let input = input_shape(memory, program, work, 0)?;
    let dims = input.dims();
    if dims.len() != 3
        || dims[1] != attributes.input_channels
        || input_shape(memory, program, work, 1)?.dims()
            != [
                attributes.output_channels,
                attributes.input_channels,
                attributes.kernel,
            ]
        || input_shape(memory, program, work, 2)?.rank() != 0
        || input_shape(memory, program, work, 3)?.rank() != 0
        || (has_bias
            && input_shape(memory, program, work, 4)?.dims() != [attributes.output_channels])
    {
        return Err(DispatchError::ShapeConversion);
    }
    let weight_scale = read_f32_scalar(memory, program, work, 2)?;
    let weight_zero = read_u8_scalar(memory, program, work, 3)?;
    if !weight_scale.is_finite()
        || weight_scale <= 0.0
        || u32::from(weight_zero) != attributes.weight_zero
    {
        return Err(DispatchError::InvalidControlTensor);
    }
    let params = trueos_ttstt_cpu::QConv1dParams {
        batch: dims[0] as usize,
        input_channels: dims[1] as usize,
        input_width: dims[2] as usize,
        output_channels: attributes.output_channels as usize,
        kernel_width: attributes.kernel as usize,
        stride: attributes.stride as usize,
        dilation: attributes.dilation as usize,
        pad_left: attributes.pad_left as usize,
        pad_right: attributes.pad_right as usize,
        input_zero_point: 0,
        weight_zero_points: trueos_ttstt_cpu::RhsZeroPoints::Scalar(
            trueos_ttstt_cpu::signed_u8_zero_point(weight_zero),
        ),
        weight_row_sums: None,
    };
    let output_width =
        u32::try_from(params.output_width()?).map_err(|_| DispatchError::ShapeConversion)?;
    Ok(OutputShapes::one(
        RuntimeShape::new(&[dims[0], attributes.output_channels, output_width])
            .map_err(|_| DispatchError::ShapeConversion)?,
    ))
}

fn infer_bilstm<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: BiLstmAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    require_op_arity(work, 6, 3)?;
    let expected_width = if attributes.profile == 1 { 512 } else { 640 };
    if attributes.input_width != expected_width {
        return Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::BiLstm256,
        });
    }
    let input = input_shape(memory, program, work, 0)?;
    let dims = input.dims();
    if dims.len() != 3
        || dims[0] == 0
        || dims[0] > trueos_kokoro_lstm::MAX_SEQUENCE_LENGTH as u32
        || dims[1] != 1
        || dims[2] != expected_width
        || input_shape(memory, program, work, 1)?.dims()
            != [2, trueos_kokoro_lstm::GATE_ELEMENTS as u32, expected_width]
        || input_shape(memory, program, work, 2)?.dims()
            != [
                2,
                trueos_kokoro_lstm::GATE_ELEMENTS as u32,
                trueos_kokoro_lstm::HIDDEN_SIZE as u32,
            ]
        || input_shape(memory, program, work, 3)?.dims()
            != [2, trueos_kokoro_lstm::BIAS_ELEMENTS_PER_DIRECTION as u32]
        || input_shape(memory, program, work, 4)?.dims()
            != [2, 1, trueos_kokoro_lstm::HIDDEN_SIZE as u32]
        || input_shape(memory, program, work, 5)?.dims()
            != [2, 1, trueos_kokoro_lstm::HIDDEN_SIZE as u32]
    {
        return Err(DispatchError::ShapeConversion);
    }
    let state = RuntimeShape::new(&[2, 1, trueos_kokoro_lstm::HIDDEN_SIZE as u32])
        .map_err(|_| DispatchError::ShapeConversion)?;
    Ok(OutputShapes::three(
        RuntimeShape::new(&[dims[0], 2, 1, trueos_kokoro_lstm::HIDDEN_SIZE as u32])
            .map_err(|_| DispatchError::ShapeConversion)?,
        state,
        state,
    ))
}

struct SmallControl<T: Copy, const CAPACITY: usize> {
    values: [T; CAPACITY],
    len: usize,
}

fn read_i64_control<
    const CAPACITY: usize,
    const SHAPES: usize,
    const EXTERNALS: usize,
    const BINDINGS: usize,
>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    input: u16,
) -> Result<SmallControl<i64, CAPACITY>, DispatchError> {
    let tensor = op_input(program, work, input)?;
    memory.with_read::<i64, _, _>(tensor, |values, _| {
        if values.len() > CAPACITY {
            return Err(DispatchError::InvalidControlTensor);
        }
        let mut copied = [0_i64; CAPACITY];
        copied[..values.len()].copy_from_slice(values);
        Ok(SmallControl {
            values: copied,
            len: values.len(),
        })
    })?
}

fn read_f32_control<
    const CAPACITY: usize,
    const SHAPES: usize,
    const EXTERNALS: usize,
    const BINDINGS: usize,
>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    input: u16,
) -> Result<SmallControl<f32, CAPACITY>, DispatchError> {
    let tensor = op_input(program, work, input)?;
    memory.with_read::<f32, _, _>(tensor, |values, _| {
        if values.len() > CAPACITY {
            return Err(DispatchError::InvalidControlTensor);
        }
        let mut copied = [0.0_f32; CAPACITY];
        copied[..values.len()].copy_from_slice(values);
        Ok(SmallControl {
            values: copied,
            len: values.len(),
        })
    })?
}

fn read_i64_scalar<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    input: u16,
) -> Result<i64, DispatchError> {
    let values = read_i64_control::<1, _, _, _>(memory, program, work, input)?;
    if values.len == 1 {
        Ok(values.values[0])
    } else {
        Err(DispatchError::InvalidControlTensor)
    }
}

fn read_f32_scalar<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    input: u16,
) -> Result<f32, DispatchError> {
    let values = read_f32_control::<1, _, _, _>(memory, program, work, input)?;
    if values.len == 1 {
        Ok(values.values[0])
    } else {
        Err(DispatchError::InvalidControlTensor)
    }
}

fn read_i32_scalar<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    input: u16,
) -> Result<i32, DispatchError> {
    let tensor = op_input(program, work, input)?;
    memory.with_read::<i32, _, _>(tensor, |values, _| {
        if values.len() == 1 {
            Ok(values[0])
        } else {
            Err(DispatchError::InvalidControlTensor)
        }
    })?
}

fn read_u8_scalar<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    input: u16,
) -> Result<u8, DispatchError> {
    let tensor = op_input(program, work, input)?;
    memory.with_read::<u8, _, _>(tensor, |values, _| {
        if values.len() == 1 {
            Ok(values[0])
        } else {
            Err(DispatchError::InvalidControlTensor)
        }
    })?
}

fn validate_stft_controls<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
) -> Result<(), DispatchError> {
    if read_i64_scalar(memory, program, work, 1)? != trueos_kokoro_stft::FRAME_STEP as i64
        || read_i64_scalar(memory, program, work, 3)? != trueos_kokoro_stft::FRAME_LENGTH as i64
    {
        return Err(DispatchError::InvalidControlTensor);
    }
    let window = read_f32_control::<{ trueos_kokoro_stft::FRAME_LENGTH }, _, _, _>(
        memory, program, work, 2,
    )?;
    if window.len != trueos_kokoro_stft::FRAME_LENGTH
        || window
            .values
            .iter()
            .zip(trueos_kokoro_stft::HANN_WINDOW_BITS)
            .any(|(value, expected)| value.to_bits() != expected)
    {
        return Err(DispatchError::InvalidControlTensor);
    }
    Ok(())
}

fn runtime_shape_i64(dimensions: &[i64]) -> Result<RuntimeShape, DispatchError> {
    let mut dims = [1_u32; 4];
    if dimensions.len() > dims.len() {
        return Err(DispatchError::ShapeConversion);
    }
    for (destination, &dimension) in dims.iter_mut().zip(dimensions) {
        *destination = u32::try_from(dimension).map_err(|_| DispatchError::ShapeConversion)?;
        if *destination == 0 {
            return Err(DispatchError::ShapeConversion);
        }
    }
    RuntimeShape::new(&dims[..dimensions.len()]).map_err(|_| DispatchError::ShapeConversion)
}

fn runtime_from_layout(shape: trueos_kokoro_layout::Shape) -> Result<RuntimeShape, DispatchError> {
    let mut dims = [1_u32; 4];
    for (destination, &dimension) in dims.iter_mut().zip(shape.dims()) {
        *destination = u32::try_from(dimension).map_err(|_| DispatchError::ShapeConversion)?;
    }
    RuntimeShape::new(&dims[..shape.rank()]).map_err(|_| DispatchError::ShapeConversion)
}

fn matmul_profile(
    attributes: MatMulAttributes,
    lhs: RuntimeShape,
    rhs: RuntimeShape,
) -> Result<(trueos_kokoro_gemm::KokoroMatMul, RuntimeShape), DispatchError> {
    require_rank(lhs, attributes.lhs_rank)?;
    require_rank(rhs, attributes.rhs_rank)?;
    let lhs_dims = lhs.dims();
    let rhs_dims = rhs.dims();
    let (profile, output) = match attributes.profile {
        1 => {
            if (
                attributes.constant_roles,
                attributes.k,
                attributes.n,
                attributes.lane,
                attributes.frame_axis,
            ) != (0, 64, 0, 64, 2)
                || lhs_dims[0] != 1
                || lhs_dims[1] != trueos_kokoro_gemm::ATTENTION_HEADS as u32
                || lhs_dims[3] != trueos_kokoro_gemm::ATTENTION_HEAD_WIDTH as u32
                || rhs_dims
                    != [
                        1,
                        trueos_kokoro_gemm::ATTENTION_HEADS as u32,
                        trueos_kokoro_gemm::ATTENTION_HEAD_WIDTH as u32,
                        lhs_dims[2],
                    ]
            {
                return Err(DispatchError::ShapeConversion);
            }
            let sequence =
                usize::try_from(lhs_dims[2]).map_err(|_| DispatchError::ShapeConversion)?;
            (
                trueos_kokoro_gemm::KokoroMatMul::AttentionScores { sequence },
                RuntimeShape::new(&[
                    1,
                    trueos_kokoro_gemm::ATTENTION_HEADS as u32,
                    lhs_dims[2],
                    lhs_dims[2],
                ])
                .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        2 => {
            if (
                attributes.constant_roles,
                attributes.k,
                attributes.n,
                attributes.lane,
                attributes.frame_axis,
            ) != (0, 0, 64, 64, 2)
                || lhs_dims[0] != 1
                || lhs_dims[1] != trueos_kokoro_gemm::ATTENTION_HEADS as u32
                || lhs_dims[3] != lhs_dims[2]
                || rhs_dims
                    != [
                        1,
                        trueos_kokoro_gemm::ATTENTION_HEADS as u32,
                        lhs_dims[2],
                        trueos_kokoro_gemm::ATTENTION_HEAD_WIDTH as u32,
                    ]
            {
                return Err(DispatchError::ShapeConversion);
            }
            let sequence =
                usize::try_from(lhs_dims[2]).map_err(|_| DispatchError::ShapeConversion)?;
            (
                trueos_kokoro_gemm::KokoroMatMul::AttentionContext { sequence },
                RuntimeShape::new(&[
                    1,
                    trueos_kokoro_gemm::ATTENTION_HEADS as u32,
                    lhs_dims[2],
                    trueos_kokoro_gemm::ATTENTION_HEAD_WIDTH as u32,
                ])
                .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        3 | 4 => {
            let (channels, expected_channels) = if attributes.profile == 3 {
                (trueos_kokoro_gemm::DurationChannels::Prosody640, 640)
            } else {
                (trueos_kokoro_gemm::DurationChannels::Text512, 512)
            };
            if (
                attributes.constant_roles,
                attributes.k,
                attributes.n,
                attributes.lane,
                attributes.frame_axis,
            ) != (0, expected_channels, 512, 1, 1)
                || lhs_dims[0] != 1
                || lhs_dims[1] != expected_channels
                || rhs_dims[0] != lhs_dims[2]
            {
                return Err(DispatchError::ShapeConversion);
            }
            let sequence =
                usize::try_from(lhs_dims[2]).map_err(|_| DispatchError::ShapeConversion)?;
            let frames =
                usize::try_from(rhs_dims[1]).map_err(|_| DispatchError::ShapeConversion)?;
            (
                trueos_kokoro_gemm::KokoroMatMul::DurationProjection {
                    channels,
                    sequence,
                    frames,
                },
                RuntimeShape::new(&[1, expected_channels, rhs_dims[1]])
                    .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        5 => {
            if (
                attributes.constant_roles,
                attributes.k,
                attributes.n,
                attributes.lane,
                attributes.frame_axis,
            ) != (0b10, 9, 1, 1, 1)
                || lhs_dims[0] != 1
                || lhs_dims[2] != 9
                || rhs_dims != [9, 1]
            {
                return Err(DispatchError::ShapeConversion);
            }
            let samples =
                usize::try_from(lhs_dims[1]).map_err(|_| DispatchError::ShapeConversion)?;
            (
                trueos_kokoro_gemm::KokoroMatMul::SourceLinear { samples },
                RuntimeShape::new(&[1, lhs_dims[1], 1])
                    .map_err(|_| DispatchError::ShapeConversion)?,
            )
        }
        _ => {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: OpCode::MatMul,
            });
        }
    };
    require_rank(output, attributes.output_rank)?;
    let dimensions = profile.dimensions()?;
    let lhs_elements = usize::try_from(
        lhs.element_count()
            .map_err(|_| DispatchError::ShapeConversion)?,
    )
    .map_err(|_| DispatchError::ShapeConversion)?;
    let rhs_elements = usize::try_from(
        rhs.element_count()
            .map_err(|_| DispatchError::ShapeConversion)?,
    )
    .map_err(|_| DispatchError::ShapeConversion)?;
    let output_elements = usize::try_from(
        output
            .element_count()
            .map_err(|_| DispatchError::ShapeConversion)?,
    )
    .map_err(|_| DispatchError::ShapeConversion)?;
    if dimensions.lhs_elements()? != lhs_elements
        || dimensions.rhs_elements()? != rhs_elements
        || dimensions.output_elements()? != output_elements
    {
        return Err(DispatchError::ShapeConversion);
    }
    Ok((profile, output))
}

fn resize_profile(
    attributes: ResizeAttributes,
) -> Result<(trueos_kokoro_resize::ResizeMode, trueos_kokoro_resize::ResizeScale, f32), DispatchError>
{
    let profile = match attributes.profile {
        1 => (
            trueos_kokoro_resize::ResizeMode::NearestAsymmetric,
            trueos_kokoro_resize::ResizeScale::Up2,
            2.0_f32,
        ),
        2 => (
            trueos_kokoro_resize::ResizeMode::NearestAsymmetric,
            trueos_kokoro_resize::ResizeScale::Up300,
            300.0_f32,
        ),
        3 => (
            trueos_kokoro_resize::ResizeMode::LinearHalfPixel,
            trueos_kokoro_resize::ResizeScale::Down300,
            1.0_f32 / 300.0_f32,
        ),
        4 => (
            trueos_kokoro_resize::ResizeMode::LinearHalfPixel,
            trueos_kokoro_resize::ResizeScale::Up300,
            300.0_f32,
        ),
        _ => {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: OpCode::Resize,
            });
        }
    };
    let expected_mode = match attributes.mode {
        ResizeMode::Nearest => trueos_kokoro_resize::ResizeMode::NearestAsymmetric,
        ResizeMode::Linear => trueos_kokoro_resize::ResizeMode::LinearHalfPixel,
    };
    if profile.0 != expected_mode {
        return Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::Resize,
        });
    }
    Ok(profile)
}

fn conv_profile(
    attributes: FloatConvAttributes,
    input_width: usize,
) -> Result<trueos_kokoro_conv::Profile, DispatchError> {
    match attributes.profile {
        1 if attributes.opcode == OpCode::FloatConv1d => {
            Ok(trueos_kokoro_conv::Profile::PostConv128To22 { input_width })
        }
        2 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            Ok(trueos_kokoro_conv::Profile::EncoderDepthwise512 { input_width })
        }
        3 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            Ok(trueos_kokoro_conv::Profile::DecoderDepthwise1090 { input_width })
        }
        4 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            Ok(trueos_kokoro_conv::Profile::Upsample512To256 { input_width })
        }
        5 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            Ok(trueos_kokoro_conv::Profile::Upsample256To128 { input_width })
        }
        6 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            Ok(trueos_kokoro_conv::Profile::Istft22To1 { input_width })
        }
        _ => Err(DispatchError::UnsupportedAttributeProfile {
            opcode: attributes.opcode,
        }),
    }
}

fn validate_conv_attributes(
    attributes: FloatConvAttributes,
    profile: trueos_kokoro_conv::Profile,
) -> Result<(), DispatchError> {
    let parameters = profile.parameters();
    if parameters.input_channels != attributes.input_channels as usize
        || parameters.output_channels != attributes.output_channels as usize
        || parameters.kernel_width != attributes.kernel as usize
        || parameters.stride != attributes.stride as usize
        || attributes.dilation != 1
        || parameters.pad_left != attributes.pad_left as usize
        || parameters.pad_right != attributes.pad_right as usize
        || parameters.output_padding != attributes.output_padding as usize
        || parameters.groups != attributes.groups as usize
        || parameters.has_bias != attributes.has_bias
    {
        Err(DispatchError::UnsupportedAttributeProfile {
            opcode: attributes.opcode,
        })
    } else {
        Ok(())
    }
}

fn infer_transpose<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: TransposeAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    require_op_arity(work, 1, 1)?;
    let input = input_shape(memory, program, work, 0)?;
    require_rank(input, attributes.rank)?;
    let mut dims = [1_u32; 4];
    for (output_axis, destination) in dims
        .iter_mut()
        .enumerate()
        .take(usize::from(attributes.rank))
    {
        *destination = input.dims()[attributes.permutation[output_axis] as usize];
    }
    Ok(OutputShapes::one(
        RuntimeShape::new(&dims[..usize::from(attributes.rank)])
            .map_err(|_| DispatchError::ShapeConversion)?,
    ))
}

fn infer_gather<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: GatherAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    require_op_arity(work, 2, 1)?;
    let data = input_shape(memory, program, work, 0)?;
    let indices = input_shape(memory, program, work, 1)?;
    require_rank(data, attributes.data_rank)?;
    require_rank(indices, attributes.indices_rank)?;
    let axis = normalize_axis(attributes.axis, data.rank())?;
    let mut dims = [1_u32; 4];
    dims[..axis].copy_from_slice(&data.dims()[..axis]);
    let indices_end = axis + indices.dims().len();
    dims[axis..indices_end].copy_from_slice(indices.dims());
    dims[indices_end..usize::from(attributes.output_rank)]
        .copy_from_slice(&data.dims()[axis + 1..]);
    Ok(OutputShapes::one(
        RuntimeShape::new(&dims[..usize::from(attributes.output_rank)])
            .map_err(|_| DispatchError::ShapeConversion)?,
    ))
}

fn infer_concat<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: ConcatAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    require_op_arity(work, u16::from(attributes.input_count), 1)?;
    let first = input_shape(memory, program, work, 0)?;
    require_rank(first, attributes.rank)?;
    let axis = normalize_axis(attributes.axis, first.rank())?;
    let mut dims = [1_u32; 4];
    dims[..first.dims().len()].copy_from_slice(first.dims());
    dims[axis] = 0;
    for input in 0..u16::from(attributes.input_count) {
        let shape = input_shape(memory, program, work, input)?;
        if shape.rank() != first.rank() {
            return Err(DispatchError::ShapeConversion);
        }
        for dimension in 0..shape.dims().len() {
            if dimension != axis && shape.dims()[dimension] != first.dims()[dimension] {
                return Err(DispatchError::ShapeConversion);
            }
        }
        dims[axis] = dims[axis]
            .checked_add(shape.dims()[axis])
            .ok_or(DispatchError::ShapeConversion)?;
    }
    Ok(OutputShapes::one(
        RuntimeShape::new(&dims[..first.dims().len()])
            .map_err(|_| DispatchError::ShapeConversion)?,
    ))
}

fn infer_split<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: SplitAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    require_op_arity(work, 2, 2)?;
    let input = input_shape(memory, program, work, 0)?;
    require_rank(input, attributes.rank)?;
    let lengths = read_i64_control::<2, _, _, _>(memory, program, work, 1)?;
    if lengths.len != 2
        || lengths.values[0] != i64::from(attributes.first_axis_len)
        || lengths.values[1] != i64::from(attributes.second_axis_len)
    {
        return Err(DispatchError::InvalidControlTensor);
    }
    let axis = normalize_axis(attributes.axis, input.rank())?;
    if input.dims()[axis]
        != attributes
            .first_axis_len
            .checked_add(attributes.second_axis_len)
            .ok_or(DispatchError::ShapeConversion)?
    {
        return Err(DispatchError::ShapeConversion);
    }
    let mut first = [1_u32; 4];
    let mut second = [1_u32; 4];
    first[..input.dims().len()].copy_from_slice(input.dims());
    second[..input.dims().len()].copy_from_slice(input.dims());
    first[axis] = attributes.first_axis_len;
    second[axis] = attributes.second_axis_len;
    Ok(OutputShapes::two(
        RuntimeShape::new(&first[..input.dims().len()])
            .map_err(|_| DispatchError::ShapeConversion)?,
        RuntimeShape::new(&second[..input.dims().len()])
            .map_err(|_| DispatchError::ShapeConversion)?,
    ))
}

fn infer_expand<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: ExpandAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    require_op_arity(work, 2, 1)?;
    let input = input_shape(memory, program, work, 0)?;
    require_rank(input, attributes.input_rank)?;
    let target = read_i64_control::<4, _, _, _>(memory, program, work, 1)?;
    if target.len != usize::from(attributes.output_rank) {
        return Err(DispatchError::InvalidControlTensor);
    }
    if attributes.control_mode == ControlMode::Initializer {
        for (index, &dimension) in target.values[..target.len].iter().enumerate() {
            if dimension != i64::from(attributes.target_dims[index]) {
                return Err(DispatchError::InvalidControlTensor);
            }
        }
    }
    let target_shape = runtime_shape_i64(&target.values[..target.len])?;
    let mut target_dims = [1usize; 4];
    for (destination, &dimension) in target_dims.iter_mut().zip(target_shape.dims()) {
        *destination = usize::try_from(dimension).map_err(|_| DispatchError::ShapeConversion)?;
    }
    let output =
        trueos_kokoro_layout::expand_shape(layout_shape(input)?, &target_dims[..target.len])?;
    Ok(OutputShapes::one(runtime_from_layout(output)?))
}

fn infer_slice<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: SliceAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    let control_count =
        2 + u16::from(attributes.flags & 1 != 0) + u16::from(attributes.flags & 2 != 0);
    require_op_arity(work, 1 + control_count, 1)?;
    let input = input_shape(memory, program, work, 0)?;
    require_rank(input, attributes.rank)?;
    let mut controls = [0_i64; 4];
    for (slot, control) in controls
        .iter_mut()
        .enumerate()
        .take(usize::from(control_count))
    {
        *control = read_i64_scalar(memory, program, work, slot as u16 + 1)?;
        if attributes.control_modes[slot] == ControlMode::Initializer
            && *control != attributes.control_values[slot]
        {
            return Err(DispatchError::InvalidControlTensor);
        }
    }
    let axis = if attributes.flags & 1 != 0 {
        normalize_axis(
            i32::try_from(controls[2]).map_err(|_| DispatchError::InvalidControlTensor)?,
            input.rank(),
        )?
    } else {
        0
    };
    if attributes.flags & 2 != 0 && controls[3] != 1 {
        return Err(DispatchError::InvalidControlTensor);
    }
    let dimension = i64::from(input.dims()[axis]);
    let start = normalize_slice_bound(controls[0], dimension);
    let end = normalize_slice_bound(controls[1], dimension);
    let length =
        u32::try_from(end.saturating_sub(start)).map_err(|_| DispatchError::ShapeConversion)?;
    let mut dims = [1_u32; 4];
    dims[..input.dims().len()].copy_from_slice(input.dims());
    dims[axis] = length;
    Ok(OutputShapes::one(
        RuntimeShape::new(&dims[..input.dims().len()])
            .map_err(|_| DispatchError::ShapeConversion)?,
    ))
}

fn normalize_slice_bound(bound: i64, dimension: i64) -> i64 {
    if bound < 0 {
        bound.saturating_add(dimension).clamp(0, dimension)
    } else {
        bound.clamp(0, dimension)
    }
}

fn infer_pad<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: PadAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    require_op_arity(work, 2, 1)?;
    let input = input_shape(memory, program, work, 0)?;
    require_rank(input, attributes.rank)?;
    let count = usize::from(attributes.rank) * 2;
    let pads = read_i64_control::<8, _, _, _>(memory, program, work, 1)?;
    if pads.len != count {
        return Err(DispatchError::InvalidControlTensor);
    }
    let mut dims = [1_u32; 4];
    dims[..input.dims().len()].copy_from_slice(input.dims());
    for (axis, dimension) in dims
        .iter_mut()
        .enumerate()
        .take(usize::from(attributes.rank))
    {
        if pads.values[axis] != i64::from(attributes.pads[axis])
            || pads.values[usize::from(attributes.rank) + axis]
                != i64::from(attributes.pads[usize::from(attributes.rank) + axis])
        {
            return Err(DispatchError::InvalidControlTensor);
        }
        let before = attributes.pads[axis];
        let after = attributes.pads[usize::from(attributes.rank) + axis];
        if (before != 0 && before >= input.dims()[axis])
            || (after != 0 && after >= input.dims()[axis])
        {
            return Err(DispatchError::ShapeConversion);
        }
        *dimension = before
            .checked_add(input.dims()[axis])
            .and_then(|value| value.checked_add(after))
            .ok_or(DispatchError::ShapeConversion)?;
    }
    Ok(OutputShapes::one(
        RuntimeShape::new(&dims[..input.dims().len()])
            .map_err(|_| DispatchError::ShapeConversion)?,
    ))
}

fn infer_view<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: ViewAttributes,
) -> Result<OutputShapes, DispatchError> {
    require_whole_unit(work)?;
    require_op_arity(work, 2, 1)?;
    let input = input_shape(memory, program, work, 0)?;
    require_rank(input, attributes.input_rank)?;
    let control = read_i64_control::<4, _, _, _>(memory, program, work, 1)?;
    if attributes.static_control {
        if control.len != usize::from(attributes.count) {
            return Err(DispatchError::InvalidControlTensor);
        }
        for (index, &value) in control.values[..control.len].iter().enumerate() {
            if value != i64::from(attributes.parameters[index]) {
                return Err(DispatchError::InvalidControlTensor);
            }
        }
    }
    let input_layout = layout_shape(input)?;
    let output_layout = match attributes.opcode {
        OpCode::Reshape => {
            trueos_kokoro_layout::reshape_view(input_layout, &control.values[..control.len], false)?
        }
        OpCode::Unsqueeze => {
            let mut axes = [0_isize; 4];
            for (destination, &axis) in axes.iter_mut().zip(&control.values[..control.len]) {
                *destination = isize::try_from(axis).map_err(|_| DispatchError::ShapeConversion)?;
            }
            trueos_kokoro_layout::unsqueeze_view(input_layout, &axes[..control.len])?
        }
        OpCode::Squeeze => {
            let mut axes = [0_isize; 4];
            for (destination, &axis) in axes.iter_mut().zip(&control.values[..control.len]) {
                *destination = isize::try_from(axis).map_err(|_| DispatchError::ShapeConversion)?;
            }
            trueos_kokoro_layout::squeeze_view(input_layout, Some(&axes[..control.len]))?
        }
        opcode => return Err(DispatchError::UnsupportedOpcode { opcode }),
    };
    let output = runtime_from_layout(output_layout)?;
    require_rank(output, attributes.output_rank)?;
    validate_view_descriptor(program, work, attributes)?;
    Ok(OutputShapes::one(output))
}

fn validate_view_descriptor(
    program: &Program<'_>,
    work: WorkSlice,
    attributes: ViewAttributes,
) -> Result<(), DispatchError> {
    let input_id = op_input(program, work, 0)?;
    let output_id = program
        .op_output(work.op(), 0)
        .ok_or(DispatchError::InvalidArity)?;
    let input = program
        .tensor(input_id)
        .ok_or(DispatchError::ShapeConversion)?;
    let output = program
        .tensor(output_id)
        .ok_or(DispatchError::ShapeConversion)?;
    let expected_dtype = match attributes.dtype {
        AttributeDType::Float => DType::F32,
        AttributeDType::Int32 => DType::I32,
        AttributeDType::Int64 => DType::I64,
        _ => return Err(DispatchError::ShapeConversion),
    };
    let root = if input.storage == StorageKind::View {
        input.view_of
    } else {
        input_id
    };
    let input_storage = program
        .resolve_storage(input_id)
        .map_err(|_| DispatchError::ShapeConversion)?;
    let output_storage = program
        .resolve_storage(output_id)
        .map_err(|_| DispatchError::ShapeConversion)?;
    if input.dtype != expected_dtype
        || output.dtype != expected_dtype
        || output.storage != StorageKind::View
        || output.view_of != root
        || output_storage.owner != input_storage.owner
        || output_storage.offset != input_storage.offset
    {
        return Err(DispatchError::ShapeConversion);
    }
    Ok(())
}

fn validate_view_storage<const SHAPES: usize, const EXTERNALS: usize, const BINDINGS: usize>(
    memory: &TensorMemory<'_, '_, '_, SHAPES, EXTERNALS, BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: ViewAttributes,
    expected: RuntimeShape,
) -> Result<(), DispatchError> {
    let output = program
        .op_output(work.op(), 0)
        .ok_or(DispatchError::InvalidArity)?;
    let actual = match attributes.dtype {
        AttributeDType::Float => memory.with_read::<f32, _, _>(output, |_, shape| shape)?,
        AttributeDType::Int32 => memory.with_read::<i32, _, _>(output, |_, shape| shape)?,
        AttributeDType::Int64 => memory.with_read::<i64, _, _>(output, |_, shape| shape)?,
        _ => return Err(DispatchError::ShapeConversion),
    };
    if actual == expected {
        Ok(())
    } else {
        Err(DispatchError::ShapeConversion)
    }
}

impl<
    'dispatch,
    'memory,
    'artifact,
    'buffers,
    'workspace,
    const SHAPES: usize,
    const EXTERNALS: usize,
    const BINDINGS: usize,
> ExecDispatcher
    for CpuDispatcher<
        'dispatch,
        'memory,
        'artifact,
        'buffers,
        'workspace,
        SHAPES,
        EXTERNALS,
        BINDINGS,
    >
{
    type Error = DispatchError;

    fn dispatch(
        &mut self,
        program: &Program<'_>,
        work: WorkSlice,
    ) -> Result<DispatchResult, Self::Error> {
        let record = program
            .op_attributes(work.op())
            .ok_or(DispatchError::MissingAttributes)?;
        let attributes = decode(record, work.op().opcode)?;
        if !native_dispatch_supported(attributes) {
            return Err(DispatchError::UnsupportedOpcode {
                opcode: attributes.opcode(),
            });
        }
        if native_dispatch_requires_workspace(attributes) && self.workspace.is_none() {
            return Err(DispatchError::WorkspaceRequired {
                opcode: attributes.opcode(),
            });
        }
        let output_shapes = infer_output_shapes(self.memory, program, work, attributes)?;
        self.memory
            .declare_op_outputs(work.op_index(), output_shapes.as_slice())?;
        if let Attributes::View(view) = attributes {
            validate_view_storage(self.memory, program, work, view, output_shapes.values[0])?;
            return Ok(DispatchResult::Completed);
        }
        let conv = self.conv;
        let gemm = self.gemm;
        let quant = self.quant;
        let workspace = self.workspace.as_deref_mut();
        self.memory
            .with_op(work.op_index(), |access| {
                execute(access, program, work, attributes, conv, gemm, quant, workspace)
            })
            .map_err(DispatchError::Memory)?
    }
}

fn execute<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    program: &Program<'_>,
    work: WorkSlice,
    attributes: Attributes,
    conv: trueos_kokoro_conv::Dispatcher,
    gemm: trueos_kokoro_gemm::Dispatcher,
    quant: trueos_ttstt_cpu::Dispatcher,
    workspace: Option<&mut CpuWorkspace<'_>>,
) -> Result<DispatchResult, DispatchError> {
    match attributes {
        Attributes::Binary(value) => {
            require_whole_unit(work)?;
            execute_binary(access, work, value)?;
        }
        Attributes::Comparison(value) => {
            require_whole_unit(work)?;
            execute_comparison(access, value)?;
        }
        Attributes::Cast(value) => {
            require_whole_unit(work)?;
            execute_cast(access, value)?;
        }
        Attributes::ConstantOfShape(value) => {
            require_whole_unit(work)?;
            require_arity(access, 1, 1)?;
            let dimensions = access.input::<i64>(0)?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_scalar::constant_of_shape_f32(
                &dimensions,
                f32::from_bits(value.fill_bits),
                &mut output,
            )?;
        }
        Attributes::CumSum(value) => {
            require_whole_unit(work)?;
            require_arity(access, 2, 1)?;
            let input = access.input::<f32>(0)?;
            let axis = access.input::<i32>(1)?;
            if axis.len() != 1 || axis[0] != value.axis {
                return Err(DispatchError::InvalidControlTensor);
            }
            let shape = layout_shape(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_scalar::cumulative_sum_f32(
                &input,
                shape,
                value.axis as isize,
                &mut output,
            )?;
        }
        Attributes::DequantizeLinear(_) => {
            require_whole_unit(work)?;
            require_arity(access, 3, 1)?;
            let input = access.input::<i8>(0)?;
            let scale = access.input::<f32>(1)?;
            let zero = access.input::<i8>(2)?;
            if scale.len() != 1 || zero.len() != 1 {
                return Err(DispatchError::InvalidControlTensor);
            }
            let shape = layout_shape(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_scalar::dequantize_linear_i8_scalar(
                &input,
                shape,
                scale[0],
                zero[0],
                &mut output,
            )?;
        }
        Attributes::Where(value) => {
            require_whole_unit(work)?;
            execute_where(access, value.dtype)?;
        }
        Attributes::MatMul(value) => {
            require_whole_unit(work)?;
            execute_matmul(access, value, gemm)?;
        }
        Attributes::DynamicQuantizedGemm(value) => {
            require_whole_unit(work)?;
            execute_quant_gemm(
                access,
                value,
                quant,
                require_workspace(workspace, OpCode::DynamicQuantizedGemm)?,
            )?;
        }
        Attributes::DynamicQuantizedConv1d(value) => {
            require_whole_unit(work)?;
            execute_quant_conv(
                access,
                value,
                quant,
                require_workspace(workspace, OpCode::DynamicQuantizedConv1d)?,
            )?;
        }
        Attributes::BiLstm256(value) => {
            require_whole_unit(work)?;
            execute_bilstm(access, value, require_workspace(workspace, OpCode::BiLstm256)?)?;
        }
        Attributes::Pow(_) => {
            require_whole_unit(work)?;
            require_arity(access, 2, 1)?;
            let input = access.input::<f32>(0)?;
            let exponent = access.input::<f32>(1)?;
            if exponent.len() != 1 || exponent[0].to_bits() != 2.0_f32.to_bits() {
                return Err(DispatchError::InvalidControlTensor);
            }
            let input_layout = contiguous_f32(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            let output_layout = contiguous_f32(output.shape())?;
            trueos_kokoro_f32::pow_square(&input, input_layout, &mut output, output_layout)?;
        }
        Attributes::Range => {
            require_whole_unit(work)?;
            require_arity(access, 3, 1)?;
            let start = access.input::<i64>(0)?;
            let limit = access.input::<i64>(1)?;
            let delta = access.input::<i64>(2)?;
            if start.len() != 1 || limit.len() != 1 || delta.len() != 1 {
                return Err(DispatchError::InvalidControlTensor);
            }
            let mut output = access.output::<i64>(0)?;
            trueos_kokoro_scalar::range_i64(start[0], limit[0], delta[0], &mut output)?;
        }
        Attributes::Resize(value) => return execute_resize(access, work, value),
        Attributes::FloatConv(value) => {
            return execute_float_conv(access, work, value, conv);
        }
        Attributes::Unary(value) => {
            require_whole_unit(work)?;
            execute_unary(access, work, value)?;
        }
        Attributes::LeakyRelu(value) => {
            require_whole_unit(work)?;
            require_arity(access, 1, 1)?;
            let input = access.input::<f32>(0)?;
            let input_layout = contiguous_f32(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            let output_layout = contiguous_f32(output.shape())?;
            trueos_kokoro_f32::leaky_relu(
                &input,
                input_layout,
                f32::from_bits(value.alpha_bits),
                &mut output,
                output_layout,
            )?;
        }
        Attributes::ReduceMean(value) => {
            require_whole_unit(work)?;
            require_arity(access, 2, 1)?;
            let input = access.input::<f32>(0)?;
            let axes = access.input::<i64>(1)?;
            if axes.len() != 1 || axes[0] != i64::from(value.axis) {
                return Err(DispatchError::InvalidControlTensor);
            }
            let shape = f32_shape(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_f32::reduce_mean(&input, shape, value.axis as isize, true, &mut output)?;
        }
        Attributes::LayerNormalization(value) => {
            require_whole_unit(work)?;
            require_arity(access, 3, 1)?;
            let input = access.input::<f32>(0)?;
            let scale = access.input::<f32>(1)?;
            let bias = access.input::<f32>(2)?;
            let shape = f32_shape(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_f32::layer_normalization(
                &input,
                shape,
                value.axis as isize,
                &scale,
                &bias,
                f32::from_bits(value.epsilon_bits),
                &mut output,
            )?;
        }
        Attributes::Softmax(value) => {
            require_whole_unit(work)?;
            require_arity(access, 1, 1)?;
            let input = access.input::<f32>(0)?;
            let shape = f32_shape(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_f32::softmax(&input, shape, value.axis as isize, &mut output)?;
        }
        Attributes::FastGelu(_) => {
            require_whole_unit(work)?;
            require_arity(access, 2, 1)?;
            let input = access.input::<f32>(0)?;
            let bias = access.input::<f32>(1)?;
            let shape = f32_shape(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_f32::fast_gelu(&input, shape, Some(&bias), &mut output)?;
        }
        Attributes::SkipLayerNormalization(value) => {
            require_whole_unit(work)?;
            require_arity(access, 4, 1)?;
            let input = access.input::<f32>(0)?;
            let skip = access.input::<f32>(1)?;
            let scale = access.input::<f32>(2)?;
            let bias = access.input::<f32>(3)?;
            let shape = f32_shape(input.shape())?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_f32::skip_layer_normalization(
                &input,
                &skip,
                shape,
                &scale,
                &bias,
                f32::from_bits(value.epsilon_bits),
                &mut output,
            )?;
        }
        Attributes::ResolveDecoderShape(_) => {
            require_whole_unit(work)?;
            return execute_resolve(access, program);
        }
        Attributes::FixedStft20(_) => execute_stft(access)?,
        Attributes::Transpose(value) => execute_transpose(access, value)?,
        Attributes::Gather(value) => execute_gather(access, value)?,
        Attributes::Concat(value) => execute_concat(access, value)?,
        Attributes::Split(value) => execute_split(access, value)?,
        Attributes::Expand(value) => execute_expand(access, value)?,
        Attributes::Shape(_) => execute_shape(access, program)?,
        Attributes::Slice(value) => execute_slice(access, value)?,
        Attributes::Pad(value) => execute_pad(access, value)?,
        Attributes::NonZero => execute_nonzero(access)?,
        Attributes::ScatterNd(_) => execute_scatter(access)?,
        Attributes::View(_) => {
            return Err(DispatchError::UnsupportedOpcode {
                opcode: attributes.opcode(),
            });
        }
    }
    Ok(DispatchResult::Completed)
}

fn require_workspace<'workspace, 'buffers>(
    workspace: Option<&'workspace mut CpuWorkspace<'buffers>>,
    opcode: OpCode,
) -> Result<&'workspace mut CpuWorkspace<'buffers>, DispatchError> {
    workspace.ok_or(DispatchError::WorkspaceRequired { opcode })
}

fn execute_transpose<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: TransposeAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 1, 1)?;
    let mut permutation = [0_usize; 4];
    for (destination, &axis) in permutation
        .iter_mut()
        .zip(&attributes.permutation[..usize::from(attributes.rank)])
    {
        *destination = usize::try_from(axis).map_err(|_| DispatchError::ShapeConversion)?;
    }
    match attributes.dtype {
        AttributeDType::Float => execute_transpose_t::<f32, BINDINGS>(
            access,
            &permutation[..usize::from(attributes.rank)],
        ),
        AttributeDType::Int64 => execute_transpose_t::<i64, BINDINGS>(
            access,
            &permutation[..usize::from(attributes.rank)],
        ),
        _ => Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::Transpose,
        }),
    }
}

fn execute_transpose_t<T: TensorElement, const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    permutation: &[usize],
) -> Result<(), DispatchError> {
    let input = access.input::<T>(0)?;
    let shape = layout_shape(input.shape())?;
    let mut output = access.output::<T>(0)?;
    trueos_kokoro_layout::transpose(&input, shape, permutation, &mut output)?;
    Ok(())
}

fn execute_gather<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: GatherAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 2, 1)?;
    match attributes.dtype {
        AttributeDType::Float => execute_gather_t::<f32, BINDINGS>(access, attributes.axis),
        AttributeDType::Int8 => execute_gather_t::<i8, BINDINGS>(access, attributes.axis),
        AttributeDType::Int64 => execute_gather_t::<i64, BINDINGS>(access, attributes.axis),
        _ => Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::Gather,
        }),
    }
}

fn execute_gather_t<T: TensorElement, const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    axis: i32,
) -> Result<(), DispatchError> {
    let data = access.input::<T>(0)?;
    let indices = access.input::<i64>(1)?;
    let data_shape = layout_shape(data.shape())?;
    let indices_shape = layout_shape(indices.shape())?;
    let mut output = access.output::<T>(0)?;
    trueos_kokoro_layout::gather(
        &data,
        data_shape,
        &indices,
        indices_shape,
        axis as isize,
        &mut output,
    )?;
    Ok(())
}

fn execute_concat<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: ConcatAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, u16::from(attributes.input_count), 1)?;
    match attributes.dtype {
        AttributeDType::Float => {
            execute_concat_t::<f32, BINDINGS>(access, attributes.axis, attributes.input_count)
        }
        AttributeDType::Int64 => {
            execute_concat_t::<i64, BINDINGS>(access, attributes.axis, attributes.input_count)
        }
        _ => Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::Concat,
        }),
    }
}

fn execute_concat_t<T: TensorElement, const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    axis: i32,
    count: u8,
) -> Result<(), DispatchError> {
    let first = access.input::<T>(0)?;
    let first_shape = layout_shape(first.shape())?;
    let second = access.input::<T>(1)?;
    let second_shape = layout_shape(second.shape())?;
    let mut output = access.output::<T>(0)?;
    match count {
        2 => {
            let inputs = [
                trueos_kokoro_layout::TensorView::new(&first, first_shape)?,
                trueos_kokoro_layout::TensorView::new(&second, second_shape)?,
            ];
            trueos_kokoro_layout::concat(&inputs, axis as isize, &mut output)?;
        }
        3 => {
            let third = access.input::<T>(2)?;
            let inputs = [
                trueos_kokoro_layout::TensorView::new(&first, first_shape)?,
                trueos_kokoro_layout::TensorView::new(&second, second_shape)?,
                trueos_kokoro_layout::TensorView::new(&third, layout_shape(third.shape())?)?,
            ];
            trueos_kokoro_layout::concat(&inputs, axis as isize, &mut output)?;
        }
        4 => {
            let third = access.input::<T>(2)?;
            let fourth = access.input::<T>(3)?;
            let inputs = [
                trueos_kokoro_layout::TensorView::new(&first, first_shape)?,
                trueos_kokoro_layout::TensorView::new(&second, second_shape)?,
                trueos_kokoro_layout::TensorView::new(&third, layout_shape(third.shape())?)?,
                trueos_kokoro_layout::TensorView::new(&fourth, layout_shape(fourth.shape())?)?,
            ];
            trueos_kokoro_layout::concat(&inputs, axis as isize, &mut output)?;
        }
        _ => return Err(DispatchError::InvalidArity),
    }
    Ok(())
}

fn execute_split<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: SplitAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 2, 2)?;
    let input = access.input::<f32>(0)?;
    let lengths = access.input::<i64>(1)?;
    if lengths.len() != 2
        || lengths[0] != i64::from(attributes.first_axis_len)
        || lengths[1] != i64::from(attributes.second_axis_len)
    {
        return Err(DispatchError::InvalidControlTensor);
    }
    let shape = layout_shape(input.shape())?;
    let mut first = access.output::<f32>(0)?;
    let mut second = access.output::<f32>(1)?;
    trueos_kokoro_layout::split_two(
        &input,
        shape,
        attributes.axis as isize,
        attributes.first_axis_len as usize,
        &mut first,
        &mut second,
    )?;
    Ok(())
}

fn execute_expand<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: ExpandAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 2, 1)?;
    match attributes.dtype {
        AttributeDType::Float => execute_expand_t::<f32, BINDINGS>(access),
        AttributeDType::Int64 => execute_expand_t::<i64, BINDINGS>(access),
        _ => Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::Expand,
        }),
    }
}

fn execute_expand_t<T: TensorElement, const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
) -> Result<(), DispatchError> {
    let input = access.input::<T>(0)?;
    let target = access.input::<i64>(1)?;
    let input_shape = layout_shape(input.shape())?;
    let mut dims = [0_usize; 4];
    if target.len() > dims.len() {
        return Err(DispatchError::InvalidControlTensor);
    }
    for (destination, &dimension) in dims.iter_mut().zip(target.iter()) {
        *destination =
            usize::try_from(dimension).map_err(|_| DispatchError::InvalidControlTensor)?;
        if *destination == 0 {
            return Err(DispatchError::InvalidControlTensor);
        }
    }
    let mut output = access.output::<T>(0)?;
    trueos_kokoro_layout::expand(&input, input_shape, &dims[..target.len()], &mut output)?;
    Ok(())
}

fn execute_shape<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    program: &Program<'_>,
) -> Result<(), DispatchError> {
    require_arity(access, 1, 1)?;
    let tensor_id = access
        .input_tensor_id(0)
        .ok_or(DispatchError::InvalidArity)?;
    let dtype = program
        .tensor(tensor_id)
        .ok_or(DispatchError::ShapeConversion)?
        .dtype;
    let shape = match dtype {
        DType::F32 => access.input::<f32>(0)?.shape(),
        DType::I64 => access.input::<i64>(0)?.shape(),
        _ => {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: OpCode::Shape,
            });
        }
    };
    let mut output = access.output::<i64>(0)?;
    trueos_kokoro_layout::shape_of(layout_shape(shape)?, 0, None, &mut output)?;
    Ok(())
}

fn execute_slice<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: SliceAttributes,
) -> Result<(), DispatchError> {
    require_arity(
        access,
        3 + u16::from(attributes.flags & 1 != 0) + u16::from(attributes.flags & 2 != 0),
        1,
    )?;
    match attributes.dtype {
        AttributeDType::Float => execute_slice_t::<f32, BINDINGS>(access, attributes),
        AttributeDType::Int64 => execute_slice_t::<i64, BINDINGS>(access, attributes),
        _ => Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::Slice,
        }),
    }
}

fn execute_slice_t<T: TensorElement, const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: SliceAttributes,
) -> Result<(), DispatchError> {
    let input = access.input::<T>(0)?;
    let starts = access.input::<i64>(1)?;
    let ends = access.input::<i64>(2)?;
    if starts.len() != 1 || ends.len() != 1 {
        return Err(DispatchError::InvalidControlTensor);
    }
    let axes = if attributes.flags & 1 != 0 {
        let value = access.input::<i64>(3)?;
        if value.len() != 1 {
            return Err(DispatchError::InvalidControlTensor);
        }
        Some(value)
    } else {
        None
    };
    let steps_index = if attributes.flags & 1 != 0 { 4 } else { 3 };
    let steps = if attributes.flags & 2 != 0 {
        let value = access.input::<i64>(steps_index)?;
        if value.len() != 1 {
            return Err(DispatchError::InvalidControlTensor);
        }
        Some(value)
    } else {
        None
    };
    let mut axis_values = [0_isize; 1];
    let axes_slice = if let Some(values) = axes.as_ref() {
        axis_values[0] =
            isize::try_from(values[0]).map_err(|_| DispatchError::InvalidControlTensor)?;
        Some(&axis_values[..])
    } else {
        None
    };
    let mut output = access.output::<T>(0)?;
    trueos_kokoro_layout::slice(
        &input,
        layout_shape(input.shape())?,
        &starts,
        &ends,
        axes_slice,
        steps.as_deref(),
        &mut output,
    )?;
    Ok(())
}

fn execute_pad<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: PadAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 2, 1)?;
    let input = access.input::<f32>(0)?;
    let control = access.input::<i64>(1)?;
    let count = usize::from(attributes.rank) * 2;
    if control.len() != count {
        return Err(DispatchError::InvalidControlTensor);
    }
    let mut pads = [0_usize; 8];
    for index in 0..count {
        if control[index] != i64::from(attributes.pads[index]) {
            return Err(DispatchError::InvalidControlTensor);
        }
        pads[index] = attributes.pads[index] as usize;
    }
    let mut output = access.output::<f32>(0)?;
    trueos_kokoro_layout::reflect_pad(
        &input,
        layout_shape(input.shape())?,
        &pads[..count],
        &mut output,
    )?;
    Ok(())
}

fn execute_nonzero<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
) -> Result<(), DispatchError> {
    require_arity(access, 1, 1)?;
    let input = access.input::<bool>(0)?;
    let mut output = access.output::<i64>(0)?;
    let count =
        trueos_kokoro_layout::nonzero_bool(&input, layout_shape(input.shape())?, &mut output)?;
    let output_shape = output.shape();
    let dims = output_shape.dims();
    if dims != [input.shape().rank() as u32, count as u32] {
        return Err(DispatchError::ShapeConversion);
    }
    Ok(())
}

fn execute_scatter<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
) -> Result<(), DispatchError> {
    require_arity(access, 3, 1)?;
    let data = access.input::<f32>(0)?;
    let indices = access.input::<i64>(1)?;
    let updates = access.input::<f32>(2)?;
    let mut output = access.output::<f32>(0)?;
    trueos_kokoro_layout::scatter_nd_ordered(
        &data,
        layout_shape(data.shape())?,
        &indices,
        layout_shape(indices.shape())?,
        &updates,
        &mut output,
    )?;
    Ok(())
}

fn execute_quant_gemm<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: QuantGemmAttributes,
    dispatcher: trueos_ttstt_cpu::Dispatcher,
    workspace: &mut CpuWorkspace<'_>,
) -> Result<(), DispatchError> {
    let has_bias = attributes.bias_mode == BiasMode::Float;
    require_arity(access, if has_bias { 5 } else { 4 }, 1)?;
    let activation = access.input::<f32>(0)?;
    let weights = access.input::<i8>(1)?;
    let weight_scales = access.input::<f32>(2)?;
    let weight_zero_points = access.input::<i8>(3)?;
    let bias = if has_bias {
        Some(access.input::<f32>(4)?)
    } else {
        None
    };
    let k = usize::try_from(attributes.k).map_err(|_| DispatchError::ShapeConversion)?;
    let n = usize::try_from(attributes.n).map_err(|_| DispatchError::ShapeConversion)?;
    let packed_elements = k.checked_mul(n).ok_or(DispatchError::ShapeConversion)?;
    if k == 0
        || n == 0
        || !activation.len().is_multiple_of(k)
        || weights.len() != packed_elements
        || weight_scales.len() != n
        || weight_zero_points.len() != n
        || bias.as_ref().is_some_and(|values| values.len() != n)
    {
        return Err(DispatchError::ShapeConversion);
    }
    let rows = activation.len() / k;
    if rows == 0 {
        return Err(DispatchError::ShapeConversion);
    }
    let output_elements = rows.checked_mul(n).ok_or(DispatchError::ShapeConversion)?;
    let quantization = trueos_ttstt_cpu::dynamic_quantization_parameters(&activation)?;
    for &scale in weight_scales.iter() {
        trueos_ttstt_cpu::conv_integer_scale(quantization.scale, scale)?;
    }
    if bias
        .as_ref()
        .is_some_and(|values| values.iter().any(|value| !value.is_finite()))
    {
        return Err(DispatchError::InvalidControlTensor);
    }

    let packed = workspace
        .packed_i8
        .get_mut(..packed_elements)
        .ok_or(WorkspaceError::PackedI8TooSmall)?;
    let row_sums = workspace
        .row_sums_i32
        .get_mut(..n)
        .ok_or(WorkspaceError::RowSumsI32TooSmall)?;
    let quantized_row = workspace
        .quant_u8
        .get_mut(..k)
        .ok_or(WorkspaceError::QuantU8TooSmall)?;
    let accumulators = workspace
        .accum_i32
        .get_mut(..n)
        .ok_or(WorkspaceError::AccumI32TooSmall)?;
    trueos_ttstt_cpu::pack_matmul_weights_i8(&weights, k, n, packed, row_sums)?;

    let mut output = access.output::<f32>(0)?;
    if output.len() != output_elements {
        return Err(DispatchError::ShapeConversion);
    }
    for row in 0..rows {
        let input_start = row * k;
        trueos_ttstt_cpu::quantize_linear_u8_with_parameters(
            &activation[input_start..input_start + k],
            quantization,
            quantized_row,
        )?;
        dispatcher.qgemm(
            quantized_row,
            packed,
            accumulators,
            trueos_ttstt_cpu::QGemmParams {
                m: 1,
                n,
                k,
                lhs_zero_point: quantization.zero_point,
                rhs_zero_points: trueos_ttstt_cpu::RhsZeroPoints::PerOutput(&weight_zero_points),
                rhs_row_sums: Some(row_sums),
            },
        )?;
        let output_start = row * n;
        trueos_ttstt_cpu::dequantize_matmul_integer_per_output(
            accumulators,
            1,
            n,
            quantization.scale,
            &weight_scales,
            &mut output[output_start..output_start + n],
        )?;
        if let Some(values) = bias.as_ref() {
            for (destination, &value) in output[output_start..output_start + n]
                .iter_mut()
                .zip(values.iter())
            {
                *destination += value;
            }
        }
    }
    Ok(())
}

fn execute_quant_conv<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: QuantConvAttributes,
    dispatcher: trueos_ttstt_cpu::Dispatcher,
    workspace: &mut CpuWorkspace<'_>,
) -> Result<(), DispatchError> {
    let has_bias = attributes.bias_mode == BiasMode::QuantizedInt32;
    require_arity(access, if has_bias { 5 } else { 4 }, 1)?;
    if attributes.groups != 1 {
        return Err(DispatchError::UnsupportedAttributeProfile {
            opcode: OpCode::DynamicQuantizedConv1d,
        });
    }
    let input = access.input::<f32>(0)?;
    let weights = access.input::<u8>(1)?;
    let weight_scale = access.input::<f32>(2)?;
    let weight_zero = access.input::<u8>(3)?;
    let bias = if has_bias {
        Some(access.input::<f32>(4)?)
    } else {
        None
    };
    let input_shape = input.shape();
    let input_dims = input_shape.dims();
    if input_dims.len() != 3 || weight_scale.len() != 1 || weight_zero.len() != 1 {
        return Err(DispatchError::ShapeConversion);
    }
    if u32::from(weight_zero[0]) != attributes.weight_zero {
        return Err(DispatchError::InvalidControlTensor);
    }
    let batch = input_dims[0] as usize;
    let input_channels = attributes.input_channels as usize;
    let input_width = input_dims[2] as usize;
    let output_channels = attributes.output_channels as usize;
    let kernel_width = attributes.kernel as usize;
    let reduction = input_channels
        .checked_mul(kernel_width)
        .ok_or(DispatchError::ShapeConversion)?;
    let packed_elements = output_channels
        .checked_mul(reduction)
        .ok_or(DispatchError::ShapeConversion)?;
    if input_dims[1] as usize != input_channels
        || weights.len() != packed_elements
        || bias
            .as_ref()
            .is_some_and(|values| values.len() != output_channels)
    {
        return Err(DispatchError::ShapeConversion);
    }
    let packed = workspace
        .packed_i8
        .get_mut(..packed_elements)
        .ok_or(WorkspaceError::PackedI8TooSmall)?;
    let row_sums = workspace
        .row_sums_i32
        .get_mut(..output_channels)
        .ok_or(WorkspaceError::RowSumsI32TooSmall)?;
    let blocked_reduction = reduction
        .checked_mul(4)
        .ok_or(DispatchError::ShapeConversion)?;
    let patch_elements = if workspace.quant_u8.len() >= blocked_reduction {
        blocked_reduction
    } else {
        reduction
    };
    let patch_scratch = workspace
        .quant_u8
        .get_mut(..patch_elements)
        .ok_or(WorkspaceError::QuantU8TooSmall)?;
    let bias_scratch = if has_bias {
        workspace
            .bias_i32
            .get_mut(..output_channels)
            .ok_or(WorkspaceError::BiasI32TooSmall)?
    } else {
        &mut workspace.bias_i32[..0]
    };
    trueos_ttstt_cpu::pack_conv1d_weights_u8(
        &weights,
        output_channels,
        input_channels,
        kernel_width,
        packed,
        row_sums,
    )?;
    let params = trueos_ttstt_cpu::QConv1dParams {
        batch,
        input_channels,
        input_width,
        output_channels,
        kernel_width,
        stride: attributes.stride as usize,
        dilation: attributes.dilation as usize,
        pad_left: attributes.pad_left as usize,
        pad_right: attributes.pad_right as usize,
        input_zero_point: 0,
        weight_zero_points: trueos_ttstt_cpu::RhsZeroPoints::Scalar(
            trueos_ttstt_cpu::signed_u8_zero_point(weight_zero[0]),
        ),
        weight_row_sums: Some(row_sums),
    };
    let output_width = params.output_width()?;
    let expected_output = batch
        .checked_mul(output_channels)
        .and_then(|elements| elements.checked_mul(output_width))
        .ok_or(DispatchError::ShapeConversion)?;
    let mut output = access.output::<f32>(0)?;
    if output.len() != expected_output {
        return Err(DispatchError::ShapeConversion);
    }
    dispatcher.qconv1d_dequantized(
        &input,
        packed,
        &mut output,
        patch_scratch,
        bias_scratch,
        params,
        weight_scale[0],
        bias.as_deref(),
    )?;
    Ok(())
}

fn execute_bilstm<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: BiLstmAttributes,
    workspace: &mut CpuWorkspace<'_>,
) -> Result<(), DispatchError> {
    require_arity(access, 6, 3)?;
    let input_width = match (attributes.profile, attributes.input_width) {
        (1, 512) => trueos_kokoro_lstm::InputWidth::Text512,
        (2..=6, 640) => trueos_kokoro_lstm::InputWidth::Prosody640,
        _ => {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: OpCode::BiLstm256,
            });
        }
    };
    let input = access.input::<f32>(0)?;
    let weights = access.input::<f32>(1)?;
    let recurrent = access.input::<f32>(2)?;
    let bias = access.input::<f32>(3)?;
    let initial_hidden = access.input::<f32>(4)?;
    let initial_cell = access.input::<f32>(5)?;
    let input_shape = input.shape();
    let input_dims = input_shape.dims();
    if input_dims.len() != 3 || input_dims[1] != 1 || input_dims[2] != attributes.input_width {
        return Err(DispatchError::ShapeConversion);
    }
    let sequence_length = input_dims[0] as usize;
    let problem = trueos_kokoro_lstm::Problem::new(
        sequence_length,
        input_width,
        &input,
        &weights,
        &recurrent,
        &bias,
    )?;
    let gates = workspace
        .lstm_gates_f32
        .get_mut(..trueos_kokoro_lstm::GATE_SCRATCH_ELEMENTS)
        .ok_or(WorkspaceError::LstmGatesF32TooSmall)?;
    let mut output = access.output::<f32>(0)?;
    let mut hidden = access.output::<f32>(1)?;
    let mut cell = access.output::<f32>(2)?;
    let buffers = trueos_kokoro_lstm::Buffers::new(&mut output, &mut hidden, &mut cell, gates);
    let mut invocation = trueos_kokoro_lstm::CooperativeLstm::start_with_state(
        problem,
        buffers,
        &initial_hidden,
        &initial_cell,
    )?;
    let mut dense = trueos_kokoro_lstm::DispatchedDense::detect();
    while !invocation.is_complete() {
        invocation.advance(&mut dense)?;
    }
    Ok(())
}

fn execute_matmul<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: MatMulAttributes,
    dispatcher: trueos_kokoro_gemm::Dispatcher,
) -> Result<(), DispatchError> {
    require_arity(access, 2, 1)?;
    let lhs = access.input::<f32>(0)?;
    let rhs = access.input::<f32>(1)?;
    let (profile, expected_output) = matmul_profile(attributes, lhs.shape(), rhs.shape())?;
    let mut output = access.output::<f32>(0)?;
    if output.shape() != expected_output {
        return Err(DispatchError::ShapeConversion);
    }
    dispatcher.matmul(profile, &lhs, &rhs, &mut output)?;
    Ok(())
}

fn execute_stft<const BINDINGS: usize>(access: &OpAccess<BINDINGS>) -> Result<(), DispatchError> {
    require_arity(access, 4, 1)?;
    let input = access.input::<f32>(0)?;
    let frame_step = access.input::<i64>(1)?;
    let window = access.input::<f32>(2)?;
    let frame_length = access.input::<i64>(3)?;
    if frame_step.len() != 1
        || frame_step[0] != trueos_kokoro_stft::FRAME_STEP as i64
        || frame_length.len() != 1
        || frame_length[0] != trueos_kokoro_stft::FRAME_LENGTH as i64
        || window.len() != trueos_kokoro_stft::FRAME_LENGTH
        || window
            .iter()
            .zip(trueos_kokoro_stft::HANN_WINDOW_BITS)
            .any(|(value, expected)| value.to_bits() != expected)
    {
        return Err(DispatchError::InvalidControlTensor);
    }
    let dims = input.shape();
    if dims.rank() != 2 {
        return Err(DispatchError::ShapeConversion);
    }
    let problem =
        trueos_kokoro_stft::Problem::new(dims.dims()[0] as usize, dims.dims()[1] as usize, &input)?;
    let mut output = access.output::<f32>(0)?;
    let mut state = trueos_kokoro_stft::CooperativeStft::start(problem, &mut output)?;
    let budget = state.total_frames().max(1);
    while !state.is_complete() {
        state.advance(budget)?;
    }
    Ok(())
}

fn execute_binary<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    work: WorkSlice,
    attributes: BinaryAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 2, 1)?;
    if attributes.dtype == AttributeDType::Int64 {
        if attributes.opcode != OpCode::Add {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: attributes.opcode,
            });
        }
        let lhs = access.input::<i64>(0)?;
        let rhs = access.input::<i64>(1)?;
        let lhs_shape = layout_shape(lhs.shape())?;
        let rhs_shape = layout_shape(rhs.shape())?;
        let mut output = access.output::<i64>(0)?;
        trueos_kokoro_scalar::add_i64(&lhs, lhs_shape, &rhs, rhs_shape, &mut output)?;
        return Ok(());
    }
    let lhs = access.input::<f32>(0)?;
    let rhs = access.input::<f32>(1)?;
    let lhs_layout = contiguous_f32(lhs.shape())?;
    let rhs_layout = contiguous_f32(rhs.shape())?;
    let mut output = access.output::<f32>(0)?;
    let output_layout = contiguous_f32(output.shape())?;
    match attributes.opcode {
        OpCode::Add => {
            trueos_kokoro_f32::add(&lhs, lhs_layout, &rhs, rhs_layout, &mut output, output_layout)?
        }
        OpCode::Mul => {
            trueos_kokoro_f32::mul(&lhs, lhs_layout, &rhs, rhs_layout, &mut output, output_layout)?
        }
        OpCode::Div => {
            if work.op_index() == KOKORO_STFT_PHASE_DIV_OP_INDEX {
                trueos_kokoro_f32::div_ieee(
                    &lhs,
                    lhs_layout,
                    &rhs,
                    rhs_layout,
                    &mut output,
                    output_layout,
                )?
            } else {
                trueos_kokoro_f32::div(
                    &lhs,
                    lhs_layout,
                    &rhs,
                    rhs_layout,
                    &mut output,
                    output_layout,
                )?
            }
        }
        OpCode::Sub => {
            trueos_kokoro_f32::sub(&lhs, lhs_layout, &rhs, rhs_layout, &mut output, output_layout)?
        }
        opcode => return Err(DispatchError::UnsupportedOpcode { opcode }),
    }
    Ok(())
}

fn execute_comparison<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: ComparisonAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 2, 1)?;
    match (attributes.opcode, attributes.input_dtype) {
        (OpCode::And, AttributeDType::Bool) => {
            let lhs = access.input::<bool>(0)?;
            let rhs = access.input::<bool>(1)?;
            let lhs_shape = layout_shape(lhs.shape())?;
            let rhs_shape = layout_shape(rhs.shape())?;
            let mut output = access.output::<bool>(0)?;
            trueos_kokoro_scalar::and_bool(&lhs, lhs_shape, &rhs, rhs_shape, &mut output)?;
        }
        (OpCode::Equal, AttributeDType::Int64) => {
            let lhs = access.input::<i64>(0)?;
            let rhs = access.input::<i64>(1)?;
            let lhs_shape = layout_shape(lhs.shape())?;
            let rhs_shape = layout_shape(rhs.shape())?;
            let mut output = access.output::<bool>(0)?;
            trueos_kokoro_scalar::equal_i64(&lhs, lhs_shape, &rhs, rhs_shape, &mut output)?;
        }
        (OpCode::Greater, AttributeDType::Float) => {
            let lhs = access.input::<f32>(0)?;
            let rhs = access.input::<f32>(1)?;
            let lhs_shape = layout_shape(lhs.shape())?;
            let rhs_shape = layout_shape(rhs.shape())?;
            let mut output = access.output::<bool>(0)?;
            trueos_kokoro_scalar::greater_f32(&lhs, lhs_shape, &rhs, rhs_shape, &mut output)?;
        }
        (OpCode::GreaterOrEqual, AttributeDType::Float) => {
            let lhs = access.input::<f32>(0)?;
            let rhs = access.input::<f32>(1)?;
            let lhs_shape = layout_shape(lhs.shape())?;
            let rhs_shape = layout_shape(rhs.shape())?;
            let mut output = access.output::<bool>(0)?;
            trueos_kokoro_scalar::greater_or_equal_f32(
                &lhs,
                lhs_shape,
                &rhs,
                rhs_shape,
                &mut output,
            )?;
        }
        (OpCode::Less, AttributeDType::Float) => {
            let lhs = access.input::<f32>(0)?;
            let rhs = access.input::<f32>(1)?;
            let lhs_shape = layout_shape(lhs.shape())?;
            let rhs_shape = layout_shape(rhs.shape())?;
            let mut output = access.output::<bool>(0)?;
            trueos_kokoro_scalar::less_f32(&lhs, lhs_shape, &rhs, rhs_shape, &mut output)?;
        }
        (OpCode::Less, AttributeDType::Int64) => {
            let lhs = access.input::<i64>(0)?;
            let rhs = access.input::<i64>(1)?;
            let lhs_shape = layout_shape(lhs.shape())?;
            let rhs_shape = layout_shape(rhs.shape())?;
            let mut output = access.output::<bool>(0)?;
            trueos_kokoro_scalar::less_i64(&lhs, lhs_shape, &rhs, rhs_shape, &mut output)?;
        }
        (opcode, _) => return Err(DispatchError::UnsupportedAttributeProfile { opcode }),
    }
    Ok(())
}

fn execute_cast<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    attributes: CastAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 1, 1)?;
    match (attributes.input_dtype, attributes.output_dtype) {
        (AttributeDType::Float, AttributeDType::Bool) => {
            let input = access.input::<f32>(0)?;
            let mut output = access.output::<bool>(0)?;
            trueos_kokoro_scalar::cast_f32_to_bool(&input, &mut output)?;
        }
        (AttributeDType::Int64, AttributeDType::Float) => {
            let input = access.input::<i64>(0)?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_scalar::cast_i64_to_f32(&input, &mut output)?;
        }
        (AttributeDType::Bool, AttributeDType::Float) => {
            let input = access.input::<bool>(0)?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_scalar::cast_bool_to_f32(&input, &mut output)?;
        }
        _ => {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: OpCode::Cast,
            });
        }
    }
    Ok(())
}

fn execute_where<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    dtype: AttributeDType,
) -> Result<(), DispatchError> {
    require_arity(access, 3, 1)?;
    let condition = access.input::<bool>(0)?;
    let condition_shape = layout_shape(condition.shape())?;
    match dtype {
        AttributeDType::Float => {
            let when_true = access.input::<f32>(1)?;
            let when_false = access.input::<f32>(2)?;
            let true_shape = layout_shape(when_true.shape())?;
            let false_shape = layout_shape(when_false.shape())?;
            let mut output = access.output::<f32>(0)?;
            trueos_kokoro_scalar::where_f32(
                &condition,
                condition_shape,
                &when_true,
                true_shape,
                &when_false,
                false_shape,
                &mut output,
            )?;
        }
        AttributeDType::Int64 => {
            let when_true = access.input::<i64>(1)?;
            let when_false = access.input::<i64>(2)?;
            let true_shape = layout_shape(when_true.shape())?;
            let false_shape = layout_shape(when_false.shape())?;
            let mut output = access.output::<i64>(0)?;
            trueos_kokoro_scalar::where_i64(
                &condition,
                condition_shape,
                &when_true,
                true_shape,
                &when_false,
                false_shape,
                &mut output,
            )?;
        }
        _ => {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: OpCode::Where,
            });
        }
    }
    Ok(())
}

fn execute_unary<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    work: WorkSlice,
    attributes: UnaryAttributes,
) -> Result<(), DispatchError> {
    require_arity(access, 1, 1)?;
    let input = access.input::<f32>(0)?;
    let input_layout = contiguous_f32(input.shape())?;
    let mut output = access.output::<f32>(0)?;
    let output_layout = contiguous_f32(output.shape())?;
    match attributes.opcode {
        OpCode::Atan if work.op_index() == KOKORO_STFT_PHASE_ATAN_OP_INDEX => {
            trueos_kokoro_f32::atan_ieee(&input, input_layout, &mut output, output_layout)?
        }
        OpCode::Atan => trueos_kokoro_f32::atan(&input, input_layout, &mut output, output_layout)?,
        OpCode::Cos => trueos_kokoro_f32::cos(&input, input_layout, &mut output, output_layout)?,
        OpCode::Exp => trueos_kokoro_f32::exp(&input, input_layout, &mut output, output_layout)?,
        OpCode::Floor => {
            trueos_kokoro_f32::floor(&input, input_layout, &mut output, output_layout)?
        }
        OpCode::Round => {
            trueos_kokoro_f32::round(&input, input_layout, &mut output, output_layout)?
        }
        OpCode::Sigmoid => {
            trueos_kokoro_f32::sigmoid(&input, input_layout, &mut output, output_layout)?
        }
        OpCode::Sin => trueos_kokoro_f32::sin(&input, input_layout, &mut output, output_layout)?,
        OpCode::Sqrt => trueos_kokoro_f32::sqrt(&input, input_layout, &mut output, output_layout)?,
        OpCode::Tanh => trueos_kokoro_f32::tanh(&input, input_layout, &mut output, output_layout)?,
        opcode => return Err(DispatchError::UnsupportedOpcode { opcode }),
    }
    Ok(())
}

fn execute_resize<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    work: WorkSlice,
    attributes: ResizeAttributes,
) -> Result<DispatchResult, DispatchError> {
    require_arity(access, 2, 1)?;
    let input = access.input::<f32>(0)?;
    let scales = access.input::<f32>(1)?;
    let input_shape = input.shape();
    let dims = input_shape.dims();
    if dims.len() != 3 || scales.len() != 3 {
        return Err(DispatchError::InvalidControlTensor);
    }
    let (mode, scale, expected_scale) = match attributes.profile {
        1 => (
            trueos_kokoro_resize::ResizeMode::NearestAsymmetric,
            trueos_kokoro_resize::ResizeScale::Up2,
            2.0_f32,
        ),
        2 => (
            trueos_kokoro_resize::ResizeMode::NearestAsymmetric,
            trueos_kokoro_resize::ResizeScale::Up300,
            300.0_f32,
        ),
        3 => (
            trueos_kokoro_resize::ResizeMode::LinearHalfPixel,
            trueos_kokoro_resize::ResizeScale::Down300,
            1.0_f32 / 300.0_f32,
        ),
        4 => (
            trueos_kokoro_resize::ResizeMode::LinearHalfPixel,
            trueos_kokoro_resize::ResizeScale::Up300,
            300.0_f32,
        ),
        _ => {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: OpCode::Resize,
            });
        }
    };
    let expected_mode = match attributes.mode {
        ResizeMode::Nearest => trueos_kokoro_resize::ResizeMode::NearestAsymmetric,
        ResizeMode::Linear => trueos_kokoro_resize::ResizeMode::LinearHalfPixel,
    };
    if mode != expected_mode
        || scales[0].to_bits() != 1.0_f32.to_bits()
        || scales[1].to_bits() != 1.0_f32.to_bits()
        || scales[2].to_bits() != expected_scale.to_bits()
    {
        return Err(DispatchError::InvalidControlTensor);
    }
    let plan = trueos_kokoro_resize::ResizePlan::new(
        usize::try_from(dims[0]).map_err(|_| DispatchError::ShapeConversion)?,
        usize::try_from(dims[1]).map_err(|_| DispatchError::ShapeConversion)?,
        usize::try_from(dims[2]).map_err(|_| DispatchError::ShapeConversion)?,
        mode,
        scale,
    )?;
    let mut output = access.output::<f32>(0)?;
    let completion = cooperative_completion(work, output.len())?;
    let start = (work.unit_start() as usize).min(output.len());
    let end = (work.unit_end() as usize).min(output.len());
    if start < end {
        plan.run_range(&input, &mut output, start, end - start)?;
    }
    Ok(completion)
}

fn execute_float_conv<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    work: WorkSlice,
    attributes: FloatConvAttributes,
    dispatcher: trueos_kokoro_conv::Dispatcher,
) -> Result<DispatchResult, DispatchError> {
    require_arity(access, if attributes.has_bias { 3 } else { 2 }, 1)?;
    let input = access.input::<f32>(0)?;
    let weights = access.input::<f32>(1)?;
    let bias = if attributes.has_bias {
        Some(access.input::<f32>(2)?)
    } else {
        None
    };
    let input_shape = input.shape();
    let input_dims = input_shape.dims();
    if input_dims.len() != 3 || input_dims[0] != 1 {
        return Err(DispatchError::ShapeConversion);
    }
    let input_width = input_dims[2] as usize;
    let profile = match attributes.profile {
        1 if attributes.opcode == OpCode::FloatConv1d => {
            trueos_kokoro_conv::Profile::PostConv128To22 { input_width }
        }
        2 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            trueos_kokoro_conv::Profile::EncoderDepthwise512 { input_width }
        }
        3 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            trueos_kokoro_conv::Profile::DecoderDepthwise1090 { input_width }
        }
        4 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            trueos_kokoro_conv::Profile::Upsample512To256 { input_width }
        }
        5 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            trueos_kokoro_conv::Profile::Upsample256To128 { input_width }
        }
        6 if attributes.opcode == OpCode::FloatConvTranspose1d => {
            trueos_kokoro_conv::Profile::Istft22To1 { input_width }
        }
        _ => {
            return Err(DispatchError::UnsupportedAttributeProfile {
                opcode: attributes.opcode,
            });
        }
    };
    let parameters = profile.parameters();
    if parameters.input_channels != attributes.input_channels as usize
        || parameters.output_channels != attributes.output_channels as usize
        || parameters.kernel_width != attributes.kernel as usize
        || parameters.stride != attributes.stride as usize
        || attributes.dilation != 1
        || parameters.pad_left != attributes.pad_left as usize
        || parameters.pad_right != attributes.pad_right as usize
        || parameters.output_padding != attributes.output_padding as usize
        || parameters.groups != attributes.groups as usize
        || parameters.has_bias != attributes.has_bias
    {
        return Err(DispatchError::UnsupportedAttributeProfile {
            opcode: attributes.opcode,
        });
    }
    let problem = trueos_kokoro_conv::Problem::new(profile, &input, &weights, bias.as_deref())?;
    let dimensions = problem.dimensions();
    let mut output = access.output::<f32>(0)?;
    if output.len() != dimensions.output_elements()? {
        return Err(DispatchError::InvalidWorkContract {
            opcode: attributes.opcode,
        });
    }
    let completion = cooperative_completion(work, output.len())?;
    let mut linear = (work.unit_start() as usize).min(output.len());
    let end = (work.unit_end() as usize).min(output.len());
    while linear < end {
        let channel = linear / dimensions.output_width;
        let time = linear % dimensions.output_width;
        let count = (end - linear).min(dimensions.output_width - time);
        dispatcher.convolve_tile_with_lane(
            problem,
            &mut output,
            trueos_kokoro_conv::Tile {
                channel_start: channel,
                channel_count: 1,
                time_start: time,
                time_count: count,
            },
            dispatcher.best_lane(),
        )?;
        linear += count;
    }
    Ok(completion)
}

fn cooperative_completion(
    work: WorkSlice,
    runtime_work_units: usize,
) -> Result<DispatchResult, DispatchError> {
    let runtime_work_units =
        u32::try_from(runtime_work_units).map_err(|_| DispatchError::InvalidWorkContract {
            opcode: work.op().opcode,
        })?;
    if runtime_work_units > work.op().work_units {
        return Err(DispatchError::InvalidWorkContract {
            opcode: work.op().opcode,
        });
    }
    if work.unit_end() >= runtime_work_units {
        Ok(DispatchResult::CompletedOperation { runtime_work_units })
    } else {
        Ok(DispatchResult::Completed)
    }
}

fn execute_resolve<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    program: &Program<'_>,
) -> Result<DispatchResult, DispatchError> {
    require_arity(access, 2, 2)?;
    let logits = access.input::<f32>(0)?;
    let speed = access.input::<f32>(1)?;
    let logits_shape = logits.shape();
    let dims = logits_shape.dims();
    if dims.len() != 3
        || dims[0] != 1
        || dims[2] != trueos_kokoro_duration::KOKORO_DURATION_BINS as u32
        || speed.len() != 1
    {
        return Err(DispatchError::InvalidControlTensor);
    }
    let token_count = dims[1] as usize;
    let frame_limit = program
        .phase(Phase::Phase1)
        .ok_or(DispatchError::InvalidControlTensor)?
        .frame_count_max;
    let mut cumulative = access.output::<i64>(0)?;
    let mut frame_scalar = access.output::<i64>(1)?;
    if frame_scalar.len() != 1 {
        return Err(DispatchError::InvalidControlTensor);
    }
    let frame_count = trueos_kokoro_duration::resolve_decoder_shape(
        &logits,
        token_count,
        speed[0],
        frame_limit,
        &mut cumulative,
    )?;
    frame_scalar[0] = i64::from(frame_count);
    Ok(DispatchResult::FrameCount(frame_count))
}

fn require_whole_unit(work: WorkSlice) -> Result<(), DispatchError> {
    if work.op().work_units == 1 && work.unit_start() == 0 && work.unit_count() == 1 {
        Ok(())
    } else {
        Err(DispatchError::InvalidWorkContract {
            opcode: work.op().opcode,
        })
    }
}

fn require_arity<const BINDINGS: usize>(
    access: &OpAccess<BINDINGS>,
    inputs: u16,
    outputs: u16,
) -> Result<(), DispatchError> {
    if access.input_count() == inputs && access.output_count() == outputs {
        Ok(())
    } else {
        Err(DispatchError::InvalidArity)
    }
}

fn shape_dims(shape: RuntimeShape) -> Result<([usize; 4], usize), DispatchError> {
    let mut dims = [0_usize; 4];
    for (index, &dimension) in shape.dims().iter().enumerate() {
        dims[index] = usize::try_from(dimension).map_err(|_| DispatchError::ShapeConversion)?;
    }
    Ok((dims, shape.dims().len()))
}

fn f32_shape(shape: RuntimeShape) -> Result<trueos_kokoro_f32::Shape, DispatchError> {
    let (dims, rank) = shape_dims(shape)?;
    trueos_kokoro_f32::Shape::new(&dims[..rank]).map_err(DispatchError::F32)
}

fn contiguous_f32(shape: RuntimeShape) -> Result<trueos_kokoro_f32::TensorLayout, DispatchError> {
    Ok(trueos_kokoro_f32::TensorLayout::contiguous(f32_shape(shape)?))
}

fn layout_shape(shape: RuntimeShape) -> Result<trueos_kokoro_layout::Shape, DispatchError> {
    let (dims, rank) = shape_dims(shape)?;
    trueos_kokoro_layout::Shape::new(&dims[..rank]).map_err(DispatchError::Layout)
}
