use std::hint::black_box;
use std::time::Instant;
use trueos_ttstt_cpu::{
    Dispatcher, Lane, QConv1dParams, RhsZeroPoints, pack_conv1d_weights_u8, signed_u8_zero_point,
};

const C: usize = 128;
const O: usize = 128;
const W: usize = 8_281;
const K: usize = 11;
const D: usize = 5;
const ITERATIONS: usize = 5;

fn main() {
    let dispatcher = Dispatcher::detect();
    let mut state = 0x243F_6A88_85A3_08D3u64;
    let mut input = vec![0.0f32; C * W];
    let mut weights = vec![0u8; O * C * K];
    for value in &mut input {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *value = (state as i16) as f32 / 8192.0;
    }
    for value in &mut weights {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *value = state as u8;
    }
    let mut packed = vec![0i8; weights.len()];
    let mut sums = vec![0i32; O];
    pack_conv1d_weights_u8(&weights, O, C, K, &mut packed, &mut sums).unwrap();
    let params = QConv1dParams {
        batch: 1,
        input_channels: C,
        input_width: W,
        output_channels: O,
        kernel_width: K,
        stride: 1,
        dilation: D,
        pad_left: 25,
        pad_right: 25,
        input_zero_point: 0,
        weight_zero_points: RhsZeroPoints::Scalar(signed_u8_zero_point(61)),
        weight_row_sums: Some(&sums),
    };
    let mut output = vec![0.0f32; O * params.output_width().unwrap()];
    let mut patch = vec![0u8; 13_080];
    if !dispatcher.supports(Lane::AvxVnni) {
        println!("avx-vnni unavailable");
        return;
    }
    run(dispatcher, &input, &packed, &mut output, &mut patch, params);
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        run(
            dispatcher,
            black_box(&input),
            black_box(&packed),
            black_box(&mut output),
            black_box(&mut patch),
            params,
        );
    }
    let checksum = checksum(&output);
    assert_eq!(checksum, 0x1EB1_CAE7);
    println!(
        "elapsed_ms={:.3} iterations={ITERATIONS} checksum={checksum:08x}",
        start.elapsed().as_secs_f64() * 1e3 / ITERATIONS as f64,
    );
}

fn run(
    dispatcher: Dispatcher,
    input: &[f32],
    packed: &[i8],
    output: &mut [f32],
    patch: &mut [u8],
    params: QConv1dParams<'_>,
) {
    dispatcher
        .qconv1d_dequantized_with_lane(
            input,
            packed,
            output,
            patch,
            &mut [],
            params,
            0.03125,
            None,
            Lane::AvxVnni,
        )
        .unwrap();
}

fn checksum(values: &[f32]) -> u32 {
    values
        .iter()
        .enumerate()
        .fold(0x9E37_79B9u32, |sum, (index, value)| {
            sum.rotate_left(5) ^ value.to_bits().rotate_left(((index & 31) + 1) as u32)
        })
}
