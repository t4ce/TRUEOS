use std::vec;
use std::vec::Vec;

use super::*;

const TEXT_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/text512_t3.bin");
const PROSODY_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/prosody640_t2.bin");

struct OwnedProblem {
    sequence: usize,
    width: InputWidth,
    input: Vec<f32>,
    weights: Vec<f32>,
    recurrent: Vec<f32>,
    bias: Vec<f32>,
}

impl OwnedProblem {
    fn fixture(sequence: usize, width: InputWidth) -> Self {
        let channels = width.channels();
        let mut input = vec![0.0; sequence * channels];
        for time in 0..sequence {
            for channel in 0..channels {
                let raw = ((time * 19 + channel * 7 + 3) % 29) as i32 - 14;
                input[time * channels + channel] = raw as f32 / 16.0;
            }
        }

        let mut weights = vec![0.0; DIRECTIONS * GATE_ELEMENTS * channels];
        let mut recurrent = vec![0.0; DIRECTIONS * GATE_ELEMENTS * HIDDEN_SIZE];
        let mut bias = vec![0.0; DIRECTIONS * BIAS_ELEMENTS_PER_DIRECTION];
        for direction in 0..DIRECTIONS {
            for gate in 0..GATE_COUNT {
                for hidden in 0..HIDDEN_SIZE {
                    let row = gate * HIDDEN_SIZE + hidden;
                    let weight_row = (direction * GATE_ELEMENTS + row) * channels;
                    let input_a = (hidden * 17 + gate * 29 + direction * 31) % channels;
                    let mut input_b = (hidden * 43 + gate * 11 + direction * 7 + 5) % channels;
                    if input_b == input_a {
                        input_b = (input_b + 1) % channels;
                    }
                    debug_assert_ne!(input_a, input_b);
                    let raw_a = ((hidden * 3 + gate * 5 + direction * 7) % 9) as i32 - 4;
                    let raw_b = ((hidden * 5 + gate * 7 + direction * 2 + 1) % 7) as i32 - 3;
                    weights[weight_row + input_a] = raw_a as f32 / 16.0;
                    weights[weight_row + input_b] = raw_b as f32 / 32.0;

                    let recurrent_row = (direction * GATE_ELEMENTS + row) * HIDDEN_SIZE;
                    let recurrent_a = hidden;
                    let mut recurrent_b = (hidden + 1 + gate * 13 + direction * 3) % HIDDEN_SIZE;
                    if recurrent_b == recurrent_a {
                        recurrent_b = (recurrent_b + 1) % HIDDEN_SIZE;
                    }
                    let recurrent_raw_a =
                        ((hidden * 11 + gate * 3 + direction * 5 + 2) % 7) as i32 - 3;
                    let recurrent_raw_b =
                        ((hidden * 7 + gate * 5 + direction * 11 + 1) % 5) as i32 - 2;
                    recurrent[recurrent_row + recurrent_a] = recurrent_raw_a as f32 / 32.0;
                    recurrent[recurrent_row + recurrent_b] = recurrent_raw_b as f32 / 64.0;

                    let wb_raw = ((hidden * 13 + gate * 17 + direction * 19 + 4) % 17) as i32 - 8;
                    let rb_raw = ((hidden * 23 + gate * 7 + direction * 3 + 6) % 13) as i32 - 6;
                    let bias_base = direction * BIAS_ELEMENTS_PER_DIRECTION;
                    bias[bias_base + row] = wb_raw as f32 / 64.0;
                    bias[bias_base + GATE_ELEMENTS + row] = rb_raw as f32 / 64.0;
                }
            }
        }

        Self {
            sequence,
            width,
            input,
            weights,
            recurrent,
            bias,
        }
    }

    fn problem(&self) -> Problem<'_> {
        Problem::new(
            self.sequence,
            self.width,
            &self.input,
            &self.weights,
            &self.recurrent,
            &self.bias,
        )
        .unwrap()
    }
}

#[derive(Debug)]
struct Fixture<'a> {
    sequence: usize,
    width: usize,
    y: &'a [u8],
    y_h: &'a [u8],
    y_c: &'a [u8],
}

fn fixture(blob: &[u8]) -> Fixture<'_> {
    const HEADER: usize = 8 + 6 * 4;
    assert!(blob.len() >= HEADER);
    assert_eq!(&blob[..8], b"KORLSTM1");
    let version = read_u32(blob, 8);
    let sequence = read_u32(blob, 12) as usize;
    let width = read_u32(blob, 16) as usize;
    let y_len = read_u32(blob, 20) as usize;
    let y_h_len = read_u32(blob, 24) as usize;
    let y_c_len = read_u32(blob, 28) as usize;
    assert_eq!(version, 1);
    assert_eq!(blob.len(), HEADER + 4 * (y_len + y_h_len + y_c_len));

    let y_start = HEADER;
    let y_h_start = y_start + 4 * y_len;
    let y_c_start = y_h_start + 4 * y_h_len;
    Fixture {
        sequence,
        width,
        y: &blob[y_start..y_h_start],
        y_h: &blob[y_h_start..y_c_start],
        y_c: &blob[y_c_start..],
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn assert_ort_close(label: &str, actual: &[f32], expected: &[u8]) {
    assert_eq!(expected.len(), actual.len() * 4, "{label} length");
    let (expected, remainder) = expected.as_chunks::<4>();
    assert!(remainder.is_empty());
    let mut maximum_difference = 0.0_f32;
    for (index, (&candidate, encoded)) in actual.iter().zip(expected).enumerate() {
        let oracle = f32::from_le_bytes(*encoded);
        let difference = (candidate - oracle).abs();
        maximum_difference = maximum_difference.max(difference);
        let tolerance = 2.0e-7 + 2.0e-6 * oracle.abs();
        assert!(
            difference <= tolerance,
            "{label}[{index}]: scalar={candidate:?}, ORT={oracle:?}, \
             difference={difference:?}, tolerance={tolerance:?}"
        );
    }
    std::println!("{label}: max abs difference from ORT = {maximum_difference:e}");
}

fn run_fixture(blob: &[u8], expected_width: InputWidth) {
    let oracle = fixture(blob);
    assert_eq!(oracle.width, expected_width.channels());
    let model = OwnedProblem::fixture(oracle.sequence, expected_width);
    let problem = model.problem();
    let mut output = vec![f32::NAN; problem.output_elements()];
    let mut hidden = vec![7.0; STATE_ELEMENTS];
    let mut cell = vec![8.0; STATE_ELEMENTS];
    let mut gates = vec![9.0; GATE_SCRATCH_ELEMENTS];
    let buffers = Buffers::new(&mut output, &mut hidden, &mut cell, &mut gates);
    let mut run = CooperativeLstm::start_zeroed(problem, buffers).unwrap();
    assert!(run.hidden().iter().all(|&value| value == 0.0));
    assert!(run.cell().iter().all(|&value| value == 0.0));

    let mut dense = ScalarDense;
    let mut observed = 0;
    loop {
        let expected_work = if observed < oracle.sequence {
            Some((0, observed))
        } else if observed < 2 * oracle.sequence {
            Some((1, 2 * oracle.sequence - 1 - observed))
        } else {
            None
        };
        assert_eq!(run.next_step(), expected_work);
        match run.advance(&mut dense).unwrap() {
            Advance::Advanced(step) => {
                let (direction, sequence_index) = expected_work.unwrap();
                observed += 1;
                assert_eq!(step.direction, direction);
                assert_eq!(step.sequence_index, sequence_index);
                assert_eq!(step.completed_steps, observed);
                assert_eq!(step.total_steps, 2 * oracle.sequence);
            }
            Advance::Complete => break,
        }
    }
    assert_eq!(observed, 2 * oracle.sequence);
    assert!(run.is_complete());
    assert_eq!(run.completed_steps(), run.total_steps());
    assert_eq!(run.advance(&mut dense).unwrap(), Advance::Complete);

    assert_ort_close("Y", run.output(), oracle.y);
    assert_ort_close("Y_h", run.hidden(), oracle.y_h);
    assert_ort_close("Y_c", run.cell(), oracle.y_c);

    let scalar_output = run.output().to_vec();
    let scalar_hidden = run.hidden().to_vec();
    let scalar_cell = run.cell().to_vec();
    let mut dispatched = DispatchedDense::detect();
    let (vector_output, vector_hidden, vector_cell) = run_to_completion(problem, &mut dispatched);
    assert_bits_equal("dispatched Y", &vector_output, &scalar_output);
    assert_bits_equal("dispatched Y_h", &vector_hidden, &scalar_hidden);
    assert_bits_equal("dispatched Y_c", &vector_cell, &scalar_cell);
    if dispatched.supports(DenseLane::Avx2Fma) {
        assert_eq!(dispatched.last_path(), Some(DensePath::Avx2Gather));
    } else {
        assert_eq!(dispatched.last_path(), Some(DensePath::Scalar));
    }
}

fn run_to_completion<D>(problem: Problem<'_>, dense: &mut D) -> (Vec<f32>, Vec<f32>, Vec<f32>)
where
    D: DenseKernel,
    D::Error: core::fmt::Debug,
{
    let mut output = vec![f32::NAN; problem.output_elements()];
    let mut hidden = vec![7.0; STATE_ELEMENTS];
    let mut cell = vec![8.0; STATE_ELEMENTS];
    let mut gates = vec![9.0; GATE_SCRATCH_ELEMENTS];
    let buffers = Buffers::new(&mut output, &mut hidden, &mut cell, &mut gates);
    let mut run = CooperativeLstm::start_zeroed(problem, buffers).unwrap();
    while !run.is_complete() {
        assert!(matches!(run.advance(dense).unwrap(), Advance::Advanced(_)));
    }
    assert_eq!(run.advance(dense).unwrap(), Advance::Complete);
    (run.output().to_vec(), run.hidden().to_vec(), run.cell().to_vec())
}

fn assert_bits_equal(label: &str, actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len(), "{label} length");
    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(
            actual.to_bits(),
            expected.to_bits(),
            "{label}[{index}]: actual={actual:?}, expected={expected:?}"
        );
    }
}

#[test]
fn mlas_fma3_default_activations_match_ort_1_28_bits() {
    // Generated with ONNX Runtime 1.28.0 CPUExecutionProvider on the x86-64
    // development host. More than eight elements force the MLAS FMA3 vector
    // kernel and the endpoints cover both of its clamped domains.
    const INPUTS: [f32; 18] = [
        -100.0, -20.0, -18.0, -9.0, -5.0, -2.0, -1.0, -0.125, -0.0, 0.0, 0.125, 1.0, 2.0, 5.0, 9.0,
        18.0, 20.0, 100.0,
    ];
    const LOGISTIC_BITS: [u32; 18] = [
        0x0000_0000,
        0x0000_0000,
        0x0000_0000,
        0x3901_6000,
        0x3bdb_5040,
        0x3df4_20a8,
        0x3e89_b2b1,
        0x3ef0_0553,
        0x3f00_0000,
        0x3f00_0000,
        0x3f07_fd56,
        0x3f3b_26a8,
        0x3f61_7beb,
        0x3f7e_4960,
        0x3f7f_f7ea,
        0x3f80_0000,
        0x3f80_0000,
        0x3f80_0000,
    ];
    const TANH_BITS: [u32; 18] = [
        0xbf80_0000,
        0xbf80_0000,
        0xbf80_0000,
        0xbf80_0000,
        0xbf7f_fa0c,
        0xbf76_ca83,
        0xbf42_f7d6,
        0xbdfe_acc9,
        0x8000_0000,
        0x0000_0000,
        0x3dfe_acc9,
        0x3f42_f7d6,
        0x3f76_ca83,
        0x3f7f_fa0c,
        0x3f80_0000,
        0x3f80_0000,
        0x3f80_0000,
        0x3f80_0000,
    ];

    for (index, input) in INPUTS.into_iter().enumerate() {
        assert_eq!(mlas_logistic(input).to_bits(), LOGISTIC_BITS[index], "logistic({input:?})");
        assert_eq!(mlas_tanh(input).to_bits(), TANH_BITS[index], "tanh({input:?})");
    }
    assert!(mlas_logistic(f32::NAN).is_nan());
    assert!(mlas_tanh(f32::NAN).is_nan());
}

#[test]
fn input_and_recurrent_bias_are_rounded_together_before_gate_addition() {
    let mut gate = [100_000_000.0];
    add_fused_bias(&mut gate, &[-100_000_000.0], &[1.0]);
    assert_eq!(gate, [0.0]);
}

#[test]
fn official_ort_1_27_text512_semantic_oracle_matches() {
    run_fixture(TEXT_FIXTURE, InputWidth::Text512);
}

#[test]
fn official_ort_1_27_prosody640_real_shape_matches() {
    run_fixture(PROSODY_FIXTURE, InputWidth::Prosody640);
}

#[test]
fn scalar_dense_accumulates_row_major_with_existing_output() {
    let matrix = [1.0, 2.0, 3.0, -4.0, 5.0, -6.0];
    let vector = [0.5, -2.0, 4.0];
    let mut output = [10.0, -3.0];
    ScalarDense
        .accumulate(2, 3, &matrix, &vector, &mut output)
        .unwrap();
    assert_eq!(output, [18.5, -39.0]);
}

#[test]
fn avx_gather_and_prepacked_lanes_preserve_every_scalar_fma_bit() {
    let capabilities = DispatchedDense::detect().capabilities();
    if !capabilities.supports(DenseLane::Avx2Fma) {
        return;
    }
    for columns in [HIDDEN_SIZE, 512, 640] {
        let matrix: Vec<_> = (0..GATE_ELEMENTS * columns)
            .map(|index| (((index * 17) % 37) as i32 - 18) as f32 / 10.0)
            .collect();
        let vector: Vec<_> = (0..columns)
            .map(|index| (((index * 11) % 31) as i32 - 15) as f32 / 10.0)
            .collect();
        let initial: Vec<_> = (0..GATE_ELEMENTS)
            .map(|index| (((index * 7) % 23) as i32 - 11) as f32 / 10.0)
            .collect();
        let mut scalar = initial.clone();
        ScalarDense
            .accumulate(GATE_ELEMENTS, columns, &matrix, &vector, &mut scalar)
            .unwrap();

        let mut gather = initial.clone();
        let mut dispatched = DispatchedDense::detect();
        dispatched
            .accumulate_with_lane(
                GATE_ELEMENTS,
                columns,
                &matrix,
                &vector,
                &mut gather,
                DenseLane::Avx2Fma,
            )
            .unwrap();
        assert_eq!(dispatched.last_path(), Some(DensePath::Avx2Gather));
        assert_bits_equal("AVX gather", &gather, &scalar);

        let mut packed_storage = vec![0.0; matrix.len()];
        let packed =
            PrepackedMatrix::pack(&matrix, GATE_ELEMENTS, columns, &mut packed_storage).unwrap();
        let bindings = [packed];
        let mut dispatched = DispatchedDense::detect_with_prepacked(&bindings);
        let mut packed_output = initial.clone();
        dispatched
            .accumulate_with_lane(
                GATE_ELEMENTS,
                columns,
                &matrix,
                &vector,
                &mut packed_output,
                DenseLane::Avx2Fma,
            )
            .unwrap();
        assert_eq!(dispatched.last_path(), Some(DensePath::Avx2Prepacked));
        assert_bits_equal("AVX prepacked", &packed_output, &scalar);
    }
}

fn assert_prepacked_full_output(width: InputWidth, sequence: usize) {
    let model = OwnedProblem::fixture(sequence, width);
    let scalar_problem = model.problem();
    let mut scalar = ScalarDense;
    let expected = run_to_completion(scalar_problem, &mut scalar);

    let input_matrix_elements = GATE_ELEMENTS * width.channels();
    let recurrent_matrix_elements = GATE_ELEMENTS * HIDDEN_SIZE;
    let mut packed_input_0 = vec![0.0; input_matrix_elements];
    let mut packed_input_1 = vec![0.0; input_matrix_elements];
    let mut packed_recurrent_0 = vec![0.0; recurrent_matrix_elements];
    let mut packed_recurrent_1 = vec![0.0; recurrent_matrix_elements];
    let input_0 = &model.weights[..input_matrix_elements];
    let input_1 = &model.weights[input_matrix_elements..2 * input_matrix_elements];
    let recurrent_0 = &model.recurrent[..recurrent_matrix_elements];
    let recurrent_1 = &model.recurrent[recurrent_matrix_elements..2 * recurrent_matrix_elements];
    let bindings = [
        PrepackedMatrix::pack(input_0, GATE_ELEMENTS, width.channels(), &mut packed_input_0)
            .unwrap(),
        PrepackedMatrix::pack(input_1, GATE_ELEMENTS, width.channels(), &mut packed_input_1)
            .unwrap(),
        PrepackedMatrix::pack(recurrent_0, GATE_ELEMENTS, HIDDEN_SIZE, &mut packed_recurrent_0)
            .unwrap(),
        PrepackedMatrix::pack(recurrent_1, GATE_ELEMENTS, HIDDEN_SIZE, &mut packed_recurrent_1)
            .unwrap(),
    ];
    let mut dispatched = DispatchedDense::detect_with_prepacked(&bindings);
    let actual = run_to_completion(model.problem(), &mut dispatched);
    assert_bits_equal("prepacked full Y", &actual.0, &expected.0);
    assert_bits_equal("prepacked full Y_h", &actual.1, &expected.1);
    assert_bits_equal("prepacked full Y_c", &actual.2, &expected.2);
    if dispatched.supports(DenseLane::Avx2Fma) {
        assert_eq!(dispatched.last_path(), Some(DensePath::Avx2Prepacked));
    }
}

#[test]
fn prepacked_adapter_preserves_full_text_and_prosody_outputs() {
    assert_prepacked_full_output(InputWidth::Text512, 3);
    assert_prepacked_full_output(InputWidth::Prosody640, 2);
}

#[test]
fn dispatched_validation_and_capability_contract_fail_closed() {
    let dispatcher = DispatchedDense::detect();
    let capabilities = dispatcher.capabilities();
    if capabilities.supports(DenseLane::Avx2Fma) {
        assert!(capabilities.ymm_state());
        assert!(capabilities.avx2());
        assert!(capabilities.fma());
    }

    let matrix = vec![0.0; GATE_ELEMENTS * HIDDEN_SIZE];
    let vector = vec![0.0; HIDDEN_SIZE];
    let mut accumulator = vec![17.0; GATE_ELEMENTS];
    let mut scalar_only = DispatchedDense {
        capabilities: CpuCapabilities::default(),
        prepacked: &[],
        last_path: None,
    };
    assert_eq!(
        scalar_only.accumulate_with_lane(
            GATE_ELEMENTS,
            HIDDEN_SIZE,
            &matrix,
            &vector,
            &mut accumulator,
            DenseLane::Avx2Fma,
        ),
        Err(DenseError::UnsupportedLane)
    );
    assert!(accumulator.iter().all(|&value| value == 17.0));
    assert_eq!(
        scalar_only.accumulate_with_lane(
            GATE_ELEMENTS - 1,
            HIDDEN_SIZE,
            &matrix,
            &vector,
            &mut accumulator,
            DenseLane::Scalar,
        ),
        Err(DenseError::UnsupportedShape)
    );
    assert!(accumulator.iter().all(|&value| value == 17.0));
    let mut short_packed = vec![0.0; matrix.len() - 1];
    assert_eq!(
        PrepackedMatrix::pack(&matrix, GATE_ELEMENTS, HIDDEN_SIZE, &mut short_packed,).unwrap_err(),
        DenseError::PackedLengthMismatch
    );
}

#[test]
fn rejected_workspace_is_transactional() {
    let model = OwnedProblem::fixture(1, InputWidth::Text512);
    let problem = model.problem();
    let mut output = vec![31.0; problem.output_elements() - 1];
    let mut hidden = vec![32.0; STATE_ELEMENTS];
    let mut cell = vec![33.0; STATE_ELEMENTS];
    let mut gates = vec![34.0; GATE_SCRATCH_ELEMENTS];

    let result = CooperativeLstm::start_zeroed(
        problem,
        Buffers::new(&mut output, &mut hidden, &mut cell, &mut gates),
    );
    assert!(matches!(result, Err(ContractError::OutputLengthMismatch)));
    assert!(output.iter().all(|&value| value == 31.0));
    assert!(hidden.iter().all(|&value| value == 32.0));
    assert!(cell.iter().all(|&value| value == 33.0));
    assert!(gates.iter().all(|&value| value == 34.0));
}

#[test]
fn rejected_explicit_state_is_transactional() {
    let model = OwnedProblem::fixture(1, InputWidth::Text512);
    let problem = model.problem();
    let mut output = vec![41.0; problem.output_elements()];
    let mut hidden = vec![42.0; STATE_ELEMENTS];
    let mut cell = vec![43.0; STATE_ELEMENTS];
    let mut gates = vec![44.0; GATE_SCRATCH_ELEMENTS];
    let initial_hidden = vec![1.0; STATE_ELEMENTS];
    let initial_cell = vec![2.0; STATE_ELEMENTS - 1];

    let result = CooperativeLstm::start_with_state(
        problem,
        Buffers::new(&mut output, &mut hidden, &mut cell, &mut gates),
        &initial_hidden,
        &initial_cell,
    );
    assert!(matches!(result, Err(ContractError::InitialCellLengthMismatch)));
    assert!(output.iter().all(|&value| value == 41.0));
    assert!(hidden.iter().all(|&value| value == 42.0));
    assert!(cell.iter().all(|&value| value == 43.0));
    assert!(gates.iter().all(|&value| value == 44.0));
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InjectedError {
    SecondDenseCall,
}

struct FailSecondDense {
    calls: usize,
}

impl DenseKernel for FailSecondDense {
    type Error = InjectedError;

    fn accumulate(
        &mut self,
        rows: usize,
        columns: usize,
        matrix: &[f32],
        vector: &[f32],
        accumulator: &mut [f32],
    ) -> Result<(), Self::Error> {
        self.calls += 1;
        if self.calls == 2 {
            accumulator.fill(1234.0);
            return Err(InjectedError::SecondDenseCall);
        }
        let result = ScalarDense.accumulate(rows, columns, matrix, vector, accumulator);
        match result {
            Ok(()) => Ok(()),
            Err(never) => match never {},
        }
    }
}

#[test]
fn dense_failure_changes_only_disposable_gates_and_can_retry() {
    let model = OwnedProblem::fixture(1, InputWidth::Text512);
    let problem = model.problem();
    let mut output = vec![51.0; problem.output_elements()];
    let mut hidden = vec![52.0; STATE_ELEMENTS];
    let mut cell = vec![53.0; STATE_ELEMENTS];
    let mut gates = vec![54.0; GATE_SCRATCH_ELEMENTS];
    let initial_hidden: Vec<_> = (0..STATE_ELEMENTS)
        .map(|index| index as f32 / 4096.0)
        .collect();
    let initial_cell: Vec<_> = (0..STATE_ELEMENTS)
        .map(|index| -(index as f32) / 2048.0)
        .collect();
    let buffers = Buffers::new(&mut output, &mut hidden, &mut cell, &mut gates);
    let mut run =
        CooperativeLstm::start_with_state(problem, buffers, &initial_hidden, &initial_cell)
            .unwrap();
    let output_before = run.output().to_vec();
    let hidden_before = run.hidden().to_vec();
    let cell_before = run.cell().to_vec();

    let mut failing = FailSecondDense { calls: 0 };
    assert_eq!(run.advance(&mut failing), Err(InjectedError::SecondDenseCall));
    assert_eq!(run.next_step(), Some((0, 0)));
    assert_eq!(run.completed_steps(), 0);
    assert_eq!(run.output(), output_before);
    assert_eq!(run.hidden(), hidden_before);
    assert_eq!(run.cell(), cell_before);

    let mut scalar = ScalarDense;
    assert!(matches!(
        run.advance(&mut scalar).unwrap(),
        Advance::Advanced(CompletedStep {
            direction: 0,
            sequence_index: 0,
            completed_steps: 1,
            total_steps: 2,
        })
    ));
    assert_eq!(run.next_step(), Some((1, 0)));
}

#[test]
fn exact_problem_contract_rejects_nearby_shapes() {
    assert!(matches!(
        Problem::new(0, InputWidth::Text512, &[], &[], &[], &[]),
        Err(ContractError::EmptySequence)
    ));
    assert!(matches!(
        Problem::new(MAX_SEQUENCE_LENGTH + 1, InputWidth::Text512, &[], &[], &[], &[],),
        Err(ContractError::SequenceTooLong)
    ));

    let model = OwnedProblem::fixture(1, InputWidth::Prosody640);
    assert!(matches!(
        Problem::new(
            1,
            InputWidth::Text512,
            &model.input,
            &model.weights,
            &model.recurrent,
            &model.bias,
        ),
        Err(ContractError::InputLengthMismatch)
    ));
    assert!(matches!(
        Problem::new(
            1,
            InputWidth::Prosody640,
            &model.input,
            &model.weights[..model.weights.len() - 1],
            &model.recurrent,
            &model.bias,
        ),
        Err(ContractError::WeightLengthMismatch)
    ));
}
