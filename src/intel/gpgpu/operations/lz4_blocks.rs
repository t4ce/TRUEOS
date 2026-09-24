// Reusable buffer codec, deliberately independent of files, paths and archives.
include!(
    "../../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lz4_blocks.contract.rs"
);
const LZ4_BLOCKS_BIN: &[u8] = include_bytes!(
    "../../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lz4_blocks.bin"
);
const LZ4_BLOCKS_SPV: &[u8] = include_bytes!(
    "../../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lz4_blocks.spv"
);
const LZ4_BLOCKS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::contracted(
    "lz4_blocks",
    LZ4_BLOCKS_BIN,
    LZ4_BLOCKS_SPV,
    &LZ4_BLOCKS_ADLS_CPP_ABI_CONTRACT,
);
const CODEC_RCS_GPU_VA_RING_BASE: u64 = 0x0860_0000;
const CODEC_RCS_GPU_VA: DirectRcsGpuVa = DirectRcsGpuVa {
    ring: CODEC_RCS_GPU_VA_RING_BASE,
    context: 0x0861_0000,
    result: 0x0864_0000,
    batch: 0x0870_0000,
    job_slots: 1,
    map_general_auxiliary: false,
};
const _: () = assert!(
    FONT_RCS_GPU_VA_BATCH_BASE + DIRECT_RCS_BATCH_BYTES as u64 <= CODEC_RCS_GPU_VA_RING_BASE
);
const _: () = assert!(
    CODEC_RCS_GPU_VA.batch + DIRECT_RCS_BATCH_BYTES as u64 <= DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE
);
const LZ4_KERNEL_GPU: u64 = 0x0D00_0000; // private codec PPGTT
const LZ4_ARENA_GPU: u64 = 0x1000_0000;
const LZ4_REGION_BYTES: usize = 5 * 1024 * 1024;
const LZ4_DESC_OFFSET: usize = 2 * LZ4_REGION_BYTES;
const LZ4_HASH_OFFSET: usize = LZ4_DESC_OFFSET + 8192;
const LZ4_ARENA_BYTES: usize = LZ4_HASH_OFFSET + 256 * 1024;
const _: () = assert!(LZ4_ARENA_GPU + LZ4_ARENA_BYTES as u64 <= DIRECT_RCS_PPGTT_LIMIT_BYTES);
const _: () = assert!(LZ4_KERNEL_GPU + LZ4_BLOCKS_BIN.len() as u64 <= LZ4_ARENA_GPU);
pub(crate) const LZ4_GPU_BATCH_BLOCKS: usize = 256;
static CODEC_RCS_GGTT_MAPPING: spin::Once<bool> = spin::Once::new();
static CODEC_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
static CODEC_RCS_TIMEOUT_POLL_PROBE_LOGGED: AtomicBool = AtomicBool::new(false);
static CODEC_RCS_SUBMIT_RUNTIME: Mutex<DirectRcsSubmitRuntime> =
    Mutex::new(DirectRcsSubmitRuntime::new());
static LZ4_RESOURCES: Mutex<Option<Lz4Resources>> = Mutex::new(None);
static LZ4_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct Lz4Resources {
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    arena: *mut u8,
}
unsafe impl Send for Lz4Resources {}
unsafe impl Sync for Lz4Resources {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Lz4GpuError {
    Unavailable,
    InvalidInput,
    Capacity,
    Submission,
    Timeout,
}

// RAII admission is async-compatible. No spin guard survives an await. An
// abandoned in-flight operation quarantines its private context and retains
// every DMA allocation, so a late GPU cannot overwrite the next caller.
struct Lz4Admission {
    inflight: bool,
}
impl Drop for Lz4Admission {
    fn drop(&mut self) {
        if self.inflight {
            quarantine_direct_rcs_lane(DirectRcsLane::Codec, "lz4-waiter-dropped-inflight");
        }
        LZ4_ACTIVE.store(false, Ordering::Release);
    }
}

pub(crate) fn lz4_gpu_available() -> bool {
    super::claimed_device().is_some_and(|dev| {
        LZ4_BLOCKS_ARTIFACT
            .target_policy
            .supports(dev.device_id, dev.revision_id)
    }) && !CODEC_RCS_CONTEXT_QUARANTINED.load(Ordering::Acquire)
}

fn lz4_resources(dev: super::Dev) -> Result<Lz4Resources, Lz4GpuError> {
    if let Some(resources) = *LZ4_RESOURCES.lock() {
        return Ok(resources);
    }
    let state = allocate_direct_rcs_state(CODEC_RCS_GPU_VA).ok_or(Lz4GpuError::Unavailable)?;
    let upload = upload_ppgtt_resident_artifact(dev, LZ4_BLOCKS_ARTIFACT, LZ4_KERNEL_GPU)
        .ok_or(Lz4GpuError::Unavailable)?;
    let (phys, arena) = crate::dma::alloc(LZ4_ARENA_BYTES, 4096).ok_or(Lz4GpuError::Unavailable)?;
    unsafe {
        core::ptr::write_bytes(arena, 0, LZ4_ARENA_BYTES);
    }
    super::dma_flush(arena, LZ4_ARENA_BYTES);
    if !direct_rcs_forcewake(dev)
        || !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
        || !direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        || !direct_rcs_map_ppgtt_kernel(state, LZ4_ARENA_GPU, phys, LZ4_ARENA_BYTES)
    {
        // Control mappings cannot be retried using freshly allocated backing.
        quarantine_direct_rcs_lane(DirectRcsLane::Codec, "lz4-initialization-failed");
        return Err(Lz4GpuError::Unavailable);
    }
    let resources = Lz4Resources {
        state,
        upload,
        arena,
    };
    *LZ4_RESOURCES.lock() = Some(resources);
    Ok(resources)
}

/// Batched standard LZ4 block transform. Each tuple is (input, output capacity).
/// Compression accepts at most 64 KiB per block; decompression accepts standard
/// independent blocks up to 4 MiB. Output is read only after exact retirement.
pub(crate) async fn lz4_gpu_blocks(
    blocks: &[(&[u8], usize)],
    encode: bool,
) -> Result<Vec<Vec<u8>>, Lz4GpuError> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }
    if blocks.len() > LZ4_GPU_BATCH_BLOCKS {
        return Err(Lz4GpuError::Capacity);
    }
    let mut input_bytes = 0usize;
    let mut output_bytes = 0usize;
    for &(input, capacity) in blocks {
        if input.len() > 4 * 1024 * 1024
            || capacity > 4 * 1024 * 1024 + 16464
            || (encode && (input.len() > 65536 || capacity < input.len() + input.len() / 255 + 16))
        {
            return Err(Lz4GpuError::InvalidInput);
        }
        input_bytes = input_bytes
            .checked_add(input.len())
            .ok_or(Lz4GpuError::Capacity)?;
        output_bytes = output_bytes
            .checked_add(capacity)
            .ok_or(Lz4GpuError::Capacity)?;
    }
    if input_bytes > LZ4_REGION_BYTES || output_bytes > LZ4_REGION_BYTES {
        return Err(Lz4GpuError::Capacity);
    }
    while LZ4_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
    }
    let mut admission = Lz4Admission { inflight: false };
    if !lz4_gpu_available() {
        return Err(Lz4GpuError::Unavailable);
    }
    let dev = super::claimed_device().ok_or(Lz4GpuError::Unavailable)?;
    let resources = match lz4_resources(dev) {
        Ok(resources) => resources,
        Err(error) => {
            quarantine_direct_rcs_lane(DirectRcsLane::Codec, "lz4-resource-initialization-failed");
            return Err(error);
        }
    };
    let mut src_offset = 0usize;
    let mut dst_offset = 0usize;
    for (index, &(input, capacity)) in blocks.iter().enumerate() {
        let descriptor = [
            src_offset as u32,
            input.len() as u32,
            dst_offset as u32,
            capacity as u32,
            0,
            1,
        ];
        unsafe {
            core::ptr::copy_nonoverlapping(
                input.as_ptr(),
                resources.arena.add(src_offset),
                input.len(),
            );
            core::ptr::copy_nonoverlapping(
                descriptor.as_ptr(),
                resources.arena.add(LZ4_DESC_OFFSET + index * 24).cast(),
                6,
            );
        }
        src_offset += input.len();
        dst_offset += capacity;
    }
    super::dma_flush(resources.arena, input_bytes);
    super::dma_flush(unsafe { resources.arena.add(LZ4_DESC_OFFSET) }, blocks.len() * 24);
    if !direct_rcs_forcewake(dev)
        || !encode_lz4_batch(resources, blocks.len() as u32, u32::from(!encode))
    {
        return Err(Lz4GpuError::Submission);
    }
    let started = direct_rcs_now_tick();
    loop {
        match direct_rcs_submit_batch_on_lane_state(dev, resources.state, DirectRcsLane::Codec) {
            DirectRcsSubmissionState::Submitted => {
                admission.inflight = true;
                break;
            }
            DirectRcsSubmissionState::Ambiguous => {
                admission.inflight = true;
                return Err(Lz4GpuError::Submission);
            }
            DirectRcsSubmissionState::Rejected => {
                if CODEC_RCS_CONTEXT_QUARANTINED.load(Ordering::Acquire)
                    || direct_rcs_elapsed_ms_since(started) > 2000
                {
                    return Err(Lz4GpuError::Submission);
                }
                trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
            }
        }
    }
    loop {
        let observed = direct_rcs_read_result_slot(resources.state, 1);
        if direct_rcs_retirement_proof_on_lane(
            resources.state,
            DirectRcsLane::Codec,
            observed == LZ4_POST_MARKER,
        )
        .complete()
        {
            complete_direct_rcs_submission_on_lane(DirectRcsLane::Codec);
            admission.inflight = false;
            break;
        }
        if direct_rcs_elapsed_ms_since(started) > 2000 {
            return Err(Lz4GpuError::Timeout);
        }
        trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
    }
    super::dma_flush(unsafe { resources.arena.add(LZ4_REGION_BYTES) }, output_bytes);
    super::dma_flush(unsafe { resources.arena.add(LZ4_DESC_OFFSET) }, blocks.len() * 24);
    let mut output = Vec::with_capacity(blocks.len());
    let mut offset = 0;
    for (index, &(_, capacity)) in blocks.iter().enumerate() {
        let descriptor = unsafe {
            resources
                .arena
                .add(LZ4_DESC_OFFSET + index * 24)
                .cast::<u32>()
        };
        let length = unsafe { core::ptr::read_volatile(descriptor.add(4)) } as usize;
        let status = unsafe { core::ptr::read_volatile(descriptor.add(5)) };
        if status != 0 || length > capacity {
            return Err(Lz4GpuError::InvalidInput);
        }
        output.push(
            unsafe {
                core::slice::from_raw_parts(resources.arena.add(LZ4_REGION_BYTES + offset), length)
            }
            .to_vec(),
        );
        offset += capacity;
    }
    Ok(output)
}
