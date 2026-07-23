//! One-shot Gen12 AVC/VDEnc command and surface-upload probe.
//!
//! This is deliberately one gate short of a hardware encoder. It converts one
//! embedded I420 frame to a linear NV12 source surface, binds the complete
//! fixed-CQP IDR resource graph, and proves that the GuC-owned VCS0 batch
//! retires. The coded buffer is not inspected here, so retirement must never be
//! reported as a successful hardware encode.

use core::sync::atomic::{AtomicU8, Ordering};

use spin::Mutex;

use super::engine as media;

// Keep this fixed diagnostic window above render/UI producer allocations
// (which end at 0x4000_0000) and below display's direct-scanout aliases.
const RING_GPU: u64 = 0x4100_0000;
const CONTEXT_GPU: u64 = 0x4101_0000;
const ARENA_GPU: u64 = 0x4110_0000;
const RING_BYTES: usize = 16 * 1024;
const CONTEXT_BYTES: usize = 22 * 4096;
const ARENA_BYTES: usize = 4 * 1024 * 1024;

const BATCH_OFFSET: usize = 0x0000_0000;
const BATCH_BYTES: usize = 64 * 1024;
const RESULT_OFFSET: usize = 0x0001_0000;
const RESULT_BYTES: usize = 4096;
const SOURCE_OFFSET: usize = 0x0002_0000;
const SOURCE_BYTES: usize = 512 * 512 * 3 / 2;
const RECON_OFFSET: usize = 0x0008_0000;
const RECON_BYTES: usize = SOURCE_BYTES;
const DS_OFFSET: usize = 0x000e_0000;
const DS_BYTES: usize = 128 * 128 * 3 / 2;
const BITSTREAM_OFFSET: usize = 0x0010_0000;
const BITSTREAM_BYTES: usize = 2 * 1024 * 1024;
const MFX_STATS_OFFSET: usize = 0x0030_0000;
const VDENC_STATS_OFFSET: usize = 0x0031_0000;
const SLICE_SIZE_OFFSET: usize = 0x0032_0000;
const INTRA_ROWSTORE_OFFSET: usize = 0x0033_0000;
const DEBLOCK_ROWSTORE_OFFSET: usize = 0x0034_0000;
const BSP_ROWSTORE_OFFSET: usize = 0x0035_0000;
const SCRATCH_BYTES: usize = 64 * 1024;

const BATCH_GPU: u64 = ARENA_GPU + BATCH_OFFSET as u64;
const RESULT_GPU: u64 = ARENA_GPU + RESULT_OFFSET as u64;
const SOURCE_GPU: u64 = ARENA_GPU + SOURCE_OFFSET as u64;
const RECON_GPU: u64 = ARENA_GPU + RECON_OFFSET as u64;
const DS_GPU: u64 = ARENA_GPU + DS_OFFSET as u64;
const BITSTREAM_GPU: u64 = ARENA_GPU + BITSTREAM_OFFSET as u64;
const MFX_STATS_GPU: u64 = ARENA_GPU + MFX_STATS_OFFSET as u64;
const VDENC_STATS_GPU: u64 = ARENA_GPU + VDENC_STATS_OFFSET as u64;
const SLICE_SIZE_GPU: u64 = ARENA_GPU + SLICE_SIZE_OFFSET as u64;
const INTRA_ROWSTORE_GPU: u64 = ARENA_GPU + INTRA_ROWSTORE_OFFSET as u64;
const DEBLOCK_ROWSTORE_GPU: u64 = ARENA_GPU + DEBLOCK_ROWSTORE_OFFSET as u64;
const BSP_ROWSTORE_GPU: u64 = ARENA_GPU + BSP_ROWSTORE_OFFSET as u64;

const TIMEOUT_NS: u64 = 100_000_000;
const POLL_LIMIT: u32 = 2_000_000;
const EXPECTED_CODEC_PACKETS: usize = 34;
const EXPECTED_BATCH_BYTES: usize = 2_340;

const _: () = {
    assert!(RING_GPU % crate::intel::WARM_ALIGN as u64 == 0);
    assert!(CONTEXT_GPU % crate::intel::WARM_ALIGN as u64 == 0);
    assert!(ARENA_GPU % crate::intel::WARM_ALIGN as u64 == 0);
    assert!(BATCH_OFFSET + BATCH_BYTES <= RESULT_OFFSET);
    assert!(RESULT_OFFSET + RESULT_BYTES <= SOURCE_OFFSET);
    assert!(SOURCE_OFFSET + SOURCE_BYTES <= RECON_OFFSET);
    assert!(RECON_OFFSET + RECON_BYTES <= DS_OFFSET);
    assert!(DS_OFFSET + DS_BYTES <= BITSTREAM_OFFSET);
    assert!(BITSTREAM_OFFSET + BITSTREAM_BYTES <= MFX_STATS_OFFSET);
    assert!(BSP_ROWSTORE_OFFSET + SCRATCH_BYTES <= ARENA_BYTES);
    assert!(ARENA_GPU + ARENA_BYTES as u64 <= 0x5000_0000);
};

const KICKOFF_MARKER: u32 = 0x4156_4301;
const CODEC_BEGIN_MARKER: u32 = 0x4156_4302;
const CODEC_END_MARKER: u32 = 0x4156_4303;
const COMPLETE_MARKER: u32 = 0x4156_4304;

// Xe_LPM+ VDBOX0 completion/status registers. Keep these as a read-only,
// timeout-only diagnostic surface; command-stream status stores remain the
// authority once the hardware encode path is promoted beyond this probe.
const MFX_ERROR_FLAG: usize = 0x1C_0800;
const MFX_FRAME_CRC: usize = 0x1C_0850;
const MFX_MB_COUNT: usize = 0x1C_0868;
const MFC_BITSTREAM_BYTECOUNT_FRAME: usize = 0x1C_08A0;
const MFC_BITSTREAM_SE_BITCOUNT_FRAME: usize = 0x1C_08A4;
const MFC_IMAGE_STATUS_MASK: usize = 0x1C_08B4;
const MFC_IMAGE_STATUS_CONTROL: usize = 0x1C_08B8;
const MFC_QP_STATUS_COUNT: usize = 0x1C_08BC;
const MFC_BITSTREAM_BYTECOUNT_SLICE: usize = 0x1C_08D0;
const MFC_AVC_NUM_SLICES: usize = 0x1C_0954;
const GEN8_RING_FAULT_REG: usize = 0x0000_4094;
const GEN8_FAULT_TLB_DATA0: usize = 0x0000_4B10;
const GEN8_FAULT_TLB_DATA1: usize = 0x0000_4B14;
const GEN12_FAULT_TLB_DATA0: usize = 0x0000_CEB8;
const GEN12_FAULT_TLB_DATA1: usize = 0x0000_CEBC;

const MI_FORCE_WAKEUP_MFX: [u32; 2] = [0x0e80_0000, 0x0300_0200];
const MFX_PIPE_MODE_SELECT: [u32; 5] = [0x7000_0003, 0x0002_22d2, 0, 0, 0];
const MFX_SURFACE_RECON: [u32; 6] = [
    0x7001_0004,
    0,
    0x07fc_1ff0,
    0x4800_0ff8,
    0x0000_0200,
    0x0000_0200,
];
const MFX_SURFACE_SOURCE: [u32; 6] = [
    0x7001_0004,
    4,
    0x07fc_1ff0,
    0x4800_0ff8,
    0x0000_0200,
    0x0000_0200,
];
const MFX_SURFACE_DS: [u32; 6] = [
    0x7001_0004,
    5,
    0x01fc_07f0,
    0x4800_03f8,
    0x0000_0080,
    0x0000_0080,
];
const MFX_AVC_IMG_STATE: [u32; 21] = [
    0x7100_0013,
    0x0000_0400,
    0x001f_001f,
    0x0000_2000,
    0x0000_1514,
    0x0800_008f,
    0x0fff_0a8c,
    0,
    0,
    0,
    0x7fff_0000,
    0x8000_0000,
    0,
    0,
    0,
    0,
    0,
    0x0000_0100,
    0,
    0,
    0,
];
const MFX_AVC_SLICE_STATE: [u32; 11] = [
    0x7103_0009,
    0x0000_0002,
    0,
    0x001a_0000,
    0,
    0x0020_0000,
    0x000b_2000,
    0,
    0,
    0,
    0,
];

const VDENC_CONTROL_STATE: [u32; 2] = [0x708b_0000, 0x0000_0002];
const VDENC_PIPE_MODE_SELECT: [u32; 6] = [0x7080_0004, 0x0120_0002, 0x0005_0f09, 0, 0, 0x0002_0100];
const VDENC_SRC_SURFACE_STATE: [u32; 6] = [
    0x7081_0004,
    0,
    0x07fc_1ff0,
    0x0070_0ff8,
    0x0000_0200,
    0x0000_0200,
];
const VDENC_REF_SURFACE_STATE: [u32; 6] = [
    0x7082_0004,
    0,
    0x07fc_1ff0,
    0x0000_0ff8,
    0x0000_0200,
    0x0000_0200,
];
const VDENC_DS_REF_SURFACE_STATE: [u32; 10] = [
    0x7083_0008,
    0,
    0x01fc_07f0,
    0x0000_03f8,
    0x0000_0080,
    0x0000_0080,
    0,
    0,
    0,
    0,
];
const VDENC_CMD3: [u32; 23] = [
    0x708a_0015,
    0x0501_0000,
    0x1a1a_0a0a,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0x0000_0004,
    0x0c24_0400,
    0,
    0x0018_0000,
    0x0c0e_2406,
    0,
    0,
    0,
    0,
    0x000d_0042,
];
const VDENC_AVC_IMG_STATE: [u32; 26] = [
    0x7085_0018,
    0x0000_0301,
    0x7002_8000,
    0x0020_001f,
    0,
    0,
    0,
    0xffff_0000,
    0x0100_2000,
    0,
    0x03e8_0000,
    0x0bb8_07d0,
    0x0f00_0000,
    0x07d0_0000,
    0xff20_001a,
    0x0bb8_0002,
    0x0e10_0004,
    0x1388_0006,
    0x1f40_000a,
    0x2328_0012,
    0,
    0,
    0x3300_0000,
    0,
    0,
    0,
];
const VDENC_AVC_SLICE_STATE: [u32; 4] = [0x708c_0002, 0x0000_000d, 0, 0];
const VDENC_WEIGHTS_OFFSETS_STATE: [u32; 7] = [0x7088_0005, 0x0001_0001, 0x0001_0001, 0, 0, 0, 0];
const VDENC_WALKER_STATE: [u32; 3] = [0x7087_0001, 0x1000_0000, 0x0000_0020];
const VD_PIPELINE_FLUSH: [u32; 2] = [0x7780_0000, 0x0002_001a];

const SPS: [u8; 14] = [
    0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x0a, 0xf8, 0x10, 0x02, 0x09, 0x36, 0x02,
];
const PPS: [u8; 8] = [0x00, 0x00, 0x00, 0x01, 0x68, 0xce, 0x38, 0x80];
const IDR_SLICE_HEADER: [u8; 8] = [0x00, 0x00, 0x00, 0x01, 0x25, 0x88, 0x84, 0x28];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum AvcEncodeProbeState {
    NotRun = 0,
    Deferred = 1,
    Preparing = 2,
    Submitted = 3,
    Passed = 4,
    Failed = 5,
    Quarantined = 6,
}

impl AvcEncodeProbeState {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Deferred,
            2 => Self::Preparing,
            3 => Self::Submitted,
            4 => Self::Passed,
            5 => Self::Failed,
            6 => Self::Quarantined,
            _ => Self::NotRun,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcEncodeProbeFailure {
    None,
    DeviceUnavailable,
    Vcs0Unavailable,
    GucTransportUnavailable,
    TransportProbeUnavailable,
    LaneBusy,
    LaneQuarantined,
    ForcewakeUnavailable,
    BackingAllocation,
    EmbeddedFrameUnavailable,
    SurfaceConversion,
    BatchBuild,
    ContextBuild,
    RegisterRejected,
    SubmitRejected,
    CompletionTimeout,
    MarkerMismatch,
    ContextTeardown,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcEncodeTimeoutDiagnostics {
    pub(crate) valid: bool,
    pub(crate) ring_start: u32,
    pub(crate) ring_ctl: u32,
    pub(crate) ring_head: u32,
    pub(crate) ring_tail: u32,
    pub(crate) ring_acthd_lo: u32,
    pub(crate) ring_acthd_hi: u32,
    pub(crate) acthd_region: &'static str,
    pub(crate) acthd_offset_bytes: u32,
    pub(crate) acthd_dword: u32,
    pub(crate) bbaddr_lo: u32,
    pub(crate) bbaddr_hi: u32,
    pub(crate) dma_fadd_lo: u32,
    pub(crate) dma_fadd_hi: u32,
    pub(crate) bbstate: u32,
    pub(crate) esr: u32,
    pub(crate) instdone: u32,
    pub(crate) instps: u32,
    pub(crate) psmi_ctl: u32,
    pub(crate) nopid: u32,
    pub(crate) ipeir: u32,
    pub(crate) ipehr: u32,
    pub(crate) fault_gen8: u32,
    pub(crate) fault_gen12: u32,
    pub(crate) fault_tlb_data0_gen8: u32,
    pub(crate) fault_tlb_data1_gen8: u32,
    pub(crate) fault_tlb_data0_gen12: u32,
    pub(crate) fault_tlb_data1_gen12: u32,
    pub(crate) mfx_error: u32,
    pub(crate) mfx_frame_crc: u32,
    pub(crate) mfx_mb_count: u32,
    pub(crate) mfc_bitstream_bytecount_frame: u32,
    pub(crate) mfc_bitstream_se_bitcount_frame: u32,
    pub(crate) mfc_bitstream_bytecount_slice: u32,
    pub(crate) mfc_image_status_mask: u32,
    pub(crate) mfc_image_status_control: u32,
    pub(crate) mfc_qp_status_count: u32,
    pub(crate) mfc_avc_num_slices: u32,
    pub(crate) bitstream_head: [u32; 8],
    pub(crate) mfx_stats_head: [u32; 4],
    pub(crate) vdenc_stats_head: [u32; 4],
    pub(crate) slice_size_head: [u32; 4],
}

impl AvcEncodeTimeoutDiagnostics {
    const EMPTY: Self = Self {
        valid: false,
        ring_start: 0,
        ring_ctl: 0,
        ring_head: 0,
        ring_tail: 0,
        ring_acthd_lo: 0,
        ring_acthd_hi: 0,
        acthd_region: "none",
        acthd_offset_bytes: 0,
        acthd_dword: 0,
        bbaddr_lo: 0,
        bbaddr_hi: 0,
        dma_fadd_lo: 0,
        dma_fadd_hi: 0,
        bbstate: 0,
        esr: 0,
        instdone: 0,
        instps: 0,
        psmi_ctl: 0,
        nopid: 0,
        ipeir: 0,
        ipehr: 0,
        fault_gen8: 0,
        fault_gen12: 0,
        fault_tlb_data0_gen8: 0,
        fault_tlb_data1_gen8: 0,
        fault_tlb_data0_gen12: 0,
        fault_tlb_data1_gen12: 0,
        mfx_error: 0,
        mfx_frame_crc: 0,
        mfx_mb_count: 0,
        mfc_bitstream_bytecount_frame: 0,
        mfc_bitstream_se_bitcount_frame: 0,
        mfc_bitstream_bytecount_slice: 0,
        mfc_image_status_mask: 0,
        mfc_image_status_control: 0,
        mfc_qp_status_count: 0,
        mfc_avc_num_slices: 0,
        bitstream_head: [0; 8],
        mfx_stats_head: [0; 4],
        vdenc_stats_head: [0; 4],
        slice_size_head: [0; 4],
    };
}

impl AvcEncodeProbeFailure {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DeviceUnavailable => "device-unavailable",
            Self::Vcs0Unavailable => "vcs0-unavailable",
            Self::GucTransportUnavailable => "guc-transport-unavailable",
            Self::TransportProbeUnavailable => "guc-vcs0-transport-probe-unavailable",
            Self::LaneBusy => "vcs0-lane-busy",
            Self::LaneQuarantined => "vcs0-lane-quarantined",
            Self::ForcewakeUnavailable => "vcs0-forcewake-unavailable",
            Self::BackingAllocation => "encode-probe-backing-allocation",
            Self::EmbeddedFrameUnavailable => "embedded-frame-unavailable",
            Self::SurfaceConversion => "i420-to-nv12-conversion",
            Self::BatchBuild => "avc-idr-batch-build",
            Self::ContextBuild => "vcs0-context-build",
            Self::RegisterRejected => "guc-register-rejected",
            Self::SubmitRejected => "guc-submit-rejected",
            Self::CompletionTimeout => "completion-timeout",
            Self::MarkerMismatch => "ordered-marker-mismatch",
            Self::ContextTeardown => "guc-context-teardown",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcEncodeProbeReport {
    pub(crate) state: AvcEncodeProbeState,
    pub(crate) failure: AvcEncodeProbeFailure,
    pub(crate) forcewake: bool,
    pub(crate) backing_ready: bool,
    pub(crate) surface_uploaded: bool,
    pub(crate) batch_ready: bool,
    pub(crate) context_ready: bool,
    pub(crate) registered: bool,
    pub(crate) submitted: bool,
    pub(crate) retired: bool,
    pub(crate) context_destroyed: bool,
    pub(crate) bitstream_buffer_bound: bool,
    pub(crate) source_i420_bytes: usize,
    pub(crate) source_nv12_bytes: usize,
    pub(crate) source_i420_fnv1a32: u32,
    pub(crate) source_nv12_fnv1a32: u32,
    pub(crate) batch_bytes: usize,
    pub(crate) codec_packets: usize,
    pub(crate) serial: u64,
    pub(crate) hwlrca_lo: u32,
    pub(crate) hwlrca_hi: u32,
    pub(crate) kickoff: u32,
    pub(crate) codec_begin: u32,
    pub(crate) codec_end: u32,
    pub(crate) complete: u32,
    pub(crate) poll_iters: u32,
    pub(crate) elapsed_us: u64,
    pub(crate) timeout_diagnostics: AvcEncodeTimeoutDiagnostics,
}

impl AvcEncodeProbeReport {
    const EMPTY: Self = Self {
        state: AvcEncodeProbeState::NotRun,
        failure: AvcEncodeProbeFailure::None,
        forcewake: false,
        backing_ready: false,
        surface_uploaded: false,
        batch_ready: false,
        context_ready: false,
        registered: false,
        submitted: false,
        retired: false,
        context_destroyed: false,
        bitstream_buffer_bound: false,
        source_i420_bytes: 0,
        source_nv12_bytes: 0,
        source_i420_fnv1a32: 0,
        source_nv12_fnv1a32: 0,
        batch_bytes: 0,
        codec_packets: 0,
        serial: 0,
        hwlrca_lo: 0,
        hwlrca_hi: 0,
        kickoff: 0,
        codec_begin: 0,
        codec_end: 0,
        complete: 0,
        poll_iters: 0,
        elapsed_us: 0,
        timeout_diagnostics: AvcEncodeTimeoutDiagnostics::EMPTY,
    };
}

struct ProbeBacking {
    ring_virt: *mut u8,
    context_virt: *mut u8,
    arena_virt: *mut u8,
    ppgtt: crate::intel::ppgtt::SparsePpgtt,
}

unsafe impl Send for ProbeBacking {}

static STATE: AtomicU8 = AtomicU8::new(AvcEncodeProbeState::NotRun as u8);
static REPORT: Mutex<AvcEncodeProbeReport> = Mutex::new(AvcEncodeProbeReport::EMPTY);
static BACKING: Mutex<Option<ProbeBacking>> = Mutex::new(None);

pub(crate) const fn commands_wired() -> bool {
    true
}

pub(crate) fn passed() -> bool {
    STATE.load(Ordering::Acquire) == AvcEncodeProbeState::Passed as u8
}

pub(crate) fn snapshot() -> AvcEncodeProbeReport {
    *REPORT.lock()
}

pub(crate) fn run_once() -> AvcEncodeProbeReport {
    let current = AvcEncodeProbeState::from_raw(STATE.load(Ordering::Acquire));
    if current != AvcEncodeProbeState::NotRun {
        return snapshot();
    }

    let Some(dev) = crate::intel::claimed_device() else {
        return deferred(AvcEncodeProbeFailure::DeviceUnavailable);
    };
    let (engine, _) = media::default_decode_engine_and_window();
    if engine.id.instance != 0 || !engine.capabilities.decode {
        return deferred(AvcEncodeProbeFailure::Vcs0Unavailable);
    }
    if !crate::intel::guc_submission::INTEL_GUC_SCHEDULER.ready() {
        return deferred(AvcEncodeProbeFailure::GucTransportUnavailable);
    }
    if !super::guc_probe::passed() {
        return deferred(AvcEncodeProbeFailure::TransportProbeUnavailable);
    }

    let lane = match media::try_acquire_vcs0_lane() {
        Ok(lane) => lane,
        Err(media::MediaVcs0LaneAcquireError::Busy) => {
            return deferred(AvcEncodeProbeFailure::LaneBusy);
        }
        Err(media::MediaVcs0LaneAcquireError::Quarantined) => {
            return deferred(AvcEncodeProbeFailure::LaneQuarantined);
        }
    };
    if STATE
        .compare_exchange(
            AvcEncodeProbeState::NotRun as u8,
            AvcEncodeProbeState::Preparing as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return snapshot();
    }

    let started_ns = crate::chronos::monotonic_nanos();
    let mut report = AvcEncodeProbeReport {
        state: AvcEncodeProbeState::Preparing,
        ..AvcEncodeProbeReport::EMPTY
    };
    publish(report);

    report.forcewake = media::wake_media_engine_for_guc(dev, engine);
    if !report.forcewake {
        return fail(report, AvcEncodeProbeFailure::ForcewakeUnavailable, started_ns);
    }

    let mut backing_slot = BACKING.lock();
    if backing_slot.is_none() {
        *backing_slot = build_backing(dev);
    }
    let Some(backing) = backing_slot.as_ref() else {
        return fail(report, AvcEncodeProbeFailure::BackingAllocation, started_ns);
    };
    report.backing_ready = true;

    unsafe {
        core::ptr::write_bytes(backing.ring_virt, 0, RING_BYTES);
        core::ptr::write_bytes(backing.context_virt, 0, CONTEXT_BYTES);
        core::ptr::write_bytes(backing.arena_virt, 0, ARENA_BYTES);
    }

    let Some(i420) = trueos_h264_encode_probe::embedded_sequence_frame_i420(0) else {
        return fail(report, AvcEncodeProbeFailure::EmbeddedFrameUnavailable, started_ns);
    };
    let source_virt = unsafe { backing.arena_virt.add(SOURCE_OFFSET) };
    let source = unsafe { core::slice::from_raw_parts_mut(source_virt, SOURCE_BYTES) };
    if !convert_i420_512_to_nv12(i420, source) {
        return fail(report, AvcEncodeProbeFailure::SurfaceConversion, started_ns);
    }
    report.surface_uploaded = true;
    report.source_i420_bytes = i420.len();
    report.source_nv12_bytes = source.len();
    report.source_i420_fnv1a32 = fnv1a32(i420);
    report.source_nv12_fnv1a32 = fnv1a32(source);

    let batch_virt = unsafe { backing.arena_virt.add(BATCH_OFFSET) };
    let Some((batch_bytes, codec_packets)) = build_idr_batch(batch_virt) else {
        return fail(report, AvcEncodeProbeFailure::BatchBuild, started_ns);
    };
    report.batch_ready = true;
    report.bitstream_buffer_bound = true;
    report.batch_bytes = batch_bytes;
    report.codec_packets = codec_packets;

    let Some(ring_tail_bytes) = media::build_ring_batch_start_words(
        backing.ring_virt,
        RING_BYTES,
        0,
        RESULT_GPU,
        KICKOFF_MARKER,
        BATCH_GPU,
    ) else {
        return fail(report, AvcEncodeProbeFailure::ContextBuild, started_ns);
    };
    let Some(ring_ctl) = media::ring_ctl_value_for_size(RING_BYTES) else {
        return fail(report, AvcEncodeProbeFailure::ContextBuild, started_ns);
    };
    if !media::init_gen12_video_context_image(
        backing.context_virt,
        CONTEXT_BYTES,
        engine.ring_base,
        0,
        RING_GPU as u32,
        ring_tail_bytes as u32,
        ring_ctl,
        CONTEXT_GPU as u32,
        backing.ppgtt.pml4_phys(),
        false,
    ) {
        return fail(report, AvcEncodeProbeFailure::ContextBuild, started_ns);
    }
    report.context_ready = true;

    crate::intel::dma_flush(backing.arena_virt, ARENA_BYTES);
    crate::intel::dma_flush(backing.ring_virt, ring_tail_bytes);
    crate::intel::dma_flush(backing.context_virt, CONTEXT_BYTES);
    crate::intel::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);

    let (hwlrca_lo, hwlrca_hi) = media::build_media_guc_context_descriptor(CONTEXT_GPU);
    report.hwlrca_lo = hwlrca_lo;
    report.hwlrca_hi = hwlrca_hi;
    let token = match crate::intel::guc_submission::INTEL_GUC_SCHEDULER.register(
        dev,
        crate::gpu::physical::EngineClass::VideoDecode,
        hwlrca_lo,
        hwlrca_hi,
    ) {
        Ok(token) => token,
        Err(_) => return fail(report, AvcEncodeProbeFailure::RegisterRejected, started_ns),
    };
    report.registered = true;

    let submission = match crate::intel::guc_submission::INTEL_GUC_SCHEDULER.submit(dev, token) {
        Ok(submission) => submission,
        Err(_) => {
            report.context_destroyed = crate::intel::guc_submission::INTEL_GUC_SCHEDULER
                .destroy(dev, token)
                .is_ok();
            if !report.context_destroyed {
                return quarantine(
                    lane,
                    report,
                    AvcEncodeProbeFailure::ContextTeardown,
                    started_ns,
                );
            }
            return fail(report, AvcEncodeProbeFailure::SubmitRejected, started_ns);
        }
    };
    report.state = AvcEncodeProbeState::Submitted;
    report.submitted = true;
    report.serial = submission.serial;
    publish(report);

    let result_virt = unsafe { backing.arena_virt.add(RESULT_OFFSET) };
    let deadline = crate::chronos::monotonic_nanos().saturating_add(TIMEOUT_NS);
    while report.poll_iters < POLL_LIMIT {
        crate::intel::dma_flush(result_virt, RESULT_BYTES);
        report.complete = media::read_result_dword(result_virt, media::MEDIA_RESULT_COMPLETE_SLOT);
        if report.complete == COMPLETE_MARKER {
            report.retired = true;
            break;
        }
        report.poll_iters = report.poll_iters.saturating_add(1);
        if crate::chronos::monotonic_nanos() >= deadline {
            break;
        }
        core::hint::spin_loop();
    }

    crate::intel::dma_flush(result_virt, RESULT_BYTES);
    report.kickoff = media::read_result_dword(result_virt, media::MEDIA_RESULT_KICKOFF_SLOT);
    report.codec_begin = media::read_result_dword(result_virt, media::MEDIA_RESULT_PRESUBMIT_SLOT);
    report.codec_end = media::read_result_dword(result_virt, media::MEDIA_RESULT_POSTSUBMIT_SLOT);
    report.complete = media::read_result_dword(result_virt, media::MEDIA_RESULT_COMPLETE_SLOT);
    if !report.retired {
        report.timeout_diagnostics = capture_timeout_diagnostics(dev, engine, backing);
        return quarantine(lane, report, AvcEncodeProbeFailure::CompletionTimeout, started_ns);
    }

    report.context_destroyed = crate::intel::guc_submission::INTEL_GUC_SCHEDULER
        .destroy(dev, token)
        .is_ok();
    if !report.context_destroyed {
        return quarantine(lane, report, AvcEncodeProbeFailure::ContextTeardown, started_ns);
    }

    if report.kickoff != KICKOFF_MARKER
        || report.codec_begin != CODEC_BEGIN_MARKER
        || report.codec_end != CODEC_END_MARKER
        || report.complete != COMPLETE_MARKER
    {
        return fail(report, AvcEncodeProbeFailure::MarkerMismatch, started_ns);
    }

    report.state = AvcEncodeProbeState::Passed;
    report.failure = AvcEncodeProbeFailure::None;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn build_backing(dev: crate::intel::Dev) -> Option<ProbeBacking> {
    let (ring_phys, ring_virt) = crate::dma::alloc(RING_BYTES, crate::intel::WARM_ALIGN)?;
    let (context_phys, context_virt) = crate::dma::alloc(CONTEXT_BYTES, crate::intel::WARM_ALIGN)?;
    let (arena_phys, arena_virt) = crate::dma::alloc(ARENA_BYTES, crate::intel::WARM_ALIGN)?;

    if !crate::intel::map_ggtt(dev, ring_phys, RING_BYTES, RING_GPU)
        || !crate::intel::map_ggtt(dev, context_phys, CONTEXT_BYTES, CONTEXT_GPU)
    {
        return None;
    }
    crate::intel::ggtt_invalidate(dev);
    let ppgtt =
        crate::intel::ppgtt::build_sparse_ppgtt_for_ranges(&[crate::intel::ppgtt::PpgttRange {
            gpu: ARENA_GPU,
            phys: arena_phys,
            bytes: ARENA_BYTES,
        }])?;
    Some(ProbeBacking {
        ring_virt,
        context_virt,
        arena_virt,
        ppgtt,
    })
}

fn convert_i420_512_to_nv12(i420: &[u8], nv12: &mut [u8]) -> bool {
    const LUMA_BYTES: usize = 512 * 512;
    const CHROMA_PLANE_BYTES: usize = 256 * 256;
    if i420.len() != SOURCE_BYTES || nv12.len() != SOURCE_BYTES {
        return false;
    }
    nv12[..LUMA_BYTES].copy_from_slice(&i420[..LUMA_BYTES]);
    let cb = &i420[LUMA_BYTES..LUMA_BYTES + CHROMA_PLANE_BYTES];
    let cr = &i420[LUMA_BYTES + CHROMA_PLANE_BYTES..];
    let uv = &mut nv12[LUMA_BYTES..];
    for (pair, (&cb, &cr)) in uv.chunks_exact_mut(2).zip(cb.iter().zip(cr.iter())) {
        pair[0] = cb;
        pair[1] = cr;
    }
    true
}

fn build_idr_batch(batch_virt: *mut u8) -> Option<(usize, usize)> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            batch_virt.cast::<u32>(),
            BATCH_BYTES / core::mem::size_of::<u32>(),
        )
    };
    batch.fill(0);
    let mut idx = 0usize;
    let mut packet_count = 0usize;

    push_words(batch, &mut idx, &MI_FORCE_WAKEUP_MFX)?;
    if !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        RESULT_GPU + media::MEDIA_RESULT_PRESUBMIT_SLOT,
        CODEC_BEGIN_MARKER,
    ) {
        return None;
    }

    push_packet(batch, &mut idx, &VDENC_CONTROL_STATE, &mut packet_count)?;
    if !media::emit_mfx_wait(batch, &mut idx) {
        return None;
    }
    packet_count += 1;
    push_packet(batch, &mut idx, &MFX_PIPE_MODE_SELECT, &mut packet_count)?;
    if !media::emit_mfx_wait(batch, &mut idx) {
        return None;
    }
    packet_count += 1;
    push_packet(batch, &mut idx, &MFX_SURFACE_RECON, &mut packet_count)?;
    push_packet(batch, &mut idx, &MFX_SURFACE_SOURCE, &mut packet_count)?;
    push_packet(batch, &mut idx, &MFX_SURFACE_DS, &mut packet_count)?;

    let mfx_pipe_buf = mfx_pipe_buf_addr_state();
    push_packet(batch, &mut idx, &mfx_pipe_buf, &mut packet_count)?;
    let mfx_ind_obj = mfx_ind_obj_base_addr_state();
    push_packet(batch, &mut idx, &mfx_ind_obj, &mut packet_count)?;
    let mfx_bsp = mfx_bsp_buf_base_addr_state();
    push_packet(batch, &mut idx, &mfx_bsp, &mut packet_count)?;

    push_packet(batch, &mut idx, &VDENC_PIPE_MODE_SELECT, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_SRC_SURFACE_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_REF_SURFACE_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_DS_REF_SURFACE_STATE, &mut packet_count)?;
    let vdenc_pipe_buf = vdenc_pipe_buf_addr_state();
    push_packet(batch, &mut idx, &vdenc_pipe_buf, &mut packet_count)?;

    push_packet(batch, &mut idx, &MFX_AVC_IMG_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_CMD3, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_AVC_IMG_STATE, &mut packet_count)?;
    for matrix_type in 0..4u32 {
        let qm = mfx_qm_state(matrix_type);
        push_packet(batch, &mut idx, &qm, &mut packet_count)?;
    }
    for matrix_type in 0..4u32 {
        let fqm = mfx_fqm_state(matrix_type);
        push_packet(batch, &mut idx, &fqm, &mut packet_count)?;
    }

    push_packet(batch, &mut idx, &MFX_AVC_SLICE_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_AVC_SLICE_STATE, &mut packet_count)?;
    push_pak_insert(batch, &mut idx, &SPS, 112, false, false, 0, false)?;
    packet_count += 1;
    push_pak_insert(batch, &mut idx, &PPS, 64, false, false, 0, false)?;
    packet_count += 1;
    push_pak_insert(batch, &mut idx, &IDR_SLICE_HEADER, 61, true, true, 4, true)?;
    packet_count += 1;

    push_packet(batch, &mut idx, &VDENC_WEIGHTS_OFFSETS_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VDENC_WALKER_STATE, &mut packet_count)?;
    push_packet(batch, &mut idx, &VD_PIPELINE_FLUSH, &mut packet_count)?;

    if !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        RESULT_GPU + media::MEDIA_RESULT_POSTSUBMIT_SLOT,
        CODEC_END_MARKER,
    ) {
        return None;
    }
    let flush = media::begin_batch_packet(
        batch,
        &mut idx,
        5,
        // VD_PIPELINE_FLUSH immediately above already waits for MFX/VDEnc and
        // flushes the VDEnc pipeline. Combining another video-pipeline cache
        // invalidate with this post-sync write parks VCS0 on Xe_LPM+ even
        // though the coded frame and status registers are complete. Keep this
        // final command as the ordered memory completion fence only.
        media::MI_FLUSH_DW | media::MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE,
    )?;
    media::packet_write_addr64(batch, flush, 1, RESULT_GPU + media::MEDIA_RESULT_COMPLETE_SLOT);
    batch[flush + 3] = COMPLETE_MARKER;
    batch[flush + 4] = 0;
    if idx.saturating_add(3) > batch.len() {
        return None;
    }
    batch[idx] = media::MI_ARB_CHECK;
    batch[idx + 1] = media::MI_BATCH_BUFFER_END;
    batch[idx + 2] = media::MI_NOOP;
    idx += 3;
    let batch_bytes = idx * core::mem::size_of::<u32>();
    if packet_count != EXPECTED_CODEC_PACKETS || batch_bytes != EXPECTED_BATCH_BYTES {
        return None;
    }
    Some((batch_bytes, packet_count))
}

fn push_packet(
    batch: &mut [u32],
    idx: &mut usize,
    words: &[u32],
    packet_count: &mut usize,
) -> Option<()> {
    push_words(batch, idx, words)?;
    *packet_count = packet_count.saturating_add(1);
    Some(())
}

fn push_words(batch: &mut [u32], idx: &mut usize, words: &[u32]) -> Option<()> {
    let end = idx.checked_add(words.len())?;
    batch.get_mut(*idx..end)?.copy_from_slice(words);
    *idx = end;
    Some(())
}

fn set_addr(words: &mut [u32], dword: usize, gpu: u64) {
    words[dword] = gpu as u32;
    words[dword + 1] = (gpu >> 32) as u32;
}

fn mfx_pipe_buf_addr_state() -> [u32; 68] {
    let mut words = [0u32; 68];
    words[0] = 0x7002_0042;
    for (dword, gpu, attr_dword, attr) in [
        (1, RECON_GPU, 3, 1),
        (4, RECON_GPU, 6, 1),
        (7, SOURCE_GPU, 9, 1),
        (10, MFX_STATS_GPU, 12, 1),
        (13, INTRA_ROWSTORE_GPU, 15, 1),
        (16, DEBLOCK_ROWSTORE_GPU, 18, 1),
        (52, MFX_STATS_GPU, 54, 1),
        (62, DS_GPU, 64, 2),
        (65, SLICE_SIZE_GPU, 67, 2),
    ] {
        set_addr(&mut words, dword, gpu);
        words[attr_dword] = attr;
    }
    words
}

fn mfx_ind_obj_base_addr_state() -> [u32; 26] {
    let mut words = [0u32; 26];
    words[0] = 0x7003_0018;
    set_addr(&mut words, 21, BITSTREAM_GPU);
    words[23] = 2;
    set_addr(&mut words, 24, BITSTREAM_GPU.saturating_add(BITSTREAM_BYTES as u64));
    words
}

fn mfx_bsp_buf_base_addr_state() -> [u32; 10] {
    let mut words = [0u32; 10];
    words[0] = 0x7004_0008;
    set_addr(&mut words, 1, BSP_ROWSTORE_GPU);
    words[3] = 2;
    set_addr(&mut words, 4, BSP_ROWSTORE_GPU);
    words[6] = 2;
    words
}

fn vdenc_pipe_buf_addr_state() -> [u32; 89] {
    let mut words = [0u32; 89];
    words[0] = 0x7084_0057;
    for (dword, gpu, attr_dword) in [
        (10, SOURCE_GPU, 12),
        (16, INTRA_ROWSTORE_GPU, 18),
        (34, VDENC_STATS_GPU, 36),
        (49, DS_GPU, 51),
    ] {
        set_addr(&mut words, dword, gpu);
        words[attr_dword] = 2;
    }
    words
}

fn mfx_qm_state(matrix_type: u32) -> [u32; 18] {
    let mut words = [0x1010_1010u32; 18];
    words[0] = 0x7007_0010;
    words[1] = matrix_type & 3;
    words
}

fn mfx_fqm_state(matrix_type: u32) -> [u32; 34] {
    let mut words = [0x0100_0100u32; 34];
    words[0] = 0x7008_0020;
    words[1] = matrix_type & 3;
    words
}

fn push_pak_insert(
    batch: &mut [u32],
    idx: &mut usize,
    bytes: &[u8],
    bit_count: usize,
    last_header: bool,
    emulate: bool,
    skip_emulation_bytes: u8,
    slice_header: bool,
) -> Option<()> {
    if bit_count == 0 || bit_count > bytes.len().checked_mul(8)? {
        return None;
    }
    let payload_dwords = bytes.len().div_ceil(4);
    let total_dwords = 2usize.checked_add(payload_dwords)?;
    let start =
        media::begin_batch_packet(batch, idx, total_dwords, 0x7048_0000 | payload_dwords as u32)?;
    let bits_in_last_dword = match bit_count % 32 {
        0 => 32,
        bits => bits,
    };
    let mut control = (bits_in_last_dword as u32) << 8;
    control |= (last_header as u32) << 2;
    control |= (emulate as u32) << 3;
    control |= ((skip_emulation_bytes as u32) & 0x0f) << 4;
    control |= (slice_header as u32) << 14;
    if !emulate {
        control |= 1 << 15;
    }
    batch[start + 1] = control;
    for (byte_index, byte) in bytes.iter().copied().enumerate() {
        batch[start + 2 + byte_index / 4] |= (byte as u32) << ((byte_index % 4) * 8);
    }
    Some(())
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in bytes {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn capture_timeout_diagnostics(
    dev: crate::intel::Dev,
    engine: media::MediaEngineDescriptor,
    backing: &ProbeBacking,
) -> AvcEncodeTimeoutDiagnostics {
    let bitstream_virt = unsafe { backing.arena_virt.add(BITSTREAM_OFFSET) };
    let mfx_stats_virt = unsafe { backing.arena_virt.add(MFX_STATS_OFFSET) };
    let vdenc_stats_virt = unsafe { backing.arena_virt.add(VDENC_STATS_OFFSET) };
    let slice_size_virt = unsafe { backing.arena_virt.add(SLICE_SIZE_OFFSET) };
    crate::intel::dma_flush(bitstream_virt, 8 * core::mem::size_of::<u32>());
    crate::intel::dma_flush(mfx_stats_virt, 4 * core::mem::size_of::<u32>());
    crate::intel::dma_flush(vdenc_stats_virt, 4 * core::mem::size_of::<u32>());
    crate::intel::dma_flush(slice_size_virt, 4 * core::mem::size_of::<u32>());

    let base = engine.ring_base;
    let ring_acthd_lo = crate::intel::mmio_read(dev, base + media::RING_ACTHD);
    let ring_acthd_hi = crate::intel::mmio_read(dev, base + media::RING_ACTHD_UDW);
    let acthd = ((ring_acthd_hi as u64) << 32) | ring_acthd_lo as u64;
    let (acthd_region, acthd_offset_bytes, acthd_dword) = classify_acthd(acthd, backing);

    AvcEncodeTimeoutDiagnostics {
        valid: true,
        ring_start: crate::intel::mmio_read(dev, base + media::RING_START),
        ring_ctl: crate::intel::mmio_read(dev, base + media::RING_CTL),
        ring_head: crate::intel::mmio_read(dev, base + media::RING_HEAD),
        ring_tail: crate::intel::mmio_read(dev, base + media::RING_TAIL),
        ring_acthd_lo,
        ring_acthd_hi,
        acthd_region,
        acthd_offset_bytes,
        acthd_dword,
        bbaddr_lo: crate::intel::mmio_read(dev, base + media::RING_BBADDR),
        bbaddr_hi: crate::intel::mmio_read(dev, base + media::RING_BBADDR_UDW),
        dma_fadd_lo: crate::intel::mmio_read(dev, base + media::RING_DMA_FADD),
        dma_fadd_hi: crate::intel::mmio_read(dev, base + media::RING_DMA_FADD_UDW),
        bbstate: crate::intel::mmio_read(dev, base + media::RING_BBSTATE),
        esr: crate::intel::mmio_read(dev, base + media::RING_ESR),
        instdone: crate::intel::mmio_read(dev, base + media::RING_INSTDONE),
        instps: crate::intel::mmio_read(dev, base + media::RING_INSTPS),
        psmi_ctl: crate::intel::mmio_read(dev, base + media::RING_PSMI_CTL),
        nopid: crate::intel::mmio_read(dev, base + media::RING_NOPID),
        ipeir: crate::intel::mmio_read(dev, base + media::RING_IPEIR),
        ipehr: crate::intel::mmio_read(dev, base + media::RING_IPEHR),
        fault_gen8: crate::intel::mmio_read(dev, GEN8_RING_FAULT_REG),
        fault_gen12: crate::intel::mmio_read(dev, media::GEN12_RING_FAULT_REG),
        fault_tlb_data0_gen8: crate::intel::mmio_read(dev, GEN8_FAULT_TLB_DATA0),
        fault_tlb_data1_gen8: crate::intel::mmio_read(dev, GEN8_FAULT_TLB_DATA1),
        fault_tlb_data0_gen12: crate::intel::mmio_read(dev, GEN12_FAULT_TLB_DATA0),
        fault_tlb_data1_gen12: crate::intel::mmio_read(dev, GEN12_FAULT_TLB_DATA1),
        mfx_error: crate::intel::mmio_read(dev, MFX_ERROR_FLAG),
        mfx_frame_crc: crate::intel::mmio_read(dev, MFX_FRAME_CRC),
        mfx_mb_count: crate::intel::mmio_read(dev, MFX_MB_COUNT),
        mfc_bitstream_bytecount_frame: crate::intel::mmio_read(dev, MFC_BITSTREAM_BYTECOUNT_FRAME),
        mfc_bitstream_se_bitcount_frame: crate::intel::mmio_read(
            dev,
            MFC_BITSTREAM_SE_BITCOUNT_FRAME,
        ),
        mfc_bitstream_bytecount_slice: crate::intel::mmio_read(dev, MFC_BITSTREAM_BYTECOUNT_SLICE),
        mfc_image_status_mask: crate::intel::mmio_read(dev, MFC_IMAGE_STATUS_MASK),
        mfc_image_status_control: crate::intel::mmio_read(dev, MFC_IMAGE_STATUS_CONTROL),
        mfc_qp_status_count: crate::intel::mmio_read(dev, MFC_QP_STATUS_COUNT),
        mfc_avc_num_slices: crate::intel::mmio_read(dev, MFC_AVC_NUM_SLICES),
        bitstream_head: read_dword_head::<8>(bitstream_virt),
        mfx_stats_head: read_dword_head::<4>(mfx_stats_virt),
        vdenc_stats_head: read_dword_head::<4>(vdenc_stats_virt),
        slice_size_head: read_dword_head::<4>(slice_size_virt),
    }
}

fn classify_acthd(acthd: u64, backing: &ProbeBacking) -> (&'static str, u32, u32) {
    if let Some(offset) = acthd.checked_sub(BATCH_GPU) {
        if offset < BATCH_BYTES as u64 {
            let offset = offset as usize;
            let dword = unsafe {
                core::ptr::read_volatile(backing.arena_virt.add(BATCH_OFFSET + offset).cast())
            };
            return ("batch", offset as u32, dword);
        }
    }
    if let Some(offset) = acthd.checked_sub(RING_GPU) {
        if offset < RING_BYTES as u64 {
            let offset = offset as usize;
            let dword = unsafe { core::ptr::read_volatile(backing.ring_virt.add(offset).cast()) };
            return ("ring", offset as u32, dword);
        }
    }
    ("other", 0, 0)
}

fn read_dword_head<const N: usize>(ptr: *mut u8) -> [u32; N] {
    let mut words = [0u32; N];
    for (index, word) in words.iter_mut().enumerate() {
        *word = unsafe { core::ptr::read_volatile(ptr.add(index * 4).cast::<u32>()) };
    }
    words
}

fn deferred(failure: AvcEncodeProbeFailure) -> AvcEncodeProbeReport {
    let report = AvcEncodeProbeReport {
        state: AvcEncodeProbeState::Deferred,
        failure,
        ..AvcEncodeProbeReport::EMPTY
    };
    *REPORT.lock() = report;
    report
}

fn fail(
    mut report: AvcEncodeProbeReport,
    failure: AvcEncodeProbeFailure,
    started_ns: u64,
) -> AvcEncodeProbeReport {
    report.state = AvcEncodeProbeState::Failed;
    report.failure = failure;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn quarantine(
    lane: media::MediaVcs0LaneGuard,
    mut report: AvcEncodeProbeReport,
    failure: AvcEncodeProbeFailure,
    started_ns: u64,
) -> AvcEncodeProbeReport {
    lane.quarantine();
    report.state = AvcEncodeProbeState::Quarantined;
    report.failure = failure;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn publish(report: AvcEncodeProbeReport) {
    *REPORT.lock() = report;
    STATE.store(report.state as u8, Ordering::Release);
}

fn elapsed_us(started_ns: u64) -> u64 {
    crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000
}
