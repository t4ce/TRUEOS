use std::hint::black_box;
use std::time::{Duration, Instant};

use trueos_kokoro_gemm::{Dispatcher, DurationChannels, KokoroMatMul, Lane};

fn pattern(length: usize, multiplier: usize, modulus: usize, center: i32) -> Vec<f32> {
    (0..length)
        .map(|index| (((index * multiplier) % modulus) as i32 - center) as f32 / 64.0)
        .collect()
}

fn bench_lane(
    dispatcher: Dispatcher,
    profile: KokoroMatMul,
    lane: Lane,
    iterations: usize,
) -> (Duration, f64, u32) {
    let dimensions = profile.dimensions().unwrap();
    let lhs = pattern(dimensions.lhs_elements().unwrap(), 17, 31, 15);
    let rhs = pattern(dimensions.rhs_elements().unwrap(), 13, 29, 14);
    let mut output = vec![0.0f32; dimensions.output_elements().unwrap()];
    dispatcher
        .matmul_with_lane(profile, &lhs, &rhs, &mut output, lane)
        .unwrap();

    let started = Instant::now();
    for _ in 0..iterations {
        dispatcher
            .matmul_with_lane(
                black_box(profile),
                black_box(&lhs),
                black_box(&rhs),
                black_box(&mut output),
                lane,
            )
            .unwrap();
    }
    let elapsed = started.elapsed();
    let operations = dimensions.scalar_operations().unwrap() as f64 * iterations as f64;
    let gflops = operations / elapsed.as_secs_f64() / 1.0e9;
    let checksum = output
        .iter()
        .fold(0u32, |accumulator, value| accumulator ^ value.to_bits());
    (elapsed / iterations as u32, gflops, checksum)
}

fn main() {
    let dispatcher = Dispatcher::detect();
    let best = dispatcher.best_lane();
    println!(
        "Kokoro f32 GEMM: best={} avx2={} fma={} ymm={}",
        best.as_str(),
        dispatcher.capabilities().avx2(),
        dispatcher.capabilities().fma(),
        dispatcher.capabilities().ymm_state(),
    );

    let cases = [
        ("attention-scores 12x18x64x18", KokoroMatMul::AttentionScores { sequence: 18 }, 100),
        ("attention-context 12x18x18x64", KokoroMatMul::AttentionContext { sequence: 18 }, 100),
        (
            "duration-prosody 640x18x69",
            KokoroMatMul::DurationProjection {
                channels: DurationChannels::Prosody640,
                sequence: 18,
                frames: 69,
            },
            40,
        ),
        (
            "duration-text 512x18x69",
            KokoroMatMul::DurationProjection {
                channels: DurationChannels::Text512,
                sequence: 18,
                frames: 69,
            },
            40,
        ),
        ("source-linear 41400x9x1", KokoroMatMul::SourceLinear { samples: 41_400 }, 100),
    ];

    for (name, profile, iterations) in cases {
        let lanes = if best == Lane::Scalar {
            [Some(Lane::Scalar), None]
        } else {
            [Some(Lane::Scalar), Some(best)]
        };
        for lane in lanes.into_iter().flatten() {
            let (latency, gflops, checksum) = bench_lane(dispatcher, profile, lane, iterations);
            println!(
                "{name:<36} {:<13} {:>9.3} ms  {:>7.3} GFLOP/s checksum=0x{checksum:08X}",
                lane.as_str(),
                latency.as_secs_f64() * 1.0e3,
                gflops,
            );
        }
    }
}
