#![no_std]
#![deny(unsafe_code)]

//! Scalar, comparison, cast, and control-tensor operations used by Kokoro.
//!
//! This is not a general ONNX interpreter. It provides the exact dtype set and
//! rank-four multidirectional broadcasting admitted by the sealed model. All
//! fallible validation happens before caller-owned output is modified.

use core::mem::size_of;

use trueos_kokoro_layout::{MAX_RANK, Shape};

pub const PINNED_CAST_NODES: usize = 333;
pub const PINNED_RANGE_NODES: usize = 2;
pub const PINNED_CUM_SUM_NODES: usize = 2;
pub const PINNED_EQUAL_NODES: usize = 4;
pub const PINNED_GREATER_NODES: usize = 3;
pub const PINNED_GREATER_OR_EQUAL_NODES: usize = 1;
pub const PINNED_LESS_NODES: usize = 2;
pub const PINNED_AND_NODES: usize = 1;
pub const PINNED_WHERE_NODES: usize = 7;
pub const PINNED_CONSTANT_OF_SHAPE_NODES: usize = 6;
pub const PINNED_DEQUANTIZE_LINEAR_NODES: usize = 4;
pub const PINNED_INT64_ADD_NODES: usize = 1;
pub const PINNED_OPERATOR_NODES: usize = PINNED_CAST_NODES
    + PINNED_RANGE_NODES
    + PINNED_CUM_SUM_NODES
    + PINNED_EQUAL_NODES
    + PINNED_GREATER_NODES
    + PINNED_GREATER_OR_EQUAL_NODES
    + PINNED_LESS_NODES
    + PINNED_AND_NODES
    + PINNED_WHERE_NODES
    + PINNED_CONSTANT_OF_SHAPE_NODES
    + PINNED_DEQUANTIZE_LINEAR_NODES
    + PINNED_INT64_ADD_NODES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BufferLengthMismatch,
    ShapeOverflow,
    BroadcastMismatch,
    OutputShapeMismatch,
    InvalidAxis,
    InvalidRange,
    InvalidShape,
    Aliasing,
    NonFiniteInput,
    NonFiniteOutput,
    InvalidScale,
    CastOutOfRange,
    IntegerOverflow,
}

#[derive(Clone, Copy)]
enum Comparison {
    Equal,
    Greater,
    GreaterOrEqual,
    Less,
}

/// ONNX `Cast` from INT32 to FLOAT.
pub fn cast_i32_to_f32(input: &[i32], output: &mut [f32]) -> Result<(), Error> {
    validate_same_len(input, output)?;
    reject_overlap(output, input)?;
    for (destination, &value) in output.iter_mut().zip(input) {
        *destination = value as f32;
    }
    Ok(())
}

/// ONNX `Cast` from INT64 to FLOAT.
pub fn cast_i64_to_f32(input: &[i64], output: &mut [f32]) -> Result<(), Error> {
    validate_same_len(input, output)?;
    reject_overlap(output, input)?;
    for (destination, &value) in output.iter_mut().zip(input) {
        *destination = value as f32;
    }
    Ok(())
}

/// ONNX `Cast` from BOOL to FLOAT.
pub fn cast_bool_to_f32(input: &[bool], output: &mut [f32]) -> Result<(), Error> {
    validate_same_len(input, output)?;
    reject_overlap(output, input)?;
    for (destination, &value) in output.iter_mut().zip(input) {
        *destination = if value { 1.0 } else { 0.0 };
    }
    Ok(())
}

/// ONNX `Cast` from FLOAT to BOOL for finite model values.
pub fn cast_f32_to_bool(input: &[f32], output: &mut [bool]) -> Result<(), Error> {
    validate_same_len(input, output)?;
    reject_overlap(output, input)?;
    validate_finite(input)?;
    for (destination, &value) in output.iter_mut().zip(input) {
        *destination = value != 0.0;
    }
    Ok(())
}

/// ONNX `Cast` from FLOAT to INT32, truncating toward zero.
pub fn cast_f32_to_i32(input: &[f32], output: &mut [i32]) -> Result<(), Error> {
    validate_same_len(input, output)?;
    reject_overlap(output, input)?;
    for &value in input {
        if !value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        if !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
            return Err(Error::CastOutOfRange);
        }
    }
    for (destination, &value) in output.iter_mut().zip(input) {
        *destination = value as i32;
    }
    Ok(())
}

/// ONNX `Cast` from FLOAT to INT64, truncating toward zero.
pub fn cast_f32_to_i64(input: &[f32], output: &mut [i64]) -> Result<(), Error> {
    validate_same_len(input, output)?;
    reject_overlap(output, input)?;
    for &value in input {
        if !value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) {
            return Err(Error::CastOutOfRange);
        }
    }
    for (destination, &value) in output.iter_mut().zip(input) {
        *destination = value as i64;
    }
    Ok(())
}

/// ONNX INT64 `Range` with an exact caller-sized destination.
pub fn range_i64(start: i64, limit: i64, delta: i64, output: &mut [i64]) -> Result<(), Error> {
    let count = range_count(start, limit, delta)?;
    if output.len() != count {
        return Err(Error::BufferLengthMismatch);
    }
    for index in 0..count {
        let value = i128::from(start) + index as i128 * i128::from(delta);
        i64::try_from(value).map_err(|_| Error::IntegerOverflow)?;
    }
    for (index, destination) in output.iter_mut().enumerate() {
        let value = i128::from(start) + index as i128 * i128::from(delta);
        *destination = value as i64;
    }
    Ok(())
}

pub fn range_count(start: i64, limit: i64, delta: i64) -> Result<usize, Error> {
    if delta == 0 {
        return Err(Error::InvalidRange);
    }
    let start = i128::from(start);
    let limit = i128::from(limit);
    let delta = i128::from(delta);
    let count = if delta > 0 {
        if start >= limit {
            0
        } else {
            (limit - start + delta - 1) / delta
        }
    } else if start <= limit {
        0
    } else {
        let magnitude = -delta;
        (start - limit + magnitude - 1) / magnitude
    };
    usize::try_from(count).map_err(|_| Error::ShapeOverflow)
}

/// ONNX `CumSum` for a contiguous INT64 tensor and one static axis.
pub fn cumulative_sum_i64(
    input: &[i64],
    shape: Shape,
    axis: isize,
    output: &mut [i64],
) -> Result<(), Error> {
    validate_shape_buffers(input, output, shape)?;
    reject_overlap(output, input)?;
    let axis = normalize_axis(shape, axis)?;
    let (outer, axis_len, inner) = axis_geometry(shape, axis);
    for outer_index in 0..outer {
        for inner_index in 0..inner {
            let mut sum = 0_i64;
            for axis_index in 0..axis_len {
                let index = (outer_index * axis_len + axis_index) * inner + inner_index;
                sum = sum
                    .checked_add(input[index])
                    .ok_or(Error::IntegerOverflow)?;
            }
        }
    }
    for outer_index in 0..outer {
        for inner_index in 0..inner {
            let mut sum = 0_i64;
            for axis_index in 0..axis_len {
                let index = (outer_index * axis_len + axis_index) * inner + inner_index;
                sum += input[index];
                output[index] = sum;
            }
        }
    }
    Ok(())
}

/// ONNX `CumSum` for a contiguous FLOAT tensor and one static axis.
pub fn cumulative_sum_f32(
    input: &[f32],
    shape: Shape,
    axis: isize,
    output: &mut [f32],
) -> Result<(), Error> {
    validate_shape_buffers(input, output, shape)?;
    reject_overlap(output, input)?;
    let axis = normalize_axis(shape, axis)?;
    let (outer, axis_len, inner) = axis_geometry(shape, axis);
    for outer_index in 0..outer {
        for inner_index in 0..inner {
            let mut sum = 0.0_f32;
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
        }
    }
    for outer_index in 0..outer {
        for inner_index in 0..inner {
            let mut sum = 0.0_f32;
            for axis_index in 0..axis_len {
                let index = (outer_index * axis_len + axis_index) * inner + inner_index;
                sum += input[index];
                output[index] = sum;
            }
        }
    }
    Ok(())
}

pub fn equal_i64(
    lhs: &[i64],
    lhs_shape: Shape,
    rhs: &[i64],
    rhs_shape: Shape,
    output: &mut [bool],
) -> Result<Shape, Error> {
    compare_i64(Comparison::Equal, lhs, lhs_shape, rhs, rhs_shape, output)
}

/// ONNX INT64 `Less` with rank-four multidirectional broadcasting.
pub fn less_i64(
    lhs: &[i64],
    lhs_shape: Shape,
    rhs: &[i64],
    rhs_shape: Shape,
    output: &mut [bool],
) -> Result<Shape, Error> {
    compare_i64(Comparison::Less, lhs, lhs_shape, rhs, rhs_shape, output)
}

pub fn greater_f32(
    lhs: &[f32],
    lhs_shape: Shape,
    rhs: &[f32],
    rhs_shape: Shape,
    output: &mut [bool],
) -> Result<Shape, Error> {
    compare_f32(Comparison::Greater, lhs, lhs_shape, rhs, rhs_shape, output)
}

pub fn greater_or_equal_f32(
    lhs: &[f32],
    lhs_shape: Shape,
    rhs: &[f32],
    rhs_shape: Shape,
    output: &mut [bool],
) -> Result<Shape, Error> {
    compare_f32(Comparison::GreaterOrEqual, lhs, lhs_shape, rhs, rhs_shape, output)
}

/// ONNX FLOAT `Less` with rank-four multidirectional broadcasting.
pub fn less_f32(
    lhs: &[f32],
    lhs_shape: Shape,
    rhs: &[f32],
    rhs_shape: Shape,
    output: &mut [bool],
) -> Result<Shape, Error> {
    compare_f32(Comparison::Less, lhs, lhs_shape, rhs, rhs_shape, output)
}

/// The remaining pinned INT64 `Add`, with multidirectional broadcasting.
///
/// Every sum is proved representable before output is modified. This is the
/// index-grid addition feeding the generator ISTFT ScatterND lane.
pub fn add_i64(
    lhs: &[i64],
    lhs_shape: Shape,
    rhs: &[i64],
    rhs_shape: Shape,
    output: &mut [i64],
) -> Result<Shape, Error> {
    validate_buffer(lhs, lhs_shape)?;
    validate_buffer(rhs, rhs_shape)?;
    let output_shape = broadcast_shape(lhs_shape, rhs_shape)?;
    validate_buffer(output, output_shape)?;
    reject_overlap(output, lhs)?;
    reject_overlap(output, rhs)?;

    for index in 0..output_shape.element_count() {
        let lhs = lhs[broadcast_offset(index, output_shape, lhs_shape)];
        let rhs = rhs[broadcast_offset(index, output_shape, rhs_shape)];
        lhs.checked_add(rhs).ok_or(Error::IntegerOverflow)?;
    }
    for (index, destination) in output.iter_mut().enumerate() {
        let lhs = lhs[broadcast_offset(index, output_shape, lhs_shape)];
        let rhs = rhs[broadcast_offset(index, output_shape, rhs_shape)];
        *destination = lhs.wrapping_add(rhs);
    }
    Ok(output_shape)
}

/// ONNX INT8 `DequantizeLinear` with scalar scale and scalar zero point.
///
/// The four pinned embedding nodes use rank-three inputs and zero point zero;
/// accepting any checked rank-four shape and scalar i8 zero point keeps the
/// primitive faithful to the ONNX operation without introducing per-axis
/// quantization that the graph never uses.
pub fn dequantize_linear_i8_scalar(
    input: &[i8],
    shape: Shape,
    scale: f32,
    zero_point: i8,
    output: &mut [f32],
) -> Result<Shape, Error> {
    validate_buffer(input, shape)?;
    validate_buffer(output, shape)?;
    reject_overlap(output, input)?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(Error::InvalidScale);
    }
    for &value in input {
        let centered = i32::from(value) - i32::from(zero_point);
        let dequantized = centered as f32 * scale;
        if !dequantized.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    for (destination, &value) in output.iter_mut().zip(input) {
        let centered = i32::from(value) - i32::from(zero_point);
        *destination = centered as f32 * scale;
    }
    Ok(shape)
}

pub fn and_bool(
    lhs: &[bool],
    lhs_shape: Shape,
    rhs: &[bool],
    rhs_shape: Shape,
    output: &mut [bool],
) -> Result<Shape, Error> {
    validate_buffer(lhs, lhs_shape)?;
    validate_buffer(rhs, rhs_shape)?;
    let output_shape = broadcast_shape(lhs_shape, rhs_shape)?;
    validate_buffer(output, output_shape)?;
    reject_overlap(output, lhs)?;
    reject_overlap(output, rhs)?;
    for (index, destination) in output.iter_mut().enumerate() {
        *destination = lhs[broadcast_offset(index, output_shape, lhs_shape)]
            && rhs[broadcast_offset(index, output_shape, rhs_shape)];
    }
    Ok(output_shape)
}

pub fn where_i64(
    condition: &[bool],
    condition_shape: Shape,
    when_true: &[i64],
    true_shape: Shape,
    when_false: &[i64],
    false_shape: Shape,
    output: &mut [i64],
) -> Result<Shape, Error> {
    where_copy(condition, condition_shape, when_true, true_shape, when_false, false_shape, output)
}

pub fn where_f32(
    condition: &[bool],
    condition_shape: Shape,
    when_true: &[f32],
    true_shape: Shape,
    when_false: &[f32],
    false_shape: Shape,
    output: &mut [f32],
) -> Result<Shape, Error> {
    validate_finite(when_true)?;
    validate_finite(when_false)?;
    where_copy(condition, condition_shape, when_true, true_shape, when_false, false_shape, output)
}

/// Pinned FLOAT `ConstantOfShape` with a scalar fill value.
pub fn constant_of_shape_f32(
    dimensions: &[i64],
    value: f32,
    output: &mut [f32],
) -> Result<Shape, Error> {
    if dimensions.len() > MAX_RANK || !value.is_finite() {
        return Err(Error::InvalidShape);
    }
    let mut dims = [0usize; MAX_RANK];
    for (axis, &dimension) in dimensions.iter().enumerate() {
        if dimension <= 0 {
            return Err(Error::InvalidShape);
        }
        dims[axis] = usize::try_from(dimension).map_err(|_| Error::InvalidShape)?;
    }
    let shape = Shape::new(&dims[..dimensions.len()]).map_err(|_| Error::InvalidShape)?;
    if output.len() != shape.element_count() {
        return Err(Error::BufferLengthMismatch);
    }
    output.fill(value);
    Ok(shape)
}

fn compare_i64(
    operation: Comparison,
    lhs: &[i64],
    lhs_shape: Shape,
    rhs: &[i64],
    rhs_shape: Shape,
    output: &mut [bool],
) -> Result<Shape, Error> {
    validate_buffer(lhs, lhs_shape)?;
    validate_buffer(rhs, rhs_shape)?;
    let output_shape = broadcast_shape(lhs_shape, rhs_shape)?;
    validate_buffer(output, output_shape)?;
    reject_overlap(output, lhs)?;
    reject_overlap(output, rhs)?;
    for (index, destination) in output.iter_mut().enumerate() {
        let lhs = lhs[broadcast_offset(index, output_shape, lhs_shape)];
        let rhs = rhs[broadcast_offset(index, output_shape, rhs_shape)];
        *destination = match operation {
            Comparison::Equal => lhs == rhs,
            Comparison::Less => lhs < rhs,
            Comparison::Greater => lhs > rhs,
            Comparison::GreaterOrEqual => lhs >= rhs,
        };
    }
    Ok(output_shape)
}

fn compare_f32(
    operation: Comparison,
    lhs: &[f32],
    lhs_shape: Shape,
    rhs: &[f32],
    rhs_shape: Shape,
    output: &mut [bool],
) -> Result<Shape, Error> {
    validate_buffer(lhs, lhs_shape)?;
    validate_buffer(rhs, rhs_shape)?;
    validate_finite(lhs)?;
    validate_finite(rhs)?;
    let output_shape = broadcast_shape(lhs_shape, rhs_shape)?;
    validate_buffer(output, output_shape)?;
    reject_overlap(output, lhs)?;
    reject_overlap(output, rhs)?;
    for (index, destination) in output.iter_mut().enumerate() {
        let lhs = lhs[broadcast_offset(index, output_shape, lhs_shape)];
        let rhs = rhs[broadcast_offset(index, output_shape, rhs_shape)];
        *destination = match operation {
            Comparison::Greater => lhs > rhs,
            Comparison::GreaterOrEqual => lhs >= rhs,
            Comparison::Equal => lhs == rhs,
            Comparison::Less => lhs < rhs,
        };
    }
    Ok(output_shape)
}

#[allow(clippy::too_many_arguments)]
fn where_copy<T: Copy>(
    condition: &[bool],
    condition_shape: Shape,
    when_true: &[T],
    true_shape: Shape,
    when_false: &[T],
    false_shape: Shape,
    output: &mut [T],
) -> Result<Shape, Error> {
    validate_buffer(condition, condition_shape)?;
    validate_buffer(when_true, true_shape)?;
    validate_buffer(when_false, false_shape)?;
    let values_shape = broadcast_shape(true_shape, false_shape)?;
    let output_shape = broadcast_shape(condition_shape, values_shape)?;
    validate_buffer(output, output_shape)?;
    reject_overlap(output, condition)?;
    reject_overlap(output, when_true)?;
    reject_overlap(output, when_false)?;
    for (index, destination) in output.iter_mut().enumerate() {
        let condition = condition[broadcast_offset(index, output_shape, condition_shape)];
        *destination = if condition {
            when_true[broadcast_offset(index, output_shape, true_shape)]
        } else {
            when_false[broadcast_offset(index, output_shape, false_shape)]
        };
    }
    Ok(output_shape)
}

fn broadcast_shape(lhs: Shape, rhs: Shape) -> Result<Shape, Error> {
    let rank = lhs.rank().max(rhs.rank());
    let mut dims = [0usize; MAX_RANK];
    for reverse_axis in 0..rank {
        let lhs_dim = trailing_dimension(lhs, reverse_axis);
        let rhs_dim = trailing_dimension(rhs, reverse_axis);
        if lhs_dim != rhs_dim && lhs_dim != 1 && rhs_dim != 1 {
            return Err(Error::BroadcastMismatch);
        }
        dims[rank - 1 - reverse_axis] = lhs_dim.max(rhs_dim);
    }
    Shape::new(&dims[..rank]).map_err(|_| Error::ShapeOverflow)
}

fn trailing_dimension(shape: Shape, reverse_axis: usize) -> usize {
    if reverse_axis < shape.rank() {
        shape.dims()[shape.rank() - 1 - reverse_axis]
    } else {
        1
    }
}

fn broadcast_offset(output_index: usize, output_shape: Shape, input_shape: Shape) -> usize {
    if input_shape.rank() == 0 {
        return 0;
    }
    let mut output_coordinates = [0usize; MAX_RANK];
    let mut remaining = output_index;
    for axis in (0..output_shape.rank()).rev() {
        output_coordinates[axis] = remaining % output_shape.dims()[axis];
        remaining /= output_shape.dims()[axis];
    }
    let leading = output_shape.rank() - input_shape.rank();
    let mut input_offset = 0usize;
    for input_axis in 0..input_shape.rank() {
        let dimension = input_shape.dims()[input_axis];
        let coordinate = if dimension == 1 {
            0
        } else {
            output_coordinates[leading + input_axis]
        };
        input_offset = input_offset * dimension + coordinate;
    }
    input_offset
}

fn normalize_axis(shape: Shape, axis: isize) -> Result<usize, Error> {
    let rank = shape.rank() as isize;
    let axis = if axis < 0 { axis + rank } else { axis };
    if (0..rank).contains(&axis) {
        Ok(axis as usize)
    } else {
        Err(Error::InvalidAxis)
    }
}

fn axis_geometry(shape: Shape, axis: usize) -> (usize, usize, usize) {
    let outer = shape.dims()[..axis].iter().product();
    let axis_len = shape.dims()[axis];
    let inner = shape.dims()[axis + 1..].iter().product();
    (outer, axis_len, inner)
}

fn validate_finite(values: &[f32]) -> Result<(), Error> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(Error::NonFiniteInput)
    }
}

fn validate_shape_buffers<T, U>(input: &[T], output: &[U], shape: Shape) -> Result<(), Error> {
    validate_buffer(input, shape)?;
    validate_buffer(output, shape)
}

fn validate_buffer<T>(buffer: &[T], shape: Shape) -> Result<(), Error> {
    if buffer.len() == shape.element_count() {
        Ok(())
    } else {
        Err(Error::BufferLengthMismatch)
    }
}

fn validate_same_len<T, U>(lhs: &[T], rhs: &[U]) -> Result<(), Error> {
    if lhs.len() == rhs.len() {
        Ok(())
    } else {
        Err(Error::BufferLengthMismatch)
    }
}

fn reject_overlap<A, B>(lhs: &[A], rhs: &[B]) -> Result<(), Error> {
    let lhs_start = lhs.as_ptr() as usize;
    let rhs_start = rhs.as_ptr() as usize;
    let lhs_end = lhs_start.saturating_add(lhs.len().saturating_mul(size_of::<A>()));
    let rhs_end = rhs_start.saturating_add(rhs.len().saturating_mul(size_of::<B>()));
    if lhs_start < rhs_end && rhs_start < lhs_end {
        Err(Error::Aliasing)
    } else {
        Ok(())
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casts_match_onnx_truncation_and_boolean_rules() {
        let input = [-2.9_f32, -0.0, 0.9, 7.1];
        let mut i32_output = [0_i32; 4];
        cast_f32_to_i32(&input, &mut i32_output).unwrap();
        assert_eq!(i32_output, [-2, 0, 0, 7]);
        let mut i64_output = [0_i64; 4];
        cast_f32_to_i64(&input, &mut i64_output).unwrap();
        assert_eq!(i64_output, [-2, 0, 0, 7]);
        let mut bool_output = [false; 4];
        cast_f32_to_bool(&input, &mut bool_output).unwrap();
        assert_eq!(bool_output, [true, false, true, true]);

        let snapshot = i32_output;
        assert_eq!(cast_f32_to_i32(&[f32::NAN; 4], &mut i32_output), Err(Error::NonFiniteInput));
        assert_eq!(i32_output, snapshot);
    }

    #[test]
    fn range_and_cumulative_sum_cover_both_graph_dtypes() {
        let mut range = [0_i64; 4];
        range_i64(-2, 6, 2, &mut range).unwrap();
        assert_eq!(range, [-2, 0, 2, 4]);
        let mut reverse = [0_i64; 3];
        range_i64(3, -3, -2, &mut reverse).unwrap();
        assert_eq!(reverse, [3, 1, -1]);

        let shape = Shape::new(&[2, 3]).unwrap();
        let mut ints = [0_i64; 6];
        cumulative_sum_i64(&[1, 2, 3, 4, 5, 6], shape, 1, &mut ints).unwrap();
        assert_eq!(ints, [1, 3, 6, 4, 9, 15]);
        let mut floats = [0.0_f32; 6];
        cumulative_sum_f32(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], shape, 0, &mut floats).unwrap();
        assert_eq!(floats, [1.0, 2.0, 3.0, 5.0, 7.0, 9.0]);
    }

    #[test]
    fn comparison_and_where_broadcast_like_onnx() {
        let matrix = Shape::new(&[2, 3]).unwrap();
        let scalar = Shape::scalar();
        let values = [-1.0_f32, 2.0, 3.0, 0.0, 5.0, -6.0];
        let mut greater = [false; 6];
        let shape = greater_f32(&values, matrix, &[1.0], scalar, &mut greater).unwrap();
        assert_eq!(shape, matrix);
        assert_eq!(greater, [false, true, true, false, true, false]);

        let mut selected = [0.0_f32; 6];
        where_f32(&greater, matrix, &[10.0], scalar, &values, matrix, &mut selected).unwrap();
        assert_eq!(selected, [-1.0, 10.0, 10.0, 0.0, 10.0, -6.0]);
    }

    #[test]
    fn constant_shape_and_errors_are_transactional() {
        let mut output = [9.0_f32; 6];
        let shape = constant_of_shape_f32(&[2, 3], 1.0, &mut output).unwrap();
        assert_eq!(shape.dims(), &[2, 3]);
        assert_eq!(output, [1.0; 6]);
        let snapshot = output;
        assert_eq!(constant_of_shape_f32(&[2, 0], 1.0, &mut output), Err(Error::InvalidShape));
        assert_eq!(output, snapshot);
    }
}
