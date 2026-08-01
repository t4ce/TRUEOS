#![no_std]
#![deny(unsafe_code)]

//! Allocation-free output conversion for native Kokoro synthesis.
//!
//! Kokoro produces mono `f32` samples at 24 kHz. TRUEOS' live PCM lane accepts
//! interleaved signed-16-bit stereo at 48 kHz. This crate performs that exact
//! 2x conversion in independently schedulable frame ranges and implements the
//! 10 ms crossfade used between model chunks by the host reference path.

pub const KOKORO_SAMPLE_RATE_HZ: u32 = 24_000;
pub const TRUEOS_SAMPLE_RATE_HZ: u32 = 48_000;
pub const TRUEOS_CHANNELS: usize = 2;
pub const CHUNK_CROSSFADE_SAMPLES_24K: usize = 240;
pub const CHUNK_CROSSFADE_FRAMES_48K: usize = CHUNK_CROSSFADE_SAMPLES_24K * 2;
/// Smooth the start of a request over 10 ms without changing the retained
/// native waveform. Kokoro can emit an isolated onset impulse before speech;
/// applying this only to presentation PCM keeps raw-model parity inspectable.
pub const STREAM_FADE_IN_FRAMES_48K: usize = 480;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    EmptyInput,
    OutputNotStereo,
    FrameCountOverflow,
    FrameRangeOutOfBounds,
    LengthMismatch,
    NonFiniteInput,
    NonFiniteOutput,
    Aliasing,
}

/// Number of 48 kHz frames produced by a complete 24 kHz waveform.
pub fn output_frames(input_samples: usize) -> Result<usize, Error> {
    input_samples
        .checked_mul(2)
        .ok_or(Error::FrameCountOverflow)
}

/// Convert one independently schedulable 48 kHz frame range.
///
/// Even output frames reproduce a source sample. Odd frames linearly
/// interpolate that sample and its successor; the final odd frame holds the
/// final source value. Each mono frame is duplicated into left and right.
/// Validation is completed before `output_stereo` is modified.
pub fn convert_frame_range(
    input_mono_24k: &[f32],
    frame_start: usize,
    output_stereo: &mut [i16],
) -> Result<usize, Error> {
    if input_mono_24k.is_empty() {
        return Err(Error::EmptyInput);
    }
    if !output_stereo.len().is_multiple_of(TRUEOS_CHANNELS) {
        return Err(Error::OutputNotStereo);
    }
    let frame_count = output_stereo.len() / TRUEOS_CHANNELS;
    let total_frames = output_frames(input_mono_24k.len())?;
    let frame_end = frame_start
        .checked_add(frame_count)
        .ok_or(Error::FrameRangeOutOfBounds)?;
    if frame_end > total_frames {
        return Err(Error::FrameRangeOutOfBounds);
    }
    reject_overlap(input_mono_24k, output_stereo)?;

    // Validate exactly the source span this work range can observe. Empty
    // output is a valid no-op and intentionally observes no input samples.
    if frame_count != 0 {
        let first_source = frame_start / 2;
        let last_frame = frame_end - 1;
        let last_source =
            (last_frame / 2 + usize::from(last_frame & 1 != 0)).min(input_mono_24k.len() - 1);
        if input_mono_24k[first_source..=last_source]
            .iter()
            .any(|sample| !sample.is_finite())
        {
            return Err(Error::NonFiniteInput);
        }
    }

    let (stereo_frames, remainder) = output_stereo.as_chunks_mut::<TRUEOS_CHANNELS>();
    debug_assert!(remainder.is_empty());
    for (local_frame, stereo) in stereo_frames.iter_mut().enumerate() {
        let output_frame = frame_start + local_frame;
        let source = output_frame / 2;
        let sample = if output_frame & 1 == 0 {
            input_mono_24k[source]
        } else {
            let next = (source + 1).min(input_mono_24k.len() - 1);
            input_mono_24k[source] * 0.5 + input_mono_24k[next] * 0.5
        };
        let sample = f32_to_i16(sample);
        stereo[0] = sample;
        stereo[1] = sample;
    }
    Ok(frame_count)
}

/// Apply the request-level 10 ms fade to one independently emitted PCM range.
///
/// `frame_start` is relative to the beginning of the request, not the current
/// PCM buffer. A smoothstep envelope strongly attenuates sub-millisecond
/// decoder settlement while reaching unity before ordinary speech begins.
/// Calling this for arbitrary consecutive ranges produces the same result as
/// calling it once for their concatenation.
pub fn fade_in_frame_range(frame_start: usize, stereo: &mut [i16]) -> Result<(), Error> {
    if !stereo.len().is_multiple_of(TRUEOS_CHANNELS) {
        return Err(Error::OutputNotStereo);
    }
    let (frames, remainder) = stereo.as_chunks_mut::<TRUEOS_CHANNELS>();
    debug_assert!(remainder.is_empty());
    frame_start
        .checked_add(frames.len())
        .ok_or(Error::FrameRangeOutOfBounds)?;
    for (local_frame, channels) in frames.iter_mut().enumerate() {
        let frame = frame_start + local_frame;
        if frame >= STREAM_FADE_IN_FRAMES_48K {
            break;
        }
        let x = frame as f32 / (STREAM_FADE_IN_FRAMES_48K - 1) as f32;
        let gain = x * x * (3.0 - 2.0 * x);
        for sample in channels {
            *sample = (*sample as f32 * gain) as i16;
        }
    }
    Ok(())
}

/// Blend equal-size retained and incoming 24 kHz boundaries.
///
/// For the native backend these slices are normally 240 samples (10 ms). The
/// weights are identical to the host reference: `(i + 1) / (N + 1)`, so
/// neither endpoint is discarded abruptly. Inputs and output must not alias;
/// every rejection leaves `output` untouched.
pub fn crossfade(
    retained_left: &[f32],
    incoming_right: &[f32],
    output: &mut [f32],
) -> Result<(), Error> {
    if retained_left.len() != incoming_right.len() || output.len() != retained_left.len() {
        return Err(Error::LengthMismatch);
    }
    reject_overlap(retained_left, output)?;
    reject_overlap(incoming_right, output)?;
    if retained_left
        .iter()
        .chain(incoming_right)
        .any(|sample| !sample.is_finite())
    {
        return Err(Error::NonFiniteInput);
    }
    let denominator = output.len() as f32 + 1.0;
    for index in 0..output.len() {
        let weight = (index + 1) as f32 / denominator;
        let sample = retained_left[index] * (1.0 - weight) + incoming_right[index] * weight;
        if !sample.is_finite() {
            return Err(Error::NonFiniteOutput);
        }
    }
    for (index, destination) in output.iter_mut().enumerate() {
        let weight = (index + 1) as f32 / denominator;
        *destination = retained_left[index] * (1.0 - weight) + incoming_right[index] * weight;
    }
    Ok(())
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

fn reject_overlap<A, B>(lhs: &[A], rhs: &[B]) -> Result<(), Error> {
    let lhs_start = lhs.as_ptr() as usize;
    let rhs_start = rhs.as_ptr() as usize;
    let lhs_end = lhs_start.saturating_add(core::mem::size_of_val(lhs));
    let rhs_end = rhs_start.saturating_add(core::mem::size_of_val(rhs));
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
    use std::vec;

    #[test]
    fn exact_two_x_interpolation_and_stereo_layout() {
        let input = [0.0_f32, 1.0, -1.0];
        let mut output = [99_i16; 12];
        assert_eq!(convert_frame_range(&input, 0, &mut output), Ok(6));
        assert_eq!(
            output,
            [
                0, 0, 16_383, 16_383, 32_767, 32_767, 0, 0, -32_767, -32_767, -32_767, -32_767,
            ]
        );
    }

    #[test]
    fn arbitrary_pcm_chunks_reassemble_bit_exactly() {
        let input = [-0.75_f32, -0.1, 0.2, 0.8, 1.2];
        let mut whole = [0_i16; 20];
        convert_frame_range(&input, 0, &mut whole).unwrap();

        let mut assembled = vec![0_i16; 20];
        convert_frame_range(&input, 0, &mut assembled[..6]).unwrap();
        convert_frame_range(&input, 3, &mut assembled[6..14]).unwrap();
        convert_frame_range(&input, 7, &mut assembled[14..]).unwrap();
        assert_eq!(assembled.as_slice(), whole.as_slice());
    }

    #[test]
    fn request_fade_is_chunk_boundary_independent() {
        let mut whole = vec![12_000_i16; (STREAM_FADE_IN_FRAMES_48K + 2) * TRUEOS_CHANNELS];
        fade_in_frame_range(0, &mut whole).unwrap();

        let split_frame = 137;
        let mut chunked = vec![12_000_i16; whole.len()];
        let split_sample = split_frame * TRUEOS_CHANNELS;
        fade_in_frame_range(0, &mut chunked[..split_sample]).unwrap();
        fade_in_frame_range(split_frame, &mut chunked[split_sample..]).unwrap();
        assert_eq!(chunked, whole);
        assert_eq!(&whole[..TRUEOS_CHANNELS], &[0, 0]);
        let unity = STREAM_FADE_IN_FRAMES_48K * TRUEOS_CHANNELS;
        assert_eq!(&whole[unity..unity + TRUEOS_CHANNELS], &[12_000, 12_000]);
    }

    #[test]
    fn crossfade_matches_host_weight_contract() {
        let left = [1.0_f32; 3];
        let right = [0.0_f32; 3];
        let mut output = [-1.0_f32; 3];
        crossfade(&left, &right, &mut output).unwrap();
        assert_eq!(output, [0.75, 0.5, 0.25]);
        assert_eq!(CHUNK_CROSSFADE_FRAMES_48K, 480);
    }

    #[test]
    fn all_rejections_are_transactional() {
        let input = [0.0_f32, f32::NAN];
        let mut output = [7_i16; 4];
        assert_eq!(convert_frame_range(&input, 1, &mut output), Err(Error::NonFiniteInput));
        assert_eq!(output, [7; 4]);
        assert_eq!(convert_frame_range(&[0.0], 1, &mut output), Err(Error::FrameRangeOutOfBounds));
        assert_eq!(output, [7; 4]);

        let mut blend = [9.0_f32; 2];
        assert_eq!(
            crossfade(&[0.0, f32::INFINITY], &[1.0, 1.0], &mut blend),
            Err(Error::NonFiniteInput)
        );
        assert_eq!(blend, [9.0; 2]);
    }
}
