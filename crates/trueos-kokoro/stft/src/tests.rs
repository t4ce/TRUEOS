use std::vec;
use std::vec::Vec;

use super::*;

const MINIMUM_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/b1_l20_minimum.bin");
const TAIL_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/b2_l24_incomplete_tail.bin");
const SECOND_FRAME_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/b2_l25_second_frame.bin");

#[derive(Debug)]
struct Fixture<'a> {
    batch: usize,
    length: usize,
    frames: usize,
    output: &'a [u8],
}

fn fixture(blob: &[u8]) -> Fixture<'_> {
    const HEADER: usize = 8 + 5 * 4;
    assert!(blob.len() >= HEADER);
    assert_eq!(&blob[..8], b"KORSTFT1");
    assert_eq!(read_u32(blob, 8), 1);
    let batch = read_u32(blob, 12) as usize;
    let length = read_u32(blob, 16) as usize;
    let frames = read_u32(blob, 20) as usize;
    let output_elements = read_u32(blob, 24) as usize;
    assert_eq!(blob.len(), HEADER + output_elements * 4);
    Fixture {
        batch,
        length,
        frames,
        output: &blob[HEADER..],
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn fixture_input(batch: usize, length: usize) -> Vec<f32> {
    let mut signal = vec![0.0; batch * length];
    for batch_index in 0..batch {
        for sample in 0..length {
            let raw = ((batch_index * 37 + sample * 11 + 5) % 41) as i32 - 20;
            signal[batch_index * length + sample] = raw as f32 / 8.0;
        }
    }
    signal
}

fn assert_ort_exact(label: &str, actual: &[f32], expected: &[u8]) {
    assert_eq!(expected.len(), actual.len() * 4, "{label} length");
    let (expected, remainder) = expected.as_chunks::<4>();
    assert!(remainder.is_empty());
    for (index, (&candidate, encoded)) in actual.iter().zip(expected).enumerate() {
        let oracle = f32::from_le_bytes(*encoded);
        assert_eq!(
            candidate.to_bits(),
            oracle.to_bits(),
            "{label}[{index}]: native={candidate:?} (0x{:08x}), ORT={oracle:?} (0x{:08x})",
            candidate.to_bits(),
            oracle.to_bits(),
        );
    }
}

fn run_fixture(blob: &[u8]) {
    let oracle = fixture(blob);
    let input = fixture_input(oracle.batch, oracle.length);
    let problem = Problem::new(oracle.batch, oracle.length, &input).unwrap();
    let shape = problem.output_shape();
    assert_eq!(shape.batch(), oracle.batch);
    assert_eq!(shape.frames(), oracle.frames);
    assert_eq!(shape.bins(), OUTPUT_BINS);
    assert_eq!(shape.components(), OUTPUT_COMPONENTS);

    let mut output = vec![f32::NAN; shape.elements()];
    let mut run = CooperativeStft::start(problem, &mut output).unwrap();
    let mut expected_batch = 0;
    let mut expected_frame = 0;
    while !run.is_complete() {
        assert_eq!(run.next_frame(), Some((expected_batch, expected_frame)));
        let budget = if expected_frame == 0 { 1 } else { usize::MAX };
        let Advance::Advanced(range) = run.advance(budget).unwrap() else {
            panic!("run completed before all expected frames");
        };
        assert_eq!(range.batch, expected_batch);
        assert_eq!(range.start_frame, expected_frame);
        assert!(range.end_frame > range.start_frame);
        assert!(range.end_frame <= oracle.frames);
        assert_eq!(range.completed_frames, run.completed_frames());
        assert_eq!(range.total_frames, oracle.batch * oracle.frames);
        expected_frame = range.end_frame;
        if expected_frame == oracle.frames {
            expected_batch += 1;
            expected_frame = 0;
        }
    }
    assert_eq!(expected_batch, oracle.batch);
    assert_eq!(run.completed_frames(), run.total_frames());
    assert_eq!(run.next_frame(), None);
    assert_eq!(run.advance(1).unwrap(), Advance::Complete);
    assert_ort_exact("STFT", run.output(), oracle.output);
}

#[test]
fn official_ort_1_27_minimum_length_matches_component_order_and_sign() {
    run_fixture(MINIMUM_FIXTURE);
}

#[test]
fn official_ort_1_27_incomplete_tail_has_no_padding() {
    run_fixture(TAIL_FIXTURE);
}

#[test]
fn official_ort_1_27_exact_second_frame_boundary_matches() {
    run_fixture(SECOND_FRAME_FIXTURE);
}

#[test]
fn model_window_and_forward_root_bits_have_exact_structure() {
    assert_eq!(HANN_WINDOW_BITS[0], 0x0000_0000);
    assert_eq!(HANN_WINDOW_BITS[10], 0x3f80_0000);
    assert_eq!(HANN_WINDOW_BITS[19], 0x3cc8_78f6);
    for index in 1..10 {
        assert_eq!(HANN_WINDOW_BITS[index], HANN_WINDOW_BITS[20 - index]);
    }

    assert_eq!(DFT_COS_BITS[0], 0x3f80_0000);
    assert_eq!(DFT_COS_BITS[5], 0x0000_0000);
    assert_eq!(DFT_COS_BITS[10], 0xbf80_0000);
    assert_eq!(DFT_COS_BITS[15], 0x0000_0000);
    assert_eq!(DFT_NEG_SIN_BITS[0], 0x0000_0000);
    assert_eq!(DFT_NEG_SIN_BITS[5], 0xbf80_0000);
    assert_eq!(DFT_NEG_SIN_BITS[10], 0x0000_0000);
    assert_eq!(DFT_NEG_SIN_BITS[15], 0x3f80_0000);
    for index in 1..10 {
        assert_eq!(DFT_COS_BITS[index], DFT_COS_BITS[20 - index]);
        assert_eq!(DFT_NEG_SIN_BITS[index] ^ 0x8000_0000, DFT_NEG_SIN_BITS[20 - index]);
    }
}

#[test]
fn one_sample_probe_places_real_then_negative_sine_imaginary() {
    let mut input = [0.0_f32; FRAME_LENGTH];
    input[1] = 1.0;
    let problem = Problem::new(1, FRAME_LENGTH, &input).unwrap();
    let mut output = [0.0_f32; OUTPUT_ELEMENTS_PER_FRAME];
    let mut run = CooperativeStft::start(problem, &mut output).unwrap();
    assert!(matches!(run.advance(1), Ok(Advance::Advanced(_))));

    let windowed = f32::from_bits(HANN_WINDOW_BITS[1]);
    // Bluestein's two FFT passes introduce the same tiny rounding residue as
    // ORT, so only the transform convention—not direct-DFT arithmetic—is the
    // contract of this focused probe. Bit parity is pinned by the fixtures.
    assert!((run.output()[0] - windowed).abs() < 2.0e-8);
    assert!(run.output()[1].abs() < 2.0e-8);
    assert!(run.output()[2] > 0.0);
    assert!(run.output()[3] < 0.0);
}

#[test]
fn samples_after_last_complete_frame_are_ignored() {
    let first = fixture_input(1, 24);
    let mut second = first.clone();
    for value in &mut second[20..] {
        *value = 123.0;
    }
    let mut first_output = [0.0; OUTPUT_ELEMENTS_PER_FRAME];
    let mut second_output = [0.0; OUTPUT_ELEMENTS_PER_FRAME];
    let mut first_run =
        CooperativeStft::start(Problem::new(1, 24, &first).unwrap(), &mut first_output).unwrap();
    let mut second_run =
        CooperativeStft::start(Problem::new(1, 24, &second).unwrap(), &mut second_output).unwrap();
    first_run.advance(1).unwrap();
    second_run.advance(1).unwrap();
    assert_eq!(first_run.output(), second_run.output());
}

#[test]
fn invalid_shape_and_nonfinite_input_are_rejected() {
    assert!(matches!(Problem::new(0, FRAME_LENGTH, &[]), Err(ContractError::EmptyBatch)));
    assert!(matches!(Problem::new(1, FRAME_LENGTH - 1, &[]), Err(ContractError::SignalTooShort)));
    assert!(matches!(
        Problem::new(usize::MAX, FRAME_LENGTH, &[]),
        Err(ContractError::ShapeOverflow)
    ));
    assert!(matches!(
        Problem::new(usize::MAX / FRAME_LENGTH, FRAME_LENGTH, &[]),
        Err(ContractError::ShapeOverflow)
    ));
    assert!(matches!(Problem::new(1, FRAME_LENGTH, &[]), Err(ContractError::InputLengthMismatch)));

    let mut signal = [0.0; FRAME_LENGTH];
    signal[7] = f32::NAN;
    assert!(matches!(Problem::new(1, FRAME_LENGTH, &signal), Err(ContractError::NonFiniteInput)));
    signal[7] = f32::INFINITY;
    assert!(matches!(Problem::new(1, FRAME_LENGTH, &signal), Err(ContractError::NonFiniteInput)));
}

#[test]
fn rejected_output_and_zero_budget_are_transactional() {
    let input = [1.0_f32; FRAME_LENGTH];
    let problem = Problem::new(1, FRAME_LENGTH, &input).unwrap();
    let mut short_output = [77.0; OUTPUT_ELEMENTS_PER_FRAME - 1];
    let rejected = CooperativeStft::start(problem, &mut short_output);
    assert!(matches!(rejected, Err(ContractError::OutputLengthMismatch)));
    assert!(short_output.iter().all(|&value| value == 77.0));

    let mut output = [88.0; OUTPUT_ELEMENTS_PER_FRAME];
    let mut run = CooperativeStft::start(problem, &mut output).unwrap();
    assert_eq!(run.advance(0), Err(AdvanceError::ZeroFrameBudget));
    assert_eq!(run.next_frame(), Some((0, 0)));
    assert_eq!(run.completed_frames(), 0);
    assert!(run.output().iter().all(|&value| value == 88.0));
}
