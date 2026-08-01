#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! Allocation-free f32 kernels for the sealed Kokoro graph.
//!
//! The pinned graph uses rank-four-or-smaller tensors for 1,444 f32 binary
//! elementwise nodes, 130 `ReduceMean`, 19 `LayerNormalization`, 12 `Softmax`,
//! 12 `FastGelu`, 12 `SkipLayerNormalization`, 256 unary math nodes, and 50
//! scalar-exponent `Pow(x, 2.0)` nodes. This crate covers 1,935 pinned nodes in
//! total, keeps ONNX operation boundaries
//! explicit, and requires caller-owned storage.

use core::mem::size_of;

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
mod avx2;

pub const MAX_RANK: usize = 4;
pub const PINNED_BINARY_NODES: usize = 1_444;
pub const PINNED_REDUCE_MEAN_NODES: usize = 130;
pub const PINNED_LAYER_NORMALIZATION_NODES: usize = 19;
pub const PINNED_SOFTMAX_NODES: usize = 12;
pub const PINNED_FAST_GELU_NODES: usize = 12;
pub const PINNED_SKIP_LAYER_NORMALIZATION_NODES: usize = 12;
pub const PINNED_UNARY_MATH_NODES: usize = 256;
pub const PINNED_POW_SQUARE_NODES: usize = 50;
pub const PINNED_NODE_COVERAGE: usize = PINNED_BINARY_NODES
    + PINNED_REDUCE_MEAN_NODES
    + PINNED_LAYER_NORMALIZATION_NODES
    + PINNED_SOFTMAX_NODES
    + PINNED_FAST_GELU_NODES
    + PINNED_SKIP_LAYER_NORMALIZATION_NODES
    + PINNED_UNARY_MATH_NODES
    + PINNED_POW_SQUARE_NODES;

/// Validation failures are reported before an output buffer is modified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    RankTooLarge,
    ZeroDimension,
    ShapeOverflow,
    BufferTooSmall,
    BroadcastMismatch,
    OutputShapeMismatch,
    OverlappingOutput,
    Aliasing,
    InvalidAxis,
    InvalidEpsilon,
    InvalidParameter,
    DomainError,
    ParameterTooSmall,
    NonFiniteInput,
    NonFiniteOutput,
    UnsupportedLane,
}

/// Runtime-selected implementation for the dominant contiguous f32 kernels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElementwiseLane {
    Scalar,
    Avx2,
}

impl ElementwiseLane {
    pub fn is_available(self) -> bool {
        match self {
            Self::Scalar => true,
            Self::Avx2 => {
                #[cfg(target_arch = "x86_64")]
                {
                    avx2::is_available()
                }
                #[cfg(not(target_arch = "x86_64"))]
                {
                    false
                }
            }
        }
    }
}

/// Detect AVX2 only when CPUID, OSXSAVE, and XCR0 all admit YMM state.
pub fn elementwise_lane() -> ElementwiseLane {
    if ElementwiseLane::Avx2.is_available() {
        ElementwiseLane::Avx2
    } else {
        ElementwiseLane::Scalar
    }
}

/// A checked scalar or rank-one-through-rank-four tensor shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shape {
    rank: u8,
    dims: [usize; MAX_RANK],
    elements: usize,
}

impl Shape {
    pub const fn scalar() -> Self {
        Self {
            rank: 0,
            dims: [0; MAX_RANK],
            elements: 1,
        }
    }

    pub fn new(dims: &[usize]) -> Result<Self, Error> {
        if dims.len() > MAX_RANK {
            return Err(Error::RankTooLarge);
        }
        let mut stored = [0usize; MAX_RANK];
        let mut elements = 1usize;
        for (axis, &dimension) in dims.iter().enumerate() {
            if dimension == 0 {
                return Err(Error::ZeroDimension);
            }
            elements = elements
                .checked_mul(dimension)
                .ok_or(Error::ShapeOverflow)?;
            stored[axis] = dimension;
        }
        Ok(Self {
            rank: dims.len() as u8,
            dims: stored,
            elements,
        })
    }

    pub const fn rank(self) -> usize {
        self.rank as usize
    }

    pub const fn element_count(self) -> usize {
        self.elements
    }

    pub fn dims(&self) -> &[usize] {
        &self.dims[..self.rank()]
    }

    pub fn dimension(self, axis: usize) -> Option<usize> {
        if axis < self.rank() {
            Some(self.dims[axis])
        } else {
            None
        }
    }

    fn normalized_axis(self, axis: isize) -> Result<usize, Error> {
        let rank = self.rank();
        if rank == 0 {
            return Err(Error::InvalidAxis);
        }
        let rank_signed = rank as isize;
        let normalized = if axis < 0 { axis + rank_signed } else { axis };
        if !(0..rank_signed).contains(&normalized) {
            Err(Error::InvalidAxis)
        } else {
            Ok(normalized as usize)
        }
    }

    fn product(self, axes: core::ops::Range<usize>) -> usize {
        let mut product = 1usize;
        for axis in axes {
            product *= self.dims[axis];
        }
        product
    }
}

/// Shape, element strides, and starting element offset for a tensor view.
///
/// Input layouts may repeat elements (including stride zero). Output layouts
/// must be non-overlapping; kernels validate that property conservatively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TensorLayout {
    shape: Shape,
    strides: [usize; MAX_RANK],
    offset: usize,
}

impl TensorLayout {
    pub fn contiguous(shape: Shape) -> Self {
        let mut strides = [0usize; MAX_RANK];
        let mut stride = 1usize;
        for axis in (0..shape.rank()).rev() {
            strides[axis] = stride;
            stride *= shape.dims[axis];
        }
        Self {
            shape,
            strides,
            offset: 0,
        }
    }

    pub fn strided(shape: Shape, strides: &[usize], offset: usize) -> Result<Self, Error> {
        if strides.len() != shape.rank() {
            return Err(Error::OutputShapeMismatch);
        }
        let mut stored = [0usize; MAX_RANK];
        stored[..strides.len()].copy_from_slice(strides);
        let layout = Self {
            shape,
            strides: stored,
            offset,
        };
        layout.required_len()?;
        Ok(layout)
    }

    pub const fn shape(self) -> Shape {
        self.shape
    }

    pub const fn offset(self) -> usize {
        self.offset
    }

    pub fn strides(&self) -> &[usize] {
        &self.strides[..self.shape.rank()]
    }

    pub fn required_len(self) -> Result<usize, Error> {
        let mut maximum = self.offset;
        for axis in 0..self.shape.rank() {
            let contribution = (self.shape.dims[axis] - 1)
                .checked_mul(self.strides[axis])
                .ok_or(Error::ShapeOverflow)?;
            maximum = maximum
                .checked_add(contribution)
                .ok_or(Error::ShapeOverflow)?;
        }
        maximum.checked_add(1).ok_or(Error::ShapeOverflow)
    }

    fn is_non_overlapping(self) -> Result<bool, Error> {
        let mut axes = [0usize, 1, 2, 3];
        for index in 1..self.shape.rank() {
            let axis = axes[index];
            let mut position = index;
            while position > 0 && self.strides[axis] < self.strides[axes[position - 1]] {
                axes[position] = axes[position - 1];
                position -= 1;
            }
            axes[position] = axis;
        }

        let mut covered_span = 1usize;
        for &axis in &axes[..self.shape.rank()] {
            let dimension = self.shape.dims[axis];
            if dimension <= 1 {
                continue;
            }
            let stride = self.strides[axis];
            if stride < covered_span {
                return Ok(false);
            }
            covered_span = covered_span
                .checked_add(
                    (dimension - 1)
                        .checked_mul(stride)
                        .ok_or(Error::ShapeOverflow)?,
                )
                .ok_or(Error::ShapeOverflow)?;
        }
        Ok(true)
    }

    fn physical_offset(self, coordinates: &[usize; MAX_RANK]) -> usize {
        let mut offset = self.offset;
        for (axis, &coordinate) in coordinates.iter().enumerate().take(self.shape.rank()) {
            offset += coordinate * self.strides[axis];
        }
        offset
    }

    fn is_contiguous(self) -> bool {
        let mut expected_stride = 1usize;
        for axis in (0..self.shape.rank()).rev() {
            let dimension = self.shape.dims[axis];
            if dimension > 1 && self.strides[axis] != expected_stride {
                return false;
            }
            expected_stride *= dimension;
        }
        true
    }
}

/// Compute ONNX multidirectional-broadcast output shape.
pub fn broadcast_shape(lhs: Shape, rhs: Shape) -> Result<Shape, Error> {
    let rank = lhs.rank().max(rhs.rank());
    let mut output = [1usize; MAX_RANK];
    for (output_axis, output_dimension) in output.iter_mut().enumerate().take(rank) {
        let lhs_axis = output_axis as isize - (rank - lhs.rank()) as isize;
        let rhs_axis = output_axis as isize - (rank - rhs.rank()) as isize;
        let lhs_dimension = if lhs_axis < 0 {
            1
        } else {
            lhs.dims[lhs_axis as usize]
        };
        let rhs_dimension = if rhs_axis < 0 {
            1
        } else {
            rhs.dims[rhs_axis as usize]
        };
        if lhs_dimension != rhs_dimension && lhs_dimension != 1 && rhs_dimension != 1 {
            return Err(Error::BroadcastMismatch);
        }
        *output_dimension = lhs_dimension.max(rhs_dimension);
    }
    Shape::new(&output[..rank])
}

#[derive(Clone, Copy)]
enum BinaryOperation {
    Add,
    Mul,
    Div,
    DivIeee,
    Sub,
}

impl BinaryOperation {
    fn apply(self, lhs: f32, rhs: f32) -> f32 {
        match self {
            Self::Add => lhs + rhs,
            Self::Mul => lhs * rhs,
            Self::Div | Self::DivIeee => lhs / rhs,
            Self::Sub => lhs - rhs,
        }
    }

    fn valid_output(self, value: f32) -> bool {
        match self {
            Self::DivIeee => !value.is_nan(),
            Self::Add | Self::Mul | Self::Div | Self::Sub => value.is_finite(),
        }
    }
}

/// ONNX multidirectional-broadcast `Add` over checked rank-four views.
pub fn add(
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    add_on_lane(elementwise_lane(), lhs, lhs_layout, rhs, rhs_layout, output, output_layout)
}

/// `Add` with an explicit lane, primarily for a worker-owned dispatcher.
pub fn add_on_lane(
    lane: ElementwiseLane,
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    binary_elementwise(
        lane,
        BinaryOperation::Add,
        lhs,
        lhs_layout,
        rhs,
        rhs_layout,
        output,
        output_layout,
    )
}

/// ONNX multidirectional-broadcast `Mul` over checked rank-four views.
pub fn mul(
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    mul_on_lane(elementwise_lane(), lhs, lhs_layout, rhs, rhs_layout, output, output_layout)
}

/// `Mul` with an explicit lane, primarily for a worker-owned dispatcher.
pub fn mul_on_lane(
    lane: ElementwiseLane,
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    binary_elementwise(
        lane,
        BinaryOperation::Mul,
        lhs,
        lhs_layout,
        rhs,
        rhs_layout,
        output,
        output_layout,
    )
}

/// ONNX multidirectional-broadcast `Div` over checked rank-four views.
pub fn div(
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    div_on_lane(elementwise_lane(), lhs, lhs_layout, rhs, rhs_layout, output, output_layout)
}

/// `Div` with an explicit lane, primarily for a worker-owned dispatcher.
pub fn div_on_lane(
    lane: ElementwiseLane,
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    binary_elementwise(
        lane,
        BinaryOperation::Div,
        lhs,
        lhs_layout,
        rhs,
        rhs_layout,
        output,
        output_layout,
    )
}

/// ONNX `Div` with the graph-pinned IEEE-754 infinity policy needed by
/// Kokoro's STFT phase reconstruction.
///
/// Finite non-zero values divided by signed zero produce signed infinity;
/// `0.0 / 0.0` remains a terminal numerical error. Callers must use this only
/// for a graph edge whose immediate consumer is an infinity-safe operation.
pub fn div_ieee(
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    div_ieee_on_lane(elementwise_lane(), lhs, lhs_layout, rhs, rhs_layout, output, output_layout)
}

/// Graph-pinned IEEE `Div` with an explicit worker lane.
pub fn div_ieee_on_lane(
    lane: ElementwiseLane,
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    binary_elementwise(
        lane,
        BinaryOperation::DivIeee,
        lhs,
        lhs_layout,
        rhs,
        rhs_layout,
        output,
        output_layout,
    )
}

/// ONNX multidirectional-broadcast `Sub` over checked rank-four views.
pub fn sub(
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    sub_on_lane(elementwise_lane(), lhs, lhs_layout, rhs, rhs_layout, output, output_layout)
}

/// `Sub` with an explicit lane, primarily for a worker-owned dispatcher.
pub fn sub_on_lane(
    lane: ElementwiseLane,
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    binary_elementwise(
        lane,
        BinaryOperation::Sub,
        lhs,
        lhs_layout,
        rhs,
        rhs_layout,
        output,
        output_layout,
    )
}

#[derive(Clone, Copy)]
enum UnaryOperation {
    Square,
    Sqrt,
    Floor,
    Sin,
    LeakyRelu(f32),
    Sigmoid,
    Round,
    Tanh,
    Atan,
    AtanIeee,
    Exp,
    Cos,
}

impl UnaryOperation {
    fn valid_input(self, value: f32) -> bool {
        value.is_finite() || matches!(self, Self::AtanIeee) && value.is_infinite()
    }

    fn apply(self, value: f32) -> Result<f32, Error> {
        let result = match self {
            Self::Square => value * value,
            Self::Sqrt => {
                if value < 0.0 {
                    return Err(Error::DomainError);
                }
                libm::sqrtf(value)
            }
            Self::Floor => libm::floorf(value),
            Self::Sin => libm::sinf(value),
            Self::LeakyRelu(alpha) => {
                if value >= 0.0 {
                    value
                } else {
                    alpha * value
                }
            }
            Self::Sigmoid => sigmoid_value(value),
            Self::Round => round_ties_even(value),
            Self::Tanh => libm::tanhf(value),
            Self::Atan | Self::AtanIeee => libm::atanf(value),
            Self::Exp => libm::expf(value),
            Self::Cos => libm::cosf(value),
        };
        if result.is_finite() {
            Ok(result)
        } else {
            Err(Error::NonFiniteOutput)
        }
    }
}

/// The pinned ONNX `Pow` family: FLOAT input raised to scalar FLOAT `2.0`.
///
/// All 50 source nodes share exponent bits `0x4000_0000`, so this deliberately
/// exposes square rather than a general, slower `powf` operation.
pub fn pow_square(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    pow_square_on_lane(elementwise_lane(), input, input_layout, output, output_layout)
}

/// Pinned square specialization with an explicit worker lane.
pub fn pow_square_on_lane(
    lane: ElementwiseLane,
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise_on_lane(
        lane,
        UnaryOperation::Square,
        input,
        input_layout,
        output,
        output_layout,
    )
}

/// ONNX `Sqrt` over checked rank-four strided views.
pub fn sqrt(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::Sqrt, input, input_layout, output, output_layout)
}

/// ONNX `Floor` over checked rank-four strided views.
pub fn floor(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::Floor, input, input_layout, output, output_layout)
}

/// ONNX `Sin` over checked rank-four strided views.
pub fn sin(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise_on_lane(
        elementwise_lane(),
        UnaryOperation::Sin,
        input,
        input_layout,
        output,
        output_layout,
    )
}

/// ONNX `LeakyRelu` with an explicit finite `alpha` attribute.
pub fn leaky_relu(
    input: &[f32],
    input_layout: TensorLayout,
    alpha: f32,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    if !alpha.is_finite() {
        return Err(Error::InvalidParameter);
    }
    unary_elementwise(UnaryOperation::LeakyRelu(alpha), input, input_layout, output, output_layout)
}

/// ONNX `Sigmoid` over checked rank-four strided views.
pub fn sigmoid(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::Sigmoid, input, input_layout, output, output_layout)
}

/// ONNX `Round`, round-to-nearest with ties to even.
pub fn round(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::Round, input, input_layout, output, output_layout)
}

/// ONNX `Tanh` over checked rank-four strided views.
pub fn tanh(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::Tanh, input, input_layout, output, output_layout)
}

/// ONNX `Atan` over checked rank-four strided views.
pub fn atan(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::Atan, input, input_layout, output, output_layout)
}

/// `Atan` accepting signed infinity for the graph-pinned STFT phase edge.
pub fn atan_ieee(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::AtanIeee, input, input_layout, output, output_layout)
}

/// ONNX `Exp` over checked rank-four strided views.
pub fn exp(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::Exp, input, input_layout, output, output_layout)
}

/// ONNX `Cos` over checked rank-four strided views.
pub fn cos(
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise(UnaryOperation::Cos, input, input_layout, output, output_layout)
}

fn unary_elementwise(
    operation: UnaryOperation,
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    unary_elementwise_on_lane(
        ElementwiseLane::Scalar,
        operation,
        input,
        input_layout,
        output,
        output_layout,
    )
}

fn unary_elementwise_on_lane(
    lane: ElementwiseLane,
    operation: UnaryOperation,
    input: &[f32],
    input_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    validate_input_layout(input, input_layout)?;
    validate_output_layout(output, output_layout)?;
    if input_layout.shape != output_layout.shape {
        return Err(Error::OutputShapeMismatch);
    }
    reject_alias(output, input)?;
    if !lane.is_available() {
        return Err(Error::UnsupportedLane);
    }

    #[cfg(target_arch = "x86_64")]
    if lane == ElementwiseLane::Avx2
        && matches!(operation, UnaryOperation::Square)
        && input_layout.is_contiguous()
        && output_layout.is_contiguous()
    {
        let elements = input_layout.shape.element_count();
        let input = contiguous_input(input, input_layout, elements).ok_or(Error::BufferTooSmall)?;
        let output =
            contiguous_output(output, output_layout, elements).ok_or(Error::BufferTooSmall)?;
        return avx2::square(input, output);
    }

    #[cfg(target_arch = "x86_64")]
    if lane == ElementwiseLane::Avx2
        && matches!(operation, UnaryOperation::Sin)
        && input_layout.is_contiguous()
        && output_layout.is_contiguous()
    {
        let elements = input_layout.shape.element_count();
        let input = contiguous_input(input, input_layout, elements).ok_or(Error::BufferTooSmall)?;
        let output =
            contiguous_output(output, output_layout, elements).ok_or(Error::BufferTooSmall)?;
        return avx2::sin(input, output);
    }

    let shape = input_layout.shape;
    let mut coordinates = [0usize; MAX_RANK];
    for linear in 0..shape.element_count() {
        unravel(linear, shape, &mut coordinates);
        let input_offset = input_layout.physical_offset(&coordinates);
        let value = input[input_offset];
        if !operation.valid_input(value) {
            return Err(Error::NonFiniteInput);
        }
        // A finite sine is finite by construction. Avoid evaluating the
        // dominant Snake activation twice merely to preserve transactional
        // output; the validation pass still rejects every non-finite input.
        if !matches!(operation, UnaryOperation::Sin) {
            operation.apply(value)?;
        }
    }
    for linear in 0..shape.element_count() {
        unravel(linear, shape, &mut coordinates);
        let input_offset = input_layout.physical_offset(&coordinates);
        let output_offset = output_layout.physical_offset(&coordinates);
        output[output_offset] = operation.apply(input[input_offset])?;
    }
    Ok(())
}

fn sigmoid_value(value: f32) -> f32 {
    // The algebraically equivalent negative branch avoids an intermediate
    // infinity for large finite negative inputs.
    if value >= 0.0 {
        1.0 / (1.0 + libm::expf(-value))
    } else {
        let exponential = libm::expf(value);
        exponential / (1.0 + exponential)
    }
}

fn round_ties_even(value: f32) -> f32 {
    // Every finite f32 at or above 2^23 is already integral.
    if value.to_bits() & 0x7FFF_FFFF >= 0x4B00_0000 {
        return value;
    }
    let truncated = value as i32;
    let fraction = value - truncated as f32;
    let rounded = if fraction > 0.5 || (fraction == 0.5 && truncated & 1 != 0) {
        truncated + 1
    } else if fraction < -0.5 || (fraction == -0.5 && truncated & 1 != 0) {
        truncated - 1
    } else {
        truncated
    };
    if rounded == 0 {
        f32::from_bits(value.to_bits() & 0x8000_0000)
    } else {
        rounded as f32
    }
}

#[allow(clippy::too_many_arguments)]
fn binary_elementwise(
    lane: ElementwiseLane,
    operation: BinaryOperation,
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
) -> Result<(), Error> {
    validate_input_layout(lhs, lhs_layout)?;
    validate_input_layout(rhs, rhs_layout)?;
    validate_output_layout(output, output_layout)?;
    let expected = broadcast_shape(lhs_layout.shape, rhs_layout.shape)?;
    if output_layout.shape != expected {
        return Err(Error::OutputShapeMismatch);
    }
    reject_alias(output, lhs)?;
    reject_alias(output, rhs)?;
    if !lane.is_available() {
        return Err(Error::UnsupportedLane);
    }

    #[cfg(target_arch = "x86_64")]
    if lane == ElementwiseLane::Avx2 && output_layout.is_contiguous() {
        let elements = expected.element_count();
        if lhs_layout.shape == expected
            && rhs_layout.shape == expected
            && lhs_layout.is_contiguous()
            && rhs_layout.is_contiguous()
        {
            let lhs = contiguous_input(lhs, lhs_layout, elements).ok_or(Error::BufferTooSmall)?;
            let rhs = contiguous_input(rhs, rhs_layout, elements).ok_or(Error::BufferTooSmall)?;
            let output =
                contiguous_output(output, output_layout, elements).ok_or(Error::BufferTooSmall)?;
            return avx2::binary_pair(operation, lhs, rhs, output);
        }
        if lhs_layout.shape.element_count() == 1
            && rhs_layout.shape == expected
            && rhs_layout.is_contiguous()
        {
            let lhs = *lhs.get(lhs_layout.offset).ok_or(Error::BufferTooSmall)?;
            let rhs = contiguous_input(rhs, rhs_layout, elements).ok_or(Error::BufferTooSmall)?;
            let output =
                contiguous_output(output, output_layout, elements).ok_or(Error::BufferTooSmall)?;
            return avx2::binary_lhs_scalar(operation, lhs, rhs, output);
        }
        if rhs_layout.shape.element_count() == 1
            && lhs_layout.shape == expected
            && lhs_layout.is_contiguous()
        {
            let lhs = contiguous_input(lhs, lhs_layout, elements).ok_or(Error::BufferTooSmall)?;
            let rhs = *rhs.get(rhs_layout.offset).ok_or(Error::BufferTooSmall)?;
            let output =
                contiguous_output(output, output_layout, elements).ok_or(Error::BufferTooSmall)?;
            return avx2::binary_rhs_scalar(operation, lhs, rhs, output);
        }
        if rhs_layout.is_contiguous()
            && lhs_layout.shape == expected
            && lhs_layout.is_contiguous()
            && let Some(row_elements) = trailing_scalar_rows(rhs_layout.shape, expected)
        {
            let lhs = contiguous_input(lhs, lhs_layout, elements).ok_or(Error::BufferTooSmall)?;
            let rhs_elements = rhs_layout.shape.element_count();
            let rhs =
                contiguous_input(rhs, rhs_layout, rhs_elements).ok_or(Error::BufferTooSmall)?;
            let output =
                contiguous_output(output, output_layout, elements).ok_or(Error::BufferTooSmall)?;
            return avx2::binary_rhs_row_scalar(operation, lhs, rhs, row_elements, output);
        }
        if lhs_layout.is_contiguous()
            && rhs_layout.shape == expected
            && rhs_layout.is_contiguous()
            && let Some(row_elements) = trailing_scalar_rows(lhs_layout.shape, expected)
        {
            let lhs_elements = lhs_layout.shape.element_count();
            let lhs =
                contiguous_input(lhs, lhs_layout, lhs_elements).ok_or(Error::BufferTooSmall)?;
            let rhs = contiguous_input(rhs, rhs_layout, elements).ok_or(Error::BufferTooSmall)?;
            let output =
                contiguous_output(output, output_layout, elements).ok_or(Error::BufferTooSmall)?;
            return avx2::binary_lhs_row_scalar(operation, lhs, rhs, row_elements, output);
        }
    }

    let mut coordinates = [0usize; MAX_RANK];
    for linear in 0..expected.element_count() {
        unravel(linear, expected, &mut coordinates);
        let lhs_offset = broadcast_offset(lhs_layout, expected, &coordinates);
        let rhs_offset = broadcast_offset(rhs_layout, expected, &coordinates);
        let lhs_value = lhs[lhs_offset];
        let rhs_value = rhs[rhs_offset];
        if !lhs_value.is_finite() || !rhs_value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        if !operation.valid_output(operation.apply(lhs_value, rhs_value)) {
            return Err(Error::NonFiniteOutput);
        }
    }

    for linear in 0..expected.element_count() {
        unravel(linear, expected, &mut coordinates);
        let lhs_offset = broadcast_offset(lhs_layout, expected, &coordinates);
        let rhs_offset = broadcast_offset(rhs_layout, expected, &coordinates);
        let output_offset = output_layout.physical_offset(&coordinates);
        output[output_offset] = operation.apply(lhs[lhs_offset], rhs[rhs_offset]);
    }
    Ok(())
}

fn trailing_scalar_rows(input: Shape, output: Shape) -> Option<usize> {
    let rank = output.rank();
    if rank == 0 || input.rank() != rank {
        return None;
    }
    let last = rank - 1;
    if input.dims[last] != 1 || output.dims[last] <= 1 {
        return None;
    }
    if input.dims[..last] != output.dims[..last] {
        return None;
    }
    Some(output.dims[last])
}

fn contiguous_input(data: &[f32], layout: TensorLayout, elements: usize) -> Option<&[f32]> {
    let end = layout.offset.checked_add(elements)?;
    data.get(layout.offset..end)
}

fn contiguous_output(
    data: &mut [f32],
    layout: TensorLayout,
    elements: usize,
) -> Option<&mut [f32]> {
    let end = layout.offset.checked_add(elements)?;
    data.get_mut(layout.offset..end)
}

fn validate_input_layout(data: &[f32], layout: TensorLayout) -> Result<(), Error> {
    if layout.required_len()? > data.len() {
        Err(Error::BufferTooSmall)
    } else {
        Ok(())
    }
}

fn validate_output_layout(data: &[f32], layout: TensorLayout) -> Result<(), Error> {
    if layout.required_len()? > data.len() {
        return Err(Error::BufferTooSmall);
    }
    if !layout.is_non_overlapping()? {
        return Err(Error::OverlappingOutput);
    }
    Ok(())
}

fn unravel(mut linear: usize, shape: Shape, coordinates: &mut [usize; MAX_RANK]) {
    coordinates.fill(0);
    for axis in (0..shape.rank()).rev() {
        coordinates[axis] = linear % shape.dims[axis];
        linear /= shape.dims[axis];
    }
}

fn broadcast_offset(
    layout: TensorLayout,
    output_shape: Shape,
    coordinates: &[usize; MAX_RANK],
) -> usize {
    let leading = output_shape.rank() - layout.shape.rank();
    let mut offset = layout.offset;
    for input_axis in 0..layout.shape.rank() {
        let coordinate = if layout.shape.dims[input_axis] == 1 {
            0
        } else {
            coordinates[leading + input_axis]
        };
        offset += coordinate * layout.strides[input_axis];
    }
    offset
}

/// Return the exact output shape of ONNX `ReduceMean` for one axis.
pub fn reduced_shape(shape: Shape, axis: isize, keep_dims: bool) -> Result<Shape, Error> {
    let axis = shape.normalized_axis(axis)?;
    if keep_dims {
        let mut dims = shape.dims;
        dims[axis] = 1;
        Shape::new(&dims[..shape.rank()])
    } else {
        let mut dims = [0usize; MAX_RANK];
        let mut destination = 0usize;
        for source in 0..shape.rank() {
            if source != axis {
                dims[destination] = shape.dims[source];
                destination += 1;
            }
        }
        Shape::new(&dims[..destination])
    }
}

/// ONNX `ReduceMean` over one axis of a contiguous row-major tensor.
pub fn reduce_mean(
    input: &[f32],
    shape: Shape,
    axis: isize,
    keep_dims: bool,
    output: &mut [f32],
) -> Result<(), Error> {
    let axis = shape.normalized_axis(axis)?;
    let output_shape = reduced_shape(shape, axis as isize, keep_dims)?;
    validate_contiguous_input(input, shape)?;
    validate_contiguous_output(output, output_shape)?;
    reject_alias(output, input)?;

    let outer = shape.product(0..axis);
    let axis_len = shape.dims[axis];
    let inner = shape.product(axis + 1..shape.rank());
    for outer_index in 0..outer {
        for inner_index in 0..inner {
            let mean = reduced_mean_value(input, outer_index, inner_index, axis_len, inner)?;
            if !mean.is_finite() {
                return Err(Error::NonFiniteOutput);
            }
        }
    }
    for outer_index in 0..outer {
        for inner_index in 0..inner {
            output[outer_index * inner + inner_index] =
                reduced_mean_value(input, outer_index, inner_index, axis_len, inner)?;
        }
    }
    Ok(())
}

fn reduced_mean_value(
    input: &[f32],
    outer_index: usize,
    inner_index: usize,
    axis_len: usize,
    inner: usize,
) -> Result<f32, Error> {
    let mut sum = 0.0f32;
    for axis_index in 0..axis_len {
        let index = (outer_index * axis_len + axis_index) * inner + inner_index;
        let value = input[index];
        if !value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        sum += value;
        if !sum.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    Ok(sum / axis_len as f32)
}

/// ONNX `LayerNormalization` for a contiguous tensor and normalized suffix.
pub fn layer_normalization(
    input: &[f32],
    shape: Shape,
    axis: isize,
    scale: &[f32],
    bias: &[f32],
    epsilon: f32,
    output: &mut [f32],
) -> Result<(), Error> {
    let axis = shape.normalized_axis(axis)?;
    validate_epsilon(epsilon)?;
    validate_contiguous_input(input, shape)?;
    validate_contiguous_output(output, shape)?;
    let normalized = shape.product(axis..shape.rank());
    if scale.len() < normalized || bias.len() < normalized {
        return Err(Error::ParameterTooSmall);
    }
    reject_alias(output, input)?;
    reject_alias(output, scale)?;
    reject_alias(output, bias)?;
    validate_finite(&scale[..normalized])?;
    validate_finite(&bias[..normalized])?;

    let rows = shape.element_count() / normalized;
    for row in 0..rows {
        let start = row * normalized;
        let (mean, inverse_stddev) = direct_statistics(input, start, normalized, epsilon)?;
        for column in 0..normalized {
            let result = normalized_value(
                input[start + column],
                mean,
                inverse_stddev,
                scale[column],
                bias[column],
            );
            if !result.is_finite() {
                return Err(Error::NonFiniteOutput);
            }
        }
    }
    for row in 0..rows {
        let start = row * normalized;
        let (mean, inverse_stddev) = direct_statistics(input, start, normalized, epsilon)?;
        for column in 0..normalized {
            output[start + column] = normalized_value(
                input[start + column],
                mean,
                inverse_stddev,
                scale[column],
                bias[column],
            );
        }
    }
    Ok(())
}

/// ONNX `Softmax` over one axis of a contiguous row-major tensor.
pub fn softmax(input: &[f32], shape: Shape, axis: isize, output: &mut [f32]) -> Result<(), Error> {
    let axis = shape.normalized_axis(axis)?;
    validate_contiguous_input(input, shape)?;
    validate_contiguous_output(output, shape)?;
    reject_alias(output, input)?;
    validate_finite(&input[..shape.element_count()])?;

    let outer = shape.product(0..axis);
    let axis_len = shape.dims[axis];
    let inner = shape.product(axis + 1..shape.rank());
    for outer_index in 0..outer {
        for inner_index in 0..inner {
            let (maximum, sum) =
                softmax_statistics(input, outer_index, inner_index, axis_len, inner)?;
            for axis_index in 0..axis_len {
                let index = (outer_index * axis_len + axis_index) * inner + inner_index;
                let result = libm::expf(input[index] - maximum) / sum;
                if !result.is_finite() {
                    return Err(Error::NonFiniteOutput);
                }
            }
        }
    }
    for outer_index in 0..outer {
        for inner_index in 0..inner {
            let (maximum, sum) =
                softmax_statistics(input, outer_index, inner_index, axis_len, inner)?;
            for axis_index in 0..axis_len {
                let index = (outer_index * axis_len + axis_index) * inner + inner_index;
                output[index] = libm::expf(input[index] - maximum) / sum;
            }
        }
    }
    Ok(())
}

/// Microsoft `FastGelu`, including its optional last-dimension bias.
pub fn fast_gelu(
    input: &[f32],
    shape: Shape,
    bias: Option<&[f32]>,
    output: &mut [f32],
) -> Result<(), Error> {
    if shape.rank() == 0 {
        return Err(Error::InvalidAxis);
    }
    validate_contiguous_input(input, shape)?;
    validate_contiguous_output(output, shape)?;
    reject_alias(output, input)?;
    let width = shape.dims[shape.rank() - 1];
    if let Some(values) = bias {
        if values.len() < width {
            return Err(Error::ParameterTooSmall);
        }
        reject_alias(output, values)?;
        validate_finite(&values[..width])?;
    }

    for index in 0..shape.element_count() {
        let input_value = input[index];
        if !input_value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        let biased = input_value + bias.map_or(0.0, |values| values[index % width]);
        if !biased.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
        if !fast_gelu_value(biased).is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    for index in 0..shape.element_count() {
        let biased = input[index] + bias.map_or(0.0, |values| values[index % width]);
        output[index] = fast_gelu_value(biased);
    }
    Ok(())
}

/// Microsoft `SkipLayerNormalization` used by all twelve ALBERT blocks.
pub fn skip_layer_normalization(
    input: &[f32],
    skip: &[f32],
    shape: Shape,
    scale: &[f32],
    bias: &[f32],
    epsilon: f32,
    output: &mut [f32],
) -> Result<(), Error> {
    if shape.rank() == 0 {
        return Err(Error::InvalidAxis);
    }
    validate_epsilon(epsilon)?;
    validate_contiguous_input(input, shape)?;
    validate_contiguous_input(skip, shape)?;
    validate_contiguous_output(output, shape)?;
    let width = shape.dims[shape.rank() - 1];
    if scale.len() < width || bias.len() < width {
        return Err(Error::ParameterTooSmall);
    }
    reject_alias(output, input)?;
    reject_alias(output, skip)?;
    reject_alias(output, scale)?;
    reject_alias(output, bias)?;
    validate_finite(&scale[..width])?;
    validate_finite(&bias[..width])?;

    let rows = shape.element_count() / width;
    for row in 0..rows {
        let start = row * width;
        let (mean, inverse_stddev) = skip_statistics(input, skip, start, width, epsilon)?;
        for column in 0..width {
            let combined = input[start + column] + skip[start + column];
            let result =
                normalized_value(combined, mean, inverse_stddev, scale[column], bias[column]);
            if !result.is_finite() {
                return Err(Error::NonFiniteOutput);
            }
        }
    }
    for row in 0..rows {
        let start = row * width;
        let (mean, inverse_stddev) = skip_statistics(input, skip, start, width, epsilon)?;
        for column in 0..width {
            let combined = input[start + column] + skip[start + column];
            output[start + column] =
                normalized_value(combined, mean, inverse_stddev, scale[column], bias[column]);
        }
    }
    Ok(())
}

fn direct_statistics(
    input: &[f32],
    start: usize,
    width: usize,
    epsilon: f32,
) -> Result<(f32, f32), Error> {
    let mut sum = 0.0f32;
    for &value in &input[start..start + width] {
        if !value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        sum += value;
        if !sum.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    let mean = sum / width as f32;
    let mut squared_sum = 0.0f32;
    for &value in &input[start..start + width] {
        let centered = value - mean;
        squared_sum += centered * centered;
        if !squared_sum.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    inverse_standard_deviation(squared_sum / width as f32, epsilon).map(|inverse| (mean, inverse))
}

fn skip_statistics(
    input: &[f32],
    skip: &[f32],
    start: usize,
    width: usize,
    epsilon: f32,
) -> Result<(f32, f32), Error> {
    let mut sum = 0.0f32;
    for column in 0..width {
        let lhs = input[start + column];
        let rhs = skip[start + column];
        if !lhs.is_finite() || !rhs.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        let combined = lhs + rhs;
        if !combined.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
        sum += combined;
        if !sum.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    let mean = sum / width as f32;
    let mut squared_sum = 0.0f32;
    for column in 0..width {
        let combined = input[start + column] + skip[start + column];
        let centered = combined - mean;
        squared_sum += centered * centered;
        if !squared_sum.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    inverse_standard_deviation(squared_sum / width as f32, epsilon).map(|inverse| (mean, inverse))
}

fn inverse_standard_deviation(variance: f32, epsilon: f32) -> Result<f32, Error> {
    let adjusted = variance + epsilon;
    if !adjusted.is_finite() || adjusted <= 0.0 {
        return Err(Error::NonFiniteOutput);
    }
    let inverse = 1.0f32 / libm::sqrtf(adjusted);
    if inverse.is_finite() {
        Ok(inverse)
    } else {
        Err(Error::NonFiniteOutput)
    }
}

fn normalized_value(value: f32, mean: f32, inverse: f32, scale: f32, bias: f32) -> f32 {
    let centered = value - mean;
    let normalized = centered * inverse;
    let scaled = normalized * scale;
    scaled + bias
}

fn softmax_statistics(
    input: &[f32],
    outer_index: usize,
    inner_index: usize,
    axis_len: usize,
    inner: usize,
) -> Result<(f32, f32), Error> {
    let first = outer_index * axis_len * inner + inner_index;
    let mut maximum = input[first];
    for axis_index in 1..axis_len {
        let index = (outer_index * axis_len + axis_index) * inner + inner_index;
        maximum = maximum.max(input[index]);
    }
    let mut sum = 0.0f32;
    for axis_index in 0..axis_len {
        let index = (outer_index * axis_len + axis_index) * inner + inner_index;
        sum += libm::expf(input[index] - maximum);
        if !sum.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    if sum > 0.0 {
        Ok((maximum, sum))
    } else {
        Err(Error::NonFiniteOutput)
    }
}

fn fast_gelu_value(value: f32) -> f32 {
    const CUBIC: f32 = 0.044_715;
    const SQRT_TWO_OVER_PI: f32 = 0.797_884_6;
    let squared = value * value;
    let cubic = squared * value;
    let inner = value + CUBIC * cubic;
    let gate = 1.0 + libm::tanhf(SQRT_TWO_OVER_PI * inner);
    (0.5 * value) * gate
}

fn validate_contiguous_input(data: &[f32], shape: Shape) -> Result<(), Error> {
    if data.len() < shape.element_count() {
        Err(Error::BufferTooSmall)
    } else {
        Ok(())
    }
}

fn validate_contiguous_output(data: &[f32], shape: Shape) -> Result<(), Error> {
    if data.len() < shape.element_count() {
        Err(Error::BufferTooSmall)
    } else {
        Ok(())
    }
}

fn validate_epsilon(epsilon: f32) -> Result<(), Error> {
    if epsilon.is_finite() && epsilon >= 0.0 {
        Ok(())
    } else {
        Err(Error::InvalidEpsilon)
    }
}

fn validate_finite(values: &[f32]) -> Result<(), Error> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(Error::NonFiniteInput)
    }
}

fn reject_alias(output: &[f32], input: &[f32]) -> Result<(), Error> {
    if memory_ranges_overlap(output.as_ptr(), output.len(), input.as_ptr(), input.len()) {
        Err(Error::Aliasing)
    } else {
        Ok(())
    }
}

fn memory_ranges_overlap(lhs: *const f32, lhs_len: usize, rhs: *const f32, rhs_len: usize) -> bool {
    if lhs_len == 0 || rhs_len == 0 {
        return false;
    }
    let lhs_start = lhs as usize;
    let rhs_start = rhs as usize;
    let lhs_end = lhs_start.saturating_add(lhs_len.saturating_mul(size_of::<f32>()));
    let rhs_end = rhs_start.saturating_add(rhs_len.saturating_mul(size_of::<f32>()));
    lhs_start < rhs_end && rhs_start < lhs_end
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORT127_POW_SQUARE: &[u8] = include_bytes!("../tests/fixtures/ort127_pow_square.bin");

    type BinaryLaneKernel = fn(
        ElementwiseLane,
        &[f32],
        TensorLayout,
        &[f32],
        TensorLayout,
        &mut [f32],
        TensorLayout,
    ) -> Result<(), Error>;

    const BINARY_LANE_KERNELS: [(&str, BinaryLaneKernel); 4] = [
        ("add", add_on_lane),
        ("mul", mul_on_lane),
        ("div", div_on_lane),
        ("sub", sub_on_lane),
    ];

    // Generated with ONNX 1.22.0 and ONNX Runtime 1.28.0, optimizations
    // disabled, sequential CPU execution, opset 20 / com.microsoft opset 1.

    fn assert_close(actual: &[f32], expected_bits: &[u32], tolerance: f32) {
        assert_eq!(actual.len(), expected_bits.len());
        for (index, (&actual, &bits)) in actual.iter().zip(expected_bits).enumerate() {
            let expected = f32::from_bits(bits);
            let error = (actual - expected).abs();
            assert!(
                error <= tolerance,
                "index {index}: actual={actual:?} expected={expected:?} error={error:?}"
            );
        }
    }

    fn assert_close_relative(actual: &[f32], expected_bits: &[u32], tolerance: f32) {
        assert_eq!(actual.len(), expected_bits.len());
        for (index, (&actual, &bits)) in actual.iter().zip(expected_bits).enumerate() {
            let expected = f32::from_bits(bits);
            let error = (actual - expected).abs();
            let limit = tolerance * expected.abs().max(1.0);
            assert!(
                error <= limit,
                "index {index}: actual={actual:?} expected={expected:?} error={error:?} limit={limit:?}"
            );
        }
    }

    fn fixture_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn pinned_pow_square_matches_official_ort_1_27_fixture() {
        const HEADER: usize = 16;
        assert_eq!(&ORT127_POW_SQUARE[..8], b"KPOW1271");
        assert_eq!(fixture_u32(ORT127_POW_SQUARE, 8), 1);
        let elements = fixture_u32(ORT127_POW_SQUARE, 12) as usize;
        assert_eq!(elements, 12);
        assert_eq!(ORT127_POW_SQUARE.len(), HEADER + elements * 8);

        let mut input = [0.0_f32; 12];
        for (index, value) in input.iter_mut().enumerate() {
            *value = f32::from_bits(fixture_u32(ORT127_POW_SQUARE, HEADER + index * 4));
        }
        let shape = Shape::new(&[2, 2, 3]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let mut output = [123.0_f32; 12];
        pow_square(&input, layout, &mut output, layout).unwrap();

        let expected_start = HEADER + elements * 4;
        for (index, &value) in output.iter().enumerate() {
            assert_eq!(
                value.to_bits(),
                fixture_u32(ORT127_POW_SQUARE, expected_start + index * 4),
                "output index {index}"
            );
        }
        assert_eq!(output[2].to_bits(), 0, "negative zero squared is +0");
    }

    #[test]
    fn pow_square_numerical_failures_are_transactional() {
        let shape = Shape::new(&[3]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let mut output = [77.0_f32; 3];
        assert_eq!(
            pow_square(&[2.0, f32::MAX, 3.0], layout, &mut output, layout),
            Err(Error::NonFiniteOutput)
        );
        assert_eq!(output, [77.0; 3]);
        assert_eq!(
            pow_square(&[2.0, f32::NAN, 3.0], layout, &mut output, layout),
            Err(Error::NonFiniteInput)
        );
        assert_eq!(output, [77.0; 3]);
    }

    #[test]
    fn strided_add_obeys_multidirectional_broadcasting() {
        let lhs_shape = Shape::new(&[2, 1, 3]).unwrap();
        let rhs_shape = Shape::new(&[2, 1]).unwrap();
        let output_shape = Shape::new(&[2, 2, 3]).unwrap();
        let lhs_layout = TensorLayout::strided(lhs_shape, &[4, 3, 1], 0).unwrap();
        let rhs_layout = TensorLayout::contiguous(rhs_shape);
        let output_layout = TensorLayout::strided(output_shape, &[8, 3, 1], 0).unwrap();
        let lhs = [1.0, 2.0, 3.0, -99.0, 4.0, 5.0, 6.0];
        let rhs = [10.0, 20.0];
        let mut output = [-99.0f32; 14];

        add(&lhs, lhs_layout, &rhs, rhs_layout, &mut output, output_layout).unwrap();
        assert_eq!(
            output,
            [
                11.0, 12.0, 13.0, 21.0, 22.0, 23.0, -99.0, -99.0, 14.0, 15.0, 16.0, 24.0, 25.0,
                26.0,
            ]
        );
    }

    #[test]
    fn every_binary_operation_preserves_f32_operation_boundaries() {
        let vector = Shape::new(&[4]).unwrap();
        let scalar = Shape::scalar();
        let vector_layout = TensorLayout::contiguous(vector);
        let scalar_layout = TensorLayout::contiguous(scalar);
        let input = [1.5, -2.0, 0.25, 8.0];
        let rhs = [2.0];
        let mut output = [0.0; 4];

        mul(&input, vector_layout, &rhs, scalar_layout, &mut output, vector_layout).unwrap();
        assert_eq!(output, [3.0, -4.0, 0.5, 16.0]);
        div(&input, vector_layout, &rhs, scalar_layout, &mut output, vector_layout).unwrap();
        assert_eq!(output, [0.75, -1.0, 0.125, 4.0]);
        sub(&input, vector_layout, &rhs, scalar_layout, &mut output, vector_layout).unwrap();
        assert_eq!(output, [-0.5, -4.0, -1.75, 6.0]);
    }

    #[test]
    fn binary_validation_is_transactional() {
        let shape = Shape::new(&[2, 2]).unwrap();
        let contiguous = TensorLayout::contiguous(shape);
        let overlapping = TensorLayout::strided(shape, &[1, 1], 0).unwrap();
        let mut output = [123.0f32; 4];
        assert_eq!(
            add(&[1.0; 4], contiguous, &[2.0; 4], contiguous, &mut output, overlapping,),
            Err(Error::OverlappingOutput)
        );
        assert_eq!(output, [123.0; 4]);

        assert_eq!(
            div(&[1.0; 4], contiguous, &[1.0, 0.0, 1.0, 1.0], contiguous, &mut output, contiguous,),
            Err(Error::NonFiniteOutput)
        );
        assert_eq!(output, [123.0; 4]);
    }

    #[test]
    fn avx2_binary_pair_and_scalar_broadcast_are_bit_exact() {
        if !ElementwiseLane::Avx2.is_available() {
            return;
        }
        assert_eq!(elementwise_lane(), ElementwiseLane::Avx2);

        let shape = Shape::new(&[257]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let lhs_pattern = [
            -0.0,
            0.0,
            f32::from_bits(1),
            -f32::from_bits(1),
            f32::MIN_POSITIVE,
            -f32::MIN_POSITIVE,
            0.125,
            -3.75,
            1024.0,
        ];
        let rhs_pattern = [1.0, -1.0, 2.0, -2.0, 0.5, -0.25, 3.0, -8.0, 0.75];
        let lhs: Vec<_> = (0..shape.element_count())
            .map(|index| lhs_pattern[index % lhs_pattern.len()])
            .collect();
        let rhs: Vec<_> = (0..shape.element_count())
            .map(|index| rhs_pattern[index % rhs_pattern.len()])
            .collect();
        let scalar_shape = Shape::scalar();
        let scalar_layout = TensorLayout::contiguous(scalar_shape);

        for (name, kernel) in BINARY_LANE_KERNELS {
            let mut scalar = vec![f32::NAN; shape.element_count()];
            let mut vector = vec![f32::NAN; shape.element_count()];
            kernel(ElementwiseLane::Scalar, &lhs, layout, &rhs, layout, &mut scalar, layout)
                .unwrap();
            kernel(ElementwiseLane::Avx2, &lhs, layout, &rhs, layout, &mut vector, layout).unwrap();
            assert_eq!(
                vector
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                scalar
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                "same-shape {name}"
            );

            for lhs_is_scalar in [true, false] {
                scalar.fill(f32::NAN);
                vector.fill(f32::NAN);
                let one = [-0.75_f32];
                let (left, left_layout, right, right_layout) = if lhs_is_scalar {
                    (&one[..], scalar_layout, &rhs[..], layout)
                } else {
                    (&lhs[..], layout, &one[..], scalar_layout)
                };
                kernel(
                    ElementwiseLane::Scalar,
                    left,
                    left_layout,
                    right,
                    right_layout,
                    &mut scalar,
                    layout,
                )
                .unwrap();
                kernel(
                    ElementwiseLane::Avx2,
                    left,
                    left_layout,
                    right,
                    right_layout,
                    &mut vector,
                    layout,
                )
                .unwrap();
                assert_eq!(
                    vector
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>(),
                    scalar
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>(),
                    "scalar-broadcast {name} lhs_is_scalar={lhs_is_scalar}"
                );
            }
        }
    }

    #[test]
    fn avx2_trailing_scalar_rows_are_bit_exact_and_transactional() {
        if !ElementwiseLane::Avx2.is_available() {
            return;
        }
        let dense_shape = Shape::new(&[2, 3, 17]).unwrap();
        let row_shape = Shape::new(&[2, 3, 1]).unwrap();
        let dense_layout = TensorLayout::contiguous(dense_shape);
        let row_layout = TensorLayout::contiguous(row_shape);
        let dense_pattern = [
            0.0001_f32, -0.0001, 0.03125, -0.0625, 0.125, -3.75, 1024.0, -0.25, 2.0,
        ];
        let dense: Vec<_> = (0..dense_shape.element_count())
            .map(|index| dense_pattern[index % dense_pattern.len()])
            .collect();
        let rows = [0.5_f32, -0.75, 2.0, -3.0, 0.25, -8.0];

        for (name, kernel) in BINARY_LANE_KERNELS {
            for lhs_is_row_scalar in [true, false] {
                let (lhs, lhs_layout, rhs, rhs_layout) = if lhs_is_row_scalar {
                    (&rows[..], row_layout, &dense[..], dense_layout)
                } else {
                    (&dense[..], dense_layout, &rows[..], row_layout)
                };
                let mut scalar = vec![f32::NAN; dense_shape.element_count()];
                let mut vector = vec![f32::NAN; dense_shape.element_count()];
                kernel(
                    ElementwiseLane::Scalar,
                    lhs,
                    lhs_layout,
                    rhs,
                    rhs_layout,
                    &mut scalar,
                    dense_layout,
                )
                .unwrap();
                kernel(
                    ElementwiseLane::Avx2,
                    lhs,
                    lhs_layout,
                    rhs,
                    rhs_layout,
                    &mut vector,
                    dense_layout,
                )
                .unwrap();
                assert_eq!(
                    vector
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>(),
                    scalar
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>(),
                    "trailing-row broadcast {name} lhs_is_row_scalar={lhs_is_row_scalar}"
                );
            }
        }

        for lhs_is_row_scalar in [true, false] {
            let mut invalid_rows = rows;
            invalid_rows[5] = f32::MAX;
            let mut invalid_dense = dense.clone();
            invalid_dense[5 * 17 + 11] = f32::MAX;
            let (lhs, lhs_layout, rhs, rhs_layout) = if lhs_is_row_scalar {
                (&invalid_rows[..], row_layout, &invalid_dense[..], dense_layout)
            } else {
                (&invalid_dense[..], dense_layout, &invalid_rows[..], row_layout)
            };
            let mut output = vec![77.0_f32; dense_shape.element_count()];
            assert_eq!(
                add_on_lane(
                    ElementwiseLane::Avx2,
                    lhs,
                    lhs_layout,
                    rhs,
                    rhs_layout,
                    &mut output,
                    dense_layout,
                ),
                Err(Error::NonFiniteOutput)
            );
            assert_eq!(output, vec![77.0; dense_shape.element_count()]);
        }
    }

    #[test]
    fn avx2_square_is_bit_exact_including_tail_subnormal_and_signed_zero() {
        if !ElementwiseLane::Avx2.is_available() {
            return;
        }
        let shape = Shape::new(&[257]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let pattern = [
            -0.0,
            0.0,
            f32::from_bits(1),
            -f32::from_bits(1),
            f32::MIN_POSITIVE,
            -f32::MIN_POSITIVE,
            0.25,
            -3.75,
            10_000_000_000.0,
        ];
        let input: Vec<_> = (0..shape.element_count())
            .map(|index| pattern[index % pattern.len()])
            .collect();
        let mut scalar = vec![f32::NAN; shape.element_count()];
        let mut vector = vec![f32::NAN; shape.element_count()];
        pow_square_on_lane(ElementwiseLane::Scalar, &input, layout, &mut scalar, layout).unwrap();
        pow_square_on_lane(ElementwiseLane::Avx2, &input, layout, &mut vector, layout).unwrap();
        assert_eq!(
            vector
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            scalar
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn avx2_validation_matches_scalar_and_never_partially_writes() {
        if !ElementwiseLane::Avx2.is_available() {
            return;
        }
        let shape = Shape::new(&[19]).unwrap();
        let layout = TensorLayout::contiguous(shape);

        for (name, kernel) in BINARY_LANE_KERNELS {
            let mut lhs = vec![2.0_f32; shape.element_count()];
            let mut rhs = vec![3.0_f32; shape.element_count()];
            match name {
                "add" | "mul" => {
                    lhs[8] = f32::MAX;
                    rhs[8] = f32::MAX;
                }
                "div" => rhs[16] = 0.0,
                "sub" => {
                    lhs[7] = f32::MAX;
                    rhs[7] = -f32::MAX;
                }
                _ => unreachable!(),
            }
            let mut scalar = vec![91.0_f32; shape.element_count()];
            let mut vector = scalar.clone();
            let scalar_error =
                kernel(ElementwiseLane::Scalar, &lhs, layout, &rhs, layout, &mut scalar, layout);
            let vector_error =
                kernel(ElementwiseLane::Avx2, &lhs, layout, &rhs, layout, &mut vector, layout);
            assert_eq!(vector_error, scalar_error, "{name} error class");
            assert_eq!(vector_error, Err(Error::NonFiniteOutput), "{name}");
            assert_eq!(scalar, vec![91.0; shape.element_count()]);
            assert_eq!(vector, vec![91.0; shape.element_count()]);

            lhs.fill(2.0);
            rhs.fill(3.0);
            rhs[15] = f32::NAN;
            scalar.fill(92.0);
            vector.fill(92.0);
            let scalar_error =
                kernel(ElementwiseLane::Scalar, &lhs, layout, &rhs, layout, &mut scalar, layout);
            let vector_error =
                kernel(ElementwiseLane::Avx2, &lhs, layout, &rhs, layout, &mut vector, layout);
            assert_eq!(vector_error, scalar_error, "{name} input error class");
            assert_eq!(vector_error, Err(Error::NonFiniteInput), "{name}");
            assert_eq!(scalar, vec![92.0; shape.element_count()]);
            assert_eq!(vector, vec![92.0; shape.element_count()]);
        }

        let mut input = vec![2.0_f32; shape.element_count()];
        input[8] = f32::MAX;
        let mut scalar = vec![93.0_f32; shape.element_count()];
        let mut vector = scalar.clone();
        let scalar_error =
            pow_square_on_lane(ElementwiseLane::Scalar, &input, layout, &mut scalar, layout);
        let vector_error =
            pow_square_on_lane(ElementwiseLane::Avx2, &input, layout, &mut vector, layout);
        assert_eq!(vector_error, scalar_error);
        assert_eq!(vector_error, Err(Error::NonFiniteOutput));
        assert_eq!(scalar, vec![93.0; shape.element_count()]);
        assert_eq!(vector, vec![93.0; shape.element_count()]);
    }

    #[test]
    fn ieee_division_by_signed_zero_feeds_finite_atan_phase() {
        let shape = Shape::new(&[8]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let lhs = [1.0, -1.0, 1.0, -1.0, 2.0, -2.0, 2.0, -2.0];
        let rhs = [0.0, 0.0, -0.0, -0.0, 0.0, 0.0, -0.0, -0.0];
        let mut scalar = [0.0; 8];
        div_ieee_on_lane(ElementwiseLane::Scalar, &lhs, layout, &rhs, layout, &mut scalar, layout)
            .unwrap();
        assert_eq!(
            scalar.map(f32::to_bits),
            [
                f32::INFINITY.to_bits(),
                f32::NEG_INFINITY.to_bits(),
                f32::NEG_INFINITY.to_bits(),
                f32::INFINITY.to_bits(),
                f32::INFINITY.to_bits(),
                f32::NEG_INFINITY.to_bits(),
                f32::NEG_INFINITY.to_bits(),
                f32::INFINITY.to_bits(),
            ]
        );

        if ElementwiseLane::Avx2.is_available() {
            let mut vector = [0.0; 8];
            div_ieee_on_lane(
                ElementwiseLane::Avx2,
                &lhs,
                layout,
                &rhs,
                layout,
                &mut vector,
                layout,
            )
            .unwrap();
            assert_eq!(vector.map(f32::to_bits), scalar.map(f32::to_bits));
        }

        let mut phase = [0.0; 8];
        atan_ieee(&scalar, layout, &mut phase, layout).unwrap();
        assert_eq!(
            phase.map(f32::to_bits),
            [
                0x3fc9_0fda,
                0xbfc9_0fda,
                0xbfc9_0fda,
                0x3fc9_0fda,
                0x3fc9_0fda,
                0xbfc9_0fda,
                0xbfc9_0fda,
                0x3fc9_0fda,
            ]
        );
    }

    #[test]
    fn explicit_avx2_lane_preserves_general_strided_fallback() {
        if !ElementwiseLane::Avx2.is_available() {
            return;
        }
        let shape = Shape::new(&[2, 3]).unwrap();
        let input_layout = TensorLayout::strided(shape, &[4, 1], 1).unwrap();
        let output_layout = TensorLayout::strided(shape, &[5, 1], 1).unwrap();
        let lhs = [-9.0, 1.0, 2.0, 3.0, -9.0, 4.0, 5.0, 6.0];
        let rhs = [-9.0, 10.0, 20.0, 30.0, -9.0, 40.0, 50.0, 60.0];
        let mut scalar = [-7.0_f32; 9];
        let mut vector = scalar;
        add_on_lane(
            ElementwiseLane::Scalar,
            &lhs,
            input_layout,
            &rhs,
            input_layout,
            &mut scalar,
            output_layout,
        )
        .unwrap();
        add_on_lane(
            ElementwiseLane::Avx2,
            &lhs,
            input_layout,
            &rhs,
            input_layout,
            &mut vector,
            output_layout,
        )
        .unwrap();
        assert_eq!(vector.map(f32::to_bits), scalar.map(f32::to_bits));
    }

    #[test]
    fn reduce_mean_matches_both_pinned_axes_exactly() {
        let shape = Shape::new(&[2, 2, 3]).unwrap();
        let input = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0,
        ];
        let mut axis_zero = [0.0f32; 6];
        reduce_mean(&input, shape, 0, true, &mut axis_zero).unwrap();
        assert_eq!(axis_zero, [4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);

        let mut axis_two = [0.0f32; 4];
        reduce_mean(&input, shape, 2, true, &mut axis_two).unwrap();
        assert_eq!(axis_two, [2.0, 5.0, 8.0, 11.0]);
        assert_eq!(reduced_shape(shape, -1, true).unwrap().dims(), &[2, 2, 1]);
    }

    #[test]
    fn layer_normalization_matches_ort_fixture() {
        let shape = Shape::new(&[2, 2, 4]).unwrap();
        let input = [
            1.0, 2.0, 4.0, -3.0, 1000.0, 1000.25, 999.75, 1001.0, -0.1, 0.2, 0.3, 0.9, 5.0, -2.0,
            8.0, 1.0,
        ];
        let scale = [1.0, 0.5, -2.0, 3.0];
        let bias = [0.1, -0.2, 0.3, -0.4];
        let mut output = [0.0f32; 16];
        layer_normalization(&input, shape, -1, &scale, &bias, 1.0e-12, &mut output).unwrap();
        assert_close(
            &output,
            &[
                0x3DCC_CCCD,
                0xBB7E_8880,
                0xC003_6ACD,
                0xC0A3_6ACD,
                0xBEDE_79BB,
                0xBE4C_CCCD,
                0x401C_09AA,
                0x408D_2479,
                0xBF89_0084,
                0xBEBE_84D2,
                0x3EE0_1852,
                0x408B_3479,
                0x3F20_0ECC,
                0xBF5B_45B1,
                0xC014_DF4B,
                0xBFFC_E2FE,
            ],
            6.0e-7,
        );
    }

    #[test]
    fn softmax_matches_ort_fixture() {
        let shape = Shape::new(&[1, 1, 2, 5]).unwrap();
        let input = [-10.0, -1.0, 0.0, 1.0, 10.0, 100.0, 100.0, 99.0, -100.0, 0.0];
        let mut output = [0.0f32; 10];
        softmax(&input, shape, -1, &mut output).unwrap();
        assert_close(
            &output,
            &[
                0x310D_9D7A,
                0x378C_13FA,
                0x383E_62C4,
                0x3901_616C,
                0x3F7F_F3D9,
                0x3ED8_3A2C,
                0x3ED8_3A2C,
                0x3E1F_1753,
                0x0000_0000,
                0x0000_0000,
            ],
            2.0e-7,
        );
    }

    #[test]
    fn fast_gelu_with_bias_matches_ort_fixture() {
        let shape = Shape::new(&[2, 5]).unwrap();
        let input = [-4.0, -1.25, -0.1, 0.0, 0.75, 1.5, 3.0, 6.0, -2.5, 0.2];
        let bias = [0.1, -0.2, 0.3, -0.4, 0.5];
        let mut output = [0.0f32; 10];
        fast_gelu(&input, shape, Some(&bias), &mut output).unwrap();
        assert_close(
            &output,
            &[
                0xB8EB_7667,
                0xBDDA_D172,
                0x3DED_4385,
                0xBE0D_259F,
                0x3F8F_1142,
                0x3FC1_8DB4,
                0x4032_C59C,
                0x40C9_999A,
                0xBBA2_BF94,
                0x3F07_D372,
            ],
            3.0e-7,
        );
    }

    #[test]
    fn skip_layer_normalization_matches_ort_fixture() {
        let shape = Shape::new(&[1, 2, 4]).unwrap();
        let input = [1.0, 2.0, 4.0, -3.0, 1000.0, 1000.25, 999.75, 1001.0];
        let skip = [0.5, -1.0, 2.0, 3.0, -999.0, -999.0, -999.0, -999.0];
        let scale = [1.0, 0.5, -2.0, 3.0];
        let bias = [0.1, -0.2, 0.3, -0.4];
        let mut output = [0.0f32; 8];
        skip_layer_normalization(&input, &skip, shape, &scale, &bias, 1.0e-12, &mut output)
            .unwrap();
        assert_close(
            &output,
            &[
                0xBE2F_AE24,
                0xBEE3_893E,
                0xC044_4FEB,
                0xC04A_E04C,
                0xBEDE_79BB,
                0xBE4C_CCCD,
                0x401C_09AA,
                0x408D_2479,
            ],
            6.0e-7,
        );
    }

    #[test]
    fn normalization_rejects_nonfinite_input_without_writing() {
        let shape = Shape::new(&[1, 4]).unwrap();
        let input = [1.0, 2.0, f32::NAN, 4.0];
        let mut output = [99.0f32; 4];
        assert_eq!(
            layer_normalization(&input, shape, -1, &[1.0; 4], &[0.0; 4], 1.0e-5, &mut output,),
            Err(Error::NonFiniteInput)
        );
        assert_eq!(output, [99.0; 4]);
    }

    #[test]
    fn round_strided_matches_ort_ties_and_signed_zero_exactly() {
        let shape = Shape::new(&[2, 6]).unwrap();
        let input_layout = TensorLayout::strided(shape, &[8, 1], 0).unwrap();
        let output_layout = TensorLayout::strided(shape, &[7, 1], 0).unwrap();
        let input = [
            -3.5,
            -2.5,
            -1.5,
            -0.5,
            -0.0,
            0.0,
            f32::NAN,
            f32::NAN,
            0.5,
            1.5,
            2.5,
            3.5,
            -8_388_607.5,
            8_388_607.5,
        ];
        let mut output = [f32::from_bits(0x7FC0_0123); 13];
        round(&input, input_layout, &mut output, output_layout).unwrap();
        assert_eq!(
            output.map(f32::to_bits),
            [
                0xC080_0000,
                0xC000_0000,
                0xC000_0000,
                0x8000_0000,
                0x8000_0000,
                0x0000_0000,
                0x7FC0_0123,
                0x0000_0000,
                0x4000_0000,
                0x4000_0000,
                0x4080_0000,
                0xCB00_0000,
                0x4B00_0000,
            ]
        );
    }

    #[test]
    fn sqrt_floor_and_all_pinned_leaky_relu_alphas_match_ort() {
        let sqrt_shape = Shape::new(&[5]).unwrap();
        let sqrt_layout = TensorLayout::contiguous(sqrt_shape);
        let mut sqrt_output = [0.0f32; 5];
        sqrt(&[-0.0, 0.0, 0.25, 2.0, 100.0], sqrt_layout, &mut sqrt_output, sqrt_layout).unwrap();
        assert_eq!(
            sqrt_output.map(f32::to_bits),
            [0x8000_0000, 0, 0x3F00_0000, 0x3FB5_04F3, 0x4120_0000]
        );

        let floor_shape = Shape::new(&[9]).unwrap();
        let floor_layout = TensorLayout::contiguous(floor_shape);
        let mut floor_output = [0.0f32; 9];
        floor(
            &[-2.5, -2.0, -1.1, -0.5, -0.0, 0.0, 0.5, 1.1, 2.5],
            floor_layout,
            &mut floor_output,
            floor_layout,
        )
        .unwrap();
        assert_eq!(
            floor_output.map(f32::to_bits),
            [
                0xC040_0000,
                0xC000_0000,
                0xC000_0000,
                0xBF80_0000,
                0x8000_0000,
                0,
                0,
                0x3F80_0000,
                0x4000_0000,
            ]
        );

        let leaky_shape = Shape::new(&[6]).unwrap();
        let leaky_layout = TensorLayout::contiguous(leaky_shape);
        let input = [-10.0, -1.0, -0.0, 0.0, 0.5, 3.0];
        let mut output = [0.0f32; 6];
        leaky_relu(&input, leaky_layout, 0.2, &mut output, leaky_layout).unwrap();
        assert_eq!(
            output.map(f32::to_bits),
            [
                0xC000_0000,
                0xBE4C_CCCD,
                0x8000_0000,
                0,
                0x3F00_0000,
                0x4040_0000,
            ]
        );
        leaky_relu(&input, leaky_layout, 0.1, &mut output, leaky_layout).unwrap();
        assert_eq!(output[1].to_bits(), (-0.1f32).to_bits());
        leaky_relu(&input, leaky_layout, 0.01, &mut output, leaky_layout).unwrap();
        assert_eq!(output[1].to_bits(), (-0.01f32).to_bits());
    }

    #[test]
    fn transcendental_unary_kernels_match_ort_fixtures() {
        let shape = Shape::new(&[8]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let trig_input = [
            -10.0,
            -core::f32::consts::PI,
            -1.0,
            -0.0,
            0.0,
            1.0,
            core::f32::consts::PI,
            10.0,
        ];
        let mut output = [0.0f32; 8];
        sin(&trig_input, layout, &mut output, layout).unwrap();
        assert_close(
            &output,
            &[
                0x3F0B_44F7,
                0x33BB_BD2E,
                0xBF57_6AA4,
                0x8000_0000,
                0,
                0x3F57_6AA4,
                0xB3BB_BD2E,
                0xBF0B_44F7,
            ],
            2.0e-7,
        );
        cos(&trig_input, layout, &mut output, layout).unwrap();
        assert_close(
            &output,
            &[
                0xBF56_CD64,
                0xBF80_0000,
                0x3F0A_5140,
                0x3F80_0000,
                0x3F80_0000,
                0x3F0A_5140,
                0xBF80_0000,
                0xBF56_CD64,
            ],
            2.0e-7,
        );

        let symmetric_input = [-10.0, -3.0, -1.0, -0.0, 0.0, 1.0, 3.0, 10.0];
        tanh(&symmetric_input, layout, &mut output, layout).unwrap();
        assert_close(
            &output,
            &[
                0xBF80_0000,
                0xBF7E_BBE8,
                0xBF42_F7D6,
                0x8000_0000,
                0,
                0x3F42_F7D6,
                0x3F7E_BBE8,
                0x3F80_0000,
            ],
            2.0e-7,
        );

        let atan_input = [-100.0, -3.0, -1.0, -0.0, 0.0, 1.0, 3.0, 100.0];
        atan(&atan_input, layout, &mut output, layout).unwrap();
        assert_close(
            &output,
            &[
                0xBFC7_C830,
                0xBF9F_E0BC,
                0xBF49_0FDA,
                0x8000_0000,
                0,
                0x3F49_0FDA,
                0x3F9F_E0BC,
                0x3FC7_C830,
            ],
            2.0e-7,
        );
    }

    #[test]
    fn sigmoid_and_exp_match_ort_fixtures() {
        let shape = Shape::new(&[8]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let mut output = [0.0f32; 8];
        sigmoid(&[-100.0, -20.0, -2.0, -0.0, 0.0, 2.0, 20.0, 100.0], layout, &mut output, layout)
            .unwrap();
        assert_close(
            &output,
            &[
                0,
                0,
                0x3DF4_20A8,
                0x3F00_0000,
                0x3F00_0000,
                0x3F61_7BEB,
                0x3F80_0000,
                0x3F80_0000,
            ],
            2.0e-7,
        );

        exp(&[-100.0, -10.0, -1.0, -0.0, 0.0, 1.0, 10.0, 80.0], layout, &mut output, layout)
            .unwrap();
        assert_close_relative(
            &output,
            &[
                0x0000_001B,
                0x383E_6BCE,
                0x3EBC_5AB2,
                0x3F80_0000,
                0x3F80_0000,
                0x402D_F854,
                0x46AC_14EF,
                0x792A_BBCE,
            ],
            2.0e-7,
        );
    }

    #[test]
    fn avx2_sin_is_bit_exact_across_medium_domain_and_reduction_boundaries() {
        if !ElementwiseLane::Avx2.is_available() {
            return;
        }

        let mut input = Vec::new();
        // Cover both ends of every coarse mantissa bucket for every exponent
        // handled by libm's medium Cody-Waite reducer, in both signs.
        for exponent in 0_u32..=0x9b {
            for mantissa_prefix in 0_u32..1024 {
                let base = (exponent << 23) | (mantissa_prefix << 13);
                for tail in [0_u32, 0x1fff] {
                    let magnitude = base | tail;
                    if magnitude < 0x7f80_0000 {
                        input.push(f32::from_bits(magnitude));
                        input.push(f32::from_bits(magnitude | 0x8000_0000));
                    }
                }
            }
        }

        // Exercise every branch transition and adjacent representable values.
        for boundary in [
            0x3980_0000_u32,
            0x3f49_0fda,
            0x4016_cbe3,
            0x407b_53d1,
            0x40af_eddf,
            0x40e2_31d5,
            0x4dc9_0fdb,
        ] {
            for delta in -64_i64..=64 {
                let magnitude = (i64::from(boundary) + delta).clamp(0, 0x7f7f_ffff) as u32;
                input.push(f32::from_bits(magnitude));
                input.push(f32::from_bits(magnitude | 0x8000_0000));
            }
        }

        // The pinned graph's largest argument is about 7,512 half-pi periods.
        // Probe each such reduction point and its immediate neighbors.
        for quadrant in -7_600_i32..=7_600 {
            let center = (quadrant as f32 * core::f32::consts::FRAC_PI_2).to_bits();
            for delta in -2_i64..=2 {
                let bits = if center >> 31 == 0 {
                    (i64::from(center) + delta).clamp(0, 0x7f7f_ffff) as u32
                } else {
                    let magnitude = center & 0x7fff_ffff;
                    ((i64::from(magnitude) + delta).clamp(0, 0x7f7f_ffff) as u32) | 0x8000_0000
                };
                input.push(f32::from_bits(bits));
            }
        }
        input.extend([
            f32::MAX,
            -f32::MAX,
            1.0e20,
            -1.0e20,
            f32::from_bits(1),
            -f32::from_bits(1),
            -0.0,
        ]);

        let shape = Shape::new(&[input.len()]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let expected: Vec<_> = input.iter().copied().map(libm::sinf).collect();
        let mut observed = vec![f32::NAN; input.len()];
        sin(&input, layout, &mut observed, layout).unwrap();
        for (index, (&observed, &expected)) in observed.iter().zip(&expected).enumerate() {
            assert_eq!(
                observed.to_bits(),
                expected.to_bits(),
                "index={index} input={:?} input_bits=0x{:08x}",
                input[index],
                input[index].to_bits(),
            );
        }
    }

    #[test]
    fn unary_domain_nonfinite_and_parameter_errors_are_transactional() {
        let shape = Shape::new(&[3]).unwrap();
        let layout = TensorLayout::contiguous(shape);
        let mut output = [123.0f32; 3];
        assert_eq!(sqrt(&[4.0, -1.0, 9.0], layout, &mut output, layout), Err(Error::DomainError));
        assert_eq!(output, [123.0; 3]);
        assert_eq!(
            exp(&[0.0, 100.0, 1.0], layout, &mut output, layout),
            Err(Error::NonFiniteOutput)
        );
        assert_eq!(output, [123.0; 3]);
        assert_eq!(
            sin(&[0.0, f32::NAN, 1.0], layout, &mut output, layout),
            Err(Error::NonFiniteInput)
        );
        assert_eq!(output, [123.0; 3]);
        assert_eq!(
            leaky_relu(&[1.0; 3], layout, f32::INFINITY, &mut output, layout),
            Err(Error::InvalidParameter)
        );
        assert_eq!(output, [123.0; 3]);
    }

    #[test]
    fn shape_and_axis_errors_are_explicit() {
        assert_eq!(PINNED_POW_SQUARE_NODES, 50);
        assert_eq!(PINNED_NODE_COVERAGE, 1_935);
        assert_eq!(Shape::new(&[1, 1, 1, 1, 1]), Err(Error::RankTooLarge));
        assert_eq!(Shape::new(&[2, 0]), Err(Error::ZeroDimension));
        let shape = Shape::new(&[2, 3]).unwrap();
        assert_eq!(reduced_shape(shape, 2, true), Err(Error::InvalidAxis));
        assert_eq!(
            broadcast_shape(Shape::new(&[2, 3]).unwrap(), Shape::new(&[4]).unwrap()),
            Err(Error::BroadcastMismatch)
        );
    }
}
