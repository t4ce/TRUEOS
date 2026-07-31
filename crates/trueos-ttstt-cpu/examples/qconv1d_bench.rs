use std::hint::black_box;
use std::time::{Duration, Instant};
use trueos_ttstt_cpu::{
    Dispatcher, Lane, QConv1dParams, RhsZeroPoints, pack_conv1d_weights_u8, signed_u8_zero_point,
};

// Exact profiled Kokoro hotspot: `noise_res.1/convs1.2` (node 2570).
const INPUT_CHANNELS: usize = 128;
const OUTPUT_CHANNELS: usize = 128;
const INPUT_WIDTH: usize = 8_281;
const KERNEL_WIDTH: usize = 11;
const DILATION: usize = 5;
const PAD: usize = 25;
const ITERATIONS: usize = 2;

fn main() {
    let dispatcher = Dispatcher::detect();
    println!(
        "shape=1x{INPUT_CHANNELS}x{INPUT_WIDTH} weights={OUTPUT_CHANNELS}x{INPUT_CHANNELS}x{KERNEL_WIDTH} iterations={ITERATIONS} detected={:?} best={}",
        dispatcher.capabilities(),
        dispatcher.best_lane().as_str()
    );

    let mut state = 0x243F_6A88_85A3_08D3u64;
    let mut input = vec![0u8; INPUT_CHANNELS * INPUT_WIDTH];
    let mut weights = vec![0u8; OUTPUT_CHANNELS * INPUT_CHANNELS * KERNEL_WIDTH];
    for value in input.iter_mut().chain(weights.iter_mut()) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *value = state as u8;
    }

    let mut packed = vec![0i8; weights.len()];
    let mut weight_sums = vec![0i32; OUTPUT_CHANNELS];
    pack_conv1d_weights_u8(
        &weights,
        OUTPUT_CHANNELS,
        INPUT_CHANNELS,
        KERNEL_WIDTH,
        &mut packed,
        &mut weight_sums,
    )
    .unwrap();
    let params = QConv1dParams {
        batch: 1,
        input_channels: INPUT_CHANNELS,
        input_width: INPUT_WIDTH,
        output_channels: OUTPUT_CHANNELS,
        kernel_width: KERNEL_WIDTH,
        stride: 1,
        dilation: DILATION,
        pad_left: PAD,
        pad_right: PAD,
        input_zero_point: 96,
        weight_zero_points: RhsZeroPoints::Scalar(signed_u8_zero_point(61)),
        weight_row_sums: Some(&weight_sums),
    };
    let output_width = params.output_width().unwrap();
    let mut output = vec![0i32; OUTPUT_CHANNELS * output_width];
    // Four patches activate the VNNI temporal microkernel. The API still
    // accepts one patch for low-memory callers and non-VNNI fallbacks.
    let mut patch = vec![0u8; INPUT_CHANNELS * KERNEL_WIDTH * 4];
    let mut scalar_reference = None;

    for lane in [Lane::Scalar, Lane::Avx2, Lane::AvxVnni] {
        if !dispatcher.supports(lane) {
            println!("{:>9}: unavailable", lane.as_str());
            continue;
        }
        dispatcher
            .qconv1d_with_lane(&input, &packed, &mut output, &mut patch, params, lane)
            .unwrap();
        if let Some(expected) = scalar_reference.as_ref() {
            assert_eq!(&output, expected);
        } else {
            scalar_reference = Some(output.clone());
        }

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            dispatcher
                .qconv1d_with_lane(
                    black_box(&input),
                    black_box(&packed),
                    black_box(&mut output),
                    black_box(&mut patch),
                    params,
                    lane,
                )
                .unwrap();
        }
        report(lane, start.elapsed(), checksum(&output));
    }
}

fn report(lane: Lane, elapsed: Duration, checksum: u32) {
    let integer_ops = (INPUT_WIDTH as f64)
        * (OUTPUT_CHANNELS as f64)
        * (INPUT_CHANNELS as f64)
        * (KERNEL_WIDTH as f64)
        * 2.0
        * (ITERATIONS as f64);
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
