#![no_std]
#![deny(unsafe_code)]

//! Typed copy, view, and index kernels for the sealed Kokoro graph.
//!
//! The routines here are deliberately narrower than a general tensor runtime:
//! tensors are contiguous, ranks are at most four, and every destination is
//! caller-owned. This covers the pinned graph's transpose, gather, concat,
//! split, expand, shape, and statically proven view operations without a heap
//! or a dynamic operator framework.

use core::mem::size_of;

pub const MAX_RANK: usize = 4;
pub const PINNED_TRANSPOSE_NODES: usize = 88;
pub const PINNED_GATHER_NODES: usize = 135;
pub const PINNED_CONCAT_NODES: usize = 72;
pub const PINNED_SPLIT_NODES: usize = 74;
pub const PINNED_EXPAND_NODES: usize = 5;
pub const PINNED_SHAPE_NODES: usize = 73;
pub const PINNED_STATIC_VIEW_NODES: usize = 338;
pub const PINNED_NODE_COVERAGE: usize = PINNED_TRANSPOSE_NODES
    + PINNED_GATHER_NODES
    + PINNED_CONCAT_NODES
    + PINNED_SPLIT_NODES
    + PINNED_EXPAND_NODES
    + PINNED_SHAPE_NODES
    + PINNED_STATIC_VIEW_NODES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    RankTooLarge,
    ZeroDimension,
    ShapeOverflow,
    BufferLengthMismatch,
    UnsupportedElement,
    InvalidAxis,
    DuplicateAxis,
    InvalidPermutation,
    ShapeMismatch,
    BroadcastMismatch,
    InvalidIndex,
    EmptyInputList,
    InvalidSplit,
    InvalidReshape,
    UnsupportedAllowZero,
    Aliasing,
}

/// A scalar or a non-empty, rank-one-through-rank-four shape.
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
        let mut stored = [0; MAX_RANK];
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
        self.dims().get(axis).copied()
    }

    fn normalized_axis(self, axis: isize) -> Result<usize, Error> {
        normalize_axis(axis, self.rank())
    }

    fn product(self, range: core::ops::Range<usize>) -> usize {
        let mut value = 1usize;
        for axis in range {
            value *= self.dims[axis];
        }
        value
    }
}

/// A checked immutable contiguous tensor.
#[derive(Clone, Copy, Debug)]
pub struct TensorView<'a, T> {
    data: &'a [T],
    shape: Shape,
}

impl<'a, T> TensorView<'a, T> {
    pub fn new(data: &'a [T], shape: Shape) -> Result<Self, Error> {
        validate_element::<T>()?;
        validate_buffer(data, shape)?;
        Ok(Self { data, shape })
    }

    pub const fn data(self) -> &'a [T] {
        self.data
    }

    pub const fn shape(self) -> Shape {
        self.shape
    }
}

/// Materialize a contiguous ONNX `Transpose`.
pub fn transpose<T: Copy>(
    input: &[T],
    input_shape: Shape,
    permutation: &[usize],
    output: &mut [T],
) -> Result<Shape, Error> {
    validate_element::<T>()?;
    validate_buffer(input, input_shape)?;
    if permutation.len() != input_shape.rank() {
        return Err(Error::InvalidPermutation);
    }
    let mut seen = [false; MAX_RANK];
    let mut output_dims = [0usize; MAX_RANK];
    for (output_axis, &input_axis) in permutation.iter().enumerate() {
        if input_axis >= input_shape.rank() || seen[input_axis] {
            return Err(Error::InvalidPermutation);
        }
        seen[input_axis] = true;
        output_dims[output_axis] = input_shape.dims[input_axis];
    }
    let output_shape = Shape::new(&output_dims[..input_shape.rank()])?;
    validate_buffer(output, output_shape)?;
    reject_overlap(output, input)?;

    let mut output_coordinates = [0usize; MAX_RANK];
    let mut input_coordinates = [0usize; MAX_RANK];
    for (output_index, destination) in output.iter_mut().enumerate() {
        unravel(output_index, output_shape, &mut output_coordinates);
        for output_axis in 0..output_shape.rank() {
            input_coordinates[permutation[output_axis]] = output_coordinates[output_axis];
        }
        *destination = input[linear_index(input_shape, &input_coordinates)];
    }
    Ok(output_shape)
}

/// Materialize ONNX `Gather` for contiguous rank-four-or-smaller tensors.
///
/// Negative indices are normalized against the selected data axis. Scalar
/// indices use [`Shape::scalar`].
pub fn gather<T: Copy>(
    data: &[T],
    data_shape: Shape,
    indices: &[i64],
    indices_shape: Shape,
    axis: isize,
    output: &mut [T],
) -> Result<Shape, Error> {
    validate_element::<T>()?;
    validate_buffer(data, data_shape)?;
    validate_buffer(indices, indices_shape)?;
    let axis = data_shape.normalized_axis(axis)?;
    let output_rank = data_shape
        .rank()
        .checked_sub(1)
        .and_then(|rank| rank.checked_add(indices_shape.rank()))
        .ok_or(Error::ShapeOverflow)?;
    if output_rank > MAX_RANK {
        return Err(Error::RankTooLarge);
    }
    let mut output_dims = [0usize; MAX_RANK];
    output_dims[..axis].copy_from_slice(&data_shape.dims[..axis]);
    output_dims[axis..axis + indices_shape.rank()].copy_from_slice(indices_shape.dims());
    let suffix = data_shape.rank() - axis - 1;
    output_dims[axis + indices_shape.rank()..output_rank]
        .copy_from_slice(&data_shape.dims[axis + 1..axis + 1 + suffix]);
    let output_shape = Shape::new(&output_dims[..output_rank])?;
    validate_buffer(output, output_shape)?;
    reject_overlap(output, data)?;
    reject_overlap(output, indices)?;

    let axis_len = data_shape.dims[axis] as i64;
    for &index in indices {
        if index < -axis_len || index >= axis_len {
            return Err(Error::InvalidIndex);
        }
    }

    let mut output_coordinates = [0usize; MAX_RANK];
    let mut data_coordinates = [0usize; MAX_RANK];
    let mut index_coordinates = [0usize; MAX_RANK];
    for (output_index, destination) in output.iter_mut().enumerate() {
        unravel(output_index, output_shape, &mut output_coordinates);
        data_coordinates[..axis].copy_from_slice(&output_coordinates[..axis]);
        index_coordinates[..indices_shape.rank()]
            .copy_from_slice(&output_coordinates[axis..axis + indices_shape.rank()]);
        let index_offset = linear_index(indices_shape, &index_coordinates);
        let raw_index = indices[index_offset];
        data_coordinates[axis] = if raw_index < 0 {
            (raw_index + axis_len) as usize
        } else {
            raw_index as usize
        };
        for suffix_axis in 0..suffix {
            data_coordinates[axis + 1 + suffix_axis] =
                output_coordinates[axis + indices_shape.rank() + suffix_axis];
        }
        *destination = data[linear_index(data_shape, &data_coordinates)];
    }
    Ok(output_shape)
}

/// Materialize ONNX `Concat` for a checked list of contiguous tensors.
pub fn concat<T: Copy>(
    inputs: &[TensorView<'_, T>],
    axis: isize,
    output: &mut [T],
) -> Result<Shape, Error> {
    validate_element::<T>()?;
    let first = inputs.first().ok_or(Error::EmptyInputList)?;
    let axis = first.shape.normalized_axis(axis)?;
    let mut output_dims = first.shape.dims;
    output_dims[axis] = 0;
    for input in inputs {
        if input.shape.rank() != first.shape.rank() {
            return Err(Error::ShapeMismatch);
        }
        for dimension in 0..first.shape.rank() {
            if dimension != axis && input.shape.dims[dimension] != first.shape.dims[dimension] {
                return Err(Error::ShapeMismatch);
            }
        }
        output_dims[axis] = output_dims[axis]
            .checked_add(input.shape.dims[axis])
            .ok_or(Error::ShapeOverflow)?;
        reject_overlap(output, input.data)?;
    }
    let output_shape = Shape::new(&output_dims[..first.shape.rank()])?;
    validate_buffer(output, output_shape)?;

    let outer = first.shape.product(0..axis);
    let inner = first.shape.product(axis + 1..first.shape.rank());
    let output_axis = output_shape.dims[axis];
    for outer_index in 0..outer {
        let mut axis_offset = 0usize;
        for input in inputs {
            let input_axis = input.shape.dims[axis];
            let copy_len = input_axis * inner;
            let source_start = outer_index * copy_len;
            let destination_start = (outer_index * output_axis + axis_offset) * inner;
            output[destination_start..destination_start + copy_len]
                .copy_from_slice(&input.data[source_start..source_start + copy_len]);
            axis_offset += input_axis;
        }
    }
    Ok(output_shape)
}

/// Materialize the pinned graph's two-output ONNX `Split` form.
pub fn split_two<T: Copy>(
    input: &[T],
    input_shape: Shape,
    axis: isize,
    first_axis_len: usize,
    first_output: &mut [T],
    second_output: &mut [T],
) -> Result<(Shape, Shape), Error> {
    validate_element::<T>()?;
    validate_buffer(input, input_shape)?;
    let axis = input_shape.normalized_axis(axis)?;
    let axis_len = input_shape.dims[axis];
    if first_axis_len == 0 || first_axis_len >= axis_len {
        return Err(Error::InvalidSplit);
    }
    let second_axis_len = axis_len - first_axis_len;
    let mut first_dims = input_shape.dims;
    let mut second_dims = input_shape.dims;
    first_dims[axis] = first_axis_len;
    second_dims[axis] = second_axis_len;
    let first_shape = Shape::new(&first_dims[..input_shape.rank()])?;
    let second_shape = Shape::new(&second_dims[..input_shape.rank()])?;
    validate_buffer(first_output, first_shape)?;
    validate_buffer(second_output, second_shape)?;
    reject_overlap(first_output, input)?;
    reject_overlap(second_output, input)?;
    reject_overlap(first_output, second_output)?;

    let outer = input_shape.product(0..axis);
    let inner = input_shape.product(axis + 1..input_shape.rank());
    let first_copy = first_axis_len * inner;
    let second_copy = second_axis_len * inner;
    let input_block = axis_len * inner;
    for outer_index in 0..outer {
        let source = outer_index * input_block;
        let first_destination = outer_index * first_copy;
        let second_destination = outer_index * second_copy;
        first_output[first_destination..first_destination + first_copy]
            .copy_from_slice(&input[source..source + first_copy]);
        second_output[second_destination..second_destination + second_copy]
            .copy_from_slice(&input[source + first_copy..source + input_block]);
    }
    Ok((first_shape, second_shape))
}

/// Materialize ONNX multidirectional `Expand` into a contiguous destination.
pub fn expand<T: Copy>(
    input: &[T],
    input_shape: Shape,
    target_dims: &[usize],
    output: &mut [T],
) -> Result<Shape, Error> {
    validate_element::<T>()?;
    validate_buffer(input, input_shape)?;
    let output_shape = Shape::new(target_dims)?;
    if output_shape.rank() < input_shape.rank() {
        return Err(Error::BroadcastMismatch);
    }
    let leading = output_shape.rank() - input_shape.rank();
    for input_axis in 0..input_shape.rank() {
        let input_dim = input_shape.dims[input_axis];
        let output_dim = output_shape.dims[leading + input_axis];
        if input_dim != 1 && input_dim != output_dim {
            return Err(Error::BroadcastMismatch);
        }
    }
    validate_buffer(output, output_shape)?;
    reject_overlap(output, input)?;

    let mut output_coordinates = [0usize; MAX_RANK];
    let mut input_coordinates = [0usize; MAX_RANK];
    for (output_index, destination) in output.iter_mut().enumerate() {
        unravel(output_index, output_shape, &mut output_coordinates);
        for input_axis in 0..input_shape.rank() {
            input_coordinates[input_axis] = if input_shape.dims[input_axis] == 1 {
                0
            } else {
                output_coordinates[leading + input_axis]
            };
        }
        *destination = input[linear_index(input_shape, &input_coordinates)];
    }
    Ok(output_shape)
}

/// Execute ONNX `Shape` for a statically resolved tensor descriptor.
pub fn shape_of(
    input_shape: Shape,
    start: isize,
    end: Option<isize>,
    output: &mut [i64],
) -> Result<(), Error> {
    let rank = input_shape.rank() as isize;
    let start = normalize_shape_bound(start, rank);
    let end = normalize_shape_bound(end.unwrap_or(rank), rank);
    let length = end.saturating_sub(start) as usize;
    if output.len() != length {
        return Err(Error::BufferLengthMismatch);
    }
    for (destination, axis) in output.iter_mut().zip(start..end) {
        *destination = input_shape.dims[axis as usize] as i64;
    }
    Ok(())
}

/// Validate the pinned `allowzero=0` ONNX `Reshape` control tensor.
pub fn reshape_view(
    input_shape: Shape,
    requested: &[i64],
    allowzero: bool,
) -> Result<Shape, Error> {
    if allowzero {
        return Err(Error::UnsupportedAllowZero);
    }
    if requested.len() > MAX_RANK {
        return Err(Error::RankTooLarge);
    }
    let mut dims = [0usize; MAX_RANK];
    let mut inferred = None;
    let mut known_elements = 1usize;
    for (axis, &dimension) in requested.iter().enumerate() {
        let resolved = match dimension {
            -1 => {
                if inferred.replace(axis).is_some() {
                    return Err(Error::InvalidReshape);
                }
                1
            }
            0 => input_shape.dimension(axis).ok_or(Error::InvalidReshape)?,
            1.. => usize::try_from(dimension).map_err(|_| Error::InvalidReshape)?,
            _ => return Err(Error::InvalidReshape),
        };
        dims[axis] = resolved;
        if Some(axis) != inferred {
            known_elements = known_elements
                .checked_mul(resolved)
                .ok_or(Error::ShapeOverflow)?;
        }
    }
    if let Some(axis) = inferred {
        if known_elements == 0 || !input_shape.element_count().is_multiple_of(known_elements) {
            return Err(Error::InvalidReshape);
        }
        dims[axis] = input_shape.element_count() / known_elements;
    }
    let output = Shape::new(&dims[..requested.len()])?;
    if output.element_count() != input_shape.element_count() {
        return Err(Error::InvalidReshape);
    }
    Ok(output)
}

/// Validate ONNX `Unsqueeze` axes and return its alias shape.
pub fn unsqueeze_view(input_shape: Shape, axes: &[isize]) -> Result<Shape, Error> {
    let output_rank = input_shape
        .rank()
        .checked_add(axes.len())
        .ok_or(Error::RankTooLarge)?;
    if output_rank > MAX_RANK {
        return Err(Error::RankTooLarge);
    }
    let mut inserted = [false; MAX_RANK];
    for &axis in axes {
        let axis = normalize_axis(axis, output_rank)?;
        if inserted[axis] {
            return Err(Error::DuplicateAxis);
        }
        inserted[axis] = true;
    }
    let mut dims = [0usize; MAX_RANK];
    let mut input_axis = 0usize;
    for output_axis in 0..output_rank {
        if inserted[output_axis] {
            dims[output_axis] = 1;
        } else {
            dims[output_axis] = input_shape.dims[input_axis];
            input_axis += 1;
        }
    }
    Shape::new(&dims[..output_rank])
}

/// Validate ONNX `Squeeze` axes and return its alias shape.
pub fn squeeze_view(input_shape: Shape, axes: Option<&[isize]>) -> Result<Shape, Error> {
    let mut removed = [false; MAX_RANK];
    if let Some(axes) = axes {
        for &axis in axes {
            let axis = input_shape.normalized_axis(axis)?;
            if removed[axis] {
                return Err(Error::DuplicateAxis);
            }
            if input_shape.dims[axis] != 1 {
                return Err(Error::ShapeMismatch);
            }
            removed[axis] = true;
        }
    } else {
        for (axis, removed_axis) in removed.iter_mut().enumerate().take(input_shape.rank()) {
            *removed_axis = input_shape.dims[axis] == 1;
        }
    }
    let mut dims = [0usize; MAX_RANK];
    let mut rank = 0usize;
    for (axis, &dimension) in input_shape.dims().iter().enumerate() {
        if !removed[axis] {
            dims[rank] = dimension;
            rank += 1;
        }
    }
    Shape::new(&dims[..rank])
}

fn normalize_axis(axis: isize, rank: usize) -> Result<usize, Error> {
    if rank == 0 {
        return Err(Error::InvalidAxis);
    }
    let rank = rank as isize;
    let normalized = if axis < 0 { axis + rank } else { axis };
    if (0..rank).contains(&normalized) {
        Ok(normalized as usize)
    } else {
        Err(Error::InvalidAxis)
    }
}

fn normalize_shape_bound(bound: isize, rank: isize) -> isize {
    if bound < 0 {
        (bound + rank).clamp(0, rank)
    } else {
        bound.clamp(0, rank)
    }
}

fn validate_element<T>() -> Result<(), Error> {
    if size_of::<T>() == 0 {
        Err(Error::UnsupportedElement)
    } else {
        Ok(())
    }
}

fn validate_buffer<T>(data: &[T], shape: Shape) -> Result<(), Error> {
    if data.len() == shape.element_count() {
        Ok(())
    } else {
        Err(Error::BufferLengthMismatch)
    }
}

fn reject_overlap<A, B>(output: &[A], input: &[B]) -> Result<(), Error> {
    if memory_ranges_overlap(output, input) {
        Err(Error::Aliasing)
    } else {
        Ok(())
    }
}

fn memory_ranges_overlap<A, B>(lhs: &[A], rhs: &[B]) -> bool {
    if lhs.is_empty() || rhs.is_empty() {
        return false;
    }
    let lhs_start = lhs.as_ptr() as usize;
    let rhs_start = rhs.as_ptr() as usize;
    let lhs_end = lhs_start.saturating_add(lhs.len().saturating_mul(size_of::<A>()));
    let rhs_end = rhs_start.saturating_add(rhs.len().saturating_mul(size_of::<B>()));
    lhs_start < rhs_end && rhs_start < lhs_end
}

fn unravel(mut linear: usize, shape: Shape, coordinates: &mut [usize; MAX_RANK]) {
    coordinates.fill(0);
    for axis in (0..shape.rank()).rev() {
        coordinates[axis] = linear % shape.dims[axis];
        linear /= shape.dims[axis];
    }
}

fn linear_index(shape: Shape, coordinates: &[usize; MAX_RANK]) -> usize {
    let mut linear = 0usize;
    for (axis, &coordinate) in coordinates.iter().enumerate().take(shape.rank()) {
        linear = linear * shape.dims[axis] + coordinate;
    }
    linear
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_coverage_is_explicit() {
        assert_eq!(PINNED_NODE_COVERAGE, 785);
    }

    #[test]
    fn transpose_rank_four_and_negative_gather_match_onnx_layout() {
        let input: [i32; 12] = core::array::from_fn(|index| index as i32);
        let input_shape = Shape::new(&[1, 2, 2, 3]).unwrap();
        let mut transposed = [-1_i32; 12];
        let shape = transpose(&input, input_shape, &[0, 2, 3, 1], &mut transposed).unwrap();
        assert_eq!(shape.dims(), &[1, 2, 3, 2]);
        assert_eq!(transposed, [0, 6, 1, 7, 2, 8, 3, 9, 4, 10, 5, 11]);

        let indices = [-1_i64, 0];
        let mut gathered = [-1_i32; 8];
        let shape =
            gather(&transposed, shape, &indices, Shape::new(&[2]).unwrap(), 2, &mut gathered)
                .unwrap();
        assert_eq!(shape.dims(), &[1, 2, 2, 2]);
        assert_eq!(gathered, [2, 8, 0, 6, 5, 11, 3, 9]);
    }

    #[test]
    fn gather_scalar_and_matrix_indices_cover_rank_changes() {
        let data = [10_i64, 11, 12, 20, 21, 22];
        let shape = Shape::new(&[2, 3]).unwrap();
        let mut row = [0_i64; 3];
        let output_shape = gather(&data, shape, &[-1], Shape::scalar(), 0, &mut row).unwrap();
        assert_eq!(output_shape.dims(), &[3]);
        assert_eq!(row, [20, 21, 22]);

        let mut columns = [0_i64; 8];
        let output_shape =
            gather(&data, shape, &[2, 0, 1, 1], Shape::new(&[2, 2]).unwrap(), 1, &mut columns)
                .unwrap();
        assert_eq!(output_shape.dims(), &[2, 2, 2]);
        assert_eq!(columns, [12, 10, 11, 11, 22, 20, 21, 21]);
    }

    #[test]
    fn concat_split_and_expand_are_inverse_copy_contracts() {
        let left = [1_i32, 2, 3, 4];
        let right = [5_i32, 6, 7, 8, 9, 10];
        let left = TensorView::new(&left, Shape::new(&[2, 2]).unwrap()).unwrap();
        let right = TensorView::new(&right, Shape::new(&[2, 3]).unwrap()).unwrap();
        let mut joined = [0_i32; 10];
        let joined_shape = concat(&[left, right], 1, &mut joined).unwrap();
        assert_eq!(joined_shape.dims(), &[2, 5]);
        assert_eq!(joined, [1, 2, 5, 6, 7, 3, 4, 8, 9, 10]);

        let mut first = [0_i32; 4];
        let mut second = [0_i32; 6];
        let shapes = split_two(&joined, joined_shape, -1, 2, &mut first, &mut second).unwrap();
        assert_eq!(shapes.0.dims(), &[2, 2]);
        assert_eq!(shapes.1.dims(), &[2, 3]);
        assert_eq!(first, [1, 2, 3, 4]);
        assert_eq!(second, [5, 6, 7, 8, 9, 10]);

        let scalar_rows = [7_i32, 8];
        let mut expanded = [0_i32; 12];
        let expanded_shape =
            expand(&scalar_rows, Shape::new(&[2, 1]).unwrap(), &[3, 2, 2], &mut expanded).unwrap();
        assert_eq!(expanded_shape.dims(), &[3, 2, 2]);
        assert_eq!(expanded, [7, 7, 8, 8, 7, 7, 8, 8, 7, 7, 8, 8]);
    }

    #[test]
    fn shape_and_static_view_controls_are_checked() {
        let input = Shape::new(&[2, 3, 4]).unwrap();
        let mut dimensions = [0_i64; 2];
        shape_of(input, 1, None, &mut dimensions).unwrap();
        assert_eq!(dimensions, [3, 4]);

        assert_eq!(reshape_view(input, &[0, -1], false).unwrap().dims(), &[2, 12]);
        let unsqueezed = unsqueeze_view(input, &[0]).unwrap();
        assert_eq!(unsqueezed.dims(), &[1, 2, 3, 4]);
        assert_eq!(squeeze_view(unsqueezed, Some(&[0])).unwrap().dims(), input.dims());
        assert_eq!(
            squeeze_view(Shape::new(&[1, 2, 1]).unwrap(), None)
                .unwrap()
                .dims(),
            &[2]
        );
    }

    #[test]
    fn all_rejections_precede_destination_writes() {
        let data = [1_i32, 2, 3, 4];
        let shape = Shape::new(&[2, 2]).unwrap();
        let mut output = [99_i32; 2];
        assert_eq!(
            gather(&data, shape, &[2], Shape::scalar(), 0, &mut output),
            Err(Error::InvalidIndex)
        );
        assert_eq!(output, [99, 99]);

        assert_eq!(
            transpose(&data, shape, &[0, 0], &mut [88_i32; 4]),
            Err(Error::InvalidPermutation)
        );
        assert_eq!(reshape_view(shape, &[-1, -1], false), Err(Error::InvalidReshape));
        assert_eq!(unsqueeze_view(shape, &[0, 0]), Err(Error::DuplicateAxis));
    }
}
