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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn shape_and_axis_errors_are_explicit() {
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
