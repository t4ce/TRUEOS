#![cfg_attr(not(test), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Allocation-free float Conv1d and ConvTranspose1d for the pinned Kokoro graph.
//!
//! The graph contains one Conv1d and six ConvTranspose1d nodes in six exact
//! parameter families. Tensors use contiguous NCW layout with batch one.
//! ConvTranspose weights retain ONNX layout `[C_in, C_out / group, K]`; Conv
//! weights use `[C_out, C_in / group, K]`. Every dot product visits input
//! channels and then kernel taps in ascending order. The AVX2+FMA lane only
//! evaluates eight adjacent output times in parallel, preserving that order.

pub const PINNED_NODE_COVERAGE: usize = 7;
pub const SIMD_TIME_TILE: usize = 8;
pub const DEFAULT_CHANNEL_TILE: usize = 8;
pub const DEFAULT_TIME_TILE: usize = 64;
const MAX_TRANSPOSE_TAP_LAYERS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    EmptyInput,
    ShapeOverflow,
    InputLengthMismatch,
    WeightLengthMismatch,
    BiasLengthMismatch,
    MissingBias,
    UnexpectedBias,
    OutputLengthMismatch,
    EmptyTile,
    TileOutOfBounds,
    Aliasing,
    NonFiniteInput,
    NonFiniteOutputRisk,
    UnsupportedLane,
    AvxIndexTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorKind {
    Conv,
    ConvTranspose,
}

/// The six exact parameter families represented by seven pinned graph nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    /// Encoder F0 and N pools: two depthwise `[512, 1, 3]` operators.
    EncoderDepthwise512 { input_width: usize },
    /// Decoder pool: one depthwise `[1090, 1, 3]` operator.
    DecoderDepthwise1090 { input_width: usize },
    /// Generator upsample zero, ONNX weights `[512, 256, 20]`.
    Upsample512To256 { input_width: usize },
    /// Generator upsample one, ONNX weights `[256, 128, 12]`.
    Upsample256To128 { input_width: usize },
    /// Generator post convolution, weights `[22, 128, 7]`.
    PostConv128To22 { input_width: usize },
    /// ISTFT synthesis convolution, weights `[22, 1, 20]`, no bias.
    Istft22To1 { input_width: usize },
}

impl Profile {
    pub const fn input_width(self) -> usize {
        match self {
            Self::EncoderDepthwise512 { input_width }
            | Self::DecoderDepthwise1090 { input_width }
            | Self::Upsample512To256 { input_width }
            | Self::Upsample256To128 { input_width }
            | Self::PostConv128To22 { input_width }
            | Self::Istft22To1 { input_width } => input_width,
        }
    }

    pub const fn graph_nodes(self) -> usize {
        match self {
            Self::EncoderDepthwise512 { .. } => 2,
            _ => 1,
        }
    }

    pub const fn parameters(self) -> Parameters {
        match self {
            Self::EncoderDepthwise512 { .. } => Parameters {
                kind: OperatorKind::ConvTranspose,
                input_channels: 512,
                output_channels: 512,
                kernel_width: 3,
                stride: 2,
                pad_left: 1,
                pad_right: 1,
                output_padding: 1,
                groups: 512,
                has_bias: true,
            },
            Self::DecoderDepthwise1090 { .. } => Parameters {
                kind: OperatorKind::ConvTranspose,
                input_channels: 1_090,
                output_channels: 1_090,
                kernel_width: 3,
                stride: 2,
                pad_left: 1,
                pad_right: 1,
                output_padding: 1,
                groups: 1_090,
                has_bias: true,
            },
            Self::Upsample512To256 { .. } => Parameters {
                kind: OperatorKind::ConvTranspose,
                input_channels: 512,
                output_channels: 256,
                kernel_width: 20,
                stride: 10,
                pad_left: 5,
                pad_right: 5,
                output_padding: 0,
                groups: 1,
                has_bias: true,
            },
            Self::Upsample256To128 { .. } => Parameters {
                kind: OperatorKind::ConvTranspose,
                input_channels: 256,
                output_channels: 128,
                kernel_width: 12,
                stride: 6,
                pad_left: 3,
                pad_right: 3,
                output_padding: 0,
                groups: 1,
                has_bias: true,
            },
            Self::PostConv128To22 { .. } => Parameters {
                kind: OperatorKind::Conv,
                input_channels: 128,
                output_channels: 22,
                kernel_width: 7,
                stride: 1,
                pad_left: 3,
                pad_right: 3,
                output_padding: 0,
                groups: 1,
                has_bias: true,
            },
            Self::Istft22To1 { .. } => Parameters {
                kind: OperatorKind::ConvTranspose,
                input_channels: 22,
                output_channels: 1,
                kernel_width: 20,
                stride: 5,
                pad_left: 0,
                pad_right: 0,
                output_padding: 0,
                groups: 1,
                has_bias: false,
            },
        }
    }

    pub fn dimensions(self) -> Result<Dimensions, Error> {
        let input_width = self.input_width();
        if input_width == 0 {
            return Err(Error::EmptyInput);
        }
        let parameters = self.parameters();
        let output_width = match parameters.kind {
            OperatorKind::Conv => {
                let padded = input_width
                    .checked_add(parameters.pad_left)
                    .and_then(|value| value.checked_add(parameters.pad_right))
                    .ok_or(Error::ShapeOverflow)?;
                if padded < parameters.kernel_width {
                    return Err(Error::EmptyInput);
                }
                (padded - parameters.kernel_width) / parameters.stride + 1
            }
            OperatorKind::ConvTranspose => {
                let expanded = (input_width - 1)
                    .checked_mul(parameters.stride)
                    .and_then(|value| value.checked_add(parameters.kernel_width))
                    .and_then(|value| value.checked_add(parameters.output_padding))
                    .ok_or(Error::ShapeOverflow)?;
                expanded
                    .checked_sub(parameters.pad_left + parameters.pad_right)
                    .ok_or(Error::EmptyInput)?
            }
        };
        if output_width == 0 {
            return Err(Error::EmptyInput);
        }
        let dimensions = Dimensions {
            input_channels: parameters.input_channels,
            output_channels: parameters.output_channels,
            input_width,
            output_width,
            kernel_width: parameters.kernel_width,
            stride: parameters.stride,
            pad_left: parameters.pad_left,
            pad_right: parameters.pad_right,
            output_padding: parameters.output_padding,
            groups: parameters.groups,
        };
        // Prove all address arithmetic before an invocation is admitted.
        dimensions.input_elements()?;
        dimensions.weight_elements(parameters.kind)?;
        dimensions.output_elements()?;
        Ok(dimensions)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Parameters {
    pub kind: OperatorKind,
    pub input_channels: usize,
    pub output_channels: usize,
    pub kernel_width: usize,
    pub stride: usize,
    pub pad_left: usize,
    pub pad_right: usize,
    pub output_padding: usize,
    pub groups: usize,
    pub has_bias: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dimensions {
    pub input_channels: usize,
    pub output_channels: usize,
    pub input_width: usize,
    pub output_width: usize,
    pub kernel_width: usize,
    pub stride: usize,
    pub pad_left: usize,
    pub pad_right: usize,
    pub output_padding: usize,
    pub groups: usize,
}

impl Dimensions {
    pub fn input_elements(self) -> Result<usize, Error> {
        self.input_channels
            .checked_mul(self.input_width)
            .ok_or(Error::ShapeOverflow)
    }

    pub fn output_elements(self) -> Result<usize, Error> {
        self.output_channels
            .checked_mul(self.output_width)
            .ok_or(Error::ShapeOverflow)
    }

    pub fn weight_elements(self, kind: OperatorKind) -> Result<usize, Error> {
        let input_per_group = self.input_channels / self.groups;
        let output_per_group = self.output_channels / self.groups;
        let planes = match kind {
            OperatorKind::Conv => self.output_channels.checked_mul(input_per_group),
            OperatorKind::ConvTranspose => self.input_channels.checked_mul(output_per_group),
        }
        .ok_or(Error::ShapeOverflow)?;
        planes
            .checked_mul(self.kernel_width)
            .ok_or(Error::ShapeOverflow)
    }

    pub const fn input_channels_per_group(self) -> usize {
        self.input_channels / self.groups
    }

    pub const fn output_channels_per_group(self) -> usize {
        self.output_channels / self.groups
    }

    pub fn scalar_fused_operations(self, kind: OperatorKind) -> Result<usize, Error> {
        let mut spatial_terms = 0usize;
        for output_time in 0..self.output_width {
            for kernel in 0..self.kernel_width {
                let present = match kind {
                    OperatorKind::Conv => {
                        let padded_position = output_time * self.stride + kernel;
                        padded_position >= self.pad_left
                            && padded_position - self.pad_left < self.input_width
                    }
                    OperatorKind::ConvTranspose => {
                        let padded_output_time = output_time + self.pad_left;
                        padded_output_time >= kernel
                            && (padded_output_time - kernel).is_multiple_of(self.stride)
                            && (padded_output_time - kernel) / self.stride < self.input_width
                    }
                };
                if present {
                    spatial_terms = spatial_terms.checked_add(1).ok_or(Error::ShapeOverflow)?;
                }
            }
        }
        spatial_terms
            .checked_mul(self.input_channels_per_group())
            .and_then(|value| value.checked_mul(self.output_channels))
            .and_then(|value| value.checked_mul(2))
            .ok_or(Error::ShapeOverflow)
    }
}

/// Immutable, prevalidated tensors for one invocation.
#[derive(Clone, Copy, Debug)]
pub struct Problem<'a> {
    profile: Profile,
    parameters: Parameters,
    dimensions: Dimensions,
    input: &'a [f32],
    weights: &'a [f32],
    bias: Option<&'a [f32]>,
}

impl<'a> Problem<'a> {
    pub fn new(
        profile: Profile,
        input: &'a [f32],
        weights: &'a [f32],
        bias: Option<&'a [f32]>,
    ) -> Result<Self, Error> {
        let parameters = profile.parameters();
        let dimensions = profile.dimensions()?;
        if input.len() != dimensions.input_elements()? {
            return Err(Error::InputLengthMismatch);
        }
        if weights.len() != dimensions.weight_elements(parameters.kind)? {
            return Err(Error::WeightLengthMismatch);
        }
        match (parameters.has_bias, bias) {
            (true, None) => return Err(Error::MissingBias),
            (false, Some(_)) => return Err(Error::UnexpectedBias),
            (true, Some(values)) if values.len() != dimensions.output_channels => {
                return Err(Error::BiasLengthMismatch);
            }
            _ => {}
        }

        let input_maximum = maximum_magnitude(input)?;
        let weight_maximum = maximum_magnitude(weights)?;
        let bias_maximum = match bias {
            Some(values) => maximum_magnitude(values)?,
            None => 0.0,
        };
        let maximum_terms = match parameters.kind {
            OperatorKind::Conv => dimensions
                .input_channels_per_group()
                .checked_mul(dimensions.kernel_width),
            OperatorKind::ConvTranspose => dimensions
                .input_channels_per_group()
                .checked_mul(dimensions.kernel_width.div_ceil(dimensions.stride)),
        }
        .ok_or(Error::ShapeOverflow)?;
        let magnitude_bound = bias_maximum as f64
            + input_maximum as f64 * weight_maximum as f64 * maximum_terms as f64;
        if !magnitude_bound.is_finite() || magnitude_bound > f32::MAX as f64 {
            return Err(Error::NonFiniteOutputRisk);
        }

        Ok(Self {
            profile,
            parameters,
            dimensions,
            input,
            weights,
            bias,
        })
    }

    pub const fn profile(self) -> Profile {
        self.profile
    }

    pub const fn parameters(self) -> Parameters {
        self.parameters
    }

    pub const fn dimensions(self) -> Dimensions {
        self.dimensions
    }

    pub const fn input(self) -> &'a [f32] {
        self.input
    }

    pub const fn weights(self) -> &'a [f32] {
        self.weights
    }

    pub const fn bias(self) -> Option<&'a [f32]> {
        self.bias
    }
}

fn maximum_magnitude(values: &[f32]) -> Result<f32, Error> {
    let mut maximum = 0.0f32;
    for &value in values {
        if !value.is_finite() {
            return Err(Error::NonFiniteInput);
        }
        maximum = maximum.max(value.abs());
    }
    Ok(maximum)
}

/// A nonempty rectangular region of the NCW output plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tile {
    pub channel_start: usize,
    pub channel_count: usize,
    pub time_start: usize,
    pub time_count: usize,
}

impl Tile {
    pub const fn whole(dimensions: Dimensions) -> Self {
        Self {
            channel_start: 0,
            channel_count: dimensions.output_channels,
            time_start: 0,
            time_count: dimensions.output_width,
        }
    }

    fn checked_ends(self, dimensions: Dimensions) -> Result<(usize, usize), Error> {
        if self.channel_count == 0 || self.time_count == 0 {
            return Err(Error::EmptyTile);
        }
        let channel_end = self
            .channel_start
            .checked_add(self.channel_count)
            .ok_or(Error::TileOutOfBounds)?;
        let time_end = self
            .time_start
            .checked_add(self.time_count)
            .ok_or(Error::TileOutOfBounds)?;
        if channel_end > dimensions.output_channels || time_end > dimensions.output_width {
            return Err(Error::TileOutOfBounds);
        }
        Ok((channel_end, time_end))
    }
}

/// Allocation-free deterministic channel-major cooperative tile schedule.
#[derive(Clone, Copy, Debug)]
pub struct TileCursor {
    dimensions: Dimensions,
    channel_tile: usize,
    time_tile: usize,
    next_channel: usize,
    next_time: usize,
}

impl TileCursor {
    pub fn new(
        dimensions: Dimensions,
        channel_tile: usize,
        time_tile: usize,
    ) -> Result<Self, Error> {
        if channel_tile == 0 || time_tile == 0 {
            return Err(Error::EmptyTile);
        }
        Ok(Self {
            dimensions,
            channel_tile,
            time_tile,
            next_channel: 0,
            next_time: 0,
        })
    }

    pub fn next_tile(&mut self) -> Option<Tile> {
        if self.next_channel >= self.dimensions.output_channels {
            return None;
        }
        let tile = Tile {
            channel_start: self.next_channel,
            channel_count: self
                .channel_tile
                .min(self.dimensions.output_channels - self.next_channel),
            time_start: self.next_time,
            time_count: self
                .time_tile
                .min(self.dimensions.output_width - self.next_time),
        };
        self.next_time += tile.time_count;
        if self.next_time == self.dimensions.output_width {
            self.next_time = 0;
            self.next_channel += tile.channel_count;
        }
        Some(tile)
    }

    pub const fn is_complete(self) -> bool {
        self.next_channel >= self.dimensions.output_channels
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane {
    Scalar,
    Avx2Fma,
}

impl Lane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar-fma",
            Self::Avx2Fma => "avx2-fma-256",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuCapabilities {
    ymm_state: bool,
    avx2: bool,
    fma: bool,
}

impl CpuCapabilities {
    pub const fn ymm_state(self) -> bool {
        self.ymm_state
    }

    pub const fn avx2(self) -> bool {
        self.avx2
    }

    pub const fn fma(self) -> bool {
        self.fma
    }

    pub const fn supports(self, lane: Lane) -> bool {
        match lane {
            Lane::Scalar => true,
            Lane::Avx2Fma => self.ymm_state && self.avx2 && self.fma,
        }
    }

    pub const fn best_lane(self) -> Lane {
        if self.supports(Lane::Avx2Fma) {
            Lane::Avx2Fma
        } else {
            Lane::Scalar
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Dispatcher {
    capabilities: CpuCapabilities,
}

impl Dispatcher {
    pub fn detect() -> Self {
        Self {
            capabilities: detect_cpu_capabilities(),
        }
    }

    pub const fn capabilities(self) -> CpuCapabilities {
        self.capabilities
    }

    pub const fn best_lane(self) -> Lane {
        self.capabilities.best_lane()
    }

    pub const fn supports(self, lane: Lane) -> bool {
        self.capabilities.supports(lane)
    }

    pub fn convolve(self, problem: Problem<'_>, output: &mut [f32]) -> Result<Lane, Error> {
        let lane = self.best_lane();
        self.convolve_with_lane(problem, output, lane)?;
        Ok(lane)
    }

    pub fn convolve_with_lane(
        self,
        problem: Problem<'_>,
        output: &mut [f32],
        lane: Lane,
    ) -> Result<(), Error> {
        self.convolve_tile_with_lane(problem, output, Tile::whole(problem.dimensions), lane)
    }

    /// Evaluate one cooperative output tile; all other output elements remain unchanged.
    pub fn convolve_tile_with_lane(
        self,
        problem: Problem<'_>,
        output: &mut [f32],
        tile: Tile,
        lane: Lane,
    ) -> Result<(), Error> {
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }
        validate_output(problem, output)?;
        tile.checked_ends(problem.dimensions)?;
        match lane {
            Lane::Scalar => convolve_scalar(problem, output, tile),
            Lane::Avx2Fma => {
                #[cfg(target_arch = "x86_64")]
                {
                    if problem.dimensions.input_width > i32::MAX as usize {
                        return Err(Error::AvxIndexTooLarge);
                    }
                    unsafe {
                        convolve_avx2_fma(problem, output, tile);
                    }
                }
                #[cfg(not(target_arch = "x86_64"))]
                return Err(Error::UnsupportedLane);
            }
        }
        Ok(())
    }
}

fn validate_output(problem: Problem<'_>, output: &[f32]) -> Result<(), Error> {
    if output.len() != problem.dimensions.output_elements()? {
        return Err(Error::OutputLengthMismatch);
    }
    if memory_ranges_overlap(
        output.as_ptr(),
        output.len(),
        problem.input.as_ptr(),
        problem.input.len(),
    ) || memory_ranges_overlap(
        output.as_ptr(),
        output.len(),
        problem.weights.as_ptr(),
        problem.weights.len(),
    ) || problem.bias.is_some_and(|bias| {
        memory_ranges_overlap(output.as_ptr(), output.len(), bias.as_ptr(), bias.len())
    }) {
        return Err(Error::Aliasing);
    }
    Ok(())
}

fn convolve_scalar(problem: Problem<'_>, output: &mut [f32], tile: Tile) {
    let (channel_end, time_end) = tile.checked_ends(problem.dimensions).unwrap();
    for output_channel in tile.channel_start..channel_end {
        for output_time in tile.time_start..time_end {
            output[output_channel * problem.dimensions.output_width + output_time] =
                output_element(problem, output_channel, output_time);
        }
    }
}

#[inline]
fn output_element(problem: Problem<'_>, output_channel: usize, output_time: usize) -> f32 {
    match problem.parameters.kind {
        OperatorKind::Conv => conv_element(problem, output_channel, output_time),
        OperatorKind::ConvTranspose => conv_transpose_element(problem, output_channel, output_time),
    }
}

#[inline]
fn conv_element(problem: Problem<'_>, output_channel: usize, output_time: usize) -> f32 {
    let dimensions = problem.dimensions;
    let input_per_group = dimensions.input_channels_per_group();
    let output_per_group = dimensions.output_channels_per_group();
    let group = output_channel / output_per_group;
    let first_input_channel = group * input_per_group;
    let mut accumulator = problem.bias.map_or(0.0, |bias| bias[output_channel]);
    for local_input_channel in 0..input_per_group {
        let input_channel = first_input_channel + local_input_channel;
        let input_row = input_channel * dimensions.input_width;
        let weight_row =
            (output_channel * input_per_group + local_input_channel) * dimensions.kernel_width;
        for kernel in 0..dimensions.kernel_width {
            let padded_position = output_time * dimensions.stride + kernel;
            if padded_position < dimensions.pad_left {
                continue;
            }
            let input_time = padded_position - dimensions.pad_left;
            if input_time < dimensions.input_width {
                accumulator = libm::fmaf(
                    problem.input[input_row + input_time],
                    problem.weights[weight_row + kernel],
                    accumulator,
                );
            }
        }
    }
    accumulator
}

#[inline]
fn conv_transpose_element(problem: Problem<'_>, output_channel: usize, output_time: usize) -> f32 {
    let dimensions = problem.dimensions;
    let input_per_group = dimensions.input_channels_per_group();
    let output_per_group = dimensions.output_channels_per_group();
    let group = output_channel / output_per_group;
    let local_output_channel = output_channel - group * output_per_group;
    let first_input_channel = group * input_per_group;
    let padded_output_time = output_time + dimensions.pad_left;
    let mut accumulator = problem.bias.map_or(0.0, |bias| bias[output_channel]);
    for local_input_channel in 0..input_per_group {
        let input_channel = first_input_channel + local_input_channel;
        let input_row = input_channel * dimensions.input_width;
        let weight_row =
            (input_channel * output_per_group + local_output_channel) * dimensions.kernel_width;
        for kernel in 0..dimensions.kernel_width {
            if padded_output_time < kernel {
                continue;
            }
            let expanded_input_time = padded_output_time - kernel;
            if !expanded_input_time.is_multiple_of(dimensions.stride) {
                continue;
            }
            let input_time = expanded_input_time / dimensions.stride;
            if input_time < dimensions.input_width {
                accumulator = libm::fmaf(
                    problem.input[input_row + input_time],
                    problem.weights[weight_row + kernel],
                    accumulator,
                );
            }
        }
    }
    accumulator
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn convolve_avx2_fma(problem: Problem<'_>, output: &mut [f32], tile: Tile) {
    match problem.parameters.kind {
        OperatorKind::Conv => unsafe { conv_avx2_fma(problem, output, tile) },
        OperatorKind::ConvTranspose => match problem.profile {
            Profile::Upsample512To256 { .. } => unsafe {
                conv_transpose_dense_avx2_fma::<10>(problem, output, tile)
            },
            Profile::Upsample256To128 { .. } => unsafe {
                conv_transpose_dense_avx2_fma::<6>(problem, output, tile)
            },
            _ => unsafe { conv_transpose_avx2_fma(problem, output, tile) },
        },
    }
    core::arch::x86_64::_mm256_zeroupper();
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn conv_avx2_fma(problem: Problem<'_>, output: &mut [f32], tile: Tile) {
    use core::arch::x86_64::{_mm256_fmadd_ps, _mm256_loadu_ps, _mm256_set1_ps, _mm256_storeu_ps};

    let dimensions = problem.dimensions;
    let input_per_group = dimensions.input_channels_per_group();
    let output_per_group = dimensions.output_channels_per_group();
    let (channel_end, time_end) = tile.checked_ends(dimensions).unwrap();
    for output_channel in tile.channel_start..channel_end {
        let group = output_channel / output_per_group;
        let first_input_channel = group * input_per_group;
        let output_row = output_channel * dimensions.output_width;
        let mut output_time = tile.time_start;
        while output_time + SIMD_TIME_TILE <= time_end {
            let interior = output_time >= dimensions.pad_left
                && output_time + SIMD_TIME_TILE - 1 + dimensions.kernel_width - 1
                    < dimensions.input_width + dimensions.pad_left;
            if !interior {
                for time in output_time..output_time + SIMD_TIME_TILE {
                    output[output_row + time] = conv_element(problem, output_channel, time);
                }
                output_time += SIMD_TIME_TILE;
                continue;
            }

            let bias = problem.bias.map_or(0.0, |values| values[output_channel]);
            let mut accumulator = _mm256_set1_ps(bias);
            for local_input_channel in 0..input_per_group {
                let input_channel = first_input_channel + local_input_channel;
                let input_row = input_channel * dimensions.input_width;
                let weight_row = (output_channel * input_per_group + local_input_channel)
                    * dimensions.kernel_width;
                for kernel in 0..dimensions.kernel_width {
                    let input_start = input_row + output_time - dimensions.pad_left + kernel;
                    let input_values =
                        unsafe { _mm256_loadu_ps(problem.input.as_ptr().add(input_start)) };
                    let weight = _mm256_set1_ps(problem.weights[weight_row + kernel]);
                    accumulator = _mm256_fmadd_ps(input_values, weight, accumulator);
                }
            }
            unsafe {
                _mm256_storeu_ps(output.as_mut_ptr().add(output_row + output_time), accumulator);
            }
            output_time += SIMD_TIME_TILE;
        }
        while output_time < time_end {
            output[output_row + output_time] = conv_element(problem, output_channel, output_time);
            output_time += 1;
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy)]
struct GatherPlan {
    input_indices: [i32; SIMD_TIME_TILE],
    kernel_indices: [i32; SIMD_TIME_TILE],
    masks: [i32; SIMD_TIME_TILE],
}

#[cfg(target_arch = "x86_64")]
impl GatherPlan {
    const EMPTY: Self = Self {
        input_indices: [0; SIMD_TIME_TILE],
        kernel_indices: [0; SIMD_TIME_TILE],
        masks: [0; SIMD_TIME_TILE],
    };
}

#[cfg(target_arch = "x86_64")]
fn transpose_gather_plans(
    dimensions: Dimensions,
    output_time: usize,
) -> ([GatherPlan; MAX_TRANSPOSE_TAP_LAYERS], usize) {
    let layer_count = dimensions.kernel_width.div_ceil(dimensions.stride);
    debug_assert!(layer_count <= MAX_TRANSPOSE_TAP_LAYERS);
    let mut plans = [GatherPlan::EMPTY; MAX_TRANSPOSE_TAP_LAYERS];
    for (layer, plan) in plans.iter_mut().enumerate().take(layer_count) {
        for lane in 0..SIMD_TIME_TILE {
            let padded_time = output_time + lane + dimensions.pad_left;
            let kernel = padded_time % dimensions.stride + layer * dimensions.stride;
            if kernel >= dimensions.kernel_width || padded_time < kernel {
                continue;
            }
            let input_time = (padded_time - kernel) / dimensions.stride;
            if input_time >= dimensions.input_width {
                continue;
            }
            plan.input_indices[lane] = input_time as i32;
            plan.kernel_indices[lane] = kernel as i32;
            plan.masks[lane] = -1;
        }
    }
    (plans, layer_count)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn vector_from_i32(values: [i32; SIMD_TIME_TILE]) -> core::arch::x86_64::__m256i {
    core::arch::x86_64::_mm256_set_epi32(
        values[7], values[6], values[5], values[4], values[3], values[2], values[1], values[0],
    )
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn conv_transpose_avx2_fma(problem: Problem<'_>, output: &mut [f32], tile: Tile) {
    use core::arch::x86_64::{
        _mm256_castsi256_ps, _mm256_fmadd_ps, _mm256_mask_i32gather_ps, _mm256_set1_ps,
        _mm256_setzero_ps, _mm256_storeu_ps,
    };

    let dimensions = problem.dimensions;
    let input_per_group = dimensions.input_channels_per_group();
    let output_per_group = dimensions.output_channels_per_group();
    let (channel_end, time_end) = tile.checked_ends(dimensions).unwrap();
    for output_channel in tile.channel_start..channel_end {
        let group = output_channel / output_per_group;
        let local_output_channel = output_channel - group * output_per_group;
        let first_input_channel = group * input_per_group;
        let output_row = output_channel * dimensions.output_width;
        let mut output_time = tile.time_start;
        while output_time + SIMD_TIME_TILE <= time_end {
            let (plans, layer_count) = transpose_gather_plans(dimensions, output_time);
            let bias = problem.bias.map_or(0.0, |values| values[output_channel]);
            let mut accumulator = _mm256_set1_ps(bias);
            for local_input_channel in 0..input_per_group {
                let input_channel = first_input_channel + local_input_channel;
                let input_row = input_channel * dimensions.input_width;
                let weight_row = (input_channel * output_per_group + local_output_channel)
                    * dimensions.kernel_width;
                for plan in &plans[..layer_count] {
                    let input_indices = vector_from_i32(plan.input_indices);
                    let kernel_indices = vector_from_i32(plan.kernel_indices);
                    let mask = _mm256_castsi256_ps(vector_from_i32(plan.masks));
                    let input_values = unsafe {
                        _mm256_mask_i32gather_ps::<4>(
                            _mm256_setzero_ps(),
                            problem.input.as_ptr().add(input_row),
                            input_indices,
                            mask,
                        )
                    };
                    let weights = unsafe {
                        _mm256_mask_i32gather_ps::<4>(
                            _mm256_setzero_ps(),
                            problem.weights.as_ptr().add(weight_row),
                            kernel_indices,
                            mask,
                        )
                    };
                    accumulator = _mm256_fmadd_ps(input_values, weights, accumulator);
                }
            }
            unsafe {
                _mm256_storeu_ps(output.as_mut_ptr().add(output_row + output_time), accumulator);
            }
            output_time += SIMD_TIME_TILE;
        }
        while output_time < time_end {
            output[output_row + output_time] =
                conv_transpose_element(problem, output_channel, output_time);
            output_time += 1;
        }
    }
}

/// Dense Kokoro upsamplers have `kernel_width == 2 * stride`.  Evaluating
/// adjacent output times needs two input and two weight gathers for every
/// input channel.  Eight outputs separated by `STRIDE` instead consume
/// contiguous input vectors and reuse each scalar weight across all lanes.
///
/// The loop order for any individual output remains input-channel first and
/// then ascending kernel, so this is bit-identical to the scalar contract.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn conv_transpose_dense_avx2_fma<const STRIDE: usize>(
    problem: Problem<'_>,
    output: &mut [f32],
    tile: Tile,
) {
    use core::arch::x86_64::{_mm256_fmadd_ps, _mm256_loadu_ps, _mm256_set1_ps, _mm256_storeu_ps};

    let dimensions = problem.dimensions;
    debug_assert_eq!(dimensions.groups, 1);
    debug_assert_eq!(dimensions.stride, STRIDE);
    debug_assert_eq!(dimensions.kernel_width, 2 * STRIDE);
    let (channel_end, time_end) = tile.checked_ends(dimensions).unwrap();
    let vector_time_block = STRIDE * SIMD_TIME_TILE;

    for output_channel in tile.channel_start..channel_end {
        let output_row = output_channel * dimensions.output_width;
        let bias = problem.bias.map_or(0.0, |values| values[output_channel]);
        let mut output_time = tile.time_start;

        // Align a phase block so `(output_time + pad_left) / STRIDE` is the
        // first input time shared by all kernel residues.
        let remainder = (output_time + dimensions.pad_left) % STRIDE;
        let scalar_prefix = if remainder == 0 {
            0
        } else {
            STRIDE - remainder
        };
        let prefix_end = time_end.min(output_time + scalar_prefix);
        while output_time < prefix_end {
            output[output_row + output_time] =
                conv_transpose_element(problem, output_channel, output_time);
            output_time += 1;
        }

        while time_end - output_time >= vector_time_block {
            let base_input_time = (output_time + dimensions.pad_left) / STRIDE;
            // Both kernel layers must provide eight in-bounds contiguous
            // input samples.  Only the small model boundary remains scalar.
            if base_input_time == 0 || base_input_time + SIMD_TIME_TILE > dimensions.input_width {
                break;
            }

            for phase in 0..STRIDE {
                let mut accumulator = _mm256_set1_ps(bias);
                for input_channel in 0..dimensions.input_channels {
                    let input_row = input_channel * dimensions.input_width;
                    let weight_row = (input_channel * dimensions.output_channels + output_channel)
                        * dimensions.kernel_width;
                    for layer in 0..2 {
                        let input_start = input_row + base_input_time - layer;
                        let input_values =
                            unsafe { _mm256_loadu_ps(problem.input.as_ptr().add(input_start)) };
                        let weight =
                            _mm256_set1_ps(problem.weights[weight_row + phase + layer * STRIDE]);
                        accumulator = _mm256_fmadd_ps(input_values, weight, accumulator);
                    }
                }

                let mut lanes = [0.0_f32; SIMD_TIME_TILE];
                unsafe { _mm256_storeu_ps(lanes.as_mut_ptr(), accumulator) };
                for (lane, value) in lanes.into_iter().enumerate() {
                    output[output_row + output_time + phase + lane * STRIDE] = value;
                }
            }
            output_time += vector_time_block;
        }

        while output_time < time_end {
            output[output_row + output_time] =
                conv_transpose_element(problem, output_channel, output_time);
            output_time += 1;
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn detect_cpu_capabilities() -> CpuCapabilities {
    use core::arch::x86_64::{__cpuid, __cpuid_count};

    const CPUID_1_ECX_FMA: u32 = 1 << 12;
    const CPUID_1_ECX_OSXSAVE: u32 = 1 << 27;
    const CPUID_1_ECX_AVX: u32 = 1 << 28;
    const CPUID_7_EBX_AVX2: u32 = 1 << 5;
    const XCR0_XMM_YMM: u64 = (1 << 1) | (1 << 2);

    let maximum_leaf = __cpuid(0).eax;
    if maximum_leaf < 1 {
        return CpuCapabilities::default();
    }
    let leaf_1 = __cpuid(1);
    let avx_state_contract = CPUID_1_ECX_OSXSAVE | CPUID_1_ECX_AVX;
    if leaf_1.ecx & avx_state_contract != avx_state_contract {
        return CpuCapabilities::default();
    }
    let xcr0 = unsafe { read_xcr0() };
    if xcr0 & XCR0_XMM_YMM != XCR0_XMM_YMM || maximum_leaf < 7 {
        return CpuCapabilities::default();
    }
    let leaf_7 = __cpuid_count(7, 0);
    CpuCapabilities {
        ymm_state: true,
        avx2: leaf_7.ebx & CPUID_7_EBX_AVX2 != 0,
        fma: leaf_1.ecx & CPUID_1_ECX_FMA != 0,
    }
}

#[cfg(not(target_arch = "x86_64"))]
const fn detect_cpu_capabilities() -> CpuCapabilities {
    CpuCapabilities {
        ymm_state: false,
        avx2: false,
        fma: false,
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn read_xcr0() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "xgetbv",
            in("ecx") 0u32,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags),
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

fn memory_ranges_overlap(lhs: *const f32, lhs_len: usize, rhs: *const f32, rhs_len: usize) -> bool {
    if lhs_len == 0 || rhs_len == 0 {
        return false;
    }
    let element_bytes = core::mem::size_of::<f32>();
    let lhs_start = lhs as usize;
    let rhs_start = rhs as usize;
    let lhs_end = lhs_start.saturating_add(lhs_len.saturating_mul(element_bytes));
    let rhs_end = rhs_start.saturating_add(rhs_len.saturating_mul(element_bytes));
    lhs_start < rhs_end && rhs_start < lhs_end
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    // Full-output fingerprints and sampled words were generated with NumPy
    // 2.5.1, ONNX 1.22.0, and ONNX Runtime 1.28.0. Sessions used opset 20,
    // disabled graph optimizations, sequential execution, and one CPU thread.

    fn pattern(
        length: usize,
        multiplier: usize,
        modulus: usize,
        center: i32,
        divisor: f32,
    ) -> Vec<f32> {
        (0..length)
            .map(|index| {
                let integer = ((index * multiplier) % modulus) as i32 - center;
                integer as f32 / divisor
            })
            .collect()
    }

    fn output_fingerprint(values: &[f32]) -> u64 {
        let mut fingerprint = 0xcbf2_9ce4_8422_2325u64;
        for value in values {
            for byte in value.to_bits().to_le_bytes() {
                fingerprint ^= u64::from(byte);
                fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        fingerprint
    }

    fn fixture_tensors(profile: Profile, divisor: f32) -> (Vec<f32>, Vec<f32>, Option<Vec<f32>>) {
        let dimensions = profile.dimensions().unwrap();
        let parameters = profile.parameters();
        let input = pattern(dimensions.input_elements().unwrap(), 7, 11, 5, divisor);
        let weights =
            pattern(dimensions.weight_elements(parameters.kind).unwrap(), 5, 13, 6, divisor * 2.0);
        let bias = parameters
            .has_bias
            .then(|| pattern(dimensions.output_channels, 3, 7, 3, divisor * 4.0));
        (input, weights, bias)
    }

    fn run_ort_fixture(
        profile: Profile,
        expected_fingerprint: u64,
        indices: &[usize],
        expected_words: &[u32],
    ) {
        let dimensions = profile.dimensions().unwrap();
        let (input, weights, bias) = fixture_tensors(profile, 64.0);
        let problem = Problem::new(profile, &input, &weights, bias.as_deref()).unwrap();
        let dispatcher = Dispatcher::detect();
        let mut scalar = vec![0.0; dimensions.output_elements().unwrap()];
        dispatcher
            .convolve_with_lane(problem, &mut scalar, Lane::Scalar)
            .unwrap();
        assert_eq!(output_fingerprint(&scalar), expected_fingerprint);
        assert_eq!(indices.len(), expected_words.len());
        for (&index, &expected) in indices.iter().zip(expected_words) {
            assert_eq!(scalar[index].to_bits(), expected, "fixture index {index}");
        }

        if dispatcher.supports(Lane::Avx2Fma) {
            let mut vector = vec![0.0; scalar.len()];
            dispatcher
                .convolve_with_lane(problem, &mut vector, Lane::Avx2Fma)
                .unwrap();
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
    }

    #[test]
    fn inventory_covers_all_seven_runtime_nodes_and_onnx_lengths() {
        let profiles = [
            Profile::EncoderDepthwise512 { input_width: 69 },
            Profile::DecoderDepthwise1090 { input_width: 69 },
            Profile::Upsample512To256 { input_width: 138 },
            Profile::Upsample256To128 { input_width: 1_380 },
            Profile::PostConv128To22 { input_width: 8_281 },
            Profile::Istft22To1 { input_width: 8_281 },
        ];
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.graph_nodes())
                .sum::<usize>(),
            PINNED_NODE_COVERAGE
        );
        assert_eq!(
            profiles.map(|profile| profile.dimensions().unwrap().output_width),
            [138, 138, 1_380, 8_280, 8_281, 41_420]
        );

        let depthwise = profiles[0].parameters();
        assert_eq!(depthwise.kind, OperatorKind::ConvTranspose);
        assert_eq!((depthwise.groups, depthwise.kernel_width, depthwise.stride), (512, 3, 2));
        assert_eq!((depthwise.pad_left, depthwise.pad_right, depthwise.output_padding), (1, 1, 1));
        let istft = profiles[5].parameters();
        assert!(!istft.has_bias);
        assert_eq!((istft.input_channels, istft.output_channels, istft.kernel_width), (22, 1, 20));
    }

    #[test]
    fn all_six_families_match_full_ort_128_fingerprints() {
        run_ort_fixture(
            Profile::EncoderDepthwise512 { input_width: 5 },
            0xD3A5_B238_2534_BA8B,
            &[0, 1, 9, 2_560, 2_565, 5_119],
            &[
                0xBC36_0000,
                0xBC80_0000,
                0xBC38_0000,
                0x3C08_0000,
                0x3BC0_0000,
                0xBC40_0000,
            ],
        );
        run_ort_fixture(
            Profile::DecoderDepthwise1090 { input_width: 4 },
            0x16F7_9AFC_A695_117E,
            &[0, 1, 7, 4_360, 4_364, 8_719],
            &[
                0xBC36_0000,
                0xBC80_0000,
                0xBC18_0000,
                0x3B98_0000,
                0x3B68_0000,
                0x3BD8_0000,
            ],
        );
        run_ort_fixture(
            Profile::Upsample512To256 { input_width: 3 },
            0xE96A_EF7F_BACB_E6CE,
            &[0, 1, 29, 3_840, 3_855, 7_679],
            &[
                0xBBFC_0000,
                0xBC6A_0000,
                0xBC89_0000,
                0x3C52_0000,
                0x3C5E_0000,
                0xBB70_0000,
            ],
        );
        run_ort_fixture(
            Profile::Upsample256To128 { input_width: 3 },
            0x5823_2287_1CCB_69BD,
            &[0, 1, 17, 1_152, 1_161, 2_303],
            &[
                0xBB00_0000,
                0xBC8B_0000,
                0xBC0E_0000,
                0xBBAC_0000,
                0x3B8C_0000,
                0xBC42_0000,
            ],
        );
        run_ort_fixture(
            Profile::PostConv128To22 { input_width: 11 },
            0x8576_B6EB_1062_342F,
            &[0, 1, 10, 121, 126, 241],
            &[
                0xBC74_0000,
                0xBC82_0000,
                0xBC2C_0000,
                0x3AC0_0000,
                0x3900_0000,
                0xBB98_0000,
            ],
        );
        run_ort_fixture(
            Profile::Istft22To1 { input_width: 4 },
            0xAD69_F664_035B_1CF3,
            &[0, 1, 34, 17],
            &[0x3BD8_0000, 0x3B60_0000, 0x3AD0_0000, 0xBC4A_0000],
        );
    }

    #[test]
    fn avx_preserves_scalar_fma_order_for_nonexact_dense_transpose() {
        let dispatcher = Dispatcher::detect();
        if !dispatcher.supports(Lane::Avx2Fma) {
            return;
        }
        let profile = Profile::Upsample256To128 { input_width: 3 };
        let dimensions = profile.dimensions().unwrap();
        let (input, weights, bias) = fixture_tensors(profile, 10.0);
        let problem = Problem::new(profile, &input, &weights, bias.as_deref()).unwrap();
        let mut scalar = vec![0.0; dimensions.output_elements().unwrap()];
        let mut vector = vec![0.0; scalar.len()];
        dispatcher
            .convolve_with_lane(problem, &mut scalar, Lane::Scalar)
            .unwrap();
        dispatcher
            .convolve_with_lane(problem, &mut vector, Lane::Avx2Fma)
            .unwrap();
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
    fn dense_transpose_phase_blocks_preserve_partial_tile_results() {
        let dispatcher = Dispatcher::detect();
        if !dispatcher.supports(Lane::Avx2Fma) {
            return;
        }
        for profile in [
            Profile::Upsample512To256 { input_width: 9 },
            Profile::Upsample256To128 { input_width: 15 },
        ] {
            let dimensions = profile.dimensions().unwrap();
            let (input, weights, bias) = fixture_tensors(profile, 10.0);
            let problem = Problem::new(profile, &input, &weights, bias.as_deref()).unwrap();
            let output_channel = 3;
            let split = dimensions.output_width - 3;
            let mut scalar = vec![f32::NAN; dimensions.output_elements().unwrap()];
            let mut vector = vec![f32::NAN; scalar.len()];
            dispatcher
                .convolve_tile_with_lane(
                    problem,
                    &mut scalar,
                    Tile {
                        channel_start: output_channel,
                        channel_count: 1,
                        time_start: 0,
                        time_count: dimensions.output_width,
                    },
                    Lane::Scalar,
                )
                .unwrap();
            for (time_start, time_count) in [(0, split), (split, 3)] {
                dispatcher
                    .convolve_tile_with_lane(
                        problem,
                        &mut vector,
                        Tile {
                            channel_start: output_channel,
                            channel_count: 1,
                            time_start,
                            time_count,
                        },
                        Lane::Avx2Fma,
                    )
                    .unwrap();
            }
            let row = output_channel * dimensions.output_width;
            assert_eq!(
                vector[row..row + dimensions.output_width]
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                scalar[row..row + dimensions.output_width]
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn cooperative_tiles_cover_output_once_and_preserve_results() {
        let profile = Profile::PostConv128To22 { input_width: 19 };
        let dimensions = profile.dimensions().unwrap();
        let (input, weights, bias) = fixture_tensors(profile, 10.0);
        let problem = Problem::new(profile, &input, &weights, bias.as_deref()).unwrap();
        let dispatcher = Dispatcher::detect();
        let lane = dispatcher.best_lane();
        let mut whole = vec![0.0; dimensions.output_elements().unwrap()];
        let mut tiled = vec![f32::NAN; whole.len()];
        dispatcher
            .convolve_with_lane(problem, &mut whole, lane)
            .unwrap();
        let mut cursor = TileCursor::new(dimensions, 5, 7).unwrap();
        let mut tiles = 0;
        while let Some(tile) = cursor.next_tile() {
            dispatcher
                .convolve_tile_with_lane(problem, &mut tiled, tile, lane)
                .unwrap();
            tiles += 1;
        }
        assert!(cursor.is_complete());
        assert_eq!(tiles, 15);
        assert_eq!(
            tiled
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            whole
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );

        // Exercise transposed-convolution SIMD plans from nonzero tile starts.
        let profile = Profile::Istft22To1 { input_width: 6 };
        let dimensions = profile.dimensions().unwrap();
        let (input, weights, _) = fixture_tensors(profile, 10.0);
        let problem = Problem::new(profile, &input, &weights, None).unwrap();
        let mut whole = vec![0.0; dimensions.output_elements().unwrap()];
        let mut tiled = vec![f32::NAN; whole.len()];
        dispatcher
            .convolve_with_lane(problem, &mut whole, lane)
            .unwrap();
        let mut cursor = TileCursor::new(dimensions, 1, 13).unwrap();
        let mut tiles = 0;
        while let Some(tile) = cursor.next_tile() {
            dispatcher
                .convolve_tile_with_lane(problem, &mut tiled, tile, lane)
                .unwrap();
            tiles += 1;
        }
        assert_eq!(tiles, 4);
        assert_eq!(
            tiled
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            whole
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn validation_is_strict_fail_closed_and_transactional() {
        assert_eq!(Profile::Istft22To1 { input_width: 0 }.dimensions(), Err(Error::EmptyInput));
        let profile = Profile::Istft22To1 { input_width: 2 };
        let dimensions = profile.dimensions().unwrap();
        let (mut input, weights, _) = fixture_tensors(profile, 64.0);
        assert_eq!(
            Problem::new(profile, &input[..input.len() - 1], &weights, None).unwrap_err(),
            Error::InputLengthMismatch
        );
        assert_eq!(
            Problem::new(profile, &input, &weights, Some(&[0.0])).unwrap_err(),
            Error::UnexpectedBias
        );
        input[0] = f32::NAN;
        assert_eq!(
            Problem::new(profile, &input, &weights, None).unwrap_err(),
            Error::NonFiniteInput
        );
        input.fill(f32::MAX);
        assert_eq!(
            Problem::new(profile, &input, &weights, None).unwrap_err(),
            Error::NonFiniteOutputRisk
        );

        input.fill(0.25);
        let problem = Problem::new(profile, &input, &weights, None).unwrap();
        let mut output = vec![123.0; dimensions.output_elements().unwrap()];
        assert_eq!(
            Dispatcher::detect().convolve_tile_with_lane(
                problem,
                &mut output,
                Tile {
                    channel_start: 0,
                    channel_count: 1,
                    time_start: dimensions.output_width,
                    time_count: 1,
                },
                Lane::Scalar,
            ),
            Err(Error::TileOutOfBounds)
        );
        assert!(output.iter().all(|&value| value == 123.0));
        assert_eq!(TileCursor::new(dimensions, 0, 1).unwrap_err(), Error::EmptyTile);
        assert!(memory_ranges_overlap(
            input.as_ptr(),
            input.len(),
            input[1..].as_ptr(),
            input.len() - 1,
        ));
    }

    #[test]
    fn runtime_lane_contract_requires_complete_ymm_avx2_fma_support() {
        let dispatcher = Dispatcher::detect();
        let capabilities = dispatcher.capabilities();
        if capabilities.supports(Lane::Avx2Fma) {
            assert!(capabilities.ymm_state());
            assert!(capabilities.avx2());
            assert!(capabilities.fma());
        }
        let scalar_only = Dispatcher {
            capabilities: CpuCapabilities::default(),
        };
        let profile = Profile::Istft22To1 { input_width: 1 };
        let dimensions = profile.dimensions().unwrap();
        let (input, weights, _) = fixture_tensors(profile, 64.0);
        let problem = Problem::new(profile, &input, &weights, None).unwrap();
        assert_eq!(
            scalar_only.convolve_with_lane(
                problem,
                &mut vec![0.0; dimensions.output_elements().unwrap()],
                Lane::Avx2Fma,
            ),
            Err(Error::UnsupportedLane)
        );
    }
}
