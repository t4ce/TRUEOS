use super::*;

pub(super) const GPGPU_INTERFACE_DESCRIPTOR_DWORDS: usize = 8;

#[repr(C)]
#[derive(Copy, Clone)]
pub(super) struct GpgpuWalkerCandidate {
    magic: u32,
    version: u32,
    simd_lanes: u32,
    kernel_gpu_lo: u32,
    kernel_gpu_hi: u32,
    kernel_bytes: u32,
    input_a_gpu_lo: u32,
    input_a_gpu_hi: u32,
    input_b_gpu_lo: u32,
    input_b_gpu_hi: u32,
    result_c_gpu_lo: u32,
    result_c_gpu_hi: u32,
    lanes: u32,
    reserved: [u32; 3],
}

pub(super) fn build_gpgpu_interface_descriptor_words(
    program: GpgpuEuProgram,
    store_surface: GpgpuStoreSurfaceState,
    kernel_start_pointer: u64,
) -> [u32; GPGPU_INTERFACE_DESCRIPTOR_DWORDS] {
    const IDD_THREAD_PREEMPTION_DISABLE: u32 = 1 << 20;
    const IDD_ILLEGAL_OPCODE_EXCEPTION_ENABLE: u32 = 1 << 13;
    const IDD_SOFTWARE_EXCEPTION_ENABLE: u32 = 1 << 7;

    let mut idd_words = [0u32; GPGPU_INTERFACE_DESCRIPTOR_DWORDS];
    idd_words[0] = kernel_start_pointer as u32;
    idd_words[1] = (kernel_start_pointer >> 32) as u32;
    idd_words[2] = IDD_THREAD_PREEMPTION_DISABLE
        | if GPGPU_ENABLE_SIP_EXCEPTIONS {
            IDD_ILLEGAL_OPCODE_EXCEPTION_ENABLE | IDD_SOFTWARE_EXCEPTION_ENABLE
        } else {
            0
        };
    idd_words[3] = 0;
    idd_words[4] = if program.expects_store && store_surface.ready {
        (store_surface.binding_table_offset as u32) | 31
    } else {
        0
    };
    idd_words[5] = super::payload::gpgpu_curbe_read_length_8dw() << 16;
    idd_words[6] = GPGPU_WALKER_GROUP_THREADS;
    idd_words[7] = 0;
    idd_words
}

pub(super) fn encode_gpgpu_walker_candidate(
    warm: RenderWarmState,
    kernel_gpu: u64,
    kernel_bytes: u32,
) -> bool {
    let candidate = GpgpuWalkerCandidate {
        magic: 0x4750_4757,
        version: 1,
        simd_lanes: 8,
        kernel_gpu_lo: kernel_gpu as u32,
        kernel_gpu_hi: (kernel_gpu >> 32) as u32,
        kernel_bytes,
        input_a_gpu_lo: GPU_VA_VERTEX_BASE as u32,
        input_a_gpu_hi: (GPU_VA_VERTEX_BASE >> 32) as u32,
        input_b_gpu_lo: GPU_VA_STREAMOUT_BASE as u32,
        input_b_gpu_hi: (GPU_VA_STREAMOUT_BASE >> 32) as u32,
        result_c_gpu_lo: GPU_VA_RESULT_BASE as u32,
        result_c_gpu_hi: (GPU_VA_RESULT_BASE >> 32) as u32,
        lanes: GPGPU_PREFLIGHT_LANES as u32,
        reserved: [0; 3],
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            core::ptr::addr_of!(candidate) as *const u8,
            warm.batch_virt.add(GPGPU_WALKER_SCRATCH_OFFSET_BYTES),
            core::mem::size_of::<GpgpuWalkerCandidate>(),
        );
    }
    crate::intel::dma_flush(
        unsafe { warm.batch_virt.add(GPGPU_WALKER_SCRATCH_OFFSET_BYTES) },
        core::mem::size_of::<GpgpuWalkerCandidate>(),
    );
    true
}
