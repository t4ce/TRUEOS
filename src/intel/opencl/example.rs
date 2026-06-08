//! Example wiring for the AOT-first OpenCL runtime shape.
//!
//! This is deliberately side-effect-light. It builds the same conceptual path
//! an eventual C ABI would expose, while the backend still owns the real Intel
//! GPGPU submission details.

use super::{
    BufferRegistry, BuiltProgram, ClError, ClResult, CommandQueue, ContextId, DeviceId, EventId,
    IntelOpenClBackend, KernelArgDesc, KernelArgKind, KernelMetadata, MemFlags, NdRange,
    ProgramArtifact, QueueId, QueueProperties,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KnownAotQueueProbe {
    pub(crate) known_kernels: usize,
    pub(crate) completed_commands: usize,
    pub(crate) fill_rect_uploaded: bool,
}

pub(crate) fn fill_rect_worklist_runtime_example() -> ClResult<()> {
    let _ = fill_rect_worklist_queue_probe()?;
    Ok(())
}

pub(crate) fn fill_rect_worklist_queue_probe() -> ClResult<KnownAotQueueProbe> {
    let device = DeviceId::from_raw(1);
    let backend = IntelOpenClBackend::new();
    let caps = backend.caps();
    if !caps.aot_artifacts {
        return Err(ClError::CompilerNotAvailable);
    }

    const ARGS: &[KernelArgDesc<'_>] = &[
        KernelArgDesc::new(0, "dst_rgba8", "__global uchar4*", KernelArgKind::Buffer, 8, 8),
        KernelArgDesc::new(1, "descs", "__global const FillRectDesc*", KernelArgKind::Buffer, 8, 8),
        KernelArgDesc::new(2, "dst_bytes", "uint", KernelArgKind::Scalar, 4, 4),
        KernelArgDesc::new(3, "desc_count", "uint", KernelArgKind::Scalar, 4, 4),
    ];
    const KERNELS: &[KernelMetadata<'_>] = &[KernelMetadata::new("fill_rect_worklist_rgba8", ARGS)];
    const PROGRAM: ProgramArtifact<'_> = ProgramArtifact::new(
        "trueos.ui.primitives",
        "adls",
        super::ProgramBinaryKind::IntelGenBinary,
        crate::intel::gpgpu::FILL_RECT_WORKLIST_RGBA8_ADLS_BIN,
        KERNELS,
    );

    let built = BuiltProgram::from_artifact(&PROGRAM);
    let kernel = built
        .find_kernel("fill_rect_worklist_rgba8")
        .ok_or(ClError::InvalidKernelName)?;

    let mut memory = BufferRegistry::new();
    let dst = memory.create_buffer(MemFlags::READ_WRITE, 640 * 480 * 4)?;
    let descs = memory.create_buffer(MemFlags::READ_ONLY, 4096)?;

    let mut queue = CommandQueue::new(
        QueueId::from_raw(1),
        ContextId::from_raw(1),
        device,
        QueueProperties::EMPTY,
    );
    queue.enqueue_write_buffer(EventId::from_raw(1), descs, 0, &[], &[])?;
    queue.enqueue_known_kernel(EventId::from_raw(2), kernel.name(), NdRange::new_1d(16), &[])?;
    queue.enqueue_read_buffer(EventId::from_raw(3), dst, 0, 0, &[])?;

    let completed_commands = backend.finish_known_queue(&mut queue)?;
    let fill_rect_uploaded = backend
        .fill_rect_worklist_upload_status()
        .map(|upload| upload.is_ready())
        .unwrap_or(false);

    Ok(KnownAotQueueProbe {
        known_kernels: backend.known_kernel_count(),
        completed_commands,
        fill_rect_uploaded,
    })
}
