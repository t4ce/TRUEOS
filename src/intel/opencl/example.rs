//! Example wiring for the AOT-first OpenCL runtime shape.
//!
//! This is deliberately side-effect-light. It builds the same conceptual path
//! an eventual C ABI would expose, while the backend still owns the real Intel
//! GPGPU submission details.

use super::{
    AccessFlags, BackendCommand, BufferRegistry, BuiltProgram, ClResult, CommandKind, CommandQueue,
    DeviceId, IntelOpenClBackend, KernelArgDesc, KernelArgKind, KernelMetadata, MemFlags, NdRange,
    PlatformId, ProgramArtifact, ProgramBinaryKind, ProgramId, QueueId, QueueProperties,
};

pub(crate) fn fill_rect_worklist_runtime_example() -> ClResult<()> {
    let device = DeviceId::new(0);
    let mut backend = IntelOpenClBackend::new(device);
    let caps = backend.caps();
    if !caps.aot_binaries {
        return Err(super::ClError::Unavailable);
    }

    let program = ProgramArtifact {
        id: ProgramId::new(1),
        name: "trueos.ui.primitives",
        binary_kind: ProgramBinaryKind::IntelGen,
        target: "adls",
        kernels: &[KernelMetadata {
            name: "fill_rect_worklist_rgba8",
            source_name: "fill_rect_worklist_rgba8.cl",
            simd_width: 16,
            cross_thread_bytes: 96,
            per_thread_bytes: 96,
            args: &[
                KernelArgDesc::buffer(0, "dst_rgba8", AccessFlags::WRITE_ONLY),
                KernelArgDesc::buffer(1, "descs", AccessFlags::READ_ONLY),
                KernelArgDesc::scalar(2, "dst_bytes", KernelArgKind::U32),
                KernelArgDesc::scalar(3, "desc_count", KernelArgKind::U32),
            ],
        }],
    };
    let built = BuiltProgram::from_artifact(program)?;
    let kernel = built.create_kernel("fill_rect_worklist_rgba8")?;

    let mut memory = BufferRegistry::new();
    let dst = memory.create_buffer(MemFlags::READ_WRITE, 640 * 480 * 4)?;
    let descs = memory.create_buffer(MemFlags::READ_ONLY, 4096)?;

    let mut queue = CommandQueue::new(
        QueueId::new(1),
        PlatformId::new(0),
        device,
        QueueProperties::IN_ORDER,
    );
    queue.enqueue(CommandKind::WriteBuffer {
        mem: descs,
        offset: 0,
        bytes: 0,
    })?;
    queue.enqueue(CommandKind::Kernel {
        kernel: kernel.id,
        range: NdRange::one_dim(16),
    })?;
    queue.enqueue(CommandKind::ReadBuffer {
        mem: dst,
        offset: 0,
        bytes: 0,
    })?;

    backend.upload_known_kernel("fill_rect_worklist_rgba8")?;
    backend.dispatch(BackendCommand::KnownKernel {
        name: kernel.name,
        range: NdRange::one_dim(16),
    })?;
    queue.finish()?;
    Ok(())
}
