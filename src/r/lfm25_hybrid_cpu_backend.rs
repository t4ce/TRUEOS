//! Fixed LFM2.5 CPU + Intel IGC decode backend.
//!
//! The fixed control plane remains the 99-operation Lumen AOT schedule. CPU
//! kernels execute state, normalization, attention-reduction, and nonlinear
//! stages. Every Q8 projection is submitted to the admitted C++/IGC kernel on
//! the dedicated Intel GuC/RCS lane.

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use embassy_time::{Duration, Instant, Timer};
use sha2::{Digest, Sha256};
use spin::Mutex;

use trueos_lfm25_cpu as cpu;
use trueos_lfm25_model::lfm25::{self, NativeTensorDescriptor, TensorFormat, TensorRole};
use trueos_lfm25_model::lfm25_decode::{
    DecodeCapabilities, DecodeOpKind, EmbeddingRowPlan, LayerStateSlot,
};

use crate::r::lfm25_decode::{
    AotDecodeBackend, AotDecodeCallback, AotDecodeOutput, AotDecodeRequest, HiddenQ8, HiddenQ30,
    ResidentTensorHandle,
};
use crate::r::{lfm25_f32, lfm25_model};

const HIDDEN: usize = lfm25::MODEL_HIDDEN_SIZE as usize;
const HEADS: usize = lfm25::MODEL_ATTENTION_HEADS as usize;
const KV_HEADS: usize = lfm25::MODEL_KV_HEADS as usize;
const HEAD_DIM: usize = lfm25::MODEL_HEAD_DIMENSION as usize;
const KV_ELEMENTS: usize = KV_HEADS * HEAD_DIM;
const ATTENTION_REDUCTION_TILE: usize = 256;
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
    Model(lfm25_model::Error),
    F32(lfm25_f32::Error),
    Gpu(crate::intel::gpgpu::Lfm25Q8ProjectError),
    Kernel(cpu::Error),
    Tensor,
    TensorDomain,
    State,
    SessionImage,
    Allocation,
    ModelHash {
        observed: [u8; 32],
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

impl From<crate::intel::gpgpu::Lfm25Q8ProjectError> for HybridCpuBackendError {
    fn from(error: crate::intel::gpgpu::Lfm25Q8ProjectError) -> Self {
        Self::Gpu(error)
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
        projection_prepare_us: 50,
        projection_quantize_us: 60,
        projection_batch_us: 70,
    };
    let after = Lfm25HybridCpuPerfStats {
        attention_calls: 8,
        attention_positions: 27,
        attention_us: 41,
        projection_calls: 53,
        projection_prepare_us: 67,
        projection_quantize_us: 79,
        projection_batch_us: 93,
    };
    let delta = after.delta_since(before);
    assert!(delta.attention_calls == 3);
    assert!(delta.attention_positions == 7);
    assert!(delta.attention_us == 11);
    assert!(delta.projection_calls == 13);
    assert!(delta.projection_prepare_us == 17);
    assert!(delta.projection_quantize_us == 19);
    assert!(delta.projection_batch_us == 23);
};

struct Lfm25HybridCpuPerfCounters {
    attention_calls: AtomicU64,
    attention_positions: AtomicU64,
    attention_ticks: AtomicU64,
    projection_calls: AtomicU64,
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
            projection_prepare_ticks: AtomicU64::new(0),
            projection_quantize_ticks: AtomicU64::new(0),
            projection_batch_ticks: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> Lfm25HybridCpuPerfStats {
        Lfm25HybridCpuPerfStats {
            attention_calls: self.attention_calls.load(Ordering::Relaxed),
            attention_positions: self.attention_positions.load(Ordering::Relaxed),
            attention_us: ticks_to_us(self.attention_ticks.load(Ordering::Relaxed)),
            projection_calls: self.projection_calls.load(Ordering::Relaxed),
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

    fn record_projection(&self, prepare_ticks: u64, quantize_ticks: u64, batch_ticks: u64) {
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

pub(crate) fn lfm25_hybrid_cpu_perf_snapshot() -> Lfm25HybridCpuPerfStats {
    LFM25_HYBRID_CPU_PERF.snapshot()
}

struct CpuQ8Tensor {
    /// Preserve the normalized F32 result for CPU stages. Each admitted iGPU
    /// projection quantizes this exact vector into the fixed Q8_0 input ABI.
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

struct IntelIgcResidentModel {
    model_storage: Vec<u8>,
    model_offset: usize,
    gpu_model: crate::intel::gpgpu::Lfm25Q8ModelMapping,
}

struct IntelIgcResidentAssets {
    model: Arc<IntelIgcResidentModel>,
    f32: Arc<cpu::F32Sidecar>,
}

static RESIDENT_MODEL_STATE: AtomicU8 = AtomicU8::new(RESIDENT_COLD);
static RESIDENT_F32_STATE: AtomicU8 = AtomicU8::new(RESIDENT_COLD);
static RESIDENT_MODEL: Mutex<Option<Arc<IntelIgcResidentModel>>> = Mutex::new(None);
static RESIDENT_F32: Mutex<Option<Arc<cpu::F32Sidecar>>> = Mutex::new(None);
static RESIDENT_ASSETS: Mutex<Option<Arc<IntelIgcResidentAssets>>> = Mutex::new(None);

pub struct HybridCpuAotDecodeBackend {
    assets: Arc<IntelIgcResidentAssets>,
    slots: Vec<Option<CpuTensor>>,
    shortconv: Vec<Vec<[f32; 2]>>,
    kv: Vec<KvCache>,
    callback_sequence: u64,
}

pub type IntelIgcAotDecodeBackend = HybridCpuAotDecodeBackend;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lfm25BackendCheckpoint {
    pub(crate) position: u32,
    pub(crate) callback_sequence: u64,
}

async fn load_resident_model() -> Result<IntelIgcResidentModel, HybridCpuBackendError> {
    if !crate::intel::gpgpu::lfm25_q8_packed_project_supported() {
        return Err(HybridCpuBackendError::Gpu(
            crate::intel::gpgpu::Lfm25Q8ProjectError::UnsupportedTarget,
        ));
    }
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
    let pack_started_ms = Instant::now().as_millis();
    let packed = cpu::pack_q8x16_model_in_place(model)?;
    let packed_observed: [u8; 32] = Sha256::digest(&*model).into();
    if packed_observed != cpu::PACKED_Q8X16_IMAGE_SHA256 {
        return Err(HybridCpuBackendError::ModelHash {
            observed: packed_observed,
            expected: cpu::PACKED_Q8X16_IMAGE_SHA256,
        });
    }
    crate::log_info!(
        target: "r";
        "lfm25: packed model ready weight_layout=pair1088-x16-dp4a bytes={} tensors={} block_tiles={} quantized_values={} subnormal_scales={} pack_seal_ms={} sha256=90876f02e0cc224fe23e01c8739dcbb94d7bcc8fbfa3d36204c6267a440f5fd8\n",
        model.len(),
        packed.tensor_count,
        packed.block_tiles,
        packed.quantized_values,
        packed.subnormal_scales,
        Instant::now().as_millis().saturating_sub(pack_started_ms),
    );
    let gpu_model = crate::intel::gpgpu::bind_lfm25_q8_packed_model(model)?;
    Ok(IntelIgcResidentModel {
        model_storage,
        model_offset,
        gpu_model,
    })
}

async fn resident_model() -> Result<Arc<IntelIgcResidentModel>, HybridCpuBackendError> {
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

fn publish_resident_assets_if_complete() -> Option<Arc<IntelIgcResidentAssets>> {
    if let Some(assets) = RESIDENT_ASSETS.lock().clone() {
        return Some(assets);
    }
    let model = RESIDENT_MODEL.lock().clone()?;
    let f32 = RESIDENT_F32.lock().clone()?;
    let candidate = Arc::new(IntelIgcResidentAssets { model, f32 });
    let mut assets = RESIDENT_ASSETS.lock();
    if let Some(existing) = assets.as_ref() {
        return Some(existing.clone());
    }
    *assets = Some(candidate.clone());
    Some(candidate)
}

async fn resident_assets() -> Result<Arc<IntelIgcResidentAssets>, HybridCpuBackendError> {
    if let Some(assets) = RESIDENT_ASSETS.lock().clone() {
        return Ok(assets);
    }
    // The warm fleet prepares these independently. A direct shell open retains
    // the same fail-closed ordering when autostart has not completed yet.
    let _ = resident_f32().await?;
    let _ = resident_model().await?;
    publish_resident_assets_if_complete().ok_or(HybridCpuBackendError::State)
}

pub(crate) async fn warm_intel_igc_model() -> Result<(), HybridCpuBackendError> {
    let _ = resident_model().await?;
    Ok(())
}

pub(crate) async fn warm_intel_igc_f32() -> Result<(), HybridCpuBackendError> {
    let _ = resident_f32().await?;
    Ok(())
}

pub(crate) fn intel_igc_resident_assets_ready() -> bool {
    RESIDENT_ASSETS.lock().is_some()
}

pub async fn open_intel_igc_backend() -> Result<IntelIgcAotDecodeBackend, HybridCpuBackendError> {
    if !crate::intel::gpgpu::lfm25_q8_packed_project_supported() {
        return Err(HybridCpuBackendError::Gpu(
            crate::intel::gpgpu::Lfm25Q8ProjectError::UnsupportedTarget,
        ));
    }
    // Immutable model/F32 assets remain boot-resident. Every conversation gets
    // fresh short-convolution, K/V, tensor-slot and callback state.
    let assets = resident_assets().await?;
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
        slots: Vec::new(),
        shortconv,
        kv,
        callback_sequence: 0,
    })
}

pub async fn open_hybrid_backend() -> Result<HybridCpuAotDecodeBackend, HybridCpuBackendError> {
    open_intel_igc_backend().await
}

impl HybridCpuAotDecodeBackend {
    /// Serialize only mutable logical inference state.
    ///
    /// Immutable model/F32 assets, transient tensor slots, GPU mappings and
    /// GuC/RCS ownership deliberately remain kernel capabilities and are
    /// reacquired when this image is restored.
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
        image.extend_from_slice(&cpu::PACKED_Q8X16_IMAGE_SHA256);
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
        let packed_model_sha256 = image
            .get(40..72)
            .ok_or(HybridCpuBackendError::SessionImage)?;
        if version != SESSION_IMAGE_VERSION
            || position > lfm25::MODEL_INITIAL_CONTEXT
            || shortconv_count != trueos_lfm25_model::lfm25_decode::SHORTCONV_STATE_COUNT
            || hidden != HIDDEN
            || kv_count != trueos_lfm25_model::lfm25_decode::KV_CACHE_COUNT
            || kv_elements != KV_ELEMENTS
            || packed_model_sha256 != cpu::PACKED_Q8X16_IMAGE_SHA256
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
        let model_end = self
            .assets
            .model
            .model_offset
            .checked_add(lfm25::PINNED_NATIVE_IMAGE_BYTES as usize)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let model = self
            .assets
            .model
            .model_storage
            .get(self.assets.model.model_offset..model_end)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let start = descriptor.native_offset as usize;
        let end = start
            .checked_add(descriptor.native_bytes as usize)
            .ok_or(HybridCpuBackendError::Tensor)?;
        model.get(start..end).ok_or(HybridCpuBackendError::Tensor)
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

    async fn project_many(
        &self,
        descriptors: &[NativeTensorDescriptor],
        input: &[f32],
    ) -> Result<Vec<Vec<f32>>, HybridCpuBackendError> {
        let prepare_started = embassy_time_driver::now();
        if descriptors.is_empty()
            || descriptors.len() > crate::intel::gpgpu::LFM25_Q8_MAX_BATCH_PROJECTIONS
        {
            return Err(HybridCpuBackendError::Tensor);
        }
        let row_bytes = cpu::q8_row_bytes(input.len())?;
        let mut specs = Vec::new();
        let mut outputs = Vec::new();
        specs
            .try_reserve_exact(descriptors.len())
            .map_err(|_| HybridCpuBackendError::Allocation)?;
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
            specs.push(crate::intel::gpgpu::Lfm25Q8ProjectSpec {
                weight_offset: descriptor.native_offset,
                columns: descriptor.ggml_ne0,
                rows: descriptor.ggml_ne1,
            });
            outputs.push(vec![0.0f32; rows]);
        }
        let mut prepare_ticks = elapsed_ticks_since(prepare_started);

        let quantize_started = embassy_time_driver::now();
        let quantized = cpu::quantize_q8(input)?;
        let quantize_ticks = elapsed_ticks_since(quantize_started);
        let finish_prepare_started = embassy_time_driver::now();
        let activation = unsafe {
            core::slice::from_raw_parts(
                quantized.as_ptr() as *const u8,
                quantized.len() * cpu::Q8_BLOCK_BYTES,
            )
        };
        let mut output_slices: Vec<&mut [f32]> =
            outputs.iter_mut().map(Vec::as_mut_slice).collect();
        prepare_ticks = prepare_ticks.saturating_add(elapsed_ticks_since(finish_prepare_started));
        let batch_started = embassy_time_driver::now();
        let batch_result = crate::intel::gpgpu::lfm25_q8_project_batch(
            self.assets.model.gpu_model,
            &specs,
            activation,
            &mut output_slices,
        );
        let batch_ticks = elapsed_ticks_since(batch_started);
        if batch_result.is_ok() {
            LFM25_HYBRID_CPU_PERF.record_projection(prepare_ticks, quantize_ticks, batch_ticks);
        }
        batch_result?;
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
        let mut values = vec![0.0f32; HIDDEN];
        cpu::dequantize_q8x16_row(
            matrix,
            descriptor.ggml_ne1 as usize,
            descriptor.ggml_ne0 as usize,
            row.token as usize,
            &mut values,
        )?;
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
