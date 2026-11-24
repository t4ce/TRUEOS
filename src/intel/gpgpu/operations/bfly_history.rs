use crate::intel::render::BflyHistory;

include!("../artifacts/bfly_history_rgba8_adls.contract.rs");
include!("../artifacts/bfly_smoke_history_rgba8_adls.contract.rs");
include!("../artifacts/bfly_cloud_history_rgba8_adls.contract.rs");
include!("../artifacts/bfly_cloud_trail_rgba8_adls.contract.rs");

// The Bfly windows sit between Spirit's sprite and the CPP demo.
const BFLY_HISTORY_RGBA8_GPU: u64 = 0x0D58_0000;
const BFLY_SMOKE_HISTORY_RGBA8_GPU: u64 = 0x0D59_0000;
const BFLY_SMOKE_R8_GPU: u64 = 0x0D5A_0000;
const BFLY_CLOUD_TRAIL_RGBA8_GPU: u64 = 0x0D5A_4000;
const BFLY_CLOUD_HISTORY_RGBA8_GPU: u64 = 0x0D5B_0000;
const BFLY_CLOUD_R8_GPU: u64 = 0x0D5C_0000;
const BFLY_SMOKE_R8_BYTES: usize = 64 * 128;
const BFLY_CLOUD_R8_BYTES: usize = 512 * 512;
const _: () = {
    assert!(BFLY_SMOKE_R8_GPU + BFLY_SMOKE_R8_BYTES as u64 <= BFLY_CLOUD_TRAIL_RGBA8_GPU);
    assert!(BFLY_CLOUD_R8_GPU + BFLY_CLOUD_R8_BYTES as u64 <= CPP_DEMO_RGBA8_ADLS_GPU);
};

// The exact RPL-S compiler output matches the ADL-S artifact byte for byte.
const BFLY_HISTORY_RPLS_TARGET: GpgpuKernelTarget = GpgpuKernelTarget {
    label: "rpls",
    pci_device_ids: &[0xA780],
    revision_min: 4,
    revision_max: 4,
};
const BFLY_HISTORY_RGBA8_RPLS_CPP_ABI_CONTRACT: GpgpuKernelAbiContract = GpgpuKernelAbiContract {
    target: BFLY_HISTORY_RPLS_TARGET,
    ..BFLY_HISTORY_RGBA8_ADLS_CPP_ABI_CONTRACT
};
const BFLY_SMOKE_HISTORY_RGBA8_RPLS_CPP_ABI_CONTRACT: GpgpuKernelAbiContract =
    GpgpuKernelAbiContract {
        target: BFLY_HISTORY_RPLS_TARGET,
        ..BFLY_SMOKE_HISTORY_RGBA8_ADLS_CPP_ABI_CONTRACT
    };
const BFLY_CLOUD_HISTORY_RGBA8_RPLS_CPP_ABI_CONTRACT: GpgpuKernelAbiContract =
    GpgpuKernelAbiContract {
        target: BFLY_HISTORY_RPLS_TARGET,
        ..BFLY_CLOUD_HISTORY_RGBA8_ADLS_CPP_ABI_CONTRACT
    };
const BFLY_CLOUD_TRAIL_RGBA8_RPLS_CPP_ABI_CONTRACT: GpgpuKernelAbiContract = GpgpuKernelAbiContract {
    target: BFLY_HISTORY_RPLS_TARGET,
    ..BFLY_CLOUD_TRAIL_RGBA8_ADLS_CPP_ABI_CONTRACT
};

static BFLY_HISTORY_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static BFLY_SMOKE_HISTORY_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static BFLY_CLOUD_HISTORY_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static BFLY_CLOUD_TRAIL_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);

struct BflyHistoryR8 {
    source: BflyHistory,
    gpu: u64,
    bytes: usize,
    phys: Mutex<Option<u64>>,
}

static BFLY_SMOKE_R8: BflyHistoryR8 = BflyHistoryR8 {
    source: BflyHistory::Smoke,
    gpu: BFLY_SMOKE_R8_GPU,
    bytes: BFLY_SMOKE_R8_BYTES,
    phys: Mutex::new(None),
};
static BFLY_CLOUD_R8: BflyHistoryR8 = BflyHistoryR8 {
    source: BflyHistory::Cloud,
    gpu: BFLY_CLOUD_R8_GPU,
    bytes: BFLY_CLOUD_R8_BYTES,
    phys: Mutex::new(None),
};

fn upload_bfly_history_rgba8(
    dev: super::Dev,
    style: BflyHistory,
) -> Option<(UploadedKernelArtifact, u64)> {
    let (adls, rpls, cache, gpu, limit) = match style {
        BflyHistory::Monochrome => (
            &BFLY_HISTORY_RGBA8_ADLS_CPP_ABI_CONTRACT,
            &BFLY_HISTORY_RGBA8_RPLS_CPP_ABI_CONTRACT,
            &BFLY_HISTORY_RGBA8_UPLOAD,
            BFLY_HISTORY_RGBA8_GPU,
            BFLY_SMOKE_HISTORY_RGBA8_GPU,
        ),
        BflyHistory::Smoke => (
            &BFLY_SMOKE_HISTORY_RGBA8_ADLS_CPP_ABI_CONTRACT,
            &BFLY_SMOKE_HISTORY_RGBA8_RPLS_CPP_ABI_CONTRACT,
            &BFLY_SMOKE_HISTORY_RGBA8_UPLOAD,
            BFLY_SMOKE_HISTORY_RGBA8_GPU,
            BFLY_SMOKE_R8_GPU,
        ),
        BflyHistory::Cloud => (
            &BFLY_CLOUD_HISTORY_RGBA8_ADLS_CPP_ABI_CONTRACT,
            &BFLY_CLOUD_HISTORY_RGBA8_RPLS_CPP_ABI_CONTRACT,
            &BFLY_CLOUD_HISTORY_RGBA8_UPLOAD,
            BFLY_CLOUD_HISTORY_RGBA8_GPU,
            BFLY_CLOUD_R8_GPU,
        ),
        BflyHistory::CloudTrail => (
            &BFLY_CLOUD_TRAIL_RGBA8_ADLS_CPP_ABI_CONTRACT,
            &BFLY_CLOUD_TRAIL_RGBA8_RPLS_CPP_ABI_CONTRACT,
            &BFLY_CLOUD_TRAIL_RGBA8_UPLOAD,
            BFLY_CLOUD_TRAIL_RGBA8_GPU,
            BFLY_CLOUD_HISTORY_RGBA8_GPU,
        ),
    };
    let contract = match (dev.device_id, dev.revision_id) {
        (0x4680, 0x0C) => adls,
        (0xA780, 0x04) => rpls,
        _ => return None,
    };
    let mut resident = cache.lock();
    if let Some(upload) = *resident {
        return Some((upload, contract.entry_offset));
    }
    let bin = crate::gpu::app_shaders::bytes(style.package_id(), 0)?;
    let mapped_bytes = align_up(bin.len(), super::WARM_ALIGN)?;
    if mapped_bytes as u64 > limit - gpu {
        return None;
    }
    let spv = embedded_shader::spirv(bin);
    let upload = upload_artifact_bytes(
        dev,
        GpgpuKernelArtifact::contracted(contract.kernel_name, &[], &[], contract),
        gpu,
        bin,
        spv,
        "app-package",
        "Blueprint:.trueos.shaders",
        spv.len(),
        GpgpuArtifactAddressSpace::CallerPpgtt,
        None,
    )?;
    *resident = Some(upload);
    Some((upload, contract.entry_offset))
}

fn upload_bfly_history_r8(mask: &BflyHistoryR8) -> Option<u64> {
    let mut resident = mask.phys.lock();
    if let Some(phys) = *resident {
        return Some(phys);
    }
    let package_id = mask.source.package_id();
    let texture = crate::gpu::app_shaders::bytes(package_id, 1)?;
    if texture.len() != mask.bytes {
        return None;
    }
    let (phys, virt) = crate::dma::alloc_ppgtt(mask.bytes, super::WARM_ALIGN)?;
    // The admitted app package is immutable. Initialize this dedicated GPU
    // allocation once and retain it for the lifetime of the kernel upload.
    unsafe { core::ptr::copy_nonoverlapping(texture.as_ptr(), virt, texture.len()) };
    super::dma_flush(virt, texture.len());
    *resident = Some(phys);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: bfly history R8 upload style={:?} package={} bytes={} gpu=0x{:X}\n",
        mask.source, package_id, mask.bytes, mask.gpu,
    );
    Some(phys)
}

/// Decay the previous presented frame into empty pixels in the current frame.
/// Textured modes read their admitted, resident R8 mask.
/// Both surfaces must stay pinned until the GPU retires the submission.
pub(crate) fn bfly_history_rgba8_surface_full(
    previous: GpgpuRgba8Surface,
    current: GpgpuRgba8Surface,
    style: BflyHistory,
) -> GpgpuRgba8KernelResult {
    if !previous.is_valid()
        || !current.is_valid()
        || (previous.width, previous.height) != (current.width, current.height)
        || !previous.pitch_bytes.is_multiple_of(4)
        || !current.pitch_bytes.is_multiple_of(4)
        || gpu_ranges_overlap(previous.phys, previous.bytes, current.phys, current.bytes)
        || gpu_ranges_overlap(previous.gpu, previous.bytes, current.gpu, current.bytes)
    {
        return GpgpuRgba8KernelResult::default();
    }
    // The artifact uses the copy kernel's two-pixels-per-work-item payload.
    let params = CopyRectRgba8Params {
        src_gpu: previous.gpu,
        dst_gpu: current.gpu,
        src_pitch_bytes: previous.pitch_bytes,
        dst_pitch_bytes: current.pitch_bytes,
        width: current.width,
        height: current.height,
        ..Default::default()
    };
    let started = direct_rcs_now_tick();
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some((upload, entry_offset)) = upload_bfly_history_rgba8(dev, style) else {
        return GpgpuRgba8KernelResult::default();
    };
    let mask = match style {
        BflyHistory::Monochrome => None,
        BflyHistory::Smoke => Some(&BFLY_SMOKE_R8),
        BflyHistory::Cloud | BflyHistory::CloudTrail => Some(&BFLY_CLOUD_R8),
    };
    let texture = if let Some(mask) = mask {
        let Some(phys) = upload_bfly_history_r8(mask) else {
            return GpgpuRgba8KernelResult::default();
        };
        Some((mask, phys))
    } else {
        None
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuRgba8KernelResult::default();
    };
    let mapped = direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && direct_rcs_init_ppgtt(state)
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes)
        && direct_rcs_map_ppgtt_scanout(state, previous.gpu, previous.phys, previous.bytes)
        && direct_rcs_map_ppgtt_scanout(state, current.gpu, current.phys, current.bytes);
    let prepared = mapped
        && if let Some((mask, phys)) = texture {
            direct_rcs_map_ppgtt_region_and_publish(
                state,
                mask.gpu,
                phys,
                mask.bytes,
                super::GEN8_PAGE_PRESENT,
            ) && direct_rcs_encode_bfly_textured_history_batch(
                state,
                upload,
                entry_offset,
                params,
                previous.bytes,
                current.bytes,
                mask,
            )
        } else {
            direct_rcs_encode_copy_rect_2d_batch(
                state,
                upload,
                entry_offset,
                params,
                previous.bytes,
                current.bytes,
            )
        };
    let submission = if prepared {
        direct_rcs_submit_batch_state(dev, state)
    } else {
        DirectRcsSubmissionState::Rejected
    };
    let marker = if submission.can_poll() {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            COPY_RECT_POST_MARKER_SLOT,
            COPY_RECT_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = marker == COPY_RECT_POST_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted: submission.may_have_submitted(),
        marker,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(current)),
    }
}
