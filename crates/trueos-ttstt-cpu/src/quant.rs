//! Exact allocation-free quantization glue around Kokoro's integer kernels.

/// Errors detected before an output buffer is modified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantizationError {
    EmptyInput,
    OutputTooSmall,
    LengthMismatch,
    ShapeOverflow,
    BiasTooSmall,
    NonFiniteInput,
    InvalidScale,
    IntegerOverflow,
}

/// Scalar outputs of ONNX `DynamicQuantizeLinear` for a `u8` destination.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicQuantization {
    pub scale: f32,
    pub zero_point: u8,
}

/// Execute ONNX `DynamicQuantizeLinear` exactly in the `u8` domain.
///
/// The reduction includes zero, uses f32 arithmetic, and rounds both the zero
/// point and samples to nearest with ties to even. Non-finite input or a scale
/// that is not representable as a positive finite f32 is rejected before
/// `output` is touched; native Kokoro inference is fail-closed rather than
/// allowing exceptional floats to choose implementation-dependent integers.
pub fn dynamic_quantize_linear_u8(
    input: &[f32],
    output: &mut [u8],
) -> Result<DynamicQuantization, QuantizationError> {
    let quantization = dynamic_quantization_parameters(input)?;
    quantize_linear_u8_with_parameters(input, quantization, output)?;
    Ok(quantization)
}

/// Compute the exact ONNX `DynamicQuantizeLinear` scale and zero point
/// without materializing the quantized tensor.
///
/// This lets fused matrix and convolution adapters quantize one bounded row
/// or receptive-field patch at a time while preserving the whole-input
/// reduction semantics.
pub fn dynamic_quantization_parameters(
    input: &[f32],
) -> Result<DynamicQuantization, QuantizationError> {
    if input.is_empty() {
        return Err(QuantizationError::EmptyInput);
    }

    let mut minimum = 0.0f32;
    let mut maximum = 0.0f32;
    for &value in input {
        if !value.is_finite() {
            return Err(QuantizationError::NonFiniteInput);
        }
        minimum = minimum.min(value);
        maximum = maximum.max(value);
    }
    let range = maximum - minimum;
    let scale = if maximum == minimum {
        1.0
    } else {
        range / 255.0f32
    };
    if !scale.is_finite() || scale <= 0.0 {
        return Err(QuantizationError::InvalidScale);
    }
    let zero_point_f32 = round_ties_even_small((-minimum / scale).clamp(0.0, 255.0));
    let zero_point = zero_point_f32 as u8;

    Ok(DynamicQuantization { scale, zero_point })
}

/// Quantize a bounded slice with parameters derived from the complete source
/// tensor by [`dynamic_quantization_parameters`].
///
/// Validation completes before the destination changes, so callers may reuse
/// a small row or patch buffer transactionally.
pub fn quantize_linear_u8_with_parameters(
    input: &[f32],
    quantization: DynamicQuantization,
    output: &mut [u8],
) -> Result<(), QuantizationError> {
    if input.is_empty() {
        return Err(QuantizationError::EmptyInput);
    }
    if output.len() < input.len() {
        return Err(QuantizationError::OutputTooSmall);
    }
    if !quantization.scale.is_finite() || quantization.scale <= 0.0 {
        return Err(QuantizationError::InvalidScale);
    }
    if input.iter().any(|value| !value.is_finite()) {
        return Err(QuantizationError::NonFiniteInput);
    }

    for (&value, quantized) in input.iter().zip(output.iter_mut()) {
        let rounded =
            round_ties_even_small(value / quantization.scale) + f32::from(quantization.zero_point);
        *quantized = rounded.clamp(0.0, 255.0) as u8;
    }
    Ok(())
}

pub(crate) fn quantize_value_u8(value: f32, quantization: DynamicQuantization) -> u8 {
    let rounded =
        round_ties_even_small(value / quantization.scale) + f32::from(quantization.zero_point);
    rounded.clamp(0.0, 255.0) as u8
}

/// Compute the scalar scale used by every ConvInteger epilogue in the pinned
/// Kokoro graph: dynamic activation scale times constant weight scale.
pub fn conv_integer_scale(
    activation_scale: f32,
    weight_scale: f32,
) -> Result<f32, QuantizationError> {
    if !activation_scale.is_finite()
        || !weight_scale.is_finite()
        || activation_scale <= 0.0
        || weight_scale <= 0.0
    {
        return Err(QuantizationError::InvalidScale);
    }
    let scale = activation_scale * weight_scale;
    if !scale.is_finite() || scale <= 0.0 {
        Err(QuantizationError::InvalidScale)
    } else {
        Ok(scale)
    }
}

/// Apply an ONNX `Cast<i32, f32> -> Mul` MatMulInteger epilogue.
///
/// The pinned Kokoro graph contains 148 of these chains. `activation_scale` is
/// scalar and `weight_scales` has one value per output column. Bias is
/// deliberately absent: 136 chains schedule a distinct subsequent `Add`, and
/// the other 12 feed `FastGelu` directly. Keeping the float addition outside
/// this primitive also makes contraction into an FMA impossible here.
///
/// `accumulators` and `output` are row-major `[rows, output_columns]`. Every
/// scale is checked before `output` is modified.
pub fn dequantize_matmul_integer_per_output(
    accumulators: &[i32],
    rows: usize,
    output_columns: usize,
    activation_scale: f32,
    weight_scales: &[f32],
    output: &mut [f32],
) -> Result<(), QuantizationError> {
    if rows == 0 || output_columns == 0 {
        return Err(QuantizationError::EmptyInput);
    }
    let elements = rows
        .checked_mul(output_columns)
        .ok_or(QuantizationError::ShapeOverflow)?;
    if accumulators.len() < elements || output.len() < elements {
        return Err(QuantizationError::LengthMismatch);
    }
    if weight_scales.len() < output_columns {
        return Err(QuantizationError::LengthMismatch);
    }
    for &weight_scale in &weight_scales[..output_columns] {
        conv_integer_scale(activation_scale, weight_scale)?;
    }

    for row in 0..rows {
        let row_start = row * output_columns;
        for column in 0..output_columns {
            // These are intentionally two distinct f32 multiplications. They
            // mirror the scale-producing Mul and the post-Cast Mul in ONNX.
            let combined_scale = activation_scale * weight_scales[column];
            let cast = accumulators[row_start + column] as f32;
            output[row_start + column] = cast * combined_scale;
        }
    }
    Ok(())
}

/// Reproduce Kokoro's runtime bias chain `Div -> Floor -> Cast<i32>`.
///
/// This is deliberately floor, not round-to-nearest. The 80 biased
/// ConvInteger nodes then add these values in i32 before casting and applying
/// the returned combined scale.
pub fn quantize_conv_bias_floor(
    bias: &[f32],
    activation_scale: f32,
    weight_scale: f32,
    output: &mut [i32],
) -> Result<f32, QuantizationError> {
    if bias.is_empty() {
        return Err(QuantizationError::EmptyInput);
    }
    if output.len() < bias.len() {
        return Err(QuantizationError::OutputTooSmall);
    }
    let scale = conv_integer_scale(activation_scale, weight_scale)?;
    for &value in bias {
        if !value.is_finite() {
            return Err(QuantizationError::NonFiniteInput);
        }
        floor_to_i32(value / scale)?;
    }
    for (&value, quantized) in bias.iter().zip(output.iter_mut()) {
        *quantized = floor_to_i32(value / scale)?;
    }
    Ok(scale)
}

/// Apply the pinned ConvInteger `Add<i32> -> Cast<f32> -> Mul` epilogue.
///
/// `accumulators_ncw` and `output_ncw` use `[batch, channel, width]`. Bias is
/// either absent (the seven direct chains) or one quantized value per output
/// channel (the remaining 80). Integer addition intentionally wraps modulo
/// 2^32, matching the integer tensor operation before the f32 cast.
pub fn dequantize_conv_integer_ncw(
    accumulators_ncw: &[i32],
    batch: usize,
    channels: usize,
    width: usize,
    bias_quantized: Option<&[i32]>,
    combined_scale: f32,
    output_ncw: &mut [f32],
) -> Result<(), QuantizationError> {
    if batch == 0 || channels == 0 || width == 0 {
        return Err(QuantizationError::EmptyInput);
    }
    if !combined_scale.is_finite() || combined_scale <= 0.0 {
        return Err(QuantizationError::InvalidScale);
    }
    let elements = batch
        .checked_mul(channels)
        .and_then(|value| value.checked_mul(width))
        .ok_or(QuantizationError::ShapeOverflow)?;
    if accumulators_ncw.len() < elements || output_ncw.len() < elements {
        return Err(QuantizationError::LengthMismatch);
    }
    if bias_quantized.is_some_and(|bias| bias.len() < channels) {
        return Err(QuantizationError::BiasTooSmall);
    }

    for batch_index in 0..batch {
        for channel in 0..channels {
            let bias = bias_quantized.map_or(0, |values| values[channel]);
            let start = (batch_index * channels + channel) * width;
            for offset in 0..width {
                let value = accumulators_ncw[start + offset].wrapping_add(bias);
                output_ncw[start + offset] = value as f32 * combined_scale;
            }
        }
    }
    Ok(())
}

/// `DynamicQuantizeLinear` ratios are mathematically bounded to `[-255, 255]`,
/// so i32 truncation gives a compact dependency-free ties-to-even operation.
fn round_ties_even_small(value: f32) -> f32 {
    debug_assert!(value.is_finite());
    // A final f32 division can overshoot the real-valued 255 bound by an ULP.
    debug_assert!((-256.0..=256.0).contains(&value));
    let truncated = value as i32;
    let fraction = value - truncated as f32;
    let rounded = if fraction > 0.5 || (fraction == 0.5 && truncated & 1 != 0) {
        truncated + 1
    } else if fraction < -0.5 || (fraction == -0.5 && truncated & 1 != 0) {
        truncated - 1
    } else {
        truncated
    };
    rounded as f32
}

fn floor_to_i32(value: f32) -> Result<i32, QuantizationError> {
    // `i32::MAX as f32` rounds to 2^31, so the upper endpoint is exclusive.
    if !value.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        return Err(QuantizationError::IntegerOverflow);
    }
    let truncated = value as i32;
    if value < 0.0 && (truncated as f32) > value {
        Ok(truncated - 1)
    } else {
        Ok(truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compact vectors captured with ONNX Runtime 1.28.0 from model SHA-256
    // 6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb.
    // They exercise the same implementation shared by all 139 DQL nodes, all
    // 80 biased and seven direct ConvInteger chains, and all 148 MatMulInteger
    // Cast -> Mul chains in that pinned graph.

    #[test]
    fn dynamic_quantize_matches_onnx_ties_and_asymmetric_ranges() {
        let mut output = [0u8; 7];
        let quantization =
            dynamic_quantize_linear_u8(&[-1.0, 0.0, 1.0, -0.5, 0.5, 0.25, -0.25], &mut output)
                .unwrap();
        assert_eq!(quantization.scale.to_bits(), (2.0f32 / 255.0).to_bits());
        // f32 division produces 127.49999 here, so ONNX rounds the zero point
        // to 127 rather than the real-arithmetic answer 128.
        assert_eq!(quantization.zero_point, 127);
        assert_eq!(output, [0, 127, 254, 63, 191, 159, 95]);

        let mut positive = [0u8; 4];
        let quantization =
            dynamic_quantize_linear_u8(&[1.0, 2.0, 3.0, 4.0], &mut positive).unwrap();
        assert_eq!(quantization.scale.to_bits(), (4.0f32 / 255.0).to_bits());
        assert_eq!(quantization.zero_point, 0);
        assert_eq!(positive, [64, 127, 191, 255]);

        let mut irregular = [0u8; 6];
        let quantization =
            dynamic_quantize_linear_u8(&[-3.25, -1.0, -0.1, 0.2, 2.75, 9.0], &mut irregular)
                .unwrap();
        assert_eq!(quantization.scale.to_bits(), 0x3D44_C4C5);
        assert_eq!(quantization.zero_point, 68);
        assert_eq!(irregular, [0, 47, 66, 72, 125, 255]);
    }

    #[test]
    fn split_parameter_and_bounded_quantization_matches_whole_tensor() {
        let input = [-1.0, 0.0, 1.0, -0.5, 0.5, 0.25, -0.25];
        let mut whole = [0_u8; 7];
        let expected = dynamic_quantize_linear_u8(&input, &mut whole).unwrap();
        let observed = dynamic_quantization_parameters(&input).unwrap();
        assert_eq!(observed, expected);
        let mut split = [0_u8; 7];
        quantize_linear_u8_with_parameters(&input[..3], observed, &mut split[..3]).unwrap();
        quantize_linear_u8_with_parameters(&input[3..], observed, &mut split[3..]).unwrap();
        assert_eq!(split, whole);
    }

    #[test]
    fn dynamic_quantize_zero_range_and_errors_are_fail_closed() {
        let mut output = [99u8; 3];
        assert_eq!(
            dynamic_quantize_linear_u8(&[0.0, -0.0, 0.0], &mut output),
            Ok(DynamicQuantization {
                scale: 1.0,
                zero_point: 0,
            })
        );
        assert_eq!(output, [0, 0, 0]);

        let mut untouched = [77u8; 2];
        assert_eq!(
            dynamic_quantize_linear_u8(&[1.0, f32::NAN], &mut untouched),
            Err(QuantizationError::NonFiniteInput)
        );
        assert_eq!(untouched, [77, 77]);
        assert_eq!(
            dynamic_quantize_linear_u8(&[1.0, 2.0], &mut [0; 1]),
            Err(QuantizationError::OutputTooSmall)
        );
    }

    #[test]
    fn dynamic_quantize_matches_real_kokoro_tensor_bits() {
        // Min, max and five samples from node 2566,
        // /decoder/.../noise_res.1/Add_6_output_0_QuantizeLinear. Retaining
        // the extrema makes this compact vector reproduce the full
        // [1, 128, 8281] tensor's scalar quantization parameters exactly.
        let input = [
            0xC0DB_D8E4,
            0x4134_A209,
            0x408C_CEB7,
            0x408D_4EAE,
            0x408D_C16D,
            0x3FA2_F425,
            0xBF5A_5223,
        ]
        .map(f32::from_bits);
        let mut output = [0u8; 7];
        let quantization = dynamic_quantize_linear_u8(&input, &mut output).unwrap();
        assert_eq!(quantization.scale.to_bits(), 0x3D91_D917);
        assert_eq!(quantization.zero_point, 96);
        assert_eq!(output, [0, 255, 158, 158, 158, 114, 84]);
    }

    #[test]
    fn dynamic_quantize_uses_ties_to_even_for_zero_point_and_samples() {
        let mut output = [0u8; 8];
        let quantization = dynamic_quantize_linear_u8(
            &[-127.0, 128.0, -2.5, -1.5, -0.5, 0.5, 1.5, 2.5],
            &mut output,
        )
        .unwrap();
        assert_eq!(quantization.scale.to_bits(), 1.0f32.to_bits());
        assert_eq!(quantization.zero_point, 127);
        assert_eq!(output, [0, 255, 125, 125, 127, 127, 129, 129]);

        let quantization = dynamic_quantize_linear_u8(
            &[-127.5, 127.5, -2.5, -1.5, -0.5, 0.5, 1.5, 2.5],
            &mut output,
        )
        .unwrap();
        assert_eq!(quantization.scale.to_bits(), 1.0f32.to_bits());
        assert_eq!(quantization.zero_point, 128);
        assert_eq!(output, [0, 255, 126, 126, 128, 128, 130, 130]);
    }

    #[test]
    fn dynamic_quantize_rejects_unrepresentable_scale_without_writing() {
        let mut output = [0xA5u8; 2];
        assert_eq!(
            dynamic_quantize_linear_u8(&[-f32::MAX, f32::MAX], &mut output),
            Err(QuantizationError::InvalidScale)
        );
        assert_eq!(output, [0xA5; 2]);

        // Dividing the smallest subnormal range by 255 underflows to zero.
        assert_eq!(
            dynamic_quantize_linear_u8(&[f32::from_bits(1)], &mut output),
            Err(QuantizationError::InvalidScale)
        );
        assert_eq!(output, [0xA5; 2]);
    }

    #[test]
    fn conv_bias_preserves_floor_then_integer_add_then_float_mul() {
        let mut bias_quantized = [0i32; 3];
        let scale =
            quantize_conv_bias_floor(&[-0.10, 0.10, 1.0], 0.2, 0.15, &mut bias_quantized).unwrap();
        assert_eq!(scale.to_bits(), (0.2f32 * 0.15).to_bits());
        assert_eq!(bias_quantized, [-4, 3, 33]);

        let accumulators = [10i32, -10, 20, -20, 30, -30];
        let mut output = [0.0f32; 6];
        dequantize_conv_integer_ncw(
            &accumulators,
            1,
            3,
            2,
            Some(&bias_quantized),
            scale,
            &mut output,
        )
        .unwrap();
        let expected = [6i32, -14, 23, -17, 63, 3].map(|value| value as f32 * scale);
        assert_eq!(output.map(f32::to_bits), expected.map(f32::to_bits));
    }

    #[test]
    fn biased_conv_epilogue_matches_real_kokoro_bits() {
        // First eight channels of node 2570,
        // /decoder/.../noise_res.1/convs1.2/Conv_quant.
        let bias = [
            0x3D57_9EE0,
            0x3C82_E7F2,
            0x3DCE_BBB8,
            0xBD7C_744D,
            0xBBDE_B041,
            0x3CE8_5229,
            0x3D62_1EF2,
            0xBDCC_F01C,
        ]
        .map(f32::from_bits);
        let mut quantized = [0i32; 8];
        let scale = quantize_conv_bias_floor(
            &bias,
            f32::from_bits(0x3D91_D917),
            f32::from_bits(0x3ACB_3DD9),
            &mut quantized,
        )
        .unwrap();
        assert_eq!(scale.to_bits(), 0x38E7_94C3);
        assert_eq!(quantized, [476, 144, 914, -559, -62, 256, 499, -907]);

        let mut output = [0.0f32; 1];
        dequantize_conv_integer_ncw(&[-10_249], 1, 1, 1, Some(&quantized[..1]), scale, &mut output)
            .unwrap();
        assert_eq!(output[0].to_bits(), 0xBF8A_2328);

        dequantize_conv_integer_ncw(&[-3_460], 1, 1, 1, Some(&quantized[3..4]), scale, &mut output)
            .unwrap();
        assert_eq!(output[0].to_bits(), 0xBEE3_3A47);
    }

    #[test]
    fn direct_conv_epilogue_matches_real_kokoro_bits() {
        // Node 1871 is one of the seven bias-free ConvInteger chains.
        let mut output = [0.0f32; 3];
        dequantize_conv_integer_ncw(
            &[2_018, 23_375, 3_540],
            1,
            1,
            3,
            None,
            f32::from_bits(0x3926_5E3A),
            &mut output,
        )
        .unwrap();
        assert_eq!(output.map(f32::to_bits), [0x3EA3_EE59, 0x406D_5B57, 0x3F0F_C8F0]);
    }

    #[test]
    fn matmul_cast_mul_helper_matches_real_kokoro_bits() {
        // Five columns sampled from node 11. The activation scale is scalar;
        // weight scales and the Cast -> Mul results are per output column.
        let weight_scales = [
            0x3B57_5561,
            0x3B23_D60D,
            0x3B7D_F610,
            0x3B78_328D,
            0x3B63_92B4,
        ]
        .map(f32::from_bits);
        let mut output = [0.0f32; 5];
        dequantize_matmul_integer_per_output(
            &[-46_769, 4_586, 8_749, -28_095, 26_148],
            1,
            5,
            f32::from_bits(0x3B35_DCBE),
            &weight_scales,
            &mut output,
        )
        .unwrap();
        assert_eq!(
            output.map(f32::to_bits),
            [
                0xBEDA_55A4,
                0x3D02_5007,
                0x3DC0_AE49,
                0xBE97_2CB8,
                0x3E81_01B3
            ]
        );
    }

    #[test]
    fn matmul_cast_mul_validates_every_scale_before_writing() {
        let mut output = [f32::from_bits(0x7FC0_0001); 2];
        assert_eq!(
            dequantize_matmul_integer_per_output(
                &[1, 2],
                1,
                2,
                0.25,
                &[0.5, f32::NAN],
                &mut output,
            ),
            Err(QuantizationError::InvalidScale)
        );
        assert_eq!(output.map(f32::to_bits), [0x7FC0_0001; 2]);
        assert_eq!(
            dequantize_matmul_integer_per_output(&[1], 1, 2, 0.25, &[0.5, 0.5], &mut output),
            Err(QuantizationError::LengthMismatch)
        );
    }

    #[test]
    fn conv_epilogue_validates_shapes_scales_and_wrapping_add() {
        let mut output = [0.0f32; 1];
        dequantize_conv_integer_ncw(&[i32::MAX], 1, 1, 1, Some(&[1]), 0.5, &mut output).unwrap();
        assert_eq!(output[0].to_bits(), ((i32::MIN as f32) * 0.5).to_bits());
        assert_eq!(
            dequantize_conv_integer_ncw(&[0], 1, 1, 1, None, 0.0, &mut output),
            Err(QuantizationError::InvalidScale)
        );
        assert_eq!(
            quantize_conv_bias_floor(&[f32::INFINITY], 0.1, 0.2, &mut [0]),
            Err(QuantizationError::NonFiniteInput)
        );

        let mut untouched = [123i32; 2];
        assert_eq!(
            quantize_conv_bias_floor(&[1.0, f32::NAN], 0.1, 0.2, &mut untouched),
            Err(QuantizationError::NonFiniteInput)
        );
        assert_eq!(untouched, [123; 2]);
        assert_eq!(
            quantize_conv_bias_floor(&[], 0.1, 0.2, &mut []),
            Err(QuantizationError::EmptyInput)
        );
    }
}
