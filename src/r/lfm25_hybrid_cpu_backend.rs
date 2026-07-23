//! Hybrid LFM2.5 decode backend.
//!
//! The fixed control plane remains the 99-operation Lumen AOT schedule. CPU
//! kernels execute the stages absent from the admitted firmware, while each
//! FFN is submitted to the already-proven BAR2/MSI TRUEGA function.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use embassy_time::{Duration, Timer};
use sha2::{Digest, Sha256};

use trueos_fpga_abi::lfm25::{self, NativeTensorDescriptor, TensorFormat, TensorRole};
use trueos_fpga_abi::lfm25_decode::{DecodeCapabilities, DecodeOpKind, LayerStateSlot};
use trueos_lfm25_cpu as cpu;

use crate::r::lfm25_decode::{
    AotDecodeBackend, AotDecodeCallback, AotDecodeOutput, AotDecodeRequest, HiddenQ8, HiddenQ30,
    ResidentTensorHandle,
};
use crate::r::{lfm25_f32, lfm25_ffn, lfm25_model};

const HIDDEN: usize = lfm25::MODEL_HIDDEN_SIZE as usize;
const HEADS: usize = lfm25::MODEL_ATTENTION_HEADS as usize;
const KV_HEADS: usize = lfm25::MODEL_KV_HEADS as usize;
const HEAD_DIM: usize = lfm25::MODEL_HEAD_DIMENSION as usize;
const KV_ELEMENTS: usize = KV_HEADS * HEAD_DIM;
const MODEL_READ_CHUNK: usize = 256 * 1024;
const PROJECT_YIELD_ROWS: usize = 128;
const CPU_CONNECTION_GENERATION: u32 = 0x4350_5531; // "CPU1"
const CPU_SESSION_EPOCH: u32 = 1;

#[derive(Debug)]
pub enum HybridCpuBackendError {
    Model(lfm25_model::Error),
    F32(lfm25_f32::Error),
    Ffn(lfm25_ffn::Error),
    Kernel(cpu::Error),
    Tensor,
    TensorDomain,
    State,
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

impl From<lfm25_ffn::Error> for HybridCpuBackendError {
    fn from(error: lfm25_ffn::Error) -> Self {
        Self::Ffn(error)
    }
}

impl From<cpu::Error> for HybridCpuBackendError {
    fn from(error: cpu::Error) -> Self {
        Self::Kernel(error)
    }
}

struct CpuQ8Tensor {
    /// Preserve the F32 result for CPU stages; the Q8 blocks are the exact
    /// representation submitted when the next operation is the FPGA FFN.
    values: Vec<f32>,
    blocks: Vec<[u8; cpu::Q8_BLOCK_BYTES]>,
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

pub struct HybridCpuAotDecodeBackend {
    model: Vec<u8>,
    f32: cpu::F32Sidecar,
    slots: Vec<Option<CpuTensor>>,
    shortconv: Vec<Vec<[f32; 2]>>,
    kv: Vec<KvCache>,
    callback_sequence: u64,
}

pub async fn open_hybrid_backend() -> Result<HybridCpuAotDecodeBackend, HybridCpuBackendError> {
    if !crate::r::fpga_offload::lfm25_row_stream_available() {
        return Err(HybridCpuBackendError::Ffn(lfm25_ffn::Error::StreamUnavailable));
    }
    // This is intentionally loaded before any short-convolution or K/V state
    // is allocated, so a missing or mismatched sidecar cannot partially mutate
    // a decoder session.
    let f32 = lfm25_f32::load().await?;
    let image = lfm25_model::open().await?;
    let bytes = usize::try_from(image.len()).map_err(|_| HybridCpuBackendError::Allocation)?;
    let mut model = Vec::new();
    model
        .try_reserve_exact(bytes)
        .map_err(|_| HybridCpuBackendError::Allocation)?;
    model.resize(bytes, 0);
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

    let mut shortconv = Vec::new();
    shortconv
        .try_reserve_exact(trueos_fpga_abi::lfm25_decode::SHORTCONV_STATE_COUNT)
        .map_err(|_| HybridCpuBackendError::Allocation)?;
    for _ in 0..trueos_fpga_abi::lfm25_decode::SHORTCONV_STATE_COUNT {
        shortconv.push(vec![[0.0; 2]; HIDDEN]);
    }
    let mut kv = Vec::new();
    kv.try_reserve_exact(trueos_fpga_abi::lfm25_decode::KV_CACHE_COUNT)
        .map_err(|_| HybridCpuBackendError::Allocation)?;
    for _ in 0..trueos_fpga_abi::lfm25_decode::KV_CACHE_COUNT {
        kv.push(KvCache::default());
    }

    Ok(HybridCpuAotDecodeBackend {
        model,
        f32,
        slots: Vec::new(),
        shortconv,
        kv,
        callback_sequence: 0,
    })
}

impl HybridCpuAotDecodeBackend {
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
        let start = descriptor.native_offset as usize;
        let end = start
            .checked_add(descriptor.native_bytes as usize)
            .ok_or(HybridCpuBackendError::Tensor)?;
        self.model
            .get(start..end)
            .ok_or(HybridCpuBackendError::Tensor)
    }

    fn f32_tensor(
        &self,
        descriptor: NativeTensorDescriptor,
    ) -> Result<Vec<f32>, HybridCpuBackendError> {
        if TensorFormat::from_raw(descriptor.format) != Some(TensorFormat::Bf16Le) {
            return Err(HybridCpuBackendError::Tensor);
        }
        Ok(self.f32.tensor(descriptor.tensor_id)?.to_vec())
    }

    async fn project(
        &self,
        descriptor: NativeTensorDescriptor,
        input: &[f32],
    ) -> Result<Vec<f32>, HybridCpuBackendError> {
        if TensorFormat::from_raw(descriptor.format) != Some(TensorFormat::Q8_0)
            || descriptor.ggml_ne0 as usize != input.len()
        {
            return Err(HybridCpuBackendError::Tensor);
        }
        let rows = descriptor.ggml_ne1 as usize;
        let row_bytes = cpu::q8_row_bytes(input.len())?;
        let matrix = self.tensor(descriptor)?;
        if matrix.len()
            != rows
                .checked_mul(row_bytes)
                .ok_or(HybridCpuBackendError::Tensor)?
        {
            return Err(HybridCpuBackendError::Tensor);
        }
        let quantized = cpu::quantize_q8(input)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(rows)
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        for (row, bytes) in matrix.chunks_exact(row_bytes).enumerate() {
            output.push(cpu::q8_row_dot_q8(bytes, &quantized)?);
            if (row + 1) % PROJECT_YIELD_ROWS == 0 {
                Timer::after(Duration::from_millis(1)).await;
            }
        }
        Ok(output)
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
        let blocks = cpu::quantize_q8(&values)?;
        Ok(HiddenQ8::from_resident(self.allocate(CpuTensor::Q8(CpuQ8Tensor { values, blocks }))?))
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

    fn embedding(
        &mut self,
        native_offset: u32,
        native_bytes: u32,
    ) -> Result<HiddenQ30, HybridCpuBackendError> {
        let start = native_offset as usize;
        let end = start
            .checked_add(native_bytes as usize)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let row = self
            .model
            .get(start..end)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let mut values = vec![0.0f32; HIDDEN];
        cpu::dequantize_q8_row(row, &mut values)?;
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
        let mut query = self
            .project(Self::descriptor(Some(layer), TensorRole::Query)?, &input_values)
            .await?;
        let mut key = self
            .project(Self::descriptor(Some(layer), TensorRole::Key)?, &input_values)
            .await?;
        let value = self
            .project(Self::descriptor(Some(layer), TensorRole::Value)?, &input_values)
            .await?;
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
        let mut context = vec![0.0f32; HIDDEN];
        let scale = 1.0 / libm::sqrtf(HEAD_DIM as f32);
        let mut scores = Vec::new();
        scores
            .try_reserve_exact(positions)
            .map_err(|_| HybridCpuBackendError::Allocation)?;
        for query_head in 0..HEADS {
            scores.clear();
            let query_start = query_head * HEAD_DIM;
            let query_values = &query[query_start..query_start + HEAD_DIM];
            let kv_head = cpu::gqa_kv_head(query_head, HEADS, KV_HEADS)?;
            for cache_position in 0..positions {
                let key_start = cache_position * KV_ELEMENTS + kv_head * HEAD_DIM;
                let key_values = &self.kv[slot].keys[key_start..key_start + HEAD_DIM];
                let mut dot = 0.0f32;
                for (&query, &key) in query_values.iter().zip(key_values) {
                    dot += query * cpu::f16_cache_f32(key);
                }
                scores.push(dot * scale);
            }
            cpu::softmax_in_place(&mut scores)?;
            let output_start = query_head * HEAD_DIM;
            for dimension in 0..HEAD_DIM {
                let mut sum = 0.0f32;
                for (cache_position, &weight) in scores.iter().enumerate() {
                    let value_index = cache_position * KV_ELEMENTS + kv_head * HEAD_DIM + dimension;
                    sum += weight * cpu::f16_cache_f32(self.kv[slot].values[value_index]);
                }
                context[output_start + dimension] = sum;
            }
        }
        let output = self
            .project(Self::descriptor(Some(layer), TensorRole::AttentionOutput)?, &context)
            .await?;
        self.release_q8(input)?;
        self.allocate_q30(output)
    }

    fn residual(
        &mut self,
        residual: HiddenQ30,
        branch: HiddenQ30,
    ) -> Result<HiddenQ30, HybridCpuBackendError> {
        let residual_values = self.q30_values(residual)?.to_vec();
        let branch_values = self.q30_values(branch)?.to_vec();
        let output = cpu::add(&residual_values, &branch_values)?;
        self.release_q30(residual)?;
        self.release_q30(branch)?;
        self.allocate_q30(output)
    }

    async fn ffn(
        &mut self,
        layer: u8,
        input: HiddenQ8,
    ) -> Result<HiddenQ30, HybridCpuBackendError> {
        let blocks = self.q8_tensor(input)?.blocks.clone();
        let output = lfm25_ffn::execute_layer(layer, blocks, |_| {}).await?;
        let values = cpu::q30_to_f32(&output.output_q30)?;
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
                AotDecodeOutput::HiddenQ30(self.embedding(row.native_offset, row.native_bytes)?)
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
}
