//! Minimal bridge from the future OpenCL runtime to the existing Intel GPGPU
//! AOT artifact upload path.

use super::{
    queue::{CommandKind, CommandQueue},
    registry,
    types::{ClError, ClResult, NdRange},
    validation::{
        KnownAotValidationReport, validate_known_aot_registry, validate_known_aot_status,
    },
};
use crate::intel::gpgpu;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BackendCaps {
    pub(crate) aot_artifacts: bool,
    pub(crate) upload_status: bool,
    pub(crate) known_kernel_upload: bool,
    pub(crate) known_kernel_execute_stub: bool,
    pub(crate) source_compile: bool,
    pub(crate) svm: bool,
}

impl BackendCaps {
    pub(crate) const INTEL_OPENCL_BRIDGE: Self = Self {
        aot_artifacts: true,
        upload_status: true,
        known_kernel_upload: true,
        known_kernel_execute_stub: true,
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
pub(crate) struct UploadedKernelRef {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
    pub(crate) mapped_bytes: usize,
    pub(crate) verified: bool,
}

impl UploadedKernelRef {
    pub(crate) const fn is_ready(self) -> bool {
        self.verified && self.bytes != 0
    }
}

impl From<gpgpu::UploadedKernelArtifact> for UploadedKernelRef {
    fn from(upload: gpgpu::UploadedKernelArtifact) -> Self {
        Self {
            name: upload.name,
            target: upload.target,
            gpu: upload.gpu,
            phys: upload.phys,
            bytes: upload.bytes,
            mapped_bytes: upload.mapped_bytes,
            verified: upload.verified,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BackendExecutionStub {
    pub(crate) kernel_name: &'static str,
    pub(crate) upload: Option<UploadedKernelRef>,
    pub(crate) recognized: bool,
    pub(crate) submitted: bool,
}

impl BackendExecutionStub {
    const fn unknown(kernel_name: &'static str) -> Self {
        Self {
            kernel_name,
            upload: None,
            recognized: false,
            submitted: false,
        }
    }

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
    FillRectWorklistRgba8Stub {
        dst: gpgpu::GpgpuRgba8Surface,
        rect: gpgpu::GpgpuRect,
        color_rgba: u32,
    },
    Sprite64WorklistRgba8Stub {
        placements: &'a [gpgpu::GpgpuSprite64Placement],
        present: bool,
        present_reason: &'a str,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BackendCommandResult {
    UploadStatus(Option<UploadedKernelRef>),
    UploadMany { attempted: usize, uploaded: usize },
    ExecuteStub(BackendExecutionStub),
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct IntelOpenClBackend {
    caps: BackendCaps,
}

impl IntelOpenClBackend {
    pub(crate) const fn new() -> Self {
        Self {
            caps: BackendCaps::INTEL_OPENCL_BRIDGE,
        }
    }

    pub(crate) const fn caps(&self) -> BackendCaps {
        self.caps
    }

    pub(crate) fn upload_status(&self, kernel_name: &str) -> Option<UploadedKernelRef> {
        registry::known_aot_kernel(kernel_name)
            .and_then(|kernel| kernel.status())
            .map(UploadedKernelRef::from)
    }

    pub(crate) fn upload_known_aot(&self, kernel_name: &str) -> Option<UploadedKernelRef> {
        registry::known_aot_kernel(kernel_name)
            .and_then(|kernel| kernel.upload())
            .map(UploadedKernelRef::from)
    }

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

    pub(crate) fn upload_fill_rect_worklist_rgba8(&self) -> Option<UploadedKernelRef> {
        gpgpu::upload_fill_rect_worklist_rgba8_kernel().map(UploadedKernelRef::from)
    }

    pub(crate) fn upload_sprite64_worklist_rgba8(&self) -> Option<UploadedKernelRef> {
        gpgpu::upload_sprite64_worklist_rgba8_kernel().map(UploadedKernelRef::from)
    }

    pub(crate) fn fill_rect_worklist_upload_status(&self) -> Option<UploadedKernelRef> {
        gpgpu::fill_rect_worklist_rgba8_upload_status().map(UploadedKernelRef::from)
    }

    pub(crate) fn sprite64_worklist_upload_status(&self) -> Option<UploadedKernelRef> {
        gpgpu::sprite64_worklist_rgba8_upload_status().map(UploadedKernelRef::from)
    }

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

    pub(crate) fn execute_fill_rect_worklist_rgba8_stub(
        &self,
        _dst: gpgpu::GpgpuRgba8Surface,
        _rect: gpgpu::GpgpuRect,
        _color_rgba: u32,
    ) -> BackendExecutionStub {
        BackendExecutionStub::recognized(
            gpgpu::FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
            self.fill_rect_worklist_upload_status(),
        )
    }

    pub(crate) fn execute_sprite64_worklist_rgba8_stub(
        &self,
        _placements: &[gpgpu::GpgpuSprite64Placement],
        _present: bool,
        _present_reason: &str,
    ) -> BackendExecutionStub {
        BackendExecutionStub::recognized(
            gpgpu::SPRITE64_WORKLIST_RGBA8_KERNEL_NAME,
            self.sprite64_worklist_upload_status(),
        )
    }

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
            BackendCommand::FillRectWorklistRgba8Stub {
                dst,
                rect,
                color_rgba,
            } => BackendCommandResult::ExecuteStub(
                self.execute_fill_rect_worklist_rgba8_stub(dst, rect, color_rgba),
            ),
            BackendCommand::Sprite64WorklistRgba8Stub {
                placements,
                present,
                present_reason,
            } => BackendCommandResult::ExecuteStub(self.execute_sprite64_worklist_rgba8_stub(
                placements,
                present,
                present_reason,
            )),
        }
    }

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

    pub(crate) fn known_kernel_count(&self) -> usize {
        registry::KNOWN_AOT_KERNELS.len()
    }

    pub(crate) fn known_kernel(
        &self,
        kernel_name: &str,
    ) -> Option<&'static registry::KnownAotKernel> {
        registry::known_aot_kernel(kernel_name)
    }

    pub(crate) fn validate_known_aot_registry(&self) -> KnownAotValidationReport {
        validate_known_aot_registry()
    }

    pub(crate) fn validate_known_aot_status(&self) -> KnownAotValidationReport {
        validate_known_aot_status()
    }
}
