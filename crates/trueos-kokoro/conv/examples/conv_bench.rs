use std::hint::black_box;
use std::time::{Duration, Instant};

use trueos_kokoro_conv::{Dispatcher, Lane, Problem, Profile};

fn pattern(
    length: usize,
    multiplier: usize,
    modulus: usize,
    center: i32,
    divisor: f32,
) -> Vec<f32> {
    (0..length)
        .map(|index| (((index * multiplier) % modulus) as i32 - center) as f32 / divisor)
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

fn bench_lane(
    dispatcher: Dispatcher,
    problem: Problem<'_>,
    output: &mut [f32],
    lane: Lane,
    iterations: u32,
) -> Duration {
    dispatcher
        .convolve_with_lane(problem, output, lane)
        .unwrap();
    let started = Instant::now();
    for _ in 0..iterations {
        dispatcher
            .convolve_with_lane(black_box(problem), black_box(output), lane)
            .unwrap();
    }
    started.elapsed() / iterations
}

fn run_case(name: &str, profile: Profile, dispatcher: Dispatcher) {
    let dimensions = profile.dimensions().unwrap();
    let parameters = profile.parameters();
    let input = pattern(dimensions.input_elements().unwrap(), 7, 31, 15, 64.0);
    let weights = pattern(dimensions.weight_elements(parameters.kind).unwrap(), 11, 29, 14, 128.0);
    let bias = pattern(dimensions.output_channels, 5, 17, 8, 256.0);
    let problem = Problem::new(profile, &input, &weights, Some(&bias)).unwrap();
    let operations = dimensions.scalar_fused_operations(parameters.kind).unwrap() as f64;
    let mut scalar_output = vec![0.0; dimensions.output_elements().unwrap()];
    let scalar = bench_lane(dispatcher, problem, &mut scalar_output, Lane::Scalar, 1);
    println!(
        "{name:<31} {:<13} {:>9.3} ms {:>7.3} GFLOP/s fingerprint=0x{:016X}",
        Lane::Scalar.as_str(),
        scalar.as_secs_f64() * 1.0e3,
        operations / scalar.as_secs_f64() / 1.0e9,
        fingerprint(&scalar_output),
    );

    if dispatcher.supports(Lane::Avx2Fma) {
        let mut vector_output = vec![0.0; scalar_output.len()];
        let vector = bench_lane(dispatcher, problem, &mut vector_output, Lane::Avx2Fma, 3);
        assert!(
            scalar_output
                .iter()
                .zip(&vector_output)
                .all(|(scalar, vector)| scalar.to_bits() == vector.to_bits())
        );
        println!(
            "{name:<31} {:<13} {:>9.3} ms {:>7.3} GFLOP/s speedup={:>5.2}x fingerprint=0x{:016X}",
            Lane::Avx2Fma.as_str(),
            vector.as_secs_f64() * 1.0e3,
            operations / vector.as_secs_f64() / 1.0e9,
            scalar.as_secs_f64() / vector.as_secs_f64(),
            fingerprint(&vector_output),
        );
    }
}

fn main() {
    let dispatcher = Dispatcher::detect();
    println!(
        "Kokoro float Conv: best={} avx2={} fma={} ymm={}",
        dispatcher.best_lane().as_str(),
        dispatcher.capabilities().avx2(),
        dispatcher.capabilities().fma(),
        dispatcher.capabilities().ymm_state(),
    );
    run_case(
        "upsample0 512x138 -> 256x1380",
        Profile::Upsample512To256 { input_width: 138 },
        dispatcher,
    );
    run_case(
        "upsample1 256x1380 -> 128x8280",
        Profile::Upsample256To128 { input_width: 1_380 },
        dispatcher,
    );
}
