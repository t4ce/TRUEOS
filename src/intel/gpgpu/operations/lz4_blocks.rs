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
include!("../../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lz4_decode_cooperative.contract.rs");
const LZ4_DECODE_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::contracted(
    "lz4_decode_cooperative",
    include_bytes!("../../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lz4_decode_cooperative.bin"),
    include_bytes!("../../../../crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lz4_decode_cooperative.spv"),
    &LZ4_DECODE_COOPERATIVE_ADLS_CPP_ABI_CONTRACT,
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
const LZ4_DECODE_KERNEL_GPU: u64 = 0x0D20_0000;
const LZ4_ARENA_GPU: u64 = 0x1000_0000;
pub(crate) const LZ4_GPU_DECODE_BATCH_BYTES: usize = 16 * 1024 * 1024;
const LZ4_REGION_BYTES: usize = LZ4_GPU_DECODE_BATCH_BYTES;
const LZ4_DESC_OFFSET: usize = 2 * LZ4_REGION_BYTES;
const LZ4_HASH_OFFSET: usize = LZ4_DESC_OFFSET + 8192;
const LZ4_ARENA_BYTES: usize = LZ4_HASH_OFFSET + 256 * 1024;
const _: () = assert!(LZ4_ARENA_GPU + LZ4_ARENA_BYTES as u64 <= DIRECT_RCS_PPGTT_LIMIT_BYTES);
const _: () = assert!(LZ4_KERNEL_GPU + LZ4_BLOCKS_BIN.len() as u64 <= LZ4_DECODE_KERNEL_GPU);
pub(crate) const LZ4_GPU_BATCH_BLOCKS: usize = 256;
static CODEC_RCS_GGTT_MAPPING: spin::Once<bool> = spin::Once::new();
static CODEC_RCS_CONTEXT_QUARANTINED: AtomicBool = AtomicBool::new(false);
static CODEC_RCS_TIMEOUT_POLL_PROBE_LOGGED: AtomicBool = AtomicBool::new(false);
static CODEC_RCS_SUBMIT_RUNTIME: Mutex<DirectRcsSubmitRuntime> =
    Mutex::new(DirectRcsSubmitRuntime::new());
static LZ4_RESOURCES: Mutex<Option<Lz4Resources>> = Mutex::new(None);
static CODEC_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static LZ4_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct Lz4Resources {
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    decode_upload: UploadedKernelArtifact,
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
        && CODEC_RCS_GGTT_MAPPING.get().copied() == Some(true)
}

fn codec_rcs_state_once() -> Option<DirectRcsState> {
    let mut state_slot = CODEC_RCS_STATE.lock();
    if CODEC_RCS_CONTEXT_QUARANTINED.load(Ordering::Acquire) {
        return None;
    }
    if let Some(state) = *state_slot {
        return Some(state);
    }
    let state = allocate_direct_rcs_state(CODEC_RCS_GPU_VA)?;
    *state_slot = Some(state);
    Some(state)
}

fn lz4_resources(dev: super::Dev) -> Result<Lz4Resources, Lz4GpuError> {
    if let Some(resources) = *LZ4_RESOURCES.lock() {
        return Ok(resources);
    }
    // Reuse the exact backing installed by the boot GGTT owner. Allocating a
    // new ring here would leave the immutable control mappings pointing at a
    // different generation even if the readiness check passed.
    let state = codec_rcs_state_once().ok_or(Lz4GpuError::Unavailable)?;
    let upload = upload_ppgtt_resident_artifact(dev, LZ4_BLOCKS_ARTIFACT, LZ4_KERNEL_GPU)
        .ok_or(Lz4GpuError::Unavailable)?;
    let decode_upload = upload_ppgtt_resident_artifact(dev, LZ4_DECODE_ARTIFACT, LZ4_DECODE_KERNEL_GPU)
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
        || !direct_rcs_map_ppgtt_kernel(state, decode_upload.gpu, decode_upload.phys, decode_upload.mapped_bytes)
        || !direct_rcs_map_ppgtt_kernel(state, LZ4_ARENA_GPU, phys, LZ4_ARENA_BYTES)
    {
        // Control mappings cannot be retried using freshly allocated backing.
        quarantine_direct_rcs_lane(DirectRcsLane::Codec, "lz4-initialization-failed");
        return Err(Lz4GpuError::Unavailable);
    }
    let resources = Lz4Resources {
        state,
        upload,
        decode_upload,
        arena,
    };
    *LZ4_RESOURCES.lock() = Some(resources);
    crate::log_info!(target: "gpgpu"; "intel/gpgpu: lz4-resources ready=1 control_mapping=boot-owned ring_phys=0x{:X} ppgtt_phys=0x{:X} arena_bytes={}\n", state.ring_phys, state.ppgtt_phys, LZ4_ARENA_BYTES);
    Ok(resources)
}

/// Batched standard LZ4 block transform. Each tuple is (input, output capacity).
/// Compression accepts at most 64 KiB per block; decompression accepts standard
/// independent blocks up to 4 MiB. Output is read only after exact retirement.
pub(crate) async fn lz4_gpu_blocks(
    blocks: &[(&[u8], usize)],
    encode: bool,
) -> Result<Vec<Vec<u8>>, Lz4GpuError> {
    let mut output = Vec::with_capacity(blocks.len());
    lz4_gpu_blocks_with_output(blocks, encode, |bytes| {
        output.push(bytes.to_vec());
        Ok(())
    }).await?;
    Ok(output)
}

/// Consume retired output directly, avoiding an intermediate allocation and
/// copy for each block in frame decoding. A sink error cannot expose live DMA.
pub(crate) async fn lz4_gpu_blocks_with_output(
    blocks: &[(&[u8], usize)],
    encode: bool,
    mut consume: impl FnMut(&[u8]) -> Result<(), Lz4GpuError>,
) -> Result<(), Lz4GpuError> {
    if blocks.is_empty() {
        return Ok(());
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
    if !direct_rcs_forcewake(dev) {
        crate::log_warn!(target: "gpgpu"; "intel/gpgpu: lz4-failed phase=gt-ready blocks={} encode={}\n", blocks.len(), encode);
        return Err(Lz4GpuError::Submission);
    }
    if !encode_lz4_batch(resources, blocks.len() as u32, u32::from(!encode)) {
        crate::log_warn!(target: "gpgpu"; "intel/gpgpu: lz4-failed phase=batch-encode blocks={} encode={}\n", blocks.len(), encode);
        return Err(Lz4GpuError::Submission);
    }
    let admission_started = direct_rcs_now_tick();
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
                    || direct_rcs_elapsed_ms_since(admission_started) > 2000
                {
                    crate::log_warn!(target: "gpgpu"; "intel/gpgpu: lz4-failed phase=admission blocks={} encode={} elapsed_ms={} quarantined={}\n", blocks.len(), encode, direct_rcs_elapsed_ms_since(admission_started), CODEC_RCS_CONTEXT_QUARANTINED.load(Ordering::Acquire));
                    return Err(Lz4GpuError::Submission);
                }
                trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
            }
        }
    }
    // Admission can spend nearly its entire budget waiting for the broker.
    // Start a separate retirement deadline only after GuC accepted the job.
    let started = direct_rcs_now_tick();
    let admission_ms = direct_rcs_elapsed_ms_since(admission_started);
    loop {
        let observed = direct_rcs_read_result_slot(resources.state, LZ4_POST_MARKER_SLOT);
        let proof = direct_rcs_retirement_proof_on_lane(
            resources.state,
            DirectRcsLane::Codec,
            observed == LZ4_POST_MARKER,
        );
        if proof.complete() {
            complete_direct_rcs_submission_on_lane(DirectRcsLane::Codec);
            admission.inflight = false;
            break;
        }
        if direct_rcs_elapsed_ms_since(started) > 2000 {
            crate::log_warn!(target: "gpgpu"; "intel/gpgpu: lz4-failed phase=retirement blocks={} encode={} input_bytes={} output_capacity={} admission_ms={} elapsed_ms={} pre_marker=0x{:08X} post_marker=0x{:08X} marker_observed={} saved_head={} published_tail={}\n", blocks.len(), encode, input_bytes, output_bytes, admission_ms, direct_rcs_elapsed_ms_since(started), direct_rcs_read_result_slot(resources.state, 0), observed, proof.marker_observed, proof.saved_head_bytes, proof.published_tail_bytes);
            // Diagnostic sampling only: descriptors may still be GPU-owned.
            // These observations never authorize output reads or DMA reuse.
            super::dma_flush(unsafe { resources.arena.add(LZ4_DESC_OFFSET) }, blocks.len() * 24);
            let mut completed = 0;
            let mut first_pending = None;
            for index in 0..blocks.len() {
                let descriptor = unsafe { resources.arena.add(LZ4_DESC_OFFSET + index * 24).cast::<u32>() };
                let status = unsafe { core::ptr::read_volatile(descriptor.add(5)) };
                if status == 0 {
                    completed += 1;
                } else if first_pending.is_none() {
                    first_pending = Some((index, status, unsafe { core::ptr::read_volatile(descriptor.add(4)) }));
                }
            }
            let activity = activity_snapshot();
            let gt = crate::intel::gt_state::read(dev);
            crate::log_warn!(target: "gpgpu"; "intel/gpgpu: lz4-timeout-snapshot completed_blocks={} total_blocks={} first_pending={:?} batch_gpu=0x{:X} acthd=0x{:08X} ring_start=0x{:08X} ring_head=0x{:08X} ring_tail=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} instdone=0x{:08X} instps=0x{:08X} fault=0x{:08X} fault_data0=0x{:08X} fault_data1=0x{:08X} actual_mhz={} requested_mhz={} throttle=0x{:08X} scope=shared-rcs-registers+unretired-descriptor-observations\n", completed, blocks.len(), first_pending, resources.state.gpu_va.batch, activity.acthd, activity.ring_start, activity.ring_head, activity.ring_tail, activity.ipeir, activity.ipehr, activity.eir, activity.instdone, activity.instps, super::mmio_read(dev, 0xCEC4), super::mmio_read(dev, 0xCEB8), super::mmio_read(dev, 0xCEBC), gt.actual_mhz, gt.requested_mhz, gt.throttle_reasons_raw);
            quarantine_direct_rcs_lane(DirectRcsLane::Codec, if proof.marker_observed {
                "lz4-context-save-timeout"
            } else {
                "lz4-marker-timeout"
            });
            // The context and its backing remain pinned by quarantine. Drop
            // must not misreport this timeout as a cancelled future.
            admission.inflight = false;
            return Err(Lz4GpuError::Timeout);
        }
        trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
    }
    let submission = CODEC_RCS_SUBMIT_RUNTIME.lock().submissions;
    if submission <= 4 || submission.is_power_of_two() {
        crate::log_info!(target: "gpgpu"; "intel/gpgpu: lz4-retired submission={} blocks={} encode={} input_bytes={} output_capacity={} admission_ms={} elapsed_ms={}\n", submission, blocks.len(), encode, input_bytes, output_bytes, admission_ms, direct_rcs_elapsed_ms_since(started));
    }
    super::dma_flush(unsafe { resources.arena.add(LZ4_REGION_BYTES) }, output_bytes);
    super::dma_flush(unsafe { resources.arena.add(LZ4_DESC_OFFSET) }, blocks.len() * 24);
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
            crate::log_warn!(target: "gpgpu"; "intel/gpgpu: lz4-failed phase=block-result index={} blocks={} encode={} status={} length={} capacity={} input_bytes={} marker=0x{:X}\n", index, blocks.len(), encode, status, length, capacity, blocks[index].0.len(), direct_rcs_read_result_slot(resources.state, LZ4_POST_MARKER_SLOT));
            return Err(Lz4GpuError::InvalidInput);
        }
        consume(unsafe {
                core::slice::from_raw_parts(resources.arena.add(LZ4_REGION_BYTES + offset), length)
            })?;
        offset += capacity;
    }
    Ok(())
}
