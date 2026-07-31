//! Fail-closed native Kokoro backend boundary.
//!
//! The resident service owns all model bytes for the life of the kernel.  This
//! module turns those bytes into validated, zero-copy views during a staged
//! warm job and executes the fully admitted graph through the native CPU
//! dispatcher.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};

use spin::Mutex;
use trueos_kokoro_aot::{
    ARENA_ALIGNMENT, ArenaPlanError, DType, OpCode, ParseOptions, Phase, Program, STATIC_DIM,
    SlotKind, StorageKind, TensorDesc, WorkBudget,
};
use trueos_kokoro_audio::{convert_frame_range, output_frames};
use trueos_kokoro_dispatch::{
    CpuDispatcher, CpuWorkspace, KOKORO_CPU_WORKSPACE_REQUIREMENTS, decode,
    native_dispatch_requires_workspace, native_dispatch_supported,
};
use trueos_kokoro_exec::{
    Executor, ExecutorFault, ResolvedPhase, RuntimeShape, SliceEvent, TensorShapeTable,
};
use trueos_kokoro_g2p::{FrontendOutput, Model as G2pModel, prepare_english_with};
use trueos_kokoro_lexicon::Lexicon;
use trueos_kokoro_memory::{ExternalBindings, TensorMemory};
use trueos_kokoro_voice::{PINNED_ARCHIVE_SHA256, STYLE_WIDTH, VoiceArchive};

use super::ttstt_service::{
    BackendTtsRequest, Direction, InferenceJob, JobProgress, ModelSet, SpeechBackend, SttRequest,
    TTS_PCM_CHANNELS, TTS_PCM_CHUNK_MAX_FRAMES, TtsAudioChunk, TtsOutput, TtsOutputError,
    WorkerContext, install_speech_backend,
};

const BACKEND_NAME: &str = "kokoro-kkaot-cpu";
const KOKORO_AOT_PATH: &str = "models/kokoro/kokoro.kkaot";

const EXPECTED_ARTIFACT_SHA256: [u8; 32] = [
    0xf1, 0xf5, 0xcc, 0xc6, 0x68, 0xe1, 0x71, 0x30, 0x1e, 0x72, 0x20, 0x03, 0x39, 0x92, 0xef, 0xcb,
    0x26, 0x69, 0xf9, 0xd4, 0x01, 0x59, 0x1a, 0xac, 0xe4, 0xf1, 0x02, 0x5a, 0xc1, 0xe3, 0x49, 0x98,
];
const EXPECTED_MODEL_SHA256: [u8; 32] = [
    0x23, 0x9d, 0x9f, 0x4d, 0xf1, 0x12, 0xa3, 0x75, 0xbe, 0xa5, 0x21, 0x46, 0x57, 0x0b, 0x97, 0xeb,
    0x5c, 0x5a, 0xf7, 0x27, 0xc0, 0x07, 0x76, 0x1e, 0xe1, 0x21, 0xed, 0x12, 0x3f, 0xd1, 0xab, 0x29,
];

const TENSOR_COUNT: u32 = 4_744;
const SLOT_COUNT: u32 = 2_055;
const OP_COUNT: u32 = 2_227;
const BINDING_COUNT: u32 = 7_314;
const ARTIFACT_BYTES: usize = 124_081_360;
const DATA_BYTES: usize = 123_223_824;
const PHASE_ONE_OP_START: u32 = 1_079;
const COMPILER_FRAME_CEILING: u32 = 2_560;
/// Phase zero has no runtime-sized storage and therefore one exact arena.
const PHASE_ZERO_ARENA_BYTES: u64 = 33_229_952;
const PHASE_ONE_ARENA_MIN_BYTES: u64 = 12_475_392;
const PHASE_ONE_ARENA_MAX_BYTES: u64 = 1_572_883_968;
/// A duration above this authenticated capacity must split and retry the
/// current text chunk. It must never be clamped or over-allocated.
const SERVICE_FRAME_CEILING: u32 = 1_024;
/// The pinned RTen oracle maps 412 decoder frames to 247,200 24-kHz samples.
const WAVEFORM_SAMPLES_PER_FRAME: u32 = 600;
const MAX_DATA_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SEALED_ARENA_BYTES: u64 = PHASE_ONE_ARENA_MAX_BYTES;

const MIN_SPEED: f32 = 0.5;
const MAX_SPEED: f32 = 2.0;

const _: () = {
    assert!(STYLE_WIDTH == 256);
    assert!(DATA_BYTES as u64 <= MAX_DATA_BYTES);
    assert!(SERVICE_FRAME_CEILING < COMPILER_FRAME_CEILING);
    assert!(COMPILER_FRAME_CEILING * WAVEFORM_SAMPLES_PER_FRAME == 1_536_000);
    assert!(PHASE_ZERO_ARENA_BYTES % ARENA_ALIGNMENT as u64 == 0);
    assert!(PHASE_ONE_ARENA_MIN_BYTES % ARENA_ALIGNMENT as u64 == 0);
    assert!(PHASE_ONE_ARENA_MAX_BYTES % ARENA_ALIGNMENT as u64 == 0);
    assert!(PHASE_ONE_ARENA_MIN_BYTES <= PHASE_ONE_ARENA_MAX_BYTES);
    assert!(ATOMIC_WORK_UNITS_PER_SLICE != 0);
    assert!(COOPERATIVE_WORK_UNITS_PER_SLICE > ATOMIC_WORK_UNITS_PER_SLICE);
    assert!(
        SEALED_FLOAT_CONV_WORK_UNITS + SEALED_RESIZE_WORK_UNITS + SEALED_ATOMIC_OPS as u64
            == SEALED_WORK_UNITS
    );
};

const TOKENS_TENSOR_ID: u32 = 0;
const STYLE_TENSOR_ID: u32 = 1;
const SPEED_TENSOR_ID: u32 = 2;
const WAVEFORM_TENSOR_ID: u32 = TENSOR_COUNT - 1;
const SHAPE_CAPACITY: usize = TENSOR_COUNT as usize;
const SLOT_CAPACITY: usize = SLOT_COUNT as usize;
const MAX_OP_BINDINGS: usize = 16;
/// The pinned max-frame plan contains this exact number of scheduler work
/// units. Most are runtime-sized Resize/float-convolution coordinates; the
/// remaining records are whole-operation adapters with one sealed unit each.
const SEALED_WORK_UNITS: u64 = 97_778_896;
/// Preserve short scheduling checkpoints while walking atomic graph records.
const ATOMIC_WORK_UNITS_PER_SLICE: u32 = 64;
/// Coordinate cap selected from the slowest pinned float-convolution profile.
/// On the i9-13900K oracle it bounds a cooperative tile to about 15 ms; the
/// target i5-14500T retains the same AVX2/FMA and AVX-VNNI execution lanes.
const COOPERATIVE_WORK_UNITS_PER_SLICE: u32 = 32_768;
const SEALED_ATOMIC_OPS: u32 = 2_214;
const SEALED_COOPERATIVE_OPS: u32 = 13;
const SEALED_FLOAT_CONV_WORK_UNITS: u64 = 71_546_922;
const SEALED_RESIZE_WORK_UNITS: u64 = 26_229_760;

// These gates name work that cannot be inferred from a syntactically valid
// artifact. Keeping them explicit prevents a future decoder-only change from
// accidentally making shell2 claim that speech is available.
const RUNTIME_SHAPE_PROPAGATION_COMPLETE: bool = true;
const EXECUTOR_MEMORY_BRIDGE_COMPLETE: bool = true;
// Two independent host executions completed all 2,227 operations with the
// same 249,600-sample payload SHA-256
// 57c2b9b5782ae67a98fd6321034c7c270b44b47147cbbedcdd5f20f0c4ad1ecb.
// The pinned Whisper round-trip recovered the complete reference sentence.
// `verify_kokoro_waveform.py --native-acceptance` rechecks that evidence.
const NATIVE_WAVEFORM_ACCEPTANCE_COMPLETE: bool = true;
const DISPATCH_FAMILY_BLOCKER: &str = "kokoro-dispatch-contract-incomplete";

const WARM_COLD: u8 = 0;
const WARM_RUNNING: u8 = 1;
const WARM_READY: u8 = 2;
const WARM_REJECTED: u8 = 3;

static WARM_STATUS: AtomicU8 = AtomicU8::new(WARM_COLD);
static WARM_FAILURE_REASON: Mutex<Option<&'static str>> = Mutex::new(None);
static WARM_ASSETS: Mutex<Option<WarmAssets>> = Mutex::new(None);
static BACKEND: KokoroBackend = KokoroBackend;

/// Install the singleton before the residency service reaches Ready.
pub fn install() {
    let _ = install_speech_backend(&BACKEND);
}

struct KokoroBackend;

struct WarmAssets {
    program: Program<'static>,
    voices: VoiceArchive<'static>,
    g2p: G2pModel<'static>,
    lexicon: Lexicon<'static>,
    coverage: DispatchCoverage,
}

#[derive(Clone, Copy)]
struct DispatchCoverage {
    missing_ops: u32,
    first_missing: Option<OpCode>,
    workspace_ops: u32,
}

impl DispatchCoverage {
    fn scan(program: &Program<'_>) -> Result<Self, &'static str> {
        let mut coverage = Self {
            missing_ops: 0,
            first_missing: None,
            workspace_ops: 0,
        };
        for op_index in 0..program.op_count() {
            let op = program.op(op_index).ok_or("kokoro-op-missing")?;
            let record = program
                .op_attributes(op)
                .ok_or("kokoro-op-attributes-missing")?;
            let attributes = decode(record, op.opcode).map_err(|error| {
                crate::log_warn!(
                    target: "ttstt";
                    "ttstt: kokoro attribute rejected op={} opcode={:?} error={:?}\n",
                    op_index,
                    op.opcode,
                    error
                );
                "kokoro-attribute-contract-rejected"
            })?;
            if !native_dispatch_supported(attributes) {
                coverage.missing_ops = coverage.missing_ops.saturating_add(1);
                coverage.first_missing.get_or_insert(op.opcode);
            }
            if native_dispatch_requires_workspace(attributes) {
                coverage.workspace_ops = coverage.workspace_ops.saturating_add(1);
            }
        }
        Ok(coverage)
    }
}

fn runtime_blocker(assets: &WarmAssets) -> Option<&'static str> {
    if !(assets.program.artifact().as_ptr() as usize).is_multiple_of(ARENA_ALIGNMENT as usize)
        || !(assets.program.data().as_ptr() as usize).is_multiple_of(ARENA_ALIGNMENT as usize)
    {
        Some("kokoro-artifact-memory-misaligned")
    } else if assets.coverage.missing_ops != 0 {
        Some(DISPATCH_FAMILY_BLOCKER)
    } else if !RUNTIME_SHAPE_PROPAGATION_COMPLETE {
        Some("kokoro-runtime-shapes-incomplete")
    } else if !EXECUTOR_MEMORY_BRIDGE_COMPLETE {
        Some("kokoro-executor-memory-bridge-incomplete")
    } else if !NATIVE_WAVEFORM_ACCEPTANCE_COMPLETE {
        Some("kokoro-waveform-oracle-incomplete")
    } else {
        None
    }
}

impl SpeechBackend for KokoroBackend {
    fn name(&self) -> &'static str {
        BACKEND_NAME
    }

    fn ready(&self) -> bool {
        WARM_STATUS.load(Ordering::Acquire) == WARM_READY
    }

    fn tts_ready(&self) -> bool {
        if !self.ready() {
            return false;
        }
        WARM_ASSETS
            .lock()
            .as_ref()
            .is_some_and(|assets| runtime_blocker(assets).is_none())
    }

    fn stt_ready(&self) -> bool {
        false
    }

    fn warm_failure_reason(&self) -> Option<&'static str> {
        if WARM_STATUS.load(Ordering::Acquire) == WARM_REJECTED {
            *WARM_FAILURE_REASON.lock()
        } else {
            None
        }
    }

    fn create_warm_job(&self) -> Result<Box<dyn InferenceJob>, &'static str> {
        WARM_STATUS
            .compare_exchange(WARM_COLD, WARM_RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|state| match state {
                WARM_RUNNING => "kokoro-warm-already-running",
                WARM_READY => "kokoro-already-warm",
                WARM_REJECTED => "kokoro-warm-permanently-rejected",
                _ => "kokoro-warm-state-invalid",
            })?;
        Ok(Box::new(KokoroWarmJob::new()))
    }

    fn create_tts_job(
        &self,
        request: BackendTtsRequest,
    ) -> Result<Box<dyn InferenceJob>, &'static str> {
        validate_tts_request(&request)?;
        let assets = WARM_ASSETS.lock();
        let assets = assets.as_ref().ok_or("kokoro-not-warm")?;
        if let Some(reason) = runtime_blocker(assets) {
            return Err(reason);
        }
        // The constructor is present so the frontend/memory ownership boundary
        // is sealed before numerical completion. The explicit gates above make
        // this unreachable until execution and waveform parity are complete.
        Ok(Box::new(KokoroTtsJob::new(request)))
    }

    fn create_stt_job(&self, _request: SttRequest) -> Result<Box<dyn InferenceJob>, &'static str> {
        Err("kokoro-backend-does-not-implement-stt")
    }
}

fn validate_tts_request(request: &BackendTtsRequest) -> Result<(), &'static str> {
    if request.request.text.trim().is_empty() {
        return Err("kokoro-empty-text");
    }
    if request.request.voice.trim().is_empty() {
        return Err("kokoro-empty-voice");
    }
    if !request.request.speed.is_finite()
        || !(MIN_SPEED..=MAX_SPEED).contains(&request.request.speed)
    {
        return Err("kokoro-speed-out-of-range");
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum WarmStage {
    Program,
    Voices,
    G2p,
    Lexicon,
    Seal,
}

struct KokoroWarmJob {
    stage: WarmStage,
    program: Option<Program<'static>>,
    voices: Option<VoiceArchive<'static>>,
    g2p: Option<G2pModel<'static>>,
    lexicon: Option<Lexicon<'static>>,
}

impl KokoroWarmJob {
    const fn new() -> Self {
        Self {
            stage: WarmStage::Program,
            program: None,
            voices: None,
            g2p: None,
            lexicon: None,
        }
    }

    fn reject(&self, reason: &'static str) -> JobProgress {
        *WARM_FAILURE_REASON.lock() = Some(reason);
        WARM_STATUS.store(WARM_REJECTED, Ordering::Release);
        JobProgress::Failed(reason)
    }
}

impl Drop for KokoroWarmJob {
    fn drop(&mut self) {
        // A constructed job can be dropped before admission when the bounded
        // service queue is temporarily full. Re-arm that transient case; a
        // parsed success or authenticated rejection has already left Running
        // and must retain its terminal state.
        let _ = WARM_STATUS.compare_exchange(
            WARM_RUNNING,
            WARM_COLD,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

impl InferenceJob for KokoroWarmJob {
    fn direction(&self) -> Direction {
        Direction::Warmup
    }

    fn run_slice(&mut self, models: &'static ModelSet, _worker: WorkerContext) -> JobProgress {
        match self.stage {
            WarmStage::Program => {
                if models.kokoro().path() != KOKORO_AOT_PATH {
                    return self.reject("kokoro-native-artifact-not-resident");
                }
                let options = ParseOptions {
                    expected_artifact_sha256: Some(&EXPECTED_ARTIFACT_SHA256),
                    expected_model_sha256: Some(&EXPECTED_MODEL_SHA256),
                    expected_voices_sha256: Some(&PINNED_ARCHIVE_SHA256),
                    max_tensors: TENSOR_COUNT,
                    max_slots: SLOT_COUNT,
                    max_ops: OP_COUNT,
                    max_bindings: BINDING_COUNT,
                    max_data_bytes: MAX_DATA_BYTES,
                    max_arena_bytes: MAX_SEALED_ARENA_BYTES,
                };
                let program = match Program::parse_with_options(models.kokoro().bytes(), options) {
                    Ok(program) => program,
                    Err(error) => {
                        crate::log_warn!(
                            target: "ttstt";
                            "ttstt: kokoro KKAOT rejected error={:?}\n",
                            error
                        );
                        return self.reject("kokoro-kkaot-rejected");
                    }
                };
                if let Err(reason) = validate_program_contract(&program) {
                    return self.reject(reason);
                }
                self.program = Some(program);
                self.stage = WarmStage::Voices;
                JobProgress::Pending
            }
            WarmStage::Voices => {
                let voices = match VoiceArchive::parse(models.kokoro_voices().bytes()) {
                    Ok(voices) => voices,
                    Err(error) => {
                        crate::log_warn!(
                            target: "ttstt";
                            "ttstt: kokoro voice archive rejected error={:?}\n",
                            error
                        );
                        return self.reject("kokoro-voices-rejected");
                    }
                };
                self.voices = Some(voices);
                self.stage = WarmStage::G2p;
                JobProgress::Pending
            }
            WarmStage::G2p => {
                let Some(image) = models.kokoro_g2p() else {
                    return self.reject("kokoro-g2p-not-resident");
                };
                let g2p = match G2pModel::parse_pinned_english(image.bytes()) {
                    Ok(g2p) => g2p,
                    Err(error) => {
                        crate::log_warn!(
                            target: "ttstt";
                            "ttstt: kokoro G2P rejected error={:?}\n",
                            error
                        );
                        return self.reject("kokoro-g2p-rejected");
                    }
                };
                self.g2p = Some(g2p);
                self.stage = WarmStage::Lexicon;
                JobProgress::Pending
            }
            WarmStage::Lexicon => {
                let Some(image) = models.kokoro_lexicon() else {
                    return self.reject("kokoro-misaki-lexicon-not-resident");
                };
                let lexicon = match Lexicon::parse_pinned_us(image.bytes()) {
                    Ok(lexicon) => lexicon,
                    Err(error) => {
                        crate::log_warn!(
                            target: "ttstt";
                            "ttstt: kokoro Misaki lexicon rejected error={:?}\n",
                            error
                        );
                        return self.reject("kokoro-misaki-lexicon-rejected");
                    }
                };
                self.lexicon = Some(lexicon);
                self.stage = WarmStage::Seal;
                JobProgress::Pending
            }
            WarmStage::Seal => {
                let Some(program) = self.program.take() else {
                    return self.reject("kokoro-warm-program-state-lost");
                };
                let Some(voices) = self.voices.take() else {
                    return self.reject("kokoro-warm-voices-state-lost");
                };
                let Some(g2p) = self.g2p.take() else {
                    return self.reject("kokoro-warm-g2p-state-lost");
                };
                let Some(lexicon) = self.lexicon.take() else {
                    return self.reject("kokoro-warm-lexicon-state-lost");
                };
                let coverage = match DispatchCoverage::scan(&program) {
                    Ok(coverage) => coverage,
                    Err(reason) => return self.reject(reason),
                };
                let memory = g2p.memory_usage();
                let assets = WarmAssets {
                    program,
                    voices,
                    g2p,
                    lexicon,
                    coverage,
                };
                let tts_ready = runtime_blocker(&assets).is_none();
                let blocker = runtime_blocker(&assets).unwrap_or("none");
                crate::log_info!(
                    target: "ttstt";
                    "ttstt: kokoro native warm authenticated artifact_bytes={} tensors={} slots={} ops={} bindings={} sealed_work_units={} atomic_slice_units={} cooperative_slice_units={} phase0_arena_bytes={} phase1_arena_min_bytes={} phase1_arena_max_bytes={} frame_proof_max={} service_frame_cap={} waveform_max_samples_24k={} voices={} g2p_borrowed_bytes={} g2p_index_bytes={} lexicon_bytes={} lexicon_entries={} lexicon_variants={} dispatch_missing_ops={} dispatch_workspace_ops={} first_missing={:?} asset_ready=1 tts_ready={} blocker={}\n",
                    assets.program.artifact().len(),
                    assets.program.tensor_count(),
                    assets.program.slot_count(),
                    assets.program.op_count(),
                    assets.program.binding_count(),
                    SEALED_WORK_UNITS,
                    ATOMIC_WORK_UNITS_PER_SLICE,
                    COOPERATIVE_WORK_UNITS_PER_SLICE,
                    PHASE_ZERO_ARENA_BYTES,
                    PHASE_ONE_ARENA_MIN_BYTES,
                    PHASE_ONE_ARENA_MAX_BYTES,
                    COMPILER_FRAME_CEILING,
                    SERVICE_FRAME_CEILING,
                    COMPILER_FRAME_CEILING * WAVEFORM_SAMPLES_PER_FRAME,
                    assets.voices.len(),
                    memory.borrowed_model_bytes,
                    memory.allocated_index_bytes,
                    assets.lexicon.resident_bytes(),
                    assets.lexicon.entry_count(),
                    assets.lexicon.variant_count(),
                    assets.coverage.missing_ops,
                    assets.coverage.workspace_ops,
                    assets.coverage.first_missing,
                    tts_ready as u8,
                    blocker
                );
                *WARM_ASSETS.lock() = Some(assets);
                WARM_STATUS.store(WARM_READY, Ordering::Release);
                JobProgress::Complete
            }
        }
    }
}

fn validate_program_contract(program: &Program<'_>) -> Result<(), &'static str> {
    if program.artifact().len() != ARTIFACT_BYTES
        || program.data().len() != DATA_BYTES
        || program.tensor_count() != TENSOR_COUNT
        || program.slot_count() != SLOT_COUNT
        || program.op_count() != OP_COUNT
        || program.binding_count() != BINDING_COUNT
    {
        return Err("kokoro-kkaot-structural-count-mismatch");
    }
    let mut sealed_work_units = 0u64;
    let mut atomic_ops = 0u32;
    let mut cooperative_ops = 0u32;
    let mut float_conv_work_units = 0u64;
    let mut resize_work_units = 0u64;
    for op_index in 0..program.op_count() {
        let op = program.op(op_index).ok_or("kokoro-op-descriptor-missing")?;
        sealed_work_units = sealed_work_units
            .checked_add(u64::from(op.work_units))
            .ok_or("kokoro-work-unit-total-overflow")?;
        if op.work_units == 1 {
            atomic_ops = atomic_ops
                .checked_add(1)
                .ok_or("kokoro-atomic-op-count-overflow")?;
            continue;
        }
        cooperative_ops = cooperative_ops
            .checked_add(1)
            .ok_or("kokoro-cooperative-op-count-overflow")?;
        match op.opcode {
            OpCode::Resize => {
                resize_work_units = resize_work_units
                    .checked_add(u64::from(op.work_units))
                    .ok_or("kokoro-resize-work-unit-overflow")?;
            }
            OpCode::FloatConv1d | OpCode::FloatConvTranspose1d => {
                float_conv_work_units = float_conv_work_units
                    .checked_add(u64::from(op.work_units))
                    .ok_or("kokoro-float-conv-work-unit-overflow")?;
            }
            _ => return Err("kokoro-work-unit-family-mismatch"),
        }
    }
    if sealed_work_units != SEALED_WORK_UNITS
        || atomic_ops != SEALED_ATOMIC_OPS
        || cooperative_ops != SEALED_COOPERATIVE_OPS
        || float_conv_work_units != SEALED_FLOAT_CONV_WORK_UNITS
        || resize_work_units != SEALED_RESIZE_WORK_UNITS
    {
        return Err("kokoro-work-unit-contract-mismatch");
    }
    if program.model_sha256() != &EXPECTED_MODEL_SHA256
        || program.voices_sha256() != &PINNED_ARCHIVE_SHA256
    {
        return Err("kokoro-kkaot-provenance-mismatch");
    }
    let phase0 = program
        .phase(Phase::Phase0)
        .ok_or("kokoro-kkaot-phase0-missing")?;
    let phase1 = program
        .phase(Phase::Phase1)
        .ok_or("kokoro-kkaot-phase1-missing")?;
    if phase0.op_start != 0
        || phase0.op_end != PHASE_ONE_OP_START
        || phase0.runtime_sized
        || phase0.arena_min_bytes != PHASE_ZERO_ARENA_BYTES
        || phase0.arena_max_bytes != PHASE_ZERO_ARENA_BYTES
        || phase0.arena_alignment != ARENA_ALIGNMENT
        || phase0.frame_count_min != 0
        || phase0.frame_count_max != 0
        || phase1.op_start != PHASE_ONE_OP_START
        || phase1.op_end != OP_COUNT
        || !phase1.runtime_sized
        || phase1.arena_min_bytes != PHASE_ONE_ARENA_MIN_BYTES
        || phase1.arena_max_bytes != PHASE_ONE_ARENA_MAX_BYTES
        || phase1.arena_alignment != ARENA_ALIGNMENT
        || phase1.frame_count_min != 1
        || phase1.frame_count_max != COMPILER_FRAME_CEILING
    {
        return Err("kokoro-kkaot-phase-contract-mismatch");
    }

    validate_external_tensor(
        program.tensor(TOKENS_TENSOR_ID),
        DType::I64,
        &[1, 512],
        Phase::Phase0,
        true,
        false,
    )?;
    validate_external_tensor(
        program.tensor(STYLE_TENSOR_ID),
        DType::F32,
        &[1, STYLE_WIDTH as u32],
        Phase::Phase0,
        true,
        false,
    )?;
    validate_external_tensor(
        program.tensor(SPEED_TENSOR_ID),
        DType::F32,
        &[1],
        Phase::Phase0,
        true,
        false,
    )?;
    let waveform = program
        .tensor(WAVEFORM_TENSOR_ID)
        .ok_or("kokoro-waveform-tensor-missing")?;
    validate_external_tensor(
        Some(waveform),
        DType::F32,
        &[COMPILER_FRAME_CEILING * WAVEFORM_SAMPLES_PER_FRAME],
        Phase::Phase1,
        false,
        true,
    )?;
    if waveform.symbolic_dim != 0
        || waveform.frame_multiplier != WAVEFORM_SAMPLES_PER_FRAME
        || waveform.frame_addend != 0
    {
        return Err("kokoro-waveform-affine-contract-mismatch");
    }

    let mut graph_external_count = 0u32;
    for tensor_id in 0..program.tensor_count() {
        let tensor = program
            .tensor(tensor_id)
            .ok_or("kokoro-tensor-descriptor-missing")?;
        if tensor.is_input() || tensor.is_output() {
            graph_external_count = graph_external_count.saturating_add(1);
        }
    }
    if graph_external_count != 4 {
        return Err("kokoro-graph-external-count-mismatch");
    }
    Ok(())
}

fn executor_work_units_per_slice(program: &Program<'_>, executor: &Executor<SLOT_CAPACITY>) -> u32 {
    let cursor = executor.cursor();
    match program.op(cursor.op_index()) {
        Some(op)
            if op.work_units > 1
                && matches!(
                    op.opcode,
                    OpCode::Resize | OpCode::FloatConv1d | OpCode::FloatConvTranspose1d
                ) =>
        {
            COOPERATIVE_WORK_UNITS_PER_SLICE
        }
        _ => ATOMIC_WORK_UNITS_PER_SLICE,
    }
}

fn validate_external_tensor(
    tensor: Option<TensorDesc>,
    dtype: DType,
    dims: &[u32],
    phase: Phase,
    input: bool,
    output: bool,
) -> Result<(), &'static str> {
    let tensor = tensor.ok_or("kokoro-external-tensor-missing")?;
    if tensor.dtype != dtype
        || tensor.storage != StorageKind::External
        || tensor.phase != phase
        || tensor.rank as usize != dims.len()
        || &tensor.max_dims[..dims.len()] != dims
        || tensor.is_input() != input
        || tensor.is_output() != output
    {
        return Err("kokoro-external-tensor-contract-mismatch");
    }
    if tensor_id_is_static_input(input, output) && tensor.symbolic_dim != STATIC_DIM {
        return Err("kokoro-input-unexpectedly-dynamic");
    }
    Ok(())
}

const fn tensor_id_is_static_input(input: bool, output: bool) -> bool {
    input && !output
}

enum TtsStage {
    Frontend,
    PrepareChunk,
    Execute,
    EmitPcm,
    Finish,
}

/// Cooperative ownership for one serialized shell2 request. Factory admission
/// remains fail-closed until the native waveform parity gate is sealed, but the
/// complete execution and PCM path lives here rather than behind a host shim.
struct KokoroTtsJob {
    request: BackendTtsRequest,
    stage: TtsStage,
    frontend: Option<FrontendOutput>,
    model_chunk_index: usize,
    invocation: Option<InvocationScaffold>,
    emission: Option<PcmEmission>,
}

impl KokoroTtsJob {
    const fn new(request: BackendTtsRequest) -> Self {
        Self {
            request,
            stage: TtsStage::Frontend,
            frontend: None,
            model_chunk_index: 0,
            invocation: None,
            emission: None,
        }
    }

    fn fail(&mut self, reason: &'static str) -> JobProgress {
        if let Some(invocation) = self.invocation.as_mut() {
            invocation.cancel();
        }
        if let Some(capture) = self.request.request.capture.as_ref() {
            capture.fail(reason);
        }
        let _ = self.request.output.finish_error(reason);
        JobProgress::Failed(reason)
    }

    /// Replace an over-cap current chunk with two deterministic subchunks and
    /// rerun phase zero. The duration result is never clamped and no phase-one
    /// allocation is attempted for a service-rejected frame count.
    fn split_current_chunk_for_retry(&mut self) -> Result<(), &'static str> {
        self.invocation = None;
        self.emission = None;
        let frontend = self.frontend.as_mut().ok_or("kokoro-frontend-state-lost")?;
        let range = frontend
            .chunks
            .get(self.model_chunk_index)
            .cloned()
            .ok_or("kokoro-model-chunk-state-lost")?;
        let (first, second) =
            split_retry_range(&frontend.token_ids, range).ok_or("kokoro-frame-cap-unsplittable")?;
        frontend
            .chunks
            .try_reserve(1)
            .map_err(|_| "kokoro-chunk-split-allocation-failed")?;
        frontend.chunks[self.model_chunk_index] = first;
        frontend.chunks.insert(self.model_chunk_index + 1, second);
        self.stage = TtsStage::PrepareChunk;
        Ok(())
    }
}

fn split_retry_range(
    token_ids: &[u8],
    range: core::ops::Range<usize>,
) -> Option<(core::ops::Range<usize>, core::ops::Range<usize>)> {
    if range.end > token_ids.len() || range.start.checked_add(1)? >= range.end {
        return None;
    }
    let midpoint = range.start + (range.end - range.start) / 2;
    let split = (range.start + 1..=midpoint)
        .rev()
        .find(|&boundary| retry_boundary_token(token_ids[boundary - 1]))
        .or_else(|| {
            (midpoint + 1..range.end)
                .find(|&boundary| retry_boundary_token(token_ids[boundary - 1]))
        })
        .unwrap_or(midpoint);
    Some((range.start..split, split..range.end))
}

const fn retry_boundary_token(token: u8) -> bool {
    // Sentence, clause, and whitespace tokens from the sealed Kokoro vocab.
    matches!(token, 1 | 2 | 3 | 4 | 5 | 6 | 9 | 10 | 16)
}

impl InferenceJob for KokoroTtsJob {
    fn direction(&self) -> Direction {
        Direction::TextToSpeech
    }

    fn run_slice(&mut self, _models: &'static ModelSet, _worker: WorkerContext) -> JobProgress {
        if self.request.output.cancelled() {
            return self.fail("tts-stream-cancelled");
        }
        match self.stage {
            TtsStage::Frontend => {
                let assets_guard = WARM_ASSETS.lock();
                let Some(assets) = assets_guard.as_ref() else {
                    return self.fail("kokoro-warm-state-lost");
                };
                let frontend = match prepare_english_with(
                    &assets.g2p,
                    &self.request.request.text,
                    Some(&assets.lexicon),
                ) {
                    Ok(frontend) if !frontend.token_ids.is_empty() => frontend,
                    Ok(_) => return self.fail("kokoro-frontend-produced-no-tokens"),
                    Err(error) => {
                        crate::log_warn!(
                            target: "ttstt";
                            "ttstt: kokoro frontend rejected error={:?}\n",
                            error
                        );
                        return self.fail("kokoro-frontend-rejected");
                    }
                };
                self.frontend = Some(frontend);
                self.stage = TtsStage::PrepareChunk;
                JobProgress::Pending
            }
            TtsStage::PrepareChunk => {
                let Some(frontend) = self.frontend.as_ref() else {
                    return self.fail("kokoro-frontend-state-lost");
                };
                let Some(range) = frontend.chunks.get(self.model_chunk_index).cloned() else {
                    if self.model_chunk_index == 0 {
                        return self.fail("kokoro-finished-without-waveform");
                    }
                    self.stage = TtsStage::Finish;
                    return JobProgress::Pending;
                };
                let assets = WARM_ASSETS.lock();
                let Some(assets) = assets.as_ref() else {
                    return self.fail("kokoro-warm-state-lost");
                };
                let invocation = match InvocationScaffold::prepare(
                    &assets.program,
                    &assets.voices,
                    &frontend.token_ids[range],
                    frontend.token_ids.len(),
                    &self.request.request.voice,
                    self.request.request.speed,
                ) {
                    Ok(invocation) => invocation,
                    Err(reason) => return self.fail(reason),
                };
                self.invocation = Some(invocation);
                self.stage = TtsStage::Execute;
                JobProgress::Pending
            }
            TtsStage::Execute => {
                let assets_guard = WARM_ASSETS.lock();
                let Some(assets) = assets_guard.as_ref() else {
                    return self.fail("kokoro-warm-state-lost");
                };
                let Some(invocation) = self.invocation.as_mut() else {
                    return self.fail("kokoro-invocation-state-lost");
                };
                match invocation.run_slice(&assets.program) {
                    InvocationProgress::Pending => JobProgress::Pending,
                    InvocationProgress::SplitRequired(frame_count) => {
                        crate::log_info!(
                            target: "ttstt";
                            "ttstt: kokoro chunk split retry model_chunk={} frames={} service_cap={}\n",
                            self.model_chunk_index,
                            frame_count,
                            SERVICE_FRAME_CEILING
                        );
                        drop(assets_guard);
                        match self.split_current_chunk_for_retry() {
                            Ok(()) => JobProgress::Pending,
                            Err(reason) => self.fail(reason),
                        }
                    }
                    InvocationProgress::Complete => {
                        let Some(invocation) = self.invocation.take() else {
                            return self.fail("kokoro-invocation-state-lost");
                        };
                        let waveform = match invocation.into_waveform() {
                            Ok(waveform) => waveform,
                            Err(reason) => return self.fail(reason),
                        };
                        let Some(frontend) = self.frontend.as_ref() else {
                            return self.fail("kokoro-frontend-state-lost");
                        };
                        let Some(range) = frontend.chunks.get(self.model_chunk_index) else {
                            return self.fail("kokoro-model-chunk-state-lost");
                        };
                        let phonemes = match u16::try_from(range.end - range.start) {
                            Ok(phonemes) if phonemes != 0 => phonemes,
                            _ => return self.fail("kokoro-model-chunk-size-invalid"),
                        };
                        let model_chunk_index = match u32::try_from(self.model_chunk_index) {
                            Ok(index) => index,
                            Err(_) => return self.fail("kokoro-model-chunk-index-overflow"),
                        };
                        self.emission =
                            match PcmEmission::new(waveform, model_chunk_index, phonemes) {
                                Ok(emission) => Some(emission),
                                Err(reason) => return self.fail(reason),
                            };
                        self.stage = TtsStage::EmitPcm;
                        JobProgress::Pending
                    }
                    InvocationProgress::Failed(reason) => self.fail(reason),
                }
            }
            TtsStage::EmitPcm => {
                let Some(emission) = self.emission.as_mut() else {
                    return self.fail("kokoro-pcm-emission-state-lost");
                };
                match emission.run_slice(&self.request.output) {
                    Ok(EmissionProgress::Pending) => JobProgress::Pending,
                    Ok(EmissionProgress::Complete) => {
                        let Some(emission) = self.emission.take() else {
                            return self.fail("kokoro-pcm-emission-state-lost");
                        };
                        if let Some(capture) = self.request.request.capture.as_ref() {
                            let (waveform, model_chunk_index, model_chunk_phonemes) =
                                emission.into_capture_parts();
                            capture.push_raw_model_chunk(
                                model_chunk_index,
                                model_chunk_phonemes,
                                waveform,
                            );
                        }
                        self.model_chunk_index = match self.model_chunk_index.checked_add(1) {
                            Some(index) => index,
                            None => return self.fail("kokoro-model-chunk-index-overflow"),
                        };
                        self.stage = TtsStage::PrepareChunk;
                        JobProgress::Pending
                    }
                    Err(reason) => self.fail(reason),
                }
            }
            TtsStage::Finish => {
                if self.request.output.finish_success() {
                    JobProgress::Complete
                } else {
                    JobProgress::Failed("kokoro-output-finish-raced")
                }
            }
        }
    }
}

struct PendingPcm {
    chunk: TtsAudioChunk,
    frame_end: usize,
}

struct PcmEmission {
    waveform: Vec<f32>,
    next_frame: usize,
    total_frames: usize,
    model_chunk_index: u32,
    model_chunk_phonemes: u16,
    pending: Option<PendingPcm>,
}

#[derive(Clone, Copy)]
enum EmissionProgress {
    Pending,
    Complete,
}

impl PcmEmission {
    fn new(
        waveform: Vec<f32>,
        model_chunk_index: u32,
        model_chunk_phonemes: u16,
    ) -> Result<Self, &'static str> {
        if waveform.is_empty() {
            return Err("kokoro-waveform-empty");
        }
        if waveform.iter().any(|sample| !sample.is_finite()) {
            return Err("kokoro-waveform-non-finite");
        }
        let total_frames =
            output_frames(waveform.len()).map_err(|_| "kokoro-pcm-frame-count-invalid")?;
        if total_frames == 0 {
            return Err("kokoro-waveform-empty");
        }
        Ok(Self {
            waveform,
            next_frame: 0,
            total_frames,
            model_chunk_index,
            model_chunk_phonemes,
            pending: None,
        })
    }

    fn run_slice(&mut self, output: &TtsOutput) -> Result<EmissionProgress, &'static str> {
        if self.pending.is_none() {
            if self.next_frame == self.total_frames {
                return Ok(EmissionProgress::Complete);
            }
            let remaining = self.total_frames - self.next_frame;
            let frames = remaining.min(TTS_PCM_CHUNK_MAX_FRAMES);
            let sample_count = frames
                .checked_mul(TTS_PCM_CHANNELS)
                .ok_or("kokoro-pcm-size-overflow")?;
            let mut samples = Vec::new();
            samples
                .try_reserve_exact(sample_count)
                .map_err(|_| "kokoro-pcm-allocation-failed")?;
            samples.resize(sample_count, 0_i16);
            convert_frame_range(&self.waveform, self.next_frame, &mut samples)
                .map_err(|_| "kokoro-pcm-conversion-failed")?;
            let frame_end = self
                .next_frame
                .checked_add(frames)
                .ok_or("kokoro-pcm-frame-count-invalid")?;
            self.pending = Some(PendingPcm {
                chunk: TtsAudioChunk {
                    samples_i16_stereo_48k: samples,
                    model_chunk_index: self.model_chunk_index,
                    model_chunk_phonemes: self.model_chunk_phonemes,
                    end_of_model_chunk: frame_end == self.total_frames,
                },
                frame_end,
            });
        }

        let Some(pending) = self.pending.take() else {
            return Err("kokoro-pcm-emission-state-lost");
        };
        let frame_end = pending.frame_end;
        match output.try_push(pending.chunk) {
            Ok(()) => {
                self.next_frame = frame_end;
                if self.next_frame == self.total_frames {
                    Ok(EmissionProgress::Complete)
                } else {
                    Ok(EmissionProgress::Pending)
                }
            }
            Err(TtsOutputError::WouldBlock(chunk)) => {
                self.pending = Some(PendingPcm { chunk, frame_end });
                Ok(EmissionProgress::Pending)
            }
            Err(TtsOutputError::Closed(_)) => Err("kokoro-pcm-output-closed"),
            Err(TtsOutputError::Invalid { reason, .. }) => Err(reason),
        }
    }

    fn into_capture_parts(self) -> (Vec<f32>, u32, u16) {
        (self.waveform, self.model_chunk_index, self.model_chunk_phonemes)
    }
}

#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct ArenaLine([u8; 64]);

struct AlignedArena {
    lines: Vec<ArenaLine>,
    bytes: usize,
}

impl AlignedArena {
    fn try_new(bytes: usize) -> Result<Self, &'static str> {
        let line_count = bytes.checked_add(63).ok_or("kokoro-arena-size-overflow")? / 64;
        let mut lines = Vec::new();
        lines
            .try_reserve_exact(line_count)
            .map_err(|_| "kokoro-arena-allocation-failed")?;
        lines.resize(line_count, ArenaLine([0; 64]));
        Ok(Self { lines, bytes })
    }

    fn as_mut_bytes(&mut self) -> &mut [u8] {
        // SAFETY: ArenaLine is a repr(C), 64-byte-aligned wrapper containing
        // exactly 64 initialized bytes. `bytes` never exceeds its Vec storage,
        // and this exclusive borrow keeps the resulting byte slice unique.
        unsafe { core::slice::from_raw_parts_mut(self.lines.as_mut_ptr().cast::<u8>(), self.bytes) }
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: the same representation proof as `as_mut_bytes` applies;
        // this borrow is shared and bounded by the logical byte length.
        unsafe { core::slice::from_raw_parts(self.lines.as_ptr().cast::<u8>(), self.bytes) }
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
    fn try_new() -> Result<Self, &'static str> {
        let required = KOKORO_CPU_WORKSPACE_REQUIREMENTS;
        Ok(Self {
            quant_u8: try_zeroed_vec(required.quant_u8, 0_u8)?,
            packed_i8: try_zeroed_vec(required.packed_i8, 0_i8)?,
            accum_i32: try_zeroed_vec(required.accum_i32, 0_i32)?,
            row_sums_i32: try_zeroed_vec(required.row_sums_i32, 0_i32)?,
            bias_i32: try_zeroed_vec(required.bias_i32, 0_i32)?,
            lstm_gates_f32: try_zeroed_vec(required.lstm_gates_f32, 0.0_f32)?,
        })
    }

    fn workspace(&mut self) -> Result<CpuWorkspace<'_>, &'static str> {
        CpuWorkspace::new(
            &mut self.quant_u8,
            &mut self.packed_i8,
            &mut self.accum_i32,
            &mut self.row_sums_i32,
            &mut self.bias_i32,
            &mut self.lstm_gates_f32,
        )
        .map_err(|_| "kokoro-cpu-workspace-rejected")
    }
}

fn try_zeroed_vec<T: Clone>(len: usize, value: T) -> Result<Vec<T>, &'static str> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(len)
        .map_err(|_| "kokoro-cpu-workspace-allocation-failed")?;
    output.resize(len, value);
    Ok(output)
}

#[derive(Clone, Copy)]
enum InvocationStage {
    PhaseZero,
    PhaseOne(ResolvedPhase),
    Complete,
}

enum InvocationProgress {
    Pending,
    SplitRequired(u32),
    Complete,
    Failed(&'static str),
}

/// External bindings and phase-zero arena proven without retaining any
/// self-referential borrows. TensorMemory is reconstructed around these stable
/// owned buffers for each future executor slice.
struct InvocationScaffold {
    shapes: Box<TensorShapeTable<SHAPE_CAPACITY>>,
    padded_tokens: Vec<i64>,
    style: [f32; STYLE_WIDTH],
    speed: [f32; 1],
    phase_zero_arena: Option<AlignedArena>,
    phase_one_arena: Option<AlignedArena>,
    slot_bases: Vec<u64>,
    waveform: Vec<f32>,
    workspace: WorkspaceBuffers,
    executor: Executor<SLOT_CAPACITY>,
    stage: InvocationStage,
}

impl InvocationScaffold {
    fn prepare(
        program: &Program<'_>,
        voices: &VoiceArchive<'_>,
        token_ids: &[u8],
        full_phoneme_count: usize,
        voice_name: &str,
        speed: f32,
    ) -> Result<Self, &'static str> {
        if token_ids.is_empty() || token_ids.len() > 510 {
            return Err("kokoro-model-chunk-size-invalid");
        }
        if !speed.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
            return Err("kokoro-speed-out-of-range");
        }
        let padded_len = token_ids
            .len()
            .checked_add(2)
            .ok_or("kokoro-token-count-overflow")?;
        let mut padded_tokens = Vec::new();
        padded_tokens
            .try_reserve_exact(padded_len)
            .map_err(|_| "kokoro-token-allocation-failed")?;
        padded_tokens.push(0);
        padded_tokens.extend(token_ids.iter().map(|&token| i64::from(token)));
        padded_tokens.push(0);

        let voice = voices
            .lookup(voice_name)
            .map_err(|_| "kokoro-voice-not-found")?;
        let mut style = [0.0_f32; STYLE_WIDTH];
        // Match the Ubuntu/tts-rs path: every subchunk uses one stable style
        // row selected from the full request token count. VoiceArchive clamps
        // counts above 509 to the archive's last valid row transactionally.
        voice
            .decode_style(full_phoneme_count, &mut style)
            .map_err(|_| "kokoro-style-decode-failed")?;

        let mut shapes = Box::new(TensorShapeTable::<SHAPE_CAPACITY>::new());
        shapes
            .initialize(program)
            .map_err(|_| "kokoro-shape-table-init-failed")?;
        shapes
            .bind_external(
                program,
                TOKENS_TENSOR_ID,
                RuntimeShape::new(&[1, padded_len as u32])
                    .map_err(|_| "kokoro-token-shape-invalid")?,
            )
            .map_err(|_| "kokoro-token-shape-bind-failed")?;
        shapes
            .bind_external(
                program,
                STYLE_TENSOR_ID,
                RuntimeShape::new(&[1, STYLE_WIDTH as u32])
                    .map_err(|_| "kokoro-style-shape-invalid")?,
            )
            .map_err(|_| "kokoro-style-shape-bind-failed")?;
        shapes
            .bind_external(
                program,
                SPEED_TENSOR_ID,
                RuntimeShape::new(&[1]).map_err(|_| "kokoro-speed-shape-invalid")?,
            )
            .map_err(|_| "kokoro-speed-shape-bind-failed")?;

        let phase_zero_bytes = usize::try_from(
            program
                .phase(Phase::Phase0)
                .ok_or("kokoro-phase0-missing")?
                .arena_min_bytes,
        )
        .map_err(|_| "kokoro-phase0-arena-too-large")?;
        let phase_zero_arena = AlignedArena::try_new(phase_zero_bytes)?;
        let workspace = WorkspaceBuffers::try_new()?;
        let mut scaffold = Self {
            shapes,
            padded_tokens,
            style,
            speed: [speed],
            phase_zero_arena: Some(phase_zero_arena),
            phase_one_arena: None,
            slot_bases: Vec::new(),
            waveform: Vec::new(),
            workspace,
            executor: Executor::new(),
            stage: InvocationStage::PhaseZero,
        };
        scaffold.validate_phase_zero_bridge(program)?;
        Ok(scaffold)
    }

    fn validate_phase_zero_bridge(&mut self, program: &Program<'_>) -> Result<(), &'static str> {
        let Self {
            shapes,
            padded_tokens,
            style,
            speed,
            phase_zero_arena,
            ..
        } = self;
        let phase_zero_arena = phase_zero_arena
            .as_mut()
            .ok_or("kokoro-phase0-arena-state-lost")?;
        let mut externals = ExternalBindings::<3>::new();
        externals
            .bind_input(program, shapes.as_ref(), TOKENS_TENSOR_ID, padded_tokens)
            .map_err(|_| "kokoro-token-memory-bind-failed")?;
        externals
            .bind_input(program, shapes.as_ref(), STYLE_TENSOR_ID, style)
            .map_err(|_| "kokoro-style-memory-bind-failed")?;
        externals
            .bind_input(program, shapes.as_ref(), SPEED_TENSOR_ID, speed)
            .map_err(|_| "kokoro-speed-memory-bind-failed")?;
        let _memory = TensorMemory::<'_, '_, '_, SHAPE_CAPACITY, 3, MAX_OP_BINDINGS>::phase_zero(
            program,
            shapes.as_mut(),
            phase_zero_arena.as_mut_bytes(),
            &mut externals,
        )
        .map_err(|error| {
            crate::log_warn!(
                target: "ttstt";
                "ttstt: kokoro phase0 memory bridge rejected stage=prepare error={:?}\n",
                error
            );
            "kokoro-phase0-memory-bridge-rejected"
        })?;
        Ok(())
    }

    fn run_slice(&mut self, program: &Program<'_>) -> InvocationProgress {
        match self.stage {
            InvocationStage::PhaseZero => self.run_phase_zero_slice(program),
            InvocationStage::PhaseOne(admission) => self.run_phase_one_slice(program, admission),
            InvocationStage::Complete => InvocationProgress::Complete,
        }
    }

    fn run_phase_zero_slice(&mut self, program: &Program<'_>) -> InvocationProgress {
        let Self {
            shapes,
            padded_tokens,
            style,
            speed,
            phase_zero_arena,
            workspace,
            executor,
            ..
        } = self;
        let Some(phase_zero_arena) = phase_zero_arena.as_mut() else {
            return InvocationProgress::Failed("kokoro-phase0-arena-state-lost");
        };
        let mut externals = ExternalBindings::<3>::new();
        if externals
            .bind_input(program, shapes.as_ref(), TOKENS_TENSOR_ID, padded_tokens)
            .is_err()
        {
            return InvocationProgress::Failed("kokoro-token-memory-bind-failed");
        }
        if externals
            .bind_input(program, shapes.as_ref(), STYLE_TENSOR_ID, style)
            .is_err()
        {
            return InvocationProgress::Failed("kokoro-style-memory-bind-failed");
        }
        if externals
            .bind_input(program, shapes.as_ref(), SPEED_TENSOR_ID, speed)
            .is_err()
        {
            return InvocationProgress::Failed("kokoro-speed-memory-bind-failed");
        }
        let mut memory =
            match TensorMemory::<'_, '_, '_, SHAPE_CAPACITY, 3, MAX_OP_BINDINGS>::phase_zero(
                program,
                shapes.as_mut(),
                phase_zero_arena.as_mut_bytes(),
                &mut externals,
            ) {
                Ok(memory) => memory,
                Err(error) => {
                    crate::log_warn!(
                        target: "ttstt";
                        "ttstt: kokoro phase0 memory bridge rejected stage=run error={:?}\n",
                        error
                    );
                    return InvocationProgress::Failed("kokoro-phase0-memory-bridge-rejected");
                }
            };
        let mut cpu_workspace = match workspace.workspace() {
            Ok(workspace) => workspace,
            Err(reason) => return InvocationProgress::Failed(reason),
        };
        let mut dispatcher = CpuDispatcher::new_with_workspace(&mut memory, &mut cpu_workspace);
        let mut budget = match WorkBudget::new(executor_work_units_per_slice(program, executor)) {
            Ok(budget) => budget,
            Err(_) => return InvocationProgress::Failed("kokoro-executor-budget-invalid"),
        };
        let report = executor.run_slice(program, &mut dispatcher, &mut budget);
        match report.event {
            SliceEvent::BudgetExhausted if report.consumed != 0 => InvocationProgress::Pending,
            SliceEvent::BudgetExhausted => {
                Self::log_execution_failure::<trueos_kokoro_dispatch::DispatchError>(
                    program,
                    executor,
                    "phase0-stalled",
                    None,
                );
                InvocationProgress::Failed("kokoro-phase0-made-no-progress")
            }
            SliceEvent::PhaseAdmitted(admission) => {
                if admission.frame_count() > SERVICE_FRAME_CEILING {
                    return InvocationProgress::SplitRequired(admission.frame_count());
                }
                match self.prepare_phase_one(program, admission) {
                    Ok(()) => InvocationProgress::Pending,
                    Err(reason) => InvocationProgress::Failed(reason),
                }
            }
            SliceEvent::DispatchFailed(error) => {
                Self::log_execution_failure(program, executor, "phase0-dispatch", Some(&error));
                InvocationProgress::Failed("kokoro-phase0-dispatch-failed")
            }
            SliceEvent::Faulted(ExecutorFault::Arena(ArenaPlanError::FrameCountOutOfRange)) => {
                // The executor intentionally does not expose an unadmitted
                // frame scalar. A sealed range rejection is sufficient to
                // split and rerun phase zero without allocating phase one.
                InvocationProgress::SplitRequired(COMPILER_FRAME_CEILING.saturating_add(1))
            }
            SliceEvent::Faulted(error) => {
                Self::log_execution_failure::<trueos_kokoro_dispatch::DispatchError>(
                    program,
                    executor,
                    "phase0-fault",
                    None,
                );
                crate::log_warn!(
                    target: "ttstt";
                    "ttstt: kokoro phase0 executor fault error={:?}\n",
                    error
                );
                InvocationProgress::Failed("kokoro-phase0-executor-fault")
            }
            SliceEvent::Complete => {
                InvocationProgress::Failed("kokoro-phase0-completed-without-admission")
            }
            SliceEvent::Cancelled => InvocationProgress::Failed("kokoro-executor-cancelled"),
        }
    }

    fn prepare_phase_one(
        &mut self,
        program: &Program<'_>,
        admission: ResolvedPhase,
    ) -> Result<(), &'static str> {
        let frame_count = admission.frame_count();
        if frame_count == 0 {
            return Err("kokoro-frame-count-zero");
        }
        if frame_count > SERVICE_FRAME_CEILING {
            return Err("kokoro-frame-count-split-required");
        }
        let mut slot_bases = Vec::new();
        slot_bases
            .try_reserve_exact(program.slot_count() as usize)
            .map_err(|_| "kokoro-slot-table-allocation-failed")?;
        slot_bases.extend_from_slice(self.executor.slot_bases());
        if slot_bases.len() != program.slot_count() as usize {
            return Err("kokoro-slot-table-count-mismatch");
        }
        let arena_bytes = usize::try_from(admission.arena_bytes())
            .map_err(|_| "kokoro-phase1-arena-too-large")?;
        let samples = frame_count
            .checked_mul(WAVEFORM_SAMPLES_PER_FRAME)
            .and_then(|samples| usize::try_from(samples).ok())
            .ok_or("kokoro-waveform-size-overflow")?;
        self.shapes
            .bind_external(
                program,
                WAVEFORM_TENSOR_ID,
                RuntimeShape::new(&[samples as u32])
                    .map_err(|_| "kokoro-waveform-shape-invalid")?,
            )
            .map_err(|_| "kokoro-waveform-shape-bind-failed")?;
        let mut phase_one_arena = AlignedArena::try_new(arena_bytes)?;
        let phase_zero_arena = self
            .phase_zero_arena
            .take()
            .ok_or("kokoro-phase0-arena-state-lost")?;
        copy_shared_slots(program, &phase_zero_arena, &mut phase_one_arena, frame_count)?;
        let mut waveform = Vec::new();
        waveform
            .try_reserve_exact(samples)
            .map_err(|_| "kokoro-waveform-allocation-failed")?;
        waveform.resize(samples, 0.0);
        self.phase_one_arena = Some(phase_one_arena);
        self.slot_bases = slot_bases;
        self.waveform = waveform;
        self.stage = InvocationStage::PhaseOne(admission);
        Ok(())
    }

    fn run_phase_one_slice(
        &mut self,
        program: &Program<'_>,
        admission: ResolvedPhase,
    ) -> InvocationProgress {
        let Self {
            shapes,
            phase_one_arena,
            slot_bases,
            waveform,
            workspace,
            executor,
            stage,
            ..
        } = self;
        let Some(phase_one_arena) = phase_one_arena.as_mut() else {
            return InvocationProgress::Failed("kokoro-phase1-arena-state-lost");
        };
        let mut externals = ExternalBindings::<1>::new();
        if externals
            .bind_output(program, shapes.as_ref(), WAVEFORM_TENSOR_ID, waveform.as_mut_slice())
            .is_err()
        {
            return InvocationProgress::Failed("kokoro-waveform-memory-bind-failed");
        }
        let mut memory =
            match TensorMemory::<'_, '_, '_, SHAPE_CAPACITY, 1, MAX_OP_BINDINGS>::phase_one(
                program,
                shapes.as_mut(),
                phase_one_arena.as_mut_bytes(),
                admission,
                slot_bases,
                &mut externals,
            ) {
                Ok(memory) => memory,
                Err(_) => {
                    return InvocationProgress::Failed("kokoro-phase1-memory-bridge-rejected");
                }
            };
        let mut cpu_workspace = match workspace.workspace() {
            Ok(workspace) => workspace,
            Err(reason) => return InvocationProgress::Failed(reason),
        };
        let mut dispatcher = CpuDispatcher::new_with_workspace(&mut memory, &mut cpu_workspace);
        let mut budget = match WorkBudget::new(executor_work_units_per_slice(program, executor)) {
            Ok(budget) => budget,
            Err(_) => return InvocationProgress::Failed("kokoro-executor-budget-invalid"),
        };
        let report = executor.run_slice(program, &mut dispatcher, &mut budget);
        match report.event {
            SliceEvent::BudgetExhausted if report.consumed != 0 => InvocationProgress::Pending,
            SliceEvent::BudgetExhausted => {
                Self::log_execution_failure::<trueos_kokoro_dispatch::DispatchError>(
                    program,
                    executor,
                    "phase1-stalled",
                    None,
                );
                InvocationProgress::Failed("kokoro-phase1-made-no-progress")
            }
            SliceEvent::Complete => {
                *stage = InvocationStage::Complete;
                InvocationProgress::Complete
            }
            SliceEvent::DispatchFailed(error) => {
                Self::log_execution_failure(program, executor, "phase1-dispatch", Some(&error));
                InvocationProgress::Failed("kokoro-phase1-dispatch-failed")
            }
            SliceEvent::Faulted(error) => {
                Self::log_execution_failure::<trueos_kokoro_dispatch::DispatchError>(
                    program,
                    executor,
                    "phase1-fault",
                    None,
                );
                crate::log_warn!(
                    target: "ttstt";
                    "ttstt: kokoro phase1 executor fault error={:?}\n",
                    error
                );
                InvocationProgress::Failed("kokoro-phase1-executor-fault")
            }
            SliceEvent::PhaseAdmitted(_) => {
                InvocationProgress::Failed("kokoro-phase1-duplicate-admission")
            }
            SliceEvent::Cancelled => InvocationProgress::Failed("kokoro-executor-cancelled"),
        }
    }

    fn log_execution_failure<E: core::fmt::Debug>(
        program: &Program<'_>,
        executor: &Executor<SLOT_CAPACITY>,
        kind: &'static str,
        error: Option<&E>,
    ) {
        let cursor = executor.cursor();
        let opcode = program.op(cursor.op_index()).map(|op| op.opcode);
        crate::log_warn!(
            target: "ttstt";
            "ttstt: kokoro execution failure kind={} op={} opcode={:?} unit={} error={:?}\n",
            kind,
            cursor.op_index(),
            opcode,
            cursor.unit_offset(),
            error
        );
    }

    fn cancel(&mut self) {
        let _ = self.executor.cancel();
    }

    fn into_waveform(self) -> Result<Vec<f32>, &'static str> {
        if !matches!(self.stage, InvocationStage::Complete) {
            return Err("kokoro-waveform-before-executor-complete");
        }
        if self.waveform.is_empty() {
            return Err("kokoro-waveform-empty");
        }
        Ok(self.waveform)
    }
}

fn copy_shared_slots(
    program: &Program<'_>,
    phase_zero: &AlignedArena,
    phase_one: &mut AlignedArena,
    frame_count: u32,
) -> Result<(), &'static str> {
    let source = phase_zero.as_bytes();
    let destination = phase_one.as_mut_bytes();
    for slot_id in 0..program.slot_count() {
        let slot = program
            .slot(slot_id)
            .ok_or("kokoro-shared-slot-descriptor-missing")?;
        if slot.kind != SlotKind::Fixed || slot.phase != Phase::Shared {
            continue;
        }
        let bytes = slot
            .bytes_at(frame_count)
            .map_err(|_| "kokoro-shared-slot-span-invalid")?;
        let start =
            usize::try_from(slot.fixed_offset).map_err(|_| "kokoro-shared-slot-range-invalid")?;
        let bytes = usize::try_from(bytes).map_err(|_| "kokoro-shared-slot-range-invalid")?;
        let end = start
            .checked_add(bytes)
            .ok_or("kokoro-shared-slot-range-invalid")?;
        let source_slot = source
            .get(start..end)
            .ok_or("kokoro-shared-slot-exceeds-phase0")?;
        let destination_slot = destination
            .get_mut(start..end)
            .ok_or("kokoro-shared-slot-exceeds-phase1")?;
        destination_slot.copy_from_slice(source_slot);
    }
    Ok(())
}
