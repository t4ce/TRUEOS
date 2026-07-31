use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use trueos_kokoro_aot::{ARENA_ALIGNMENT, ParseOptions, Phase, Program, SlotKind, WorkBudget};
use trueos_kokoro_dispatch::{CpuDispatcher, CpuWorkspace, KOKORO_CPU_WORKSPACE_REQUIREMENTS};
use trueos_kokoro_exec::{Executor, ResolvedPhase, RuntimeShape, SliceEvent, TensorShapeTable};
use trueos_kokoro_g2p::{Model as G2pModel, canonicalize_ipa, prepare_english_with};
use trueos_kokoro_lexicon::Lexicon;
use trueos_kokoro_memory::{ExternalBindings, TensorMemory};
use trueos_kokoro_voice::{PINNED_ARCHIVE_SHA256, STYLE_WIDTH, VoiceArchive};

const TOKENS_TENSOR_ID: u32 = 0;
const STYLE_TENSOR_ID: u32 = 1;
const SPEED_TENSOR_ID: u32 = 2;
const TENSOR_COUNT: u32 = 4_744;
const SLOT_COUNT: u32 = 2_055;
const OP_COUNT: u32 = 2_227;
const BINDING_COUNT: u32 = 7_314;
const WAVEFORM_TENSOR_ID: u32 = TENSOR_COUNT - 1;
const SHAPE_CAPACITY: usize = TENSOR_COUNT as usize;
const SLOT_CAPACITY: usize = SLOT_COUNT as usize;
const MAX_OP_BINDINGS: usize = 16;
const WAVEFORM_SAMPLES_PER_FRAME: u32 = 600;
const SAMPLE_RATE: u32 = 24_000;
const MAX_DATA_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ARENA_BYTES: u64 = 1_572_883_968;

const EXPECTED_ARTIFACT_SHA256: [u8; 32] = [
    0xf1, 0xf5, 0xcc, 0xc6, 0x68, 0xe1, 0x71, 0x30, 0x1e, 0x72, 0x20, 0x03, 0x39, 0x92, 0xef, 0xcb,
    0x26, 0x69, 0xf9, 0xd4, 0x01, 0x59, 0x1a, 0xac, 0xe4, 0xf1, 0x02, 0x5a, 0xc1, 0xe3, 0x49, 0x98,
];
const EXPECTED_MODEL_SHA256: [u8; 32] = [
    0x23, 0x9d, 0x9f, 0x4d, 0xf1, 0x12, 0xa3, 0x75, 0xbe, 0xa5, 0x21, 0x46, 0x57, 0x0b, 0x97, 0xeb,
    0x5c, 0x5a, 0xf7, 0x27, 0xc0, 0x07, 0x76, 0x1e, 0xe1, 0x21, 0xed, 0x12, 0x3f, 0xd1, 0xab, 0x29,
];

const REFERENCE_IPA: &str = "həlˈoʊ fɹʌm tɹu oʊ ɛs. ðə kwɪk bɹaʊn fɑks dʒʌmps oʊvɚ ðə leɪzi dɔɡ. spɪtʃ sɪnθəsɪs ɪz naʊ ɹʌnɪŋ ɪn ðə kɜɹnəl, wɪð ə sɪɹiəlaɪzd eɪsɪŋk kju fɔɹ ðə ʃɛl.";
const REFERENCE_FRAMES: u32 = 412;
const REFERENCE_SAMPLES: usize = 247_200;
const REFERENCE_LOGITS_SHA256: &str =
    "6d1489df516fb94f03727b2943d0d11fb3e38b3253db9191e6aa5b4be3acb77a";
const REFERENCE_PRE_QGEMM_SHA256: &str =
    "2f5fb400cb18696b71422d498b70e8dd959b82251e1dc00993d7bc2efd5274f9";
#[derive(Clone, Debug)]
enum Input {
    Ipa(String),
    Text(String),
}

#[derive(Debug)]
struct Config {
    model_dir: PathBuf,
    input: Input,
    voice: String,
    speed: f32,
    raw_path: PathBuf,
    wav_path: PathBuf,
    expect_frames: Option<u32>,
    expect_samples: Option<usize>,
    expect_wav_sha256: Option<String>,
    phase0_only: bool,
}

impl Config {
    fn parse() -> Result<Self, String> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repository = manifest
            .parent()
            .and_then(Path::parent)
            .ok_or("oracle manifest is not nested below the repository")?;
        let mut config = Self {
            model_dir: repository.join("crates/ttstt/.ttstt/models/kokoro"),
            input: Input::Ipa(REFERENCE_IPA.to_owned()),
            voice: "af_heart".to_owned(),
            speed: 1.0,
            raw_path: PathBuf::from("/tmp/trueos-kokoro-native-oracle.f32le"),
            wav_path: PathBuf::from("/tmp/trueos-kokoro-native-oracle.wav"),
            expect_frames: Some(REFERENCE_FRAMES),
            expect_samples: Some(REFERENCE_SAMPLES),
            // Native floating-point kernels are gated numerically against the
            // RTen oracle by verify_kokoro_waveform.py. Byte identity is an
            // optional stronger assertion, not a valid default requirement.
            expect_wav_sha256: None,
            phase0_only: false,
        };

        let mut args = env::args().skip(1);
        let mut custom_input = false;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "-h" | "--help" => usage(0),
                "--reference" => {
                    config.input = Input::Ipa(REFERENCE_IPA.to_owned());
                    config.expect_frames = Some(REFERENCE_FRAMES);
                    config.expect_samples = Some(REFERENCE_SAMPLES);
                    config.expect_wav_sha256 = None;
                    custom_input = false;
                }
                "--model-dir" => {
                    config.model_dir = PathBuf::from(next_value(&mut args, &argument)?)
                }
                "--ipa" => {
                    config.input = Input::Ipa(next_value(&mut args, &argument)?);
                    custom_input = true;
                }
                "--text" => {
                    config.input = Input::Text(next_value(&mut args, &argument)?);
                    custom_input = true;
                }
                "--ipa-file" => {
                    let path = next_value(&mut args, &argument)?;
                    config.input = Input::Ipa(read_text_file(&path)?);
                    custom_input = true;
                }
                "--text-file" => {
                    let path = next_value(&mut args, &argument)?;
                    config.input = Input::Text(read_text_file(&path)?);
                    custom_input = true;
                }
                "--voice" => config.voice = next_value(&mut args, &argument)?,
                "--speed" => {
                    config.speed = parse_value(&next_value(&mut args, &argument)?, &argument)?
                }
                "--phase0-only" => config.phase0_only = true,
                "--raw" => config.raw_path = PathBuf::from(next_value(&mut args, &argument)?),
                "--wav" => config.wav_path = PathBuf::from(next_value(&mut args, &argument)?),
                "--expect-frames" => {
                    config.expect_frames =
                        Some(parse_value(&next_value(&mut args, &argument)?, &argument)?)
                }
                "--expect-samples" => {
                    config.expect_samples =
                        Some(parse_value(&next_value(&mut args, &argument)?, &argument)?)
                }
                "--expect-wav-sha256" => {
                    let digest = next_value(&mut args, &argument)?.to_ascii_lowercase();
                    validate_sha256(&digest)?;
                    config.expect_wav_sha256 = Some(digest);
                }
                _ => return Err(format!("unknown argument {argument:?}; run --help")),
            }
        }

        if custom_input {
            config.expect_frames = None;
            config.expect_samples = None;
            config.expect_wav_sha256 = None;
        }
        if !config.speed.is_finite() || !(0.5..=2.0).contains(&config.speed) {
            return Err(format!("speed {} is outside the sealed 0.5..=2.0 range", config.speed));
        }
        if config.voice.is_empty() {
            return Err("voice name is empty".to_owned());
        }
        Ok(config)
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn parse_value<T: std::str::FromStr>(value: &str, option: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{option} rejected value {value:?}"))
}

fn read_text_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("invalid SHA-256 digest {value:?}"))
    }
}

fn usage(code: i32) -> ! {
    eprintln!(
        "usage: kokoro-native-oracle [OPTIONS]\n\
         \n\
         With no options, run the pinned F=412 RTen parity vector.\n\
         \n\
         Input (choose one):\n\
           --reference                 pinned IPA, frame, and sample expectations\n\
           --ipa IPA                   deterministic pre-phonemized input\n\
           --ipa-file PATH             read deterministic IPA from a file\n\
           --text TEXT                 run native G2P + Misaki first\n\
           --text-file PATH            read English text from a file\n\
         \n\
         Assets and inference:\n\
           --model-dir PATH            directory containing Kokoro assets\n\
           --voice NAME                default: af_heart\n\
           --speed FLOAT               sealed range 0.5..=2.0; default: 1\n\
           --phase0-only               stop after and report duration admission\n\
         \n\
         Outputs and optional parity gates:\n\
           --raw PATH                  little-endian f32 output path\n\
           --wav PATH                  WAVE_FORMAT_EXTENSIBLE f32 path\n\
           --expect-frames N\n\
           --expect-samples N\n\
           --expect-wav-sha256 HEX"
    );
    process::exit(code)
}

#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct AlignedLine([u8; 64]);

struct AlignedBytes {
    lines: Vec<AlignedLine>,
    len: usize,
}

impl AlignedBytes {
    fn zeroed(len: usize) -> Result<Self, String> {
        let lines = len
            .checked_add(63)
            .ok_or("aligned allocation size overflow")?
            / 64;
        let mut storage = Vec::new();
        storage
            .try_reserve_exact(lines)
            .map_err(|_| format!("failed to reserve {len} aligned bytes"))?;
        storage.resize(lines, AlignedLine([0; 64]));
        Ok(Self {
            lines: storage,
            len,
        })
    }

    fn read(path: &Path) -> Result<Self, String> {
        let len = fs::metadata(path)
            .map_err(|error| format!("cannot stat {}: {error}", path.display()))?
            .len();
        let len = usize::try_from(len)
            .map_err(|_| format!("{} is too large for this host", path.display()))?;
        let mut bytes = Self::zeroed(len)?;
        File::open(path)
            .and_then(|mut file| file.read_exact(bytes.as_mut_slice()))
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        Ok(bytes)
    }

    fn as_slice(&self) -> &[u8] {
        // SAFETY: AlignedLine has no padding, contains initialized bytes, and
        // the logical length never exceeds the Vec's allocation.
        unsafe { std::slice::from_raw_parts(self.lines.as_ptr().cast::<u8>(), self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, with an exclusive borrow of the allocation.
        unsafe { std::slice::from_raw_parts_mut(self.lines.as_mut_ptr().cast::<u8>(), self.len) }
    }
}

struct WorkspaceBuffers {
    quant_u8: Vec<u8>,
    packed_i8: Vec<i8>,
    accum_i32: Vec<i32>,
    row_sums_i32: Vec<i32>,
    bias_i32: Vec<i32>,
    lstm_gates_f32: Vec<f32>,
}

impl WorkspaceBuffers {
    fn new() -> Self {
        let required = KOKORO_CPU_WORKSPACE_REQUIREMENTS;
        Self {
            quant_u8: vec![0; required.quant_u8],
            packed_i8: vec![0; required.packed_i8],
            accum_i32: vec![0; required.accum_i32],
            row_sums_i32: vec![0; required.row_sums_i32],
            bias_i32: vec![0; required.bias_i32],
            lstm_gates_f32: vec![0.0; required.lstm_gates_f32],
        }
    }

    fn workspace(&mut self) -> Result<CpuWorkspace<'_>, String> {
        CpuWorkspace::new(
            &mut self.quant_u8,
            &mut self.packed_i8,
            &mut self.accum_i32,
            &mut self.row_sums_i32,
            &mut self.bias_i32,
            &mut self.lstm_gates_f32,
        )
        .map_err(|error| format!("CPU workspace rejected: {error:?}"))
    }
}

struct PreparedInput {
    phonemes: String,
    token_ids: Vec<u8>,
}

fn prepare_input(config: &Config) -> Result<PreparedInput, String> {
    match &config.input {
        Input::Ipa(ipa) => {
            let encoded = canonicalize_ipa(ipa.trim_end())
                .map_err(|error| format!("IPA rejected: {error:?}"))?;
            if encoded.token_ids.is_empty() || encoded.token_ids.len() > 510 {
                return Err(format!(
                    "IPA produced {} tokens; one oracle invocation requires 1..=510",
                    encoded.token_ids.len()
                ));
            }
            Ok(PreparedInput {
                phonemes: encoded.phonemes,
                token_ids: encoded.token_ids,
            })
        }
        Input::Text(text) => {
            let g2p_bytes = fs::read(config.model_dir.join("en.g2p"))
                .map_err(|error| format!("cannot read en.g2p: {error}"))?;
            let lexicon_bytes = fs::read(config.model_dir.join("misaki-us.klex"))
                .map_err(|error| format!("cannot read misaki-us.klex: {error}"))?;
            let g2p = G2pModel::parse_pinned_english(&g2p_bytes)
                .map_err(|error| format!("English G2P rejected: {error:?}"))?;
            let lexicon = Lexicon::parse_pinned_us(&lexicon_bytes)
                .map_err(|error| format!("Misaki lexicon rejected: {error:?}"))?;
            let output = prepare_english_with(&g2p, text.trim_end(), Some(&lexicon))
                .map_err(|error| format!("English frontend rejected: {error:?}"))?;
            if output.chunks.len() != 1 || output.token_ids.is_empty() {
                return Err(format!(
                    "text produced {} tokens in {} chunks; the oracle executes exactly one model chunk",
                    output.token_ids.len(),
                    output.chunks.len()
                ));
            }
            Ok(PreparedInput {
                phonemes: output.phonemes,
                token_ids: output.token_ids,
            })
        }
    }
}

fn parse_program(artifact: &[u8]) -> Result<Program<'_>, String> {
    let options = ParseOptions {
        expected_artifact_sha256: Some(&EXPECTED_ARTIFACT_SHA256),
        expected_model_sha256: Some(&EXPECTED_MODEL_SHA256),
        expected_voices_sha256: Some(&PINNED_ARCHIVE_SHA256),
        max_tensors: TENSOR_COUNT,
        max_slots: SLOT_COUNT,
        max_ops: OP_COUNT,
        max_bindings: BINDING_COUNT,
        max_data_bytes: MAX_DATA_BYTES,
        max_arena_bytes: MAX_ARENA_BYTES,
    };
    let program = Program::parse_with_options(artifact, options)
        .map_err(|error| format!("KKAOT rejected: {error:?}"))?;
    if program.tensor_count() != TENSOR_COUNT
        || program.slot_count() != SLOT_COUNT
        || program.op_count() != OP_COUNT
        || program.binding_count() != BINDING_COUNT
    {
        return Err(format!(
            "KKAOT topology mismatch: tensors={} slots={} ops={} bindings={}",
            program.tensor_count(),
            program.slot_count(),
            program.op_count(),
            program.binding_count()
        ));
    }
    Ok(program)
}

fn bind_phase_zero_shapes(
    program: &Program<'_>,
    shapes: &mut TensorShapeTable<SHAPE_CAPACITY>,
    padded_tokens: &[i64],
) -> Result<(), String> {
    shapes
        .initialize(program)
        .map_err(|error| format!("shape-table initialization failed: {error:?}"))?;
    for (tensor, dimensions) in [
        (TOKENS_TENSOR_ID, &[1, padded_tokens.len() as u32][..]),
        (STYLE_TENSOR_ID, &[1, STYLE_WIDTH as u32][..]),
        (SPEED_TENSOR_ID, &[1][..]),
    ] {
        let shape = RuntimeShape::new(dimensions)
            .map_err(|error| format!("input {tensor} shape rejected: {error:?}"))?;
        shapes
            .bind_external(program, tensor, shape)
            .map_err(|error| format!("input {tensor} binding rejected: {error:?}"))?;
    }
    Ok(())
}

fn failure_location(program: &Program<'_>, executor: &Executor<SLOT_CAPACITY>) -> String {
    let cursor = executor.cursor();
    let opcode = program.op(cursor.op_index()).map(|op| op.opcode);
    format!("op={} opcode={opcode:?} unit={}", cursor.op_index(), cursor.unit_offset())
}

fn failure_bindings(
    program: &Program<'_>,
    executor: &Executor<SLOT_CAPACITY>,
    memory: &TensorMemory<'_, '_, '_, SHAPE_CAPACITY, 1, MAX_OP_BINDINGS>,
) -> String {
    let op_index = executor.cursor().op_index();
    let Some(op) = program.op(op_index) else {
        return "bindings=unavailable".to_owned();
    };
    let mut result = String::from("bindings=[");
    let count = op.input_count + op.output_count;
    for binding in 0..count {
        if binding != 0 {
            result.push_str(", ");
        }
        let tensor = if binding < op.input_count {
            program.op_input(op, binding)
        } else {
            program.op_output(op, binding - op.input_count)
        };
        let role = if binding < op.input_count {
            "in"
        } else {
            "out"
        };
        match tensor {
            Some(tensor) => {
                let shape = memory.tensor_shape(tensor);
                let descriptor = program.tensor(tensor);
                let _ =
                    write!(result, "{role}{binding}:t{tensor}:shape={shape:?}:desc={descriptor:?}");
            }
            None => {
                let _ = write!(result, "{role}{binding}:missing");
            }
        }
    }
    result.push(']');
    result
}

#[derive(Debug)]
struct ResolverAudit {
    op_index: u32,
    logits_producer_op: u32,
    logits_producer_opcode: trueos_kokoro_aot::OpCode,
    pre_qgemm_tensor: u32,
    pre_qgemm_shape: Vec<u32>,
    pre_qgemm_len: usize,
    pre_qgemm_sha256: String,
    pre_qgemm_min: f32,
    pre_qgemm_max: f32,
    pre_qgemm_mean: f64,
    pre_qgemm_rms: f64,
    pre_qgemm_first8: Vec<f32>,
    logits_tensor: u32,
    logits_shape: Vec<u32>,
    logits_len: usize,
    logits_sha256: String,
    logits_min: f32,
    logits_max: f32,
    logits_mean: f64,
    logits_rms: f64,
    speed_tensor: u32,
    speed_shape: Vec<u32>,
    speed_value: f32,
    cumulative_tensor: u32,
    cumulative_shape: Vec<u32>,
    cumulative_sha256: String,
    cumulative: Vec<i64>,
    scalar_tensor: u32,
    scalar_shape: Vec<u32>,
    scalar_value: i64,
    duration_min: i64,
    duration_max: i64,
    duration_sum: i64,
    durations: Vec<i64>,
    duration_histogram: Vec<(i64, usize)>,
}

fn typed_sha256_f32(values: &[f32]) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update(value.to_le_bytes());
    }
    let digest: [u8; 32] = hasher.finalize().into();
    hex_bytes(&digest)
}

fn typed_sha256_i64(values: &[i64]) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update(value.to_le_bytes());
    }
    let digest: [u8; 32] = hasher.finalize().into();
    hex_bytes(&digest)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn audit_resolver<const EXTERNALS: usize>(
    program: &Program<'_>,
    memory: &TensorMemory<'_, '_, '_, SHAPE_CAPACITY, EXTERNALS, MAX_OP_BINDINGS>,
) -> Result<ResolverAudit, String> {
    let mut resolver = None;
    for op_index in 0..program.op_count() {
        let op = program
            .op(op_index)
            .ok_or_else(|| format!("operation {op_index} is missing"))?;
        if op.opcode == trueos_kokoro_aot::OpCode::ResolveDecoderShape {
            if resolver.replace((op_index, op)).is_some() {
                return Err("program contains multiple duration resolvers".to_owned());
            }
        }
    }
    let (op_index, op) = resolver.ok_or("program contains no duration resolver")?;
    if op.input_count != 2 || op.output_count != 2 {
        return Err(format!(
            "resolver arity changed: inputs={} outputs={}",
            op.input_count, op.output_count
        ));
    }
    let tensor = |input: bool, binding: u16| {
        if input {
            program.op_input(op, binding)
        } else {
            program.op_output(op, binding)
        }
        .ok_or_else(|| format!("resolver binding {binding} is missing"))
    };
    let logits_tensor = tensor(true, 0)?;
    let speed_tensor = tensor(true, 1)?;
    let cumulative_tensor = tensor(false, 0)?;
    let scalar_tensor = tensor(false, 1)?;

    let mut logits_producer = None;
    for candidate_index in 0..op_index {
        let candidate = program
            .op(candidate_index)
            .ok_or_else(|| format!("operation {candidate_index} is missing"))?;
        for output in 0..candidate.output_count {
            if program.op_output(candidate, output) == Some(logits_tensor) {
                if logits_producer
                    .replace((candidate_index, candidate))
                    .is_some()
                {
                    return Err(format!(
                        "resolver logits tensor {logits_tensor} has multiple producers"
                    ));
                }
            }
        }
    }
    let (logits_producer_op, logits_producer) = logits_producer
        .ok_or_else(|| format!("resolver logits tensor {logits_tensor} has no producer"))?;
    let pre_qgemm_tensor = program
        .op_input(logits_producer, 0)
        .ok_or("resolver logits producer has no activation input")?;

    let (
        pre_qgemm_shape,
        pre_qgemm_len,
        pre_qgemm_sha256,
        pre_qgemm_min,
        pre_qgemm_max,
        pre_qgemm_mean,
        pre_qgemm_rms,
        pre_qgemm_first8,
    ) = memory
        .with_read::<f32, _, _>(pre_qgemm_tensor, |values, shape| {
            let sum = values.iter().map(|&value| f64::from(value)).sum::<f64>();
            let square_sum = values
                .iter()
                .map(|&value| {
                    let value = f64::from(value);
                    value * value
                })
                .sum::<f64>();
            (
                shape.dims().to_vec(),
                values.len(),
                typed_sha256_f32(values),
                values.iter().copied().fold(f32::INFINITY, f32::min),
                values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                sum / values.len() as f64,
                (square_sum / values.len() as f64).sqrt(),
                values[..values.len().min(8)].to_vec(),
            )
        })
        .map_err(|error| format!("cannot audit pre-qGEMM activation: {error:?}"))?;

    let (logits_shape, logits_len, logits_sha256, logits_min, logits_max, logits_mean, logits_rms) =
        memory
            .with_read::<f32, _, _>(logits_tensor, |values, shape| {
                let sum = values.iter().map(|&value| f64::from(value)).sum::<f64>();
                let square_sum = values
                    .iter()
                    .map(|&value| {
                        let value = f64::from(value);
                        value * value
                    })
                    .sum::<f64>();
                (
                    shape.dims().to_vec(),
                    values.len(),
                    typed_sha256_f32(values),
                    values.iter().copied().fold(f32::INFINITY, f32::min),
                    values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                    sum / values.len() as f64,
                    (square_sum / values.len() as f64).sqrt(),
                )
            })
            .map_err(|error| format!("cannot audit resolver logits: {error:?}"))?;
    let (speed_shape, speed_value) = memory
        .with_read::<f32, _, _>(speed_tensor, |values, shape| {
            (shape.dims().to_vec(), values.first().copied())
        })
        .map_err(|error| format!("cannot audit resolver speed: {error:?}"))?;
    let speed_value = speed_value.ok_or("resolver speed tensor is empty")?;
    let (cumulative_shape, cumulative_sha256, cumulative) = memory
        .with_read::<i64, _, _>(cumulative_tensor, |values, shape| {
            (shape.dims().to_vec(), typed_sha256_i64(values), values.to_vec())
        })
        .map_err(|error| format!("cannot audit cumulative durations: {error:?}"))?;
    let (scalar_shape, scalar_value) = memory
        .with_read::<i64, _, _>(scalar_tensor, |values, shape| {
            (shape.dims().to_vec(), values.first().copied())
        })
        .map_err(|error| format!("cannot audit resolver scalar: {error:?}"))?;
    let scalar_value = scalar_value.ok_or("resolver scalar tensor is empty")?;

    let mut previous = 0i64;
    let mut duration_min = i64::MAX;
    let mut duration_max = i64::MIN;
    let mut durations = Vec::with_capacity(cumulative.len());
    let mut histogram = BTreeMap::new();
    for &value in &cumulative {
        let duration = value
            .checked_sub(previous)
            .ok_or("cumulative duration subtraction overflow")?;
        if duration < 1 {
            return Err(format!("resolver emitted non-positive duration {duration}"));
        }
        duration_min = duration_min.min(duration);
        duration_max = duration_max.max(duration);
        durations.push(duration);
        *histogram.entry(duration).or_insert(0usize) += 1;
        previous = value;
    }
    if cumulative.is_empty() {
        return Err("resolver cumulative output is empty".to_owned());
    }
    if previous != scalar_value {
        return Err(format!(
            "resolver scalar {scalar_value} differs from cumulative tail {previous}"
        ));
    }

    Ok(ResolverAudit {
        op_index,
        logits_producer_op,
        logits_producer_opcode: logits_producer.opcode,
        pre_qgemm_tensor,
        pre_qgemm_shape,
        pre_qgemm_len,
        pre_qgemm_sha256,
        pre_qgemm_min,
        pre_qgemm_max,
        pre_qgemm_mean,
        pre_qgemm_rms,
        pre_qgemm_first8,
        logits_tensor,
        logits_shape,
        logits_len,
        logits_sha256,
        logits_min,
        logits_max,
        logits_mean,
        logits_rms,
        speed_tensor,
        speed_shape,
        speed_value,
        cumulative_tensor,
        cumulative_shape,
        cumulative_sha256,
        cumulative,
        scalar_tensor,
        scalar_shape,
        scalar_value,
        duration_min,
        duration_max,
        duration_sum: previous,
        durations,
        duration_histogram: histogram.into_iter().collect(),
    })
}

fn print_resolver_audit(audit: &ResolverAudit) {
    println!("resolver_op={}", audit.op_index);
    println!("resolver_logits_producer_op={}", audit.logits_producer_op);
    println!("resolver_logits_producer_opcode={:?}", audit.logits_producer_opcode);
    println!("resolver_pre_qgemm_tensor={}", audit.pre_qgemm_tensor);
    println!("resolver_pre_qgemm_shape={:?}", audit.pre_qgemm_shape);
    println!("resolver_pre_qgemm_len={}", audit.pre_qgemm_len);
    println!("resolver_pre_qgemm_sha256={}", audit.pre_qgemm_sha256);
    println!("resolver_pre_qgemm_min={:.9}", audit.pre_qgemm_min);
    println!("resolver_pre_qgemm_max={:.9}", audit.pre_qgemm_max);
    println!("resolver_pre_qgemm_mean={:.12}", audit.pre_qgemm_mean);
    println!("resolver_pre_qgemm_rms={:.12}", audit.pre_qgemm_rms);
    println!("resolver_pre_qgemm_first8={:?}", audit.pre_qgemm_first8);
    println!("resolver_pre_qgemm_reference_sha256={REFERENCE_PRE_QGEMM_SHA256}");
    println!(
        "resolver_pre_qgemm_hash_match={}",
        audit.pre_qgemm_sha256 == REFERENCE_PRE_QGEMM_SHA256
    );
    println!("resolver_logits_tensor={}", audit.logits_tensor);
    println!("resolver_logits_shape={:?}", audit.logits_shape);
    println!("resolver_logits_len={}", audit.logits_len);
    println!("resolver_logits_sha256={}", audit.logits_sha256);
    println!("resolver_logits_min={:.9}", audit.logits_min);
    println!("resolver_logits_max={:.9}", audit.logits_max);
    println!("resolver_logits_mean={:.12}", audit.logits_mean);
    println!("resolver_logits_rms={:.12}", audit.logits_rms);
    println!("resolver_speed_tensor={}", audit.speed_tensor);
    println!("resolver_speed_shape={:?}", audit.speed_shape);
    println!("resolver_speed={:.9}", audit.speed_value);
    println!("resolver_cumulative_tensor={}", audit.cumulative_tensor);
    println!("resolver_cumulative_shape={:?}", audit.cumulative_shape);
    println!("resolver_cumulative_sha256={}", audit.cumulative_sha256);
    println!("resolver_cumulative={:?}", audit.cumulative);
    println!("resolver_scalar_tensor={}", audit.scalar_tensor);
    println!("resolver_scalar_shape={:?}", audit.scalar_shape);
    println!("resolver_scalar={}", audit.scalar_value);
    println!("resolver_duration_min={}", audit.duration_min);
    println!("resolver_duration_max={}", audit.duration_max);
    println!("resolver_duration_sum={}", audit.duration_sum);
    println!("resolver_duration_first16={:?}", &audit.durations[..audit.durations.len().min(16)]);
    println!("resolver_duration_histogram={:?}", audit.duration_histogram);
    println!("resolver_logits_reference_sha256={REFERENCE_LOGITS_SHA256}");
    println!("resolver_logits_hash_match={}", audit.logits_sha256 == REFERENCE_LOGITS_SHA256);
}

fn run_phase_zero(
    program: &Program<'_>,
    executor: &mut Executor<SLOT_CAPACITY>,
    shapes: &mut TensorShapeTable<SHAPE_CAPACITY>,
    arena: &mut AlignedBytes,
    padded_tokens: &[i64],
    style: &[f32; STYLE_WIDTH],
    speed: &[f32; 1],
    workspace_buffers: &mut WorkspaceBuffers,
) -> Result<(ResolvedPhase, ResolverAudit), String> {
    let mut externals = ExternalBindings::<3>::new();
    externals
        .bind_input(program, shapes, TOKENS_TENSOR_ID, padded_tokens)
        .map_err(|error| format!("token memory binding failed: {error:?}"))?;
    externals
        .bind_input(program, shapes, STYLE_TENSOR_ID, style)
        .map_err(|error| format!("style memory binding failed: {error:?}"))?;
    externals
        .bind_input(program, shapes, SPEED_TENSOR_ID, speed)
        .map_err(|error| format!("speed memory binding failed: {error:?}"))?;
    let mut memory: TensorMemory<'_, '_, '_, SHAPE_CAPACITY, 3, MAX_OP_BINDINGS> =
        TensorMemory::phase_zero(program, shapes, arena.as_mut_slice(), &mut externals)
            .map_err(|error| format!("phase-zero memory rejected: {error:?}"))?;
    let mut workspace = workspace_buffers.workspace()?;
    let mut dispatcher = CpuDispatcher::new_with_workspace(&mut memory, &mut workspace);

    loop {
        let mut budget = WorkBudget::new(u32::MAX).expect("non-zero budget");
        let report = executor.run_slice(program, &mut dispatcher, &mut budget);
        match report.event {
            SliceEvent::PhaseAdmitted(admission) => {
                let audit = audit_resolver(program, dispatcher.memory())?;
                return Ok((admission, audit));
            }
            SliceEvent::BudgetExhausted if report.consumed != 0 => {}
            SliceEvent::BudgetExhausted => {
                return Err(format!(
                    "phase zero made no progress at {}",
                    failure_location(program, executor)
                ));
            }
            SliceEvent::DispatchFailed(error) => {
                return Err(format!(
                    "phase-zero dispatch failed at {}: {error:?}",
                    failure_location(program, executor)
                ));
            }
            SliceEvent::Faulted(error) => {
                return Err(format!(
                    "phase-zero executor fault at {}: {error:?}",
                    failure_location(program, executor)
                ));
            }
            SliceEvent::Complete => return Err("phase zero completed without admission".to_owned()),
            SliceEvent::Cancelled => return Err("phase-zero executor was cancelled".to_owned()),
        }
    }
}

fn copy_shared_slots(
    program: &Program<'_>,
    phase_zero: &AlignedBytes,
    phase_one: &mut AlignedBytes,
    frame_count: u32,
) -> Result<(usize, u64), String> {
    let source = phase_zero.as_slice();
    let destination = phase_one.as_mut_slice();
    let mut slot_count = 0usize;
    let mut byte_count = 0u64;
    for slot_id in 0..program.slot_count() {
        let slot = program
            .slot(slot_id)
            .ok_or_else(|| format!("shared slot {slot_id} descriptor is missing"))?;
        if slot.kind != SlotKind::Fixed || slot.phase != Phase::Shared {
            continue;
        }
        let bytes = slot
            .bytes_at(frame_count)
            .map_err(|error| format!("shared slot {slot_id} span rejected: {error:?}"))?;
        let start = usize::try_from(slot.fixed_offset)
            .map_err(|_| format!("shared slot {slot_id} offset is too large"))?;
        let bytes = usize::try_from(bytes)
            .map_err(|_| format!("shared slot {slot_id} span is too large"))?;
        let end = start
            .checked_add(bytes)
            .ok_or_else(|| format!("shared slot {slot_id} range overflow"))?;
        let source_slot = source
            .get(start..end)
            .ok_or_else(|| format!("shared slot {slot_id} exceeds phase-zero arena"))?;
        let destination_slot = destination
            .get_mut(start..end)
            .ok_or_else(|| format!("shared slot {slot_id} exceeds phase-one arena"))?;
        destination_slot.copy_from_slice(source_slot);
        slot_count += 1;
        byte_count = byte_count
            .checked_add(bytes as u64)
            .ok_or("shared byte count overflow")?;
    }
    Ok((slot_count, byte_count))
}

fn run_phase_one(
    program: &Program<'_>,
    executor: &mut Executor<SLOT_CAPACITY>,
    admission: ResolvedPhase,
    slot_bases: &[u64],
    shapes: &mut TensorShapeTable<SHAPE_CAPACITY>,
    arena: &mut AlignedBytes,
    waveform: &mut [f32],
    workspace_buffers: &mut WorkspaceBuffers,
) -> Result<(), String> {
    let waveform_shape = RuntimeShape::new(&[waveform.len() as u32])
        .map_err(|error| format!("waveform shape rejected: {error:?}"))?;
    shapes
        .bind_external(program, WAVEFORM_TENSOR_ID, waveform_shape)
        .map_err(|error| format!("waveform shape binding failed: {error:?}"))?;
    let mut externals = ExternalBindings::<1>::new();
    externals
        .bind_output(program, shapes, WAVEFORM_TENSOR_ID, waveform)
        .map_err(|error| format!("waveform memory binding failed: {error:?}"))?;
    let mut memory: TensorMemory<'_, '_, '_, SHAPE_CAPACITY, 1, MAX_OP_BINDINGS> =
        TensorMemory::phase_one(
            program,
            shapes,
            arena.as_mut_slice(),
            admission,
            slot_bases,
            &mut externals,
        )
        .map_err(|error| format!("phase-one memory rejected: {error:?}"))?;
    let mut workspace = workspace_buffers.workspace()?;
    let mut dispatcher = CpuDispatcher::new_with_workspace(&mut memory, &mut workspace);

    loop {
        let mut budget = WorkBudget::new(u32::MAX).expect("non-zero budget");
        let report = executor.run_slice(program, &mut dispatcher, &mut budget);
        match report.event {
            SliceEvent::Complete => return Ok(()),
            SliceEvent::BudgetExhausted if report.consumed != 0 => {}
            SliceEvent::BudgetExhausted => {
                return Err(format!(
                    "phase one made no progress at {}",
                    failure_location(program, executor)
                ));
            }
            SliceEvent::DispatchFailed(error) => {
                return Err(format!(
                    "phase-one dispatch failed at {}: {error:?}; {}",
                    failure_location(program, executor),
                    failure_bindings(program, executor, dispatcher.memory())
                ));
            }
            SliceEvent::Faulted(error) => {
                return Err(format!(
                    "phase-one executor fault at {}: {error:?}",
                    failure_location(program, executor)
                ));
            }
            SliceEvent::PhaseAdmitted(_) => {
                return Err("phase one reported a duplicate admission".to_owned());
            }
            SliceEvent::Cancelled => return Err("phase-one executor was cancelled".to_owned()),
        }
    }
}

fn wav_header(sample_count: usize) -> Result<Vec<u8>, String> {
    let data_bytes = sample_count
        .checked_mul(size_of::<f32>())
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or("waveform is too large for RIFF/WAVE")?;
    let riff_bytes = data_bytes
        .checked_add(60)
        .ok_or("RIFF byte count overflow")?;
    let mut header = Vec::with_capacity(68);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&riff_bytes.to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&40u32.to_le_bytes());
    header.extend_from_slice(&0xfffeu16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&32u16.to_le_bytes());
    header.extend_from_slice(&22u16.to_le_bytes());
    header.extend_from_slice(&32u16.to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&3u32.to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    header.extend_from_slice(&0x0010u16.to_le_bytes());
    header.extend_from_slice(&[0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71]);
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    debug_assert_eq!(header.len(), 68);
    Ok(header)
}

fn waveform_bytes(waveform: &[f32]) -> Result<Vec<u8>, String> {
    let byte_count = waveform
        .len()
        .checked_mul(size_of::<f32>())
        .ok_or("raw waveform byte count overflow")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_count)
        .map_err(|_| format!("failed to reserve {byte_count} raw waveform bytes"))?;
    for sample in waveform {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(bytes)
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn write_outputs(config: &Config, waveform: &[f32]) -> Result<(String, String, usize), String> {
    let raw = waveform_bytes(waveform)?;
    let header = wav_header(waveform.len())?;
    fs::write(&config.raw_path, &raw)
        .map_err(|error| format!("cannot write {}: {error}", config.raw_path.display()))?;
    let mut wav = File::create(&config.wav_path)
        .map_err(|error| format!("cannot create {}: {error}", config.wav_path.display()))?;
    wav.write_all(&header)
        .and_then(|()| wav.write_all(&raw))
        .map_err(|error| format!("cannot write {}: {error}", config.wav_path.display()))?;

    let raw_sha256 = digest_hex(&raw);
    let mut wav_hasher = Sha256::new();
    wav_hasher.update(&header);
    wav_hasher.update(&raw);
    let wav_digest: [u8; 32] = wav_hasher.finalize().into();
    let mut wav_sha256 = String::with_capacity(64);
    for byte in wav_digest {
        write!(&mut wav_sha256, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok((raw_sha256, wav_sha256, header.len() + raw.len()))
}

fn format_seconds(duration: Duration) -> String {
    format!("{:.6}", duration.as_secs_f64())
}

fn run() -> Result<(), String> {
    let config = Config::parse()?;
    let total_start = Instant::now();
    let load_start = Instant::now();
    let artifact = AlignedBytes::read(&config.model_dir.join("kokoro.kkaot"))?;
    let voices_bytes = fs::read(config.model_dir.join("voices-v1.0.bin"))
        .map_err(|error| format!("cannot read voices-v1.0.bin: {error}"))?;
    let program = parse_program(artifact.as_slice())?;
    let voices = VoiceArchive::parse(&voices_bytes)
        .map_err(|error| format!("voice archive rejected: {error:?}"))?;
    let load_elapsed = load_start.elapsed();

    let frontend_start = Instant::now();
    let prepared = prepare_input(&config)?;
    let frontend_elapsed = frontend_start.elapsed();
    let token_count = prepared.token_ids.len();
    let mut padded_tokens = Vec::with_capacity(token_count + 2);
    padded_tokens.push(0);
    padded_tokens.extend(prepared.token_ids.iter().map(|&token| i64::from(token)));
    padded_tokens.push(0);
    let mut style = [0.0f32; STYLE_WIDTH];
    voices
        .lookup(&config.voice)
        .map_err(|error| format!("voice lookup failed: {error:?}"))?
        .decode_style(token_count, &mut style)
        .map_err(|error| format!("voice style decode failed: {error:?}"))?;
    let speed = [config.speed];

    let mut shapes = Box::new(TensorShapeTable::<SHAPE_CAPACITY>::new());
    bind_phase_zero_shapes(&program, &mut shapes, &padded_tokens)?;
    let phase_zero_bytes = usize::try_from(
        program
            .phase(Phase::Phase0)
            .ok_or("phase-zero plan is missing")?
            .arena_min_bytes,
    )
    .map_err(|_| "phase-zero arena is too large for this host")?;
    let mut phase_zero_arena = AlignedBytes::zeroed(phase_zero_bytes)?;
    debug_assert_eq!(phase_zero_arena.as_slice().as_ptr() as usize % ARENA_ALIGNMENT as usize, 0);
    let mut workspace = WorkspaceBuffers::new();
    let mut executor = Executor::<SLOT_CAPACITY>::new();

    let phase_zero_start = Instant::now();
    let (admission, resolver_audit) = run_phase_zero(
        &program,
        &mut executor,
        &mut shapes,
        &mut phase_zero_arena,
        &padded_tokens,
        &style,
        &speed,
        &mut workspace,
    )?;
    let phase_zero_elapsed = phase_zero_start.elapsed();

    let frame_count = admission.frame_count();
    println!("backend=trueos-kokoro-native-cpu");
    println!("voice={}", config.voice);
    println!("speed={}", config.speed);
    println!("phonemes={:?}", prepared.phonemes);
    println!("phoneme_tokens={token_count}");
    println!("padded_tokens={}", padded_tokens.len());
    println!("frames={frame_count}");
    println!("phase0_arena_bytes={phase_zero_bytes}");
    println!("load_seconds={}", format_seconds(load_elapsed));
    println!("frontend_seconds={}", format_seconds(frontend_elapsed));
    println!("phase0_seconds={}", format_seconds(phase_zero_elapsed));
    print_resolver_audit(&resolver_audit);
    if config.phase0_only {
        println!("total_seconds={}", format_seconds(total_start.elapsed()));
        if let Some(expected) = config.expect_frames
            && frame_count != expected
        {
            return Err(format!(
                "phase-zero frame parity failed: native={frame_count} expected={expected}"
            ));
        }
        println!("phase0_parity=ok");
        return Ok(());
    }

    let sample_count = frame_count
        .checked_mul(WAVEFORM_SAMPLES_PER_FRAME)
        .and_then(|samples| usize::try_from(samples).ok())
        .ok_or("waveform sample count overflow")?;
    let phase_one_bytes = usize::try_from(admission.arena_bytes())
        .map_err(|_| "phase-one arena is too large for this host")?;
    let mut phase_one_arena = AlignedBytes::zeroed(phase_one_bytes)?;
    let (shared_slots, shared_bytes) =
        copy_shared_slots(&program, &phase_zero_arena, &mut phase_one_arena, frame_count)?;
    let slot_bases = executor.slot_bases().to_vec();
    if slot_bases.len() != SLOT_CAPACITY {
        return Err(format!(
            "executor admitted {} slot bases, expected {SLOT_CAPACITY}",
            slot_bases.len()
        ));
    }
    let mut waveform = vec![0.0f32; sample_count];

    let phase_one_start = Instant::now();
    run_phase_one(
        &program,
        &mut executor,
        admission,
        &slot_bases,
        &mut shapes,
        &mut phase_one_arena,
        &mut waveform,
        &mut workspace,
    )?;
    let phase_one_elapsed = phase_one_start.elapsed();

    if waveform.iter().any(|sample| !sample.is_finite()) {
        return Err("native waveform contains a non-finite sample".to_owned());
    }
    let write_start = Instant::now();
    let (raw_sha256, wav_sha256, wav_bytes) = write_outputs(&config, &waveform)?;
    let write_elapsed = write_start.elapsed();

    println!("samples={sample_count}");
    println!("sample_rate_hz={SAMPLE_RATE}");
    println!("audio_seconds={:.6}", sample_count as f64 / SAMPLE_RATE as f64);
    println!("phase1_arena_bytes={phase_one_bytes}");
    println!("shared_slots_copied={shared_slots}");
    println!("shared_bytes_copied={shared_bytes}");
    println!("phase1_seconds={}", format_seconds(phase_one_elapsed));
    println!("write_seconds={}", format_seconds(write_elapsed));
    println!("total_seconds={}", format_seconds(total_start.elapsed()));
    println!("raw_path={}", config.raw_path.display());
    println!("raw_bytes={}", sample_count * size_of::<f32>());
    println!("raw_sha256={raw_sha256}");
    println!("wav_path={}", config.wav_path.display());
    println!("wav_bytes={wav_bytes}");
    println!("wav_sha256={wav_sha256}");

    if let Some(expected) = config.expect_frames
        && frame_count != expected
    {
        return Err(format!("frame parity failed: native={frame_count} expected={expected}"));
    }
    if let Some(expected) = config.expect_samples
        && sample_count != expected
    {
        return Err(format!("sample parity failed: native={sample_count} expected={expected}"));
    }
    if let Some(expected) = &config.expect_wav_sha256
        && wav_sha256 != *expected
    {
        return Err(format!(
            "waveform parity failed: native_wav_sha256={wav_sha256} expected={expected}"
        ));
    }
    println!("parity=ok");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("kokoro-native-oracle: {error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_vector_has_expected_frontend_length() {
        let encoded = canonicalize_ipa(REFERENCE_IPA).unwrap();
        assert_eq!(encoded.phonemes, REFERENCE_IPA);
        assert_eq!(encoded.token_ids.len(), 149);
    }

    #[test]
    fn wav_header_matches_the_rten_extensible_f32_contract() {
        let header = wav_header(REFERENCE_SAMPLES).unwrap();
        assert_eq!(header.len(), 68);
        assert_eq!(&header[..12], b"RIFF\xbc\x16\x0f\x00WAVE");
        assert_eq!(&header[12..20], b"fmt \x28\x00\x00\x00");
        assert_eq!(&header[20..24], b"\xfe\xff\x01\x00");
        assert_eq!(&header[60..64], b"data");
        assert_eq!(u32::from_le_bytes(header[64..68].try_into().unwrap()), 988_800);
    }
}
