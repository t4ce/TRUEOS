#![no_std]
#![deny(unsafe_code)]

//! Model-specific duration resolution for the sealed TRUEOS Kokoro graph.
//!
//! The prepared model produces one row of 50 duration logits per phoneme. The
//! graph then executes this fixed chain:
//!
//! ```text
//! Sigmoid -> ReduceSum(axis=-1) -> Div(speed) -> Round(ties-to-even)
//!         -> Clip(min=1) -> Cast<i64> -> Gather(batch=0)
//!         -> CumSum(axis=0) -> Gather(last)
//! ```
//!
//! [`resolve_decoder_shape`] fuses that chain. It writes the cumulative
//! duration vector needed by phase 1 and returns the frame count which sizes
//! the dynamic activation arena. The implementation is allocation-free and
//! validates in a first pass, so a rejected call leaves the caller's output
//! untouched.

/// Width of `encoder.predictor.duration_proj.linear_layer` in the pinned
/// Kokoro graph.
pub const KOKORO_DURATION_BINS: usize = 50;

/// Maximum phoneme tensor length admitted by the native model contract,
/// including BOS/EOS.
pub const KOKORO_MAX_TOKENS: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolveError {
    EmptyInput,
    TooManyTokens,
    LogitLengthMismatch,
    OutputLengthMismatch,
    InvalidSpeed,
    NonFiniteLogit,
    DurationOutOfRange,
    FrameCountOverflow,
    FrameLimitExceeded,
}

/// Resolve the dynamic decoder frame count for one Kokoro batch.
///
/// `biased_logits` is the contiguous `[1, token_count, 50]` result after the
/// duration projection bias. `cumulative_durations` receives the graph's
/// INT64 CumSum output. `frame_limit` is the sealed phase-1 maximum and must be
/// supplied by the AOT program rather than guessed by the kernel.
///
/// The returned count is suitable for `Program::resolve_phase_two` in the AOT
/// runtime. On every error, `cumulative_durations` remains byte-for-byte
/// unchanged.
pub fn resolve_decoder_shape(
    biased_logits: &[f32],
    token_count: usize,
    speed: f32,
    frame_limit: u32,
    cumulative_durations: &mut [i64],
) -> Result<u32, ResolveError> {
    validate_call(biased_logits, token_count, speed, cumulative_durations.len())?;

    // Pass one proves every conversion and the final arena bound before the
    // caller-owned output is changed.
    let (rows, remainder) = biased_logits.as_chunks::<KOKORO_DURATION_BINS>();
    debug_assert!(remainder.is_empty());
    let mut total = 0_i64;
    for row in rows {
        let duration = duration_for_row(row, speed)?;
        total = total
            .checked_add(duration)
            .ok_or(ResolveError::FrameCountOverflow)?;
        if total > i64::from(frame_limit) {
            return Err(ResolveError::FrameLimitExceeded);
        }
    }
    let frame_count = u32::try_from(total).map_err(|_| ResolveError::FrameCountOverflow)?;

    // The same fixed-order f32 chain is deterministic, so the second pass can
    // materialize the CumSum without scratch storage.
    let mut cumulative = 0_i64;
    for (row, output) in rows.iter().zip(cumulative_durations.iter_mut()) {
        let duration = duration_for_row(row, speed)?;
        cumulative += duration;
        *output = cumulative;
    }
    debug_assert_eq!(cumulative, total);
    Ok(frame_count)
}

fn validate_call(
    biased_logits: &[f32],
    token_count: usize,
    speed: f32,
    output_len: usize,
) -> Result<(), ResolveError> {
    if token_count == 0 {
        return Err(ResolveError::EmptyInput);
    }
    if token_count > KOKORO_MAX_TOKENS {
        return Err(ResolveError::TooManyTokens);
    }
    let expected_logits = token_count
        .checked_mul(KOKORO_DURATION_BINS)
        .ok_or(ResolveError::LogitLengthMismatch)?;
    if biased_logits.len() != expected_logits {
        return Err(ResolveError::LogitLengthMismatch);
    }
    if output_len != token_count {
        return Err(ResolveError::OutputLengthMismatch);
    }
    if !speed.is_finite() || speed <= 0.0 {
        return Err(ResolveError::InvalidSpeed);
    }
    Ok(())
}

fn duration_for_row(row: &[f32], speed: f32) -> Result<i64, ResolveError> {
    let mut sum = 0.0_f32;
    for &logit in row {
        if !logit.is_finite() {
            return Err(ResolveError::NonFiniteLogit);
        }
        sum += sigmoid(logit);
    }
    let scaled = sum / speed;
    if !scaled.is_finite() {
        return Err(ResolveError::DurationOutOfRange);
    }
    let rounded = round_ties_even_nonnegative(scaled);
    let clipped = if rounded < 1.0 { 1.0 } else { rounded };

    // 2^63 is exactly representable as f32 but is outside the i64 domain.
    // The ONNX graph never approaches it; rejecting it keeps malformed input
    // fail-closed instead of relying on a language-specific saturating cast.
    if clipped >= 9_223_372_036_854_775_808.0_f32 {
        return Err(ResolveError::DurationOutOfRange);
    }
    Ok(clipped as i64)
}

fn sigmoid(value: f32) -> f32 {
    // The negative branch is algebraically identical and matches the f32
    // kernel lane while avoiding an intermediate infinity.
    if value >= 0.0 {
        1.0 / (1.0 + libm::expf(-value))
    } else {
        let exponential = libm::expf(value);
        exponential / (1.0 + exponential)
    }
}

fn round_ties_even_nonnegative(value: f32) -> f32 {
    // All integral f32 values at and above 2^23 are already rounded.
    if value >= 8_388_608.0 {
        return value;
    }
    let lower = libm::floorf(value);
    let fraction = value - lower;
    if fraction < 0.5 {
        lower
    } else if fraction > 0.5 {
        lower + 1.0
    } else if (lower as u32) & 1 == 0 {
        lower
    } else {
        lower + 1.0
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_ort_1_28_duration_fixture_matches() {
        // Fixture generated with the exact opset-20 chain documented above
        // and ONNX Runtime 1.28.0 CPUExecutionProvider.
        let mut logits = [0.0_f32; 4 * KOKORO_DURATION_BINS];
        for token in 0..4 {
            for bin in 0..KOKORO_DURATION_BINS {
                let numerator = ((token * 37 + bin * 13) % 41) as i32 - 20;
                logits[token * KOKORO_DURATION_BINS + bin] = numerator as f32 / 7.0;
            }
        }
        let mut cumulative = [-1_i64; 4];
        let frames = resolve_decoder_shape(&logits, 4, 1.37, 1_024, &mut cumulative).unwrap();
        assert_eq!(cumulative, [18, 36, 54, 73]);
        assert_eq!(frames, 73);
    }

    #[test]
    fn pinned_ort_1_28_speed_sweep_matches() {
        let mut logits = [0.0_f32; 8 * KOKORO_DURATION_BINS];
        for (index, value) in logits.iter_mut().enumerate() {
            let raw = (((index as u64 * 1_103_515_245 + 12_345) & 0xffff) % 401) as i32;
            *value = (raw - 200) as f32 / 16.0;
        }
        let fixtures = [
            (0.5, [52, 102, 150, 194, 246, 302, 352, 400]),
            (0.85, [31, 60, 88, 114, 145, 178, 207, 235]),
            (1.0, [26, 51, 75, 97, 123, 151, 176, 200]),
            (1.37, [19, 37, 54, 70, 89, 110, 128, 145]),
            (2.0, [13, 26, 38, 49, 62, 76, 88, 100]),
            (4.0, [7, 13, 19, 25, 32, 39, 45, 51]),
        ];
        for (speed, expected) in fixtures {
            let mut cumulative = [0_i64; 8];
            let frames = resolve_decoder_shape(&logits, 8, speed, 1_024, &mut cumulative).unwrap();
            assert_eq!(cumulative, expected, "speed={speed}");
            assert_eq!(frames, expected[7] as u32, "speed={speed}");
        }
    }

    #[test]
    fn minimum_one_frame_per_token_is_preserved() {
        let logits = [-100.0_f32; 3 * KOKORO_DURATION_BINS];
        let mut cumulative = [0_i64; 3];
        let frames = resolve_decoder_shape(&logits, 3, 4.0, 3, &mut cumulative).unwrap();
        assert_eq!(cumulative, [1, 2, 3]);
        assert_eq!(frames, 3);
    }

    #[test]
    fn ties_round_to_even() {
        assert_eq!(round_ties_even_nonnegative(0.5), 0.0);
        assert_eq!(round_ties_even_nonnegative(1.5), 2.0);
        assert_eq!(round_ties_even_nonnegative(2.5), 2.0);
        assert_eq!(round_ties_even_nonnegative(3.5), 4.0);
        assert_eq!(round_ties_even_nonnegative(8_388_608.0), 8_388_608.0);
    }

    #[test]
    fn frame_limit_failure_is_transactional() {
        let logits = [0.0_f32; 2 * KOKORO_DURATION_BINS];
        let mut cumulative = [91_i64, 92];
        assert_eq!(
            resolve_decoder_shape(&logits, 2, 1.0, 49, &mut cumulative),
            Err(ResolveError::FrameLimitExceeded)
        );
        assert_eq!(cumulative, [91, 92]);
    }

    #[test]
    fn malformed_inputs_are_rejected_without_writes() {
        let good = [0.0_f32; KOKORO_DURATION_BINS];
        let mut output = [77_i64];
        assert_eq!(
            resolve_decoder_shape(&good, 1, 0.0, 100, &mut output),
            Err(ResolveError::InvalidSpeed)
        );
        assert_eq!(output, [77]);

        let mut nonfinite = good;
        nonfinite[17] = f32::NAN;
        assert_eq!(
            resolve_decoder_shape(&nonfinite, 1, 1.0, 100, &mut output),
            Err(ResolveError::NonFiniteLogit)
        );
        assert_eq!(output, [77]);

        assert_eq!(
            resolve_decoder_shape(&good[..49], 1, 1.0, 100, &mut output),
            Err(ResolveError::LogitLengthMismatch)
        );
        assert_eq!(output, [77]);
    }
}
