//! Example wiring for the AOT-first OpenCL runtime shape.
//!
//! This is deliberately side-effect-light. It builds the same conceptual path
//! an eventual C ABI would expose, while the backend still owns the real Intel
//! GPGPU submission details.

use super::{
    BackendCommand, BufferRegistry, BuiltProgram, ClError, ClResult, CommandQueue, ContextId,
    DeviceId, EventId, IntelOpenClBackend, KernelArgDesc, KernelArgKind, KernelId, KernelMetadata,
    MemFlags, NdRange, ProgramArtifact, ProgramBinaryKind, QueueId, QueueProperties,
};

pub(crate) fn fill_rect_worklist_runtime_example() -> ClResult<()> {
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
        ProgramBinaryKind::IntelGenBinary,
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
    queue.enqueue_kernel(EventId::from_raw(2), KernelId::from_raw(1), NdRange::new_1d(16), &[])?;
    queue.enqueue_read_buffer(EventId::from_raw(3), dst, 0, 0, &[])?;

    backend.require_known_aot_upload("fill_rect_worklist_rgba8")?;
    let result = backend.dispatch(BackendCommand::ExecuteKnownKernelStub {
        kernel_name: kernel.name(),
        nd_range: NdRange::new_1d(16),
    });
    match result {
        super::backend::BackendCommandResult::ExecuteStub(stub) if stub.recognized => {}
        _ => return Err(ClError::InvalidKernel),
    }
    queue.finish_with(|_| Ok(()))?;
    Ok(())
}
