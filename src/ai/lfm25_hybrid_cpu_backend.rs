//! Fixed LFM2.5 CPU + AVX-VNNI decode backend.
//!
//! The fixed control plane remains the 99-operation Lumen AOT schedule. CPU
//! kernels execute state, normalization, attention-reduction, nonlinear stages,
//! and every native-row Q8_0 projection. No graph interpreter, Q8x16 repack, or
//! generic GEMM abstraction is introduced.

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use sha2::{Digest, Sha256};
use spin::Mutex;
use trueos_time::{Duration, Timer};

use trueos_lfm25_cpu as cpu;
use trueos_lfm25_model::lfm25::{self, NativeTensorDescriptor, TensorFormat, TensorRole};
use trueos_lfm25_model::lfm25_decode::{
    DecodeCapabilities, DecodeOpKind, EmbeddingRowPlan, LayerStateSlot,
};

use crate::ai::lfm25_decode::{
    AotDecodeBackend, AotDecodeCallback, AotDecodeOutput, AotDecodeRequest, HiddenQ8, HiddenQ30,
    ResidentTensorHandle,
};
use crate::ai::{lfm25_f32, lfm25_model};
use crate::cpu_task_pool::{CpuTaskPool, CpuTaskPoolSnapshot};
use crate::wait::CompletionCell;
use crate::workers::ComputeWorkerPolicy;

const HIDDEN: usize = lfm25::MODEL_HIDDEN_SIZE as usize;
const HEADS: usize = lfm25::MODEL_ATTENTION_HEADS as usize;
const KV_HEADS: usize = lfm25::MODEL_KV_HEADS as usize;
const HEAD_DIM: usize = lfm25::MODEL_HEAD_DIMENSION as usize;
const KV_ELEMENTS: usize = KV_HEADS * HEAD_DIM;
const ATTENTION_REDUCTION_TILE: usize = 256;
const CPU_VNNI_MAX_BATCH_PROJECTIONS: usize = 3;
// Fill performance cores first, then admit E/unknown AP lanes under the same
// cap. Every selected worker rechecks VNNI before touching a shard. This is a
// separate cap from Lumen's session task pool: it bounds only one projection's
// row fan-out, while the scheduler still issues AOT operations in order.
const CPU_VNNI_ROW_POOL_CAP: usize = 16;
const CPU_VNNI_VALUES_PER_SHARD: usize = 1_024 * 1_024;
static CPU_VNNI_ROW_POOL: CpuTaskPool =
    CpuTaskPool::new(ComputeWorkerPolicy::PerformanceFirst, CPU_VNNI_ROW_POOL_CAP);
const MODEL_READ_CHUNK: usize = 256 * 1024;
const MODEL_ALIGNMENT: usize = 64;
const CPU_CONNECTION_GENERATION: u32 = 0x4350_5531; // "CPU1"
const CPU_SESSION_EPOCH: u32 = 1;
const RESIDENT_COLD: u8 = 0;
const RESIDENT_BUILDING: u8 = 1;
const RESIDENT_READY: u8 = 2;
const RESIDENT_WAIT_MS: u64 = 10;
const SESSION_IMAGE_MAGIC: [u8; 8] = *b"LUMQ8S1\0";
const SESSION_IMAGE_VERSION: u32 = 1;
const SESSION_IMAGE_HEADER_BYTES: usize = 72;

struct ResidentBuildClaim {
    state: &'static AtomicU8,
    published: bool,
}

impl ResidentBuildClaim {
    const fn new(state: &'static AtomicU8) -> Self {
        Self {
            state,
            published: false,
        }
    }

    fn publish_ready(mut self) {
        self.state.store(RESIDENT_READY, Ordering::Release);
        self.published = true;
    }
}

impl Drop for ResidentBuildClaim {
    fn drop(&mut self) {
        if !self.published {
            let _ = self.state.compare_exchange(
                RESIDENT_BUILDING,
                RESIDENT_COLD,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}

#[derive(Debug)]
pub enum HybridCpuBackendError {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Model(lfm25_model::Error),
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    F32(lfm25_f32::Error),
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Kernel(cpu::Error),
    Tensor,
    TensorDomain,
    State,
    SessionImage,
    Allocation,
    ModelHash {
        #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
        observed: [u8; 32],
        #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
        expected: [u8; 32],
    },
}

impl From<lfm25_model::Error> for HybridCpuBackendError {
    fn from(error: lfm25_model::Error) -> Self {
        Self::Model(error)
    }
}

impl From<lfm25_f32::Error> for HybridCpuBackendError {
    fn from(error: lfm25_f32::Error) -> Self {
        Self::F32(error)
    }
}

impl From<cpu::Error> for HybridCpuBackendError {
    fn from(error: cpu::Error) -> Self {
        Self::Kernel(error)
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Lfm25HybridCpuPerfStats {
    pub(crate) attention_calls: u64,
    pub(crate) attention_positions: u64,
    pub(crate) attention_us: u64,
    pub(crate) projection_calls: u64,
    pub(crate) projection_matrices: u64,
    pub(crate) projection_rows: u64,
    pub(crate) projection_row_ranges: u64,
    pub(crate) projection_pool_dispatched_ranges: u64,
    pub(crate) projection_pool_fallback_ranges: u64,
    pub(crate) projection_prepare_us: u64,
    pub(crate) projection_quantize_us: u64,
    pub(crate) projection_batch_us: u64,
}

impl Lfm25HybridCpuPerfStats {
    pub(crate) const fn delta_since(self, before: Self) -> Self {
        Self {
            attention_calls: self.attention_calls.saturating_sub(before.attention_calls),
            attention_positions: self
                .attention_positions
                .saturating_sub(before.attention_positions),
            attention_us: self.attention_us.saturating_sub(before.attention_us),
            projection_calls: self
                .projection_calls
                .saturating_sub(before.projection_calls),
            projection_matrices: self
                .projection_matrices
                .saturating_sub(before.projection_matrices),
            projection_rows: self.projection_rows.saturating_sub(before.projection_rows),
            projection_row_ranges: self
                .projection_row_ranges
                .saturating_sub(before.projection_row_ranges),
            projection_pool_dispatched_ranges: self
                .projection_pool_dispatched_ranges
                .saturating_sub(before.projection_pool_dispatched_ranges),
            projection_pool_fallback_ranges: self
                .projection_pool_fallback_ranges
                .saturating_sub(before.projection_pool_fallback_ranges),
            projection_prepare_us: self
                .projection_prepare_us
                .saturating_sub(before.projection_prepare_us),
            projection_quantize_us: self
                .projection_quantize_us
                .saturating_sub(before.projection_quantize_us),
            projection_batch_us: self
                .projection_batch_us
                .saturating_sub(before.projection_batch_us),
        }
    }
}

const _: () = {
    let before = Lfm25HybridCpuPerfStats {
        attention_calls: 5,
        attention_positions: 20,
        attention_us: 30,
        projection_calls: 40,
        projection_matrices: 50,
        projection_rows: 60,
        projection_row_ranges: 70,
        projection_pool_dispatched_ranges: 80,
        projection_pool_fallback_ranges: 90,
        projection_prepare_us: 100,
        projection_quantize_us: 110,
        projection_batch_us: 120,
    };
    let after = Lfm25HybridCpuPerfStats {
        attention_calls: 8,
        attention_positions: 27,
        attention_us: 41,
        projection_calls: 53,
        projection_matrices: 65,
        projection_rows: 83,
        projection_row_ranges: 95,
        projection_pool_dispatched_ranges: 107,
        projection_pool_fallback_ranges: 119,
        projection_prepare_us: 131,
        projection_quantize_us: 143,
        projection_batch_us: 157,
    };
    let delta = after.delta_since(before);
    assert!(delta.attention_calls == 3);
    assert!(delta.attention_positions == 7);
    assert!(delta.attention_us == 11);
    assert!(delta.projection_calls == 13);
    assert!(delta.projection_matrices == 15);
    assert!(delta.projection_rows == 23);
    assert!(delta.projection_row_ranges == 25);
    assert!(delta.projection_pool_dispatched_ranges == 27);
    assert!(delta.projection_pool_fallback_ranges == 29);
    assert!(delta.projection_prepare_us == 31);
    assert!(delta.projection_quantize_us == 33);
    assert!(delta.projection_batch_us == 37);
};

struct Lfm25HybridCpuPerfCounters {
    attention_calls: AtomicU64,
    attention_positions: AtomicU64,
    attention_ticks: AtomicU64,
    projection_calls: AtomicU64,
    projection_matrices: AtomicU64,
    projection_rows: AtomicU64,
    projection_row_ranges: AtomicU64,
    projection_pool_dispatched_ranges: AtomicU64,
    projection_pool_fallback_ranges: AtomicU64,
    projection_prepare_ticks: AtomicU64,
    projection_quantize_ticks: AtomicU64,
    projection_batch_ticks: AtomicU64,
}

impl Lfm25HybridCpuPerfCounters {
    const fn new() -> Self {
        Self {
            attention_calls: AtomicU64::new(0),
            attention_positions: AtomicU64::new(0),
            attention_ticks: AtomicU64::new(0),
            projection_calls: AtomicU64::new(0),
            projection_matrices: AtomicU64::new(0),
            projection_rows: AtomicU64::new(0),
            projection_row_ranges: AtomicU64::new(0),
            projection_pool_dispatched_ranges: AtomicU64::new(0),
            projection_pool_fallback_ranges: AtomicU64::new(0),
            projection_prepare_ticks: AtomicU64::new(0),
            projection_quantize_ticks: AtomicU64::new(0),
            projection_batch_ticks: AtomicU64::new(0),
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    fn snapshot(&self) -> Lfm25HybridCpuPerfStats {
        Lfm25HybridCpuPerfStats {
            attention_calls: self.attention_calls.load(Ordering::Relaxed),
            attention_positions: self.attention_positions.load(Ordering::Relaxed),
            attention_us: ticks_to_us(self.attention_ticks.load(Ordering::Relaxed)),
            projection_calls: self.projection_calls.load(Ordering::Relaxed),
            projection_matrices: self.projection_matrices.load(Ordering::Relaxed),
            projection_rows: self.projection_rows.load(Ordering::Relaxed),
            projection_row_ranges: self.projection_row_ranges.load(Ordering::Relaxed),
            projection_pool_dispatched_ranges: self
                .projection_pool_dispatched_ranges
                .load(Ordering::Relaxed),
            projection_pool_fallback_ranges: self
                .projection_pool_fallback_ranges
                .load(Ordering::Relaxed),
            projection_prepare_us: ticks_to_us(
                self.projection_prepare_ticks.load(Ordering::Relaxed),
            ),
            projection_quantize_us: ticks_to_us(
                self.projection_quantize_ticks.load(Ordering::Relaxed),
            ),
            projection_batch_us: ticks_to_us(self.projection_batch_ticks.load(Ordering::Relaxed)),
        }
    }

    fn record_attention(&self, positions: usize, elapsed_ticks: u64) {
        self.attention_positions
            .fetch_add(positions as u64, Ordering::Relaxed);
        self.attention_ticks
            .fetch_add(elapsed_ticks, Ordering::Relaxed);
        self.attention_calls.fetch_add(1, Ordering::Relaxed);
    }

    fn record_projection(
        &self,
        matrices: usize,
        rows: usize,
        row_ranges: usize,
        pool_dispatched_ranges: usize,
        pool_fallback_ranges: usize,
        prepare_ticks: u64,
        quantize_ticks: u64,
        batch_ticks: u64,
    ) {
        self.projection_matrices
            .fetch_add(matrices as u64, Ordering::Relaxed);
        self.projection_rows
            .fetch_add(rows as u64, Ordering::Relaxed);
        self.projection_row_ranges
            .fetch_add(row_ranges as u64, Ordering::Relaxed);
        self.projection_pool_dispatched_ranges
            .fetch_add(pool_dispatched_ranges as u64, Ordering::Relaxed);
        self.projection_pool_fallback_ranges
            .fetch_add(pool_fallback_ranges as u64, Ordering::Relaxed);
        self.projection_prepare_ticks
            .fetch_add(prepare_ticks, Ordering::Relaxed);
        self.projection_quantize_ticks
            .fetch_add(quantize_ticks, Ordering::Relaxed);
        self.projection_batch_ticks
            .fetch_add(batch_ticks, Ordering::Relaxed);
        self.projection_calls.fetch_add(1, Ordering::Relaxed);
    }
}

static LFM25_HYBRID_CPU_PERF: Lfm25HybridCpuPerfCounters = Lfm25HybridCpuPerfCounters::new();

fn elapsed_ticks_since(start_tick: u64) -> u64 {
    embassy_time_driver::now().saturating_sub(start_tick)
}

/// Bound a projection's row fan-out by its actual multiply-accumulate work,
/// rather than only its number of output rows. This admits both FFN-down
/// (1024×4608) and gate/up (4608×1024) to the same four-way lowering while
/// retaining a one-shard minimum for smaller matrices.
fn cpu_vnni_row_worker_cap(rows: usize, columns: usize, pool_cap: usize) -> Option<usize> {
    if rows == 0 || columns == 0 {
        return None;
    }
    let values = rows.checked_mul(columns)?;
    let work_cap = (values / CPU_VNNI_VALUES_PER_SHARD).max(1);
    let tile_cap = rows.div_ceil(cpu::Q8_VNNI_ROWS_PER_TILE);
    Some(pool_cap.min(work_cap).min(tile_cap))
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn ticks_to_us(ticks: u64) -> u64 {
    let elapsed_us =
        (ticks as u128).saturating_mul(1_000_000) / embassy_time_driver::TICK_HZ.max(1) as u128;
    core::cmp::min(elapsed_us, u64::MAX as u128) as u64
}

fn image_u16(image: &[u8], offset: usize) -> Result<u16, HybridCpuBackendError> {
    let bytes = image
        .get(offset..offset.saturating_add(2))
        .ok_or(HybridCpuBackendError::SessionImage)?
        .try_into()
        .map_err(|_| HybridCpuBackendError::SessionImage)?;
    Ok(u16::from_le_bytes(bytes))
}

fn image_u32(image: &[u8], offset: usize) -> Result<u32, HybridCpuBackendError> {
    let bytes = image
        .get(offset..offset.saturating_add(4))
        .ok_or(HybridCpuBackendError::SessionImage)?
        .try_into()
        .map_err(|_| HybridCpuBackendError::SessionImage)?;
    Ok(u32::from_le_bytes(bytes))
}

fn image_u64(image: &[u8], offset: usize) -> Result<u64, HybridCpuBackendError> {
    let bytes = image
        .get(offset..offset.saturating_add(8))
        .ok_or(HybridCpuBackendError::SessionImage)?
        .try_into()
        .map_err(|_| HybridCpuBackendError::SessionImage)?;
    Ok(u64::from_le_bytes(bytes))
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn lfm25_hybrid_cpu_perf_snapshot() -> Lfm25HybridCpuPerfStats {
    LFM25_HYBRID_CPU_PERF.snapshot()
}

/// Kernel-owned row-pool admission and P/E-policy telemetry for Lumen's CPU
/// lowering.  Session concurrency is intentionally not represented here.
pub(crate) fn lfm25_cpu_vnni_row_pool_snapshot() -> CpuTaskPoolSnapshot {
    CPU_VNNI_ROW_POOL.snapshot()
}

struct CpuQ8Tensor {
    /// Preserve the normalized F32 result for CPU stages. Each admitted CPU
    /// VNNI projection quantizes this exact vector into the fixed Q8_0 input ABI.
    values: Vec<f32>,
}

enum CpuTensor {
    Q30(Vec<f32>),
    Q8(CpuQ8Tensor),
}

#[derive(Default)]
struct KvCache {
    keys: Vec<u16>,
    values: Vec<u16>,
}

struct CpuVnniResidentModel {
    model_storage: Vec<u8>,
    model_offset: usize,
}

/// Result ownership for one lowered projection.  Shards return independent
/// vectors and are copied into the caller-owned output only after their join,
/// so a partial AP admission never races the local fallback.
struct CpuVnniLoweredProjection {
    rows: usize,
    ranges: usize,
    dispatched_ranges: usize,
    fallback_ranges: usize,
}

fn resident_q8_matrix(
    model: &CpuVnniResidentModel,
    descriptor: NativeTensorDescriptor,
) -> Result<&[u8], cpu::Error> {
    let model_end = model
        .model_offset
        .checked_add(lfm25::PINNED_NATIVE_IMAGE_BYTES as usize)
        .ok_or(cpu::Error::Shape)?;
    let model = model
        .model_storage
        .get(model.model_offset..model_end)
        .ok_or(cpu::Error::Shape)?;
    let start = descriptor.native_offset as usize;
    let end = start
        .checked_add(descriptor.native_bytes as usize)
        .ok_or(cpu::Error::Shape)?;
    model.get(start..end).ok_or(cpu::Error::Shape)
}

fn zeroed_projection_rows(rows: usize) -> Result<Vec<f32>, cpu::Error> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(rows)
        .map_err(|_| cpu::Error::Allocation)?;
    output.resize(rows, 0.0);
    Ok(output)
}

fn project_q8_row_range(
    projector: cpu::Q8VnniProjector,
    model: &CpuVnniResidentModel,
    descriptor: NativeTensorDescriptor,
    activation: &cpu::Q8VnniActivation,
    range: cpu::Q8VnniRowRange,
) -> Result<Vec<f32>, cpu::Error> {
    let matrix = resident_q8_matrix(model, descriptor)?;
    let mut output = zeroed_projection_rows(range.row_count())?;
    projector.project_rows(
        matrix,
        descriptor.ggml_ne1 as usize,
        descriptor.ggml_ne0 as usize,
        activation,
        range.first_row(),
        &mut output,
    )?;
    Ok(output)
}

struct CpuVnniResidentAssets {
    model: Arc<CpuVnniResidentModel>,
    f32: Arc<cpu::F32Sidecar>,
}

static RESIDENT_MODEL_STATE: AtomicU8 = AtomicU8::new(RESIDENT_COLD);
static RESIDENT_F32_STATE: AtomicU8 = AtomicU8::new(RESIDENT_COLD);
static RESIDENT_MODEL: Mutex<Option<Arc<CpuVnniResidentModel>>> = Mutex::new(None);
static RESIDENT_F32: Mutex<Option<Arc<cpu::F32Sidecar>>> = Mutex::new(None);
static RESIDENT_ASSETS: Mutex<Option<Arc<CpuVnniResidentAssets>>> = Mutex::new(None);

pub struct HybridCpuAotDecodeBackend {
    assets: Arc<CpuVnniResidentAssets>,
    projector: cpu::Q8VnniProjector,
    slots: Vec<Option<CpuTensor>>,
    shortconv: Vec<Vec<[f32; 2]>>,
    kv: Vec<KvCache>,
    callback_sequence: u64,
}

pub type CpuVnniAotDecodeBackend = HybridCpuAotDecodeBackend;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25BackendCheckpoint {
    pub(crate) position: u32,
    pub(crate) callback_sequence: u64,
}

async fn load_resident_model() -> Result<CpuVnniResidentModel, HybridCpuBackendError> {
    let image = lfm25_model::open().await?;
    let bytes = usize::try_from(image.len()).map_err(|_| HybridCpuBackendError::Allocation)?;
    let allocation_bytes = bytes
        .checked_add(MODEL_ALIGNMENT - 1)
        .ok_or(HybridCpuBackendError::Allocation)?;
    let mut model_storage = Vec::new();
    model_storage
        .try_reserve_exact(allocation_bytes)
        .map_err(|_| HybridCpuBackendError::Allocation)?;
    model_storage.resize(allocation_bytes, 0);
    let misalignment = model_storage.as_ptr() as usize % MODEL_ALIGNMENT;
    let model_offset = (MODEL_ALIGNMENT - misalignment) % MODEL_ALIGNMENT;
    let model_end = model_offset
        .checked_add(bytes)
        .ok_or(HybridCpuBackendError::Allocation)?;
    let model = model_storage
        .get_mut(model_offset..model_end)
        .ok_or(HybridCpuBackendError::Allocation)?;
    let mut hasher = Sha256::new();
    let mut offset = 0usize;
    while offset < model.len() {
        let end = core::cmp::min(offset + MODEL_READ_CHUNK, model.len());
        image
            .read_exact_at(offset as u64, &mut model[offset..end])
            .await?;
        hasher.update(&model[offset..end]);
        offset = end;
        Timer::after(Duration::from_millis(1)).await;
    }
    let observed: [u8; 32] = hasher.finalize().into();
    if observed != lfm25_model::NATIVE_IMAGE_SHA256 {
        return Err(HybridCpuBackendError::ModelHash {
            observed,
            expected: lfm25_model::NATIVE_IMAGE_SHA256,
        });
    }

    // Admit the signed-magnitude transform once. The hot loop can then use
    // VPSIGNB without checking every sealed weight byte on every token.
    let mut q8_tensors = 0usize;
    let mut q8_values = 0u64;
    let mut q8_bytes = 0u64;
    for descriptor in lfm25::generated::TENSORS
        .iter()
        .copied()
        .filter(|descriptor| TensorFormat::from_raw(descriptor.format) == Some(TensorFormat::Q8_0))
    {
        let start = descriptor.native_offset as usize;
        let end = start
            .checked_add(descriptor.native_bytes as usize)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let tensor = model.get(start..end).ok_or(HybridCpuBackendError::Tensor)?;
        cpu::validate_q8_vnni_matrix(
            tensor,
            descriptor.ggml_ne1 as usize,
            descriptor.ggml_ne0 as usize,
        )?;
        q8_tensors = q8_tensors
            .checked_add(1)
            .ok_or(HybridCpuBackendError::Tensor)?;
        q8_values = q8_values
            .checked_add(u64::from(descriptor.ggml_ne0) * u64::from(descriptor.ggml_ne1))
            .ok_or(HybridCpuBackendError::Tensor)?;
        q8_bytes = q8_bytes
            .checked_add(u64::from(descriptor.native_bytes))
            .ok_or(HybridCpuBackendError::Tensor)?;
    }
    if q8_tensors != cpu::LFM25_Q8_PROJECTION_TENSOR_COUNT
        || q8_values != cpu::LFM25_Q8_PROJECTION_QUANTIZED_VALUES
        || q8_bytes != cpu::LFM25_Q8_WEIGHT_BYTES_PER_TOKEN
    {
        return Err(HybridCpuBackendError::Tensor);
    }

    crate::log_info!(
        target: "r";
        "lfm25: native model ready weight_layout=q8_0-row34 backend=cpu-vnni image_bytes={} q8_bytes={} tensors={} blocks={} quantized_values={} model_mutation=none\n",
        model.len(),
        q8_bytes,
        q8_tensors,
        cpu::LFM25_Q8_PROJECTION_BLOCKS,
        q8_values,
    );
    Ok(CpuVnniResidentModel {
        model_storage,
        model_offset,
    })
}

async fn resident_model() -> Result<Arc<CpuVnniResidentModel>, HybridCpuBackendError> {
    loop {
        if RESIDENT_MODEL_STATE.load(Ordering::Acquire) == RESIDENT_READY {
            if let Some(model) = RESIDENT_MODEL.lock().clone() {
                return Ok(model);
            }
            RESIDENT_MODEL_STATE.store(RESIDENT_COLD, Ordering::Release);
        }
        if RESIDENT_MODEL_STATE
            .compare_exchange(RESIDENT_COLD, RESIDENT_BUILDING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let claim = ResidentBuildClaim::new(&RESIDENT_MODEL_STATE);
            match load_resident_model().await {
                Ok(model) => {
                    let model = Arc::new(model);
                    *RESIDENT_MODEL.lock() = Some(model.clone());
                    claim.publish_ready();
                    let _ = publish_resident_assets_if_complete();
                    return Ok(model);
                }
                Err(error) => return Err(error),
            }
        }
        Timer::after(Duration::from_millis(RESIDENT_WAIT_MS)).await;
    }
}

async fn resident_f32() -> Result<Arc<cpu::F32Sidecar>, HybridCpuBackendError> {
    loop {
        if RESIDENT_F32_STATE.load(Ordering::Acquire) == RESIDENT_READY {
            if let Some(f32) = RESIDENT_F32.lock().clone() {
                return Ok(f32);
            }
            RESIDENT_F32_STATE.store(RESIDENT_COLD, Ordering::Release);
        }
        if RESIDENT_F32_STATE
            .compare_exchange(RESIDENT_COLD, RESIDENT_BUILDING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let claim = ResidentBuildClaim::new(&RESIDENT_F32_STATE);
            match lfm25_f32::load().await {
                Ok(f32) => {
                    let f32 = Arc::new(f32);
                    *RESIDENT_F32.lock() = Some(f32.clone());
                    claim.publish_ready();
                    let _ = publish_resident_assets_if_complete();
                    return Ok(f32);
                }
                Err(error) => return Err(error.into()),
            }
        }
        Timer::after(Duration::from_millis(RESIDENT_WAIT_MS)).await;
    }
}

fn publish_resident_assets_if_complete() -> Option<Arc<CpuVnniResidentAssets>> {
    if let Some(assets) = RESIDENT_ASSETS.lock().clone() {
        return Some(assets);
    }
    let model = RESIDENT_MODEL.lock().clone()?;
    let f32 = RESIDENT_F32.lock().clone()?;
    let candidate = Arc::new(CpuVnniResidentAssets { model, f32 });
    let mut assets = RESIDENT_ASSETS.lock();
    if let Some(existing) = assets.as_ref() {
        return Some(existing.clone());
    }
    *assets = Some(candidate.clone());
    Some(candidate)
}

async fn resident_assets() -> Result<Arc<CpuVnniResidentAssets>, HybridCpuBackendError> {
    if let Some(assets) = RESIDENT_ASSETS.lock().clone() {
        return Ok(assets);
    }
    // The warm fleet prepares these independently. A direct shell open retains
    // the same fail-closed ordering when autostart has not completed yet.
    let _ = resident_f32().await?;
    let _ = resident_model().await?;
    publish_resident_assets_if_complete().ok_or(HybridCpuBackendError::State)
}

pub(crate) async fn warm_cpu_vnni_model() -> Result<(), HybridCpuBackendError> {
    let _ = resident_model().await?;
    Ok(())
}

pub(crate) async fn warm_cpu_vnni_f32() -> Result<(), HybridCpuBackendError> {
    let _ = resident_f32().await?;
    Ok(())
}

pub(crate) fn cpu_vnni_resident_assets_ready() -> bool {
    RESIDENT_ASSETS.lock().is_some()
}

pub async fn open_cpu_vnni_backend() -> Result<CpuVnniAotDecodeBackend, HybridCpuBackendError> {
    let projector = cpu::Q8VnniProjector::detect()?;
    // Immutable native model/F32 assets remain boot-resident. Every
    // conversation gets fresh short-convolution, K/V, tensor-slot and callback
    // state. The projector is admitted on this worker; immutable model
    // admission remains shared through the resident asset.
    let assets = resident_assets().await?;
    let row_pool = lfm25_cpu_vnni_row_pool_snapshot();
    crate::log_info!(
        target: "r";
        "lfm25: cpu-vnni lowering row_pool_policy={} configured_cap={} runtime_cap={} values_per_shard={} eligible_workers={} pcore_workers={} ecore_workers={} active={} session_pool_is_not_row_fanout=1\n",
        row_pool.policy.label(),
        row_pool.configured_cap,
        row_pool.runtime_cap,
        CPU_VNNI_VALUES_PER_SHARD,
        row_pool.worker.eligible,
        row_pool.worker.performance,
        row_pool.worker.efficiency,
        row_pool.active,
    );
    let mut shortconv = Vec::new();
    shortconv
        .try_reserve_exact(trueos_lfm25_model::lfm25_decode::SHORTCONV_STATE_COUNT)
        .map_err(|_| HybridCpuBackendError::Allocation)?;
    for _ in 0..trueos_lfm25_model::lfm25_decode::SHORTCONV_STATE_COUNT {
        shortconv.push(vec![[0.0; 2]; HIDDEN]);
    }
    let mut kv = Vec::new();
    kv.try_reserve_exact(trueos_lfm25_model::lfm25_decode::KV_CACHE_COUNT)
        .map_err(|_| HybridCpuBackendError::Allocation)?;
    for _ in 0..trueos_lfm25_model::lfm25_decode::KV_CACHE_COUNT {
        kv.push(KvCache::default());
    }

    Ok(HybridCpuAotDecodeBackend {
        assets,
        projector,
        slots: Vec::new(),
        shortconv,
        kv,
        callback_sequence: 0,
    })
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub async fn open_hybrid_backend() -> Result<HybridCpuAotDecodeBackend, HybridCpuBackendError> {
    open_cpu_vnni_backend().await
}

impl HybridCpuAotDecodeBackend {
    /// Serialize only mutable logical inference state.
    ///
    /// Immutable model/F32 assets and transient tensor slots deliberately
    /// remain kernel capabilities and are reacquired when this image is
    /// restored.
    pub(crate) fn checkpoint_state(
        &self,
        position: u32,
        callback_sequence: u64,
    ) -> Result<Vec<u8>, HybridCpuBackendError> {
        let positions =
            usize::try_from(position).map_err(|_| HybridCpuBackendError::SessionImage)?;
        if position > lfm25::MODEL_INITIAL_CONTEXT
            || callback_sequence != self.callback_sequence
            || self.shortconv.len() != trueos_lfm25_model::lfm25_decode::SHORTCONV_STATE_COUNT
            || self.kv.len() != trueos_lfm25_model::lfm25_decode::KV_CACHE_COUNT
            || self.slots.iter().any(Option::is_some)
        {
            return Err(HybridCpuBackendError::SessionImage);
        }
        let expected_kv_values = positions
            .checked_mul(KV_ELEMENTS)
            .ok_or(HybridCpuBackendError::SessionImage)?;
        if self.shortconv.iter().any(|state| state.len() != HIDDEN)
            || self.kv.iter().any(|cache| {
                cache.keys.len() != expected_kv_values || cache.values.len() != expected_kv_values
            })
        {
            return Err(HybridCpuBackendError::SessionImage);
        }
        let shortconv_bytes = self
            .shortconv
            .len()
            .checked_mul(HIDDEN)
            .and_then(|values| values.checked_mul(2))
            .and_then(|values| values.checked_mul(core::mem::size_of::<f32>()))
            .ok_or(HybridCpuBackendError::SessionImage)?;
        let kv_bytes = self
            .kv
            .len()
            .checked_mul(expected_kv_values)
            .and_then(|values| values.checked_mul(2))
            .and_then(|values| values.checked_mul(core::mem::size_of::<u16>()))
            .ok_or(HybridCpuBackendError::SessionImage)?;
        let total = SESSION_IMAGE_HEADER_BYTES
            .checked_add(shortconv_bytes)
            .and_then(|bytes| bytes.checked_add(kv_bytes))
            .ok_or(HybridCpuBackendError::SessionImage)?;
        let mut image = Vec::new();
        image
            .try_reserve_exact(total)
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        image.extend_from_slice(&SESSION_IMAGE_MAGIC);
        image.extend_from_slice(&SESSION_IMAGE_VERSION.to_le_bytes());
        image.extend_from_slice(&position.to_le_bytes());
        image.extend_from_slice(&callback_sequence.to_le_bytes());
        image.extend_from_slice(&(self.shortconv.len() as u32).to_le_bytes());
        image.extend_from_slice(&(HIDDEN as u32).to_le_bytes());
        image.extend_from_slice(&(self.kv.len() as u32).to_le_bytes());
        image.extend_from_slice(&(KV_ELEMENTS as u32).to_le_bytes());
        image.extend_from_slice(&lfm25_model::NATIVE_IMAGE_SHA256);
        for state in &self.shortconv {
            for channel in state {
                image.extend_from_slice(&channel[0].to_bits().to_le_bytes());
                image.extend_from_slice(&channel[1].to_bits().to_le_bytes());
            }
        }
        for cache in &self.kv {
            for value in &cache.keys {
                image.extend_from_slice(&value.to_le_bytes());
            }
            for value in &cache.values {
                image.extend_from_slice(&value.to_le_bytes());
            }
        }
        if image.len() != total {
            return Err(HybridCpuBackendError::SessionImage);
        }
        Ok(image)
    }

    /// Replace fresh mutable state with one validated portable session image.
    pub(crate) fn restore_state(
        &mut self,
        image: &[u8],
    ) -> Result<Lfm25BackendCheckpoint, HybridCpuBackendError> {
        if image.len() < SESSION_IMAGE_HEADER_BYTES
            || image.get(..8) != Some(SESSION_IMAGE_MAGIC.as_slice())
        {
            return Err(HybridCpuBackendError::SessionImage);
        }
        let version = image_u32(image, 8)?;
        let position = image_u32(image, 12)?;
        let callback_sequence = image_u64(image, 16)?;
        let shortconv_count = image_u32(image, 24)? as usize;
        let hidden = image_u32(image, 28)? as usize;
        let kv_count = image_u32(image, 32)? as usize;
        let kv_elements = image_u32(image, 36)? as usize;
        let native_model_sha256 = image
            .get(40..72)
            .ok_or(HybridCpuBackendError::SessionImage)?;
        if version != SESSION_IMAGE_VERSION
            || position > lfm25::MODEL_INITIAL_CONTEXT
            || shortconv_count != trueos_lfm25_model::lfm25_decode::SHORTCONV_STATE_COUNT
            || hidden != HIDDEN
            || kv_count != trueos_lfm25_model::lfm25_decode::KV_CACHE_COUNT
            || kv_elements != KV_ELEMENTS
            || native_model_sha256 != lfm25_model::NATIVE_IMAGE_SHA256
        {
            return Err(HybridCpuBackendError::SessionImage);
        }
        let positions = position as usize;
        let expected_kv_values = positions
            .checked_mul(KV_ELEMENTS)
            .ok_or(HybridCpuBackendError::SessionImage)?;
        let shortconv_bytes = shortconv_count
            .checked_mul(HIDDEN)
            .and_then(|values| values.checked_mul(2 * core::mem::size_of::<f32>()))
            .ok_or(HybridCpuBackendError::SessionImage)?;
        let kv_bytes = kv_count
            .checked_mul(expected_kv_values)
            .and_then(|values| values.checked_mul(2 * core::mem::size_of::<u16>()))
            .ok_or(HybridCpuBackendError::SessionImage)?;
        let expected = SESSION_IMAGE_HEADER_BYTES
            .checked_add(shortconv_bytes)
            .and_then(|bytes| bytes.checked_add(kv_bytes))
            .ok_or(HybridCpuBackendError::SessionImage)?;
        if image.len() != expected {
            return Err(HybridCpuBackendError::SessionImage);
        }

        let mut cursor = SESSION_IMAGE_HEADER_BYTES;
        for state in &mut self.shortconv {
            for channel in state {
                channel[0] = f32::from_bits(image_u32(image, cursor)?);
                cursor += 4;
                channel[1] = f32::from_bits(image_u32(image, cursor)?);
                cursor += 4;
            }
        }
        for cache in &mut self.kv {
            cache.keys.clear();
            cache.values.clear();
            cache
                .keys
                .try_reserve_exact(expected_kv_values)
                .map_err(|_| HybridCpuBackendError::Allocation)?;
            cache
                .values
                .try_reserve_exact(expected_kv_values)
                .map_err(|_| HybridCpuBackendError::Allocation)?;
            for _ in 0..expected_kv_values {
                cache.keys.push(image_u16(image, cursor)?);
                cursor += 2;
            }
            for _ in 0..expected_kv_values {
                cache.values.push(image_u16(image, cursor)?);
                cursor += 2;
            }
        }
        if cursor != image.len() {
            return Err(HybridCpuBackendError::SessionImage);
        }
        self.slots.clear();
        self.callback_sequence = callback_sequence;
        Ok(Lfm25BackendCheckpoint {
            position,
            callback_sequence,
        })
    }

    fn descriptor(
        layer: Option<u8>,
        role: TensorRole,
    ) -> Result<NativeTensorDescriptor, HybridCpuBackendError> {
        let layer = layer.unwrap_or(0xff);
        lfm25::generated::TENSORS
            .iter()
            .copied()
            .find(|descriptor| descriptor.layer == layer && descriptor.role == role as u8)
            .ok_or(HybridCpuBackendError::Tensor)
    }

    fn tensor(&self, descriptor: NativeTensorDescriptor) -> Result<&[u8], HybridCpuBackendError> {
        resident_q8_matrix(&self.assets.model, descriptor)
            .map_err(|_| HybridCpuBackendError::Tensor)
    }

    fn f32_tensor(
        &self,
        descriptor: NativeTensorDescriptor,
    ) -> Result<Vec<f32>, HybridCpuBackendError> {
        if TensorFormat::from_raw(descriptor.format) != Some(TensorFormat::Bf16Le) {
            return Err(HybridCpuBackendError::Tensor);
        }
        Ok(self.assets.f32.tensor(descriptor.tensor_id)?.to_vec())
    }

    async fn project(
        &self,
        descriptor: NativeTensorDescriptor,
        input: &[f32],
    ) -> Result<Vec<f32>, HybridCpuBackendError> {
        self.project_many(core::slice::from_ref(&descriptor), input)
            .await?
            .pop()
            .ok_or(HybridCpuBackendError::Tensor)
    }

    /// Lower one validated native matrix into contiguous VNNI row shards.
    ///
    /// The pool owns AP admission and P-core policy.  Each closure owns an
    /// immutable model/activation reference plus one private result vector;
    /// joining into `output` happens only on this ordered Lumen lane.  That
    /// gives partial admission a correct local fallback and keeps the fixed
    /// reduction tree bitwise invariant per output row.
    async fn project_lowered(
        &self,
        descriptor: NativeTensorDescriptor,
        activation: Arc<cpu::Q8VnniActivation>,
        output: &mut [f32],
    ) -> Result<CpuVnniLoweredProjection, HybridCpuBackendError> {
        let rows = descriptor.ggml_ne1 as usize;
        if output.len() != rows {
            return Err(HybridCpuBackendError::Tensor);
        }
        let pool = CPU_VNNI_ROW_POOL.snapshot();
        let worker_cap =
            cpu_vnni_row_worker_cap(rows, descriptor.ggml_ne0 as usize, pool.runtime_cap)
                .ok_or(HybridCpuBackendError::Tensor)?;
        if worker_cap <= 1 {
            let plan = cpu::Q8VnniRowPlan::lower(rows, 1)?;
            self.projector.project_plan(
                self.tensor(descriptor)?,
                rows,
                descriptor.ggml_ne0 as usize,
                &activation,
                &plan,
                output,
            )?;
            return Ok(CpuVnniLoweredProjection {
                rows,
                ranges: 1,
                dispatched_ranges: 0,
                fallback_ranges: 1,
            });
        }

        let plan = cpu::Q8VnniRowPlan::lower(rows, worker_cap)?;
        let model = self.assets.model.clone();
        let projector = self.projector;
        let mut completions = Vec::new();
        completions
            .try_reserve_exact(plan.worker_count())
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        let mut local_shards = Vec::new();
        local_shards
            .try_reserve_exact(plan.worker_count())
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        let mut dispatched_ranges = 0usize;
        let mut fallback_ranges = 0usize;

        for range in plan.ranges().iter().copied() {
            let completion = Arc::new(CompletionCell::new());
            let job_model = model.clone();
            let job_activation = activation.clone();
            let job_completion = completion.clone();
            match CPU_VNNI_ROW_POOL.try_dispatch(
                "lfm25-vnni-row",
                Box::new(move |context| {
                    let result = if context.vnni_supported() {
                        project_q8_row_range(
                            projector,
                            &job_model,
                            descriptor,
                            &job_activation,
                            range,
                        )
                    } else {
                        Err(cpu::Error::UnsupportedCpu)
                    };
                    let _ = job_completion.complete(result);
                }),
            ) {
                Ok(_) => {
                    dispatched_ranges += 1;
                    completions.push(Some(completion));
                    local_shards.push(None);
                }
                Err(_) => {
                    // Earlier shards can still be in flight.  Consume this
                    // one locally now, before awaiting them, so the ordered
                    // Lumen lane contributes useful work on partial admission.
                    fallback_ranges += 1;
                    completions.push(None);
                    local_shards.push(Some(project_q8_row_range(
                        projector,
                        &model,
                        descriptor,
                        &activation,
                        range,
                    )?));
                }
            }
        }

        for (index, range) in plan.ranges().iter().copied().enumerate() {
            let shard = match local_shards.get_mut(index).and_then(Option::take) {
                Some(shard) => shard,
                None => match completions.get_mut(index).and_then(Option::take) {
                    Some(completion) => match completion.join().await {
                        Ok(shard) => shard,
                        // A heterogeneous worker may fail the VNNI recheck.
                        // Recompute only its disjoint range on the admitted
                        // Lumen lane; do not make another pool submission.
                        Err(cpu::Error::UnsupportedCpu) => {
                            fallback_ranges += 1;
                            project_q8_row_range(projector, &model, descriptor, &activation, range)?
                        }
                        Err(error) => return Err(error.into()),
                    },
                    None => return Err(HybridCpuBackendError::Tensor),
                },
            };
            let destination = output
                .get_mut(range.first_row()..range.end_row())
                .ok_or(HybridCpuBackendError::Tensor)?;
            if shard.len() != destination.len() {
                return Err(HybridCpuBackendError::Tensor);
            }
            destination.copy_from_slice(&shard);
        }
        Ok(CpuVnniLoweredProjection {
            rows,
            ranges: plan.worker_count(),
            dispatched_ranges,
            fallback_ranges,
        })
    }

    async fn project_many(
        &self,
        descriptors: &[NativeTensorDescriptor],
        input: &[f32],
    ) -> Result<Vec<Vec<f32>>, HybridCpuBackendError> {
        let prepare_started = embassy_time_driver::now();
        if descriptors.is_empty() || descriptors.len() > CPU_VNNI_MAX_BATCH_PROJECTIONS {
            return Err(HybridCpuBackendError::Tensor);
        }
        let row_bytes = cpu::q8_row_bytes(input.len())?;
        let mut outputs = Vec::new();
        outputs
            .try_reserve_exact(descriptors.len())
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        for descriptor in descriptors {
            let rows = descriptor.ggml_ne1 as usize;
            if TensorFormat::from_raw(descriptor.format) != Some(TensorFormat::Q8_0)
                || descriptor.ggml_ne0 as usize != input.len()
                || descriptor.native_bytes as usize
                    != rows
                        .checked_mul(row_bytes)
                        .ok_or(HybridCpuBackendError::Tensor)?
            {
                return Err(HybridCpuBackendError::Tensor);
            }
            outputs.push(vec![0.0f32; rows]);
        }
        let prepare_ticks = elapsed_ticks_since(prepare_started);

        let quantize_started = embassy_time_driver::now();
        let activation = Arc::new(cpu::Q8VnniActivation::quantize(input)?);
        let quantize_ticks = elapsed_ticks_since(quantize_started);

        let batch_started = embassy_time_driver::now();
        let mut lowered_rows = 0usize;
        let mut lowered_ranges = 0usize;
        let mut pool_dispatched_ranges = 0usize;
        let mut pool_fallback_ranges = 0usize;
        for (descriptor, output) in descriptors.iter().zip(&mut outputs) {
            let lowered = self
                .project_lowered(*descriptor, activation.clone(), output.as_mut_slice())
                .await?;
            lowered_rows = lowered_rows
                .checked_add(lowered.rows)
                .ok_or(HybridCpuBackendError::Tensor)?;
            lowered_ranges = lowered_ranges
                .checked_add(lowered.ranges)
                .ok_or(HybridCpuBackendError::Tensor)?;
            pool_dispatched_ranges = pool_dispatched_ranges
                .checked_add(lowered.dispatched_ranges)
                .ok_or(HybridCpuBackendError::Tensor)?;
            pool_fallback_ranges = pool_fallback_ranges
                .checked_add(lowered.fallback_ranges)
                .ok_or(HybridCpuBackendError::Tensor)?;
        }
        let batch_ticks = elapsed_ticks_since(batch_started);
        LFM25_HYBRID_CPU_PERF.record_projection(
            descriptors.len(),
            lowered_rows,
            lowered_ranges,
            pool_dispatched_ranges,
            pool_fallback_ranges,
            prepare_ticks,
            quantize_ticks,
            batch_ticks,
        );
        Ok(outputs)
    }

    fn handle_index(&self, handle: ResidentTensorHandle) -> Result<usize, HybridCpuBackendError> {
        if handle.connection_generation() != CPU_CONNECTION_GENERATION
            || handle.session_epoch() != CPU_SESSION_EPOCH
        {
            return Err(HybridCpuBackendError::TensorDomain);
        }
        let index = handle.storage_slot() as usize;
        if self.slots.get(index).and_then(Option::as_ref).is_none() {
            return Err(HybridCpuBackendError::Tensor);
        }
        Ok(index)
    }

    fn q30_values(&self, tensor: HiddenQ30) -> Result<&[f32], HybridCpuBackendError> {
        match self
            .slots
            .get(self.handle_index(tensor.resident())?)
            .and_then(Option::as_ref)
        {
            Some(CpuTensor::Q30(values)) => Ok(values),
            _ => Err(HybridCpuBackendError::Tensor),
        }
    }

    fn q8_tensor(&self, tensor: HiddenQ8) -> Result<&CpuQ8Tensor, HybridCpuBackendError> {
        match self
            .slots
            .get(self.handle_index(tensor.resident())?)
            .and_then(Option::as_ref)
        {
            Some(CpuTensor::Q8(values)) => Ok(values),
            _ => Err(HybridCpuBackendError::Tensor),
        }
    }

    fn allocate(
        &mut self,
        tensor: CpuTensor,
    ) -> Result<ResidentTensorHandle, HybridCpuBackendError> {
        let index = if let Some(index) = self.slots.iter().position(Option::is_none) {
            self.slots[index] = Some(tensor);
            index
        } else {
            let index = self.slots.len();
            self.slots
                .try_reserve_exact(1)
                .map_err(|_| HybridCpuBackendError::Allocation)?;
            self.slots.push(Some(tensor));
            index
        };
        let slot = u16::try_from(index).map_err(|_| HybridCpuBackendError::Allocation)?;
        Ok(ResidentTensorHandle::new(CPU_CONNECTION_GENERATION, CPU_SESSION_EPOCH, slot))
    }

    fn allocate_q30(&mut self, values: Vec<f32>) -> Result<HiddenQ30, HybridCpuBackendError> {
        if values.len() != HIDDEN {
            return Err(HybridCpuBackendError::Tensor);
        }
        Ok(HiddenQ30::from_resident(self.allocate(CpuTensor::Q30(values))?))
    }

    fn allocate_q8(&mut self, values: Vec<f32>) -> Result<HiddenQ8, HybridCpuBackendError> {
        if values.len() != HIDDEN {
            return Err(HybridCpuBackendError::Tensor);
        }
        Ok(HiddenQ8::from_resident(self.allocate(CpuTensor::Q8(CpuQ8Tensor { values }))?))
    }

    fn release(&mut self, handle: ResidentTensorHandle) -> Result<(), HybridCpuBackendError> {
        let index = self.handle_index(handle)?;
        self.slots[index] = None;
        Ok(())
    }

    fn release_q30(&mut self, tensor: HiddenQ30) -> Result<(), HybridCpuBackendError> {
        self.release(tensor.resident())
    }

    fn release_q8(&mut self, tensor: HiddenQ8) -> Result<(), HybridCpuBackendError> {
        self.release(tensor.resident())
    }

    fn embedding(&mut self, row: EmbeddingRowPlan) -> Result<HiddenQ30, HybridCpuBackendError> {
        let descriptor = Self::descriptor(None, TensorRole::TokenEmbedding)?;
        let row_bytes = cpu::q8_row_bytes(descriptor.ggml_ne0 as usize)?;
        let expected_offset = descriptor
            .native_offset
            .checked_add(
                row.token
                    .checked_mul(row_bytes as u32)
                    .ok_or(HybridCpuBackendError::Tensor)?,
            )
            .ok_or(HybridCpuBackendError::Tensor)?;
        if row.tensor_id != descriptor.tensor_id
            || row.native_offset != expected_offset
            || row.native_bytes as usize != row_bytes
            || row.token >= descriptor.ggml_ne1
            || TensorFormat::from_raw(descriptor.format) != Some(TensorFormat::Q8_0)
            || descriptor.ggml_ne0 as usize != HIDDEN
        {
            return Err(HybridCpuBackendError::Tensor);
        }
        let matrix = self.tensor(descriptor)?;
        let row_start = (row.token as usize)
            .checked_mul(row_bytes)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let row_end = row_start
            .checked_add(row_bytes)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let native_row = matrix
            .get(row_start..row_end)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let mut values = vec![0.0f32; HIDDEN];
        cpu::dequantize_q8_row(native_row, &mut values)?;
        self.allocate_q30(values)
    }

    fn norm(
        &mut self,
        layer: Option<u8>,
        role: TensorRole,
        input: HiddenQ30,
    ) -> Result<HiddenQ8, HybridCpuBackendError> {
        let values = self.q30_values(input)?.to_vec();
        let weights = self.f32_tensor(Self::descriptor(layer, role)?)?;
        self.allocate_q8(cpu::rms_norm(&values, &weights)?)
    }

    async fn shortconv(
        &mut self,
        layer: u8,
        state_slot: LayerStateSlot,
        input: HiddenQ8,
    ) -> Result<HiddenQ30, HybridCpuBackendError> {
        let slot = match state_slot {
            LayerStateSlot::ShortConv(slot) if (slot as usize) < self.shortconv.len() => {
                slot as usize
            }
            _ => return Err(HybridCpuBackendError::State),
        };
        let input_values = self.q8_tensor(input)?.values.clone();
        let projected = self
            .project(Self::descriptor(Some(layer), TensorRole::ShortConvInput)?, &input_values)
            .await?;
        if projected.len() != 3 * HIDDEN {
            return Err(HybridCpuBackendError::Tensor);
        }
        let kernel =
            self.f32_tensor(Self::descriptor(Some(layer), TensorRole::ShortConvKernel)?)?;
        if kernel.len() != 3 * HIDDEN {
            return Err(HybridCpuBackendError::Tensor);
        }
        let (b, remainder) = projected.split_at(HIDDEN);
        let (c, x) = remainder.split_at(HIDDEN);
        let mut mixed = Vec::new();
        mixed
            .try_reserve_exact(HIDDEN)
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        for channel in 0..HIDDEN {
            let state = self.shortconv[slot][channel];
            let kernel_base = channel * 3;
            let (output, oldest, newest) = cpu::shortconv_channel(
                b[channel],
                c[channel],
                x[channel],
                state[0],
                state[1],
                [
                    kernel[kernel_base],
                    kernel[kernel_base + 1],
                    kernel[kernel_base + 2],
                ],
            )?;
            self.shortconv[slot][channel] = [oldest, newest];
            mixed.push(output);
        }
        let output = self
            .project(Self::descriptor(Some(layer), TensorRole::ShortConvOutput)?, &mixed)
            .await?;
        self.release_q8(input)?;
        self.allocate_q30(output)
    }

    async fn attention(
        &mut self,
        layer: u8,
        position: u32,
        state_slot: LayerStateSlot,
        input: HiddenQ8,
    ) -> Result<HiddenQ30, HybridCpuBackendError> {
        let slot = match state_slot {
            LayerStateSlot::KvCache(slot) if (slot as usize) < self.kv.len() => slot as usize,
            _ => return Err(HybridCpuBackendError::State),
        };
        let input_values = self.q8_tensor(input)?.values.clone();
        let descriptors = [
            Self::descriptor(Some(layer), TensorRole::Query)?,
            Self::descriptor(Some(layer), TensorRole::Key)?,
            Self::descriptor(Some(layer), TensorRole::Value)?,
        ];
        let projections = self.project_many(&descriptors, &input_values).await?;
        let attention_cpu_started = embassy_time_driver::now();
        let mut projections = projections.into_iter();
        let mut query = projections.next().ok_or(HybridCpuBackendError::Tensor)?;
        let mut key = projections.next().ok_or(HybridCpuBackendError::Tensor)?;
        let value = projections.next().ok_or(HybridCpuBackendError::Tensor)?;
        if projections.next().is_some() {
            return Err(HybridCpuBackendError::Tensor);
        }
        if query.len() != HEADS * HEAD_DIM || key.len() != KV_ELEMENTS || value.len() != KV_ELEMENTS
        {
            return Err(HybridCpuBackendError::Tensor);
        }
        let query_norm = self.f32_tensor(Self::descriptor(Some(layer), TensorRole::QueryNorm)?)?;
        let key_norm = self.f32_tensor(Self::descriptor(Some(layer), TensorRole::KeyNorm)?)?;
        for head in query.chunks_exact_mut(HEAD_DIM) {
            cpu::rms_norm_head_in_place(head, &query_norm)?;
            cpu::rope_neox_in_place(head, position)?;
        }
        for head in key.chunks_exact_mut(HEAD_DIM) {
            cpu::rms_norm_head_in_place(head, &key_norm)?;
            cpu::rope_neox_in_place(head, position)?;
        }

        let expected_cache = position as usize * KV_ELEMENTS;
        if self.kv[slot].keys.len() != expected_cache
            || self.kv[slot].values.len() != expected_cache
        {
            return Err(HybridCpuBackendError::State);
        }
        self.kv[slot]
            .keys
            .try_reserve_exact(KV_ELEMENTS)
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        self.kv[slot]
            .values
            .try_reserve_exact(KV_ELEMENTS)
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        for value in key {
            self.kv[slot].keys.push(cpu::f16_cache_bits(value)?);
        }
        for value in value {
            self.kv[slot].values.push(cpu::f16_cache_bits(value)?);
        }

        let positions = position as usize + 1;
        let padded_positions = positions
            .checked_add(ATTENTION_REDUCTION_TILE - 1)
            .map(|value| value & !(ATTENTION_REDUCTION_TILE - 1))
            .ok_or(HybridCpuBackendError::State)?;
        let mut context = vec![0.0f32; HIDDEN];
        let scale = 1.0 / libm::sqrtf(HEAD_DIM as f32);
        let mut scores = Vec::new();
        scores
            .try_reserve_exact(positions)
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(positions)
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        let mut query_values = [0.0f32; HEAD_DIM];
        let mut key_values = [0.0f32; HEAD_DIM];
        for query_head in 0..HEADS {
            scores.clear();
            let query_start = query_head * HEAD_DIM;
            for dimension in 0..HEAD_DIM {
                query_values[dimension] =
                    cpu::f16_cache_f32(cpu::f16_cache_bits(query[query_start + dimension])?);
            }
            let kv_head = cpu::gqa_kv_head(query_head, HEADS, KV_HEADS)?;
            for cache_position in 0..positions {
                let key_start = cache_position * KV_ELEMENTS + kv_head * HEAD_DIM;
                for dimension in 0..HEAD_DIM {
                    key_values[dimension] =
                        cpu::f16_cache_f32(self.kv[slot].keys[key_start + dimension]);
                }
                let dot = cpu::f32_dot_pinned(&query_values, &key_values)?;
                scores.push(dot * scale);
            }
            cpu::softmax_in_place(&mut scores)?;
            let output_start = query_head * HEAD_DIM;
            for weight in &mut scores {
                *weight = cpu::f16_cache_f32(cpu::f16_cache_bits(*weight)?);
            }
            for dimension in 0..HEAD_DIM {
                values.clear();
                for cache_position in 0..positions {
                    let value_index = cache_position * KV_ELEMENTS + kv_head * HEAD_DIM + dimension;
                    values.push(cpu::f16_cache_f32(self.kv[slot].values[value_index]));
                }
                context[output_start + dimension] =
                    cpu::f32_dot_pinned_padded(&values, &scores, padded_positions)?;
            }
        }
        let output_descriptor = Self::descriptor(Some(layer), TensorRole::AttentionOutput)?;
        LFM25_HYBRID_CPU_PERF
            .record_attention(positions, elapsed_ticks_since(attention_cpu_started));
        let output = self.project(output_descriptor, &context).await?;
        self.release_q8(input)?;
        self.allocate_q30(output)
    }

    fn residual(
        &mut self,
        residual: HiddenQ30,
        branch: HiddenQ30,
    ) -> Result<HiddenQ30, HybridCpuBackendError> {
        let residual_index = self.handle_index(residual.resident())?;
        let branch_index = self.handle_index(branch.resident())?;
        if residual_index == branch_index {
            return Err(HybridCpuBackendError::Tensor);
        }
        let (residual_slot, branch_slot) = if residual_index < branch_index {
            let (lower, upper) = self.slots.split_at_mut(branch_index);
            (&mut lower[residual_index], &mut upper[0])
        } else {
            let (lower, upper) = self.slots.split_at_mut(residual_index);
            (&mut upper[0], &mut lower[branch_index])
        };
        let Some(CpuTensor::Q30(residual_values)) = residual_slot.as_mut() else {
            return Err(HybridCpuBackendError::Tensor);
        };
        let Some(CpuTensor::Q30(branch_values)) = branch_slot.as_ref() else {
            return Err(HybridCpuBackendError::Tensor);
        };
        if residual_values.len() != HIDDEN || branch_values.len() != HIDDEN {
            return Err(HybridCpuBackendError::Tensor);
        }
        for index in 0..HIDDEN {
            residual_values[index] = residual_values[index] + branch_values[index];
        }
        *branch_slot = None;
        Ok(residual)
    }

    async fn ffn(
        &mut self,
        layer: u8,
        input: HiddenQ8,
    ) -> Result<HiddenQ30, HybridCpuBackendError> {
        let input_values = self.q8_tensor(input)?.values.clone();
        let descriptors = [
            Self::descriptor(Some(layer), TensorRole::FfnGate)?,
            Self::descriptor(Some(layer), TensorRole::FfnUp)?,
        ];
        let mut projections = self
            .project_many(&descriptors, &input_values)
            .await?
            .into_iter();
        let gate = projections.next().ok_or(HybridCpuBackendError::Tensor)?;
        let up = projections.next().ok_or(HybridCpuBackendError::Tensor)?;
        if projections.next().is_some() {
            return Err(HybridCpuBackendError::Tensor);
        }
        if gate.len() != up.len() {
            return Err(HybridCpuBackendError::Tensor);
        }
        let mut activated = Vec::new();
        activated
            .try_reserve_exact(gate.len())
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        for (&gate, &up) in gate.iter().zip(&up) {
            activated.push(cpu::silu_mul_f32_pinned(gate, up)?);
        }
        let values = self
            .project(Self::descriptor(Some(layer), TensorRole::FfnDown)?, &activated)
            .await?;
        self.release_q8(input)?;
        self.allocate_q30(values)
    }

    async fn lm_head(
        &mut self,
        input: HiddenQ8,
        native_offset: u32,
        rows: u32,
        row_bytes: u32,
    ) -> Result<(u32, i64), HybridCpuBackendError> {
        let input_values = self.q8_tensor(input)?.values.clone();
        let descriptor = Self::descriptor(None, TensorRole::TokenEmbedding)?;
        if descriptor.native_offset != native_offset
            || descriptor.ggml_ne1 != rows
            || cpu::q8_row_bytes(input_values.len())? != row_bytes as usize
        {
            return Err(HybridCpuBackendError::Tensor);
        }
        let scores = self.project(descriptor, &input_values).await?;
        let (token, score) = scores
            .iter()
            .copied()
            .enumerate()
            .reduce(|best, candidate| {
                if candidate.1 > best.1 {
                    candidate
                } else {
                    best
                }
            })
            .ok_or(HybridCpuBackendError::Tensor)?;
        self.release_q8(input)?;
        Ok((
            u32::try_from(token).map_err(|_| HybridCpuBackendError::Tensor)?,
            cpu::f32_to_q30(score)?,
        ))
    }

    fn callback(&mut self, operation: DecodeOpKind, output: AotDecodeOutput) -> AotDecodeCallback {
        self.callback_sequence = self.callback_sequence.wrapping_add(1);
        AotDecodeCallback {
            operation,
            callback_sequence: self.callback_sequence,
            output,
        }
    }
}

impl AotDecodeBackend for HybridCpuAotDecodeBackend {
    type Error = HybridCpuBackendError;

    fn capabilities(&self) -> DecodeCapabilities {
        DecodeCapabilities::ALL
    }

    fn max_context_positions(&self) -> u32 {
        lfm25::MODEL_INITIAL_CONTEXT
    }

    async fn submit(
        &mut self,
        request: AotDecodeRequest,
    ) -> Result<AotDecodeCallback, Self::Error> {
        let operation = request.kind();
        let output = match request {
            AotDecodeRequest::TokenEmbedding { row } => {
                AotDecodeOutput::HiddenQ30(self.embedding(row)?)
            }
            AotDecodeRequest::OperatorRmsNorm { layer, input } => AotDecodeOutput::HiddenQ8(
                self.norm(Some(layer), TensorRole::OperatorNorm, input)?,
            ),
            AotDecodeRequest::ShortConv {
                layer,
                position,
                state,
                input,
            } => AotDecodeOutput::StatefulHiddenQ30 {
                output: self.shortconv(layer, state, input).await?,
                state,
                position,
            },
            AotDecodeRequest::Attention {
                layer,
                position,
                state,
                input,
            } => AotDecodeOutput::StatefulHiddenQ30 {
                output: self.attention(layer, position, state, input).await?,
                state,
                position,
            },
            AotDecodeRequest::OperatorResidual {
                residual, branch, ..
            } => AotDecodeOutput::HiddenQ30(self.residual(residual, branch)?),
            AotDecodeRequest::FfnRmsNorm { layer, input } => {
                AotDecodeOutput::HiddenQ8(self.norm(Some(layer), TensorRole::FfnNorm, input)?)
            }
            AotDecodeRequest::Ffn { layer, input } => {
                AotDecodeOutput::HiddenQ30(self.ffn(layer, input).await?)
            }
            AotDecodeRequest::FfnResidual {
                residual, branch, ..
            } => AotDecodeOutput::HiddenQ30(self.residual(residual, branch)?),
            AotDecodeRequest::FinalRmsNorm { input } => {
                AotDecodeOutput::HiddenQ8(self.norm(None, TensorRole::TokenEmbeddingNorm, input)?)
            }
            AotDecodeRequest::TiedLmHeadArgmax { head, input } => {
                let (token, score_q30) = self
                    .lm_head(input, head.native_offset, head.rows, head.row_bytes)
                    .await?;
                AotDecodeOutput::Argmax {
                    token,
                    score_q30,
                    rows: head.rows,
                }
            }
        };
        Ok(self.callback(operation, output))
    }

    fn finish_prefill_token(&mut self, output: HiddenQ30) -> Result<(), Self::Error> {
        self.release_q30(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vnni_row_fanout_is_bounded_by_matrix_work_not_only_rows() {
        const CAP: usize = 16;
        assert_eq!(cpu_vnni_row_worker_cap(1_024, 4_608, CAP), Some(4));
        assert_eq!(cpu_vnni_row_worker_cap(4_608, 1_024, CAP), Some(4));
        assert_eq!(cpu_vnni_row_worker_cap(65_536, 1_024, CAP), Some(CAP));
    }

    #[test]
    fn vnni_row_fanout_retains_one_small_matrix_shard_and_rejects_invalid_shape() {
        assert_eq!(cpu_vnni_row_worker_cap(512, 1_024, 16), Some(1));
        assert_eq!(cpu_vnni_row_worker_cap(0, 1_024, 16), None);
        assert_eq!(cpu_vnni_row_worker_cap(1_024, 0, 16), None);
        assert_eq!(cpu_vnni_row_worker_cap(usize::MAX, 2, 16), None);
    }
}
