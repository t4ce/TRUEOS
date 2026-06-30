//! Small Rust-internal API seed for the future TRUEOS OpenCL CABI.
//!
//! These functions are intentionally boring wrappers over the registry/backend
//! pieces. They are the shape we can later expose through a C ABI without
//! letting the external ABI leak into the early runtime internals.

use super::{
    BuiltProgram, ClError, ClResult, GpuArtifactProducer, IntelOpenClBackend, KernelLaunchModel,
    KnownAotValidationReport, KnownKernelRole,
    backend::UploadedKernelRef,
    example::{KnownAotQueueProbe, fill_rect_worklist_queue_probe},
    registry::{KNOWN_AOT_KERNELS, known_aot_kernel},
    validation::{validate_known_aot_registry, validate_known_aot_status},
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KnownKernelInfo {
    pub(crate) index: usize,
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) role: KnownKernelRole,
    pub(crate) producer: GpuArtifactProducer,
    pub(crate) source_path: &'static str,
    pub(crate) entry_text_offset_bytes: u64,
    pub(crate) cross_thread_bytes: u32,
    pub(crate) per_thread_bytes: u32,
    pub(crate) binding_count: u32,
    pub(crate) arg_count: usize,
    pub(crate) descriptor_layout_count: usize,
    pub(crate) launch_model: KernelLaunchModel,
    pub(crate) simd_width: u32,
    pub(crate) binary_bytes: usize,
    pub(crate) spirv_bytes: usize,
    pub(crate) binary_sha256: [u8; 32],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceBuildSmoke {
    pub(crate) source_compile_cap: bool,
    pub(crate) source_build_error: Option<ClError>,
    pub(crate) registry_kernels: usize,
    pub(crate) registry_passed: bool,
    pub(crate) queue_completed_commands: usize,
    pub(crate) fill_rect_uploaded: bool,
    pub(crate) queue_error: Option<ClError>,
}

pub(crate) fn trueos_cl_known_kernel_count() -> usize {
    KNOWN_AOT_KERNELS.len()
}

pub(crate) fn trueos_cl_known_kernel_info(index: usize) -> Option<KnownKernelInfo> {
    let kernel = KNOWN_AOT_KERNELS.get(index)?;
    Some(KnownKernelInfo {
        index,
        name: kernel.name,
        target: kernel.artifact.target,
        role: kernel.role,
        producer: kernel.contract.producer,
        source_path: kernel.contract.source_path,
        entry_text_offset_bytes: kernel.contract.entry_text_offset_bytes,
        cross_thread_bytes: kernel.contract.cross_thread_bytes,
        per_thread_bytes: kernel.contract.per_thread_bytes,
        binding_count: kernel.contract.binding_count,
        arg_count: kernel.contract.args.len(),
        descriptor_layout_count: kernel.contract.descriptor_layouts.len(),
        launch_model: kernel.contract.launch.model,
        simd_width: kernel.contract.launch.simd_width,
        binary_bytes: kernel.artifact.bin.len(),
        spirv_bytes: kernel.artifact.spv.len(),
        binary_sha256: kernel.artifact.bin_sha256,
    })
}

pub(crate) fn trueos_cl_upload_known_kernel(name: &str) -> ClResult<UploadedKernelRef> {
    let backend = IntelOpenClBackend::new();
    backend.require_known_aot_upload(name)
}

pub(crate) fn trueos_cl_reload_known_kernel(name: &str) -> ClResult<UploadedKernelRef> {
    IntelOpenClBackend::new().reload_known_aot(name)
}

pub(crate) fn trueos_cl_reload_all_known_kernels() -> crate::intel::gpgpu::GpgpuArtifactReloadSummary
{
    IntelOpenClBackend::new().reload_all_known_aot()
}

pub(crate) fn trueos_cl_build_program_from_source(
    source: &str,
    options: &str,
) -> ClResult<BuiltProgram<'static>> {
    IntelOpenClBackend::new().build_program_from_source(source, options)
}

pub(crate) fn trueos_cl_known_kernel_uploaded(name: &str) -> ClResult<bool> {
    if known_aot_kernel(name).is_none() {
        return Err(ClError::InvalidKernelName);
    }
    Ok(IntelOpenClBackend::new()
        .upload_status(name)
        .map(|upload| upload.is_ready())
        .unwrap_or(false))
}

pub(crate) fn trueos_cl_probe_known_aot_queue() -> ClResult<KnownAotQueueProbe> {
    fill_rect_worklist_queue_probe()
}

pub(crate) fn trueos_cl_validate_known_aot_registry() -> KnownAotValidationReport {
    validate_known_aot_registry()
}

pub(crate) fn trueos_cl_validate_known_aot_status() -> KnownAotValidationReport {
    validate_known_aot_status()
}

pub(crate) fn trueos_cl_source_build_smoke() -> SourceBuildSmoke {
    let backend = IntelOpenClBackend::new();
    let caps = backend.caps();
    let source_build_error = match backend
        .build_program_from_source(crate::intel::gpgpu::FILL_RECT_RGBA8_OPENCL_SOURCE, "")
    {
        Ok(_) => None,
        Err(err) => Some(err),
    };
    let registry = validate_known_aot_registry();
    let (queue_completed_commands, fill_rect_uploaded, queue_error) =
        match fill_rect_worklist_queue_probe() {
            Ok(probe) => (probe.completed_commands, probe.fill_rect_uploaded, None),
            Err(err) => (0, false, Some(err)),
        };

    SourceBuildSmoke {
        source_compile_cap: caps.source_compile,
        source_build_error,
        registry_kernels: registry.registry_kernels,
        registry_passed: registry.passed(),
        queue_completed_commands,
        fill_rect_uploaded,
        queue_error,
    }
}
