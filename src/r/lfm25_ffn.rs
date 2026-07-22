//! One complete, sealed LFM2.5 layer-0 FFN executed through fixed FPGA calls.
//!
//! The native model remains a pinned TRUEOSFS file. Rust performs only range
//! reads, Q8_0 activation packing, orchestration, and verification; every
//! projection dot/scale accumulation and SiLU(gate)*up value is produced by
//! the ahead-of-time TRUEGA circuits and completed through the MSI callback
//! worker.

extern crate alloc;

use alloc::vec::Vec;
use half::f16;
use sha2::{Digest, Sha256};

use crate::r::{fpga_offload, lfm25_model};

const GOLDEN: &[u8; 64_000] =
    include_bytes!("../../crates/trueos-fpga-abi/truega/artifacts/lfm25_layer0_ffn.golden.bin");
const GOLDEN_SHA256: [u8; 32] = [
    0xeb, 0x12, 0x4c, 0x33, 0x3e, 0x7a, 0x70, 0x95, 0xa7, 0x8f, 0xc6, 0xc0, 0x00, 0x4f, 0x90, 0xa4,
    0x3f, 0xa8, 0x25, 0xbd, 0xfd, 0x1a, 0x8f, 0x74, 0xac, 0x9d, 0x67, 0xc5, 0x38, 0x48, 0x41, 0x85,
];
const GATE_Q30_SHA256: [u8; 32] = [
    0x83, 0xea, 0x62, 0x62, 0x6c, 0x1f, 0xe4, 0x36, 0x77, 0xc5, 0x59, 0x90, 0x3d, 0xf9, 0x83, 0xb6,
    0xfc, 0x86, 0x48, 0xfa, 0x10, 0x8f, 0x89, 0xa6, 0xa5, 0xb7, 0xd4, 0xbd, 0xc3, 0x13, 0xad, 0x65,
];
const UP_Q30_SHA256: [u8; 32] = [
    0x4a, 0xa1, 0xd6, 0x08, 0xbb, 0x02, 0xf1, 0xb8, 0x2b, 0x4e, 0x70, 0x6c, 0x87, 0x0b, 0x51, 0xe5,
    0x06, 0x0b, 0x32, 0x4d, 0x7e, 0xa9, 0x7f, 0xe7, 0x4c, 0x30, 0x2a, 0x3e, 0xf4, 0xe3, 0x58, 0xa1,
];
const SILU_Q30_SHA256: [u8; 32] = [
    0x0b, 0xcf, 0x3a, 0xfd, 0x42, 0x70, 0xe2, 0xd4, 0xe1, 0xb7, 0xea, 0xd0, 0xc0, 0x9b, 0x11, 0xf2,
    0xea, 0x08, 0x09, 0x82, 0xed, 0x5d, 0xe2, 0x38, 0xdd, 0x7b, 0x88, 0xed, 0xb1, 0xaf, 0x5a, 0x63,
];
const DOWN_Q30_SHA256: [u8; 32] = [
    0x32, 0xe1, 0xf3, 0xdb, 0x56, 0x1c, 0xc2, 0x7a, 0x7b, 0xe3, 0xdc, 0x7d, 0x35, 0xf7, 0x84, 0xda,
    0xf1, 0x0b, 0x4c, 0xb7, 0x8f, 0xd2, 0x85, 0x80, 0x13, 0x4b, 0xda, 0x83, 0x70, 0x07, 0xda, 0x7a,
];

const GOLDEN_PAYLOAD_OFFSET: usize = 512;
const GOLDEN_VECTOR_LENGTHS: [usize; 5] = [1024, 4608, 4608, 4608, 1024];
const Q8_BLOCK_VALUES: usize = 32;
const Q8_BLOCK_BYTES: usize = 34;
const MODEL_READ_CHUNK: usize = 256 * 1024;
const PROJECTION_BOUND: f32 = 2.0e-6;
const SILU_BOUND: f32 = 2.0e-6;
const PREFLIGHT_GATE_ROW: usize = 125;
const PREFLIGHT_GATE_BLOCKS: usize = 6;
pub const FPGA_PREFLIGHT_CALLS: u64 = PREFLIGHT_GATE_BLOCKS as u64;
/// 221,184 two-block projection calls, 208 activation-cache loads, and 4,608
/// fixed SiLU calls. The six single-block preflight calls remain outside this
/// sealed full-pipeline count.
pub const FPGA_CALLS_PER_FFN: u64 = 226_000;
/// One gate+up+SiLU retirement per intermediate row and one down retirement
/// per output row. This is the first BAR2 streaming checkpoint.
pub const FPGA_STREAM_ROWS_PER_FFN: u64 = 4_608 + 1_024;
pub const FFN_OUTPUT_ELEMENTS: usize = 1_024;
pub const FFN_INPUT_ELEMENTS: usize = 1_024;
pub const FFN_LAYER_COUNT: usize = trueos_fpga_abi::lfm25::MODEL_LAYER_COUNT;
pub const Q8_0_BLOCK_VALUES: usize = Q8_BLOCK_VALUES;
pub const Q8_0_BLOCK_BYTES: usize = Q8_BLOCK_BYTES;
pub type Q8_0Block = [u8; Q8_0_BLOCK_BYTES];

pub fn expected_fpga_calls() -> u64 {
    if fpga_offload::lfm25_row_stream_available() {
        FPGA_STREAM_ROWS_PER_FFN
    } else {
        FPGA_CALLS_PER_FFN
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Preflight,
    Gate,
    Up,
    Silu,
    Down,
}

impl Stage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Gate => "gate",
            Self::Up => "up",
            Self::Silu => "silu",
            Self::Down => "down",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress {
    pub stage: Stage,
    pub completed: usize,
    pub total: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub gate_max_abs: f32,
    pub up_max_abs: f32,
    pub silu_max_abs: f32,
    pub down_max_abs: f32,
    pub gate_sha256: [u8; 32],
    pub up_sha256: [u8; 32],
    pub silu_sha256: [u8; 32],
    pub down_sha256: [u8; 32],
    pub fpga_calls: u64,
    pub interrupt_delta: u64,
    pub timeout_recovery_delta: u64,
    pub streamed: bool,
}

/// The exact hardware output retained for a framework-facing forward pass.
pub struct Execution {
    pub output_q30: Vec<i64>,
    pub report: Report,
}

/// Completion evidence returned by a production FFN forward pass.
///
/// Unlike [`Report`], this contains no golden-vector comparisons. It describes
/// the actual runtime activation, selected model layer, hardware output, and
/// interrupt completion path.
#[derive(Clone, Copy, Debug)]
pub struct ForwardReport {
    pub layer: u8,
    pub output_sha256: [u8; 32],
    pub fpga_calls: u64,
    pub interrupt_delta: u64,
    pub timeout_recovery_delta: u64,
    pub streamed: bool,
}

/// Runtime hardware output retained for a framework-facing forward pass.
pub struct ForwardExecution {
    pub output_q30: Vec<i64>,
    pub report: ForwardReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Golden,
    Model(lfm25_model::Error),
    Tensor,
    Layer,
    StreamUnavailable,
    BufferUnavailable,
    Arithmetic,
    Fpga(fpga_offload::Error),
    HardwareMismatch {
        stage: Stage,
        row: u16,
        block: u8,
        activation_scale: u16,
        weight_scale: u16,
        observed_dot: i32,
        expected_dot: i32,
        observed_term_q30: i64,
        expected_term_q30: i64,
        observed_row_q30: i64,
        expected_row_q30: i64,
    },
    ProjectionBound {
        stage: Stage,
        row: u16,
        observed_q30: i64,
        expected_f32_bits: u32,
        error_f32_bits: u32,
    },
    FixedVectorMismatch(Stage),
    CompletionPath,
}

struct PendingPair {
    block: usize,
    weight: [u8; Q8_BLOCK_BYTES],
    expected_dot: i32,
    expected_term_q30: i64,
    expected_row_q30: i64,
    call: fpga_offload::Lfm25CachedPairCall,
}

impl From<lfm25_model::Error> for Error {
    fn from(value: lfm25_model::Error) -> Self {
        Self::Model(value)
    }
}

impl From<fpga_offload::Error> for Error {
    fn from(value: fpga_offload::Error) -> Self {
        Self::Fpga(value)
    }
}

/// Construct the checked layer-0 activation used by the sealed verifier.
///
/// The result has the same owned Q8_0 representation accepted by
/// [`execute_layer`]; the sealed fixture is therefore only an input producer,
/// not a separate module ABI.
pub fn sealed_layer0_activation() -> Result<Vec<Q8_0Block>, Error> {
    validate_golden()?;
    quantize_golden_vector(0)
}

/// Keep `lumen hello` an exact sealed-vector proof while it exercises the
/// production runtime-input path.
pub fn verify_sealed_layer0_forward(report: &ForwardReport) -> Result<(), Error> {
    if report.layer != 0 {
        return Err(Error::Layer);
    }
    require_hash(Stage::Down, report.output_sha256, DOWN_Q30_SHA256)
}

/// Execute one LFM2.5 FFN layer from an ordinary runtime Q8_0 activation.
///
/// Layer selection is ahead-of-time metadata from the sealed native model.
/// Gate, up, SiLU multiplication, and down projection all execute in the
/// proven BAR2/MSI FPGA fast path. Golden comparison remains exclusively in
/// [`run`] and [`run_with_output`].
pub async fn execute_layer(
    layer: u8,
    activation: Vec<Q8_0Block>,
    progress: impl FnMut(Progress),
) -> Result<ForwardExecution, Error> {
    if usize::from(layer) >= FFN_LAYER_COUNT
        || activation.len() * Q8_BLOCK_VALUES != FFN_INPUT_ELEMENTS
    {
        return Err(if usize::from(layer) >= FFN_LAYER_COUNT {
            Error::Layer
        } else {
            Error::Tensor
        });
    }
    if !fpga_offload::lfm25_row_stream_available() {
        return Err(Error::StreamUnavailable);
    }

    // Production forwards use the already admitted native-image record. A
    // whole-image hash on every layer/token would turn model sealing into the
    // inference hot path; the exhaustive diagnostic below retains that check.
    let image = lfm25_model::open().await?;
    let _lane = fpga_offload::acquire_lfm25_ffn_step_lane().await;
    run_layer_streamed(image, layer, activation, progress).await
}

pub async fn run(progress: impl FnMut(Progress)) -> Result<Report, Error> {
    Ok(run_with_output(progress).await?.report)
}

pub async fn run_with_output(mut progress: impl FnMut(Progress)) -> Result<Execution, Error> {
    validate_golden()?;
    let image = lfm25_model::open().await?;
    lfm25_model::verify_with_progress(&image, |_, _| {}).await?;
    let _lane = fpga_offload::acquire_lfm25_ffn_step_lane().await;
    let normalized = quantize_golden_vector(0)?;
    preflight_gate_transition(&image, &normalized).await?;
    progress(Progress {
        stage: Stage::Preflight,
        completed: 1,
        total: 1,
    });

    if fpga_offload::lfm25_row_stream_available() {
        return run_streamed(image, normalized, progress).await;
    }

    // The six-call preflight is intentionally outside the full-pipeline
    // counters so the sealed 226,000-call contract stays unchanged.
    let before = fpga_offload::stats();
    let (gate, gate_max_abs) = project(
        &image,
        tensor("blk.0.ffn_gate.weight")?,
        &normalized,
        1,
        Stage::Gate,
        &mut progress,
    )
    .await?;
    let gate_sha256 = q30_vector_sha256(&gate);
    require_hash(Stage::Gate, gate_sha256, GATE_Q30_SHA256)?;

    let (up, up_max_abs) =
        project(&image, tensor("blk.0.ffn_up.weight")?, &normalized, 2, Stage::Up, &mut progress)
            .await?;
    let up_sha256 = q30_vector_sha256(&up);
    require_hash(Stage::Up, up_sha256, UP_Q30_SHA256)?;

    let mut silu = try_i64_vec(gate.len())?;
    let mut silu_max_abs = 0.0f32;
    for (index, (&gate_q30, &up_q30)) in gate.iter().zip(&up).enumerate() {
        let value = fpga_offload::lfm25_silu_mul_q30(gate_q30, up_q30).await?;
        silu.push(value);
        let expected = golden_f32(3, index)?;
        let error = q30_error(value, expected);
        silu_max_abs = silu_max_abs.max(error);
        if error > SILU_BOUND {
            return Err(Error::ProjectionBound {
                stage: Stage::Silu,
                row: index as u16,
                observed_q30: value,
                expected_f32_bits: expected.to_bits(),
                error_f32_bits: error.to_bits(),
            });
        }
        if index % 512 == 511 || index + 1 == gate.len() {
            progress(Progress {
                stage: Stage::Silu,
                completed: index + 1,
                total: gate.len(),
            });
        }
    }
    let silu_sha256 = q30_vector_sha256(&silu);
    require_hash(Stage::Silu, silu_sha256, SILU_Q30_SHA256)?;

    let down_activation = quantize_q30_vector(&silu)?;
    let (down, down_max_abs) = project(
        &image,
        tensor("blk.0.ffn_down.weight")?,
        &down_activation,
        4,
        Stage::Down,
        &mut progress,
    )
    .await?;
    let down_sha256 = q30_vector_sha256(&down);
    require_hash(Stage::Down, down_sha256, DOWN_Q30_SHA256)?;

    let after = fpga_offload::stats();
    let fpga_calls = after
        .lfm25_ffn_step_completed
        .saturating_sub(before.lfm25_ffn_step_completed);
    let interrupt_delta = after.interrupts.saturating_sub(before.interrupts);
    let timeout_recovery_delta = after
        .timeout_recoveries
        .saturating_sub(before.timeout_recoveries);
    if fpga_calls != FPGA_CALLS_PER_FFN
        || interrupt_delta < FPGA_CALLS_PER_FFN
        || timeout_recovery_delta != 0
    {
        return Err(Error::CompletionPath);
    }

    Ok(Execution {
        output_q30: down,
        report: Report {
            gate_max_abs,
            up_max_abs,
            silu_max_abs,
            down_max_abs,
            gate_sha256,
            up_sha256,
            silu_sha256,
            down_sha256,
            fpga_calls,
            interrupt_delta,
            timeout_recovery_delta,
            streamed: false,
        },
    })
}

async fn run_streamed(
    image: lfm25_model::NativeImage,
    normalized: Vec<[u8; Q8_BLOCK_BYTES]>,
    mut progress: impl FnMut(Progress),
) -> Result<Execution, Error> {
    let gate_descriptor = tensor("blk.0.ffn_gate.weight")?;
    let up_descriptor = tensor("blk.0.ffn_up.weight")?;
    let down_descriptor = tensor("blk.0.ffn_down.weight")?;
    if gate_descriptor.format != 2
        || up_descriptor.format != 2
        || down_descriptor.format != 2
        || gate_descriptor.ggml_ne0 != 1_024
        || up_descriptor.ggml_ne0 != 1_024
        || gate_descriptor.ggml_ne1 != 4_608
        || up_descriptor.ggml_ne1 != 4_608
        || down_descriptor.ggml_ne0 != 4_608
        || down_descriptor.ggml_ne1 != 1_024
        || normalized.len() != 32
    {
        return Err(Error::Tensor);
    }

    // Read all three exact layer matrices before taking exclusive ownership of
    // the shared MSI bridge. The FPGA remains a fixed asynchronous function;
    // TRUEOS still owns the sealed model and all file I/O.
    let gate_matrix = read_tensor(&image, gate_descriptor).await?;
    let up_matrix = read_tensor(&image, up_descriptor).await?;
    let down_matrix = read_tensor(&image, down_descriptor).await?;

    let mut gate = try_i64_vec(4_608)?;
    let mut up = try_i64_vec(4_608)?;
    let mut silu = try_i64_vec(4_608)?;
    let mut down = try_i64_vec(1_024)?;
    let mut gate_max_abs = 0.0f32;
    let mut up_max_abs = 0.0f32;
    let mut silu_max_abs = 0.0f32;
    let mut down_max_abs = 0.0f32;

    let before = fpga_offload::stats();
    let completion_before = fpga_offload::lfm25_stream_completion_count()?;
    let _transport = fpga_offload::acquire_lfm25_stream_transport().await;
    fpga_offload::lfm25_stream_load_activation(&normalized)?;

    const NARROW_ROW_BYTES: usize = 32 * Q8_BLOCK_BYTES;
    for row in 0..4_608usize {
        let row_start = row * NARROW_ROW_BYTES;
        let row_end = row_start + NARROW_ROW_BYTES;
        let gate_blocks = gate_matrix.get(row_start..row_end).ok_or(Error::Tensor)?;
        let up_blocks = up_matrix.get(row_start..row_end).ok_or(Error::Tensor)?;
        let result =
            fpga_offload::lfm25_stream_gate_up_row(row as u32, gate_blocks, up_blocks).await?;
        gate.push(result.gate_q30);
        up.push(result.up_q30);
        silu.push(result.result_q30);

        for (stage, vector, value, bound, max_abs) in [
            (Stage::Gate, 1usize, result.gate_q30, PROJECTION_BOUND, &mut gate_max_abs),
            (Stage::Up, 2usize, result.up_q30, PROJECTION_BOUND, &mut up_max_abs),
            (Stage::Silu, 3usize, result.result_q30, SILU_BOUND, &mut silu_max_abs),
        ] {
            let expected = golden_f32(vector, row)?;
            let error = q30_error(value, expected);
            *max_abs = (*max_abs).max(error);
            if error > bound {
                return Err(Error::ProjectionBound {
                    stage,
                    row: row as u16,
                    observed_q30: value,
                    expected_f32_bits: expected.to_bits(),
                    error_f32_bits: error.to_bits(),
                });
            }
        }

        if row % 512 == 511 || row + 1 == 4_608 {
            progress(Progress {
                stage: Stage::Gate,
                completed: row + 1,
                total: 4_608,
            });
        }
    }

    let gate_sha256 = q30_vector_sha256(&gate);
    let up_sha256 = q30_vector_sha256(&up);
    let silu_sha256 = q30_vector_sha256(&silu);
    require_hash(Stage::Gate, gate_sha256, GATE_Q30_SHA256)?;
    require_hash(Stage::Up, up_sha256, UP_Q30_SHA256)?;
    require_hash(Stage::Silu, silu_sha256, SILU_Q30_SHA256)?;
    for stage in [Stage::Up, Stage::Silu] {
        for quarter in 1..=4 {
            progress(Progress {
                stage,
                completed: 4_608 * quarter / 4,
                total: 4_608,
            });
        }
    }

    let down_activation = quantize_q30_vector(&silu)?;
    fpga_offload::lfm25_stream_load_activation(&down_activation)?;
    const WIDE_ROW_BYTES: usize = 144 * Q8_BLOCK_BYTES;
    for row in 0..1_024usize {
        let row_start = row * WIDE_ROW_BYTES;
        let row_end = row_start + WIDE_ROW_BYTES;
        let weight_blocks = down_matrix.get(row_start..row_end).ok_or(Error::Tensor)?;
        let value = fpga_offload::lfm25_stream_down_row(row as u32, weight_blocks).await?;
        down.push(value);
        let expected = golden_f32(4, row)?;
        let error = q30_error(value, expected);
        down_max_abs = down_max_abs.max(error);
        if error > PROJECTION_BOUND {
            return Err(Error::ProjectionBound {
                stage: Stage::Down,
                row: row as u16,
                observed_q30: value,
                expected_f32_bits: expected.to_bits(),
                error_f32_bits: error.to_bits(),
            });
        }
        if row % 128 == 127 || row + 1 == 1_024 {
            progress(Progress {
                stage: Stage::Down,
                completed: row + 1,
                total: 1_024,
            });
        }
    }

    let down_sha256 = q30_vector_sha256(&down);
    require_hash(Stage::Down, down_sha256, DOWN_Q30_SHA256)?;
    let completion_after = fpga_offload::lfm25_stream_completion_count()?;
    let fpga_calls = completion_after.wrapping_sub(completion_before) as u64;
    let after = fpga_offload::stats();
    let interrupt_delta = after.interrupts.saturating_sub(before.interrupts);
    let timeout_recovery_delta = after
        .timeout_recoveries
        .saturating_sub(before.timeout_recoveries);
    if fpga_calls != FPGA_STREAM_ROWS_PER_FFN
        || interrupt_delta < FPGA_STREAM_ROWS_PER_FFN
        || timeout_recovery_delta != 0
    {
        return Err(Error::CompletionPath);
    }

    Ok(Execution {
        output_q30: down,
        report: Report {
            gate_max_abs,
            up_max_abs,
            silu_max_abs,
            down_max_abs,
            gate_sha256,
            up_sha256,
            silu_sha256,
            down_sha256,
            fpga_calls,
            interrupt_delta,
            timeout_recovery_delta,
            streamed: true,
        },
    })
}

/// Production BAR2 executor shared by every generated FFN layer.
///
/// This deliberately does not consult the layer-0 golden artifact. The only
/// tensor identity source is the generated native model descriptor table.
async fn run_layer_streamed(
    image: lfm25_model::NativeImage,
    layer: u8,
    activation: Vec<Q8_0Block>,
    mut progress: impl FnMut(Progress),
) -> Result<ForwardExecution, Error> {
    use trueos_fpga_abi::lfm25::TensorRole;

    let gate_descriptor = layer_tensor(layer, TensorRole::FfnGate)?;
    let up_descriptor = layer_tensor(layer, TensorRole::FfnUp)?;
    let down_descriptor = layer_tensor(layer, TensorRole::FfnDown)?;
    if gate_descriptor.format != 2
        || up_descriptor.format != 2
        || down_descriptor.format != 2
        || gate_descriptor.ggml_ne0 != FFN_INPUT_ELEMENTS as u32
        || up_descriptor.ggml_ne0 != FFN_INPUT_ELEMENTS as u32
        || gate_descriptor.ggml_ne1 != 4_608
        || up_descriptor.ggml_ne1 != 4_608
        || down_descriptor.ggml_ne0 != 4_608
        || down_descriptor.ggml_ne1 != FFN_OUTPUT_ELEMENTS as u32
        || activation.len() * Q8_BLOCK_VALUES != FFN_INPUT_ELEMENTS
    {
        return Err(Error::Tensor);
    }

    // Model I/O happens before exclusive ownership of the MSI bridge. The
    // runtime tensor itself remains in native Q8_0 form throughout the API.
    let gate_matrix = read_tensor(&image, gate_descriptor).await?;
    let up_matrix = read_tensor(&image, up_descriptor).await?;
    let down_matrix = read_tensor(&image, down_descriptor).await?;
    let mut silu = try_i64_vec(4_608)?;
    let mut down = try_i64_vec(FFN_OUTPUT_ELEMENTS)?;

    let before = fpga_offload::stats();
    let completion_before = fpga_offload::lfm25_stream_completion_count()?;
    let _transport = fpga_offload::acquire_lfm25_stream_transport().await;
    fpga_offload::lfm25_stream_load_activation(&activation)?;

    const NARROW_ROW_BYTES: usize = 32 * Q8_BLOCK_BYTES;
    for row in 0..4_608usize {
        let row_start = row * NARROW_ROW_BYTES;
        let row_end = row_start + NARROW_ROW_BYTES;
        let gate_blocks = gate_matrix.get(row_start..row_end).ok_or(Error::Tensor)?;
        let up_blocks = up_matrix.get(row_start..row_end).ok_or(Error::Tensor)?;
        let result =
            fpga_offload::lfm25_stream_gate_up_row(row as u32, gate_blocks, up_blocks).await?;
        silu.push(result.result_q30);
        if row % 512 == 511 || row + 1 == 4_608 {
            progress(Progress {
                stage: Stage::Gate,
                completed: row + 1,
                total: 4_608,
            });
        }
    }

    // Gate/up/SiLU are a single fused hardware retirement, but preserve the
    // semantic stage events expected by existing callers.
    for stage in [Stage::Up, Stage::Silu] {
        for quarter in 1..=4 {
            progress(Progress {
                stage,
                completed: 4_608 * quarter / 4,
                total: 4_608,
            });
        }
    }

    let down_activation = quantize_q30_vector(&silu)?;
    fpga_offload::lfm25_stream_load_activation(&down_activation)?;
    const WIDE_ROW_BYTES: usize = 144 * Q8_BLOCK_BYTES;
    for row in 0..FFN_OUTPUT_ELEMENTS {
        let row_start = row * WIDE_ROW_BYTES;
        let row_end = row_start + WIDE_ROW_BYTES;
        let weight_blocks = down_matrix.get(row_start..row_end).ok_or(Error::Tensor)?;
        down.push(fpga_offload::lfm25_stream_down_row(row as u32, weight_blocks).await?);
        if row % 128 == 127 || row + 1 == FFN_OUTPUT_ELEMENTS {
            progress(Progress {
                stage: Stage::Down,
                completed: row + 1,
                total: FFN_OUTPUT_ELEMENTS,
            });
        }
    }

    let completion_after = fpga_offload::lfm25_stream_completion_count()?;
    let fpga_calls = completion_after.wrapping_sub(completion_before) as u64;
    let after = fpga_offload::stats();
    let interrupt_delta = after.interrupts.saturating_sub(before.interrupts);
    let timeout_recovery_delta = after
        .timeout_recoveries
        .saturating_sub(before.timeout_recoveries);
    if fpga_calls != FPGA_STREAM_ROWS_PER_FFN
        || interrupt_delta < FPGA_STREAM_ROWS_PER_FFN
        || timeout_recovery_delta != 0
    {
        return Err(Error::CompletionPath);
    }

    Ok(ForwardExecution {
        report: ForwardReport {
            layer,
            output_sha256: q30_vector_sha256(&down),
            fpga_calls,
            interrupt_delta,
            timeout_recovery_delta,
            streamed: true,
        },
        output_q30: down,
    })
}

async fn preflight_gate_transition(
    image: &lfm25_model::NativeImage,
    activation: &[[u8; Q8_BLOCK_BYTES]],
) -> Result<(), Error> {
    let descriptor = tensor("blk.0.ffn_gate.weight")?;
    let blocks_per_row = descriptor.ggml_ne0 as usize / Q8_BLOCK_VALUES;
    if descriptor.format != 2
        || blocks_per_row != 32
        || activation.len() != blocks_per_row
        || PREFLIGHT_GATE_BLOCKS > blocks_per_row
    {
        return Err(Error::Tensor);
    }

    let row_bytes = blocks_per_row * Q8_BLOCK_BYTES;
    let mut expected_row_q30 = 0i64;
    for block in 0..PREFLIGHT_GATE_BLOCKS {
        let mut weight = [0u8; Q8_BLOCK_BYTES];
        let offset = descriptor.native_offset as u64
            + (PREFLIGHT_GATE_ROW * row_bytes + block * Q8_BLOCK_BYTES) as u64;
        image.read_exact_at(offset, &mut weight).await?;
        let activation_block = &activation[block];
        let expected_dot = integer_dot(activation_block, &weight);
        let expected_term_q30 = q30_term(
            expected_dot,
            u16::from_le_bytes([activation_block[0], activation_block[1]]),
            u16::from_le_bytes([weight[0], weight[1]]),
        )?;
        expected_row_q30 = expected_row_q30
            .checked_add(expected_term_q30)
            .ok_or(Error::Arithmetic)?;
        let result = fpga_offload::lfm25_q8_projection_block(
            false,
            block == 0,
            false,
            block as u8,
            activation_block,
            &weight,
        )
        .await?;
        require_hardware_result(
            Stage::Preflight,
            PREFLIGHT_GATE_ROW,
            block,
            activation_block,
            &weight,
            result,
            expected_dot,
            expected_term_q30,
            expected_row_q30,
        )?;
    }
    Ok(())
}

async fn project(
    image: &lfm25_model::NativeImage,
    descriptor: trueos_fpga_abi::lfm25::NativeTensorDescriptor,
    activation: &[[u8; Q8_BLOCK_BYTES]],
    golden_vector: usize,
    stage: Stage,
    progress: &mut impl FnMut(Progress),
) -> Result<(Vec<i64>, f32), Error> {
    let blocks_per_row = descriptor.ggml_ne0 as usize / Q8_BLOCK_VALUES;
    let rows = descriptor.ggml_ne1 as usize;
    if descriptor.format != 2
        || descriptor.ggml_ne0 as usize % Q8_BLOCK_VALUES != 0
        || activation.len() != blocks_per_row
        || !matches!(blocks_per_row, 32 | 144)
        || descriptor.native_bytes as usize != rows * blocks_per_row * Q8_BLOCK_BYTES
    {
        return Err(Error::Tensor);
    }
    let wide = blocks_per_row == 144;
    let matrix = read_tensor(image, descriptor).await?;
    let row_bytes = blocks_per_row * Q8_BLOCK_BYTES;
    let mut output = try_i64_vec(rows)?;
    let mut max_abs = 0.0f32;

    // Activations are identical for every matrix row. Load each native block
    // once, then carry two weight blocks in every normal 72-byte slot call.
    for (block, activation_block) in activation.iter().enumerate() {
        fpga_offload::lfm25_cache_q8_activation(wide, block as u8, activation_block).await?;
    }

    for row in 0..rows {
        let mut row_q30 = 0i64;
        let mut expected_row_q30 = 0i64;
        let mut pending: Vec<PendingPair> = Vec::new();
        pending
            .try_reserve_exact(blocks_per_row / 2)
            .map_err(|_| Error::BufferUnavailable)?;
        for block in (0..blocks_per_row).step_by(2) {
            let activation0 = &activation[block];
            let activation1 = &activation[block + 1];
            let offset0 = row * row_bytes + block * Q8_BLOCK_BYTES;
            let offset1 = offset0 + Q8_BLOCK_BYTES;
            let weight0: &[u8; Q8_BLOCK_BYTES] = matrix
                .get(offset0..offset0 + Q8_BLOCK_BYTES)
                .ok_or(Error::Tensor)?
                .try_into()
                .map_err(|_| Error::Tensor)?;
            let weight1: &[u8; Q8_BLOCK_BYTES] = matrix
                .get(offset1..offset1 + Q8_BLOCK_BYTES)
                .ok_or(Error::Tensor)?
                .try_into()
                .map_err(|_| Error::Tensor)?;

            let expected_dot0 = integer_dot(activation0, weight0);
            let expected_term0 = q30_term(
                expected_dot0,
                u16::from_le_bytes([activation0[0], activation0[1]]),
                u16::from_le_bytes([weight0[0], weight0[1]]),
            )?;
            expected_row_q30 = expected_row_q30
                .checked_add(expected_term0)
                .ok_or(Error::Arithmetic)?;

            let expected_dot1 = integer_dot(activation1, weight1);
            let expected_term1 = q30_term(
                expected_dot1,
                u16::from_le_bytes([activation1[0], activation1[1]]),
                u16::from_le_bytes([weight1[0], weight1[1]]),
            )?;
            expected_row_q30 = expected_row_q30
                .checked_add(expected_term1)
                .ok_or(Error::Arithmetic)?;

            let call = match fpga_offload::submit_lfm25_q8_cached_pair(
                wide,
                block == 0,
                block + 2 == blocks_per_row,
                block as u8,
                weight0,
                weight1,
            ) {
                Ok(call) => call,
                Err(error) => {
                    // Calls already queued for this stateful row must retire
                    // before the lane guard can be released.
                    for pending_pair in pending {
                        let _ = pending_pair.call.complete().await;
                    }
                    return Err(Error::Fpga(error));
                }
            };
            pending.push(PendingPair {
                block,
                weight: *weight1,
                expected_dot: expected_dot1,
                expected_term_q30: expected_term1,
                expected_row_q30,
                call,
            });
        }

        // The worker consumes these FIFO with exactly one package in flight.
        // Keep draining after the first error so no stateful slot-2 operation
        // can outlive the FFN lane guard.
        let mut row_error = None;
        for pending_pair in pending {
            match pending_pair.call.complete().await {
                Ok(result) => {
                    if row_error.is_none() {
                        let block = pending_pair.block + 1;
                        if let Err(error) = require_hardware_result(
                            stage,
                            row,
                            block,
                            &activation[block],
                            &pending_pair.weight,
                            result,
                            pending_pair.expected_dot,
                            pending_pair.expected_term_q30,
                            pending_pair.expected_row_q30,
                        ) {
                            row_error = Some(error);
                        }
                    }
                    row_q30 = result.row_q30;
                }
                Err(error) => {
                    if row_error.is_none() {
                        row_error = Some(Error::Fpga(error));
                    }
                }
            }
        }
        if let Some(error) = row_error {
            return Err(error);
        }
        output.push(row_q30);
        let expected = golden_f32(golden_vector, row)?;
        let error = q30_error(row_q30, expected);
        max_abs = max_abs.max(error);
        if error > PROJECTION_BOUND {
            return Err(Error::ProjectionBound {
                stage,
                row: row as u16,
                observed_q30: row_q30,
                expected_f32_bits: expected.to_bits(),
                error_f32_bits: error.to_bits(),
            });
        }
        let interval = if rows > 2048 { 512 } else { 128 };
        if row % interval == interval - 1 || row + 1 == rows {
            progress(Progress {
                stage,
                completed: row + 1,
                total: rows,
            });
        }
    }

    Ok((output, max_abs))
}

fn require_hardware_result(
    stage: Stage,
    row: usize,
    block: usize,
    activation: &[u8; Q8_BLOCK_BYTES],
    weight: &[u8; Q8_BLOCK_BYTES],
    result: trueos_fpga_abi::builtins::lfm25_ffn_step::Q8RowBlockResult,
    expected_dot: i32,
    expected_term_q30: i64,
    expected_row_q30: i64,
) -> Result<(), Error> {
    if result.dot == expected_dot
        && result.term_q30 == expected_term_q30
        && result.row_q30 == expected_row_q30
    {
        return Ok(());
    }
    Err(Error::HardwareMismatch {
        stage,
        row: row as u16,
        block: block as u8,
        activation_scale: u16::from_le_bytes([activation[0], activation[1]]),
        weight_scale: u16::from_le_bytes([weight[0], weight[1]]),
        observed_dot: result.dot,
        expected_dot,
        observed_term_q30: result.term_q30,
        expected_term_q30,
        observed_row_q30: result.row_q30,
        expected_row_q30,
    })
}

fn integer_dot(left: &[u8; Q8_BLOCK_BYTES], right: &[u8; Q8_BLOCK_BYTES]) -> i32 {
    left[2..]
        .iter()
        .zip(&right[2..])
        .map(|(&left, &right)| i32::from(left as i8) * i32::from(right as i8))
        .sum()
}

fn q30_term(dot: i32, activation_scale: u16, weight_scale: u16) -> Result<i64, Error> {
    let (activation_significand, activation_exponent) = half_parts(activation_scale)?;
    let (weight_significand, weight_exponent) = half_parts(weight_scale)?;
    if activation_significand == 0 || weight_significand == 0 || dot == 0 {
        return Ok(0);
    }
    let raw = i64::from(dot)
        .checked_mul(i64::from(activation_significand))
        .and_then(|value| value.checked_mul(i64::from(weight_significand)))
        .ok_or(Error::Arithmetic)?;
    let shift = activation_exponent + weight_exponent - 20;
    if shift >= 0 {
        raw.checked_shl(shift as u32).ok_or(Error::Arithmetic)
    } else {
        Ok(round_shift_right_even(raw, (-shift) as u32))
    }
}

fn half_parts(bits: u16) -> Result<(u16, i32), Error> {
    if bits & 0x8000 != 0 {
        return Err(Error::Arithmetic);
    }
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let fraction = bits & 0x03ff;
    match exponent {
        0 => Ok((fraction, 1)),
        31 => Err(Error::Arithmetic),
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

async fn read_tensor(
    image: &lfm25_model::NativeImage,
    descriptor: trueos_fpga_abi::lfm25::NativeTensorDescriptor,
) -> Result<Vec<u8>, Error> {
    let bytes = descriptor.native_bytes as usize;
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes)
        .map_err(|_| Error::BufferUnavailable)?;
    output.resize(bytes, 0);
    let mut done = 0usize;
    while done < bytes {
        let chunk = core::cmp::min(MODEL_READ_CHUNK, bytes - done);
        image
            .read_exact_at(
                descriptor.native_offset as u64 + done as u64,
                &mut output[done..done + chunk],
            )
            .await?;
        done += chunk;
    }
    Ok(output)
}

fn tensor(name: &str) -> Result<trueos_fpga_abi::lfm25::NativeTensorDescriptor, Error> {
    let index = trueos_fpga_abi::lfm25::generated::TENSOR_NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .ok_or(Error::Tensor)?;
    Ok(trueos_fpga_abi::lfm25::generated::TENSORS[index])
}

fn layer_tensor(
    layer: u8,
    role: trueos_fpga_abi::lfm25::TensorRole,
) -> Result<trueos_fpga_abi::lfm25::NativeTensorDescriptor, Error> {
    if usize::from(layer) >= FFN_LAYER_COUNT {
        return Err(Error::Layer);
    }
    trueos_fpga_abi::lfm25::generated::TENSORS
        .iter()
        .copied()
        .find(|descriptor| descriptor.layer == layer && descriptor.role == role as u8)
        .ok_or(Error::Tensor)
}

fn quantize_golden_vector(index: usize) -> Result<Vec<[u8; Q8_BLOCK_BYTES]>, Error> {
    let length = *GOLDEN_VECTOR_LENGTHS.get(index).ok_or(Error::Golden)?;
    let mut values = try_f32_vec(length)?;
    for element in 0..length {
        values.push(golden_f32(index, element)?);
    }
    quantize_f32_vector(&values)
}

fn quantize_q30_vector(values: &[i64]) -> Result<Vec<[u8; Q8_BLOCK_BYTES]>, Error> {
    let mut float_values = try_f32_vec(values.len())?;
    for value in values {
        float_values.push(*value as f32 / ((1u64 << 30) as f32));
    }
    quantize_f32_vector(&float_values)
}

fn quantize_f32_vector(values: &[f32]) -> Result<Vec<[u8; Q8_BLOCK_BYTES]>, Error> {
    if values.len() % Q8_BLOCK_VALUES != 0 {
        return Err(Error::Arithmetic);
    }
    let blocks = values.len() / Q8_BLOCK_VALUES;
    let mut output = Vec::new();
    output
        .try_reserve_exact(blocks)
        .map_err(|_| Error::BufferUnavailable)?;
    for values in values.chunks_exact(Q8_BLOCK_VALUES) {
        let maximum = values
            .iter()
            .fold(0.0f32, |current, value| current.max(value.abs()));
        let scale = maximum / 127.0;
        let inverse = if maximum == 0.0 { 0.0 } else { 127.0 / maximum };
        let mut block = [0u8; Q8_BLOCK_BYTES];
        block[..2].copy_from_slice(&f16::from_f32(scale).to_bits().to_le_bytes());
        for (quant, value) in block[2..].iter_mut().zip(values) {
            *quant = (libm::rintf(*value * inverse) as i8) as u8;
        }
        output.push(block);
    }
    Ok(output)
}

fn golden_f32(vector: usize, element: usize) -> Result<f32, Error> {
    let length = *GOLDEN_VECTOR_LENGTHS.get(vector).ok_or(Error::Golden)?;
    if element >= length {
        return Err(Error::Golden);
    }
    let preceding = GOLDEN_VECTOR_LENGTHS[..vector].iter().sum::<usize>();
    let offset = GOLDEN_PAYLOAD_OFFSET + (preceding + element) * 4;
    let bytes = GOLDEN.get(offset..offset + 4).ok_or(Error::Golden)?;
    let value = f32::from_bits(u32::from_le_bytes(bytes.try_into().map_err(|_| Error::Golden)?));
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::Golden)
    }
}

fn validate_golden() -> Result<(), Error> {
    if &GOLDEN[..8] != b"TGAGFFN1"
        || GOLDEN[92..124] != lfm25_model::NATIVE_IMAGE_SHA256
        || GOLDEN[124..156] != trueos_fpga_abi::lfm25::generated::MODEL_CONTRACT_SHA256
        || <[u8; 32]>::from(Sha256::digest(GOLDEN)) != GOLDEN_SHA256
    {
        return Err(Error::Golden);
    }
    Ok(())
}

fn q30_error(actual: i64, expected: f32) -> f32 {
    (actual as f32 / ((1u64 << 30) as f32) - expected).abs()
}

fn q30_vector_sha256(values: &[i64]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update(value.to_le_bytes());
    }
    hasher.finalize().into()
}

fn require_hash(stage: Stage, observed: [u8; 32], expected: [u8; 32]) -> Result<(), Error> {
    if observed == expected {
        Ok(())
    } else {
        Err(Error::FixedVectorMismatch(stage))
    }
}

fn try_i64_vec(capacity: usize) -> Result<Vec<i64>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| Error::BufferUnavailable)?;
    Ok(values)
}

fn try_f32_vec(capacity: usize) -> Result<Vec<f32>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| Error::BufferUnavailable)?;
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_golden_and_vector_offsets_are_exact() {
        validate_golden().unwrap();
        assert_eq!(golden_f32(0, 0).unwrap().to_bits(), 0x3cd6_d3a5);
        assert_eq!(golden_f32(3, 0).unwrap().to_bits(), 0xb90b_4f42);
        assert_eq!(golden_f32(4, 1023).unwrap().to_bits(), 0x3a45_5612);
    }

    #[test]
    fn sealed_input_quantizes_to_the_runtime_fixture() {
        let blocks = quantize_golden_vector(0).unwrap();
        assert_eq!(blocks.len(), 32);
        assert_eq!(blocks[0], trueos_fpga_abi::builtins::lfm25_ffn_step::GOLDEN_ACTIVATION);
    }

    #[test]
    fn every_generated_layer_has_one_exact_ffn_tensor_set() {
        use trueos_fpga_abi::lfm25::TensorRole;

        for layer in 0..FFN_LAYER_COUNT as u8 {
            let gate = layer_tensor(layer, TensorRole::FfnGate).unwrap();
            let up = layer_tensor(layer, TensorRole::FfnUp).unwrap();
            let down = layer_tensor(layer, TensorRole::FfnDown).unwrap();
            assert_eq!((gate.ggml_ne0, gate.ggml_ne1), (1_024, 4_608));
            assert_eq!((up.ggml_ne0, up.ggml_ne1), (1_024, 4_608));
            assert_eq!((down.ggml_ne0, down.ggml_ne1), (4_608, 1_024));
            assert_eq!((gate.format, up.format, down.format), (2, 2, 2));
        }
    }
}
