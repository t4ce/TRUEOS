//! Minimal bridge from the future OpenCL runtime to the existing Intel GPGPU
//! AOT artifact upload path.

use super::{
    BuiltProgram,
    queue::{CommandKind, CommandQueue},
    registry,
    types::{ClError, ClResult, NdRange},
    validation::{
        KnownAotValidationReport, validate_known_aot_registry, validate_known_aot_status,
    },
};
use crate::intel::gpgpu;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct BackendCaps {
    pub(crate) aot_artifacts: bool,
    pub(crate) upload_status: bool,
    pub(crate) known_kernel_upload: bool,
    pub(crate) known_kernel_execute_stub: bool,
    /// Exact checked-in source strings can resolve to already baked artifacts.
    pub(crate) known_source_aot_lookup: bool,
    /// No Clang, NEO, IGC, or other source compiler exists in the TRUEOS image.
    pub(crate) source_compile: bool,
    pub(crate) svm: bool,
}

impl BackendCaps {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const INTEL_OPENCL_BRIDGE: Self = Self {
        aot_artifacts: true,
        upload_status: true,
        known_kernel_upload: true,
        known_kernel_execute_stub: true,
        known_source_aot_lookup: true,
        source_compile: false,
        svm: false,
    };
}

impl Default for BackendCaps {
    fn default() -> Self {
        Self::INTEL_OPENCL_BRIDGE
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct UploadedKernelRef {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) source: &'static str,
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
    pub(crate) mapped_bytes: usize,
    pub(crate) verified: bool,
    pub(crate) bin_sha256: [u8; 32],
}

impl UploadedKernelRef {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn is_ready(self) -> bool {
        self.verified && self.bytes != 0
    }
}

impl From<gpgpu::UploadedKernelArtifact> for UploadedKernelRef {
    fn from(upload: gpgpu::UploadedKernelArtifact) -> Self {
        Self {
            name: upload.name,
            target: upload.target,
            source: upload.source,
            gpu: upload.gpu,
            phys: upload.phys,
            bytes: upload.bytes,
            mapped_bytes: upload.mapped_bytes,
            verified: upload.verified,
            bin_sha256: upload.bin_sha256,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct BackendExecutionStub {
    pub(crate) kernel_name: &'static str,
    pub(crate) upload: Option<UploadedKernelRef>,
    pub(crate) recognized: bool,
    pub(crate) submitted: bool,
}

impl BackendExecutionStub {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    const fn unknown(kernel_name: &'static str) -> Self {
        Self {
            kernel_name,
            upload: None,
            recognized: false,
            submitted: false,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    const fn recognized(kernel_name: &'static str, upload: Option<UploadedKernelRef>) -> Self {
        Self {
            kernel_name,
            upload,
            recognized: true,
            submitted: false,
        }
    }
}

#[derive(Debug)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) enum BackendCommand<'a> {
    QueryUploadStatus {
        kernel_name: &'static str,
    },
    UploadKnownAot {
        kernel_name: &'static str,
    },
    UploadAllKnownAot {
        out: &'a mut [Option<UploadedKernelRef>],
    },
    ExecuteKnownKernelStub {
        kernel_name: &'static str,
        nd_range: NdRange,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) enum BackendCommandResult {
    UploadStatus(Option<UploadedKernelRef>),
    UploadMany { attempted: usize, uploaded: usize },
    ExecuteStub(BackendExecutionStub),
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct IntelOpenClBackend {
    caps: BackendCaps,
}

impl IntelOpenClBackend {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn new() -> Self {
        Self {
            caps: BackendCaps::INTEL_OPENCL_BRIDGE,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn caps(&self) -> BackendCaps {
        self.caps
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn upload_status(&self, kernel_name: &str) -> Option<UploadedKernelRef> {
        registry::known_aot_kernel(kernel_name)
            .and_then(|kernel| kernel.status())
            .map(UploadedKernelRef::from)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn upload_known_aot(&self, kernel_name: &str) -> Option<UploadedKernelRef> {
        registry::known_aot_kernel(kernel_name)
            .and_then(|kernel| kernel.upload())
            .map(UploadedKernelRef::from)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn require_known_aot_upload(
        &self,
        kernel_name: &str,
    ) -> ClResult<UploadedKernelRef> {
        if !registry::is_known_aot_kernel(kernel_name) {
            return Err(ClError::InvalidKernelName);
        }
        let upload = self
            .upload_known_aot(kernel_name)
            .ok_or(ClError::OutOfResources)?;
        if upload.is_ready() {
            Ok(upload)
        } else {
            Err(ClError::InvalidBinary)
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn reload_known_aot(&self, kernel_name: &str) -> ClResult<UploadedKernelRef> {
        match gpgpu::reload_known_kernel_artifact(kernel_name) {
            Ok(upload) => Ok(UploadedKernelRef::from(upload)),
            Err(gpgpu::GpgpuArtifactReloadError::UnknownKernel) => Err(ClError::InvalidKernelName),
            Err(gpgpu::GpgpuArtifactReloadError::NoClaimedDevice) => Err(ClError::OutOfResources),
            Err(gpgpu::GpgpuArtifactReloadError::UploadFailed) => Err(ClError::InvalidBinary),
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn reload_all_known_aot(&self) -> gpgpu::GpgpuArtifactReloadSummary {
        gpgpu::reload_all_known_kernel_artifacts()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn build_program_from_source(
        &self,
        source: &str,
        options: &str,
    ) -> ClResult<BuiltProgram<'static>> {
        if !options.trim().is_empty() {
            return Err(ClError::InvalidBuildOptions);
        }

        if self.caps.known_source_aot_lookup {
            if let Some(program) = registry::build_program_from_known_source(source, options) {
                return Ok(program);
            }
        }
        if !self.caps.source_compile {
            return Err(ClError::CompilerNotAvailable);
        }
        Err(ClError::BuildProgramFailure)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn upload_all_known_aot(
        &self,
        out: &mut [Option<UploadedKernelRef>],
    ) -> (usize, usize) {
        let mut attempted = 0usize;
        let mut uploaded = 0usize;
        for (slot, kernel) in out
            .iter_mut()
            .zip(registry::KNOWN_AOT_KERNELS.iter().copied())
        {
            attempted = attempted.saturating_add(1);
            *slot = kernel.upload().map(UploadedKernelRef::from);
            if slot.is_some() {
                uploaded = uploaded.saturating_add(1);
            }
        }
        (attempted, uploaded)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn execute_known_kernel_stub(
        &self,
        kernel_name: &'static str,
        _nd_range: NdRange,
    ) -> BackendExecutionStub {
        if !registry::is_known_aot_kernel(kernel_name) {
            return BackendExecutionStub::unknown(kernel_name);
        }
        BackendExecutionStub::recognized(kernel_name, self.upload_status(kernel_name))
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn dispatch(&self, command: BackendCommand<'_>) -> BackendCommandResult {
        match command {
            BackendCommand::QueryUploadStatus { kernel_name } => {
                BackendCommandResult::UploadStatus(self.upload_status(kernel_name))
            }
            BackendCommand::UploadKnownAot { kernel_name } => {
                BackendCommandResult::UploadStatus(self.upload_known_aot(kernel_name))
            }
            BackendCommand::UploadAllKnownAot { out } => {
                let (attempted, uploaded) = self.upload_all_known_aot(out);
                BackendCommandResult::UploadMany {
                    attempted,
                    uploaded,
                }
            }
            BackendCommand::ExecuteKnownKernelStub {
                kernel_name,
                nd_range,
            } => BackendCommandResult::ExecuteStub(
                self.execute_known_kernel_stub(kernel_name, nd_range),
            ),
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn dispatch_checked(
        &self,
        command: BackendCommand<'_>,
    ) -> ClResult<BackendCommandResult> {
        match command {
            BackendCommand::UploadKnownAot { kernel_name } => self
                .require_known_aot_upload(kernel_name)
                .map(|upload| BackendCommandResult::UploadStatus(Some(upload))),
            BackendCommand::ExecuteKnownKernelStub {
                kernel_name,
                nd_range,
            } => {
                nd_range.validate()?;
                let upload = self.require_known_aot_upload(kernel_name)?;
                Ok(BackendCommandResult::ExecuteStub(BackendExecutionStub::recognized(
                    kernel_name,
                    Some(upload),
                )))
            }
            other => Ok(self.dispatch(other)),
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn finish_known_queue(&self, queue: &mut CommandQueue) -> ClResult<usize> {
        queue.finish_with(|command| match &command.kind {
            CommandKind::WriteBuffer { .. } | CommandKind::ReadBuffer { .. } => Ok(()),
            CommandKind::KnownKernel {
                kernel_name,
                nd_range,
            } => {
                nd_range.validate()?;
                self.require_known_aot_upload(kernel_name)?;
                Ok(())
            }
            CommandKind::Kernel { .. } => Err(ClError::InvalidKernel),
        })
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn known_kernel_count(&self) -> usize {
        registry::KNOWN_AOT_KERNELS.len()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn known_kernel(
        &self,
        kernel_name: &str,
    ) -> Option<&'static registry::KnownAotKernel> {
        registry::known_aot_kernel(kernel_name)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn validate_known_aot_registry(&self) -> KnownAotValidationReport {
        validate_known_aot_registry()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn validate_known_aot_status(&self) -> KnownAotValidationReport {
        validate_known_aot_status()
    }
}
