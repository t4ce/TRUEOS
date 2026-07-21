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
const BLOCK_GOLDEN_MAGIC: &[u8; 8] = b"TGAQ8B01";
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
const BLOCK_HEADER_BYTES: usize = 256;
const BLOCK_INPUT_BYTES: usize = 2 * Q8_0_BLOCK_BYTES;
const BLOCK_OUTPUT_BYTES: usize = 12;
const BLOCK_PAYLOAD_BYTES: usize = BLOCK_INPUT_BYTES + BLOCK_OUTPUT_BYTES;
const BLOCK_SEAL_OFFSET: usize = 192;
const BLOCK_INPUT_OFFSET: usize = BLOCK_HEADER_BYTES;
const BLOCK_OUTPUT_OFFSET: usize = BLOCK_INPUT_OFFSET + BLOCK_INPUT_BYTES;
const BLOCK_ARTIFACT_BYTES: usize = BLOCK_HEADER_BYTES + BLOCK_PAYLOAD_BYTES;
const BLOCK_FLAG_NATIVE_VERIFIED: u16 = 1 << 0;
const CANONICAL_TENSOR_NAME: &str = "blk.0.ffn_gate.weight";
const CANONICAL_VECTOR_INDEX: usize = 0;
const CANONICAL_ROW: u32 = 0;
const CANONICAL_BLOCK: u32 = 0;
const PINNED_FULL_GOLDEN_SHA256: [u8; 32] = [
    0xeb, 0x12, 0x4c, 0x33, 0x3e, 0x7a, 0x70, 0x95, 0xa7, 0x8f, 0xc6, 0xc0, 0x00, 0x4f, 0x90, 0xa4,
    0x3f, 0xa8, 0x25, 0xbd, 0xfd, 0x1a, 0x8f, 0x74, 0xac, 0x9d, 0x67, 0xc5, 0x38, 0x48, 0x41, 0x85,
];
const PINNED_VECTORS_SHA256: [u8; 32] = [
    0x24, 0x7a, 0xf3, 0xe0, 0x6a, 0x7c, 0xb4, 0x0d, 0xdf, 0x87, 0x1d, 0x24, 0x43, 0x02, 0x38, 0x83,
    0x44, 0x1c, 0xfc, 0x82, 0x24, 0x7c, 0x4d, 0xb7, 0xe3, 0xbd, 0x55, 0xe0, 0x91, 0x04, 0x8e, 0xfe,
];
const F32_PROJECTION_MAX_ABS_BOUND: f64 = 2.0e-6;
const FIXED_Q30_MAX_ABS_BOUND: f64 = 2.0e-6;
const SILU_PRODUCT_MAX_ABS_BOUND: f64 = 2.0e-6;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Q8Block {
    scale_bits: u16,
    quants: [i8; Q8_0_BLOCK_VALUES],
}

#[derive(Clone)]
struct CanonicalBlock {
    activation: Q8Block,
    weight: Q8Block,
    dot: i32,
    term_q30: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockCoordinates {
    tensor_id: u16,
    layer: u8,
    role: u8,
    row: u32,
    block: u32,
    native_block_offset: u32,
}

#[derive(Debug)]
struct VerifiedBlockArtifact {
    full_golden_sha256: [u8; 32],
    vectors_sha256: [u8; 32],
    coordinates: BlockCoordinates,
    input: [u8; BLOCK_INPUT_BYTES],
    output: [u8; BLOCK_OUTPUT_BYTES],
    payload_sha256: [u8; 32],
    seal_sha256: [u8; 32],
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
    q30_raw: Vec<i64>,
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
    if arguments.len() == 4 && arguments[1] == "pipeline" {
        return verify_fixed_pipeline(Path::new(&arguments[2]), Path::new(&arguments[3]));
    }
    if arguments.len() == 6 && arguments[1] == "block" {
        return seal_block_artifact(
            Path::new(&arguments[2]),
            Path::new(&arguments[3]),
            Path::new(&arguments[4]),
            Path::new(&arguments[5]),
        );
    }
    if arguments.len() == 5 && arguments[1] == "verify-block" {
        return verify_block_artifact(
            Path::new(&arguments[2]),
            Path::new(&arguments[3]),
            Path::new(&arguments[4]),
        );
    }
    if arguments.len() != 6 || arguments[1] != "seal" {
        let program = arguments
            .first()
            .map(String::as_str)
            .unwrap_or("lfm25-golden");
        return Err(format!(
            "usage:\n  {program} seal TRACE.raw MODEL.gguf NATIVE.bin GOLDEN.bin\n  \
             {program} verify GOLDEN.bin\n  \
             {program} pipeline GOLDEN.bin NATIVE.bin\n  \
             {program} block GOLDEN.bin GOLDEN.bin.vectors NATIVE.bin BLOCK.bin\n  \
             {program} verify-block BLOCK.bin GOLDEN.bin GOLDEN.bin.vectors\n\
             simulation vectors are written beside GOLDEN.bin as GOLDEN.bin.vectors",
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
    let (payload_hash, expected_seal) = validate_artifact(&artifact)?;
    println!(
        "verified={} bytes={} payload_sha256={} seal_sha256={}",
        path.display(),
        artifact.len(),
        hex(&payload_hash),
        hex(&expected_seal)
    );
    Ok(())
}

fn verify_fixed_pipeline(golden_path: &Path, native_path: &Path) -> Result<(), String> {
    let artifact =
        fs::read(golden_path).map_err(|e| format!("read {}: {e}", golden_path.display()))?;
    validate_artifact(&artifact)?;
    verify_file(
        native_path,
        PINNED_NATIVE_IMAGE_BYTES as u64,
        PINNED_NATIVE_IMAGE_SHA256,
        "native image",
    )?;
    let vectors = artifact_vectors(&artifact)?;
    let gate = project(native_path, "blk.0.ffn_gate.weight", &vectors[0])?;
    let up = project(native_path, "blk.0.ffn_up.weight", &vectors[0])?;
    let silu_q30_raw: Vec<i64> = gate
        .q30_raw
        .iter()
        .zip(&up.q30_raw)
        .map(|(&gate, &up)| fixed_silu_mul_q30(gate, up))
        .collect();
    let silu_q30: Vec<f32> = silu_q30_raw
        .iter()
        .map(|&value| value as f32 / ((1u64 << 30) as f32))
        .collect();
    let down = project(native_path, "blk.0.ffn_down.weight", &silu_q30)?;

    let gate_stats = compare(&gate.q30, &vectors[1]);
    let up_stats = compare(&up.q30, &vectors[2]);
    let silu_stats = compare(&silu_q30, &vectors[3]);
    let down_stats = compare(&down.q30, &vectors[4]);
    for (name, stats) in [
        ("pipeline-gate", gate_stats),
        ("pipeline-up", up_stats),
        ("pipeline-silu", silu_stats),
        ("pipeline-down", down_stats),
    ] {
        print_stats(name, stats);
    }
    println!(
        "fixed-pipeline gate={} up={} silu={} down={} silu_max_abs={:.9e} down_max_abs={:.9e}",
        gate.q30.len(),
        up.q30.len(),
        silu_q30.len(),
        down.q30.len(),
        silu_stats.max_abs,
        down_stats.max_abs,
    );
    println!(
        "fixed-pipeline sample0 gate_q30={} up_q30={} silu_q30={} down_q30={}",
        gate.q30_raw[0], up.q30_raw[0], silu_q30_raw[0], down.q30_raw[0],
    );
    Ok(())
}

fn artifact_vectors(artifact: &[u8]) -> Result<Vec<Vec<f32>>, String> {
    let mut vectors = Vec::with_capacity(VECTOR_LENGTHS.len());
    let mut payload_offset = PAYLOAD_OFFSET;
    for (&length, &name) in VECTOR_LENGTHS.iter().zip(&VECTOR_NAMES) {
        let end = payload_offset
            .checked_add(length * 4)
            .ok_or("golden vector offset overflow")?;
        let bytes = artifact
            .get(payload_offset..end)
            .ok_or_else(|| format!("golden vector {name} is truncated"))?;
        let mut vector = Vec::with_capacity(length);
        for word in bytes.chunks_exact(4) {
            vector.push(f32::from_bits(u32::from_le_bytes(word.try_into().unwrap())));
        }
        vectors.push(vector);
        payload_offset = end;
    }
    Ok(vectors)
}

fn fixed_silu_mul_q30(gate: i64, up: i64) -> i64 {
    const ONE_Q30: i64 = 1i64 << 30;
    const HALF_Q30: i64 = ONE_Q30 / 2;
    const C1_Q30: i64 = 268_435_456;
    const C3_Q30: i64 = -22_369_621;
    const C5_Q30: i64 = 2_236_962;
    const C7_Q30: i64 = -226_359;
    const C9_Q30: i64 = 22_931;

    // The captured layer-0 gate stays inside +/-1.01.  Keep one bit of guard
    // range so the hardware does not introduce a discontinuity at exactly 1.0.
    let x = gate.clamp(-2 * ONE_Q30, 2 * ONE_Q30);
    let x2 = mul_q30_round_even(x, x);
    let mut polynomial = C9_Q30;
    polynomial = C7_Q30 + mul_q30_round_even(x2, polynomial);
    polynomial = C5_Q30 + mul_q30_round_even(x2, polynomial);
    polynomial = C3_Q30 + mul_q30_round_even(x2, polynomial);
    polynomial = C1_Q30 + mul_q30_round_even(x2, polynomial);
    let sigmoid = HALF_Q30 + mul_q30_round_even(x, polynomial);
    let silu = mul_q30_round_even(gate, sigmoid);
    mul_q30_round_even(silu, up)
}

fn mul_q30_round_even(left: i64, right: i64) -> i64 {
    let product = i128::from(left) * i128::from(right);
    let negative = product < 0;
    let magnitude = product.unsigned_abs();
    let quotient = magnitude >> 30;
    let remainder = magnitude & ((1u128 << 30) - 1);
    let halfway = 1u128 << 29;
    let rounded =
        quotient + u128::from(remainder > halfway || (remainder == halfway && quotient & 1 != 0));
    if negative {
        -(rounded as i64)
    } else {
        rounded as i64
    }
}

fn validate_artifact(artifact: &[u8]) -> Result<([u8; 32], [u8; 32]), String> {
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
    let mut seal_view = artifact.to_vec();
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
    Ok((payload_hash, expected_seal))
}

fn seal_block_artifact(
    full_golden_path: &Path,
    vectors_path: &Path,
    native_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let full_golden = fs::read(full_golden_path)
        .map_err(|e| format!("read {}: {e}", full_golden_path.display()))?;
    validate_artifact(&full_golden)?;
    let vectors =
        fs::read(vectors_path).map_err(|e| format!("read {}: {e}", vectors_path.display()))?;
    let canonical = derive_canonical_block(&full_golden, &vectors)?;
    let coordinates = canonical_coordinates()?;

    verify_file(
        native_path,
        PINNED_NATIVE_IMAGE_BYTES as u64,
        PINNED_NATIVE_IMAGE_SHA256,
        "native image",
    )?;
    let native_weight = read_native_q8_block(native_path, coordinates.native_block_offset)?;
    if native_weight != canonical.weight {
        return Err("canonical vector weight does not match the sealed native image".into());
    }

    let input = encode_block_input(&canonical);
    let output = encode_block_output(&canonical);
    let artifact =
        build_block_artifact(sha256(&full_golden), sha256(&vectors), coordinates, input, output)?;
    write_atomic(output_path, &artifact)?;
    let checksum = sha256(&artifact);
    let checksum_path = PathBuf::from(format!("{}.sha256", output_path.display()));
    let output_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid block artifact output name")?;
    let checksum_line = format!("{}  {output_name}\n", hex(&checksum));
    write_atomic(&checksum_path, checksum_line.as_bytes())?;

    println!(
        "block={} bytes={} sha256={} input_bytes={} output_bytes={} tensor={} layer={} row={} block_index={} native_verified=true",
        output_path.display(),
        artifact.len(),
        hex(&checksum),
        BLOCK_INPUT_BYTES,
        BLOCK_OUTPUT_BYTES,
        coordinates.tensor_id,
        coordinates.layer,
        coordinates.row,
        coordinates.block,
    );
    println!("checksum={}", checksum_path.display());
    Ok(())
}

fn verify_block_artifact(
    block_path: &Path,
    full_golden_path: &Path,
    vectors_path: &Path,
) -> Result<(), String> {
    let full_golden = fs::read(full_golden_path)
        .map_err(|e| format!("read {}: {e}", full_golden_path.display()))?;
    validate_artifact(&full_golden)?;
    let vectors =
        fs::read(vectors_path).map_err(|e| format!("read {}: {e}", vectors_path.display()))?;
    let canonical = derive_canonical_block(&full_golden, &vectors)?;
    let expected_coordinates = canonical_coordinates()?;
    let artifact =
        fs::read(block_path).map_err(|e| format!("read {}: {e}", block_path.display()))?;
    let verified = validate_block_artifact(&artifact)?;

    if verified.full_golden_sha256 != sha256(&full_golden) {
        return Err("block artifact full-FFN golden hash mismatch".into());
    }
    if verified.vectors_sha256 != sha256(&vectors) {
        return Err("block artifact simulation-vector hash mismatch".into());
    }
    if verified.coordinates != expected_coordinates {
        return Err("block artifact tensor coordinates mismatch".into());
    }
    if verified.input != encode_block_input(&canonical) {
        return Err("block artifact input payload mismatch".into());
    }
    if verified.output != encode_block_output(&canonical) {
        return Err("block artifact output payload mismatch".into());
    }

    println!(
        "verified-block={} bytes={} artifact_sha256={} payload_sha256={} seal_sha256={} dot={} term_q30={}",
        block_path.display(),
        artifact.len(),
        hex(&sha256(&artifact)),
        hex(&verified.payload_sha256),
        hex(&verified.seal_sha256),
        canonical.dot,
        canonical.term_q30,
    );
    Ok(())
}

fn canonical_coordinates() -> Result<BlockCoordinates, String> {
    let descriptor_index = TENSOR_NAMES
        .iter()
        .position(|name| *name == CANONICAL_TENSOR_NAME)
        .ok_or("canonical gate tensor is absent from the model contract")?;
    let descriptor = TENSORS[descriptor_index];
    let blocks_per_row = descriptor.ggml_ne0 as usize / Q8_0_BLOCK_VALUES;
    if descriptor.format != 2
        || descriptor.layer != 0
        || descriptor.ggml_ne0 as usize % Q8_0_BLOCK_VALUES != 0
        || CANONICAL_ROW >= descriptor.ggml_ne1
        || CANONICAL_BLOCK as usize >= blocks_per_row
    {
        return Err("canonical gate tensor shape/format mismatch".into());
    }
    let linear_block = (CANONICAL_ROW as usize)
        .checked_mul(blocks_per_row)
        .and_then(|value| value.checked_add(CANONICAL_BLOCK as usize))
        .ok_or("canonical native block index overflow")?;
    let byte_offset = (descriptor.native_offset as usize)
        .checked_add(
            linear_block
                .checked_mul(Q8_0_BLOCK_BYTES)
                .ok_or("canonical native block byte offset overflow")?,
        )
        .ok_or("canonical native block byte offset overflow")?;
    let native_block_offset = u32::try_from(byte_offset)
        .map_err(|_| "canonical native block offset exceeds u32".to_string())?;
    Ok(BlockCoordinates {
        tensor_id: descriptor.tensor_id,
        layer: descriptor.layer,
        role: descriptor.role,
        row: CANONICAL_ROW,
        block: CANONICAL_BLOCK,
        native_block_offset,
    })
}

fn derive_canonical_block(full_golden: &[u8], vectors: &[u8]) -> Result<CanonicalBlock, String> {
    validate_artifact(full_golden)?;
    if sha256(full_golden) != PINNED_FULL_GOLDEN_SHA256 {
        return Err("canonical block source is not the pinned full-FFN golden".into());
    }
    if sha256(vectors) != PINNED_VECTORS_SHA256 {
        return Err("canonical block source is not the pinned simulation-vector file".into());
    }
    let normalized = golden_vector(full_golden, CANONICAL_VECTOR_INDEX)?;
    let activation = quantize_q8_0(&normalized)
        .into_iter()
        .next()
        .ok_or("normalized input did not produce a Q8_0 block")?;
    let vector = parse_canonical_vector(vectors)?;
    if vector.activation != activation {
        return Err("canonical vector activation does not derive from the sealed FFN input".into());
    }
    let dot = integer_dot(&activation.quants, &vector.weight.quants);
    if vector.dot != dot {
        return Err(format!("canonical vector integer dot {} != recomputed {dot}", vector.dot));
    }
    let term_q30 = q30_term(dot, activation.scale_bits, vector.weight.scale_bits)?;
    if vector.term_q30 != term_q30 {
        return Err(format!(
            "canonical vector Q30 term {} != recomputed {term_q30}",
            vector.term_q30
        ));
    }
    Ok(CanonicalBlock {
        activation,
        weight: vector.weight,
        dot,
        term_q30,
    })
}

fn golden_vector(artifact: &[u8], index: usize) -> Result<Vec<f32>, String> {
    if index >= VECTOR_LENGTHS.len() {
        return Err(format!("golden vector index {index} is out of range"));
    }
    let descriptor = HEADER_BYTES + index * DESCRIPTOR_BYTES;
    let elements = artifact_u32(artifact, descriptor + 4)? as usize;
    let relative_offset = artifact_u32(artifact, descriptor + 8)? as usize;
    let byte_count = artifact_u32(artifact, descriptor + 12)? as usize;
    if elements != VECTOR_LENGTHS[index] || byte_count != elements * 4 {
        return Err(format!("golden vector descriptor {index} has invalid dimensions"));
    }
    let start = PAYLOAD_OFFSET
        .checked_add(relative_offset)
        .ok_or("golden vector offset overflow")?;
    let payload = artifact
        .get(start..start + byte_count)
        .ok_or("truncated golden vector payload")?;
    let mut values = Vec::with_capacity(elements);
    for bytes in payload.chunks_exact(4) {
        let value = f32::from_bits(u32::from_le_bytes(bytes.try_into().unwrap()));
        if !value.is_finite() {
            return Err(format!("golden vector {index} contains non-finite data"));
        }
        values.push(value);
    }
    Ok(values)
}

fn parse_canonical_vector(bytes: &[u8]) -> Result<CanonicalBlock, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "simulation vectors are not UTF-8")?;
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 12 {
            return Err(format!("simulation vector has {} fields, expected 12", fields.len()));
        }
        let row = parse_u32_decimal(fields[0], "row")?;
        let block = parse_u32_decimal(fields[1], "block")?;
        if row != CANONICAL_ROW || block != CANONICAL_BLOCK {
            continue;
        }
        if found.is_some() {
            return Err("simulation vectors contain duplicate canonical block".into());
        }
        if fields[2] != "1" || fields[3] != "0" {
            return Err("canonical vector first/last markers mismatch".into());
        }
        let activation = parse_q8_block(fields[4], fields[6], "activation")?;
        let weight = parse_q8_block(fields[5], fields[7], "weight")?;
        let dot = fields[8]
            .parse::<i32>()
            .map_err(|_| "canonical vector dot is not i32".to_string())?;
        let term_q30 = i64::from_le_bytes(
            u64::from_str_radix(fields[9], 16)
                .map_err(|_| "canonical vector Q30 term is not 64-bit hex".to_string())?
                .to_le_bytes(),
        );
        u64::from_str_radix(fields[10], 16)
            .map_err(|_| "canonical vector row reference is not 64-bit hex".to_string())?;
        fields[11]
            .parse::<i64>()
            .map_err(|_| "canonical vector bound is not i64".to_string())?;
        found = Some(CanonicalBlock {
            activation,
            weight,
            dot,
            term_q30,
        });
    }
    found.ok_or_else(|| "simulation vectors lack canonical gate row 0 block 0".into())
}

fn parse_u32_decimal(value: &str, label: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("simulation vector {label} is not u32"))
}

fn parse_q8_block(scale: &str, packed_quants: &str, label: &str) -> Result<Q8Block, String> {
    let scale_bits = u16::from_str_radix(scale, 16)
        .map_err(|_| format!("canonical {label} scale is not u16 hex"))?;
    if packed_quants.len() != Q8_0_BLOCK_VALUES * 2 {
        return Err(format!("canonical {label} quant payload is not 32 bytes"));
    }
    let mut packed = Vec::with_capacity(Q8_0_BLOCK_VALUES);
    for offset in (0..packed_quants.len()).step_by(2) {
        packed.push(
            u8::from_str_radix(&packed_quants[offset..offset + 2], 16)
                .map_err(|_| format!("canonical {label} quants are not hex"))?,
        );
    }
    packed.reverse();
    let mut quants = [0i8; Q8_0_BLOCK_VALUES];
    for (destination, source) in quants.iter_mut().zip(packed) {
        *destination = source as i8;
    }
    Ok(Q8Block { scale_bits, quants })
}

fn read_native_q8_block(path: &Path, offset: u32) -> Result<Q8Block, String> {
    let mut bytes = [0u8; Q8_0_BLOCK_BYTES];
    let mut native = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    native
        .seek(SeekFrom::Start(offset as u64))
        .map_err(|e| format!("seek canonical native block: {e}"))?;
    native
        .read_exact(&mut bytes)
        .map_err(|e| format!("read canonical native block: {e}"))?;
    decode_block(&bytes)
}

fn encode_q8_block(block: &Q8Block) -> [u8; Q8_0_BLOCK_BYTES] {
    let mut output = [0u8; Q8_0_BLOCK_BYTES];
    output[..2].copy_from_slice(&block.scale_bits.to_le_bytes());
    for (destination, quant) in output[2..].iter_mut().zip(block.quants) {
        *destination = quant as u8;
    }
    output
}

fn encode_block_input(block: &CanonicalBlock) -> [u8; BLOCK_INPUT_BYTES] {
    let mut output = [0u8; BLOCK_INPUT_BYTES];
    output[..Q8_0_BLOCK_BYTES].copy_from_slice(&encode_q8_block(&block.activation));
    output[Q8_0_BLOCK_BYTES..].copy_from_slice(&encode_q8_block(&block.weight));
    output
}

fn encode_block_output(block: &CanonicalBlock) -> [u8; BLOCK_OUTPUT_BYTES] {
    let mut output = [0u8; BLOCK_OUTPUT_BYTES];
    output[..4].copy_from_slice(&block.dot.to_le_bytes());
    output[4..].copy_from_slice(&block.term_q30.to_le_bytes());
    output
}

fn build_block_artifact(
    full_golden_sha256: [u8; 32],
    vectors_sha256: [u8; 32],
    coordinates: BlockCoordinates,
    input: [u8; BLOCK_INPUT_BYTES],
    output: [u8; BLOCK_OUTPUT_BYTES],
) -> Result<Vec<u8>, String> {
    let mut artifact = vec![0u8; BLOCK_ARTIFACT_BYTES];
    put(&mut artifact, 0, BLOCK_GOLDEN_MAGIC);
    put_u16(&mut artifact, 8, 1);
    put_u16(&mut artifact, 10, BLOCK_HEADER_BYTES as u16);
    put_u16(&mut artifact, 12, BLOCK_INPUT_BYTES as u16);
    put_u16(&mut artifact, 14, BLOCK_OUTPUT_BYTES as u16);
    put_u32(&mut artifact, 16, MODEL_GENERATION);
    put_u16(&mut artifact, 20, coordinates.tensor_id);
    artifact[22] = coordinates.layer;
    artifact[23] = coordinates.role;
    put_u32(&mut artifact, 24, coordinates.row);
    put_u32(&mut artifact, 28, coordinates.block);
    put(&mut artifact, 32, &full_golden_sha256);
    put(&mut artifact, 64, &PINNED_NATIVE_IMAGE_SHA256);
    put(&mut artifact, 96, &MODEL_CONTRACT_SHA256);
    put(&mut artifact, 128, &vectors_sha256);
    put_u32(&mut artifact, 224, coordinates.native_block_offset);
    put_u32(&mut artifact, 228, BLOCK_INPUT_OFFSET as u32);
    put_u32(&mut artifact, 232, BLOCK_OUTPUT_OFFSET as u32);
    put_u16(&mut artifact, 236, CANONICAL_VECTOR_INDEX as u16);
    put_u16(&mut artifact, 238, BLOCK_FLAG_NATIVE_VERIFIED);
    put(&mut artifact, BLOCK_INPUT_OFFSET, &input);
    put(&mut artifact, BLOCK_OUTPUT_OFFSET, &output);
    let payload_hash = sha256(&artifact[BLOCK_INPUT_OFFSET..]);
    put(&mut artifact, 160, &payload_hash);
    let mut seal_view = artifact.clone();
    seal_view[BLOCK_SEAL_OFFSET..BLOCK_SEAL_OFFSET + 32].fill(0);
    let seal = sha256(&seal_view);
    put(&mut artifact, BLOCK_SEAL_OFFSET, &seal);
    validate_block_artifact(&artifact)?;
    Ok(artifact)
}

fn validate_block_artifact(artifact: &[u8]) -> Result<VerifiedBlockArtifact, String> {
    if artifact.len() != BLOCK_ARTIFACT_BYTES
        || artifact.get(..8) != Some(BLOCK_GOLDEN_MAGIC)
        || artifact_u16(artifact, 8)? != 1
        || artifact_u16(artifact, 10)? as usize != BLOCK_HEADER_BYTES
        || artifact_u16(artifact, 12)? as usize != BLOCK_INPUT_BYTES
        || artifact_u16(artifact, 14)? as usize != BLOCK_OUTPUT_BYTES
        || artifact_u32(artifact, 16)? != MODEL_GENERATION
        || artifact_u32(artifact, 228)? as usize != BLOCK_INPUT_OFFSET
        || artifact_u32(artifact, 232)? as usize != BLOCK_OUTPUT_OFFSET
        || artifact_u16(artifact, 236)? as usize != CANONICAL_VECTOR_INDEX
        || artifact_u16(artifact, 238)? != BLOCK_FLAG_NATIVE_VERIFIED
        || artifact[240..BLOCK_HEADER_BYTES]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err("block artifact header mismatch".into());
    }
    if artifact.get(64..96) != Some(PINNED_NATIVE_IMAGE_SHA256.as_slice()) {
        return Err("block artifact native image hash mismatch".into());
    }
    if artifact.get(96..128) != Some(MODEL_CONTRACT_SHA256.as_slice()) {
        return Err("block artifact model contract hash mismatch".into());
    }
    let payload_sha256 = sha256(&artifact[BLOCK_INPUT_OFFSET..]);
    if artifact.get(160..192) != Some(payload_sha256.as_slice()) {
        return Err("block artifact payload hash mismatch".into());
    }
    let seal_sha256: [u8; 32] = artifact[BLOCK_SEAL_OFFSET..BLOCK_SEAL_OFFSET + 32]
        .try_into()
        .unwrap();
    let mut seal_view = artifact.to_vec();
    seal_view[BLOCK_SEAL_OFFSET..BLOCK_SEAL_OFFSET + 32].fill(0);
    if sha256(&seal_view) != seal_sha256 {
        return Err("block artifact seal mismatch".into());
    }
    Ok(VerifiedBlockArtifact {
        full_golden_sha256: artifact[32..64].try_into().unwrap(),
        vectors_sha256: artifact[128..160].try_into().unwrap(),
        coordinates: BlockCoordinates {
            tensor_id: artifact_u16(artifact, 20)?,
            layer: artifact[22],
            role: artifact[23],
            row: artifact_u32(artifact, 24)?,
            block: artifact_u32(artifact, 28)?,
            native_block_offset: artifact_u32(artifact, 224)?,
        },
        input: artifact[BLOCK_INPUT_OFFSET..BLOCK_OUTPUT_OFFSET]
            .try_into()
            .unwrap(),
        output: artifact[BLOCK_OUTPUT_OFFSET..].try_into().unwrap(),
        payload_sha256,
        seal_sha256,
    })
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
    let mut q30_raw = Vec::with_capacity(descriptor.ggml_ne1 as usize);
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
        q30_raw.push(q30_sum);
    }
    Ok(ProjectionResult {
        fp32,
        q30,
        q30_raw,
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

    const CHECKED_FULL_GOLDEN: &[u8] =
        include_bytes!("../../../artifacts/lfm25_layer0_ffn.golden.bin");
    const CHECKED_VECTORS: &[u8] =
        include_bytes!("../../../artifacts/lfm25_layer0_ffn.golden.bin.vectors");
    const CHECKED_BLOCK_GOLDEN: &[u8] =
        include_bytes!("../../../artifacts/lfm25_q8_block.golden.bin");

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

    #[test]
    fn checked_in_block_golden_is_canonical_and_reproducible() {
        validate_artifact(CHECKED_FULL_GOLDEN).unwrap();
        let canonical = derive_canonical_block(CHECKED_FULL_GOLDEN, CHECKED_VECTORS).unwrap();
        assert_eq!(canonical.activation.scale_bits, 0x1830);
        assert_eq!(canonical.weight.scale_bits, 0x0cb9);
        assert_eq!(canonical.dot, -14_901);
        assert_eq!(canonical.term_q30, -9_429_888);
        assert_eq!(
            hex(&sha256(
                &[
                    encode_block_input(&canonical).as_slice(),
                    encode_block_output(&canonical).as_slice()
                ]
                .concat()
            )),
            "2faaf8b87bc3d121642d60b3c95019f61fc88b1bc17c7b264933d06fa3e8f1d1"
        );

        let coordinates = canonical_coordinates().unwrap();
        assert_eq!(coordinates.tensor_id, 4);
        assert_eq!(coordinates.native_block_offset, 0x048c_9000);
        let rebuilt = build_block_artifact(
            sha256(CHECKED_FULL_GOLDEN),
            sha256(CHECKED_VECTORS),
            coordinates,
            encode_block_input(&canonical),
            encode_block_output(&canonical),
        )
        .unwrap();
        assert_eq!(rebuilt, CHECKED_BLOCK_GOLDEN);
        assert_eq!(
            hex(&sha256(&rebuilt)),
            "d05cd8cd89f23dcdae758c7b8fe2a27a55d6ad8de60a33ade60c089da558eed2"
        );
    }

    #[test]
    fn block_golden_detects_payload_and_seal_tampering() {
        let mut payload_tampered = CHECKED_BLOCK_GOLDEN.to_vec();
        payload_tampered[BLOCK_INPUT_OFFSET + 7] ^= 1;
        assert!(validate_block_artifact(&payload_tampered)
            .unwrap_err()
            .contains("payload hash"));

        let mut seal_tampered = CHECKED_BLOCK_GOLDEN.to_vec();
        seal_tampered[BLOCK_SEAL_OFFSET] ^= 1;
        assert!(validate_block_artifact(&seal_tampered)
            .unwrap_err()
            .contains("seal mismatch"));
    }
}
