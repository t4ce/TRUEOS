#!/usr/bin/env python3
"""Exercise every anchored source transform against a compact current-source fixture.

This does not replace applying against a real TRUEOS checkout. It makes the
bundle's transformation logic reproducible without carrying copies of the large
existing source files.
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def load_apply_module():
    spec = importlib.util.spec_from_file_location("trueos_vnni_apply", ROOT / "apply.py")
    if spec is None or spec.loader is None:
        raise AssertionError("could not load apply.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def cpu_lib_fixture() -> str:
    return """//! Scalar, deterministic LFM2.5 CPU kernels for TRUEOS's hybrid decoder.
//!
//! This crate deliberately owns numerical primitives only. Model I/O, token
//! scheduling, Intel GPU submission, and cache ownership remain in TRUEOS.

extern crate alloc;

use alloc::collections::BTreeMap;

pub const PACKED_Q8X16_SUBNORMAL_SCALES: u64 = 25_994;

pub enum Error {
    Vocabulary,
    Allocation,
    NonFinite,
}
"""


def backend_fixture() -> str:
    return """//! Fixed LFM2.5 CPU + Intel IGC decode backend.
//!
//! The fixed control plane remains the 99-operation Lumen AOT schedule. CPU
//! kernels execute state, normalization, attention-reduction, and nonlinear
//! stages. Every Q8 projection is submitted to the admitted C++/IGC kernel on
//! the dedicated Intel GuC/RCS lane.

use trueos_time::{Duration, Instant, Timer};
const ATTENTION_REDUCTION_TILE: usize = 256;

pub enum HybridCpuBackendError {
    #[expect(dead_code, reason = \"baseline archived in tools/warnings_last\")]
    Gpu(crate::intel::gpgpu::Lfm25Q8ProjectError),
    Kernel(cpu::Error),
}

impl From<crate::intel::gpgpu::Lfm25Q8ProjectError> for HybridCpuBackendError {
    fn from(error: crate::intel::gpgpu::Lfm25Q8ProjectError) -> Self {
        Self::Gpu(error)
    }
}

struct CpuQ8Tensor {
    /// Preserve the normalized F32 result for CPU stages. Each admitted iGPU
    /// projection quantizes this exact vector into the fixed Q8_0 input ABI.
    values: Vec<f32>,
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

pub struct HybridCpuAotDecodeBackend {
    assets: Arc<IntelIgcResidentAssets>,
    slots: Vec<Option<CpuTensor>>,
}

pub type IntelIgcAotDecodeBackend = HybridCpuAotDecodeBackend;

async fn load_resident_model() -> Result<IntelIgcResidentModel, HybridCpuBackendError> {
    let packed = cpu::pack_q8x16_model_in_place(model)?;
    let gpu_model = crate::intel::gpgpu::bind_lfm25_q8_packed_model(model)?;
    Ok(IntelIgcResidentModel { model_storage, model_offset, gpu_model })
}

async fn resident_model() -> Result<Arc<IntelIgcResidentModel>, HybridCpuBackendError> {
    todo!()
}

pub(crate) async fn warm_intel_igc_model() -> Result<(), HybridCpuBackendError> {
    todo!()
}

pub(crate) async fn warm_intel_igc_f32() -> Result<(), HybridCpuBackendError> {
    todo!()
}

pub(crate) fn intel_igc_resident_assets_ready() -> bool {
    false
}

pub async fn open_intel_igc_backend() -> Result<IntelIgcAotDecodeBackend, HybridCpuBackendError> {
    todo!()
}

#[expect(dead_code, reason = \"baseline archived in tools/warnings_last\")]
pub async fn open_hybrid_backend() -> Result<HybridCpuAotDecodeBackend, HybridCpuBackendError> {
    open_intel_igc_backend().await
}

impl HybridCpuAotDecodeBackend {
    /// Immutable model/F32 assets, transient tensor slots, GPU mappings and
    /// GuC/RCS ownership deliberately remain kernel capabilities and are
    /// reacquired when this image is restored.
    fn checkpoint_state(&self) {
        image.extend_from_slice(&cpu::PACKED_Q8X16_IMAGE_SHA256);
        let packed_model_sha256 = &image[40..72];
        let _ = packed_model_sha256;
    }

    async fn project_many(
        &self,
        descriptors: &[NativeTensorDescriptor],
        input: &[f32],
    ) -> Result<Vec<Vec<f32>>, HybridCpuBackendError> {
        crate::intel::gpgpu::lfm25_q8_project_batch(
            self.assets.model.gpu_model,
            specs,
            activation,
            output_slices,
        )?;
        todo!()
    }

    fn handle_index(&self) {
        todo!()
    }

    fn embedding(&mut self, row: EmbeddingRowPlan) -> Result<HiddenQ30, HybridCpuBackendError> {
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
}
"""


def decode_fixture() -> str:
    return """//! Lumen module adapter for the fixed LFM2.5 decode scheduler.
//!
//! The adapter owns exactly one [`DecodeSession`] and one backend. A decode
//! call therefore represents one token on one ordered lane. Production uses
//! scalar CPU state stages and the admitted Intel C++/IGC projection program;
//! no runtime graph is interpreted.

/// callback. The generic boundary keeps the scheduler independent of the Intel
/// execution transport.

use crate::r::lfm25_hybrid_cpu_backend::IntelIgcAotDecodeBackend;

/// Bind the sealed scalar CPU stages and the admitted Intel C++/IGC projection
/// program to the same fixed 99-operation Lumen module.
pub async fn open_intel_igc() {
    let _ = open_intel_igc_backend().await;
}

pub fn checkpoint_intel_igc() {}
pub fn restore_intel_igc() {}
"""


def service_fixture() -> str:
    return """//! VM-scoped Lumen inference capability for replicatable Blueprints.
//!
//! The Blueprint owns chat/tool policy and stores the portable mutable session
//! image in its private memory. The kernel owns immutable model assets and the
//! CPU+IGC+GuC execution lane. No GPU handle, model pointer or host allocation
//! crosses this boundary.

use crate::lumen::decode::{checkpoint_intel_igc, restore_intel_igc};
type LfmModule = crate::lumen::decode::Lfm25Decode<
    crate::r::lfm25_hybrid_cpu_backend::IntelIgcAotDecodeBackend,
>;

async fn open() {
    let _ = crate::lumen::decode::open_intel_igc().await;
    let action = \"action=release-gpu-session\";
    let _ = (action, checkpoint_intel_igc, restore_intel_igc);
}
"""


def boot_fixture() -> str:
    return """//! Optional post-compositor warmup for the one boot-resident Lumen model.
//!
//! Only immutable, reusable assets are prepared here. Conversation state,
//! prompt prefill, and GPU submissions remain demand-driven.

fn service_task() {
    let physical_gpu_ready = crate::gpu::physical::physical_device().is_some();
    if !crate::intel::guc_submission_ready() || !physical_gpu_ready {
        return;
    }
    let started = Instant::now();
    let submissions_before = crate::intel::gpgpu::lfm25_q8_project_stats().submissions;

    let tokenizer_started = Instant::now();
    let _ = tokenizer_started;
    let _ = crate::r::lfm25_hybrid_cpu_backend::warm_intel_igc_model();
    let _ = crate::r::lfm25_hybrid_cpu_backend::warm_intel_igc_f32();
    let model_log = \"lfm25: boot-warm stage=model-ready elapsed_ms={} resident=1 layout=pair1088-x16-dp4a gpu_runtime_mapping=ready warm_contract=no-submit\\n\";
    let _ = model_log;

    let assets_ready = crate::r::lfm25_hybrid_cpu_backend::intel_igc_resident_assets_ready();
    let accepted = assets_ready;
    let submissions_after = crate::intel::gpgpu::lfm25_q8_project_stats().submissions;
    let _ = (accepted, submissions_before, submissions_after, started);
}
"""


def main() -> None:
    apply = load_apply_module()

    cpu = apply.transform_lfm25_cpu_lib(cpu_lib_fixture())
    assert "mod cpu_vnni;" in cpu
    assert "UnsupportedCpu" in cpu
    assert "LFM25_Q8_WEIGHT_BYTES_PER_TOKEN" in cpu

    backend = apply.transform_backend(backend_fixture())
    for required in (
        "Q8VnniProjector",
        "validate_q8_vnni_matrix",
        "Q8VnniActivation::quantize",
        "dequantize_q8_row",
        "LFM25_Q8_WEIGHT_BYTES_PER_TOKEN",
        "open_cpu_vnni_backend",
    ):
        assert required in backend, required
    for forbidden in (
        "pack_q8x16_model_in_place",
        "lfm25_q8_project_batch",
        "dequantize_q8x16_row",
        "gpu_model",
        "open_intel_igc_backend",
        "warm_intel_igc_model",
    ):
        assert forbidden not in backend, forbidden

    decode = apply.transform_decode(decode_fixture())
    assert "CpuVnniAotDecodeBackend" in decode
    assert "open_cpu_vnni" in decode
    assert "intel_igc" not in decode

    service = apply.transform_lumen_service(service_fixture())
    assert "CpuVnniAotDecodeBackend" in service
    assert "release-cpu-session" in service
    assert "intel_igc" not in service

    warm = apply.transform_boot_warm(boot_fixture())
    assert "Q8VnniProjector::detect()" in warm
    assert "warm_cpu_vnni_model" in warm
    assert "warm_contract=no-project" in warm
    assert "crate::intel" not in warm

    # Exercise the command-line dry-run and write paths against the same compact
    # fixture. Source anchors are still the authority when the user applies the
    # bundle to the real checkout.
    with tempfile.TemporaryDirectory(prefix="trueos-vnni-transform-") as temporary:
        repo = Path(temporary)
        sources = {
            "crates/trueos-lfm25-cpu/src/lib.rs": cpu_lib_fixture(),
            "src/r/lfm25_hybrid_cpu_backend.rs": backend_fixture(),
            "src/lumen/decode.rs": decode_fixture(),
            "src/r/lumen_service.rs": service_fixture(),
            "src/r/lfm25_boot_warm.rs": boot_fixture(),
        }
        for relative, source in sources.items():
            destination = repo / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(source, encoding="utf-8")
        subprocess.run(
            ["git", "init", "-q", str(repo)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        dry_run = subprocess.run(
            [sys.executable, str(ROOT / "apply.py"), str(repo), "--check"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        assert "cpu_vnni.rs" in dry_run.stdout
        assert "open_cpu_vnni_backend" in dry_run.stdout
        subprocess.run(
            [sys.executable, str(ROOT / "apply.py"), str(repo)],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        generated = repo / "crates/trueos-lfm25-cpu/src/cpu_vnni.rs"
        assert generated.read_bytes() == (
            ROOT / "files/crates/trueos-lfm25-cpu/src/cpu_vnni.rs"
        ).read_bytes()
        assert "open_cpu_vnni_backend" in (
            repo / "src/r/lfm25_hybrid_cpu_backend.rs"
        ).read_text(encoding="utf-8")

    with tempfile.TemporaryDirectory(prefix="trueos-vnni-drift-") as temporary:
        repo = Path(temporary)
        sources = {
            "crates/trueos-lfm25-cpu/src/lib.rs": cpu_lib_fixture(),
            "src/r/lfm25_hybrid_cpu_backend.rs": backend_fixture(),
            "src/lumen/decode.rs": decode_fixture(),
            "src/r/lumen_service.rs": service_fixture(),
            # Deliberately drift the last source anchor so every earlier
            # transform has been evaluated but no destination is written.
            "src/r/lfm25_boot_warm.rs": boot_fixture().replace(
                "GPU submissions remain demand-driven",
                "GPU work remains demand-driven",
            ),
        }
        for relative, source in sources.items():
            destination = repo / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(source, encoding="utf-8")
        original_lib = (repo / "crates/trueos-lfm25-cpu/src/lib.rs").read_bytes()
        subprocess.run(
            ["git", "init", "-q", str(repo)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        failed = subprocess.run(
            [sys.executable, str(ROOT / "apply.py"), str(repo)],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        assert failed.returncode != 0
        assert (repo / "crates/trueos-lfm25-cpu/src/lib.rs").read_bytes() == original_lib
        assert not (repo / "crates/trueos-lfm25-cpu/src/cpu_vnni.rs").exists()

    print("transformer fixture + dry-run/apply + drift guard: PASS")


if __name__ == "__main__":
    main()
