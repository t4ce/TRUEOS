use half::f16;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use trueos_fpga_abi::lfm25::{
    generated::{MODEL_CONTRACT_SHA256, TENSORS, TENSOR_NAMES},
    MODEL_GENERATION, PINNED_GGUF_BYTES, PINNED_GGUF_SHA256, PINNED_NATIVE_IMAGE_BYTES,
    PINNED_NATIVE_IMAGE_SHA256, Q8_0_BLOCK_BYTES, Q8_0_BLOCK_VALUES,
};

const RAW_MAGIC: &[u8; 8] = b"TGALRAW1";
const GOLDEN_MAGIC: &[u8; 8] = b"TGAGFFN1";
const LLAMA_COMMIT: [u8; 20] = [
    0x76, 0xf4, 0x6a, 0xd2, 0x9d, 0x61, 0xfd, 0x8c, 0x14, 0x01, 0xe8, 0x22, 0x18, 0x42, 0x93, 0x4b,
    0xf6, 0x2a, 0x60, 0x64,
];
const VECTOR_NAMES: [&str; 5] = [
    "normalized_input",
    "gate_projection",
    "up_projection",
    "silu_gate_mul_up",
    "down_projection",
];
const VECTOR_LENGTHS: [usize; 5] = [1024, 4608, 4608, 4608, 1024];
const HEADER_BYTES: usize = 256;
const DESCRIPTOR_BYTES: usize = 48;
const PAYLOAD_OFFSET: usize = 512;
const SEAL_OFFSET: usize = 188;
const F32_PROJECTION_MAX_ABS_BOUND: f64 = 2.0e-6;
const FIXED_Q30_MAX_ABS_BOUND: f64 = 2.0e-6;
const SILU_PRODUCT_MAX_ABS_BOUND: f64 = 2.0e-6;

#[derive(Clone)]
struct Q8Block {
    scale_bits: u16,
    quants: [i8; Q8_0_BLOCK_VALUES],
}

#[derive(Default, Clone, Copy)]
struct ErrorStats {
    max_abs: f64,
    max_rel: f64,
    squared_sum: f64,
    count: usize,
}

impl ErrorStats {
    fn observe(&mut self, actual: f32, expected: f32) {
        let difference = f64::from(actual) - f64::from(expected);
        let absolute = difference.abs();
        self.max_abs = self.max_abs.max(absolute);
        self.max_rel = self
            .max_rel
            .max(absolute / f64::from(expected).abs().max(1.0e-9));
        self.squared_sum += difference * difference;
        self.count += 1;
    }

    fn rmse(self) -> f64 {
        (self.squared_sum / self.count as f64).sqrt()
    }
}

struct ProjectionResult {
    fp32: Vec<f32>,
    q30: Vec<f32>,
    activation: Vec<Q8Block>,
    matrix: Vec<u8>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lfm25-golden: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() == 3 && arguments[1] == "verify" {
        return verify_artifact(Path::new(&arguments[2]));
    }
    if arguments.len() != 6 || arguments[1] != "seal" {
        return Err(format!(
            "usage:\n  {} seal TRACE.raw MODEL.gguf NATIVE.bin GOLDEN.bin\n  {} verify GOLDEN.bin\n\
             simulation vectors are written beside GOLDEN.bin as GOLDEN.bin.vectors",
            arguments
                .first()
                .map(String::as_str)
                .unwrap_or("lfm25-golden"),
            arguments
                .first()
                .map(String::as_str)
                .unwrap_or("lfm25-golden"),
        ));
    }
    let trace_path = Path::new(&arguments[2]);
    let gguf_path = Path::new(&arguments[3]);
    let native_path = Path::new(&arguments[4]);
    let golden_path = Path::new(&arguments[5]);
    let vectors_path = PathBuf::from(format!("{}.vectors", golden_path.display()));

    verify_file(gguf_path, PINNED_GGUF_BYTES as u64, PINNED_GGUF_SHA256, "GGUF")?;
    verify_file(
        native_path,
        PINNED_NATIVE_IMAGE_BYTES as u64,
        PINNED_NATIVE_IMAGE_SHA256,
        "native image",
    )?;
    let vectors = read_trace(trace_path)?;

    let gate = project(native_path, "blk.0.ffn_gate.weight", &vectors[0])?;
    let up = project(native_path, "blk.0.ffn_up.weight", &vectors[0])?;
    let down = project(native_path, "blk.0.ffn_down.weight", &vectors[3])?;

    let gate_fp32 = compare(&gate.fp32, &vectors[1]);
    let up_fp32 = compare(&up.fp32, &vectors[2]);
    let down_fp32 = compare(&down.fp32, &vectors[4]);
    let gate_q30 = compare(&gate.q30, &vectors[1]);
    let up_q30 = compare(&up.q30, &vectors[2]);
    let down_q30 = compare(&down.q30, &vectors[4]);
    let q30_internal = [
        compare(&gate.q30, &gate.fp32),
        compare(&up.q30, &up.fp32),
        compare(&down.q30, &down.fp32),
    ];
    let silu_expected: Vec<f32> = vectors[1]
        .iter()
        .zip(&vectors[2])
        .map(|(&gate_value, &up_value)| (gate_value / (1.0 + (-gate_value).exp())) * up_value)
        .collect();
    let silu = compare(&silu_expected, &vectors[3]);

    for (name, stats) in [
        ("gate/fp32", gate_fp32),
        ("up/fp32", up_fp32),
        ("down/fp32", down_fp32),
    ] {
        require_bound(name, stats, F32_PROJECTION_MAX_ABS_BOUND)?;
    }
    for (name, stats) in [
        ("gate/q30", gate_q30),
        ("up/q30", up_q30),
        ("down/q30", down_q30),
    ] {
        require_bound(name, stats, FIXED_Q30_MAX_ABS_BOUND)?;
    }
    require_bound("silu-product", silu, SILU_PRODUCT_MAX_ABS_BOUND)?;

    let artifact = build_artifact(&vectors)?;
    write_atomic(golden_path, &artifact)?;
    let simulation = build_simulation_vectors(&gate, &up, &down, &vectors)?;
    write_atomic(&vectors_path, simulation.as_bytes())?;

    println!(
        "golden={} bytes={} sha256={}",
        golden_path.display(),
        artifact.len(),
        hex(&sha256(&artifact))
    );
    println!(
        "vectors={} rows=5 blocks={}",
        vectors_path.display(),
        simulation
            .lines()
            .filter(|line| !line.starts_with('#'))
            .count()
    );
    for (name, stats) in [
        ("gate/fp32", gate_fp32),
        ("up/fp32", up_fp32),
        ("down/fp32", down_fp32),
        ("gate/q30", gate_q30),
        ("up/q30", up_q30),
        ("down/q30", down_q30),
        ("silu-product", silu),
    ] {
        print_stats(name, stats);
    }
    println!(
        "q30-vs-fp32 max_abs gate={:.9e} up={:.9e} down={:.9e}",
        q30_internal[0].max_abs, q30_internal[1].max_abs, q30_internal[2].max_abs
    );
    Ok(())
}

fn verify_artifact(path: &Path) -> Result<(), String> {
    let artifact = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let expected_payload_bytes = VECTOR_LENGTHS.iter().sum::<usize>() * 4;
    if artifact.len() != PAYLOAD_OFFSET + expected_payload_bytes
        || artifact.get(0..8) != Some(GOLDEN_MAGIC)
        || artifact_u16(&artifact, 8)? != 1
        || artifact_u16(&artifact, 10)? as usize != HEADER_BYTES
        || artifact_u16(&artifact, 12)? as usize != DESCRIPTOR_BYTES
        || artifact_u16(&artifact, 14)? != 5
        || artifact_u32(&artifact, 16)? != MODEL_GENERATION
        || artifact_u32(&artifact, 20)? != 0
        || artifact_u32(&artifact, 24)? != 1
        || artifact_u32(&artifact, 28)? != 0x0000_0007
        || artifact_u32(&artifact, 32)? as usize != PAYLOAD_OFFSET
        || artifact_u32(&artifact, 36)? as usize != expected_payload_bytes
    {
        return Err("golden artifact header mismatch".into());
    }
    for (offset, expected, label) in [
        (40, LLAMA_COMMIT.as_slice(), "llama commit"),
        (60, PINNED_GGUF_SHA256.as_slice(), "GGUF hash"),
        (92, PINNED_NATIVE_IMAGE_SHA256.as_slice(), "native image hash"),
        (124, MODEL_CONTRACT_SHA256.as_slice(), "model contract hash"),
    ] {
        if artifact.get(offset..offset + expected.len()) != Some(expected) {
            return Err(format!("golden artifact {label} mismatch"));
        }
    }
    let payload_hash = sha256(&artifact[PAYLOAD_OFFSET..]);
    if artifact.get(156..188) != Some(payload_hash.as_slice()) {
        return Err("golden artifact payload hash mismatch".into());
    }
    let expected_seal: [u8; 32] = artifact[SEAL_OFFSET..SEAL_OFFSET + 32].try_into().unwrap();
    let mut seal_view = artifact.clone();
    seal_view[SEAL_OFFSET..SEAL_OFFSET + 32].fill(0);
    if sha256(&seal_view) != expected_seal {
        return Err("golden artifact seal mismatch".into());
    }

    let mut payload_cursor = 0usize;
    for index in 0..5 {
        let descriptor = HEADER_BYTES + index * DESCRIPTOR_BYTES;
        let name_bytes = &artifact[descriptor + 16..descriptor + 48];
        let name_end = name_bytes.iter().position(|byte| *byte == 0).unwrap_or(32);
        if artifact_u16(&artifact, descriptor)? != index as u16
            || artifact_u16(&artifact, descriptor + 2)? != 1
            || artifact_u32(&artifact, descriptor + 4)? as usize != VECTOR_LENGTHS[index]
            || artifact_u32(&artifact, descriptor + 8)? as usize != payload_cursor
            || artifact_u32(&artifact, descriptor + 12)? as usize != VECTOR_LENGTHS[index] * 4
            || &name_bytes[..name_end] != VECTOR_NAMES[index].as_bytes()
        {
            return Err(format!("golden artifact vector descriptor {index} mismatch"));
        }
        payload_cursor += VECTOR_LENGTHS[index] * 4;
    }
    println!(
        "verified={} bytes={} payload_sha256={} seal_sha256={}",
        path.display(),
        artifact.len(),
        hex(&payload_hash),
        hex(&expected_seal)
    );
    Ok(())
}

fn read_trace(path: &Path) -> Result<Vec<Vec<f32>>, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut cursor = 0usize;
    if take(&bytes, &mut cursor, 8)? != RAW_MAGIC {
        return Err("trace magic mismatch".into());
    }
    if read_u32(&bytes, &mut cursor)? != 1 || read_u32(&bytes, &mut cursor)? != 1 {
        return Err("trace version/token mismatch".into());
    }
    if read_u32(&bytes, &mut cursor)? != 5 || read_u32(&bytes, &mut cursor)? != 0 {
        return Err("trace vector header mismatch".into());
    }
    let mut output = Vec::with_capacity(5);
    for index in 0..5 {
        let name_bytes = take(&bytes, &mut cursor, 32)?;
        let name_end = name_bytes.iter().position(|byte| *byte == 0).unwrap_or(32);
        let name =
            std::str::from_utf8(&name_bytes[..name_end]).map_err(|_| "trace name is not UTF-8")?;
        if name != VECTOR_NAMES[index] {
            return Err(format!(
                "trace vector {index} is {name}, expected {}",
                VECTOR_NAMES[index]
            ));
        }
        let elements = read_u32(&bytes, &mut cursor)? as usize;
        if elements != VECTOR_LENGTHS[index] || read_u32(&bytes, &mut cursor)? != 0 {
            return Err(format!("trace vector {name} has invalid descriptor"));
        }
        let mut values = Vec::with_capacity(elements);
        for _ in 0..elements {
            let value = f32::from_bits(read_u32(&bytes, &mut cursor)?);
            if !value.is_finite() {
                return Err(format!("trace vector {name} contains non-finite data"));
            }
            values.push(value);
        }
        output.push(values);
    }
    if cursor != bytes.len() {
        return Err(format!("trace has {} trailing bytes", bytes.len() - cursor));
    }
    Ok(output)
}

fn project(
    native_path: &Path,
    tensor_name: &str,
    input: &[f32],
) -> Result<ProjectionResult, String> {
    let descriptor_index = TENSOR_NAMES
        .iter()
        .position(|name| *name == tensor_name)
        .ok_or_else(|| format!("missing tensor {tensor_name}"))?;
    let descriptor = TENSORS[descriptor_index];
    if descriptor.format != 2 || descriptor.ggml_ne0 as usize != input.len() {
        return Err(format!("tensor contract mismatch for {tensor_name}"));
    }
    let mut matrix = vec![0u8; descriptor.native_bytes as usize];
    let mut native =
        File::open(native_path).map_err(|e| format!("open {}: {e}", native_path.display()))?;
    native
        .seek(SeekFrom::Start(descriptor.native_offset as u64))
        .map_err(|e| format!("seek {tensor_name}: {e}"))?;
    native
        .read_exact(&mut matrix)
        .map_err(|e| format!("read {tensor_name}: {e}"))?;

    let activation = quantize_q8_0(input);
    let blocks_per_row = input.len() / Q8_0_BLOCK_VALUES;
    let row_bytes = blocks_per_row * Q8_0_BLOCK_BYTES;
    if row_bytes * descriptor.ggml_ne1 as usize != matrix.len() {
        return Err(format!("row layout mismatch for {tensor_name}"));
    }
    let mut fp32 = Vec::with_capacity(descriptor.ggml_ne1 as usize);
    let mut q30 = Vec::with_capacity(descriptor.ggml_ne1 as usize);
    for row in 0..descriptor.ggml_ne1 as usize {
        let mut fp32_sum = 0.0f32;
        let mut q30_sum = 0i64;
        for block in 0..blocks_per_row {
            let weight = decode_block(&matrix[row * row_bytes + block * Q8_0_BLOCK_BYTES..])?;
            let dot = integer_dot(&activation[block].quants, &weight.quants);
            let scale = f16::from_bits(activation[block].scale_bits).to_f32()
                * f16::from_bits(weight.scale_bits).to_f32();
            fp32_sum += dot as f32 * scale;
            q30_sum = q30_sum
                .checked_add(q30_term(dot, activation[block].scale_bits, weight.scale_bits)?)
                .ok_or_else(|| format!("Q30 accumulator overflow in {tensor_name}"))?;
        }
        fp32.push(fp32_sum);
        q30.push(q30_sum as f32 / ((1u64 << 30) as f32));
    }
    Ok(ProjectionResult {
        fp32,
        q30,
        activation,
        matrix,
    })
}

fn quantize_q8_0(input: &[f32]) -> Vec<Q8Block> {
    assert_eq!(input.len() % Q8_0_BLOCK_VALUES, 0);
    let (blocks, remainder) = input.as_chunks::<Q8_0_BLOCK_VALUES>();
    assert!(remainder.is_empty());
    blocks
        .iter()
        .map(|values| {
            let maximum = values
                .iter()
                .fold(0.0f32, |current, value| current.max(value.abs()));
            let scale = maximum / 127.0;
            let inverse = if maximum == 0.0 { 0.0 } else { 127.0 / maximum };
            let mut quants = [0i8; Q8_0_BLOCK_VALUES];
            for (quant, value) in quants.iter_mut().zip(values) {
                *quant = (*value * inverse).round_ties_even() as i8;
            }
            Q8Block {
                scale_bits: f16::from_f32(scale).to_bits(),
                quants,
            }
        })
        .collect()
}

fn decode_block(bytes: &[u8]) -> Result<Q8Block, String> {
    if bytes.len() < Q8_0_BLOCK_BYTES {
        return Err("truncated Q8_0 block".into());
    }
    let scale_bits = u16::from_le_bytes([bytes[0], bytes[1]]);
    let mut quants = [0i8; Q8_0_BLOCK_VALUES];
    for (destination, source) in quants.iter_mut().zip(&bytes[2..34]) {
        *destination = *source as i8;
    }
    Ok(Q8Block { scale_bits, quants })
}

fn integer_dot(left: &[i8; 32], right: &[i8; 32]) -> i32 {
    left.iter()
        .zip(right)
        .map(|(&a, &b)| i32::from(a) * i32::from(b))
        .sum()
}

fn q30_term(dot: i32, activation_scale: u16, weight_scale: u16) -> Result<i64, String> {
    let (activation_significand, activation_exponent) = half_parts(activation_scale)?;
    let (weight_significand, weight_exponent) = half_parts(weight_scale)?;
    if activation_significand == 0 || weight_significand == 0 || dot == 0 {
        return Ok(0);
    }
    let raw = i64::from(dot)
        .checked_mul(i64::from(activation_significand))
        .and_then(|value| value.checked_mul(i64::from(weight_significand)))
        .ok_or_else(|| "Q30 scale product overflow".to_string())?;
    let shift = activation_exponent + weight_exponent - 20;
    if shift >= 0 {
        raw.checked_shl(shift as u32)
            .ok_or_else(|| "Q30 left shift overflow".into())
    } else {
        Ok(round_shift_right_even(raw, (-shift) as u32))
    }
}

fn half_parts(bits: u16) -> Result<(u16, i32), String> {
    if bits & 0x8000 != 0 {
        return Err(format!("negative Q8_0 scale 0x{bits:04x}"));
    }
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let fraction = bits & 0x03ff;
    match exponent {
        0 => Ok((fraction, 1)),
        31 => Err(format!("non-finite Q8_0 scale 0x{bits:04x}")),
        _ => Ok((1024 + fraction, exponent)),
    }
}

fn round_shift_right_even(value: i64, shift: u32) -> i64 {
    let negative = value < 0;
    let magnitude = value.unsigned_abs();
    if shift >= 64 {
        return 0;
    }
    let quotient = magnitude >> shift;
    let mask = if shift == 0 { 0 } else { (1u64 << shift) - 1 };
    let remainder = magnitude & mask;
    let halfway = if shift == 0 { 0 } else { 1u64 << (shift - 1) };
    let rounded =
        quotient + u64::from(remainder > halfway || (remainder == halfway && quotient & 1 != 0));
    if negative {
        -(rounded as i64)
    } else {
        rounded as i64
    }
}

fn compare(actual: &[f32], expected: &[f32]) -> ErrorStats {
    assert_eq!(actual.len(), expected.len());
    let mut stats = ErrorStats::default();
    for (&actual, &expected) in actual.iter().zip(expected) {
        stats.observe(actual, expected);
    }
    stats
}

fn require_bound(name: &str, stats: ErrorStats, bound: f64) -> Result<(), String> {
    if stats.max_abs > bound {
        Err(format!("{name} max_abs {:.9e} exceeds {:.9e}", stats.max_abs, bound))
    } else {
        Ok(())
    }
}

fn print_stats(name: &str, stats: ErrorStats) {
    println!(
        "verify {name:16} max_abs={:.9e} rmse={:.9e} max_rel={:.9e}",
        stats.max_abs,
        stats.rmse(),
        stats.max_rel
    );
}

fn build_artifact(vectors: &[Vec<f32>]) -> Result<Vec<u8>, String> {
    let payload_bytes = vectors.iter().map(|vector| vector.len() * 4).sum::<usize>();
    let mut artifact = vec![0u8; PAYLOAD_OFFSET];
    put(&mut artifact, 0, GOLDEN_MAGIC);
    put_u16(&mut artifact, 8, 1);
    put_u16(&mut artifact, 10, HEADER_BYTES as u16);
    put_u16(&mut artifact, 12, DESCRIPTOR_BYTES as u16);
    put_u16(&mut artifact, 14, vectors.len() as u16);
    put_u32(&mut artifact, 16, MODEL_GENERATION);
    put_u32(&mut artifact, 20, 0);
    put_u32(&mut artifact, 24, 1);
    put_u32(&mut artifact, 28, 0x0000_0007);
    put_u32(&mut artifact, 32, PAYLOAD_OFFSET as u32);
    put_u32(&mut artifact, 36, payload_bytes as u32);
    put(&mut artifact, 40, &LLAMA_COMMIT);
    put(&mut artifact, 60, &PINNED_GGUF_SHA256);
    put(&mut artifact, 92, &PINNED_NATIVE_IMAGE_SHA256);
    put(&mut artifact, 124, &MODEL_CONTRACT_SHA256);

    let mut payload_cursor = 0usize;
    for (index, vector) in vectors.iter().enumerate() {
        let descriptor = HEADER_BYTES + index * DESCRIPTOR_BYTES;
        put_u16(&mut artifact, descriptor, index as u16);
        put_u16(&mut artifact, descriptor + 2, 1);
        put_u32(&mut artifact, descriptor + 4, vector.len() as u32);
        put_u32(&mut artifact, descriptor + 8, payload_cursor as u32);
        put_u32(&mut artifact, descriptor + 12, (vector.len() * 4) as u32);
        put(&mut artifact, descriptor + 16, VECTOR_NAMES[index].as_bytes());
        for value in vector {
            artifact.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        payload_cursor += vector.len() * 4;
    }
    if artifact.len() != PAYLOAD_OFFSET + payload_bytes {
        return Err("golden artifact size accounting failed".into());
    }
    let payload_hash = sha256(&artifact[PAYLOAD_OFFSET..]);
    put(&mut artifact, 156, &payload_hash);
    let mut seal_view = artifact.clone();
    seal_view[SEAL_OFFSET..SEAL_OFFSET + 32].fill(0);
    let seal = sha256(&seal_view);
    put(&mut artifact, SEAL_OFFSET, &seal);

    let mut verify_view = artifact.clone();
    verify_view[SEAL_OFFSET..SEAL_OFFSET + 32].fill(0);
    if sha256(&verify_view) != artifact[SEAL_OFFSET..SEAL_OFFSET + 32] {
        return Err("golden artifact seal verification failed".into());
    }
    Ok(artifact)
}

fn build_simulation_vectors(
    gate: &ProjectionResult,
    up: &ProjectionResult,
    down: &ProjectionResult,
    golden: &[Vec<f32>],
) -> Result<String, String> {
    let mut output = String::from(
        "# TRUEGA Q8_0 native-block vectors v1\n\
         # row block first last a_scale w_scale a_quants w_quants expected_dot expected_term_q30_hex expected_fp_q30_hex fp_bound_q30\n",
    );
    let fp_bound_q30 = (FIXED_Q30_MAX_ABS_BOUND * ((1u64 << 30) as f64)).ceil() as i64;
    append_row(&mut output, 0, gate, f32_to_q30(golden[1][0]), fp_bound_q30)?;
    append_row(&mut output, 1, up, f32_to_q30(golden[2][0]), fp_bound_q30)?;
    append_row(&mut output, 2, down, f32_to_q30(golden[4][0]), fp_bound_q30)?;

    let ones = Q8Block {
        scale_bits: 0x3c00,
        quants: [127; 32],
    };
    let negatives = Q8Block {
        scale_bits: 0x3c00,
        quants: [-128; 32],
    };
    let edge_dot = integer_dot(&ones.quants, &negatives.quants);
    let edge_term = q30_term(edge_dot, ones.scale_bits, negatives.scale_bits)?;
    append_block(&mut output, 3, 0, true, true, &ones, &negatives, edge_term, 0)?;
    let mut alternating_a = [0i8; 32];
    let mut alternating_b = [0i8; 32];
    for lane in 0..32 {
        alternating_a[lane] = if lane & 1 == 0 { 127 } else { -128 };
        alternating_b[lane] = if lane & 2 == 0 { -128 } else { 127 };
    }
    let alternating_activation = Q8Block {
        scale_bits: 0x3555,
        quants: alternating_a,
    };
    let alternating_weight = Q8Block {
        scale_bits: 0x2e66,
        quants: alternating_b,
    };
    let alternating_dot = integer_dot(&alternating_activation.quants, &alternating_weight.quants);
    let alternating_term = q30_term(
        alternating_dot,
        alternating_activation.scale_bits,
        alternating_weight.scale_bits,
    )?;
    append_block(
        &mut output,
        4,
        0,
        true,
        true,
        &alternating_activation,
        &alternating_weight,
        alternating_term,
        0,
    )?;
    Ok(output)
}

fn append_row(
    output: &mut String,
    row: u32,
    result: &ProjectionResult,
    expected_fp_q30: i64,
    fp_bound_q30: i64,
) -> Result<(), String> {
    let blocks = result.activation.len();
    for block in 0..blocks {
        let weight = decode_block(&result.matrix[block * Q8_0_BLOCK_BYTES..])?;
        append_block(
            output,
            row,
            block,
            block == 0,
            block + 1 == blocks,
            &result.activation[block],
            &weight,
            expected_fp_q30,
            fp_bound_q30,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_block(
    output: &mut String,
    row: u32,
    block: usize,
    first: bool,
    last: bool,
    activation: &Q8Block,
    weight: &Q8Block,
    expected_fp_q30: i64,
    fp_bound_q30: i64,
) -> Result<(), String> {
    let dot = integer_dot(&activation.quants, &weight.quants);
    let term = q30_term(dot, activation.scale_bits, weight.scale_bits)?;
    output.push_str(&format!(
        "{row} {block} {} {} {:04x} {:04x} {} {} {dot} {:016x} {:016x} {fp_bound_q30}\n",
        u8::from(first),
        u8::from(last),
        activation.scale_bits,
        weight.scale_bits,
        packed_quants(&activation.quants),
        packed_quants(&weight.quants),
        term as u64,
        expected_fp_q30 as u64,
    ));
    Ok(())
}

fn f32_to_q30(value: f32) -> i64 {
    (f64::from(value) * ((1u64 << 30) as f64)).round_ties_even() as i64
}

fn packed_quants(quants: &[i8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for value in quants.iter().rev() {
        output.push_str(&format!("{:02x}", *value as u8));
    }
    output
}

fn verify_file(
    path: &Path,
    expected_bytes: u64,
    expected_hash: [u8; 32],
    label: &str,
) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    if metadata.len() != expected_bytes {
        return Err(format!("{label} size {} != {expected_bytes}", metadata.len()));
    }
    let actual = sha256_file(path)?;
    if actual != expected_hash {
        return Err(format!("{label} SHA-256 {} != {}", hex(&actual), hex(&expected_hash)));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<[u8; 32], String> {
    let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().into())
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    if fs::read(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let stage = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid output name")?,
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)
        .map_err(|e| format!("create {}: {e}", stage.display()))?;
    file.write_all(contents)
        .map_err(|e| format!("write {}: {e}", stage.display()))?;
    file.sync_all()
        .map_err(|e| format!("sync {}: {e}", stage.display()))?;
    fs::rename(&stage, path).map_err(|e| format!("publish {}: {e}", path.display()))
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, count: usize) -> Result<&'a [u8], String> {
    let end = cursor.checked_add(count).ok_or("trace offset overflow")?;
    let value = bytes.get(*cursor..end).ok_or("truncated trace")?;
    *cursor = end;
    Ok(value)
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(take(bytes, cursor, 4)?.try_into().unwrap()))
}

fn artifact_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or("truncated golden artifact")?;
    Ok(u16::from_le_bytes(value.try_into().unwrap()))
}

fn artifact_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or("truncated golden artifact")?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    put(bytes, offset, &value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    put(bytes, offset, &value.to_le_bytes());
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_integer_dot_extremes() {
        assert_eq!(integer_dot(&[127; 32], &[-128; 32]), -520_192);
        assert_eq!(integer_dot(&[-128; 32], &[-128; 32]), 524_288);
    }

    #[test]
    fn half_scale_to_q30_is_exact() {
        assert_eq!(q30_term(17, 0x3c00, 0x3c00).unwrap(), 17i64 << 30);
        assert_eq!(q30_term(-3, 0x3800, 0x3400).unwrap(), -402_653_184);
    }
}
