#![cfg_attr(not(test), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Integer CPU primitives for the Kokoro inference path.
//!
//! The model's dynamically quantized projections multiply unsigned activations
//! by signed weights. This crate keeps that contract explicit and implements
//! three bit-identical lanes:
//!
//! - scalar, available everywhere;
//! - 256-bit AVX2, using widening multiplies rather than saturating
//!   `vpmaddubsw`;
//! - 256-bit AVX-VNNI, using `vpdpbusd`.
//!
//! [`Dispatcher::detect`] probes the current CPU and the OS-owned XMM/YMM state.
//! It intentionally does not cache the result globally, so a bare-metal caller
//! can construct one after enabling XCR0 on each worker CPU. AVX-512 state is
//! neither required nor used.

mod quant;

pub use quant::{
    DynamicQuantization, QuantizationError, conv_integer_scale, dequantize_conv_integer_ncw,
    dequantize_matmul_integer_per_output, dynamic_quantization_parameters,
    dynamic_quantize_linear_u8, quantize_conv_bias_floor, quantize_linear_u8_with_parameters,
};

/// A CPU implementation of the unsigned-by-signed integer dot product.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane {
    Scalar,
    Avx2,
    AvxVnni,
}

impl Lane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Avx2 => "avx2",
            Self::AvxVnni => "avx-vnni",
        }
    }
}

/// SIMD state and instruction support observed on the current CPU.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuCapabilities {
    ymm_state: bool,
    avx2: bool,
    avx_vnni: bool,
}

impl CpuCapabilities {
    pub const fn ymm_state(self) -> bool {
        self.ymm_state
    }

    pub const fn avx2(self) -> bool {
        self.avx2
    }

    pub const fn avx_vnni(self) -> bool {
        self.avx_vnni
    }

    pub const fn supports(self, lane: Lane) -> bool {
        match lane {
            Lane::Scalar => true,
            Lane::Avx2 => self.ymm_state && self.avx2,
            Lane::AvxVnni => self.ymm_state && self.avx2 && self.avx_vnni,
        }
    }

    pub const fn best_lane(self) -> Lane {
        if self.supports(Lane::AvxVnni) {
            Lane::AvxVnni
        } else if self.supports(Lane::Avx2) {
            Lane::Avx2
        } else {
            Lane::Scalar
        }
    }
}

/// Errors returned before a kernel touches its input or output buffers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    EmptyReduction,
    EmptyMatrix,
    LengthMismatch,
    ShapeOverflow,
    LhsTooSmall,
    RhsTooSmall,
    OutputTooSmall,
    ZeroPointsTooSmall,
    RowSumsTooSmall,
    ScalesTooSmall,
    BiasTooSmall,
    ScratchTooSmall,
    NonFiniteInput,
    InvalidScale,
    InvalidQuantization,
    InvalidConvolutionShape,
    UnsupportedLane,
}

/// A scalar or per-output-channel signed weight zero point.
#[derive(Clone, Copy, Debug)]
pub enum RhsZeroPoints<'a> {
    Scalar(i8),
    PerOutput(&'a [i8]),
}

impl RhsZeroPoints<'_> {
    #[inline]
    fn validate(self, outputs: usize) -> Result<(), Error> {
        match self {
            Self::Scalar(_) => Ok(()),
            Self::PerOutput(values) if values.len() >= outputs => Ok(()),
            Self::PerOutput(_) => Err(Error::ZeroPointsTooSmall),
        }
    }

    #[inline]
    fn get(self, output: usize) -> i8 {
        match self {
            Self::Scalar(value) => value,
            Self::PerOutput(values) => values[output],
        }
    }
}

/// Row-major quantized matrix multiplication parameters.
///
/// `lhs` has shape `[m, k]`. `rhs_transposed` has the CPU-native shape
/// `[n, k]`, so each output channel is contiguous. An ONNX `[k, n]` weight
/// should be transposed during the model's offline packing step. `output` has
/// shape `[m, n]` and contains the exact `i32` MatMulInteger accumulators.
///
/// `rhs_row_sums`, when present, holds the raw signed sum of each `[k]` weight
/// row. Supplying these offline-computed sums removes zero-point bookkeeping
/// from the hot dot-product loop.
#[derive(Clone, Copy, Debug)]
pub struct QGemmParams<'a> {
    pub m: usize,
    pub n: usize,
    pub k: usize,
    pub lhs_zero_point: u8,
    pub rhs_zero_points: RhsZeroPoints<'a>,
    pub rhs_row_sums: Option<&'a [i32]>,
}

impl QGemmParams<'_> {
    fn validate(self, lhs: &[u8], rhs: &[i8], output: &[i32]) -> Result<(), Error> {
        if self.m == 0 || self.n == 0 {
            return Err(Error::EmptyMatrix);
        }
        if self.k == 0 {
            return Err(Error::EmptyReduction);
        }
        let lhs_len = self.m.checked_mul(self.k).ok_or(Error::ShapeOverflow)?;
        let rhs_len = self.n.checked_mul(self.k).ok_or(Error::ShapeOverflow)?;
        let output_len = self.m.checked_mul(self.n).ok_or(Error::ShapeOverflow)?;
        if lhs.len() < lhs_len {
            return Err(Error::LhsTooSmall);
        }
        if rhs.len() < rhs_len {
            return Err(Error::RhsTooSmall);
        }
        if output.len() < output_len {
            return Err(Error::OutputTooSmall);
        }
        self.rhs_zero_points.validate(self.n)?;
        if self
            .rhs_row_sums
            .is_some_and(|row_sums| row_sums.len() < self.n)
        {
            return Err(Error::RowSumsTooSmall);
        }
        Ok(())
    }
}

/// Allocation-free group-one ONNX ConvInteger parameters for one spatial
/// dimension. All 87 ConvInteger nodes in the pinned Kokoro graph use group
/// one; unsupported grouped/depthwise layouts are therefore not implied.
///
/// The input and output use ONNX's `[batch, channels, width]` layout. Packed
/// weights use `[output_channel, kernel_x, input_channel]`, which makes every
/// receptive field one contiguous VNNI dot product after it is gathered into
/// the caller-owned `patch_scratch` buffer.
///
/// Kokoro stores ConvInteger weights as `u8`. Pack them once with
/// [`pack_conv1d_weights_u8`] and translate their zero points with
/// [`signed_u8_zero_point`]. This lossless `u8 -> i8` domain shift lets the
/// same AVX-VNNI primitive implement `(x - x_zero_point) * (w - w_zero_point)`.
#[derive(Clone, Copy, Debug)]
pub struct QConv1dParams<'a> {
    pub batch: usize,
    pub input_channels: usize,
    pub input_width: usize,
    pub output_channels: usize,
    pub kernel_width: usize,
    pub stride: usize,
    pub dilation: usize,
    pub pad_left: usize,
    pub pad_right: usize,
    pub input_zero_point: u8,
    pub weight_zero_points: RhsZeroPoints<'a>,
    pub weight_row_sums: Option<&'a [i32]>,
}

impl QConv1dParams<'_> {
    /// Return the exact ONNX `NOTSET` output width for this 1-D convolution.
    pub fn output_width(self) -> Result<usize, Error> {
        if self.batch == 0
            || self.input_channels == 0
            || self.input_width == 0
            || self.output_channels == 0
        {
            return Err(Error::EmptyMatrix);
        }
        if self.kernel_width == 0 {
            return Err(Error::EmptyReduction);
        }
        if self.stride == 0 || self.dilation == 0 {
            return Err(Error::InvalidConvolutionShape);
        }
        let effective_kernel = self
            .kernel_width
            .checked_sub(1)
            .and_then(|width| width.checked_mul(self.dilation))
            .and_then(|width| width.checked_add(1))
            .ok_or(Error::ShapeOverflow)?;
        let padded_width = self
            .input_width
            .checked_add(self.pad_left)
            .and_then(|width| width.checked_add(self.pad_right))
            .ok_or(Error::ShapeOverflow)?;
        if padded_width < effective_kernel {
            return Err(Error::InvalidConvolutionShape);
        }
        Ok((padded_width - effective_kernel) / self.stride + 1)
    }

    fn validate(
        self,
        input: &[u8],
        weights: &[i8],
        output: &[i32],
        patch_scratch: &[u8],
    ) -> Result<usize, Error> {
        let output_width = self.output_width()?;
        let reduction = self
            .input_channels
            .checked_mul(self.kernel_width)
            .ok_or(Error::ShapeOverflow)?;
        let input_len = self
            .batch
            .checked_mul(self.input_channels)
            .and_then(|elements| elements.checked_mul(self.input_width))
            .ok_or(Error::ShapeOverflow)?;
        let weights_len = self
            .output_channels
            .checked_mul(reduction)
            .ok_or(Error::ShapeOverflow)?;
        let output_len = self
            .batch
            .checked_mul(self.output_channels)
            .and_then(|elements| elements.checked_mul(output_width))
            .ok_or(Error::ShapeOverflow)?;
        if input.len() < input_len {
            return Err(Error::LhsTooSmall);
        }
        if weights.len() < weights_len {
            return Err(Error::RhsTooSmall);
        }
        if output.len() < output_len {
            return Err(Error::OutputTooSmall);
        }
        if patch_scratch.len() < reduction {
            return Err(Error::ScratchTooSmall);
        }
        self.weight_zero_points.validate(self.output_channels)?;
        if self
            .weight_row_sums
            .is_some_and(|row_sums| row_sums.len() < self.output_channels)
        {
            return Err(Error::RowSumsTooSmall);
        }
        Ok(output_width)
    }

    fn validate_dequantized(
        self,
        input: &[f32],
        weights: &[i8],
        output: &[f32],
        patch_scratch: &[u8],
        bias: Option<&[f32]>,
        bias_scratch: &[i32],
    ) -> Result<(usize, usize), Error> {
        let output_width = self.output_width()?;
        let reduction = self
            .input_channels
            .checked_mul(self.kernel_width)
            .ok_or(Error::ShapeOverflow)?;
        let input_len = self
            .batch
            .checked_mul(self.input_channels)
            .and_then(|elements| elements.checked_mul(self.input_width))
            .ok_or(Error::ShapeOverflow)?;
        let weights_len = self
            .output_channels
            .checked_mul(reduction)
            .ok_or(Error::ShapeOverflow)?;
        let output_len = self
            .batch
            .checked_mul(self.output_channels)
            .and_then(|elements| elements.checked_mul(output_width))
            .ok_or(Error::ShapeOverflow)?;
        if input.len() < input_len {
            return Err(Error::LhsTooSmall);
        }
        if weights.len() < weights_len {
            return Err(Error::RhsTooSmall);
        }
        if output.len() < output_len {
            return Err(Error::OutputTooSmall);
        }
        if patch_scratch.len() < reduction {
            return Err(Error::ScratchTooSmall);
        }
        self.weight_zero_points.validate(self.output_channels)?;
        if self
            .weight_row_sums
            .is_some_and(|row_sums| row_sums.len() < self.output_channels)
        {
            return Err(Error::RowSumsTooSmall);
        }
        if bias.is_some_and(|values| values.len() < self.output_channels)
            || (bias.is_some() && bias_scratch.len() < self.output_channels)
        {
            return Err(Error::BiasTooSmall);
        }
        Ok((output_width, input_len))
    }
}

/// A per-current-CPU runtime dispatcher.
#[derive(Clone, Copy, Debug)]
pub struct Dispatcher {
    capabilities: CpuCapabilities,
}

impl Dispatcher {
    /// Probe CPUID and XCR0 on the current CPU.
    ///
    /// AVX2 or AVX-VNNI is admitted only when the processor advertises AVX and
    /// OSXSAVE and XCR0 currently enables both XMM and YMM state. This prevents
    /// safe entry points from executing a VEX instruction before TRUEOS has
    /// established its extended-state contract.
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

    /// Compute one `(u8 - lhs_zero_point) * (i8 - rhs_zero_point)` dot product.
    pub fn dot(
        self,
        lhs: &[u8],
        rhs: &[i8],
        lhs_zero_point: u8,
        rhs_zero_point: i8,
    ) -> Result<(i32, Lane), Error> {
        let lane = self.best_lane();
        let value = self.dot_with_lane(lhs, rhs, lhs_zero_point, rhs_zero_point, lane)?;
        Ok((value, lane))
    }

    /// Compute one dot product on an explicitly selected, runtime-checked lane.
    pub fn dot_with_lane(
        self,
        lhs: &[u8],
        rhs: &[i8],
        lhs_zero_point: u8,
        rhs_zero_point: i8,
        lane: Lane,
    ) -> Result<i32, Error> {
        validate_dot(lhs, rhs)?;
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }
        let (raw, lhs_sum, rhs_sum) = raw_dot_and_sums(lhs, rhs, lane);
        Ok(apply_zero_points(raw, lhs_sum, rhs_sum, lhs.len(), lhs_zero_point, rhs_zero_point))
    }

    /// Run row-major `u8 x i8 -> i32` QGEMM on the best available lane.
    pub fn qgemm(
        self,
        lhs: &[u8],
        rhs_transposed: &[i8],
        output: &mut [i32],
        params: QGemmParams<'_>,
    ) -> Result<Lane, Error> {
        let lane = self.best_lane();
        self.qgemm_with_lane(lhs, rhs_transposed, output, params, lane)?;
        Ok(lane)
    }

    /// Run QGEMM on an explicitly selected, runtime-checked lane.
    pub fn qgemm_with_lane(
        self,
        lhs: &[u8],
        rhs_transposed: &[i8],
        output: &mut [i32],
        params: QGemmParams<'_>,
        lane: Lane,
    ) -> Result<(), Error> {
        params.validate(lhs, rhs_transposed, output)?;
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }

        for m in 0..params.m {
            let lhs_start = m * params.k;
            let lhs_row = &lhs[lhs_start..lhs_start + params.k];
            let prepared_lhs_sum = params.rhs_row_sums.map(|_| sum_u8(lhs_row));

            for n in 0..params.n {
                let rhs_start = n * params.k;
                let rhs_row = &rhs_transposed[rhs_start..rhs_start + params.k];
                let rhs_zero_point = params.rhs_zero_points.get(n);
                let value = if let (Some(lhs_sum), Some(rhs_sums)) =
                    (prepared_lhs_sum, params.rhs_row_sums)
                {
                    let raw = raw_dot(lhs_row, rhs_row, lane);
                    apply_zero_points(
                        raw,
                        lhs_sum,
                        rhs_sums[n],
                        params.k,
                        params.lhs_zero_point,
                        rhs_zero_point,
                    )
                } else {
                    let (raw, lhs_sum, rhs_sum) = raw_dot_and_sums(lhs_row, rhs_row, lane);
                    apply_zero_points(
                        raw,
                        lhs_sum,
                        rhs_sum,
                        params.k,
                        params.lhs_zero_point,
                        rhs_zero_point,
                    )
                };
                output[m * params.n + n] = value;
            }
        }
        Ok(())
    }

    /// Run allocation-free 1-D ConvInteger on the best available lane.
    pub fn qconv1d(
        self,
        input_ncl: &[u8],
        weights_oki: &[i8],
        output_ncl: &mut [i32],
        patch_scratch: &mut [u8],
        params: QConv1dParams<'_>,
    ) -> Result<Lane, Error> {
        let lane = self.best_lane();
        self.qconv1d_with_lane(input_ncl, weights_oki, output_ncl, patch_scratch, params, lane)?;
        Ok(lane)
    }

    /// Run the fused Kokoro ConvInteger epilogue without materializing either
    /// the complete quantized activation or an i32 output tensor.
    ///
    /// Activation quantization parameters are reduced exactly once over the
    /// whole `input_ncl`. Quantization is then performed only for each bounded
    /// receptive-field patch; every i32 dot result is immediately bias-added,
    /// cast, scaled, and committed to the f32 destination. Optional bias is
    /// quantized into caller-owned scratch before the destination is touched.
    #[allow(clippy::too_many_arguments)]
    pub fn qconv1d_dequantized(
        self,
        input_ncl: &[f32],
        weights_oki: &[i8],
        output_ncl: &mut [f32],
        patch_scratch: &mut [u8],
        bias_scratch: &mut [i32],
        params: QConv1dParams<'_>,
        weight_scale: f32,
        bias: Option<&[f32]>,
    ) -> Result<(Lane, DynamicQuantization), Error> {
        let lane = self.best_lane();
        let quantization = self.qconv1d_dequantized_with_lane(
            input_ncl,
            weights_oki,
            output_ncl,
            patch_scratch,
            bias_scratch,
            params,
            weight_scale,
            bias,
            lane,
        )?;
        Ok((lane, quantization))
    }

    /// Explicit-lane form of [`Self::qconv1d_dequantized`].
    #[allow(clippy::too_many_arguments)]
    pub fn qconv1d_dequantized_with_lane(
        self,
        input_ncl: &[f32],
        weights_oki: &[i8],
        output_ncl: &mut [f32],
        patch_scratch: &mut [u8],
        bias_scratch: &mut [i32],
        params: QConv1dParams<'_>,
        weight_scale: f32,
        bias: Option<&[f32]>,
        lane: Lane,
    ) -> Result<DynamicQuantization, Error> {
        let (output_width, input_len) = params.validate_dequantized(
            input_ncl,
            weights_oki,
            output_ncl,
            patch_scratch,
            bias,
            bias_scratch,
        )?;
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }
        let activation_quantization = dynamic_quantization_parameters(&input_ncl[..input_len])
            .map_err(map_quantization_error)?;
        let params = QConv1dParams {
            input_zero_point: activation_quantization.zero_point,
            ..params
        };
        let combined_scale;
        let bias_quantized = if let Some(values) = bias {
            combined_scale = quantize_conv_bias_floor(
                &values[..params.output_channels],
                activation_quantization.scale,
                weight_scale,
                &mut bias_scratch[..params.output_channels],
            )
            .map_err(map_quantization_error)?;
            Some(&bias_scratch[..params.output_channels])
        } else {
            combined_scale = conv_integer_scale(activation_quantization.scale, weight_scale)
                .map_err(map_quantization_error)?;
            None
        };
        let reduction = params
            .input_channels
            .checked_mul(params.kernel_width)
            .ok_or(Error::ShapeOverflow)?;

        for batch in 0..params.batch {
            let mut output_x = 0usize;
            #[cfg(target_arch = "x86_64")]
            if lane == Lane::AvxVnni
                && patch_scratch.len() >= reduction.checked_mul(4).ok_or(Error::ShapeOverflow)?
            {
                while output_x + 4 <= output_width {
                    let blocked_patches = &mut patch_scratch[..reduction * 4];
                    for position in 0..4 {
                        let patch_start = position * reduction;
                        gather_qconv1d_patch_f32(
                            input_ncl,
                            &mut blocked_patches[patch_start..patch_start + reduction],
                            params,
                            activation_quantization,
                            batch,
                            output_x + position,
                        )?;
                    }
                    let lhs_sums = [
                        sum_u8(&blocked_patches[..reduction]),
                        sum_u8(&blocked_patches[reduction..reduction * 2]),
                        sum_u8(&blocked_patches[reduction * 2..reduction * 3]),
                        sum_u8(&blocked_patches[reduction * 3..reduction * 4]),
                    ];
                    for output_channel in 0..params.output_channels {
                        let weights_start = output_channel * reduction;
                        let weights = &weights_oki[weights_start..weights_start + reduction];
                        let raw = unsafe { raw_dot4_avx_vnni(blocked_patches, reduction, weights) };
                        let weight_sum = params
                            .weight_row_sums
                            .map_or_else(|| sum_i8(weights), |sums| sums[output_channel]);
                        let weight_zero_point = params.weight_zero_points.get(output_channel);
                        let output_start = (batch * params.output_channels + output_channel)
                            * output_width
                            + output_x;
                        let bias = bias_quantized.map_or(0, |values| values[output_channel]);
                        for position in 0..4 {
                            let accumulator = apply_zero_points(
                                raw[position],
                                lhs_sums[position],
                                weight_sum,
                                reduction,
                                params.input_zero_point,
                                weight_zero_point,
                            )
                            .wrapping_add(bias);
                            output_ncl[output_start + position] =
                                accumulator as f32 * combined_scale;
                        }
                    }
                    output_x += 4;
                }
            }

            while output_x < output_width {
                let patch = &mut patch_scratch[..reduction];
                gather_qconv1d_patch_f32(
                    input_ncl,
                    patch,
                    params,
                    activation_quantization,
                    batch,
                    output_x,
                )?;
                let lhs_sum = sum_u8(patch);
                for output_channel in 0..params.output_channels {
                    let weights_start = output_channel * reduction;
                    let weights = &weights_oki[weights_start..weights_start + reduction];
                    let weight_sum = params
                        .weight_row_sums
                        .map_or_else(|| sum_i8(weights), |sums| sums[output_channel]);
                    let accumulator = apply_zero_points(
                        raw_dot(patch, weights, lane),
                        lhs_sum,
                        weight_sum,
                        reduction,
                        params.input_zero_point,
                        params.weight_zero_points.get(output_channel),
                    )
                    .wrapping_add(bias_quantized.map_or(0, |values| values[output_channel]));
                    let output_index =
                        (batch * params.output_channels + output_channel) * output_width + output_x;
                    output_ncl[output_index] = accumulator as f32 * combined_scale;
                }
                output_x += 1;
            }
        }
        Ok(activation_quantization)
    }

    /// Run allocation-free 1-D ConvInteger on an explicitly selected lane.
    ///
    /// Out-of-bounds spatial taps are filled with `input_zero_point`, which is
    /// zero in the centered integer domain required by ONNX padding semantics.
    pub fn qconv1d_with_lane(
        self,
        input_ncl: &[u8],
        weights_oki: &[i8],
        output_ncl: &mut [i32],
        patch_scratch: &mut [u8],
        params: QConv1dParams<'_>,
        lane: Lane,
    ) -> Result<(), Error> {
        let output_width = params.validate(input_ncl, weights_oki, output_ncl, patch_scratch)?;
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }
        let reduction = params.input_channels * params.kernel_width;

        for batch in 0..params.batch {
            let mut output_x = 0usize;

            // The i5 production lane reuses one weight-vector load across
            // four temporal positions. Callers opt in simply by supplying
            // four patches of scratch; the one-patch contract stays valid.
            #[cfg(target_arch = "x86_64")]
            if lane == Lane::AvxVnni
                && patch_scratch.len() >= reduction.checked_mul(4).ok_or(Error::ShapeOverflow)?
            {
                while output_x + 4 <= output_width {
                    let blocked_patches = &mut patch_scratch[..reduction * 4];
                    for position in 0..4 {
                        let patch_start = position * reduction;
                        gather_qconv1d_patch(
                            input_ncl,
                            &mut blocked_patches[patch_start..patch_start + reduction],
                            params,
                            batch,
                            output_x + position,
                        )?;
                    }
                    let lhs_sums = [
                        sum_u8(&blocked_patches[..reduction]),
                        sum_u8(&blocked_patches[reduction..reduction * 2]),
                        sum_u8(&blocked_patches[reduction * 2..reduction * 3]),
                        sum_u8(&blocked_patches[reduction * 3..reduction * 4]),
                    ];
                    for output_channel in 0..params.output_channels {
                        let weights_start = output_channel * reduction;
                        let weights = &weights_oki[weights_start..weights_start + reduction];
                        let raw = unsafe { raw_dot4_avx_vnni(blocked_patches, reduction, weights) };
                        let weight_sum = params
                            .weight_row_sums
                            .map_or_else(|| sum_i8(weights), |sums| sums[output_channel]);
                        let weight_zero_point = params.weight_zero_points.get(output_channel);
                        let output_start = (batch * params.output_channels + output_channel)
                            * output_width
                            + output_x;
                        for position in 0..4 {
                            output_ncl[output_start + position] = apply_zero_points(
                                raw[position],
                                lhs_sums[position],
                                weight_sum,
                                reduction,
                                params.input_zero_point,
                                weight_zero_point,
                            );
                        }
                    }
                    output_x += 4;
                }
            }

            while output_x < output_width {
                let patch = &mut patch_scratch[..reduction];
                gather_qconv1d_patch(input_ncl, patch, params, batch, output_x)?;
                let lhs_sum = sum_u8(patch);
                for output_channel in 0..params.output_channels {
                    let weights_start = output_channel * reduction;
                    let weights = &weights_oki[weights_start..weights_start + reduction];
                    let weight_sum = params
                        .weight_row_sums
                        .map_or_else(|| sum_i8(weights), |sums| sums[output_channel]);
                    let weight_zero_point = params.weight_zero_points.get(output_channel);
                    let value = apply_zero_points(
                        raw_dot(patch, weights, lane),
                        lhs_sum,
                        weight_sum,
                        reduction,
                        params.input_zero_point,
                        weight_zero_point,
                    );
                    let output_index =
                        (batch * params.output_channels + output_channel) * output_width + output_x;
                    output_ncl[output_index] = value;
                }
                output_x += 1;
            }
        }
        Ok(())
    }
}

fn gather_qconv1d_patch(
    input_ncl: &[u8],
    patch: &mut [u8],
    params: QConv1dParams<'_>,
    batch: usize,
    output_x: usize,
) -> Result<(), Error> {
    let output_origin = output_x
        .checked_mul(params.stride)
        .ok_or(Error::ShapeOverflow)?;
    for kernel_x in 0..params.kernel_width {
        let padded_x = kernel_x
            .checked_mul(params.dilation)
            .and_then(|offset| output_origin.checked_add(offset))
            .ok_or(Error::ShapeOverflow)?;
        let patch_start = kernel_x * params.input_channels;
        let patch_slice = &mut patch[patch_start..patch_start + params.input_channels];
        let Some(input_x) = padded_x.checked_sub(params.pad_left) else {
            patch_slice.fill(params.input_zero_point);
            continue;
        };
        if input_x >= params.input_width {
            patch_slice.fill(params.input_zero_point);
            continue;
        }
        for (input_channel, value) in patch_slice.iter_mut().enumerate() {
            let input_index =
                (batch * params.input_channels + input_channel) * params.input_width + input_x;
            *value = input_ncl[input_index];
        }
    }
    Ok(())
}

fn gather_qconv1d_patch_f32(
    input_ncl: &[f32],
    patch: &mut [u8],
    params: QConv1dParams<'_>,
    quantization: DynamicQuantization,
    batch: usize,
    output_x: usize,
) -> Result<(), Error> {
    let output_origin = output_x
        .checked_mul(params.stride)
        .ok_or(Error::ShapeOverflow)?;
    for kernel_x in 0..params.kernel_width {
        let padded_x = kernel_x
            .checked_mul(params.dilation)
            .and_then(|offset| output_origin.checked_add(offset))
            .ok_or(Error::ShapeOverflow)?;
        let patch_start = kernel_x * params.input_channels;
        let patch_slice = &mut patch[patch_start..patch_start + params.input_channels];
        let Some(input_x) = padded_x.checked_sub(params.pad_left) else {
            patch_slice.fill(params.input_zero_point);
            continue;
        };
        if input_x >= params.input_width {
            patch_slice.fill(params.input_zero_point);
            continue;
        }
        for (input_channel, value) in patch_slice.iter_mut().enumerate() {
            let input_index =
                (batch * params.input_channels + input_channel) * params.input_width + input_x;
            *value = quant::quantize_value_u8(input_ncl[input_index], quantization);
        }
    }
    Ok(())
}

fn map_quantization_error(error: QuantizationError) -> Error {
    match error {
        QuantizationError::NonFiniteInput => Error::NonFiniteInput,
        QuantizationError::InvalidScale => Error::InvalidScale,
        _ => Error::InvalidQuantization,
    }
}

/// Shift one ONNX `u8` weight or zero point into the signed VNNI domain.
///
/// The mapping is exact because `signed(w) - signed(zp) == w - zp` for all
/// `u8` values. It changes representation only; it does not requantize.
pub const fn signed_u8_zero_point(value: u8) -> i8 {
    (value ^ 0x80) as i8
}

/// Pack ONNX Conv1D weights from `[output, input, kernel]` `u8` into the
/// CPU-native `[output, kernel, input]` signed domain and compute row sums.
pub fn pack_conv1d_weights_u8(
    weights_ock: &[u8],
    output_channels: usize,
    input_channels: usize,
    kernel_width: usize,
    packed_oki: &mut [i8],
    packed_row_sums: &mut [i32],
) -> Result<(), Error> {
    if output_channels == 0 || input_channels == 0 {
        return Err(Error::EmptyMatrix);
    }
    if kernel_width == 0 {
        return Err(Error::EmptyReduction);
    }
    let reduction = input_channels
        .checked_mul(kernel_width)
        .ok_or(Error::ShapeOverflow)?;
    let elements = output_channels
        .checked_mul(reduction)
        .ok_or(Error::ShapeOverflow)?;
    if weights_ock.len() < elements {
        return Err(Error::RhsTooSmall);
    }
    if packed_oki.len() < elements {
        return Err(Error::OutputTooSmall);
    }
    if packed_row_sums.len() < output_channels {
        return Err(Error::RowSumsTooSmall);
    }

    for (output_channel, packed_row_sum) in
        packed_row_sums.iter_mut().take(output_channels).enumerate()
    {
        let mut row_sum = 0i32;
        for kernel_x in 0..kernel_width {
            for input_channel in 0..input_channels {
                let source =
                    (output_channel * input_channels + input_channel) * kernel_width + kernel_x;
                let destination =
                    (output_channel * kernel_width + kernel_x) * input_channels + input_channel;
                let packed = signed_u8_zero_point(weights_ock[source]);
                packed_oki[destination] = packed;
                row_sum = row_sum.wrapping_add(i32::from(packed));
            }
        }
        *packed_row_sum = row_sum;
    }
    Ok(())
}

/// Pack an ONNX MatMulInteger weight from `[k, n]` into the CPU-native
/// transposed `[n, k]` layout and compute one signed row sum per output.
///
/// The pinned artifact preserves canonical ONNX initializer bytes. Calling
/// this once while warming the model produces the layout accepted by
/// [`Dispatcher::qgemm`] without changing or duplicating quantization. The
/// packed bytes remain signed `i8`; only their order changes.
pub fn pack_matmul_weights_i8(
    weights_kn: &[i8],
    k: usize,
    n: usize,
    packed_nk: &mut [i8],
    packed_row_sums: &mut [i32],
) -> Result<(), Error> {
    if n == 0 {
        return Err(Error::EmptyMatrix);
    }
    if k == 0 {
        return Err(Error::EmptyReduction);
    }
    let elements = k.checked_mul(n).ok_or(Error::ShapeOverflow)?;
    if weights_kn.len() < elements {
        return Err(Error::RhsTooSmall);
    }
    if packed_nk.len() < elements {
        return Err(Error::OutputTooSmall);
    }
    if packed_row_sums.len() < n {
        return Err(Error::RowSumsTooSmall);
    }

    for (output, packed_row_sum) in packed_row_sums.iter_mut().take(n).enumerate() {
        let mut row_sum = 0i32;
        let destination = output * k;
        for reduction in 0..k {
            let value = weights_kn[reduction * n + output];
            packed_nk[destination + reduction] = value;
            row_sum = row_sum.wrapping_add(i32::from(value));
        }
        *packed_row_sum = row_sum;
    }
    Ok(())
}

/// Compute raw signed row sums for an offline/native weight layout.
pub fn prepare_rhs_row_sums(
    rhs_transposed: &[i8],
    n: usize,
    k: usize,
    output: &mut [i32],
) -> Result<(), Error> {
    if n == 0 {
        return Err(Error::EmptyMatrix);
    }
    if k == 0 {
        return Err(Error::EmptyReduction);
    }
    let rhs_len = n.checked_mul(k).ok_or(Error::ShapeOverflow)?;
    if rhs_transposed.len() < rhs_len {
        return Err(Error::RhsTooSmall);
    }
    if output.len() < n {
        return Err(Error::OutputTooSmall);
    }
    for row in 0..n {
        output[row] = sum_i8(&rhs_transposed[row * k..(row + 1) * k]);
    }
    Ok(())
}

/// Apply ONNX-style per-output scales and optional bias to QGEMM accumulators.
pub fn dequantize_per_output(
    input: &[i32],
    m: usize,
    n: usize,
    lhs_scale: f32,
    rhs_scales: &[f32],
    bias: Option<&[f32]>,
    output: &mut [f32],
) -> Result<(), Error> {
    if m == 0 || n == 0 {
        return Err(Error::EmptyMatrix);
    }
    let elements = m.checked_mul(n).ok_or(Error::ShapeOverflow)?;
    if input.len() < elements {
        return Err(Error::LhsTooSmall);
    }
    if output.len() < elements {
        return Err(Error::OutputTooSmall);
    }
    if rhs_scales.len() < n {
        return Err(Error::ScalesTooSmall);
    }
    if bias.is_some_and(|values| values.len() < n) {
        return Err(Error::BiasTooSmall);
    }
    for row in 0..m {
        for column in 0..n {
            let index = row * n + column;
            output[index] = input[index] as f32 * (lhs_scale * rhs_scales[column])
                + bias.map_or(0.0, |values| values[column]);
        }
    }
    Ok(())
}

#[inline]
fn validate_dot(lhs: &[u8], rhs: &[i8]) -> Result<(), Error> {
    if lhs.is_empty() {
        Err(Error::EmptyReduction)
    } else if lhs.len() != rhs.len() {
        Err(Error::LengthMismatch)
    } else {
        Ok(())
    }
}

#[inline]
fn apply_zero_points(
    raw: i32,
    lhs_sum: i32,
    rhs_sum: i32,
    k: usize,
    lhs_zero_point: u8,
    rhs_zero_point: i8,
) -> i32 {
    let lhs_zero_point = i32::from(lhs_zero_point);
    let rhs_zero_point = i32::from(rhs_zero_point);
    raw.wrapping_sub(rhs_zero_point.wrapping_mul(lhs_sum))
        .wrapping_sub(lhs_zero_point.wrapping_mul(rhs_sum))
        .wrapping_add(
            (k as i32)
                .wrapping_mul(lhs_zero_point)
                .wrapping_mul(rhs_zero_point),
        )
}

#[inline]
fn raw_dot(lhs: &[u8], rhs: &[i8], lane: Lane) -> i32 {
    match lane {
        Lane::Scalar => raw_dot_scalar(lhs, rhs),
        #[cfg(target_arch = "x86_64")]
        Lane::Avx2 => unsafe { raw_dot_avx2(lhs, rhs) },
        #[cfg(target_arch = "x86_64")]
        Lane::AvxVnni => unsafe { raw_dot_avx_vnni(lhs, rhs) },
        #[cfg(not(target_arch = "x86_64"))]
        Lane::Avx2 | Lane::AvxVnni => unreachable!(),
    }
}

#[inline]
fn raw_dot_and_sums(lhs: &[u8], rhs: &[i8], lane: Lane) -> (i32, i32, i32) {
    match lane {
        Lane::Scalar => raw_dot_and_sums_scalar(lhs, rhs),
        #[cfg(target_arch = "x86_64")]
        Lane::Avx2 => unsafe { raw_dot_and_sums_avx2(lhs, rhs) },
        #[cfg(target_arch = "x86_64")]
        Lane::AvxVnni => unsafe { raw_dot_and_sums_avx_vnni(lhs, rhs) },
        #[cfg(not(target_arch = "x86_64"))]
        Lane::Avx2 | Lane::AvxVnni => unreachable!(),
    }
}

fn raw_dot_scalar(lhs: &[u8], rhs: &[i8]) -> i32 {
    lhs.iter()
        .copied()
        .zip(rhs.iter().copied())
        .fold(0i32, |sum, (lhs, rhs)| sum.wrapping_add(i32::from(lhs).wrapping_mul(i32::from(rhs))))
}

fn raw_dot_and_sums_scalar(lhs: &[u8], rhs: &[i8]) -> (i32, i32, i32) {
    lhs.iter().copied().zip(rhs.iter().copied()).fold(
        (0i32, 0i32, 0i32),
        |(dot, lhs_sum, rhs_sum), (lhs, rhs)| {
            (
                dot.wrapping_add(i32::from(lhs).wrapping_mul(i32::from(rhs))),
                lhs_sum.wrapping_add(i32::from(lhs)),
                rhs_sum.wrapping_add(i32::from(rhs)),
            )
        },
    )
}

fn sum_u8(values: &[u8]) -> i32 {
    values
        .iter()
        .copied()
        .fold(0i32, |sum, value| sum.wrapping_add(i32::from(value)))
}

fn sum_i8(values: &[i8]) -> i32 {
    values
        .iter()
        .copied()
        .fold(0i32, |sum, value| sum.wrapping_add(i32::from(value)))
}

#[cfg(target_arch = "x86_64")]
fn detect_cpu_capabilities() -> CpuCapabilities {
    use core::arch::x86_64::{__cpuid, __cpuid_count};

    const CPUID_1_ECX_OSXSAVE: u32 = 1 << 27;
    const CPUID_1_ECX_AVX: u32 = 1 << 28;
    const CPUID_7_0_EBX_AVX2: u32 = 1 << 5;
    const CPUID_7_1_EAX_AVX_VNNI: u32 = 1 << 4;
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

    let leaf_7_0 = __cpuid_count(7, 0);
    let avx2 = leaf_7_0.ebx & CPUID_7_0_EBX_AVX2 != 0;
    let avx_vnni =
        avx2 && leaf_7_0.eax >= 1 && __cpuid_count(7, 1).eax & CPUID_7_1_EAX_AVX_VNNI != 0;
    CpuCapabilities {
        ymm_state: true,
        avx2,
        avx_vnni,
    }
}

#[cfg(not(target_arch = "x86_64"))]
const fn detect_cpu_capabilities() -> CpuCapabilities {
    CpuCapabilities {
        ymm_state: false,
        avx2: false,
        avx_vnni: false,
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

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn raw_dot_avx2(lhs: &[u8], rhs: &[i8]) -> i32 {
    use core::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_castsi256_si128, _mm256_cvtepi8_epi16,
        _mm256_cvtepu8_epi16, _mm256_extracti128_si256, _mm256_loadu_si256, _mm256_madd_epi16,
        _mm256_setzero_si256, _mm256_storeu_si256,
    };

    let mut index = 0usize;
    let mut accumulator = _mm256_setzero_si256();
    while index + 32 <= lhs.len() {
        let lhs_bytes = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_bytes = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };

        let lhs_low = _mm256_cvtepu8_epi16(_mm256_castsi256_si128(lhs_bytes));
        let lhs_high = _mm256_cvtepu8_epi16(_mm256_extracti128_si256::<1>(lhs_bytes));
        let rhs_low = _mm256_cvtepi8_epi16(_mm256_castsi256_si128(rhs_bytes));
        let rhs_high = _mm256_cvtepi8_epi16(_mm256_extracti128_si256::<1>(rhs_bytes));
        accumulator = _mm256_add_epi32(
            accumulator,
            _mm256_add_epi32(
                _mm256_madd_epi16(lhs_low, rhs_low),
                _mm256_madd_epi16(lhs_high, rhs_high),
            ),
        );
        index += 32;
    }

    let mut lanes = [0i32; 8];
    unsafe { _mm256_storeu_si256(lanes.as_mut_ptr().cast::<__m256i>(), accumulator) };
    let mut sum = lanes.into_iter().fold(0i32, i32::wrapping_add);
    while index < lhs.len() {
        sum = sum.wrapping_add(i32::from(lhs[index]).wrapping_mul(i32::from(rhs[index])));
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn raw_dot_and_sums_avx2(lhs: &[u8], rhs: &[i8]) -> (i32, i32, i32) {
    use core::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_castsi256_si128, _mm256_cvtepi8_epi16,
        _mm256_cvtepu8_epi16, _mm256_extracti128_si256, _mm256_loadu_si256, _mm256_madd_epi16,
        _mm256_set1_epi16, _mm256_setzero_si256, _mm256_storeu_si256,
    };

    let ones = _mm256_set1_epi16(1);
    let mut index = 0usize;
    let mut dot_accumulator = _mm256_setzero_si256();
    let mut lhs_accumulator = _mm256_setzero_si256();
    let mut rhs_accumulator = _mm256_setzero_si256();
    while index + 32 <= lhs.len() {
        let lhs_bytes = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_bytes = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };
        let lhs_low = _mm256_cvtepu8_epi16(_mm256_castsi256_si128(lhs_bytes));
        let lhs_high = _mm256_cvtepu8_epi16(_mm256_extracti128_si256::<1>(lhs_bytes));
        let rhs_low = _mm256_cvtepi8_epi16(_mm256_castsi256_si128(rhs_bytes));
        let rhs_high = _mm256_cvtepi8_epi16(_mm256_extracti128_si256::<1>(rhs_bytes));

        dot_accumulator = _mm256_add_epi32(
            dot_accumulator,
            _mm256_add_epi32(
                _mm256_madd_epi16(lhs_low, rhs_low),
                _mm256_madd_epi16(lhs_high, rhs_high),
            ),
        );
        lhs_accumulator = _mm256_add_epi32(
            lhs_accumulator,
            _mm256_add_epi32(_mm256_madd_epi16(lhs_low, ones), _mm256_madd_epi16(lhs_high, ones)),
        );
        rhs_accumulator = _mm256_add_epi32(
            rhs_accumulator,
            _mm256_add_epi32(_mm256_madd_epi16(rhs_low, ones), _mm256_madd_epi16(rhs_high, ones)),
        );
        index += 32;
    }

    let mut dot_lanes = [0i32; 8];
    let mut lhs_lanes = [0i32; 8];
    let mut rhs_lanes = [0i32; 8];
    unsafe {
        _mm256_storeu_si256(dot_lanes.as_mut_ptr().cast::<__m256i>(), dot_accumulator);
        _mm256_storeu_si256(lhs_lanes.as_mut_ptr().cast::<__m256i>(), lhs_accumulator);
        _mm256_storeu_si256(rhs_lanes.as_mut_ptr().cast::<__m256i>(), rhs_accumulator);
    }
    let mut dot = dot_lanes.into_iter().fold(0i32, i32::wrapping_add);
    let mut lhs_sum = lhs_lanes.into_iter().fold(0i32, i32::wrapping_add);
    let mut rhs_sum = rhs_lanes.into_iter().fold(0i32, i32::wrapping_add);
    while index < lhs.len() {
        dot = dot.wrapping_add(i32::from(lhs[index]).wrapping_mul(i32::from(rhs[index])));
        lhs_sum = lhs_sum.wrapping_add(i32::from(lhs[index]));
        rhs_sum = rhs_sum.wrapping_add(i32::from(rhs[index]));
        index += 1;
    }
    (dot, lhs_sum, rhs_sum)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,avxvnni")]
unsafe fn raw_dot_avx_vnni(lhs: &[u8], rhs: &[i8]) -> i32 {
    use core::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_dpbusd_avx_epi32, _mm256_loadu_si256,
        _mm256_setzero_si256, _mm256_storeu_si256,
    };

    let mut index = 0usize;
    // DPBUSD has a multi-cycle dependency latency. Four independent chains
    // expose enough instruction-level parallelism for the i5-14500T while
    // retaining the exact modulo-2^32 reduction.
    let mut accumulator_0 = _mm256_setzero_si256();
    let mut accumulator_1 = _mm256_setzero_si256();
    let mut accumulator_2 = _mm256_setzero_si256();
    let mut accumulator_3 = _mm256_setzero_si256();
    while index + 128 <= lhs.len() {
        let lhs_0 = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_0 = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };
        let lhs_1 = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index + 32).cast::<__m256i>()) };
        let rhs_1 = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index + 32).cast::<__m256i>()) };
        let lhs_2 = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index + 64).cast::<__m256i>()) };
        let rhs_2 = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index + 64).cast::<__m256i>()) };
        let lhs_3 = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index + 96).cast::<__m256i>()) };
        let rhs_3 = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index + 96).cast::<__m256i>()) };
        accumulator_0 = _mm256_dpbusd_avx_epi32(accumulator_0, lhs_0, rhs_0);
        accumulator_1 = _mm256_dpbusd_avx_epi32(accumulator_1, lhs_1, rhs_1);
        accumulator_2 = _mm256_dpbusd_avx_epi32(accumulator_2, lhs_2, rhs_2);
        accumulator_3 = _mm256_dpbusd_avx_epi32(accumulator_3, lhs_3, rhs_3);
        index += 128;
    }
    let mut accumulator = _mm256_add_epi32(
        _mm256_add_epi32(accumulator_0, accumulator_1),
        _mm256_add_epi32(accumulator_2, accumulator_3),
    );
    while index + 32 <= lhs.len() {
        let lhs_bytes = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_bytes = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };
        accumulator = _mm256_dpbusd_avx_epi32(accumulator, lhs_bytes, rhs_bytes);
        index += 32;
    }

    let mut lanes = [0i32; 8];
    unsafe { _mm256_storeu_si256(lanes.as_mut_ptr().cast::<__m256i>(), accumulator) };
    let mut sum = lanes.into_iter().fold(0i32, i32::wrapping_add);
    while index < lhs.len() {
        sum = sum.wrapping_add(i32::from(lhs[index]).wrapping_mul(i32::from(rhs[index])));
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,avxvnni")]
unsafe fn raw_dot4_avx_vnni(patches: &[u8], patch_stride: usize, rhs: &[i8]) -> [i32; 4] {
    use core::arch::x86_64::{
        __m256i, _mm256_dpbusd_avx_epi32, _mm256_loadu_si256, _mm256_setzero_si256,
        _mm256_storeu_si256,
    };

    debug_assert!(patch_stride >= rhs.len());
    debug_assert!(patches.len() >= patch_stride * 4);
    let mut accumulators = [
        _mm256_setzero_si256(),
        _mm256_setzero_si256(),
        _mm256_setzero_si256(),
        _mm256_setzero_si256(),
    ];
    let mut index = 0usize;
    while index + 32 <= rhs.len() {
        let weights = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };
        for (position, accumulator) in accumulators.iter_mut().enumerate() {
            let activation = unsafe {
                _mm256_loadu_si256(
                    patches
                        .as_ptr()
                        .add(position * patch_stride + index)
                        .cast::<__m256i>(),
                )
            };
            *accumulator = _mm256_dpbusd_avx_epi32(*accumulator, activation, weights);
        }
        index += 32;
    }

    let mut sums = [0i32; 4];
    for position in 0..4 {
        let mut lanes = [0i32; 8];
        unsafe {
            _mm256_storeu_si256(lanes.as_mut_ptr().cast::<__m256i>(), accumulators[position]);
        }
        sums[position] = lanes.into_iter().fold(0i32, i32::wrapping_add);
    }
    while index < rhs.len() {
        for position in 0..4 {
            sums[position] = sums[position].wrapping_add(
                i32::from(patches[position * patch_stride + index])
                    .wrapping_mul(i32::from(rhs[index])),
            );
        }
        index += 1;
    }
    sums
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,avxvnni")]
unsafe fn raw_dot_and_sums_avx_vnni(lhs: &[u8], rhs: &[i8]) -> (i32, i32, i32) {
    use core::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_castsi256_si128, _mm256_cvtepi8_epi16,
        _mm256_cvtepu8_epi16, _mm256_dpbusd_avx_epi32, _mm256_extracti128_si256,
        _mm256_loadu_si256, _mm256_madd_epi16, _mm256_set1_epi16, _mm256_setzero_si256,
        _mm256_storeu_si256,
    };

    let ones = _mm256_set1_epi16(1);
    let mut index = 0usize;
    let mut dot_accumulator = _mm256_setzero_si256();
    let mut lhs_accumulator = _mm256_setzero_si256();
    let mut rhs_accumulator = _mm256_setzero_si256();
    while index + 32 <= lhs.len() {
        let lhs_bytes = unsafe { _mm256_loadu_si256(lhs.as_ptr().add(index).cast::<__m256i>()) };
        let rhs_bytes = unsafe { _mm256_loadu_si256(rhs.as_ptr().add(index).cast::<__m256i>()) };
        dot_accumulator = _mm256_dpbusd_avx_epi32(dot_accumulator, lhs_bytes, rhs_bytes);

        let lhs_low = _mm256_cvtepu8_epi16(_mm256_castsi256_si128(lhs_bytes));
        let lhs_high = _mm256_cvtepu8_epi16(_mm256_extracti128_si256::<1>(lhs_bytes));
        let rhs_low = _mm256_cvtepi8_epi16(_mm256_castsi256_si128(rhs_bytes));
        let rhs_high = _mm256_cvtepi8_epi16(_mm256_extracti128_si256::<1>(rhs_bytes));
        lhs_accumulator = _mm256_add_epi32(
            lhs_accumulator,
            _mm256_add_epi32(_mm256_madd_epi16(lhs_low, ones), _mm256_madd_epi16(lhs_high, ones)),
        );
        rhs_accumulator = _mm256_add_epi32(
            rhs_accumulator,
            _mm256_add_epi32(_mm256_madd_epi16(rhs_low, ones), _mm256_madd_epi16(rhs_high, ones)),
        );
        index += 32;
    }

    let mut dot_lanes = [0i32; 8];
    let mut lhs_lanes = [0i32; 8];
    let mut rhs_lanes = [0i32; 8];
    unsafe {
        _mm256_storeu_si256(dot_lanes.as_mut_ptr().cast::<__m256i>(), dot_accumulator);
        _mm256_storeu_si256(lhs_lanes.as_mut_ptr().cast::<__m256i>(), lhs_accumulator);
        _mm256_storeu_si256(rhs_lanes.as_mut_ptr().cast::<__m256i>(), rhs_accumulator);
    }
    let mut dot = dot_lanes.into_iter().fold(0i32, i32::wrapping_add);
    let mut lhs_sum = lhs_lanes.into_iter().fold(0i32, i32::wrapping_add);
    let mut rhs_sum = rhs_lanes.into_iter().fold(0i32, i32::wrapping_add);
    while index < lhs.len() {
        dot = dot.wrapping_add(i32::from(lhs[index]).wrapping_mul(i32::from(rhs[index])));
        lhs_sum = lhs_sum.wrapping_add(i32::from(lhs[index]));
        rhs_sum = rhs_sum.wrapping_add(i32::from(rhs[index]));
        index += 1;
    }
    (dot, lhs_sum, rhs_sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    fn vectors(len: usize) -> (Vec<u8>, Vec<i8>) {
        let mut state = 0x6A09_E667_F3BC_C909u64;
        let mut lhs = Vec::with_capacity(len);
        let mut rhs = Vec::with_capacity(len);
        for index in 0..len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            lhs.push((state as u8).wrapping_add(index as u8));
            rhs.push((state.rotate_left(19) as u8).wrapping_add((index * 29) as u8) as i8);
        }
        (lhs, rhs)
    }

    fn available_lanes(dispatcher: Dispatcher) -> impl Iterator<Item = Lane> {
        [Lane::Scalar, Lane::Avx2, Lane::AvxVnni]
            .into_iter()
            .filter(move |lane| dispatcher.supports(*lane))
    }

    #[test]
    fn every_available_lane_matches_scalar_for_tails_and_zero_points() {
        let dispatcher = Dispatcher::detect();
        for len in [
            1, 2, 3, 4, 15, 16, 31, 32, 33, 63, 64, 65, 127, 257, 768, 1024, 11264,
        ] {
            let (lhs, rhs) = vectors(len);
            for (lhs_zero_point, rhs_zero_point) in [(0, 0), (128, 0), (247, -13), (255, 127)] {
                let expected = dispatcher
                    .dot_with_lane(&lhs, &rhs, lhs_zero_point, rhs_zero_point, Lane::Scalar)
                    .unwrap();
                for lane in available_lanes(dispatcher) {
                    assert_eq!(
                        dispatcher
                            .dot_with_lane(&lhs, &rhs, lhs_zero_point, rhs_zero_point, lane)
                            .unwrap(),
                        expected,
                        "lane={lane:?} len={len} lhs_zp={lhs_zero_point} rhs_zp={rhs_zero_point}"
                    );
                }
            }
        }
    }

    #[test]
    fn avx2_full_range_input_does_not_saturate_intermediate_pairs() {
        let dispatcher = Dispatcher::detect();
        if !dispatcher.supports(Lane::Avx2) {
            return;
        }
        let lhs = [255u8; 64];
        let rhs = [127i8; 64];
        let expected = 255 * 127 * 64;
        assert_eq!(
            dispatcher
                .dot_with_lane(&lhs, &rhs, 0, 0, Lane::Avx2)
                .unwrap(),
            expected
        );
    }

    #[test]
    fn qgemm_prepared_and_inline_sums_match_every_lane() {
        const M: usize = 3;
        const N: usize = 5;
        const K: usize = 65;
        let dispatcher = Dispatcher::detect();
        let (lhs, _) = vectors(M * K);
        let (_, rhs) = vectors(N * K);
        let rhs_zero_points = [-17, 0, 9, 127, -128];
        let mut rhs_sums = [0i32; N];
        prepare_rhs_row_sums(&rhs, N, K, &mut rhs_sums).unwrap();

        let mut expected = [0i32; M * N];
        let scalar_params = QGemmParams {
            m: M,
            n: N,
            k: K,
            lhs_zero_point: 193,
            rhs_zero_points: RhsZeroPoints::PerOutput(&rhs_zero_points),
            rhs_row_sums: Some(&rhs_sums),
        };
        dispatcher
            .qgemm_with_lane(&lhs, &rhs, &mut expected, scalar_params, Lane::Scalar)
            .unwrap();

        for lane in available_lanes(dispatcher) {
            let mut prepared = [0i32; M * N];
            dispatcher
                .qgemm_with_lane(&lhs, &rhs, &mut prepared, scalar_params, lane)
                .unwrap();
            assert_eq!(prepared, expected, "prepared lane={lane:?}");

            let mut inline = [0i32; M * N];
            let inline_params = QGemmParams {
                rhs_row_sums: None,
                ..scalar_params
            };
            dispatcher
                .qgemm_with_lane(&lhs, &rhs, &mut inline, inline_params, lane)
                .unwrap();
            assert_eq!(inline, expected, "inline lane={lane:?}");
        }
    }

    #[test]
    fn automatic_dispatch_is_supported_and_exact() {
        let dispatcher = Dispatcher::detect();
        let (lhs, rhs) = vectors(769);
        let expected = dispatcher
            .dot_with_lane(&lhs, &rhs, 131, -7, Lane::Scalar)
            .unwrap();
        let (observed, lane) = dispatcher.dot(&lhs, &rhs, 131, -7).unwrap();
        assert!(dispatcher.supports(lane));
        assert_eq!(lane, dispatcher.best_lane());
        assert_eq!(observed, expected);
    }

    #[test]
    fn capability_contract_never_admits_vnni_without_avx2_and_ymm() {
        let capabilities = Dispatcher::detect().capabilities();
        if capabilities.avx_vnni() {
            assert!(capabilities.avx2());
            assert!(capabilities.ymm_state());
        }
        if capabilities.avx2() {
            assert!(capabilities.ymm_state());
        }
    }

    #[test]
    fn dequantization_applies_per_output_scale_and_bias() {
        let input = [100i32, -200, 300, -400];
        let mut output = [0.0f32; 4];
        dequantize_per_output(&input, 2, 2, 0.25, &[0.5, 2.0], Some(&[1.0, -3.0]), &mut output)
            .unwrap();
        assert_eq!(output, [13.5, -103.0, 38.5, -203.0]);
    }

    #[test]
    fn validation_rejects_bad_shapes_and_unavailable_lanes() {
        let dispatcher = Dispatcher::detect();
        assert_eq!(dispatcher.dot(&[], &[], 0, 0), Err(Error::EmptyReduction));
        assert_eq!(dispatcher.dot(&[1, 2], &[1], 0, 0), Err(Error::LengthMismatch));

        let params = QGemmParams {
            m: 1,
            n: 2,
            k: 3,
            lhs_zero_point: 0,
            rhs_zero_points: RhsZeroPoints::PerOutput(&[0]),
            rhs_row_sums: None,
        };
        assert_eq!(
            dispatcher.qgemm(&[0; 3], &[0; 6], &mut [0; 2], params),
            Err(Error::ZeroPointsTooSmall)
        );

        let unavailable = [Lane::AvxVnni, Lane::Avx2]
            .into_iter()
            .find(|lane| !dispatcher.supports(*lane));
        if let Some(lane) = unavailable {
            assert_eq!(
                dispatcher.dot_with_lane(&[1], &[1], 0, 0, lane),
                Err(Error::UnsupportedLane)
            );
        }
    }

    #[test]
    fn scalar_reference_matches_direct_centered_arithmetic() {
        let dispatcher = Dispatcher::detect();
        let lhs = [0u8, 1, 127, 128, 254, 255];
        let rhs = [-128i8, -127, -1, 0, 126, 127];
        let lhs_zero_point = 123u8;
        let rhs_zero_point = -19i8;
        let direct = lhs.iter().zip(rhs.iter()).fold(0i32, |sum, (&lhs, &rhs)| {
            sum.wrapping_add(
                (i32::from(lhs) - i32::from(lhs_zero_point))
                    .wrapping_mul(i32::from(rhs) - i32::from(rhs_zero_point)),
            )
        });
        assert_eq!(
            dispatcher
                .dot_with_lane(&lhs, &rhs, lhs_zero_point, rhs_zero_point, Lane::Scalar,)
                .unwrap(),
            direct
        );
    }

    #[test]
    fn matmulinteger_kn_pack_transposes_and_precomputes_rows() {
        const K: usize = 3;
        const N: usize = 4;

        let weights_kn = [
            -7i8, 2, 11, -13, // k=0
            5, -17, 19, 23, // k=1
            -29, 31, -37, 41, // k=2
        ];
        let mut packed_nk = [0i8; K * N];
        let mut row_sums = [0i32; N];
        pack_matmul_weights_i8(&weights_kn, K, N, &mut packed_nk, &mut row_sums).unwrap();

        assert_eq!(packed_nk, [-7, 5, -29, 2, -17, 31, 11, 19, -37, -13, 23, 41]);
        assert_eq!(row_sums, [-31, 16, -7, 51]);

        let input = [3u8, 127, 251];
        let zero_points = [-3i8, 0, 7, -11];
        let params = QGemmParams {
            m: 1,
            n: N,
            k: K,
            lhs_zero_point: 113,
            rhs_zero_points: RhsZeroPoints::PerOutput(&zero_points),
            rhs_row_sums: Some(&row_sums),
        };
        let mut output = [0i32; N];
        Dispatcher::detect()
            .qgemm_with_lane(&input, &packed_nk, &mut output, params, Lane::Scalar)
            .unwrap();
        let expected = core::array::from_fn(|column| {
            (0..K).fold(0i32, |sum, reduction| {
                sum.wrapping_add(
                    (i32::from(input[reduction]) - i32::from(params.lhs_zero_point)).wrapping_mul(
                        i32::from(weights_kn[reduction * N + column])
                            - i32::from(zero_points[column]),
                    ),
                )
            })
        });
        assert_eq!(output, expected);
    }

    #[test]
    fn matmulinteger_kn_pack_validation_is_fail_closed() {
        let source = [1i8; 6];
        let mut packed = [0i8; 6];
        let mut sums = [0i32; 3];
        assert_eq!(
            pack_matmul_weights_i8(&source, 0, 3, &mut packed, &mut sums),
            Err(Error::EmptyReduction)
        );
        assert_eq!(
            pack_matmul_weights_i8(&source, 2, 0, &mut packed, &mut sums),
            Err(Error::EmptyMatrix)
        );
        assert_eq!(
            pack_matmul_weights_i8(&source[..5], 2, 3, &mut packed, &mut sums),
            Err(Error::RhsTooSmall)
        );
        assert_eq!(
            pack_matmul_weights_i8(&source, 2, 3, &mut packed[..5], &mut sums),
            Err(Error::OutputTooSmall)
        );
        assert_eq!(
            pack_matmul_weights_i8(&source, 2, 3, &mut packed, &mut sums[..2]),
            Err(Error::RowSumsTooSmall)
        );
    }

    #[test]
    fn convinteger_u8_pack_and_every_lane_match_onnx_edges() {
        const BATCH: usize = 2;
        const INPUT_CHANNELS: usize = 5;
        const INPUT_WIDTH: usize = 9;
        const OUTPUT_CHANNELS: usize = 3;
        const KERNEL_WIDTH: usize = 3;

        let dispatcher = Dispatcher::detect();
        let input = (0..BATCH * INPUT_CHANNELS * INPUT_WIDTH)
            .map(|index| (index as u8).wrapping_mul(37).wrapping_add(11))
            .collect::<Vec<_>>();
        let weights_ock = (0..OUTPUT_CHANNELS * INPUT_CHANNELS * KERNEL_WIDTH)
            .map(|index| (index as u8).wrapping_mul(53).wrapping_add(7))
            .collect::<Vec<_>>();
        let weight_zero_points_u8 = [0u8, 73, 255];
        let weight_zero_points = weight_zero_points_u8.map(signed_u8_zero_point);
        let mut packed = vec![0i8; weights_ock.len()];
        let mut row_sums = [0i32; OUTPUT_CHANNELS];
        pack_conv1d_weights_u8(
            &weights_ock,
            OUTPUT_CHANNELS,
            INPUT_CHANNELS,
            KERNEL_WIDTH,
            &mut packed,
            &mut row_sums,
        )
        .unwrap();

        assert_eq!(signed_u8_zero_point(0), -128);
        assert_eq!(signed_u8_zero_point(128), 0);
        assert_eq!(signed_u8_zero_point(255), 127);
        for output_channel in 0..OUTPUT_CHANNELS {
            for kernel_x in 0..KERNEL_WIDTH {
                for input_channel in 0..INPUT_CHANNELS {
                    let source =
                        (output_channel * INPUT_CHANNELS + input_channel) * KERNEL_WIDTH + kernel_x;
                    let destination =
                        (output_channel * KERNEL_WIDTH + kernel_x) * INPUT_CHANNELS + input_channel;
                    assert_eq!(packed[destination], signed_u8_zero_point(weights_ock[source]));
                }
            }
        }

        let params = QConv1dParams {
            batch: BATCH,
            input_channels: INPUT_CHANNELS,
            input_width: INPUT_WIDTH,
            output_channels: OUTPUT_CHANNELS,
            kernel_width: KERNEL_WIDTH,
            stride: 2,
            dilation: 2,
            pad_left: 2,
            pad_right: 3,
            input_zero_point: 131,
            weight_zero_points: RhsZeroPoints::PerOutput(&weight_zero_points),
            weight_row_sums: Some(&row_sums),
        };
        let output_width = params.output_width().unwrap();
        assert_eq!(output_width, 5);

        let mut expected = vec![0i32; BATCH * OUTPUT_CHANNELS * output_width];
        for batch in 0..BATCH {
            for output_channel in 0..OUTPUT_CHANNELS {
                for output_x in 0..output_width {
                    let mut accumulator = 0i32;
                    for kernel_x in 0..KERNEL_WIDTH {
                        let padded_x = output_x * params.stride + kernel_x * params.dilation;
                        let Some(input_x) = padded_x.checked_sub(params.pad_left) else {
                            continue;
                        };
                        if input_x >= INPUT_WIDTH {
                            continue;
                        }
                        for input_channel in 0..INPUT_CHANNELS {
                            let input_index =
                                (batch * INPUT_CHANNELS + input_channel) * INPUT_WIDTH + input_x;
                            let weight_index = (output_channel * INPUT_CHANNELS + input_channel)
                                * KERNEL_WIDTH
                                + kernel_x;
                            accumulator = accumulator.wrapping_add(
                                (i32::from(input[input_index])
                                    - i32::from(params.input_zero_point))
                                .wrapping_mul(
                                    i32::from(weights_ock[weight_index])
                                        - i32::from(weight_zero_points_u8[output_channel]),
                                ),
                            );
                        }
                    }
                    expected
                        [(batch * OUTPUT_CHANNELS + output_channel) * output_width + output_x] =
                        accumulator;
                }
            }
        }

        for lane in available_lanes(dispatcher) {
            let mut observed = vec![0i32; expected.len()];
            let mut scratch = [0u8; INPUT_CHANNELS * KERNEL_WIDTH];
            dispatcher
                .qconv1d_with_lane(&input, &packed, &mut observed, &mut scratch, params, lane)
                .unwrap();
            assert_eq!(observed, expected, "prepared lane={lane:?}");

            if lane == Lane::AvxVnni {
                let mut blocked = vec![0i32; expected.len()];
                let mut blocked_scratch = [0u8; INPUT_CHANNELS * KERNEL_WIDTH * 4];
                dispatcher
                    .qconv1d_with_lane(
                        &input,
                        &packed,
                        &mut blocked,
                        &mut blocked_scratch,
                        params,
                        lane,
                    )
                    .unwrap();
                assert_eq!(blocked, expected, "four-position lane={lane:?}");
            }

            let mut inline = vec![0i32; expected.len()];
            let inline_params = QConv1dParams {
                weight_row_sums: None,
                ..params
            };
            dispatcher
                .qconv1d_with_lane(&input, &packed, &mut inline, &mut scratch, inline_params, lane)
                .unwrap();
            assert_eq!(inline, expected, "inline lane={lane:?}");
        }
    }

    #[test]
    fn convinteger_validation_rejects_zero_stride_and_short_scratch() {
        let dispatcher = Dispatcher::detect();
        let params = QConv1dParams {
            batch: 1,
            input_channels: 2,
            input_width: 4,
            output_channels: 1,
            kernel_width: 3,
            stride: 0,
            dilation: 1,
            pad_left: 1,
            pad_right: 1,
            input_zero_point: 0,
            weight_zero_points: RhsZeroPoints::Scalar(0),
            weight_row_sums: None,
        };
        assert_eq!(params.output_width(), Err(Error::InvalidConvolutionShape));

        let valid = QConv1dParams {
            stride: 1,
            ..params
        };
        assert_eq!(
            dispatcher.qconv1d(&[0; 8], &[0; 6], &mut [0; 4], &mut [0; 5], valid),
            Err(Error::ScratchTooSmall)
        );
    }

    #[test]
    fn prepared_row_sum_validation_is_explicit() {
        let rhs = vec![1i8; 12];
        let mut too_short = [0i32; 2];
        assert_eq!(prepare_rhs_row_sums(&rhs, 3, 4, &mut too_short), Err(Error::OutputTooSmall));
    }
}
