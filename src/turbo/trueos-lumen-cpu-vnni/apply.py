#!/usr/bin/env python3
"""Apply the LFM2.5 native-Q8 AVX-VNNI Lumen integration to a TRUEOS tree.

The transformer is intentionally narrow: every edited span is anchored to the
current TRUEOS implementation and fails closed if that implementation drifted.
Run with --check first to print the exact diff without writing files.
"""

from __future__ import annotations

import argparse
import difflib
import subprocess
import sys
from pathlib import Path

EXPECTED_BASES = {
    "02e0e7a8add0fd793d8fd8d5084c0c7e7dc9a3b8",
    "b613742bc74946ae04941f039f613affc5221c38",
    "a926b7135be4b8b6381f4e0cd6b5d1288d8db3b8",
}


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def replace_span(text: str, start: str, end: str, replacement: str, label: str) -> str:
    start_index = text.find(start)
    if start_index < 0:
        raise RuntimeError(f"{label}: start marker not found")
    if text.find(start, start_index + 1) >= 0:
        raise RuntimeError(f"{label}: start marker is not unique")
    end_index = text.find(end, start_index + len(start))
    if end_index < 0:
        raise RuntimeError(f"{label}: end marker not found")
    return text[:start_index] + replacement + text[end_index:]


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as error:
        raise RuntimeError(f"missing TRUEOS file: {path}") from error


def current_commit(repo: Path) -> str | None:
    try:
        result = subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "HEAD"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return result.stdout.strip()


def transform_lfm25_cpu_lib(text: str) -> str:
    text = replace_once(
        text,
        """//! Scalar, deterministic LFM2.5 CPU kernels for TRUEOS's hybrid decoder.
//!
//! This crate deliberately owns numerical primitives only. Model I/O, token
//! scheduling, Intel GPU submission, and cache ownership remain in TRUEOS.""",
        """//! Deterministic LFM2.5 CPU kernels for TRUEOS's fixed decoder.
//!
//! This crate owns numerical primitives, including the native-row AVX-VNNI Q8
//! projection. Model I/O, token scheduling, worker ownership, and cache
//! ownership remain in TRUEOS.""",
        "lfm25-cpu crate documentation",
    )
    text = replace_once(
        text,
        "extern crate alloc;\n\nuse alloc::collections::BTreeMap;",
        """extern crate alloc;

mod cpu_vnni;

pub use cpu_vnni::{
    Q8_VNNI_ROWS_PER_TILE, Q8VnniActivation, Q8VnniCapabilities, Q8VnniProjector,
    validate_q8_vnni_matrix,
};

use alloc::collections::BTreeMap;""",
        "lfm25-cpu module export",
    )
    text = replace_once(
        text,
        "pub const PACKED_Q8X16_SUBNORMAL_SCALES: u64 = 25_994;",
        """pub const PACKED_Q8X16_SUBNORMAL_SCALES: u64 = 25_994;
pub const LFM25_Q8_PROJECTION_TENSOR_COUNT: usize = 93;
pub const LFM25_Q8_PROJECTION_QUANTIZED_VALUES: u64 = 354_418_688;
pub const LFM25_Q8_PROJECTION_BLOCKS: u64 =
    LFM25_Q8_PROJECTION_QUANTIZED_VALUES / Q8_BLOCK_VALUES as u64;
pub const LFM25_Q8_WEIGHT_BYTES_PER_TOKEN: u64 =
    LFM25_Q8_PROJECTION_BLOCKS * Q8_BLOCK_BYTES as u64;

const _: () = assert!(LFM25_Q8_PROJECTION_QUANTIZED_VALUES % Q8_BLOCK_VALUES as u64 == 0);
const _: () = assert!(LFM25_Q8_PROJECTION_BLOCKS == 11_075_584);
const _: () = assert!(LFM25_Q8_WEIGHT_BYTES_PER_TOKEN == 376_569_856);
const _: () = assert!(PACKED_Q8X16_TENSOR_COUNT == LFM25_Q8_PROJECTION_TENSOR_COUNT);
const _: () = assert!(PACKED_Q8X16_QUANTIZED_VALUES == LFM25_Q8_PROJECTION_QUANTIZED_VALUES);""",
        "lfm25-cpu neutral Q8 model constants",
    )
    text = replace_once(
        text,
        """    Vocabulary,
    Allocation,
    NonFinite,
}""",
        """    Vocabulary,
    Allocation,
    UnsupportedCpu,
    NonFinite,
}""",
        "lfm25-cpu unsupported CPU error",
    )
    return text


def transform_backend(text: str) -> str:
    text = replace_once(
        text,
        """//! Fixed LFM2.5 CPU + Intel IGC decode backend.
//!
//! The fixed control plane remains the 99-operation Lumen AOT schedule. CPU
//! kernels execute state, normalization, attention-reduction, and nonlinear
//! stages. Every Q8 projection is submitted to the admitted C++/IGC kernel on
//! the dedicated Intel GuC/RCS lane.""",
        """//! Fixed LFM2.5 CPU + AVX-VNNI decode backend.
//!
//! The fixed control plane remains the 99-operation Lumen AOT schedule. CPU
//! kernels execute state, normalization, attention-reduction, nonlinear stages,
//! and every native-row Q8_0 projection. No graph interpreter, Q8x16 repack, or
//! generic GEMM abstraction is introduced.""",
        "backend module documentation",
    )
    text = replace_once(
        text,
        """    /// Preserve the normalized F32 result for CPU stages. Each admitted iGPU
    /// projection quantizes this exact vector into the fixed Q8_0 input ABI.""",
        """    /// Preserve the normalized F32 result for CPU stages. Each admitted CPU
    /// VNNI projection quantizes this exact vector into the fixed Q8_0 input ABI.""",
        "backend Q8 tensor documentation",
    )
    text = replace_once(
        text,
        "use trueos_time::{Duration, Instant, Timer};",
        "use trueos_time::{Duration, Timer};",
        "backend time imports",
    )
    text = replace_once(
        text,
        "const ATTENTION_REDUCTION_TILE: usize = 256;",
        """const ATTENTION_REDUCTION_TILE: usize = 256;
const CPU_VNNI_MAX_BATCH_PROJECTIONS: usize = 3;""",
        "backend batch constant",
    )
    text = replace_once(
        text,
        """    #[expect(dead_code, reason = \"baseline archived in tools/warnings_last\")]
    Gpu(crate::intel::gpgpu::Lfm25Q8ProjectError),
""",
        "",
        "backend GPU error variant",
    )
    text = replace_once(
        text,
        """impl From<crate::intel::gpgpu::Lfm25Q8ProjectError> for HybridCpuBackendError {
    fn from(error: crate::intel::gpgpu::Lfm25Q8ProjectError) -> Self {
        Self::Gpu(error)
    }
}

""",
        "",
        "backend GPU error conversion",
    )
    text = replace_once(
        text,
        """struct IntelIgcResidentModel {
    model_storage: Vec<u8>,
    model_offset: usize,
    gpu_model: crate::intel::gpgpu::Lfm25Q8ModelMapping,
}

struct IntelIgcResidentAssets {
    model: Arc<IntelIgcResidentModel>,
    f32: Arc<cpu::F32Sidecar>,
}""",
        """struct CpuVnniResidentModel {
    model_storage: Vec<u8>,
    model_offset: usize,
}

struct CpuVnniResidentAssets {
    model: Arc<CpuVnniResidentModel>,
    f32: Arc<cpu::F32Sidecar>,
}""",
        "backend resident model types",
    )
    text = text.replace("IntelIgcResidentModel", "CpuVnniResidentModel")
    text = text.replace("IntelIgcResidentAssets", "CpuVnniResidentAssets")
    text = replace_once(
        text,
        """pub struct HybridCpuAotDecodeBackend {
    assets: Arc<CpuVnniResidentAssets>,
    slots: Vec<Option<CpuTensor>>,""",
        """pub struct HybridCpuAotDecodeBackend {
    assets: Arc<CpuVnniResidentAssets>,
    projector: cpu::Q8VnniProjector,
    slots: Vec<Option<CpuTensor>>,""",
        "backend per-worker projector",
    )
    text = replace_once(
        text,
        "pub type IntelIgcAotDecodeBackend = HybridCpuAotDecodeBackend;",
        "pub type CpuVnniAotDecodeBackend = HybridCpuAotDecodeBackend;",
        "backend public alias",
    )

    load_function = r'''async fn load_resident_model() -> Result<CpuVnniResidentModel, HybridCpuBackendError> {
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
    for descriptor in lfm25::generated::TENSORS.iter().copied().filter(|descriptor| {
        TensorFormat::from_raw(descriptor.format) == Some(TensorFormat::Q8_0)
    }) {
        let start = descriptor.native_offset as usize;
        let end = start
            .checked_add(descriptor.native_bytes as usize)
            .ok_or(HybridCpuBackendError::Tensor)?;
        let tensor = model
            .get(start..end)
            .ok_or(HybridCpuBackendError::Tensor)?;
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
'''
    text = replace_span(
        text,
        "async fn load_resident_model()",
        "\nasync fn resident_model()",
        load_function,
        "backend native model loader",
    )

    open_block = r'''pub(crate) async fn warm_cpu_vnni_model() -> Result<(), HybridCpuBackendError> {
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
'''
    text = replace_span(
        text,
        "pub(crate) async fn warm_intel_igc_model()",
        "\nimpl HybridCpuAotDecodeBackend {",
        open_block,
        "backend warm/open surface",
    )

    text = replace_once(
        text,
        """    /// Immutable model/F32 assets, transient tensor slots, GPU mappings and
    /// GuC/RCS ownership deliberately remain kernel capabilities and are
    /// reacquired when this image is restored.""",
        """    /// Immutable model/F32 assets and transient tensor slots deliberately
    /// remain kernel capabilities and are reacquired when this image is
    /// restored.""",
        "backend checkpoint documentation",
    )
    text = text.replace(
        "cpu::PACKED_Q8X16_IMAGE_SHA256",
        "lfm25_model::NATIVE_IMAGE_SHA256",
    )
    text = text.replace("packed_model_sha256", "native_model_sha256")

    project_many = r'''    async fn project_many(
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
        let activation = cpu::Q8VnniActivation::quantize(input)?;
        let quantize_ticks = elapsed_ticks_since(quantize_started);

        let batch_started = embassy_time_driver::now();
        let projector = self.projector;
        for (descriptor, output) in descriptors.iter().zip(&mut outputs) {
            projector.project(
                self.tensor(*descriptor)?,
                descriptor.ggml_ne1 as usize,
                descriptor.ggml_ne0 as usize,
                &activation,
                output.as_mut_slice(),
            )?;
        }
        let batch_ticks = elapsed_ticks_since(batch_started);
        LFM25_HYBRID_CPU_PERF.record_projection(prepare_ticks, quantize_ticks, batch_ticks);
        Ok(outputs)
    }
'''
    text = replace_span(
        text,
        "    async fn project_many(",
        "\n    fn handle_index(",
        project_many,
        "backend CPU VNNI projection",
    )

    text = replace_once(
        text,
        """        let matrix = self.tensor(descriptor)?;
        let mut values = vec![0.0f32; HIDDEN];
        cpu::dequantize_q8x16_row(
            matrix,
            descriptor.ggml_ne1 as usize,
            descriptor.ggml_ne0 as usize,
            row.token as usize,
            &mut values,
        )?;
        self.allocate_q30(values)""",
        """        let matrix = self.tensor(descriptor)?;
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
        self.allocate_q30(values)""",
        "backend native tied embedding",
    )

    forbidden = [
        "pack_q8x16_model_in_place",
        "lfm25_q8_project_batch",
        "dequantize_q8x16_row",
        "gpu_model",
        "open_intel_igc_backend",
        "warm_intel_igc_model",
    ]
    for token in forbidden:
        if token in text:
            raise RuntimeError(f"backend postcondition failed: {token!r} remains")
    if "crate::intel" in text or "IntelIgc" in text or "intel_igc" in text:
        raise RuntimeError("backend postcondition failed: Intel projection path remains")
    return text


def transform_decode(text: str) -> str:
    text = replace_once(
        text,
        """//! call therefore represents one token on one ordered lane. Production uses
//! scalar CPU state stages and the admitted Intel C++/IGC projection program;
//! no runtime graph is interpreted.""",
        """//! call therefore represents one token on one ordered lane. Production uses
//! scalar CPU state stages and the admitted native-row AVX-VNNI projection
//! kernel; no runtime graph is interpreted.""",
        "Lumen adapter documentation",
    )
    text = replace_once(
        text,
        """/// callback. The generic boundary keeps the scheduler independent of the Intel
/// execution transport.""",
        """/// callback. The generic boundary keeps the scheduler independent of the
/// projection transport.""",
        "Lumen generic backend documentation",
    )
    text = text.replace("open_intel_igc", "open_cpu_vnni")
    text = text.replace("checkpoint_intel_igc", "checkpoint_cpu_vnni")
    text = text.replace("restore_intel_igc", "restore_cpu_vnni")
    text = text.replace("IntelIgcAotDecodeBackend", "CpuVnniAotDecodeBackend")
    text = text.replace("open_intel_igc_backend", "open_cpu_vnni_backend")
    text = replace_once(
        text,
        "/// Bind the sealed scalar CPU stages and the admitted Intel C++/IGC projection\n/// program to the same fixed 99-operation Lumen module.",
        "/// Bind the sealed scalar CPU stages and native-row AVX-VNNI projection\n/// kernel to the same fixed 99-operation Lumen module.",
        "Lumen CPU backend documentation",
    )
    if "intel_igc" in text or "IntelIgc" in text or "Intel C++/IGC" in text:
        raise RuntimeError("Lumen adapter postcondition failed: Intel IGC name remains")
    return text


def transform_lumen_service(text: str) -> str:
    text = replace_once(
        text,
        """//! image in its private memory. The kernel owns immutable model assets and the
//! CPU+IGC+GuC execution lane. No GPU handle, model pointer or host allocation
//! crosses this boundary.""",
        """//! image in its private memory. The kernel owns immutable model assets and the
//! CPU+AVX-VNNI execution lane. No model pointer or host allocation crosses
//! this boundary.""",
        "Lumen service documentation",
    )
    text = text.replace("checkpoint_intel_igc", "checkpoint_cpu_vnni")
    text = text.replace("restore_intel_igc", "restore_cpu_vnni")
    text = text.replace("open_intel_igc", "open_cpu_vnni")
    text = text.replace("IntelIgcAotDecodeBackend", "CpuVnniAotDecodeBackend")
    text = text.replace("action=release-gpu-session", "action=release-cpu-session")
    if "intel_igc" in text or "IntelIgc" in text:
        raise RuntimeError("Lumen service postcondition failed: Intel IGC name remains")
    return text


def transform_boot_warm(text: str) -> str:
    text = replace_once(
        text,
        """//! Only immutable, reusable assets are prepared here. Conversation state,
//! prompt prefill, and GPU submissions remain demand-driven.""",
        """//! Only immutable, reusable assets are prepared here. Conversation state,
//! prompt prefill, and CPU projection work remain demand-driven.""",
        "boot warm documentation",
    )

    admission_and_start = r'''    let projector = match trueos_lfm25_cpu::Q8VnniProjector::detect() {
        Ok(projector) => projector,
        Err(error) => {
            crate::log_warn!(
                target: "service";
                "lfm25: boot-warm stage=deferred accepted=0 executor_slot={} backend=cpu-vnni error={:?} action=leave-cold-for-demand-open\n",
                actual_worker_slot,
                error,
            );
            return;
        }
    };
    let capabilities = projector.capabilities();

    let started = Instant::now();
    crate::log_info!(
        target: "service";
        "lfm25: boot-warm stage=start scope=reusable-assets executor_slot={} core_kind={} settle_ms={} tokenizer_artifact_bytes={} model_artifact_bytes={} f32_artifact_bytes={} backend=cpu-vnni ymm_state={} avx2={} avx_vnni={} fma={} warm_contract=no-project conversation_state=deferred prompt_prefill=deferred\n",
        actual_worker_slot,
        actual_core_kind,
        crate::allcaps::lumen::BOOT_RESIDENT_WARM_SETTLE_MS,
        crate::ai::lfm25_tokenizer::TOKENIZER_BYTES,
        crate::ai::lfm25_model::NATIVE_IMAGE_BYTES,
        trueos_lfm25_cpu::F32_SIDECAR_BYTES,
        capabilities.ymm_state() as u8,
        capabilities.avx2() as u8,
        capabilities.avx_vnni() as u8,
        capabilities.fma() as u8,
    );

'''
    text = replace_span(
        text,
        "    let physical_gpu_ready =",
        "    let tokenizer_started =",
        admission_and_start,
        "boot warm CPU admission",
    )
    text = text.replace("warm_intel_igc_model", "warm_cpu_vnni_model")
    text = text.replace("warm_intel_igc_f32", "warm_cpu_vnni_f32")
    text = text.replace("intel_igc_resident_assets_ready", "cpu_vnni_resident_assets_ready")
    text = replace_once(
        text,
        "lfm25: boot-warm stage=model-ready elapsed_ms={} resident=1 layout=pair1088-x16-dp4a gpu_runtime_mapping=ready warm_contract=no-submit\\n",
        "lfm25: boot-warm stage=model-ready elapsed_ms={} resident=1 layout=q8_0-row34 backend=cpu-vnni model_mutation=none warm_contract=no-project\\n",
        "boot warm model log",
    )

    completion = r'''    let assets_ready = crate::ai::lfm25_hybrid_cpu_backend::cpu_vnni_resident_assets_ready();
    let accepted = tokenizer_ready && model_ready && f32_ready && assets_ready;
    crate::log_info!(
        target: "service";
        "lfm25: boot-warm stage=done accepted={} elapsed_ms={} tokenizer_ready={} model_ready={} f32_ready={} resident_assets_ready={} executor_slot={} backend=cpu-vnni warm_contract=no-project conversation_state=deferred prompt_prefill=deferred first_lum_work=session-state-allocation+prompt-encode+first-cpu-projection\n",
        accepted as u8,
        elapsed_ms_since(started),
        tokenizer_ready as u8,
        model_ready as u8,
        f32_ready as u8,
        assets_ready as u8,
        actual_worker_slot,
    );
'''
    text = replace_span(
        text,
        "    let assets_ready =",
        "\n}",
        completion.rstrip("\n"),
        "boot warm completion",
    )
    if "crate::intel" in text or "physical_gpu_ready" in text or "no-submit" in text:
        raise RuntimeError("boot warm postcondition failed: GPU warm path remains")
    return text


def build_changes(repo: Path, bundle: Path) -> dict[Path, tuple[str, str]]:
    targets: dict[Path, tuple[str, str]] = {}

    def transformed(relative: str, function) -> None:
        path = repo / relative
        before = read(path)
        after = function(before)
        if before == after:
            raise RuntimeError(f"{relative}: transformation made no changes")
        targets[path] = (before, after)

    transformed("crates/trueos-lfm25-cpu/src/lib.rs", transform_lfm25_cpu_lib)
    transformed("src/ai/lfm25_hybrid_cpu_backend.rs", transform_backend)
    transformed("src/lumen/decode.rs", transform_decode)
    transformed("src/ai/lumen_service.rs", transform_lumen_service)

    transformed("src/ai/lfm25_boot_warm.rs", transform_boot_warm)

    module_path = repo / "crates/trueos-lfm25-cpu/src/cpu_vnni.rs"
    module_after = read(bundle / "files/crates/trueos-lfm25-cpu/src/cpu_vnni.rs")
    module_before = module_path.read_text(encoding="utf-8") if module_path.exists() else ""
    if module_before == module_after:
        raise RuntimeError("cpu_vnni.rs already contains the bundle version")
    targets[module_path] = (module_before, module_after)
    return targets


def diff_for(repo: Path, changes: dict[Path, tuple[str, str]]) -> str:
    chunks: list[str] = []
    for path in sorted(changes, key=lambda value: str(value)):
        before, after = changes[path]
        relative = path.relative_to(repo).as_posix()
        from_name = f"a/{relative}" if before else "/dev/null"
        to_name = f"b/{relative}"
        chunks.extend(
            difflib.unified_diff(
                before.splitlines(keepends=True),
                after.splitlines(keepends=True),
                fromfile=from_name,
                tofile=to_name,
                n=3,
            )
        )
    return "".join(chunks)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("repo", type=Path, help="path to a TRUEOS checkout")
    parser.add_argument(
        "--check",
        action="store_true",
        help="validate anchors and print the diff without writing",
    )
    parser.add_argument(
        "--write-patch",
        type=Path,
        help="also write the generated unified diff to this path",
    )
    args = parser.parse_args()

    repo = args.repo.resolve()
    bundle = Path(__file__).resolve().parent
    if not (repo / ".git").exists():
        print(f"error: not a git checkout: {repo}", file=sys.stderr)
        return 2

    commit = current_commit(repo)
    if commit:
        status = "recognized" if commit in EXPECTED_BASES else "newer-or-different"
        print(
            f"TRUEOS HEAD {commit} ({status}); exact source anchors remain authoritative",
            file=sys.stderr if args.check else sys.stdout,
        )

    try:
        changes = build_changes(repo, bundle)
    except RuntimeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    patch = diff_for(repo, changes)
    if args.write_patch:
        args.write_patch.write_text(patch, encoding="utf-8")
    if args.check:
        sys.stdout.write(patch)
        return 0

    for path, (_, after) in changes.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(after, encoding="utf-8")
    print("applied native-Q8 CPU VNNI Lumen integration:")
    for path in sorted(changes, key=lambda value: str(value)):
        print(f"  {path.relative_to(repo)}")
    print("next: cargo test -p trueos-lfm25-cpu --lib")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
