use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::Mutex;

pub(crate) const COPY_RECT_RGBA8_KERNEL_NAME: &str = "copy_rect_rgba8";
pub(crate) const COPY_RECT_RGBA8_OPENCL_SOURCE: &str = include_str!("kernels/copy_rect_rgba8.cl");
pub(crate) const CLEAR_RECT_RGBA8_WHITE_KERNEL_NAME: &str = "clear_rect_rgba8_white";
pub(crate) const CLEAR_RECT_RGBA8_WHITE_OPENCL_SOURCE: &str =
    include_str!("kernels/clear_rect_rgba8_white.cl");
pub(crate) const EMPTY_EOT_KERNEL_NAME: &str = "empty_eot";
pub(crate) const EMPTY_EOT_OPENCL_SOURCE: &str = include_str!("kernels/empty_eot.cl");
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.bin");
pub(crate) const COPY_RECT_RGBA8_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/copy_rect_rgba8.spv");
pub(crate) const CLEAR_RECT_RGBA8_WHITE_ADLS_BIN: &[u8] =
    include_bytes!("kernels/artifacts/adls/clear_rect_rgba8_white.bin");
pub(crate) const CLEAR_RECT_RGBA8_WHITE_ADLS_SPV: &[u8] =
    include_bytes!("kernels/artifacts/adls/clear_rect_rgba8_white.spv");
pub(crate) const EMPTY_EOT_ADLS_BIN: &[u8] = include_bytes!("kernels/artifacts/adls/empty_eot.bin");
pub(crate) const EMPTY_EOT_ADLS_SPV: &[u8] = include_bytes!("kernels/artifacts/adls/empty_eot.spv");
pub(crate) const COPY_RECT_RGBA8_ADLS_BIN_SHA256: [u8; 32] = [
    0xC6, 0x8C, 0xD7, 0x18, 0xDC, 0xD2, 0x24, 0x1D, 0xB1, 0xD0, 0xF4, 0x4D, 0xB5, 0x4B, 0x7B, 0x9B,
    0x1C, 0x70, 0x7A, 0xA8, 0x9C, 0x52, 0x4E, 0xDD, 0xC8, 0xBD, 0x2B, 0x9F, 0x69, 0x78, 0xE2, 0x49,
];
pub(crate) const CLEAR_RECT_RGBA8_WHITE_ADLS_BIN_SHA256: [u8; 32] = [
    0x96, 0xB9, 0x6D, 0x64, 0x58, 0xBA, 0xDA, 0x38, 0x28, 0xB5, 0xE0, 0x5D, 0xD0, 0xD4, 0xDE, 0xCE,
    0xD6, 0x11, 0x93, 0xCE, 0x33, 0x6F, 0xEC, 0x2F, 0x7C, 0xE0, 0xBC, 0xF0, 0xDF, 0x4D, 0x57, 0xC0,
];
pub(crate) const EMPTY_EOT_ADLS_BIN_SHA256: [u8; 32] = [
    0x72, 0x73, 0x17, 0x3D, 0xC0, 0xE3, 0xDE, 0x30, 0xED, 0x9B, 0xFA, 0x28, 0xC9, 0x03, 0xD6, 0xDB,
    0xAF, 0x49, 0x42, 0xF2, 0xF1, 0xAD, 0x1F, 0x20, 0xCC, 0xA3, 0x19, 0xCB, 0xFD, 0xD1, 0x4E, 0xAC,
];

const COPY_RECT_RGBA8_ADLS_GPU: u64 = 0x0D20_0000;
const CLEAR_RECT_RGBA8_WHITE_ADLS_GPU: u64 = 0x0D21_0000;
const EMPTY_EOT_ADLS_GPU: u64 = 0x0D22_0000;
const COPY_RECT_RGBA8_TEXT_OFFSET_BYTES: u64 = 0x40;
const EMPTY_EOT_TEXT_OFFSET_BYTES: u64 = 0x40;

const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_RING_TAIL: usize = RCS_RING_BASE + 0x30;
const RCS_RING_HEAD: usize = RCS_RING_BASE + 0x34;
const RCS_RING_ACTHD: usize = RCS_RING_BASE + 0x74;
const RCS_RING_IPEIR: usize = RCS_RING_BASE + 0x64;
const RCS_RING_IPEHR: usize = RCS_RING_BASE + 0x68;
const RCS_RING_EIR: usize = RCS_RING_BASE + 0xB0;
const RCS_RING_MI_MODE: usize = RCS_RING_BASE + 0x9C;
const RCS_RING_MODE_GEN7: usize = RCS_RING_BASE + 0x29C;
const RCS_RING_CONTEXT_CONTROL: usize = RCS_RING_BASE + 0x244;
const RCS_RING_CONTEXT_CONTROL_REF: usize = RCS_RING_BASE + 0x5A0;
const RCS_RING_EXECLIST_CONTROL: usize = RCS_RING_BASE + 0x550;
const RCS_RING_EXECLIST_SQ_LO: usize = RCS_RING_BASE + 0x510;
const RCS_RING_EXECLIST_SQ_HI: usize = RCS_RING_BASE + 0x514;
const RCS_RING_HWS_PGA: usize = RCS_RING_BASE + 0x80;
const RCS_CS_DEBUG_MODE1: usize = RCS_RING_BASE + 0xEC;
const GEN12_RCU_MODE: usize = 0x14800;
const GEN12_RCU_MODE_CCS_ENABLE: u32 = 1 << 0;
const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const FF_DOP_CLOCK_GATE_DISABLE: u32 = 1 << 1;
const RING_VALID: u32 = 1;
const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
const RING_MI_MODE_STOP_RING: u32 = 1 << 8;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_GTT: u32 = 2 << 6;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GEN8_PAGE_RW: u64 = 1 << 1;
const GEN8_PAGE_PWT: u64 = 1 << 3;
const GEN8_PAGE_PCD: u64 = 1 << 4;
const GEN8_CTX_VALID: u32 = 1 << 0;
const GEN8_CTX_PPGTT_ENABLE: u32 = 1 << 5;
const GEN8_CTX_PRIVILEGE: u32 = 1 << 8;
const GEN12_CTX_PRIORITY_NORMAL: u32 = 1 << 9;
const GEN8_CTX_ADDRESSING_MODE_SHIFT: u32 = 3;
const RENDER_MOCS: u32 = 4;
const PIPE_CONTROL_CMD: u32 = 4 | (2 << 24) | (3 << 27) | (3 << 29);
const STATE_BASE_ADDRESS_CMD: u32 = 20 | (1 << 16) | (1 << 24) | (3 << 29);
const PIPE_CONTROL_DC_FLUSH_ENABLE: u32 = 1 << 5;
const PIPE_CONTROL_FLUSH_ENABLE: u32 = 1 << 7;
const PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH: u32 = 1 << 12;
const PIPE_CONTROL_CS_STALL: u32 = 1 << 20;
const PIPE_CONTROL_FLUSH_HDC: u32 = 1 << 26;
const PIPE_CONTROL_FLUSH_BITS: u32 = PIPE_CONTROL_DC_FLUSH_ENABLE
    | PIPE_CONTROL_FLUSH_ENABLE
    | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH
    | PIPE_CONTROL_CS_STALL
    | PIPE_CONTROL_FLUSH_HDC;
const PIPE_CONTROL_INVALIDATE_BITS: u32 =
    PIPE_CONTROL_FLUSH_BITS | (1 << 8) | (1 << 10) | (1 << 11) | (1 << 13);
const MEDIA_VFE_STATE_CMD: u32 = (3 << 29) | (2 << 27) | 7;
const MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 16) | 2;
const GPGPU_WALKER_CMD: u32 = (3 << 29) | (2 << 27) | (1 << 24) | (5 << 16) | 13;
const MEDIA_STATE_FLUSH_CMD: u32 = (3 << 29) | (2 << 27) | (4 << 16);
const PIPELINE_SELECT_BASE: u32 = (3 << 29) | (1 << 27) | (1 << 24) | (4 << 16);
const PIPELINE_SELECT_GFX12_MASK: u32 = 0x13 << 8;
const PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE: u32 = 1 << 4;
const PIPELINE_SELECT_3D: u32 =
    PIPELINE_SELECT_BASE | PIPELINE_SELECT_GFX12_MASK | PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE;
const PIPELINE_SELECT_GPGPU: u32 = PIPELINE_SELECT_3D | 2;
const IDD_THREAD_PREEMPTION_DISABLE: u32 = 1 << 20;
const GPGPU_VFE_DW3_UOS: u32 = 0x00A7_0100;
const GPGPU_VFE_DW5_UOS: u32 = 0x0782_0000;
const GPGPU_WALKER_GROUP_THREADS: u32 = 1;
const GPGPU_WALKER_SIMD16_SELECT: u32 = 1;
const GPGPU_WALKER_GROUP_Z_DIM: u32 = 1;
const GPGPU_WALKER_SIMD16_MASK: u32 = 0x0000_FFFF;
const GPGPU_WALKER_BOTTOM_MASK: u32 = 0xFFFF_FFFF;
const EMPTY_EOT_IDD_OFFSET_BYTES: usize = 0x300;
const EMPTY_EOT_IDD_BYTES: usize = 8 * core::mem::size_of::<u32>();
const EMPTY_EOT_PRE_MARKER_SLOT: usize = 1;
const EMPTY_EOT_POST_MARKER_SLOT: usize = 0;
const EMPTY_EOT_PRE_MARKER: u32 = 0xC0DE_E701;
const EMPTY_EOT_POST_MARKER: u32 = 0xC0DE_E702;

const DIRECT_RCS_ENABLED: bool = true;
const DIRECT_RCS_RING_BYTES: usize = 4096;
const DIRECT_RCS_CONTEXT_BYTES: usize = 22 * 4096;
const DIRECT_RCS_BATCH_BYTES: usize = 4096;
const DIRECT_RCS_RESULT_BYTES: usize = 4096;
const DIRECT_RCS_PPGTT_PT_COUNT: usize = 128;
const DIRECT_RCS_PPGTT_BYTES: usize = (3 + DIRECT_RCS_PPGTT_PT_COUNT) * 4096;
const DIRECT_RCS_LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const DIRECT_RCS_BATCH_START_DWORDS: usize = 4;
const DIRECT_RCS_GPU_VA_RING_BASE: u64 = 0x0080_0000;
const DIRECT_RCS_GPU_VA_CONTEXT_BASE: u64 = 0x0081_0000;
const DIRECT_RCS_GPU_VA_RESULT_BASE: u64 = 0x0084_0000;
const DIRECT_RCS_GPU_VA_BATCH_BASE: u64 = 0x0180_0000;
const DIRECT_RCS_SMOKE_MARKER: u32 = 0xC0DE_5101;
const DIRECT_RCS_SMOKE_POLL_ITERS: usize = 262_144;

static COPY_RECT_RGBA8_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static CLEAR_RECT_RGBA8_WHITE_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static EMPTY_EOT_UPLOAD: Mutex<Option<UploadedKernelArtifact>> = Mutex::new(None);
static DIRECT_RCS_STATE: Mutex<Option<DirectRcsState>> = Mutex::new(None);
static DIRECT_RCS_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
static EMPTY_EOT_WALKER_RAN: AtomicBool = AtomicBool::new(false);
static DIRECT_RCS_SUBMIT_COUNTER: AtomicU32 = AtomicU32::new(0);
static DIRECT_RCS_RING_KICK_DWORD: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct CopyRectRgba8Params {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) src_x: u32,
    pub(crate) src_y: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct ClearRectRgba8WhiteParams {
    pub(crate) dst_gpu: u64,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuKernelArtifact {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) bin: &'static [u8],
    pub(crate) spv: &'static [u8],
    pub(crate) bin_sha256: [u8; 32],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct UploadedKernelArtifact {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) virt: *mut u8,
    pub(crate) bytes: usize,
    pub(crate) mapped_bytes: usize,
    pub(crate) verified: bool,
}

unsafe impl Send for UploadedKernelArtifact {}
unsafe impl Sync for UploadedKernelArtifact {}

pub(crate) const COPY_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: COPY_RECT_RGBA8_KERNEL_NAME,
    target: "adls",
    bin: COPY_RECT_RGBA8_ADLS_BIN,
    spv: COPY_RECT_RGBA8_ADLS_SPV,
    bin_sha256: COPY_RECT_RGBA8_ADLS_BIN_SHA256,
};

pub(crate) const CLEAR_RECT_RGBA8_WHITE_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: CLEAR_RECT_RGBA8_WHITE_KERNEL_NAME,
    target: "adls",
    bin: CLEAR_RECT_RGBA8_WHITE_ADLS_BIN,
    spv: CLEAR_RECT_RGBA8_WHITE_ADLS_SPV,
    bin_sha256: CLEAR_RECT_RGBA8_WHITE_ADLS_BIN_SHA256,
};

pub(crate) const EMPTY_EOT_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact {
    name: EMPTY_EOT_KERNEL_NAME,
    target: "adls",
    bin: EMPTY_EOT_ADLS_BIN,
    spv: EMPTY_EOT_ADLS_SPV,
    bin_sha256: EMPTY_EOT_ADLS_BIN_SHA256,
};

pub(crate) fn copy_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *COPY_RECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn clear_rect_rgba8_white_upload_status() -> Option<UploadedKernelArtifact> {
    *CLEAR_RECT_RGBA8_WHITE_UPLOAD.lock()
}

pub(crate) fn empty_eot_upload_status() -> Option<UploadedKernelArtifact> {
    *EMPTY_EOT_UPLOAD.lock()
}

pub(crate) fn upload_copy_rect_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *COPY_RECT_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, COPY_RECT_RGBA8_ADLS_ARTIFACT, COPY_RECT_RGBA8_ADLS_GPU)?;
    *COPY_RECT_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_clear_rect_rgba8_white_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CLEAR_RECT_RGBA8_WHITE_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: clear-rect-rgba8-white upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CLEAR_RECT_RGBA8_WHITE_ADLS_ARTIFACT,
        CLEAR_RECT_RGBA8_WHITE_ADLS_GPU,
    )?;
    *CLEAR_RECT_RGBA8_WHITE_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_empty_eot_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *EMPTY_EOT_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: empty-eot upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, EMPTY_EOT_ADLS_ARTIFACT, EMPTY_EOT_ADLS_GPU)?;
    *EMPTY_EOT_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn submit_direct_rcs_smoke_once() -> bool {
    if !DIRECT_RCS_ENABLED || DIRECT_RCS_SMOKE_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs-smoke skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(upload) = copy_rect_rgba8_upload_status() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs-smoke skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: direct-rcs-smoke failed rung=alloc\n"
        );
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let batch_ok = ppgtt_ok && direct_rcs_encode_smoke_batch(state);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result(state, DIRECT_RCS_SMOKE_MARKER)
    } else {
        0
    };
    let retired = observed == DIRECT_RCS_SMOKE_MARKER;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: direct-rcs-smoke forcewake={} ggtt={} ppgtt={} batch={} submitted={} retired={} observed=0x{:08X} expected=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=gpgpu-walker\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        observed,
        DIRECT_RCS_SMOKE_MARKER,
        upload.gpu,
        upload.gpu + COPY_RECT_RGBA8_TEXT_OFFSET_BYTES,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        super::mmio_read(dev, RCS_RING_HEAD),
        super::mmio_read(dev, RCS_RING_TAIL),
        super::mmio_read(dev, RCS_RING_ACTHD),
        super::mmio_read(dev, RCS_RING_IPEIR),
        super::mmio_read(dev, RCS_RING_IPEHR),
        super::mmio_read(dev, RCS_RING_EIR),
    );

    retired
}

pub(crate) fn submit_empty_eot_walker_once() -> bool {
    if !DIRECT_RCS_ENABLED || EMPTY_EOT_WALKER_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: empty-eot-walker skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(upload) = upload_empty_eot_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: empty-eot-walker skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: empty-eot-walker failed rung=alloc\n"
        );
        return false;
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let batch_ok = kernel_ppgtt_ok && direct_rcs_encode_empty_eot_walker_batch(state, upload);
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot(state, EMPTY_EOT_POST_MARKER_SLOT, EMPTY_EOT_POST_MARKER)
    } else {
        0
    };
    let pre_marker = direct_rcs_read_result_slot(state, EMPTY_EOT_PRE_MARKER_SLOT);
    let retired = observed == EMPTY_EOT_POST_MARKER;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: empty-eot-walker forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} batch={} submitted={} retired={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} idd_off=0x{:X} simd=16 groups=1 threads_per_group={} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=clear-rect-writes\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        pre_marker,
        observed,
        EMPTY_EOT_POST_MARKER,
        upload.gpu,
        upload.gpu + EMPTY_EOT_TEXT_OFFSET_BYTES,
        EMPTY_EOT_IDD_OFFSET_BYTES,
        GPGPU_WALKER_GROUP_THREADS,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        super::mmio_read(dev, RCS_RING_HEAD),
        super::mmio_read(dev, RCS_RING_TAIL),
        super::mmio_read(dev, RCS_RING_ACTHD),
        super::mmio_read(dev, RCS_RING_IPEIR),
        super::mmio_read(dev, RCS_RING_IPEHR),
        super::mmio_read(dev, RCS_RING_EIR),
    );

    retired
}

fn upload_artifact(
    dev: super::Dev,
    artifact: GpgpuKernelArtifact,
    gpu: u64,
) -> Option<UploadedKernelArtifact> {
    let mapped_bytes = align_up(artifact.bin.len(), super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(mapped_bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, mapped_bytes);
        core::ptr::copy_nonoverlapping(artifact.bin.as_ptr(), virt, artifact.bin.len());
    }
    super::dma_flush(virt, mapped_bytes);

    let uploaded = unsafe { core::slice::from_raw_parts(virt, artifact.bin.len()) };
    let verified = uploaded == artifact.bin;
    if !verified {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=verify phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            artifact.name,
            phys,
            gpu,
            artifact.bin.len()
        );
        crate::dma::dealloc(virt, mapped_bytes);
        return None;
    }

    if !super::map_ggtt(dev, phys, mapped_bytes, gpu) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} upload failed reason=ggtt-map phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            artifact.name,
            phys,
            gpu,
            mapped_bytes
        );
        crate::dma::dealloc(virt, mapped_bytes);
        return None;
    }
    super::ggtt_invalidate(dev);

    let upload = UploadedKernelArtifact {
        name: artifact.name,
        target: artifact.target,
        gpu,
        phys,
        virt,
        bytes: artifact.bin.len(),
        mapped_bytes,
        verified,
    };
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: {} upload ok=1 target={} phys=0x{:X} gpu=0x{:X} bytes=0x{:X} mapped=0x{:X} sha256={:02X}{:02X}{:02X}{:02X}...\n",
        artifact.name,
        artifact.target,
        upload.phys,
        upload.gpu,
        upload.bytes,
        upload.mapped_bytes,
        artifact.bin_sha256[0],
        artifact.bin_sha256[1],
        artifact.bin_sha256[2],
        artifact.bin_sha256[3],
    );
    Some(upload)
}

#[derive(Copy, Clone, Debug)]
struct DirectRcsState {
    ring_phys: u64,
    ring_virt: *mut u8,
    context_phys: u64,
    context_virt: *mut u8,
    batch_phys: u64,
    batch_virt: *mut u8,
    result_phys: u64,
    result_virt: *mut u8,
    ppgtt_phys: u64,
    ppgtt_virt: *mut u8,
}

unsafe impl Send for DirectRcsState {}
unsafe impl Sync for DirectRcsState {}

fn direct_rcs_state_once(_dev: super::Dev) -> Option<DirectRcsState> {
    if let Some(state) = *DIRECT_RCS_STATE.lock() {
        return Some(state);
    }

    let (ring_phys, ring_virt) = crate::dma::alloc(DIRECT_RCS_RING_BYTES, super::WARM_ALIGN)?;
    let (context_phys, context_virt) =
        crate::dma::alloc(DIRECT_RCS_CONTEXT_BYTES, super::WARM_ALIGN)?;
    let (batch_phys, batch_virt) = crate::dma::alloc(DIRECT_RCS_BATCH_BYTES, super::WARM_ALIGN)?;
    let (result_phys, result_virt) = crate::dma::alloc(DIRECT_RCS_RESULT_BYTES, super::WARM_ALIGN)?;
    let (ppgtt_phys, ppgtt_virt) = crate::dma::alloc(DIRECT_RCS_PPGTT_BYTES, super::WARM_ALIGN)?;

    unsafe {
        core::ptr::write_bytes(ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(context_virt, 0, DIRECT_RCS_CONTEXT_BYTES);
        core::ptr::write_bytes(batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(result_virt, 0, DIRECT_RCS_RESULT_BYTES);
        core::ptr::write_bytes(ppgtt_virt, 0, DIRECT_RCS_PPGTT_BYTES);
    }

    let state = DirectRcsState {
        ring_phys,
        ring_virt,
        context_phys,
        context_virt,
        batch_phys,
        batch_virt,
        result_phys,
        result_virt,
        ppgtt_phys,
        ppgtt_virt,
    };
    *DIRECT_RCS_STATE.lock() = Some(state);
    Some(state)
}

fn direct_rcs_map_state(dev: super::Dev, state: DirectRcsState) -> bool {
    let mapped =
        super::map_ggtt(dev, state.ring_phys, DIRECT_RCS_RING_BYTES, DIRECT_RCS_GPU_VA_RING_BASE)
            && super::map_ggtt(
                dev,
                state.context_phys,
                DIRECT_RCS_CONTEXT_BYTES,
                DIRECT_RCS_GPU_VA_CONTEXT_BASE,
            )
            && super::map_ggtt(
                dev,
                state.batch_phys,
                DIRECT_RCS_BATCH_BYTES,
                DIRECT_RCS_GPU_VA_BATCH_BASE,
            )
            && super::map_ggtt(
                dev,
                state.result_phys,
                DIRECT_RCS_RESULT_BYTES,
                DIRECT_RCS_GPU_VA_RESULT_BASE,
            );
    if mapped {
        super::ggtt_invalidate(dev);
    }
    mapped
}

fn direct_rcs_init_ppgtt(state: DirectRcsState) -> bool {
    let pml4_off = 0usize;
    let pdp_off = 4096usize;
    let pd_off = 8192usize;
    let pt_off = 12288usize;
    let pte_present_rw = super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW;
    let pde_present_rw_uc = pte_present_rw | GEN8_PAGE_PWT | GEN8_PAGE_PCD;

    unsafe {
        core::ptr::write_bytes(state.ppgtt_virt, 0, DIRECT_RCS_PPGTT_BYTES);
        let pml4 = state.ppgtt_virt.add(pml4_off) as *mut u64;
        let pdp = state.ppgtt_virt.add(pdp_off) as *mut u64;
        let pd = state.ppgtt_virt.add(pd_off) as *mut u64;
        core::ptr::write_volatile(pml4, (state.ppgtt_phys + pdp_off as u64) | pde_present_rw_uc);
        core::ptr::write_volatile(pdp, (state.ppgtt_phys + pd_off as u64) | pde_present_rw_uc);
        for index in 0..DIRECT_RCS_PPGTT_PT_COUNT {
            let pt_phys = state.ppgtt_phys + pt_off as u64 + (index as u64) * 4096;
            core::ptr::write_volatile(pd.add(index), pt_phys | pde_present_rw_uc);
        }
    }

    let ok = direct_rcs_map_ppgtt_region(
        state,
        DIRECT_RCS_GPU_VA_RING_BASE,
        state.ring_phys,
        DIRECT_RCS_RING_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        DIRECT_RCS_GPU_VA_CONTEXT_BASE,
        state.context_phys,
        DIRECT_RCS_CONTEXT_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        state.batch_phys,
        DIRECT_RCS_BATCH_BYTES,
        pte_present_rw,
    ) && direct_rcs_map_ppgtt_region(
        state,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        state.result_phys,
        DIRECT_RCS_RESULT_BYTES,
        pte_present_rw,
    );

    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    ok
}

fn direct_rcs_map_ppgtt_kernel(state: DirectRcsState, gpu: u64, phys: u64, len: usize) -> bool {
    let ok = direct_rcs_map_ppgtt_region(state, gpu, phys, len, direct_rcs_ppgtt_pte_flags());
    super::dma_flush(state.ppgtt_virt, DIRECT_RCS_PPGTT_BYTES);
    ok
}

fn direct_rcs_ppgtt_pte_flags() -> u64 {
    super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW
}

fn direct_rcs_map_ppgtt_region(
    state: DirectRcsState,
    gpu: u64,
    phys: u64,
    len: usize,
    entry_flags: u64,
) -> bool {
    let pt_off = 12288usize;
    for page in 0..len.div_ceil(4096) {
        let va_page = (gpu >> 12) + page as u64;
        let pd_index = ((va_page >> 9) & 0x1FF) as usize;
        let pt_index = (va_page & 0x1FF) as usize;
        if pd_index >= DIRECT_RCS_PPGTT_PT_COUNT {
            return false;
        }
        let pte_off = pt_off + pd_index * 4096 + pt_index * core::mem::size_of::<u64>();
        let pte = (phys + (page as u64) * 4096) & !0xFFF;
        unsafe {
            core::ptr::write_volatile(state.ppgtt_virt.add(pte_off) as *mut u64, pte | entry_flags);
        }
    }
    true
}

fn direct_rcs_forcewake(dev: super::Dev) -> bool {
    super::mmio_write(
        dev,
        FORCEWAKE_RENDER,
        super::mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK),
    );
    let _ = direct_rcs_wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK,
        0,
        FORCEWAKE_POLL_ITERS,
    );

    super::mmio_write(dev, FORCEWAKE_RENDER, super::mask_en(FORCEWAKE_KERNEL));
    let render_ok = direct_rcs_wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );
    super::mmio_write(dev, FORCEWAKE_GT, super::mask_en(FORCEWAKE_KERNEL));
    let gt_ok = direct_rcs_wait_eq(
        dev,
        FORCEWAKE_ACK_GT,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );
    super::mmio_write(
        dev,
        RCS_CS_DEBUG_MODE1,
        direct_rcs_masked_bit_enable(FF_DOP_CLOCK_GATE_DISABLE),
    );
    render_ok && gt_ok
}

fn direct_rcs_encode_smoke_batch(state: DirectRcsState) -> bool {
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
        let result = state.result_virt as *mut u32;
        core::ptr::write_volatile(result, 0);

        let batch = state.batch_virt as *mut u32;
        core::ptr::write_volatile(batch, MI_STORE_DATA_IMM_GGTT_DW1);
        core::ptr::write_volatile(batch.add(1), DIRECT_RCS_GPU_VA_RESULT_BASE as u32);
        core::ptr::write_volatile(batch.add(2), (DIRECT_RCS_GPU_VA_RESULT_BASE >> 32) as u32);
        core::ptr::write_volatile(batch.add(3), DIRECT_RCS_SMOKE_MARKER);
        core::ptr::write_volatile(batch.add(4), MI_BATCH_BUFFER_END);
        core::ptr::write_volatile(batch.add(5), MI_NOOP);
    }
    super::dma_flush(state.batch_virt, 6 * core::mem::size_of::<u32>());
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_encode_empty_eot_walker_batch(
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
) -> bool {
    if EMPTY_EOT_IDD_OFFSET_BYTES + EMPTY_EOT_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_RCS_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }

    let idd = unsafe { state.batch_virt.add(EMPTY_EOT_IDD_OFFSET_BYTES) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, EMPTY_EOT_TEXT_OFFSET_BYTES as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(idd.add(4), 0);
        core::ptr::write_volatile(idd.add(5), 0);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), 0);
    }

    let batch_len = DIRECT_RCS_BATCH_BYTES / core::mem::size_of::<u32>();
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt as *mut u32, batch_len) };
    let mut cursor = 0usize;
    let mut ok = true;

    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL | 1,
    );
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_3D);
    ok &= direct_rcs_push_pipe_control_full(
        batch,
        &mut cursor,
        (1 << 9) | (1 << 11),
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    );
    ok &= direct_rcs_push_state_base_address(
        batch,
        &mut cursor,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        upload.gpu,
    );
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS);
    ok &= direct_rcs_push(batch, &mut cursor, PIPELINE_SELECT_GPGPU);
    ok &= direct_rcs_push_pipe_control_full(batch, &mut cursor, 1 << 9, PIPE_CONTROL_CS_STALL);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_VFE_STATE_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW3_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_VFE_DW5_UOS);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, EMPTY_EOT_IDD_BYTES as u32);
    ok &= direct_rcs_push(batch, &mut cursor, EMPTY_EOT_IDD_OFFSET_BYTES as u32);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        EMPTY_EOT_PRE_MARKER_SLOT,
        EMPTY_EOT_PRE_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(
        batch,
        &mut cursor,
        (GPGPU_WALKER_SIMD16_SELECT << 30) | (GPGPU_WALKER_GROUP_THREADS - 1),
    );
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 1);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, 1);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_GROUP_Z_DIM);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_SIMD16_MASK);
    ok &= direct_rcs_push(batch, &mut cursor, GPGPU_WALKER_BOTTOM_MASK);
    ok &= direct_rcs_push(batch, &mut cursor, MEDIA_STATE_FLUSH_CMD);
    ok &= direct_rcs_push(batch, &mut cursor, 0);
    ok &= direct_rcs_push_pipe_control(batch, &mut cursor, PIPE_CONTROL_FLUSH_BITS);
    ok &= direct_rcs_push_store_marker(
        batch,
        &mut cursor,
        EMPTY_EOT_POST_MARKER_SLOT,
        EMPTY_EOT_POST_MARKER,
    );
    ok &= direct_rcs_push(batch, &mut cursor, MI_BATCH_BUFFER_END);
    ok &= direct_rcs_push(batch, &mut cursor, MI_NOOP);

    if !ok {
        return false;
    }

    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn direct_rcs_push(batch: &mut [u32], cursor: &mut usize, value: u32) -> bool {
    if *cursor >= batch.len() {
        return false;
    }
    batch[*cursor] = value;
    *cursor += 1;
    true
}

fn direct_rcs_push_pipe_control_full(
    batch: &mut [u32],
    cursor: &mut usize,
    header_flags: u32,
    dw1_flags: u32,
) -> bool {
    direct_rcs_push(batch, cursor, PIPE_CONTROL_CMD | header_flags)
        && direct_rcs_push(batch, cursor, dw1_flags)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_pipe_control(batch: &mut [u32], cursor: &mut usize, flags: u32) -> bool {
    direct_rcs_push_pipe_control_full(batch, cursor, 0, flags)
}

fn direct_rcs_push_store_marker(
    batch: &mut [u32],
    cursor: &mut usize,
    slot: usize,
    value: u32,
) -> bool {
    let dst = DIRECT_RCS_GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
    direct_rcs_push(batch, cursor, MI_STORE_DATA_IMM_GGTT_DW1)
        && direct_rcs_push(batch, cursor, dst as u32)
        && direct_rcs_push(batch, cursor, (dst >> 32) as u32)
        && direct_rcs_push(batch, cursor, value)
}

fn direct_rcs_push_state_base_address(
    batch: &mut [u32],
    cursor: &mut usize,
    indirect_object_base: u64,
    dynamic_state_base: u64,
    instruction_base: u64,
) -> bool {
    direct_rcs_push(batch, cursor, STATE_BASE_ADDRESS_CMD)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, indirect_object_base)
        && direct_rcs_push(batch, cursor, RENDER_MOCS << 16)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, 0)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, dynamic_state_base)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, 0)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, instruction_base)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_size(batch, cursor, true, 0xFFFF_F000)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, 0)
        && direct_rcs_push(batch, cursor, 0)
        && direct_rcs_push_sba_address(batch, cursor, true, RENDER_MOCS, 0)
        && direct_rcs_push(batch, cursor, 0)
}

fn direct_rcs_push_sba_address(
    batch: &mut [u32],
    cursor: &mut usize,
    enable: bool,
    mocs: u32,
    address: u64,
) -> bool {
    let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
    direct_rcs_push(batch, cursor, low) && direct_rcs_push(batch, cursor, (address >> 32) as u32)
}

fn direct_rcs_push_sba_size(
    batch: &mut [u32],
    cursor: &mut usize,
    enable: bool,
    size_bytes: usize,
) -> bool {
    let Some(size_bytes) = align_up(size_bytes, 4096) else {
        return false;
    };
    let Ok(size_bytes) = u32::try_from(size_bytes) else {
        return false;
    };
    direct_rcs_push(batch, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
}

fn direct_rcs_submit_batch(dev: super::Dev, state: DirectRcsState) -> bool {
    let ring_tail_bytes = direct_rcs_build_ring_batch_start(state, DIRECT_RCS_GPU_VA_BATCH_BASE);
    let Some(ring_ctl) = direct_rcs_ring_ctl_value(DIRECT_RCS_RING_BYTES) else {
        return false;
    };
    if !direct_rcs_init_lrc_context_image(
        state,
        DIRECT_RCS_GPU_VA_RING_BASE as u32,
        ring_tail_bytes as u32,
        ring_ctl,
    ) {
        return false;
    }
    let (context_desc_lo, context_desc_hi) =
        direct_rcs_context_descriptor(DIRECT_RCS_GPU_VA_CONTEXT_BASE);
    direct_rcs_write_lrc_ring_tail(state, ring_tail_bytes as u32);
    let pphwsp_gpu = (DIRECT_RCS_GPU_VA_CONTEXT_BASE & !0xFFF) as u32;

    super::mmio_write(dev, GEN12_RCU_MODE, direct_rcs_masked_bit_enable(GEN12_RCU_MODE_CCS_ENABLE));
    super::mmio_write(
        dev,
        RCS_RING_MODE_GEN7,
        direct_rcs_masked_bit_enable(GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    let ctx_ctl = direct_rcs_ctx_control_value(false);
    super::mmio_write(dev, RCS_RING_CONTEXT_CONTROL, ctx_ctl);
    super::mmio_write(dev, RCS_RING_CONTEXT_CONTROL_REF, ctx_ctl);
    super::mmio_write(dev, RCS_RING_MI_MODE, direct_rcs_masked_bit_disable(RING_MI_MODE_STOP_RING));
    super::mmio_write(dev, RCS_RING_HWS_PGA, pphwsp_gpu);
    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);

    direct_rcs_execlist_submit_port_push(dev, context_desc_lo, context_desc_hi, 0, 0);
    super::mmio_write(dev, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);
    super::mmio_write(dev, RCS_RING_TAIL, ring_tail_bytes as u32);
    true
}

fn direct_rcs_build_ring_batch_start(state: DirectRcsState, batch_gpu_addr: u64) -> usize {
    let ring_dwords = DIRECT_RCS_RING_BYTES / core::mem::size_of::<u32>();
    let slots = (ring_dwords / DIRECT_RCS_BATCH_START_DWORDS).max(1);
    let slot = (DIRECT_RCS_RING_KICK_DWORD
        .fetch_add(DIRECT_RCS_BATCH_START_DWORDS as u32, Ordering::AcqRel)
        as usize
        / DIRECT_RCS_BATCH_START_DWORDS)
        % slots;
    let start = slot * DIRECT_RCS_BATCH_START_DWORDS;
    unsafe {
        let dwords = state.ring_virt as *mut u32;
        core::ptr::write_volatile(dwords.add(start), MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT);
        core::ptr::write_volatile(dwords.add(start + 1), batch_gpu_addr as u32);
        core::ptr::write_volatile(dwords.add(start + 2), (batch_gpu_addr >> 32) as u32);
        core::ptr::write_volatile(dwords.add(start + 3), MI_NOOP);
    }
    let tail_bytes = (start + DIRECT_RCS_BATCH_START_DWORDS) * core::mem::size_of::<u32>();
    super::dma_flush(state.ring_virt, DIRECT_RCS_RING_BYTES);
    tail_bytes
}

fn direct_rcs_poll_result(state: DirectRcsState, expected: u32) -> u32 {
    let mut observed = 0;
    for _ in 0..DIRECT_RCS_SMOKE_POLL_ITERS {
        super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
        observed = unsafe { core::ptr::read_volatile(state.result_virt as *const u32) };
        if observed == expected {
            break;
        }
        core::hint::spin_loop();
    }
    observed
}

fn direct_rcs_poll_result_slot(state: DirectRcsState, slot: usize, expected: u32) -> u32 {
    let mut observed = 0;
    for _ in 0..DIRECT_RCS_SMOKE_POLL_ITERS {
        observed = direct_rcs_read_result_slot(state, slot);
        if observed == expected {
            break;
        }
        core::hint::spin_loop();
    }
    observed
}

fn direct_rcs_read_result_slot(state: DirectRcsState, slot: usize) -> u32 {
    let offset = slot.saturating_mul(core::mem::size_of::<u32>());
    if offset + core::mem::size_of::<u32>() > DIRECT_RCS_RESULT_BYTES {
        return 0;
    }
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    unsafe { core::ptr::read_volatile(state.result_virt.add(offset) as *const u32) }
}

fn direct_rcs_init_lrc_context_image(
    state: DirectRcsState,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
) -> bool {
    let total_dwords = DIRECT_RCS_CONTEXT_BYTES / core::mem::size_of::<u32>();
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(state.context_virt as *mut u32, total_dwords) };
    dwords.fill(0);

    let lrc = &mut dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS..];
    if lrc.len() < 192 {
        return false;
    }

    lrc[0] = MI_NOOP;
    let mut idx = 1usize;

    lrc[idx] = direct_rcs_mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x2244;
    lrc[idx + 1] = direct_rcs_ctx_control_value(false);
    lrc[idx + 2] = 0x2034;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2030;
    lrc[idx + 5] = ring_tail;
    lrc[idx + 6] = 0x2038;
    lrc[idx + 7] = ring_start;
    lrc[idx + 8] = 0x203C;
    lrc[idx + 9] = ring_ctl;
    lrc[idx + 10] = 0x2168;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2140;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x2110;
    lrc[idx + 15] = 0;
    lrc[idx + 16] = 0x211C;
    lrc[idx + 17] = 0;
    lrc[idx + 18] = 0x2114;
    lrc[idx + 19] = 0;
    lrc[idx + 20] = 0x2118;
    lrc[idx + 21] = 0;
    lrc[idx + 22] = 0x21C0;
    lrc[idx + 23] = 0;
    lrc[idx + 24] = 0x21C4;
    lrc[idx + 25] = 0;
    lrc[idx + 26] = 0x21C8;
    lrc[idx + 27] = 0;
    lrc[idx + 28] = 0x2180;
    lrc[idx + 29] = 0;
    idx += 30;

    direct_rcs_push_nops(lrc, &mut idx, 5);

    lrc[idx] = direct_rcs_mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x23A8;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x228C;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2288;
    lrc[idx + 5] = 0;
    lrc[idx + 6] = 0x2284;
    lrc[idx + 7] = 0;
    lrc[idx + 8] = 0x2280;
    lrc[idx + 9] = 0;
    lrc[idx + 10] = 0x227C;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2278;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x2274;
    lrc[idx + 15] = (state.ppgtt_phys >> 32) as u32;
    lrc[idx + 16] = 0x2270;
    lrc[idx + 17] = state.ppgtt_phys as u32;
    idx += 18;

    lrc[idx] = direct_rcs_mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x21B0;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x25A8;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x25AC;
    lrc[idx + 5] = 0;
    idx += 6;

    direct_rcs_push_nops(lrc, &mut idx, 6);

    lrc[idx] = direct_rcs_mi_lri_cmd(1, 0);
    idx += 1;
    lrc[idx] = 0x20C8;
    lrc[idx + 1] = 0x7FFF_FFFF;
    idx += 2;

    direct_rcs_push_nops(lrc, &mut idx, 13);

    lrc[idx] = direct_rcs_mi_lri_cmd(51, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x2588;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x2588;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2588;
    lrc[idx + 5] = 0;
    lrc[idx + 6] = 0x2588;
    lrc[idx + 7] = 0;
    lrc[idx + 8] = 0x2588;
    lrc[idx + 9] = 0;
    lrc[idx + 10] = 0x2588;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2028;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x209C;
    lrc[idx + 15] = direct_rcs_masked_bit_disable(RING_MI_MODE_STOP_RING);
    lrc[idx + 16] = 0x20C0;
    lrc[idx + 17] = 0;
    lrc[idx + 18] = 0x2178;
    lrc[idx + 19] = 0;
    lrc[idx + 20] = 0x217C;
    lrc[idx + 21] = 0;
    lrc[idx + 22] = 0x2358;
    lrc[idx + 23] = 0;
    lrc[idx + 24] = 0x2170;
    lrc[idx + 25] = 0;
    lrc[idx + 26] = 0x2150;
    lrc[idx + 27] = 0;
    lrc[idx + 28] = 0x2154;
    lrc[idx + 29] = 0;
    lrc[idx + 30] = 0x2158;
    lrc[idx + 31] = 0;
    lrc[idx + 32] = 0x241C;
    lrc[idx + 33] = 0;
    lrc[idx + 34] = 0x2600;
    lrc[idx + 35] = 0;
    lrc[idx + 36] = 0x2604;
    lrc[idx + 37] = 0;
    lrc[idx + 38] = 0x2608;
    lrc[idx + 39] = 0;
    lrc[idx + 40] = 0x260C;
    lrc[idx + 41] = 0;
    lrc[idx + 42] = 0x2610;
    lrc[idx + 43] = 0;
    lrc[idx + 44] = 0x2614;
    lrc[idx + 45] = 0;
    lrc[idx + 46] = 0x2618;
    lrc[idx + 47] = 0;
    lrc[idx + 48] = 0x261C;
    lrc[idx + 49] = 0;
    lrc[idx + 50] = 0x2620;
    lrc[idx + 51] = 0;
    lrc[idx + 52] = 0x2624;
    lrc[idx + 53] = 0;
    lrc[idx + 54] = 0x2628;
    lrc[idx + 55] = 0;
    lrc[idx + 56] = 0x262C;
    lrc[idx + 57] = 0;
    lrc[idx + 58] = 0x2630;
    lrc[idx + 59] = 0;
    lrc[idx + 60] = 0x2634;
    lrc[idx + 61] = 0;
    lrc[idx + 62] = 0x2638;
    lrc[idx + 63] = 0;
    lrc[idx + 64] = 0x263C;
    lrc[idx + 65] = 0;
    lrc[idx + 66] = 0x2640;
    lrc[idx + 67] = 0;
    lrc[idx + 68] = 0x2644;
    lrc[idx + 69] = 0;
    lrc[idx + 70] = 0x2648;
    lrc[idx + 71] = 0;
    lrc[idx + 72] = 0x264C;
    lrc[idx + 73] = 0;
    lrc[idx + 74] = 0x2650;
    lrc[idx + 75] = 0;
    lrc[idx + 76] = 0x2654;
    lrc[idx + 77] = 0;
    lrc[idx + 78] = 0x2658;
    lrc[idx + 79] = 0;
    lrc[idx + 80] = 0x265C;
    lrc[idx + 81] = 0;
    lrc[idx + 82] = 0x2660;
    lrc[idx + 83] = 0;
    lrc[idx + 84] = 0x2664;
    lrc[idx + 85] = 0;
    lrc[idx + 86] = 0x2668;
    lrc[idx + 87] = 0;
    lrc[idx + 88] = 0x266C;
    lrc[idx + 89] = 0;
    lrc[idx + 90] = 0x2670;
    lrc[idx + 91] = 0;
    lrc[idx + 92] = 0x2674;
    lrc[idx + 93] = 0;
    lrc[idx + 94] = 0x2678;
    lrc[idx + 95] = 0;
    lrc[idx + 96] = 0x267C;
    lrc[idx + 97] = 0;
    lrc[idx + 98] = 0x2068;
    lrc[idx + 99] = 0;
    lrc[idx + 100] = 0x2084;
    lrc[idx + 101] = 0;
    idx += 102;

    lrc[idx] = MI_NOOP;
    idx += 1;
    lrc[idx] = MI_BATCH_BUFFER_END | 1;

    super::dma_flush(state.context_virt, DIRECT_RCS_CONTEXT_BYTES);
    true
}

fn direct_rcs_write_lrc_ring_tail(state: DirectRcsState, ring_tail: u32) {
    const LRC_CONTEXT_CONTROL_VALUE_DW: usize = 3;
    const LRC_RING_TAIL_VALUE_DW: usize = 7;

    let total_dwords = DIRECT_RCS_CONTEXT_BYTES / core::mem::size_of::<u32>();
    if total_dwords <= DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW {
        return;
    }
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(state.context_virt as *mut u32, total_dwords) };
    let ctx_ctl = dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW];
    dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW] = ring_tail;
    dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW] = ctx_ctl;
    super::dma_flush(state.context_virt, DIRECT_RCS_CONTEXT_BYTES);
}

fn direct_rcs_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let desc = base
        | GEN8_CTX_VALID
        | GEN8_CTX_PPGTT_ENABLE
        | CTX_DESC_FORCE_RESTORE
        | GEN8_CTX_PRIVILEGE
        | GEN12_CTX_PRIORITY_NORMAL
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    let submit_id = DIRECT_RCS_SUBMIT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let base_context_id = (((context_gpu_addr >> 12) as u32) & 0x3FF).max(1);
    let sw_context_id = (((submit_id & 0x3FF) << 1) ^ base_context_id).max(1) & 0x7FF;
    let desc_hi = ((context_gpu_addr >> 32) as u32) | (sw_context_id << 7);
    (desc, desc_hi)
}

fn direct_rcs_execlist_submit_port_push(
    dev: super::Dev,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    super::mmio_write(dev, RCS_RING_EXECLIST_SQ_LO, context0_lo);
    super::mmio_write(dev, RCS_RING_EXECLIST_SQ_HI, context0_hi);
    super::mmio_write(dev, RCS_RING_EXECLIST_SQ_LO + 8, context1_lo);
    super::mmio_write(dev, RCS_RING_EXECLIST_SQ_HI + 8, context1_hi);
}

fn direct_rcs_ring_ctl_value(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | RING_VALID)
}

fn direct_rcs_ctx_control_value(inhibit_restore: bool) -> u32 {
    let mut ctl = direct_rcs_masked_bits_update(
        CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT,
    );
    if inhibit_restore {
        ctl |= CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT;
    }
    ctl
}

fn direct_rcs_wait_eq(dev: super::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (super::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn direct_rcs_mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | num_regs.saturating_mul(2).saturating_sub(1)
}

fn direct_rcs_push_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}

fn direct_rcs_masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

fn direct_rcs_masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

fn direct_rcs_masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}
