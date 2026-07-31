use std::{env, hint::black_box, time::Instant};

use trueos_kokoro_f32::{
    ElementwiseLane, Error, Shape, TensorLayout, add_on_lane, div_on_lane, mul_on_lane,
    pow_square_on_lane, sub_on_lane,
};

type BinaryKernel = fn(
    ElementwiseLane,
    &[f32],
    TensorLayout,
    &[f32],
    TensorLayout,
    &mut [f32],
    TensorLayout,
) -> Result<(), Error>;

fn main() {
    let elements = argument(1, 1_000_003);
    let iterations = argument(2, 20);
    let shape = Shape::new(&[elements]).expect("valid benchmark shape");
    let layout = TensorLayout::contiguous(shape);
    let scalar_layout = TensorLayout::contiguous(Shape::scalar());
    let lhs: Vec<_> = (0..elements)
        .map(|index| (index % 251) as f32 * (1.0 / 32.0) - 3.75)
        .collect();
    let rhs: Vec<_> = (0..elements)
        .map(|index| (index % 127 + 1) as f32 * (1.0 / 64.0))
        .collect();
    let scalar = [-0.75_f32];
    let mut output = vec![0.0_f32; elements];

    println!(
        "elements={elements} iterations={iterations} avx2={} bytes_per_tensor={}",
        ElementwiseLane::Avx2.is_available(),
        elements * size_of::<f32>()
    );
    if !ElementwiseLane::Avx2.is_available() {
        println!("AVX2 is unavailable after CPUID+OSXSAVE+XCR0 gating; benchmark skipped");
        return;
    }
    println!("kernel,scalar_ms,avx2_ms,speedup,checksum");

    for (name, kernel) in [
        ("add-pair", add_on_lane as BinaryKernel),
        ("mul-pair", mul_on_lane as BinaryKernel),
        ("div-pair", div_on_lane as BinaryKernel),
        ("sub-pair", sub_on_lane as BinaryKernel),
    ] {
        compare_binary(name, kernel, &lhs, layout, &rhs, layout, &mut output, layout, iterations);
    }

    for (name, kernel) in [
        ("add-rhs-scalar", add_on_lane as BinaryKernel),
        ("mul-rhs-scalar", mul_on_lane as BinaryKernel),
        ("div-rhs-scalar", div_on_lane as BinaryKernel),
        ("sub-rhs-scalar", sub_on_lane as BinaryKernel),
    ] {
        compare_binary(
            name,
            kernel,
            &lhs,
            layout,
            &scalar,
            scalar_layout,
            &mut output,
            layout,
            iterations,
        );
    }

    compare_square(&lhs, layout, &mut output, iterations);
}

#[allow(clippy::too_many_arguments)]
fn compare_binary(
    name: &str,
    kernel: BinaryKernel,
    lhs: &[f32],
    lhs_layout: TensorLayout,
    rhs: &[f32],
    rhs_layout: TensorLayout,
    output: &mut [f32],
    output_layout: TensorLayout,
    iterations: usize,
) {
    kernel(ElementwiseLane::Avx2, lhs, lhs_layout, rhs, rhs_layout, output, output_layout)
        .expect("AVX2 warmup");
    kernel(ElementwiseLane::Scalar, lhs, lhs_layout, rhs, rhs_layout, output, output_layout)
        .expect("scalar warmup");

    let scalar = time(iterations, || {
        kernel(
            ElementwiseLane::Scalar,
            black_box(lhs),
            lhs_layout,
            black_box(rhs),
            rhs_layout,
            black_box(output),
            output_layout,
        )
        .expect("scalar benchmark")
    });
    let vector = time(iterations, || {
        kernel(
            ElementwiseLane::Avx2,
            black_box(lhs),
            lhs_layout,
            black_box(rhs),
            rhs_layout,
            black_box(output),
            output_layout,
        )
        .expect("AVX2 benchmark")
    });
    report(name, scalar, vector, output);
}

fn compare_square(input: &[f32], layout: TensorLayout, output: &mut [f32], iterations: usize) {
    pow_square_on_lane(ElementwiseLane::Avx2, input, layout, output, layout).expect("AVX2 warmup");
    pow_square_on_lane(ElementwiseLane::Scalar, input, layout, output, layout)
        .expect("scalar warmup");
    let scalar = time(iterations, || {
        pow_square_on_lane(
            ElementwiseLane::Scalar,
            black_box(input),
            layout,
            black_box(output),
            layout,
        )
        .expect("scalar benchmark")
    });
    let vector = time(iterations, || {
        pow_square_on_lane(
            ElementwiseLane::Avx2,
            black_box(input),
            layout,
            black_box(output),
            layout,
        )
        .expect("AVX2 benchmark")
    });
    report("pow-square", scalar, vector, output);
}

fn time(mut iterations: usize, mut body: impl FnMut()) -> f64 {
    iterations = iterations.max(1);
    let start = Instant::now();
    for _ in 0..iterations {
        body();
    }
    start.elapsed().as_secs_f64() * 1_000.0
}

fn report(name: &str, scalar_ms: f64, vector_ms: f64, output: &[f32]) {
    let checksum = output
        .iter()
        .step_by((output.len() / 64).max(1))
        .fold(0_u32, |sum, value| sum.wrapping_add(value.to_bits()));
    println!("{name},{scalar_ms:.3},{vector_ms:.3},{:.2}x,{checksum:08x}", scalar_ms / vector_ms);
}

fn argument(index: usize, fallback: usize) -> usize {
    env::args()
        .nth(index)
        .map(|value| value.parse().expect("arguments must be positive integers"))
        .filter(|&value| value != 0)
        .unwrap_or(fallback)
}
