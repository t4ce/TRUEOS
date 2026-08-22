use std::hint::black_box;
use std::time::{Duration, Instant};

use trueos_kokoro_lstm::{
    DenseKernel, DenseLane, DispatchedDense, GATE_ELEMENTS, PrepackedMatrix, ScalarDense,
};

fn pattern(length: usize, multiplier: usize, modulus: usize, center: i32) -> Vec<f32> {
    (0..length)
        .map(|index| (((index * multiplier) % modulus) as i32 - center) as f32 / 10.0)
        .collect()
}

fn fingerprint(values: &[f32]) -> u64 {
    let mut result = 0xcbf2_9ce4_8422_2325u64;
    for value in values {
        for byte in value.to_bits().to_le_bytes() {
            result ^= u64::from(byte);
            result = result.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    result
}

fn bench_scalar(
    matrix: &[f32],
    vector: &[f32],
    initial: &[f32],
    output: &mut [f32],
    iterations: u32,
) -> Duration {
    let mut dense = ScalarDense;
    output.copy_from_slice(initial);
    dense
        .accumulate(GATE_ELEMENTS, vector.len(), matrix, vector, output)
        .unwrap();
    let started = Instant::now();
    for _ in 0..iterations {
        output.copy_from_slice(black_box(initial));
        dense
            .accumulate(
                GATE_ELEMENTS,
                vector.len(),
                black_box(matrix),
                black_box(vector),
                black_box(output),
            )
            .unwrap();
    }
    started.elapsed() / iterations
}

fn bench_dispatched(
    dense: &mut DispatchedDense<'_>,
    matrix: &[f32],
    vector: &[f32],
    initial: &[f32],
    output: &mut [f32],
    iterations: u32,
) -> Duration {
    output.copy_from_slice(initial);
    dense
        .accumulate_with_lane(
            GATE_ELEMENTS,
            vector.len(),
            matrix,
            vector,
            output,
            DenseLane::Avx2Fma,
        )
        .unwrap();
    let started = Instant::now();
    for _ in 0..iterations {
        output.copy_from_slice(black_box(initial));
        dense
            .accumulate_with_lane(
                GATE_ELEMENTS,
                vector.len(),
                black_box(matrix),
                black_box(vector),
                black_box(output),
                DenseLane::Avx2Fma,
            )
            .unwrap();
    }
    started.elapsed() / iterations
}

fn run_case(columns: usize) {
    let matrix = pattern(GATE_ELEMENTS * columns, 17, 37, 18);
    let vector = pattern(columns, 11, 31, 15);
    let initial = pattern(GATE_ELEMENTS, 7, 23, 11);
    let operations = (2 * GATE_ELEMENTS * columns) as f64;

    let mut scalar_output = vec![0.0; GATE_ELEMENTS];
    let scalar = bench_scalar(&matrix, &vector, &initial, &mut scalar_output, 30);
    println!(
        "K={columns:<3} {:<17} {:>8.3} ms {:>7.3} GFLOP/s fingerprint=0x{:016X}",
        DenseLane::Scalar.as_str(),
        scalar.as_secs_f64() * 1.0e3,
        operations / scalar.as_secs_f64() / 1.0e9,
        fingerprint(&scalar_output),
    );

    let probe = DispatchedDense::detect();
    if !probe.supports(DenseLane::Avx2Fma) {
        return;
    }

    let mut gather_output = vec![0.0; GATE_ELEMENTS];
    let mut gather = DispatchedDense::detect();
    let gather_time =
        bench_dispatched(&mut gather, &matrix, &vector, &initial, &mut gather_output, 100);
    assert!(
        gather_output
            .iter()
            .zip(&scalar_output)
            .all(|(vector, scalar)| vector.to_bits() == scalar.to_bits())
    );
    println!(
        "K={columns:<3} {:<17} {:>8.3} ms {:>7.3} GFLOP/s speedup={:>5.2}x",
        "avx2-fma-gather",
        gather_time.as_secs_f64() * 1.0e3,
        operations / gather_time.as_secs_f64() / 1.0e9,
        scalar.as_secs_f64() / gather_time.as_secs_f64(),
    );

    let mut packed_storage = vec![0.0; matrix.len()];
    let pack_started = Instant::now();
    let packed =
        PrepackedMatrix::pack(&matrix, GATE_ELEMENTS, columns, &mut packed_storage).unwrap();
    let pack_time = pack_started.elapsed();
    let bindings = [packed];
    let mut packed_dense = DispatchedDense::detect_with_prepacked(&bindings);
    let mut packed_output = vec![0.0; GATE_ELEMENTS];
    let packed_time =
        bench_dispatched(&mut packed_dense, &matrix, &vector, &initial, &mut packed_output, 200);
    assert!(
        packed_output
            .iter()
            .zip(&scalar_output)
            .all(|(vector, scalar)| vector.to_bits() == scalar.to_bits())
    );
    println!(
        "K={columns:<3} {:<17} {:>8.3} ms {:>7.3} GFLOP/s speedup={:>5.2}x pack={:.3} ms",
        "avx2-fma-packed",
        packed_time.as_secs_f64() * 1.0e3,
        operations / packed_time.as_secs_f64() / 1.0e9,
        scalar.as_secs_f64() / packed_time.as_secs_f64(),
        pack_time.as_secs_f64() * 1.0e3,
    );
}

fn main() {
    let dispatcher = DispatchedDense::detect();
    println!(
        "Kokoro LSTM dense: best={} avx2={} fma={} ymm={}",
        dispatcher.best_lane().as_str(),
        dispatcher.capabilities().avx2(),
        dispatcher.capabilities().fma(),
        dispatcher.capabilities().ymm_state(),
    );
    run_case(512);
    run_case(640);
}
