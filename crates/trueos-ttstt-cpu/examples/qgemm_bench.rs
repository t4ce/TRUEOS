use std::hint::black_box;
use std::time::{Duration, Instant};
use trueos_ttstt_cpu::{Dispatcher, Lane, QGemmParams, RhsZeroPoints, prepare_rhs_row_sums};

const M: usize = 32;
const N: usize = 256;
const K: usize = 768;
const ITERATIONS: usize = 20;

fn main() {
    let dispatcher = Dispatcher::detect();
    println!(
        "shape={M}x{N}x{K} iterations={ITERATIONS} detected={:?} best={}",
        dispatcher.capabilities(),
        dispatcher.best_lane().as_str()
    );

    let mut lhs = vec![0u8; M * K];
    let mut rhs = vec![0i8; N * K];
    let mut state = 0x243F_6A88_85A3_08D3u64;
    for value in &mut lhs {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *value = state as u8;
    }
    for value in &mut rhs {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *value = state as u8 as i8;
    }

    let mut rhs_sums = vec![0i32; N];
    prepare_rhs_row_sums(&rhs, N, K, &mut rhs_sums).unwrap();
    let params = QGemmParams {
        m: M,
        n: N,
        k: K,
        lhs_zero_point: 127,
        rhs_zero_points: RhsZeroPoints::Scalar(0),
        rhs_row_sums: Some(&rhs_sums),
    };
    let mut output = vec![0i32; M * N];
    let mut scalar_reference = None;

    for lane in [Lane::Scalar, Lane::Avx2, Lane::AvxVnni] {
        if !dispatcher.supports(lane) {
            println!("{:>9}: unavailable", lane.as_str());
            continue;
        }
        dispatcher
            .qgemm_with_lane(&lhs, &rhs, &mut output, params, lane)
            .unwrap();
        if let Some(expected) = scalar_reference.as_ref() {
            assert_eq!(&output, expected);
        } else {
            scalar_reference = Some(output.clone());
        }

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            dispatcher
                .qgemm_with_lane(
                    black_box(&lhs),
                    black_box(&rhs),
                    black_box(&mut output),
                    params,
                    lane,
                )
                .unwrap();
        }
        let elapsed = start.elapsed();
        report(lane, elapsed, checksum(&output));
    }
}

fn report(lane: Lane, elapsed: Duration, checksum: u32) {
    let integer_ops = (M as f64) * (N as f64) * (K as f64) * 2.0 * (ITERATIONS as f64);
    let giga_ops_per_second = integer_ops / elapsed.as_secs_f64() / 1.0e9;
    println!(
        "{:>9}: {:8.3} ms total  {:7.2} GOP/s  checksum=0x{checksum:08X}",
        lane.as_str(),
        elapsed.as_secs_f64() * 1.0e3,
        giga_ops_per_second,
    );
}

fn checksum(values: &[i32]) -> u32 {
    values
        .iter()
        .enumerate()
        .fold(0x9E37_79B9u32, |sum, (index, &value)| {
            sum.rotate_left(5) ^ (value as u32).rotate_left(((index & 31) + 1) as u32)
        })
}
