#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! Allocation-free f32 kernels for the sealed Kokoro graph.
//!
//! The pinned graph uses rank-four-or-smaller tensors for 1,444 f32 binary
//! elementwise nodes, 130 `ReduceMean`, 19 `LayerNormalization`, 12 `Softmax`,
//! 12 `FastGelu`, and 12 `SkipLayerNormalization` nodes. This crate keeps the
//! ONNX operation boundaries explicit and requires caller-owned storage.

use core::mem::size_of;

pub const MAX_RANK: usize = 4;

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
    ParameterTooSmall,
    NonFiniteInput,
    NonFiniteOutput,
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
}

/// Compute ONNX multidirectional-broadcast output shape.
pub fn broadcast_shape(lhs: Shape, rhs: Shape) -> Result<Shape, Error> {
    let rank = lhs.rank().max(rhs.rank());
    let mut output = [1usize; MAX_RANK];
    for output_axis in 0..rank {
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
        output[output_axis] = lhs_dimension.max(rhs_dimension);
    }
    Shape::new(&output[..rank])
}

#[derive(Clone, Copy)]
enum BinaryOperation {
    Add,
    Mul,
    Div,
    Sub,
}

impl BinaryOperation {
    fn apply(self, lhs: f32, rhs: f32) -> f32 {
        match self {
            Self::Add => lhs + rhs,
            Self::Mul => lhs * rhs,
            Self::Div => lhs / rhs,
            Self::Sub => lhs - rhs,
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
    binary_elementwise(
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
    binary_elementwise(
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
    binary_elementwise(
        BinaryOperation::Div,
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
    binary_elementwise(
        BinaryOperation::Sub,
        lhs,
        lhs_layout,
        rhs,
        rhs_layout,
        output,
        output_layout,
    )
}

fn binary_elementwise(
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
        if !operation.apply(lhs_value, rhs_value).is_finite() {
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
