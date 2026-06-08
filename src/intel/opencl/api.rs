//! Small Rust-internal API seed for the future TRUEOS OpenCL CABI.
//!
//! These functions are intentionally boring wrappers over the registry/backend
//! pieces. They are the shape we can later expose through a C ABI without
//! letting the external ABI leak into the early runtime internals.

use super::{
    ClError, ClResult, IntelOpenClBackend, KnownAotValidationReport, KnownKernelRole,
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
    pub(crate) binary_bytes: usize,
    pub(crate) spirv_bytes: usize,
    pub(crate) binary_sha256: [u8; 32],
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
        binary_bytes: kernel.artifact.bin.len(),
        spirv_bytes: kernel.artifact.spv.len(),
        binary_sha256: kernel.artifact.bin_sha256,
    })
}

pub(crate) fn trueos_cl_upload_known_kernel(name: &str) -> ClResult<UploadedKernelRef> {
    let backend = IntelOpenClBackend::new();
    backend.require_known_aot_upload(name)
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
