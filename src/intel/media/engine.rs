use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use super::xelp_media2_ngin_hw_pic::MediaEncodedStreamProof;

const MAX_MEDIA_ENGINES: usize = 4;
const MAX_MEDIA_API_ROUTES: usize = 4;

const FORCEWAKE_MEDIA_GEN11: usize = 0x0A184;
const FORCEWAKE_MEDIA_VDBOX0: usize = 0x0A540;
const FORCEWAKE_MEDIA_VDBOX1: usize = 0x0A544;
const FORCEWAKE_MEDIA_VDBOX2: usize = 0x0A548;
const FORCEWAKE_MEDIA_VDBOX3: usize = 0x0A54C;
const FORCEWAKE_ACK_MEDIA: usize = 0x0D88;
const FORCEWAKE_ACK_VDBOX0: usize = 0x0D50;
const FORCEWAKE_ACK_VDBOX1: usize = 0x0D54;
const FORCEWAKE_ACK_VDBOX2: usize = 0x0D58;
const FORCEWAKE_ACK_VDBOX3: usize = 0x0D5C;
const FORCEWAKE_ACK_VEBOX0: usize = 0x0D70;
const FORCEWAKE_ACK_VEBOX1: usize = 0x0D74;
const FORCEWAKE_ACK_VEBOX2: usize = 0x0D78;
const FORCEWAKE_ACK_VEBOX3: usize = 0x0D7C;
const FORCEWAKE_KERNEL: u32 = 1 << 0;

const GEN11_VCS0_RING_BASE: usize = 0x1C0000;
const GEN11_VCS2_RING_BASE: usize = 0x1D0000;

pub(super) const RING_TAIL: usize = 0x30;
pub(super) const RING_HEAD: usize = 0x34;
pub(super) const RING_START: usize = 0x38;
pub(super) const RING_CTL: usize = 0x3C;
pub(super) const RING_PSMI_CTL: usize = 0x50;
pub(super) const RING_ACTHD_UDW: usize = 0x5C;
pub(super) const RING_DMA_FADD_UDW: usize = 0x60;
pub(super) const RING_IPEIR: usize = 0x64;
pub(super) const RING_IPEHR: usize = 0x68;
pub(super) const RING_INSTDONE: usize = 0x6C;
pub(super) const RING_INSTPS: usize = 0x70;
pub(super) const RING_ACTHD: usize = 0x74;
pub(super) const RING_DMA_FADD: usize = 0x78;
pub(super) const RING_HWS_PGA: usize = 0x80;
pub(super) const RING_NOPID: usize = 0x94;
const RING_HWSTAM: usize = 0x98;
pub(super) const RING_MI_MODE: usize = 0x9C;
pub(super) const RING_BBSTATE: usize = 0x110;
pub(super) const RING_BBADDR: usize = 0x140;
pub(super) const RING_BBADDR_UDW: usize = 0x168;
pub(super) const RING_CONTEXT_CONTROL: usize = 0x244;
pub(super) const RING_MODE_GEN7: usize = 0x29C;
pub(super) const RING_CONTEXT_CONTROL_REF: usize = 0x5A0;
pub(super) const RING_ESR: usize = 0xB8;
pub(super) const RING_EXECLIST_STATUS_LO: usize = 0x234;
pub(super) const RING_EXECLIST_STATUS_HI: usize = 0x238;
pub(super) const RING_EXECLIST_SQ_LO: usize = 0x510;
pub(super) const RING_EXECLIST_SQ_HI: usize = 0x514;
pub(super) const RING_EXECLIST_CONTROL: usize = 0x550;
pub(super) const GEN12_RING_FAULT_REG: usize = 0x0000_CEC4;

const MEDIA_ENGINE_GPU_ADDR_BASE: u64 = 0x0900_0000;
const MEDIA_ENGINE_GPU_ADDR_STRIDE: u64 = 0x0400_0000;
const MEDIA_DEFAULT_RING_BYTES: usize = 16 * 1024;
const MEDIA_DEFAULT_CONTEXT_BYTES: usize = 22 * 4096;
const MEDIA_DEFAULT_BATCH_BYTES: usize = 32 * 1024;
const MEDIA_DEFAULT_RESULT_BYTES: usize = 4 * 4096;
const MEDIA_DEFAULT_BITSTREAM_BYTES: usize = 8 * 1024 * 1024;
const MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES: usize = 40 * 1024 * 1024;
const MEDIA_DEFAULT_AVC_SCRATCH_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MEDIA_SUBMIT_POLL_ITERS: usize = 100_000;

const MI_STORE_DWORD_IMM_GEN4: u32 = (0x20 << 23) | 2;
const MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT: u32 = MI_STORE_DWORD_IMM_GEN4 | (4 - 2);
pub(super) const MI_FLUSH_DW: u32 = (0x26 << 23) | 3;
pub(super) const MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE: u32 = 1 << 7;
pub(super) const MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE: u32 = 1 << 14;
pub(super) const MI_ARB_CHECK: u32 = 0x0280_0000;
pub(super) const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_PPGTT: u32 = 1 << 8;
const MI_BATCH_SECOND_LEVEL: u32 = 1 << 22;
pub(super) const MI_NOOP: u32 = 0;
pub(super) const MI_FORCE_WAKEUP: u32 = 29 << 23;
pub(super) const MI_FORCE_WAKEUP_MFX_WELL: u32 = (1 << 9) | (0x300 << 16);
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;

pub(super) const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_DESC_VALID: u32 = 1 << 0;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const CTX_DESC_PPGTT_ENABLE: u32 = 1 << 5;
const CTX_DESC_PRIVILEGE: u32 = 1 << 8;
const CTX_DESC_PRIORITY_NORMAL: u32 = 1 << 9;
const CTX_DESC_ADDRESSING_MODE_SHIFT: u32 = 3;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
pub(super) const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
pub(super) const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
pub(super) const STOP_RING: u32 = 1 << 8;

const MEDIA_PIPELINE_MFX: u32 = 2;
pub(super) const MEDIA_CMD_OPCODE_MFX_COMMON: u32 = 0;
pub(super) const MFX_PIPE_MODE_SELECT: u32 = 0;
pub(super) const MFX_SURFACE_STATE: u32 = 1;
pub(super) const MFX_PIPE_BUF_ADDR_STATE: u32 = 2;
pub(super) const MFX_IND_OBJ_BASE_ADDR_STATE: u32 = 3;
pub(super) const MFX_QM_STATE: u32 = 7;
pub(super) const MFX_CMD_LEN_PIPE_MODE_SELECT: u32 = 3;
pub(super) const MFX_CMD_LEN_SURFACE_STATE: u32 = 4;
pub(super) const MFX_CMD_LEN_PIPE_BUF_ADDR_STATE: u32 = 63;
pub(super) const MFX_CMD_LEN_IND_OBJ_BASE_ADDR_STATE: u32 = 24;
pub(super) const MFX_CMD_LEN_QM_STATE: u32 = 16;
pub(super) const MFX_MOCS_UC: u32 = 1;
const MFX_WAIT_SYNC: u32 = (3 << 29) | (1 << 27) | (1 << 8);

const MEDIA_RESULT_SLOT_BYTES: u64 = 8;
pub(super) const MEDIA_RESULT_KICKOFF_SLOT: u64 = 0;
pub(super) const MEDIA_RESULT_PRESUBMIT_SLOT: u64 =
    MEDIA_RESULT_KICKOFF_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_POSTSUBMIT_SLOT: u64 =
    MEDIA_RESULT_PRESUBMIT_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_COMPLETE_SLOT: u64 =
    MEDIA_RESULT_POSTSUBMIT_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT: u64 =
    MEDIA_RESULT_COMPLETE_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_BITSTREAM_BYTES_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_SAMPLE_NALS_SLOT: u64 =
    MEDIA_RESULT_BITSTREAM_BYTES_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_STAGE_FLAGS_SLOT: u64 =
    MEDIA_RESULT_SAMPLE_NALS_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT: u64 =
    MEDIA_RESULT_STAGE_FLAGS_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT: u64 =
    MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT: u64 =
    MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT + MEDIA_RESULT_SLOT_BYTES;
pub(super) const MEDIA_RESULT_FRAME_DIMS_SLOT: u64 =
    MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT + MEDIA_RESULT_SLOT_BYTES;

static MEDIA_KICKOFF_RAN: AtomicBool = AtomicBool::new(false);
static MEDIA_DECODE_RAN: AtomicBool = AtomicBool::new(false);
static MEDIA_OUTPUT_SURFACE_PROBES_ENABLED: AtomicBool = AtomicBool::new(true);
static MEDIA_KICKOFF_STATE: Mutex<Option<MediaKickoffState>> = Mutex::new(None);
static MEDIA_BACKING: Mutex<Option<MediaBitstreamBacking>> = Mutex::new(None);
// Unlike the original fixed low-address page table, this root can accept a
// leased UI4 SFC target before submission and remove it after retirement.
// The root physical address remains stable for the lifetime of the VDBOX
// context, so adding a target never requires replacing the context image.
static MEDIA_PPGTT: Mutex<Option<crate::intel::ppgtt::SparsePpgtt>> = Mutex::new(None);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum MediaCodecMode {
    TransportProbe,
    AvcDecode,
    JpegDecode,
    AvcEncode,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum MediaSubmissionOwner {
    Execlists,
    Guc,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum MediaBatchLevel {
    FirstLevel,
    SecondLevelReturn,
}

/// Complete execution contract selected before a media job mutates context or
/// engine state. The engine is supplied separately so VCS0 and VCS2 retain
/// independent ownership, quarantine, and last-completed state.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct MediaJobMode {
    pub(super) codec: MediaCodecMode,
    pub(super) owner: MediaSubmissionOwner,
    pub(super) batch_level: MediaBatchLevel,
}

impl MediaJobMode {
    pub(super) const TRANSPORT_PROBE_GUC: Self = Self {
        codec: MediaCodecMode::TransportProbe,
        owner: MediaSubmissionOwner::Guc,
        batch_level: MediaBatchLevel::FirstLevel,
    };
    pub(super) const AVC_DECODE_GUC: Self = Self {
        codec: MediaCodecMode::AvcDecode,
        owner: MediaSubmissionOwner::Guc,
        batch_level: MediaBatchLevel::FirstLevel,
    };
    pub(super) const JPEG_DECODE_EXECLISTS: Self = Self {
        codec: MediaCodecMode::JpegDecode,
        owner: MediaSubmissionOwner::Execlists,
        batch_level: MediaBatchLevel::FirstLevel,
    };
    pub(super) const AVC_ENCODE_GUC: Self = Self {
        codec: MediaCodecMode::AvcEncode,
        owner: MediaSubmissionOwner::Guc,
        batch_level: MediaBatchLevel::SecondLevelReturn,
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct MediaActiveJob {
    mode: MediaJobMode,
    generation: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct MediaSessionReservation {
    mode: MediaJobMode,
    generation: u64,
}

#[derive(Copy, Clone)]
struct MediaExecutionState {
    active: Option<MediaActiveJob>,
    reservation: Option<MediaSessionReservation>,
    quarantined: Option<MediaJobMode>,
    last_completed: Option<MediaJobMode>,
    next_generation: u64,
}

impl MediaExecutionState {
    const EMPTY: Self = Self {
        active: None,
        reservation: None,
        quarantined: None,
        last_completed: None,
        next_generation: 0,
    };

    fn allocate_generation(&mut self) -> u64 {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        self.next_generation
    }
}

static MEDIA_ENGINE_EXECUTION: Mutex<[MediaExecutionState; MAX_MEDIA_ENGINES]> =
    Mutex::new([MediaExecutionState::EMPTY; MAX_MEDIA_ENGINES]);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum MediaLaneAcquireError {
    Busy,
    Quarantined,
}

pub(super) const MEDIA_INTERLEAVE_WAIT_NS: u64 = 50_000_000;

pub(super) struct MediaLaneGuard {
    engine_instance: usize,
    mode: MediaJobMode,
    generation: u64,
    requires_reactivation: bool,
    completed: bool,
    release_on_drop: bool,
}

impl MediaLaneGuard {
    pub(super) fn quarantine(mut self) {
        let mut states = MEDIA_ENGINE_EXECUTION.lock();
        let state = &mut states[self.engine_instance];
        let active = MediaActiveJob {
            mode: self.mode,
            generation: self.generation,
        };
        if state.active == Some(active) {
            state.quarantined = Some(self.mode);
        }
        self.release_on_drop = false;
    }

    pub(super) const fn mode(&self) -> MediaJobMode {
        self.mode
    }

    pub(super) const fn requires_reactivation(&self) -> bool {
        self.requires_reactivation
    }

    pub(super) fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for MediaLaneGuard {
    fn drop(&mut self) {
        if self.release_on_drop {
            let mut states = MEDIA_ENGINE_EXECUTION.lock();
            let state = &mut states[self.engine_instance];
            let active = MediaActiveJob {
                mode: self.mode,
                generation: self.generation,
            };
            if state.active == Some(active) {
                state.last_completed = if self.completed {
                    Some(self.mode)
                } else {
                    None
                };
                state.active = None;
            }
        }
    }
}

pub(crate) struct MediaSessionGuard {
    engine: MediaEngineDescriptor,
    reservation: MediaSessionReservation,
}

impl MediaSessionGuard {
    pub(crate) const fn generation(&self) -> u64 {
        self.reservation.generation
    }

    pub(crate) const fn engine_name(&self) -> &'static str {
        self.engine.name
    }
}

impl Drop for MediaSessionGuard {
    fn drop(&mut self) {
        let mut states = MEDIA_ENGINE_EXECUTION.lock();
        let state = &mut states[self.engine.id.instance as usize];
        if state.reservation == Some(self.reservation) {
            state.reservation = None;
        }
    }
}

pub(super) fn try_reserve_avc_decode_session() -> Result<MediaSessionGuard, MediaLaneAcquireError> {
    let (engine, _) = default_decode_engine_and_window();
    let mut states = MEDIA_ENGINE_EXECUTION.lock();
    let Some(state) = states.get_mut(engine.id.instance as usize) else {
        return Err(MediaLaneAcquireError::Quarantined);
    };
    if state.quarantined.is_some() {
        return Err(MediaLaneAcquireError::Quarantined);
    }
    if state.reservation.is_some() || state.active.is_some() {
        return Err(MediaLaneAcquireError::Busy);
    }
    let reservation = MediaSessionReservation {
        mode: MediaJobMode::AVC_DECODE_GUC,
        generation: state.allocate_generation(),
    };
    state.reservation = Some(reservation);
    Ok(MediaSessionGuard {
        engine,
        reservation,
    })
}

pub(super) fn try_acquire_media_lane(
    engine: MediaEngineDescriptor,
    mode: MediaJobMode,
    session_generation: Option<u64>,
) -> Result<MediaLaneGuard, MediaLaneAcquireError> {
    let engine_instance = engine.id.instance as usize;
    let mut states = MEDIA_ENGINE_EXECUTION.lock();
    let Some(state) = states.get_mut(engine_instance) else {
        return Err(MediaLaneAcquireError::Quarantined);
    };
    if state.quarantined.is_some() {
        return Err(MediaLaneAcquireError::Quarantined);
    }
    match (state.reservation, session_generation) {
        (Some(reservation), Some(generation))
            if reservation.mode == mode && reservation.generation == generation => {}
        // On a one-VDBOX fallback SKU, live encode may take bounded frame turns
        // inside the playback reservation. Two-VDBOX platforms never enter
        // this branch because encode and decode own different state slots.
        (Some(reservation), None)
            if reservation.mode == MediaJobMode::AVC_DECODE_GUC
                && mode == MediaJobMode::AVC_ENCODE_GUC => {}
        (None, None) => {}
        _ => return Err(MediaLaneAcquireError::Busy),
    }
    if state.active.is_some() {
        return Err(MediaLaneAcquireError::Busy);
    }
    let generation = state.allocate_generation();
    let requires_reactivation = state.last_completed != Some(mode);
    state.active = Some(MediaActiveJob { mode, generation });
    Ok(MediaLaneGuard {
        engine_instance,
        mode,
        generation,
        requires_reactivation,
        completed: false,
        release_on_drop: true,
    })
}

pub(super) fn acquire_media_lane_bounded(
    engine: MediaEngineDescriptor,
    mode: MediaJobMode,
    session_generation: Option<u64>,
    wait_ns: u64,
) -> Result<MediaLaneGuard, MediaLaneAcquireError> {
    match try_acquire_media_lane(engine, mode, session_generation) {
        Ok(lane) => return Ok(lane),
        Err(MediaLaneAcquireError::Quarantined) => {
            return Err(MediaLaneAcquireError::Quarantined);
        }
        Err(MediaLaneAcquireError::Busy) => {}
    }

    let deadline = crate::chronos::monotonic_nanos().saturating_add(wait_ns);
    loop {
        match try_acquire_media_lane(engine, mode, session_generation) {
            Err(MediaLaneAcquireError::Busy) if crate::chronos::monotonic_nanos() < deadline => {
                core::hint::spin_loop();
            }
            result => return result,
        }
    }
}

pub(crate) fn set_output_surface_probes_enabled(enabled: bool) -> bool {
    MEDIA_OUTPUT_SURFACE_PROBES_ENABLED.swap(enabled, Ordering::AcqRel)
}

pub(super) fn output_surface_probes_enabled() -> bool {
    MEDIA_OUTPUT_SURFACE_PROBES_ENABLED.load(Ordering::Acquire)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaEngineClass {
    VideoDecode,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaProvisioning {
    Kickoff,
    Disabled,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaWorkloadKind {
    DecodeBitstream,
    DecodeFrame,
    EncodeFrame,
    SessionSnapshot,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaSubmissionTransport {
    Execlists,
    Guc,
    Disabled,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MediaKickoffStage {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    SubmissionWiring,
    CommandEncoding,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaEngineId {
    pub class: MediaEngineClass,
    pub instance: u8,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaCapabilities {
    pub decode: bool,
    pub enhance: bool,
    pub huc_assist: bool,
    /// The platform exposes an SFC associated with this media engine.
    pub sfc: bool,
    /// TRUEOS can encode, bind, submit, and retire a complete VD-to-SFC job.
    /// Keep this separate from physical capability so topology discovery can
    /// never accidentally enable an incomplete command stream.
    pub sfc_programmed: bool,
    pub relative_mmio_lrc: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaEngineDescriptor {
    pub id: MediaEngineId,
    pub name: &'static str,
    pub ring_base: usize,
    pub provisioning: MediaProvisioning,
    pub capabilities: MediaCapabilities,
    pub default_workload: MediaWorkloadKind,
}

impl MediaEngineDescriptor {
    const fn unused() -> Self {
        Self {
            id: MediaEngineId {
                class: MediaEngineClass::VideoDecode,
                instance: 0,
            },
            name: "unused",
            ring_base: 0,
            provisioning: MediaProvisioning::Disabled,
            capabilities: MediaCapabilities {
                decode: false,
                enhance: false,
                huc_assist: false,
                sfc: false,
                sfc_programmed: false,
                relative_mmio_lrc: false,
            },
            default_workload: MediaWorkloadKind::SessionSnapshot,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaGpuWindowLayout {
    pub ring_gpu_addr: u64,
    pub context_gpu_addr: u64,
    pub batch_gpu_addr: u64,
    pub bitstream_gpu_addr: u64,
    pub output_surface_gpu_addr: u64,
    pub avc_scratch_gpu_addr: u64,
    pub result_gpu_addr: u64,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaEngineRuntimeSnapshot {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub name: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub ring_base: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub observed: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub tail: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub head: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub start: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub ctl: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub acthd: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub mi_mode: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub mode: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub ctx_ctl: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub execlist_ctl: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub execlist_status_lo: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub execlist_status_hi: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub ipeir: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub ipehr: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub instdone: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub instps: u32,
}

impl MediaEngineRuntimeSnapshot {
    const fn unused() -> Self {
        Self {
            name: "unused",
            ring_base: 0,
            observed: false,
            tail: 0,
            head: 0,
            start: 0,
            ctl: 0,
            acthd: 0,
            mi_mode: 0,
            mode: 0,
            ctx_ctl: 0,
            execlist_ctl: 0,
            execlist_status_lo: 0,
            execlist_status_hi: 0,
            ipeir: 0,
            ipehr: 0,
            instdone: 0,
            instps: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaSliceWakeAck {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub name: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub value: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub awake: bool,
}

impl MediaSliceWakeAck {
    const fn empty() -> Self {
        Self {
            name: "unused",
            value: 0,
            awake: false,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(super) struct MediaEngineForcewakeAck {
    ack_reg: usize,
    ack_value: u32,
    awake: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaForcewakeSnapshot {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub global_req: u32,
    pub global_ack: u32,
    pub awake_count: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub slice_count: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub slices: [MediaSliceWakeAck; 8],
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaApiRoute {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub name: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub workload: MediaWorkloadKind,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub preferred_engine_class: Option<MediaEngineClass>,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub transport: MediaSubmissionTransport,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub summary: &'static str,
}

impl MediaApiRoute {
    const fn empty() -> Self {
        Self {
            name: "unused",
            workload: MediaWorkloadKind::SessionSnapshot,
            preferred_engine_class: None,
            transport: MediaSubmissionTransport::Disabled,
            summary: "",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaApiShape {
    pub route_count: usize,
    pub routes: [MediaApiRoute; MAX_MEDIA_API_ROUTES],
}

impl MediaApiShape {
    const fn empty() -> Self {
        Self {
            route_count: 0,
            routes: [MediaApiRoute::empty(); MAX_MEDIA_API_ROUTES],
        }
    }
}

/// Encode-specific readiness is intentionally separate from VDBOX decode
/// readiness. Alder/Raptor Lake hardware can encode, but TRUEOS must not route
/// a boot workload there until every software-owned gate is present.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaEncodeReadiness {
    pub device_claimed: bool,
    pub vdbox_discovered: bool,
    pub guc_transport_ready: bool,
    pub guc_media_context_wired: bool,
    pub guc_media_transport_probe_passed: bool,
    pub avc_encode_commands_wired: bool,
    pub avc_encode_probe_passed: bool,
    pub coded_bitstream_output_wired: bool,
    pub ready: bool,
}

pub(crate) fn encode_readiness() -> MediaEncodeReadiness {
    let device_claimed = super::claimed_device().is_some();
    let topology = current_topology();
    let vdbox_discovered = topology
        .engines
        .iter()
        .take(topology.active_engine_count)
        .any(|engine| engine.capabilities.decode);
    let guc_transport_ready = crate::intel::guc_submission_ready();

    // The live AVC consumers are GuC-owned: encode targets VCS0 and playback
    // decode selects the second fuse-discovered VDBOX (VCS2 on ADL-S). The
    // transport probe still validates VCS0 before encoder admission; playback
    // independently validates VCS2 through its first real completion marker.
    let guc_media_context_wired = true;
    let guc_media_transport_probe_passed = super::guc_probe::passed();
    #[cfg(feature = "trueos_h264_encode_stream")]
    let avc_encode_commands_wired = super::avc_encode_probe::commands_wired();
    #[cfg(not(feature = "trueos_h264_encode_stream"))]
    let avc_encode_commands_wired = false;
    #[cfg(feature = "trueos_h264_encode_stream")]
    let avc_encode_probe_passed = super::avc_encode_probe::passed();
    #[cfg(not(feature = "trueos_h264_encode_stream"))]
    let avc_encode_probe_passed = false;
    #[cfg(feature = "trueos_h264_encode_stream")]
    let coded_bitstream_output_wired = super::avc_encode_probe::coded_output_validated();
    #[cfg(not(feature = "trueos_h264_encode_stream"))]
    let coded_bitstream_output_wired = false;
    let ready = device_claimed
        && vdbox_discovered
        && guc_transport_ready
        && guc_media_context_wired
        && guc_media_transport_probe_passed
        && avc_encode_commands_wired
        && avc_encode_probe_passed
        && coded_bitstream_output_wired;
    MediaEncodeReadiness {
        device_claimed,
        vdbox_discovered,
        guc_transport_ready,
        guc_media_context_wired,
        guc_media_transport_probe_passed,
        avc_encode_commands_wired,
        avc_encode_probe_passed,
        coded_bitstream_output_wired,
        ready,
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaTopology {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub sku_name: &'static str,
    pub active_engine_count: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub planned_engine_count: usize,
    pub engines: [MediaEngineDescriptor; MAX_MEDIA_ENGINES],
    pub default_decode: Option<MediaEngineId>,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub default_enhance: Option<MediaEngineId>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaSurfaceProbeBand {
    pub signature: u32,
    pub active_samples: usize,
    pub sample_count: usize,
    pub min_value: u8,
    pub max_value: u8,
}

impl MediaSurfaceProbeBand {
    const fn empty() -> Self {
        Self {
            signature: 0,
            active_samples: 0,
            sample_count: 0,
            min_value: 0,
            max_value: 0,
        }
    }

    fn has_range(self) -> bool {
        self.sample_count != 0 && self.min_value != self.max_value
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaSurfaceProbe {
    pub valid: bool,
    pub luma_visible_last_row: MediaSurfaceProbeBand,
    pub luma_visible_tail8_row: MediaSurfaceProbeBand,
    pub luma_storage_pad_first_row: MediaSurfaceProbeBand,
    pub luma_storage_pad_last_row: MediaSurfaceProbeBand,
    pub luma_center_band: MediaSurfaceProbeBand,
    pub luma_prev_mb_row: MediaSurfaceProbeBand,
    pub luma_bottom_mb_row: MediaSurfaceProbeBand,
    pub cb_center_band: MediaSurfaceProbeBand,
    pub cb_center_hi_band: MediaSurfaceProbeBand,
    pub cb_prev_mb_row: MediaSurfaceProbeBand,
    pub cb_bottom_mb_row: MediaSurfaceProbeBand,
    pub cr_center_band: MediaSurfaceProbeBand,
    pub cr_prev_mb_row: MediaSurfaceProbeBand,
    pub cr_bottom_mb_row: MediaSurfaceProbeBand,
}

impl MediaSurfaceProbe {
    const fn empty() -> Self {
        Self {
            valid: false,
            luma_visible_last_row: MediaSurfaceProbeBand::empty(),
            luma_visible_tail8_row: MediaSurfaceProbeBand::empty(),
            luma_storage_pad_first_row: MediaSurfaceProbeBand::empty(),
            luma_storage_pad_last_row: MediaSurfaceProbeBand::empty(),
            luma_center_band: MediaSurfaceProbeBand::empty(),
            luma_prev_mb_row: MediaSurfaceProbeBand::empty(),
            luma_bottom_mb_row: MediaSurfaceProbeBand::empty(),
            cb_center_band: MediaSurfaceProbeBand::empty(),
            cb_center_hi_band: MediaSurfaceProbeBand::empty(),
            cb_prev_mb_row: MediaSurfaceProbeBand::empty(),
            cb_bottom_mb_row: MediaSurfaceProbeBand::empty(),
            cr_center_band: MediaSurfaceProbeBand::empty(),
            cr_prev_mb_row: MediaSurfaceProbeBand::empty(),
            cr_bottom_mb_row: MediaSurfaceProbeBand::empty(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaDecodeFrameState {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub ready: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub engine_name: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub ring_gpu_addr: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub context_gpu_addr: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub batch_gpu_addr: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub result_gpu_addr: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub bitstream_gpu_addr: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub output_surface_gpu_addr: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub bitstream_phys: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub output_surface_phys: u64,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub bitstream_bytes: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub output_surface_bytes: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub frame_width: u16,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub frame_height: u16,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub output_surface_pitch: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub sample_nal_count: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub has_idr: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub kickoff_marker: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub complete_marker: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub output_surface_signature: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub output_surface_nonzero_samples: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub output_surface_probe: MediaSurfaceProbe,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub submit_completed: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub present_attempted: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub present_ready: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub synthetic_preview: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MediaKickoffState {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub topology: MediaTopology,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub runtime_count: usize,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub runtimes: [MediaEngineRuntimeSnapshot; MAX_MEDIA_ENGINES],
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub wake: MediaForcewakeSnapshot,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub api: MediaApiShape,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub preferred_transport: MediaSubmissionTransport,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub guc_ready: bool,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub guc_status: u32,
    pub stage: MediaKickoffStage,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub last_decode_frame: Option<MediaDecodeFrameState>,
}

#[derive(Copy, Clone, Debug)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct Media2FirstFrameState {
    pub ready: bool,
    pub submit_completed: bool,
    pub present_ready: bool,
    pub frame_width: u16,
    pub frame_height: u16,
    pub output_surface_pitch: usize,
    pub output_surface_bytes: usize,
    pub output_surface_signature: u32,
    pub output_surface_nonzero_samples: usize,
    pub output_surface_probe: MediaSurfaceProbe,
    pub bitstream_bytes: usize,
    pub sample_nal_count: usize,
    pub has_idr: bool,
}

#[derive(Copy, Clone)]
pub(super) struct MediaBitstreamBacking {
    pub(super) ring_phys: u64,
    pub(super) ring_virt: *mut u8,
    pub(super) ring_bytes: usize,
    pub(super) context_phys: u64,
    pub(super) context_virt: *mut u8,
    pub(super) context_bytes: usize,
    pub(super) batch_phys: u64,
    pub(super) batch_virt: *mut u8,
    pub(super) batch_bytes: usize,
    pub(super) result_phys: u64,
    pub(super) result_virt: *mut u8,
    pub(super) result_bytes: usize,
    pub(super) bitstream_phys: u64,
    pub(super) bitstream_virt: *mut u8,
    pub(super) bitstream_bytes: usize,
    pub(super) output_surface_phys: u64,
    pub(super) output_surface_virt: *mut u8,
    pub(super) output_surface_bytes: usize,
    pub(super) avc_scratch_phys: u64,
    pub(super) avc_scratch_virt: *mut u8,
    pub(super) avc_scratch_bytes: usize,
    pub(super) ppgtt_pml4_phys: u64,
}

unsafe impl Send for MediaBitstreamBacking {}
unsafe impl Sync for MediaBitstreamBacking {}

#[inline]
fn media_msg_slice_regs() -> [(&'static str, usize); 8] {
    [
        ("vdbox0", FORCEWAKE_ACK_VDBOX0),
        ("vdbox1", FORCEWAKE_ACK_VDBOX1),
        ("vdbox2", FORCEWAKE_ACK_VDBOX2),
        ("vdbox3", FORCEWAKE_ACK_VDBOX3),
        ("vebox0", FORCEWAKE_ACK_VEBOX0),
        ("vebox1", FORCEWAKE_ACK_VEBOX1),
        ("vebox2", FORCEWAKE_ACK_VEBOX2),
        ("vebox3", FORCEWAKE_ACK_VEBOX3),
    ]
}

fn current_topology() -> MediaTopology {
    let decode0 = MediaEngineDescriptor {
        id: MediaEngineId {
            class: MediaEngineClass::VideoDecode,
            instance: 0,
        },
        name: "vcs0",
        ring_base: GEN11_VCS0_RING_BASE,
        provisioning: MediaProvisioning::Kickoff,
        capabilities: MediaCapabilities {
            decode: true,
            enhance: false,
            huc_assist: false,
            sfc: true,
            sfc_programmed: false,
            relative_mmio_lrc: true,
        },
        default_workload: MediaWorkloadKind::DecodeFrame,
    };
    let decode2 = MediaEngineDescriptor {
        id: MediaEngineId {
            class: MediaEngineClass::VideoDecode,
            instance: 2,
        },
        name: "vcs2",
        ring_base: GEN11_VCS2_RING_BASE,
        provisioning: MediaProvisioning::Kickoff,
        capabilities: MediaCapabilities {
            decode: true,
            enhance: false,
            huc_assist: false,
            sfc: true,
            sfc_programmed: false,
            relative_mmio_lrc: true,
        },
        default_workload: MediaWorkloadKind::DecodeFrame,
    };
    let physical_mask = crate::intel::claimed_device()
        .map(crate::intel::media_vdbox_mask)
        .unwrap_or(0);
    let mut engines = [MediaEngineDescriptor::unused(); MAX_MEDIA_ENGINES];
    let mut active_engine_count = 0usize;
    for candidate in [decode0, decode2] {
        if physical_mask & (1 << candidate.id.instance) != 0 {
            engines[active_engine_count] = candidate;
            active_engine_count += 1;
        }
    }
    let default_decode = if physical_mask & (1 << decode2.id.instance) != 0 {
        Some(decode2.id)
    } else if physical_mask & (1 << decode0.id.instance) != 0 {
        Some(decode0.id)
    } else {
        None
    };
    MediaTopology {
        sku_name: "xelp-adls-fuse-discovered",
        active_engine_count,
        planned_engine_count: 2,
        engines,
        default_decode,
        default_enhance: None,
    }
}

fn current_api_shape(transport: MediaSubmissionTransport) -> MediaApiShape {
    let mut api = MediaApiShape::empty();
    api.route_count = 4;
    api.routes[0] = MediaApiRoute {
        name: "media.avc.playback.decode",
        workload: MediaWorkloadKind::DecodeFrame,
        preferred_engine_class: Some(MediaEngineClass::VideoDecode),
        transport,
        summary: "submit playback H.264 decode through its GuC-owned VDBOX",
    };
    api.routes[1] = MediaApiRoute {
        name: "media.avc.rdp.encode",
        workload: MediaWorkloadKind::EncodeFrame,
        preferred_engine_class: Some(MediaEngineClass::VideoDecode),
        transport,
        summary: "submit RDP H.264 encode through its GuC-owned VDBOX",
    };
    api.routes[2] = MediaApiRoute {
        name: "media.jpeg.submit",
        workload: MediaWorkloadKind::DecodeBitstream,
        preferred_engine_class: Some(MediaEngineClass::VideoDecode),
        transport: MediaSubmissionTransport::Execlists,
        summary: "legacy boot-logo JPEG path outside live AVC scenarios",
    };
    api.routes[3] = MediaApiRoute {
        name: "media.observe.snapshot",
        workload: MediaWorkloadKind::SessionSnapshot,
        preferred_engine_class: None,
        transport: MediaSubmissionTransport::Disabled,
        summary: "snapshot forcewake and live VCS registers",
    };
    api
}

fn engine_window(slot: usize) -> MediaGpuWindowLayout {
    let base = MEDIA_ENGINE_GPU_ADDR_BASE + (slot as u64) * MEDIA_ENGINE_GPU_ADDR_STRIDE;
    MediaGpuWindowLayout {
        ring_gpu_addr: base,
        context_gpu_addr: base + 0x0001_0000,
        batch_gpu_addr: base + 0x0008_0000,
        result_gpu_addr: base + 0x0010_0000,
        bitstream_gpu_addr: base + 0x0020_0000,
        output_surface_gpu_addr: base + 0x00A0_0000,
        avc_scratch_gpu_addr: base + 0x0320_0000,
    }
}

pub(super) fn default_decode_engine_and_window() -> (MediaEngineDescriptor, MediaGpuWindowLayout) {
    let topology = current_topology();
    let index = topology
        .default_decode
        .and_then(|id| {
            topology
                .engines
                .iter()
                .take(topology.active_engine_count)
                .position(|engine| engine.id == id)
        })
        .unwrap_or(0);
    (topology.engines[index], engine_window(index))
}

pub(super) fn default_encode_engine_and_window() -> (MediaEngineDescriptor, MediaGpuWindowLayout) {
    let topology = current_topology();
    let index = topology
        .engines
        .iter()
        .take(topology.active_engine_count)
        .position(|engine| engine.id.instance == 0)
        .unwrap_or(0);
    (topology.engines[index], engine_window(index))
}

fn preferred_transport() -> MediaSubmissionTransport {
    MediaSubmissionTransport::Guc
}

fn snapshot_forcewake(dev: crate::intel::Dev) -> MediaForcewakeSnapshot {
    let mut slices = [MediaSliceWakeAck::empty(); 8];
    let mut awake_count = 0usize;
    for (idx, (name, reg)) in media_msg_slice_regs().into_iter().enumerate() {
        let value = super::mmio_read(dev, reg);
        let awake = (value & FORCEWAKE_KERNEL) != 0;
        awake_count += usize::from(awake);
        slices[idx] = MediaSliceWakeAck { name, value, awake };
    }
    MediaForcewakeSnapshot {
        global_req: super::mmio_read(dev, FORCEWAKE_MEDIA_GEN11),
        global_ack: super::mmio_read(dev, FORCEWAKE_ACK_MEDIA),
        awake_count,
        slice_count: slices.len(),
        slices,
    }
}

fn snapshot_runtime(
    dev: crate::intel::Dev,
    desc: MediaEngineDescriptor,
) -> MediaEngineRuntimeSnapshot {
    let base = desc.ring_base;
    let tail = super::mmio_read(dev, base + RING_TAIL);
    let head = super::mmio_read(dev, base + RING_HEAD);
    let start = super::mmio_read(dev, base + RING_START);
    let ctl = super::mmio_read(dev, base + RING_CTL);
    let acthd = super::mmio_read(dev, base + RING_ACTHD);
    let mi_mode = super::mmio_read(dev, base + RING_MI_MODE);
    let mode = super::mmio_read(dev, base + RING_MODE_GEN7);
    let ctx_ctl = super::mmio_read(dev, base + RING_CONTEXT_CONTROL);
    let execlist_ctl = super::mmio_read(dev, base + RING_EXECLIST_CONTROL);
    let execlist_status_lo = super::mmio_read(dev, base + RING_EXECLIST_STATUS_LO);
    let execlist_status_hi = super::mmio_read(dev, base + RING_EXECLIST_STATUS_HI);
    let ipeir = super::mmio_read(dev, base + RING_IPEIR);
    let ipehr = super::mmio_read(dev, base + RING_IPEHR);
    let instdone = super::mmio_read(dev, base + RING_INSTDONE);
    let instps = super::mmio_read(dev, base + RING_INSTPS);
    let observed = tail != 0
        || head != 0
        || start != 0
        || ctl != 0
        || acthd != 0
        || ctx_ctl != 0
        || execlist_status_lo != 0
        || execlist_status_hi != 0;

    MediaEngineRuntimeSnapshot {
        name: desc.name,
        ring_base: desc.ring_base,
        observed,
        tail,
        head,
        start,
        ctl,
        acthd,
        mi_mode,
        mode,
        ctx_ctl,
        execlist_ctl,
        execlist_status_lo,
        execlist_status_hi,
        ipeir,
        ipehr,
        instdone,
        instps,
    }
}

fn rebuild_kickoff_state(stage: MediaKickoffStage) -> Option<MediaKickoffState> {
    let dev = super::claimed_device()?;
    let topology = current_topology();
    let transport = preferred_transport();
    let mut runtimes = [MediaEngineRuntimeSnapshot::unused(); MAX_MEDIA_ENGINES];
    for (idx, desc) in topology
        .engines
        .iter()
        .take(topology.active_engine_count)
        .copied()
        .enumerate()
    {
        runtimes[idx] = snapshot_runtime(dev, desc);
    }
    Some(MediaKickoffState {
        topology,
        runtime_count: topology.active_engine_count,
        runtimes,
        wake: snapshot_forcewake(dev),
        api: current_api_shape(transport),
        preferred_transport: transport,
        guc_ready: false,
        guc_status: 0,
        stage,
        last_decode_frame: None,
    })
}

/// Capture current media forcewake and VCS register state without submitting,
/// resetting, or reconfiguring an engine.
pub(crate) fn diagnostic_snapshot() -> Option<MediaKickoffState> {
    let stage = MEDIA_KICKOFF_STATE
        .lock()
        .as_ref()
        .map(|state| state.stage)
        .unwrap_or(MediaKickoffStage::CommandEncoding);
    rebuild_kickoff_state(stage)
}

pub(crate) fn kickoff_ran() -> bool {
    MEDIA_KICKOFF_RAN.load(Ordering::Acquire)
}

pub(crate) fn decode_ran() -> bool {
    MEDIA_DECODE_RAN.load(Ordering::Acquire)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn store_kickoff_state(stage: MediaKickoffStage) {
    *MEDIA_KICKOFF_STATE.lock() = rebuild_kickoff_state(stage);
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn kickoff_once() {
    MEDIA_KICKOFF_RAN.store(true, Ordering::Release);
    store_kickoff_state(MediaKickoffStage::CommandEncoding);
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) async fn run_media2_first_frame_async() -> Option<Media2FirstFrameState> {
    kickoff_once();
    crate::log!("intel/media2: disabled reason=jpeg-only-engine-cut\n");
    store_kickoff_state(MediaKickoffStage::SubmissionWiring);
    MEDIA_DECODE_RAN.store(true, Ordering::Release);
    None
}

pub(super) fn sample_buffer_dword(
    base_virt: *mut u8,
    buffer_bytes: usize,
    offset_bytes: usize,
) -> u32 {
    if offset_bytes.saturating_add(core::mem::size_of::<u32>()) > buffer_bytes {
        return 0;
    }
    unsafe { core::ptr::read_volatile(base_virt.add(offset_bytes) as *const u32) }
}

pub(super) fn classify_media_acthd(
    acthd: u32,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    batch_tail_bytes: usize,
    ring_tail_bytes: usize,
) -> (&'static str, u32, u32) {
    let acthd_aligned = acthd & !0x3;
    let regions = [
        ("ring", windows.ring_gpu_addr, ring_tail_bytes, backing.ring_virt),
        ("batch", windows.batch_gpu_addr, batch_tail_bytes, backing.batch_virt),
        ("bitstream", windows.bitstream_gpu_addr, backing.bitstream_bytes, backing.bitstream_virt),
        (
            "output",
            windows.output_surface_gpu_addr,
            backing.output_surface_bytes,
            backing.output_surface_virt,
        ),
        (
            "avc_scratch",
            windows.avc_scratch_gpu_addr,
            backing.avc_scratch_bytes,
            backing.avc_scratch_virt,
        ),
    ];

    for (name, gpu_addr, buffer_bytes, base_virt) in regions {
        let base = gpu_addr as u32;
        if acthd_aligned < base {
            continue;
        }
        let offset = acthd_aligned.wrapping_sub(base) as usize;
        if offset < buffer_bytes {
            return (name, offset as u32, sample_buffer_dword(base_virt, buffer_bytes, offset));
        }
    }
    ("unknown", 0, 0)
}

pub(super) fn marker_base(engine: MediaEngineDescriptor) -> u32 {
    0x4D44_1000 + (engine.id.instance as u32) * 0x100
}

pub(super) fn surface_signature(bytes: &[u8]) -> (u32, usize) {
    let sample_count = bytes.len().min(4096);
    if sample_count == 0 {
        return (0, 0);
    }
    let step = (bytes.len() / sample_count.max(1)).max(1);
    let mut signature = 0u32;
    let mut nonzero = 0usize;
    let mut idx = 0usize;
    let mut seen = 0usize;
    while idx < bytes.len() && seen < sample_count {
        let value = bytes[idx];
        signature = signature.rotate_left(5) ^ u32::from(value);
        nonzero += usize::from(value != 0);
        idx = idx.saturating_add(step);
        seen += 1;
    }
    (signature, nonzero)
}

fn byte_signature(bytes: &[u8]) -> u32 {
    let mut signature = 0u32;
    for &value in bytes.iter().take(4096) {
        signature = signature.rotate_left(5) ^ u32::from(value);
    }
    signature
}

pub(crate) const MEDIA_TILE64_W: usize = 256;
pub(super) const MEDIA_TILE64_H: usize = 256;
const MEDIA_YTILE_W: usize = 128;
const MEDIA_YTILE_H: usize = 32;
pub(super) const MEDIA_NV12_BLACK_LUMA: u8 = 16;
pub(super) const MEDIA_NV12_NEUTRAL_CHROMA: u8 = 128;
pub(super) const MEDIA_AVC_ROLLBACK_CLEAR_LUMA: u8 = 0;
pub(super) const MEDIA_AVC_ROLLBACK_CLEAR_CHROMA: u8 = 0;

pub(crate) fn media_tile64_nv12_surface_layout(
    coded_height: usize,
    output_pitch: usize,
) -> Option<(usize, usize)> {
    if coded_height == 0 || output_pitch == 0 || !output_pitch.is_multiple_of(MEDIA_TILE64_W) {
        return None;
    }
    let chroma_y_offset = coded_height.next_multiple_of(MEDIA_TILE64_H);
    let total_height = chroma_y_offset.saturating_add(coded_height.div_ceil(2));
    let bytes = total_height
        .div_ceil(MEDIA_TILE64_H)
        .saturating_mul(output_pitch)
        .saturating_mul(MEDIA_TILE64_H);
    Some((chroma_y_offset, bytes))
}

#[inline(always)]
pub(crate) fn media_tile64_8bpp_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
    let tile_col = byte_x / MEDIA_TILE64_W;
    let tile_row = row_y / MEDIA_TILE64_H;
    let u = byte_x % MEDIA_TILE64_W;
    let v = row_y % MEDIA_TILE64_H;
    let within_tile = ((u & 0x0f) << 0)
        | ((v & 0x03) << 4)
        | (((u >> 4) & 0x03) << 6)
        | (((v >> 2) & 0x01) << 8)
        | (((u >> 6) & 0x01) << 9)
        | (((v >> 3) & 0x03) << 10)
        | (((u >> 7) & 0x01) << 12)
        | (((v >> 5) & 0x07) << 13);
    (tile_row * tiles_per_row + tile_col) * (64 * 1024) + within_tile
}

#[inline(always)]
fn media_ytile_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
    let tile_col = byte_x / MEDIA_YTILE_W;
    let tile_row = row_y / MEDIA_YTILE_H;
    let in_x = byte_x % MEDIA_YTILE_W;
    let in_y = row_y % MEDIA_YTILE_H;
    let oword_col = in_x / 16;
    let byte_in_oword = in_x % 16;
    let within_tile = oword_col * 512 + in_y * 16 + byte_in_oword;
    (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
}

fn probe_tiled_rect(
    surface: &[u8],
    output_pitch: usize,
    byte_x: usize,
    row_y: usize,
    width: usize,
    row_count: usize,
    baseline: u8,
) -> Option<MediaSurfaceProbeBand> {
    if width == 0 || row_count == 0 || output_pitch < byte_x.saturating_add(width) {
        return None;
    }
    let tiles_per_row = output_pitch / MEDIA_YTILE_W;
    if tiles_per_row == 0 {
        return None;
    }
    let mut signature = 0u32;
    let mut active_samples = 0usize;
    let mut sample_count = 0usize;
    let mut min_value = u8::MAX;
    let mut max_value = u8::MIN;
    for row in row_y..row_y.saturating_add(row_count) {
        for col in byte_x..byte_x.saturating_add(width) {
            let value = *surface.get(media_ytile_offset(col, row, tiles_per_row))?;
            signature = signature.rotate_left(5) ^ u32::from(value);
            active_samples += usize::from(value != baseline);
            sample_count += 1;
            min_value = min_value.min(value);
            max_value = max_value.max(value);
        }
    }
    Some(MediaSurfaceProbeBand {
        signature,
        active_samples,
        sample_count,
        min_value,
        max_value,
    })
}

fn probe_tile64_rect(
    surface: &[u8],
    output_pitch: usize,
    byte_x: usize,
    row_y: usize,
    width: usize,
    row_count: usize,
    baseline: u8,
) -> Option<MediaSurfaceProbeBand> {
    if width == 0 || row_count == 0 || output_pitch < byte_x.saturating_add(width) {
        return None;
    }
    let tiles_per_row = output_pitch / MEDIA_TILE64_W;
    if tiles_per_row == 0 {
        return None;
    }
    let mut signature = 0u32;
    let mut active_samples = 0usize;
    let mut sample_count = 0usize;
    let mut min_value = u8::MAX;
    let mut max_value = u8::MIN;
    for row in row_y..row_y.saturating_add(row_count) {
        for col in byte_x..byte_x.saturating_add(width) {
            let value = *surface.get(media_tile64_8bpp_offset(col, row, tiles_per_row))?;
            signature = signature.rotate_left(5) ^ u32::from(value);
            active_samples += usize::from(value != baseline);
            sample_count += 1;
            min_value = min_value.min(value);
            max_value = max_value.max(value);
        }
    }
    Some(MediaSurfaceProbeBand {
        signature,
        active_samples,
        sample_count,
        min_value,
        max_value,
    })
}

fn probe_linear_rect(
    surface: &[u8],
    output_pitch: usize,
    byte_x: usize,
    row_y: usize,
    width: usize,
    row_count: usize,
    baseline: u8,
) -> Option<MediaSurfaceProbeBand> {
    if width == 0 || row_count == 0 || output_pitch < byte_x.saturating_add(width) {
        return None;
    }
    let mut signature = 0u32;
    let mut active_samples = 0usize;
    let mut sample_count = 0usize;
    let mut min_value = u8::MAX;
    let mut max_value = u8::MIN;
    for row in row_y..row_y.saturating_add(row_count) {
        let row_start = row.saturating_mul(output_pitch);
        for col in byte_x..byte_x.saturating_add(width) {
            let value = *surface.get(row_start.saturating_add(col))?;
            signature = signature.rotate_left(5) ^ u32::from(value);
            active_samples += usize::from(value != baseline);
            sample_count += 1;
            min_value = min_value.min(value);
            max_value = max_value.max(value);
        }
    }
    Some(MediaSurfaceProbeBand {
        signature,
        active_samples,
        sample_count,
        min_value,
        max_value,
    })
}

fn probe_linear_nv12_chroma_rect(
    surface: &[u8],
    output_pitch: usize,
    uv_offset: usize,
    pair_x: usize,
    row_y: usize,
    pair_width: usize,
    row_count: usize,
    component_offset: usize,
) -> Option<MediaSurfaceProbeBand> {
    if pair_width == 0 || row_count == 0 || component_offset > 1 {
        return None;
    }
    let byte_x = pair_x.saturating_mul(2).saturating_add(component_offset);
    let byte_width = pair_width.saturating_mul(2);
    if output_pitch < byte_x.saturating_add(byte_width.saturating_sub(1)) {
        return None;
    }
    let mut signature = 0u32;
    let mut active_samples = 0usize;
    let mut sample_count = 0usize;
    let mut min_value = u8::MAX;
    let mut max_value = u8::MIN;
    for row in row_y..row_y.saturating_add(row_count) {
        let row_start = uv_offset.saturating_add(row.saturating_mul(output_pitch));
        for pair in 0..pair_width {
            let value = *surface.get(row_start.saturating_add(byte_x + pair * 2))?;
            signature = signature.rotate_left(5) ^ u32::from(value);
            active_samples += usize::from(value != MEDIA_NV12_NEUTRAL_CHROMA);
            sample_count += 1;
            min_value = min_value.min(value);
            max_value = max_value.max(value);
        }
    }
    Some(MediaSurfaceProbeBand {
        signature,
        active_samples,
        sample_count,
        min_value,
        max_value,
    })
}

fn probe_tiled_nv12_chroma_rect(
    surface: &[u8],
    output_pitch: usize,
    chroma_y_offset: usize,
    pair_x: usize,
    row_y: usize,
    pair_width: usize,
    row_count: usize,
    component_offset: usize,
) -> Option<MediaSurfaceProbeBand> {
    if pair_width == 0 || row_count == 0 || component_offset > 1 {
        return None;
    }
    let tiles_per_row = output_pitch / MEDIA_TILE64_W;
    if tiles_per_row == 0 {
        return None;
    }
    let byte_x = pair_x.saturating_mul(2).saturating_add(component_offset);
    let byte_width = pair_width.saturating_mul(2);
    if output_pitch < byte_x.saturating_add(byte_width.saturating_sub(1)) {
        return None;
    }
    let mut signature = 0u32;
    let mut active_samples = 0usize;
    let mut sample_count = 0usize;
    let mut min_value = u8::MAX;
    let mut max_value = u8::MIN;
    for row in row_y..row_y.saturating_add(row_count) {
        let tiled_row = chroma_y_offset.saturating_add(row);
        for pair in 0..pair_width {
            let tiled_x = byte_x + pair * 2;
            let value =
                *surface.get(media_tile64_8bpp_offset(tiled_x, tiled_row, tiles_per_row))?;
            signature = signature.rotate_left(5) ^ u32::from(value);
            active_samples += usize::from(value != MEDIA_NV12_NEUTRAL_CHROMA);
            sample_count += 1;
            min_value = min_value.min(value);
            max_value = max_value.max(value);
        }
    }
    Some(MediaSurfaceProbeBand {
        signature,
        active_samples,
        sample_count,
        min_value,
        max_value,
    })
}

fn probe_ytile_nv12_chroma_rect(
    surface: &[u8],
    output_pitch: usize,
    chroma_y_offset: usize,
    pair_x: usize,
    row_y: usize,
    pair_width: usize,
    row_count: usize,
    component_offset: usize,
) -> Option<MediaSurfaceProbeBand> {
    if pair_width == 0 || row_count == 0 || component_offset > 1 {
        return None;
    }
    let tiles_per_row = output_pitch / MEDIA_YTILE_W;
    if tiles_per_row == 0 {
        return None;
    }
    let byte_x = pair_x.saturating_mul(2).saturating_add(component_offset);
    let byte_width = pair_width.saturating_mul(2);
    if output_pitch < byte_x.saturating_add(byte_width.saturating_sub(1)) {
        return None;
    }
    let mut signature = 0u32;
    let mut active_samples = 0usize;
    let mut sample_count = 0usize;
    let mut min_value = u8::MAX;
    let mut max_value = u8::MIN;
    for row in row_y..row_y.saturating_add(row_count) {
        let tiled_row = chroma_y_offset.saturating_add(row);
        for pair in 0..pair_width {
            let tiled_x = byte_x + pair * 2;
            let value = *surface.get(media_ytile_offset(tiled_x, tiled_row, tiles_per_row))?;
            signature = signature.rotate_left(5) ^ u32::from(value);
            active_samples += usize::from(value != MEDIA_NV12_NEUTRAL_CHROMA);
            sample_count += 1;
            min_value = min_value.min(value);
            max_value = max_value.max(value);
        }
    }
    Some(MediaSurfaceProbeBand {
        signature,
        active_samples,
        sample_count,
        min_value,
        max_value,
    })
}

#[inline]
fn luma_band_to_chroma_band(luma_row: usize, luma_rows: usize) -> (usize, usize) {
    let chroma_row = luma_row / 2;
    let chroma_end = (luma_row.saturating_add(luma_rows).saturating_add(1)) / 2;
    (chroma_row, chroma_end.saturating_sub(chroma_row))
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) fn probe_output_surface(
    output_surface: &[u8],
    coded_width: u16,
    coded_height: u16,
    visible_x: u16,
    visible_y: u16,
    visible_width: u16,
    visible_height: u16,
    output_pitch: usize,
) -> MediaSurfaceProbe {
    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if coded_width == 0
        || coded_height == 0
        || visible_width == 0
        || visible_height == 0
        || output_pitch < coded_width
    {
        return MediaSurfaceProbe::empty();
    }
    let visible_bottom = visible_y.saturating_add(visible_height).min(coded_height);
    if visible_x.saturating_add(visible_width) > coded_width || visible_bottom <= visible_y {
        return MediaSurfaceProbe::empty();
    }
    let bottom_luma_rows = coded_height.min(16);
    let bottom_luma_row = coded_height.saturating_sub(bottom_luma_rows);
    let prev_luma_rows = bottom_luma_row.min(16);
    let prev_luma_row = bottom_luma_row.saturating_sub(prev_luma_rows);
    let visible_last_row = visible_bottom.saturating_sub(1);
    let visible_tail8_row = visible_bottom.saturating_sub(8).max(visible_y);
    let center_luma_rows = visible_height.min(16);
    let center_luma_row = visible_y
        .saturating_add(visible_height / 2)
        .saturating_sub(center_luma_rows / 2)
        .min(coded_height.saturating_sub(center_luma_rows));
    let chroma_y_offset = (coded_height + MEDIA_YTILE_H - 1) & !(MEDIA_YTILE_H - 1);
    let luma_storage_pad_first_row = if chroma_y_offset > coded_height {
        probe_tiled_rect(output_surface, output_pitch, 0, coded_height, coded_width, 1, 0)
    } else {
        None
    };
    let luma_storage_pad_last_row = if chroma_y_offset > coded_height {
        probe_tiled_rect(
            output_surface,
            output_pitch,
            0,
            chroma_y_offset.saturating_sub(1),
            coded_width,
            1,
            0,
        )
    } else {
        None
    };
    let chroma_plane_rows = coded_height.div_ceil(2);
    let chroma_plane_stride_rows = (chroma_plane_rows + MEDIA_YTILE_H - 1) & !(MEDIA_YTILE_H - 1);
    let cr_y_offset = chroma_y_offset.saturating_add(chroma_plane_stride_rows);
    let chroma_width = coded_width.div_ceil(2);
    let center_chroma_x = visible_x / 2;
    let center_chroma_width = visible_width
        .div_ceil(2)
        .min(chroma_width.saturating_sub(center_chroma_x));
    let center_chroma_lo_width = center_chroma_width.div_ceil(2);
    let center_chroma_hi_width = center_chroma_width
        .saturating_sub(center_chroma_lo_width)
        .max(center_chroma_lo_width);
    let center_chroma_hi_x = center_chroma_x
        .saturating_add(center_chroma_width)
        .saturating_sub(center_chroma_hi_width);
    let (center_chroma_row, center_chroma_rows) =
        luma_band_to_chroma_band(center_luma_row, center_luma_rows);
    let (prev_chroma_row, prev_chroma_rows) =
        luma_band_to_chroma_band(prev_luma_row, prev_luma_rows);
    let (bottom_chroma_row, bottom_chroma_rows) =
        luma_band_to_chroma_band(bottom_luma_row, bottom_luma_rows);
    let luma_visible_last_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_last_row,
        visible_width,
        1,
        0,
    );
    let luma_visible_tail8_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_tail8_row,
        visible_width,
        1,
        0,
    );
    let luma_center_band = probe_tiled_rect(
        output_surface,
        output_pitch,
        visible_x,
        center_luma_row,
        visible_width,
        center_luma_rows,
        0,
    );
    let luma_prev_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        prev_luma_row,
        coded_width,
        prev_luma_rows,
        0,
    );
    let luma_bottom_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        bottom_luma_row,
        coded_width,
        bottom_luma_rows,
        0,
    );
    let cb_center_band = probe_tiled_rect(
        output_surface,
        output_pitch,
        center_chroma_x,
        chroma_y_offset.saturating_add(center_chroma_row),
        center_chroma_lo_width,
        center_chroma_rows,
        0x80,
    );
    let cb_center_hi_band = probe_tiled_rect(
        output_surface,
        output_pitch,
        center_chroma_hi_x,
        chroma_y_offset.saturating_add(center_chroma_row),
        center_chroma_hi_width,
        center_chroma_rows,
        0x80,
    );
    let cb_prev_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        chroma_y_offset.saturating_add(prev_chroma_row),
        chroma_width,
        prev_chroma_rows,
        0x80,
    );
    let cb_bottom_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        chroma_y_offset.saturating_add(bottom_chroma_row),
        chroma_width,
        bottom_chroma_rows,
        0x80,
    );
    let cr_center_band = probe_tiled_rect(
        output_surface,
        output_pitch,
        center_chroma_x,
        cr_y_offset.saturating_add(center_chroma_row),
        center_chroma_lo_width,
        center_chroma_rows,
        0x80,
    );
    let cr_prev_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        cr_y_offset.saturating_add(prev_chroma_row),
        chroma_width,
        prev_chroma_rows,
        0x80,
    );
    let cr_bottom_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        cr_y_offset.saturating_add(bottom_chroma_row),
        chroma_width,
        bottom_chroma_rows,
        0x80,
    );
    let valid = luma_visible_last_row.is_some()
        && luma_visible_tail8_row.is_some()
        && luma_center_band.is_some()
        && luma_prev_mb_row.is_some()
        && luma_bottom_mb_row.is_some()
        && cb_center_band.is_some()
        && cb_center_hi_band.is_some()
        && cb_prev_mb_row.is_some()
        && cb_bottom_mb_row.is_some()
        && cr_center_band.is_some()
        && cr_prev_mb_row.is_some()
        && cr_bottom_mb_row.is_some();
    MediaSurfaceProbe {
        valid,
        luma_visible_last_row: luma_visible_last_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_visible_tail8_row: luma_visible_tail8_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_storage_pad_first_row: luma_storage_pad_first_row
            .unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_storage_pad_last_row: luma_storage_pad_last_row
            .unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_center_band: luma_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_prev_mb_row: luma_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_bottom_mb_row: luma_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_center_band: cb_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_center_hi_band: cb_center_hi_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_prev_mb_row: cb_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_bottom_mb_row: cb_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_center_band: cr_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_prev_mb_row: cr_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_bottom_mb_row: cr_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
    }
}

pub(super) fn probe_ytile_nv12_output_surface(
    output_surface: &[u8],
    coded_width: u16,
    coded_height: u16,
    visible_x: u16,
    visible_y: u16,
    visible_width: u16,
    visible_height: u16,
    output_pitch: usize,
) -> MediaSurfaceProbe {
    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if coded_width == 0
        || coded_height == 0
        || visible_width == 0
        || visible_height == 0
        || output_pitch < coded_width
        || !output_pitch.is_multiple_of(MEDIA_YTILE_W)
    {
        return MediaSurfaceProbe::empty();
    }
    let visible_bottom = visible_y.saturating_add(visible_height).min(coded_height);
    if visible_x.saturating_add(visible_width) > coded_width || visible_bottom <= visible_y {
        return MediaSurfaceProbe::empty();
    }
    let chroma_y_offset = coded_height.next_multiple_of(MEDIA_YTILE_H);
    let total_height = chroma_y_offset.saturating_add(coded_height.div_ceil(2));
    let needed = total_height
        .div_ceil(MEDIA_YTILE_H)
        .saturating_mul(output_pitch / MEDIA_YTILE_W)
        .saturating_mul(4096);
    if output_surface.len() < needed {
        return MediaSurfaceProbe::empty();
    }

    let bottom_luma_rows = coded_height.min(16);
    let bottom_luma_row = coded_height.saturating_sub(bottom_luma_rows);
    let prev_luma_rows = bottom_luma_row.min(16);
    let prev_luma_row = bottom_luma_row.saturating_sub(prev_luma_rows);
    let visible_last_row = visible_bottom.saturating_sub(1);
    let visible_tail8_row = visible_bottom.saturating_sub(8).max(visible_y);
    let center_luma_rows = visible_height.min(16);
    let center_luma_row = visible_y
        .saturating_add(visible_height / 2)
        .saturating_sub(center_luma_rows / 2)
        .min(coded_height.saturating_sub(center_luma_rows));
    let chroma_width_pairs = coded_width.div_ceil(2);
    let center_chroma_x = visible_x / 2;
    let center_chroma_width = visible_width
        .div_ceil(2)
        .min(chroma_width_pairs.saturating_sub(center_chroma_x));
    let center_chroma_lo_width = center_chroma_width.div_ceil(2);
    let center_chroma_hi_width = center_chroma_width
        .saturating_sub(center_chroma_lo_width)
        .max(center_chroma_lo_width);
    let center_chroma_hi_x = center_chroma_x
        .saturating_add(center_chroma_width)
        .saturating_sub(center_chroma_hi_width);
    let (center_chroma_row, center_chroma_rows) =
        luma_band_to_chroma_band(center_luma_row, center_luma_rows);
    let (prev_chroma_row, prev_chroma_rows) =
        luma_band_to_chroma_band(prev_luma_row, prev_luma_rows);
    let (bottom_chroma_row, bottom_chroma_rows) =
        luma_band_to_chroma_band(bottom_luma_row, bottom_luma_rows);

    let luma_visible_last_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_last_row,
        visible_width,
        1,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_visible_tail8_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_tail8_row,
        visible_width,
        1,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_center_band = probe_tiled_rect(
        output_surface,
        output_pitch,
        visible_x,
        center_luma_row,
        visible_width,
        center_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_prev_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        prev_luma_row,
        coded_width,
        prev_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_bottom_mb_row = probe_tiled_rect(
        output_surface,
        output_pitch,
        0,
        bottom_luma_row,
        coded_width,
        bottom_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let cb_center_band = probe_ytile_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        center_chroma_x,
        center_chroma_row,
        center_chroma_lo_width,
        center_chroma_rows,
        0,
    );
    let cb_center_hi_band = probe_ytile_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        center_chroma_hi_x,
        center_chroma_row,
        center_chroma_hi_width,
        center_chroma_rows,
        0,
    );
    let cb_prev_mb_row = probe_ytile_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        0,
        prev_chroma_row,
        chroma_width_pairs,
        prev_chroma_rows,
        0,
    );
    let cb_bottom_mb_row = probe_ytile_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        0,
        bottom_chroma_row,
        chroma_width_pairs,
        bottom_chroma_rows,
        0,
    );
    let cr_center_band = probe_ytile_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        center_chroma_x,
        center_chroma_row,
        center_chroma_lo_width,
        center_chroma_rows,
        1,
    );
    let cr_prev_mb_row = probe_ytile_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        0,
        prev_chroma_row,
        chroma_width_pairs,
        prev_chroma_rows,
        1,
    );
    let cr_bottom_mb_row = probe_ytile_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        0,
        bottom_chroma_row,
        chroma_width_pairs,
        bottom_chroma_rows,
        1,
    );
    let valid = luma_visible_last_row.is_some()
        && luma_visible_tail8_row.is_some()
        && luma_center_band.is_some()
        && luma_prev_mb_row.is_some()
        && luma_bottom_mb_row.is_some()
        && cb_center_band.is_some()
        && cb_center_hi_band.is_some()
        && cb_prev_mb_row.is_some()
        && cb_bottom_mb_row.is_some()
        && cr_center_band.is_some()
        && cr_prev_mb_row.is_some()
        && cr_bottom_mb_row.is_some();
    MediaSurfaceProbe {
        valid,
        luma_visible_last_row: luma_visible_last_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_visible_tail8_row: luma_visible_tail8_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_storage_pad_first_row: MediaSurfaceProbeBand::empty(),
        luma_storage_pad_last_row: MediaSurfaceProbeBand::empty(),
        luma_center_band: luma_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_prev_mb_row: luma_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_bottom_mb_row: luma_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_center_band: cb_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_center_hi_band: cb_center_hi_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_prev_mb_row: cb_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_bottom_mb_row: cb_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_center_band: cr_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_prev_mb_row: cr_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_bottom_mb_row: cr_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
    }
}

pub(super) fn probe_linear_nv12_output_surface(
    output_surface: &[u8],
    coded_width: u16,
    coded_height: u16,
    visible_x: u16,
    visible_y: u16,
    visible_width: u16,
    visible_height: u16,
    output_pitch: usize,
) -> MediaSurfaceProbe {
    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if coded_width == 0
        || coded_height == 0
        || visible_width == 0
        || visible_height == 0
        || output_pitch < coded_width
    {
        return MediaSurfaceProbe::empty();
    }
    let visible_bottom = visible_y.saturating_add(visible_height).min(coded_height);
    if visible_x.saturating_add(visible_width) > coded_width || visible_bottom <= visible_y {
        return MediaSurfaceProbe::empty();
    }
    let uv_offset = output_pitch.saturating_mul(coded_height);
    let needed = uv_offset.saturating_add(output_pitch.saturating_mul(coded_height.div_ceil(2)));
    if output_surface.len() < needed {
        return MediaSurfaceProbe::empty();
    }

    let bottom_luma_rows = coded_height.min(16);
    let bottom_luma_row = coded_height.saturating_sub(bottom_luma_rows);
    let prev_luma_rows = bottom_luma_row.min(16);
    let prev_luma_row = bottom_luma_row.saturating_sub(prev_luma_rows);
    let visible_last_row = visible_bottom.saturating_sub(1);
    let visible_tail8_row = visible_bottom.saturating_sub(8).max(visible_y);
    let center_luma_rows = visible_height.min(16);
    let center_luma_row = visible_y
        .saturating_add(visible_height / 2)
        .saturating_sub(center_luma_rows / 2)
        .min(coded_height.saturating_sub(center_luma_rows));
    let chroma_width_pairs = coded_width.div_ceil(2);
    let center_chroma_x = visible_x / 2;
    let center_chroma_width = visible_width
        .div_ceil(2)
        .min(chroma_width_pairs.saturating_sub(center_chroma_x));
    let center_chroma_lo_width = center_chroma_width.div_ceil(2);
    let center_chroma_hi_width = center_chroma_width
        .saturating_sub(center_chroma_lo_width)
        .max(center_chroma_lo_width);
    let center_chroma_hi_x = center_chroma_x
        .saturating_add(center_chroma_width)
        .saturating_sub(center_chroma_hi_width);
    let (center_chroma_row, center_chroma_rows) =
        luma_band_to_chroma_band(center_luma_row, center_luma_rows);
    let (prev_chroma_row, prev_chroma_rows) =
        luma_band_to_chroma_band(prev_luma_row, prev_luma_rows);
    let (bottom_chroma_row, bottom_chroma_rows) =
        luma_band_to_chroma_band(bottom_luma_row, bottom_luma_rows);

    let luma_visible_last_row = probe_linear_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_last_row,
        visible_width,
        1,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_visible_tail8_row = probe_linear_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_tail8_row,
        visible_width,
        1,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_center_band = probe_linear_rect(
        output_surface,
        output_pitch,
        visible_x,
        center_luma_row,
        visible_width,
        center_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_prev_mb_row = probe_linear_rect(
        output_surface,
        output_pitch,
        0,
        prev_luma_row,
        coded_width,
        prev_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_bottom_mb_row = probe_linear_rect(
        output_surface,
        output_pitch,
        0,
        bottom_luma_row,
        coded_width,
        bottom_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let cb_center_band = probe_linear_nv12_chroma_rect(
        output_surface,
        output_pitch,
        uv_offset,
        center_chroma_x,
        center_chroma_row,
        center_chroma_lo_width,
        center_chroma_rows,
        0,
    );
    let cb_center_hi_band = probe_linear_nv12_chroma_rect(
        output_surface,
        output_pitch,
        uv_offset,
        center_chroma_hi_x,
        center_chroma_row,
        center_chroma_hi_width,
        center_chroma_rows,
        0,
    );
    let cb_prev_mb_row = probe_linear_nv12_chroma_rect(
        output_surface,
        output_pitch,
        uv_offset,
        0,
        prev_chroma_row,
        chroma_width_pairs,
        prev_chroma_rows,
        0,
    );
    let cb_bottom_mb_row = probe_linear_nv12_chroma_rect(
        output_surface,
        output_pitch,
        uv_offset,
        0,
        bottom_chroma_row,
        chroma_width_pairs,
        bottom_chroma_rows,
        0,
    );
    let cr_center_band = probe_linear_nv12_chroma_rect(
        output_surface,
        output_pitch,
        uv_offset,
        center_chroma_x,
        center_chroma_row,
        center_chroma_lo_width,
        center_chroma_rows,
        1,
    );
    let cr_prev_mb_row = probe_linear_nv12_chroma_rect(
        output_surface,
        output_pitch,
        uv_offset,
        0,
        prev_chroma_row,
        chroma_width_pairs,
        prev_chroma_rows,
        1,
    );
    let cr_bottom_mb_row = probe_linear_nv12_chroma_rect(
        output_surface,
        output_pitch,
        uv_offset,
        0,
        bottom_chroma_row,
        chroma_width_pairs,
        bottom_chroma_rows,
        1,
    );
    let valid = luma_visible_last_row.is_some()
        && luma_visible_tail8_row.is_some()
        && luma_center_band.is_some()
        && luma_prev_mb_row.is_some()
        && luma_bottom_mb_row.is_some()
        && cb_center_band.is_some()
        && cb_center_hi_band.is_some()
        && cb_prev_mb_row.is_some()
        && cb_bottom_mb_row.is_some()
        && cr_center_band.is_some()
        && cr_prev_mb_row.is_some()
        && cr_bottom_mb_row.is_some();
    MediaSurfaceProbe {
        valid,
        luma_visible_last_row: luma_visible_last_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_visible_tail8_row: luma_visible_tail8_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_storage_pad_first_row: MediaSurfaceProbeBand::empty(),
        luma_storage_pad_last_row: MediaSurfaceProbeBand::empty(),
        luma_center_band: luma_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_prev_mb_row: luma_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_bottom_mb_row: luma_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_center_band: cb_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_center_hi_band: cb_center_hi_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_prev_mb_row: cb_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_bottom_mb_row: cb_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_center_band: cr_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_prev_mb_row: cr_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_bottom_mb_row: cr_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
    }
}

pub(super) fn probe_tiled_nv12_output_surface(
    output_surface: &[u8],
    coded_width: u16,
    coded_height: u16,
    visible_x: u16,
    visible_y: u16,
    visible_width: u16,
    visible_height: u16,
    output_pitch: usize,
) -> MediaSurfaceProbe {
    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if coded_width == 0
        || coded_height == 0
        || visible_width == 0
        || visible_height == 0
        || output_pitch < coded_width
    {
        return MediaSurfaceProbe::empty();
    }
    let visible_bottom = visible_y.saturating_add(visible_height).min(coded_height);
    if visible_x.saturating_add(visible_width) > coded_width || visible_bottom <= visible_y {
        return MediaSurfaceProbe::empty();
    }
    let Some((chroma_y_offset, needed)) =
        media_tile64_nv12_surface_layout(coded_height, output_pitch)
    else {
        return MediaSurfaceProbe::empty();
    };
    if output_surface.len() < needed {
        return MediaSurfaceProbe::empty();
    }

    let bottom_luma_rows = coded_height.min(16);
    let bottom_luma_row = coded_height.saturating_sub(bottom_luma_rows);
    let prev_luma_rows = bottom_luma_row.min(16);
    let prev_luma_row = bottom_luma_row.saturating_sub(prev_luma_rows);
    let visible_last_row = visible_bottom.saturating_sub(1);
    let visible_tail8_row = visible_bottom.saturating_sub(8).max(visible_y);
    let center_luma_rows = visible_height.min(16);
    let center_luma_row = visible_y
        .saturating_add(visible_height / 2)
        .saturating_sub(center_luma_rows / 2)
        .min(coded_height.saturating_sub(center_luma_rows));
    let chroma_width_pairs = coded_width.div_ceil(2);
    let center_chroma_x = visible_x / 2;
    let center_chroma_width = visible_width
        .div_ceil(2)
        .min(chroma_width_pairs.saturating_sub(center_chroma_x));
    let center_chroma_lo_width = center_chroma_width.div_ceil(2);
    let center_chroma_hi_width = center_chroma_width
        .saturating_sub(center_chroma_lo_width)
        .max(center_chroma_lo_width);
    let center_chroma_hi_x = center_chroma_x
        .saturating_add(center_chroma_width)
        .saturating_sub(center_chroma_hi_width);
    let (center_chroma_row, center_chroma_rows) =
        luma_band_to_chroma_band(center_luma_row, center_luma_rows);
    let (prev_chroma_row, prev_chroma_rows) =
        luma_band_to_chroma_band(prev_luma_row, prev_luma_rows);
    let (bottom_chroma_row, bottom_chroma_rows) =
        luma_band_to_chroma_band(bottom_luma_row, bottom_luma_rows);

    let luma_visible_last_row = probe_tile64_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_last_row,
        visible_width,
        1,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_visible_tail8_row = probe_tile64_rect(
        output_surface,
        output_pitch,
        visible_x,
        visible_tail8_row,
        visible_width,
        1,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_center_band = probe_tile64_rect(
        output_surface,
        output_pitch,
        visible_x,
        center_luma_row,
        visible_width,
        center_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_prev_mb_row = probe_tile64_rect(
        output_surface,
        output_pitch,
        0,
        prev_luma_row,
        coded_width,
        prev_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let luma_bottom_mb_row = probe_tile64_rect(
        output_surface,
        output_pitch,
        0,
        bottom_luma_row,
        coded_width,
        bottom_luma_rows,
        MEDIA_NV12_BLACK_LUMA,
    );
    let cb_center_band = probe_tiled_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        center_chroma_x,
        center_chroma_row,
        center_chroma_lo_width,
        center_chroma_rows,
        0,
    );
    let cb_center_hi_band = probe_tiled_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        center_chroma_hi_x,
        center_chroma_row,
        center_chroma_hi_width,
        center_chroma_rows,
        0,
    );
    let cb_prev_mb_row = probe_tiled_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        0,
        prev_chroma_row,
        chroma_width_pairs,
        prev_chroma_rows,
        0,
    );
    let cb_bottom_mb_row = probe_tiled_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        0,
        bottom_chroma_row,
        chroma_width_pairs,
        bottom_chroma_rows,
        0,
    );
    let cr_center_band = probe_tiled_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        center_chroma_x,
        center_chroma_row,
        center_chroma_lo_width,
        center_chroma_rows,
        1,
    );
    let cr_prev_mb_row = probe_tiled_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        0,
        prev_chroma_row,
        chroma_width_pairs,
        prev_chroma_rows,
        1,
    );
    let cr_bottom_mb_row = probe_tiled_nv12_chroma_rect(
        output_surface,
        output_pitch,
        chroma_y_offset,
        0,
        bottom_chroma_row,
        chroma_width_pairs,
        bottom_chroma_rows,
        1,
    );
    let valid = luma_visible_last_row.is_some()
        && luma_visible_tail8_row.is_some()
        && luma_center_band.is_some()
        && luma_prev_mb_row.is_some()
        && luma_bottom_mb_row.is_some()
        && cb_center_band.is_some()
        && cb_center_hi_band.is_some()
        && cb_prev_mb_row.is_some()
        && cb_bottom_mb_row.is_some()
        && cr_center_band.is_some()
        && cr_prev_mb_row.is_some()
        && cr_bottom_mb_row.is_some();
    MediaSurfaceProbe {
        valid,
        luma_visible_last_row: luma_visible_last_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_visible_tail8_row: luma_visible_tail8_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_storage_pad_first_row: MediaSurfaceProbeBand::empty(),
        luma_storage_pad_last_row: MediaSurfaceProbeBand::empty(),
        luma_center_band: luma_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_prev_mb_row: luma_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        luma_bottom_mb_row: luma_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_center_band: cb_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_center_hi_band: cb_center_hi_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_prev_mb_row: cb_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cb_bottom_mb_row: cb_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_center_band: cr_center_band.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_prev_mb_row: cr_prev_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
        cr_bottom_mb_row: cr_bottom_mb_row.unwrap_or_else(MediaSurfaceProbeBand::empty),
    }
}

pub(super) fn log_output_surface_probe(
    engine_name: &'static str,
    sample_idx: u32,
    submit_completed: bool,
    probe: MediaSurfaceProbe,
) {
    log_output_surface_probe_layout(
        engine_name,
        sample_idx,
        submit_completed,
        "tile64-nv12",
        probe,
    );
}

pub(super) fn log_output_surface_probe_layout(
    engine_name: &'static str,
    sample_idx: u32,
    submit_completed: bool,
    layout: &'static str,
    probe: MediaSurfaceProbe,
) {
    if !probe.valid {
        crate::log!(
            "intel/media2: output-probe phase=pre-present layout={} engine={} sample={} submit_completed={} valid=false\n",
            layout,
            engine_name,
            sample_idx,
            submit_completed
        );
        return;
    }
    crate::log!(
        "intel/media2: output-probe phase=pre-present layout={} engine={} sample={} submit_completed={} y_last(sig=0x{:08X} active={}/{} range={}..{}) y_tail8(sig=0x{:08X} active={}/{} range={}..{}) y_pad_first(sig=0x{:08X} active={}/{} range={}..{}) y_pad_last(sig=0x{:08X} active={}/{} range={}..{}) y_center(sig=0x{:08X} active={}/{} range={}..{}) y_prev_mb(sig=0x{:08X} active={}/{} range={}..{}) y_bottom_mb(sig=0x{:08X} active={}/{} range={}..{}) cb_center(sig=0x{:08X} active={}/{} range={}..{}) cb_center_hi(sig=0x{:08X} active={}/{} range={}..{}) cb_prev_mb(sig=0x{:08X} active={}/{} range={}..{}) cb_bottom_mb(sig=0x{:08X} active={}/{} range={}..{}) cr_center(sig=0x{:08X} active={}/{} range={}..{}) cr_prev_mb(sig=0x{:08X} active={}/{} range={}..{}) cr_bottom_mb(sig=0x{:08X} active={}/{} range={}..{})\n",
        layout,
        engine_name,
        sample_idx,
        submit_completed,
        probe.luma_visible_last_row.signature,
        probe.luma_visible_last_row.active_samples,
        probe.luma_visible_last_row.sample_count,
        probe.luma_visible_last_row.min_value,
        probe.luma_visible_last_row.max_value,
        probe.luma_visible_tail8_row.signature,
        probe.luma_visible_tail8_row.active_samples,
        probe.luma_visible_tail8_row.sample_count,
        probe.luma_visible_tail8_row.min_value,
        probe.luma_visible_tail8_row.max_value,
        probe.luma_storage_pad_first_row.signature,
        probe.luma_storage_pad_first_row.active_samples,
        probe.luma_storage_pad_first_row.sample_count,
        probe.luma_storage_pad_first_row.min_value,
        probe.luma_storage_pad_first_row.max_value,
        probe.luma_storage_pad_last_row.signature,
        probe.luma_storage_pad_last_row.active_samples,
        probe.luma_storage_pad_last_row.sample_count,
        probe.luma_storage_pad_last_row.min_value,
        probe.luma_storage_pad_last_row.max_value,
        probe.luma_center_band.signature,
        probe.luma_center_band.active_samples,
        probe.luma_center_band.sample_count,
        probe.luma_center_band.min_value,
        probe.luma_center_band.max_value,
        probe.luma_prev_mb_row.signature,
        probe.luma_prev_mb_row.active_samples,
        probe.luma_prev_mb_row.sample_count,
        probe.luma_prev_mb_row.min_value,
        probe.luma_prev_mb_row.max_value,
        probe.luma_bottom_mb_row.signature,
        probe.luma_bottom_mb_row.active_samples,
        probe.luma_bottom_mb_row.sample_count,
        probe.luma_bottom_mb_row.min_value,
        probe.luma_bottom_mb_row.max_value,
        probe.cb_center_band.signature,
        probe.cb_center_band.active_samples,
        probe.cb_center_band.sample_count,
        probe.cb_center_band.min_value,
        probe.cb_center_band.max_value,
        probe.cb_center_hi_band.signature,
        probe.cb_center_hi_band.active_samples,
        probe.cb_center_hi_band.sample_count,
        probe.cb_center_hi_band.min_value,
        probe.cb_center_hi_band.max_value,
        probe.cb_prev_mb_row.signature,
        probe.cb_prev_mb_row.active_samples,
        probe.cb_prev_mb_row.sample_count,
        probe.cb_prev_mb_row.min_value,
        probe.cb_prev_mb_row.max_value,
        probe.cb_bottom_mb_row.signature,
        probe.cb_bottom_mb_row.active_samples,
        probe.cb_bottom_mb_row.sample_count,
        probe.cb_bottom_mb_row.min_value,
        probe.cb_bottom_mb_row.max_value,
        probe.cr_center_band.signature,
        probe.cr_center_band.active_samples,
        probe.cr_center_band.sample_count,
        probe.cr_center_band.min_value,
        probe.cr_center_band.max_value,
        probe.cr_prev_mb_row.signature,
        probe.cr_prev_mb_row.active_samples,
        probe.cr_prev_mb_row.sample_count,
        probe.cr_prev_mb_row.min_value,
        probe.cr_prev_mb_row.max_value,
        probe.cr_bottom_mb_row.signature,
        probe.cr_bottom_mb_row.active_samples,
        probe.cr_bottom_mb_row.sample_count,
        probe.cr_bottom_mb_row.min_value,
        probe.cr_bottom_mb_row.max_value
    );
}

pub(super) fn output_surface_has_decoded_detail(probe: &MediaSurfaceProbe) -> bool {
    probe.valid
        && (probe.luma_visible_last_row.has_range()
            || probe.luma_visible_tail8_row.has_range()
            || probe.luma_storage_pad_first_row.has_range()
            || probe.luma_storage_pad_last_row.has_range()
            || probe.luma_center_band.has_range()
            || probe.luma_prev_mb_row.has_range()
            || probe.luma_bottom_mb_row.has_range()
            || probe.cb_center_band.has_range()
            || probe.cb_center_hi_band.has_range()
            || probe.cb_prev_mb_row.has_range()
            || probe.cb_bottom_mb_row.has_range()
            || probe.cr_center_band.has_range()
            || probe.cr_prev_mb_row.has_range()
            || probe.cr_bottom_mb_row.has_range())
}

#[inline]
pub(super) fn align_up_u32(value: u32, align: u32) -> u32 {
    if align == 0 {
        value
    } else {
        value.saturating_add(align.saturating_sub(1)) & !align.saturating_sub(1)
    }
}

#[inline]
fn masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

#[inline]
pub(super) fn masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

#[inline]
fn mi_lri_num_regs(num_regs: u32) -> u32 {
    num_regs.saturating_mul(2).saturating_sub(1)
}

#[inline]
fn mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | mi_lri_num_regs(num_regs)
}

fn push_mi_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}

fn install_media_ppgtt(ranges: &[crate::intel::ppgtt::PpgttRange]) -> Option<u64> {
    let ppgtt = crate::intel::ppgtt::build_sparse_ppgtt_for_ranges(ranges)?;
    let root = ppgtt.pml4_phys();
    *MEDIA_PPGTT.lock() = Some(ppgtt);
    Some(root)
}

/// Add an externally owned surface to the stable media address space. Callers
/// must keep the allocation and its producer lease alive until the submitted
/// job retires. The hardware activation path will additionally perform its
/// VDBOX TLB/context synchronization immediately before first use.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) fn map_media_ppgtt_range(gpu: u64, phys: u64, bytes: usize) -> bool {
    if gpu == 0
        || phys == 0
        || bytes == 0
        || !gpu.is_multiple_of(crate::intel::WARM_ALIGN as u64)
        || !phys.is_multiple_of(crate::intel::WARM_ALIGN as u64)
        || !bytes.is_multiple_of(crate::intel::WARM_ALIGN)
    {
        return false;
    }
    MEDIA_PPGTT
        .lock()
        .as_mut()
        .and_then(|ppgtt| ppgtt.map_range(crate::intel::ppgtt::PpgttRange { gpu, phys, bytes }))
        .is_some()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(super) fn unmap_media_ppgtt_range(gpu: u64, bytes: usize) -> bool {
    if gpu == 0
        || bytes == 0
        || !gpu.is_multiple_of(crate::intel::WARM_ALIGN as u64)
        || !bytes.is_multiple_of(crate::intel::WARM_ALIGN)
    {
        return false;
    }
    MEDIA_PPGTT
        .lock()
        .as_mut()
        .and_then(|ppgtt| ppgtt.unmap_range(gpu, bytes))
        .is_some()
}

pub(super) fn build_ring_batch_start_words(
    ring_virt: *mut u8,
    ring_bytes: usize,
    ring_offset: usize,
    result_gpu_addr: u64,
    prelaunch_marker: u32,
    batch_gpu_addr: u64,
    _mode: MediaJobMode,
) -> Option<usize> {
    let ring_dwords = 10;
    let ring_job_bytes = ring_dwords * core::mem::size_of::<u32>();
    if ring_virt.is_null() || ring_offset.checked_add(ring_job_bytes)? > ring_bytes {
        return None;
    }
    let base = unsafe { ring_virt.add(ring_offset) };
    let dwords = unsafe { core::slice::from_raw_parts_mut(base as *mut u32, ring_dwords) };
    dwords.fill(MI_NOOP);
    dwords[0] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT;
    dwords[1] = (result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT) as u32;
    dwords[2] = ((result_gpu_addr + MEDIA_RESULT_KICKOFF_SLOT) >> 32) as u32;
    dwords[3] = prelaunch_marker;
    dwords[4] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_PPGTT;
    dwords[5] = batch_gpu_addr as u32;
    dwords[6] = (batch_gpu_addr >> 32) as u32;
    dwords[7] = MI_ARB_CHECK;
    Some(ring_offset + ring_job_bytes)
}

/// Append one persistent-context ring entry and wrap on a power-of-two ring
/// boundary. The 64-byte stride divides every supported media ring size and
/// leaves the unused tail as MI_NOOPs.
pub(super) fn append_ring_batch_start_words(
    ring_virt: *mut u8,
    ring_bytes: usize,
    ring_offset: usize,
    result_gpu_addr: u64,
    prelaunch_marker: u32,
    batch_gpu_addr: u64,
    mode: MediaJobMode,
) -> Option<usize> {
    const ENTRY_BYTES: usize = 64;
    if ring_bytes < ENTRY_BYTES
        || !ring_bytes.is_power_of_two()
        || !ring_bytes.is_multiple_of(ENTRY_BYTES)
        || ring_offset >= ring_bytes
        || !ring_offset.is_multiple_of(ENTRY_BYTES)
    {
        return None;
    }
    let written_tail = build_ring_batch_start_words(
        ring_virt,
        ring_bytes,
        ring_offset,
        result_gpu_addr,
        prelaunch_marker,
        batch_gpu_addr,
        mode,
    )?;
    let padding_bytes = ring_offset + ENTRY_BYTES - written_tail;
    unsafe {
        let padding = core::slice::from_raw_parts_mut(
            ring_virt.add(written_tail).cast::<u32>(),
            padding_bytes / core::mem::size_of::<u32>(),
        );
        padding.fill(MI_NOOP);
    }
    Some((ring_offset + ENTRY_BYTES) & (ring_bytes - 1))
}

/// Build a first-level trampoline for a codec secondary. The ring enters this
/// primary as a normal first-level batch; MI_BATCH_BUFFER_START then selects
/// the codec batch's second-level storage. MI_BATCH_BUFFER_END in the codec
/// batch returns here, so the marker proves both codec completion and the
/// architectural batch-level return before the primary itself ends.
pub(super) fn build_primary_second_level_return_words(
    primary_virt: *mut u8,
    primary_bytes: usize,
    result_gpu_addr: u64,
    second_level_gpu_addr: u64,
    return_marker: u32,
    mode: MediaJobMode,
) -> Option<usize> {
    if mode.batch_level != MediaBatchLevel::SecondLevelReturn
        || primary_virt.is_null()
        || primary_bytes < 40
    {
        return None;
    }
    let dwords = unsafe { core::slice::from_raw_parts_mut(primary_virt.cast::<u32>(), 10) };
    dwords.fill(MI_NOOP);
    dwords[0] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_PPGTT | MI_BATCH_SECOND_LEVEL;
    dwords[1] = second_level_gpu_addr as u32;
    dwords[2] = (second_level_gpu_addr >> 32) as u32;
    dwords[3] = MI_ARB_CHECK;
    dwords[4] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT;
    dwords[5] = (result_gpu_addr + MEDIA_RESULT_COMPLETE_SLOT) as u32;
    dwords[6] = ((result_gpu_addr + MEDIA_RESULT_COMPLETE_SLOT) >> 32) as u32;
    dwords[7] = return_marker;
    dwords[8] = MI_BATCH_BUFFER_END;
    Some(40)
}

pub(super) fn ring_ctl_value_for_size(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | 1)
}

fn build_execlist_context_descriptor_for_gpu_addr(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    (
        base | CTX_DESC_VALID
            | CTX_DESC_PPGTT_ENABLE
            | CTX_DESC_PRIVILEGE
            | CTX_DESC_PRIORITY_NORMAL
            | (INTEL_LEGACY_64B_CONTEXT << CTX_DESC_ADDRESSING_MODE_SHIFT),
        (context_gpu_addr >> 32) as u32,
    )
}

fn media_sw_context_id_for_submit(context_gpu_addr: u64) -> u32 {
    let sw_context_id = ((context_gpu_addr >> 12) as u32) & 0x7FF;
    if sw_context_id == 0 { 1 } else { sw_context_id }
}

pub(super) fn build_media_execlist_context_descriptor(
    context_gpu_addr: u64,
    _engine: MediaEngineDescriptor,
    _sw_counter: u32,
    force_restore: bool,
) -> (u32, u32) {
    let (mut lo, _) = build_execlist_context_descriptor_for_gpu_addr(context_gpu_addr);
    if force_restore {
        lo |= CTX_DESC_FORCE_RESTORE;
    }
    let hi =
        ((context_gpu_addr >> 32) as u32) | (media_sw_context_id_for_submit(context_gpu_addr) << 7);
    (lo, hi)
}

/// GuC receives the stable LRCA descriptor. Unlike a direct execlist port
/// descriptor, its high dword must not carry TRUEOS's software context id and
/// the GuC registration ABI owns scheduling/restore policy.
pub(super) fn build_media_guc_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    (
        base | CTX_DESC_VALID
            | CTX_DESC_PRIVILEGE
            | (INTEL_LEGACY_64B_CONTEXT << CTX_DESC_ADDRESSING_MODE_SHIFT),
        (context_gpu_addr >> 32) as u32,
    )
}

pub(super) fn media_ctx_control_value(inhibit_restore: bool) -> u32 {
    let mut ctl =
        masked_bits_update(CTX_CTRL_INHIBIT_SYN_CTX_SWITCH, CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT);
    if inhibit_restore {
        ctl |= CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT;
    }
    ctl
}

pub(super) fn init_gen12_video_context_image(
    context_virt: *mut u8,
    context_len: usize,
    ring_base: usize,
    _ring_head: u32,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
    _hws_pga: u32,
    pml4_phys: u64,
    inhibit_restore: bool,
) -> bool {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
    const CTX_RING_TAIL_DW: usize = 7;
    const CTX_RING_START_DW: usize = 9;
    const CTX_RING_CTL_DW: usize = 11;
    if context_virt.is_null() {
        return false;
    }
    let total_dwords = context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS {
        return false;
    }
    let dwords = unsafe { core::slice::from_raw_parts_mut(context_virt as *mut u32, total_dwords) };
    dwords.fill(0);
    let state = &mut dwords[LRC_STATE_OFFSET_DWORDS..];
    if state.len() < 192 {
        return false;
    }
    let ring_base = ring_base as u32;
    let mut idx = 0usize;
    state[idx] = MI_NOOP;
    idx += 1;
    state[idx] = mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x244;
    state[idx + 1] = media_ctx_control_value(inhibit_restore);
    state[idx + 2] = ring_base + 0x34;
    state[idx + 3] = 0;
    state[idx + 4] = ring_base + 0x30;
    state[idx + 5] = ring_tail;
    state[idx + 6] = ring_base + 0x38;
    state[idx + 7] = ring_start;
    state[idx + 8] = ring_base + 0x3C;
    state[idx + 9] = ring_ctl;
    state[idx + 10] = ring_base + 0x168;
    state[idx + 11] = 0;
    state[idx + 12] = ring_base + 0x140;
    state[idx + 13] = 0;
    state[idx + 14] = ring_base + 0x110;
    state[idx + 15] = 0;
    state[idx + 16] = ring_base + 0x1C0;
    state[idx + 17] = 0;
    state[idx + 18] = ring_base + 0x1C4;
    state[idx + 19] = 0;
    state[idx + 20] = ring_base + 0x1C8;
    state[idx + 21] = 0;
    state[idx + 22] = ring_base + 0x180;
    state[idx + 23] = 0;
    state[idx + 24] = ring_base + 0x2B4;
    state[idx + 25] = 0;
    state[idx + 26] = ring_base + 0x5A8;
    state[idx + 27] = 0;
    state[idx + 28] = ring_base + 0x5AC;
    state[idx + 29] = 0;
    idx += 30;
    push_mi_nops(state, &mut idx, 5);
    state[idx] = mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    for (offset, value) in [
        (0x3A8, 0),
        (0x28C, 0),
        (0x288, 0),
        (0x284, 0),
        (0x280, 0),
        (0x27C, 0),
        (0x278, 0),
        (0x274, (pml4_phys >> 32) as u32),
        (0x270, pml4_phys as u32),
    ] {
        state[idx] = ring_base + offset;
        state[idx + 1] = value;
        idx += 2;
    }
    state[idx] = mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x1B0;
    state[idx + 1] = 0;
    state[idx + 2] = ring_base + 0x5A8;
    state[idx + 3] = 0;
    state[idx + 4] = ring_base + 0x5AC;
    state[idx + 5] = 0;
    idx += 6;
    push_mi_nops(state, &mut idx, 6);
    state[idx] = mi_lri_cmd(1, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0xC8;
    state[idx + 1] = 0x7FFF_FFFF;
    idx += 2;
    push_mi_nops(state, &mut idx, 13);
    state[idx] = mi_lri_cmd(4, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = ring_base + 0x28;
    state[idx + 1] = 0;
    state[idx + 2] = ring_base + 0x9C;
    state[idx + 3] = masked_bit_disable(STOP_RING);
    state[idx + 4] = ring_base + 0x68;
    state[idx + 5] = 0;
    state[idx + 6] = ring_base + 0x84;
    state[idx + 7] = 0;
    idx += 8;
    push_mi_nops(state, &mut idx, 8);
    state[CTX_RING_TAIL_DW] = ring_tail;
    state[CTX_RING_START_DW] = ring_start;
    state[CTX_RING_CTL_DW] = ring_ctl;
    state[idx] = MI_BATCH_BUFFER_END | 1;
    true
}

/// Publish only TAIL in a previously registered video HWLRCA. GuC owns HEAD
/// and the remainder of the context image after first registration.
pub(super) fn write_gen12_video_context_ring_tail(
    context_virt: *mut u8,
    context_len: usize,
    ring_tail: u32,
) -> bool {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
    const LRC_CONTEXT_CONTROL_VALUE_DW: usize = 3;
    const LRC_RING_TAIL_VALUE_DW: usize = 7;
    let total_dwords = context_len / core::mem::size_of::<u32>();
    let first = LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW;
    let last = LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW;
    if context_virt.is_null() || total_dwords <= last {
        return false;
    }
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(context_virt.cast::<u32>(), total_dwords) };
    let context_control = dwords[first];
    dwords[last] = ring_tail;
    dwords[first] = context_control;
    unsafe {
        crate::intel::dma_flush(
            context_virt.add(first * core::mem::size_of::<u32>()),
            (last - first + 1) * core::mem::size_of::<u32>(),
        );
    }
    true
}

/// Read the ring HEAD last saved by GuC into a video HWLRCA.
pub(super) fn read_gen12_video_context_ring_head(
    context_virt: *mut u8,
    context_len: usize,
) -> Option<u32> {
    const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
    const LRC_RING_HEAD_VALUE_DW: usize = 5;
    let index = LRC_STATE_OFFSET_DWORDS + LRC_RING_HEAD_VALUE_DW;
    if context_virt.is_null() || context_len / core::mem::size_of::<u32>() <= index {
        return None;
    }
    unsafe {
        crate::intel::dma_flush(
            context_virt.add(index * core::mem::size_of::<u32>()),
            core::mem::size_of::<u32>(),
        );
        Some(core::ptr::read_volatile(context_virt.cast::<u32>().add(index)))
    }
}

pub(super) fn emit_store_dword_ppgtt(
    batch: &mut [u32],
    idx: &mut usize,
    gpu_addr: u64,
    value: u32,
) -> bool {
    if idx.saturating_add(4) > batch.len() {
        return false;
    }
    batch[*idx] = MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT;
    batch[*idx + 1] = gpu_addr as u32;
    batch[*idx + 2] = (gpu_addr >> 32) as u32;
    batch[*idx + 3] = value;
    *idx += 4;
    true
}

#[inline]
pub(super) fn media_cmd_header(
    media_opcode: u32,
    subopcode_a: u32,
    subopcode_b: u32,
    dword_length: u32,
) -> u32 {
    (3 << 29)
        | (MEDIA_PIPELINE_MFX << 27)
        | (media_opcode << 24)
        | (subopcode_a << 21)
        | (subopcode_b << 16)
        | dword_length
}

pub(super) fn begin_batch_packet(
    batch: &mut [u32],
    idx: &mut usize,
    dword_count: usize,
    header: u32,
) -> Option<usize> {
    if idx.saturating_add(dword_count) > batch.len() {
        return None;
    }
    let start = *idx;
    let end = start + dword_count;
    batch[start..end].fill(0);
    batch[start] = header;
    *idx = end;
    Some(start)
}

#[inline]
pub(super) fn packet_write_addr64(
    batch: &mut [u32],
    packet_start: usize,
    dword_index: usize,
    gpu_addr: u64,
) {
    batch[packet_start + dword_index] = gpu_addr as u32;
    batch[packet_start + dword_index + 1] = (gpu_addr >> 32) as u32;
}

pub(super) fn emit_mfx_wait(batch: &mut [u32], idx: &mut usize) -> bool {
    if *idx >= batch.len() {
        return false;
    }
    batch[*idx] = MFX_WAIT_SYNC;
    *idx += 1;
    true
}

#[inline]
pub(super) fn read_result_dword(base_virt: *mut u8, slot_off: u64) -> u32 {
    let ptr = (base_virt as usize).saturating_add(slot_off as usize) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

pub(super) fn execlist_submit_port_push(
    dev: crate::intel::Dev,
    ring_base: usize,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_LO, context0_lo);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_HI, context0_hi);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_LO + 8, context1_lo);
    super::mmio_write(dev, ring_base + RING_EXECLIST_SQ_HI + 8, context1_hi);
}

pub(super) fn wake_media_engine_forcewake(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
) -> MediaEngineForcewakeAck {
    let (req, ack) = match engine.id.class {
        MediaEngineClass::VideoDecode => match engine.id.instance {
            0 => (FORCEWAKE_MEDIA_VDBOX0, FORCEWAKE_ACK_VDBOX0),
            1 => (FORCEWAKE_MEDIA_VDBOX1, FORCEWAKE_ACK_VDBOX1),
            2 => (FORCEWAKE_MEDIA_VDBOX2, FORCEWAKE_ACK_VDBOX2),
            _ => (FORCEWAKE_MEDIA_VDBOX3, FORCEWAKE_ACK_VDBOX3),
        },
    };
    super::mmio_write(dev, req, super::mask_en(FORCEWAKE_KERNEL));
    let mut ack_value = 0u32;
    for _ in 0..20_000 {
        ack_value = super::mmio_read(dev, ack);
        if (ack_value & FORCEWAKE_KERNEL) != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    MediaEngineForcewakeAck {
        ack_reg: ack,
        ack_value,
        awake: (ack_value & FORCEWAKE_KERNEL) != 0,
    }
}

pub(super) fn wake_media_engine_for_guc(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
) -> bool {
    wake_media_engine_forcewake(dev, engine).awake
}

const GDRST: usize = 0x0000_941C;
const GRDOM_MEDIA_VCS0_SHIFT: u32 = 5;
const MODE_IDLE: u32 = 1 << 9;
const GEN12_HWSP_CSB_WRITE_OFFSET: usize = 0xBC;
const GEN12_CSB_RESET_VALUE: u32 = 11;
const GEN12_HWSP_CSB_BUF0_OFFSET: usize = 0x40;
const GEN12_CSB_ENTRIES: usize = 12;

pub(super) fn init_csb_pointers(dev: crate::intel::Dev, ring_base: usize, hwsp_virt: *mut u8) {
    let csb_init: u32 = 0xFFFF_0000 | (GEN12_CSB_RESET_VALUE << 8) | GEN12_CSB_RESET_VALUE;
    super::mmio_write(dev, ring_base + 0x3A0, csb_init);
    let _ = super::mmio_read(dev, ring_base + 0x3A0);
    unsafe {
        core::ptr::write_volatile(
            hwsp_virt.add(GEN12_HWSP_CSB_WRITE_OFFSET) as *mut u32,
            GEN12_CSB_RESET_VALUE,
        );
        let csb_buf = hwsp_virt.add(GEN12_HWSP_CSB_BUF0_OFFSET) as *mut u64;
        for i in 0..GEN12_CSB_ENTRIES {
            core::ptr::write_volatile(csb_buf.add(i), !0u64);
        }
    }
    core::sync::atomic::fence(Ordering::SeqCst);
    super::dma_flush(hwsp_virt, GEN12_HWSP_CSB_WRITE_OFFSET + 8);
    super::mmio_write(dev, ring_base + 0x3A0, csb_init);
    let _ = super::mmio_read(dev, ring_base + 0x3A0);
}

pub(super) fn reset_media_engine(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    _context_virt: *mut u8,
) {
    let ring_base = engine.ring_base;
    let reset_domain = 1u32
        .checked_shl(GRDOM_MEDIA_VCS0_SHIFT + u32::from(engine.id.instance))
        .unwrap_or(0);
    if reset_domain == 0 {
        return;
    }
    for _ in 0..200_000u32 {
        let el = super::mmio_read(dev, ring_base + RING_EXECLIST_STATUS_LO);
        if (el >> 30) == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    super::mmio_write(dev, ring_base + RING_MI_MODE, STOP_RING | (STOP_RING << 16));
    for _ in 0..50_000u32 {
        if super::mmio_read(dev, ring_base + RING_MI_MODE) & MODE_IDLE != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    super::mmio_write(dev, GDRST, reset_domain);
    for _ in 0..500_000u32 {
        if super::mmio_read(dev, GDRST) & reset_domain == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    super::mmio_write(dev, ring_base + RING_MI_MODE, STOP_RING << 16);
    super::ggtt_invalidate(dev);
}

pub(super) fn seed_media_ring_live_state(
    dev: crate::intel::Dev,
    ring_base: usize,
    pphwsp_gpu: u32,
    ring_start: u32,
    ring_ctl: u32,
    ring_tail: u32,
) {
    super::mmio_write(dev, ring_base + RING_HEAD, 0);
    super::mmio_write(dev, ring_base + RING_TAIL, ring_tail);
    super::mmio_write(dev, ring_base + RING_START, ring_start);
    super::mmio_write(dev, ring_base + RING_CTL, ring_ctl);
    super::mmio_write(dev, ring_base + RING_MI_MODE, STOP_RING << 16);
    super::mmio_write(dev, ring_base + RING_MI_MODE, masked_bit_disable(STOP_RING));
    super::mmio_write(dev, ring_base + RING_HWS_PGA, pphwsp_gpu);
    super::mmio_write(dev, ring_base + RING_HWSTAM, !0u32);
}

// These buffers have no submitted GPU owner until the complete backing is
// published. Roll back every earlier allocation (and GGTT alias) on any
// allocation, mapping, or page-table failure. Playback may retry every frame.
struct UnsubmittedDecodeBuffer {
    dev: crate::intel::Dev,
    phys: u64,
    virt: *mut u8,
    bytes: usize,
    gpu: u64,
    mapping_attempted: bool,
}

static DECODE_BACKING_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

fn report_decode_backing_failure(stage: &str, bytes: usize) {
    if !DECODE_BACKING_FAILURE_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_error!(target: "intel-media";
            "intel/hw_pic: decode-backing failed stage={} bytes={} pmm={:?} rollback=unsubmitted-buffers-only\n",
            stage, bytes, crate::phys::pmm_stats());
    }
}

impl UnsubmittedDecodeBuffer {
    fn new(dev: crate::intel::Dev, bytes: usize, gpu: u64) -> Option<Self> {
        let Some((phys, virt)) = crate::intel::alloc_ggtt_backing(bytes, crate::intel::WARM_ALIGN)
        else {
            report_decode_backing_failure("allocation", bytes);
            return None;
        };
        Some(Self {
            dev,
            phys,
            virt,
            bytes,
            gpu,
            mapping_attempted: false,
        })
    }

    fn map(&mut self) -> bool {
        // map_ggtt may fail after writing part of a range. Clear that range too.
        self.mapping_attempted = true;
        let mapped = super::map_ggtt(self.dev, self.phys, self.bytes, self.gpu);
        if !mapped {
            report_decode_backing_failure("ggtt", self.bytes);
        }
        mapped
    }

    fn retain(self) {
        core::mem::forget(self);
    }
}

impl Drop for UnsubmittedDecodeBuffer {
    fn drop(&mut self) {
        if self.mapping_attempted
            && !crate::intel::unmap_display_scanout_ggtt(self.dev, self.bytes, self.gpu)
        {
            // Never free memory while an alias may remain installed.
            report_decode_backing_failure("ggtt-rollback-retained", self.bytes);
            return;
        }
        crate::dma::dealloc(self.virt, self.bytes);
    }
}

pub(super) fn ensure_decode_backing(
    dev: crate::intel::Dev,
    windows: MediaGpuWindowLayout,
) -> Option<MediaBitstreamBacking> {
    // Serialize first-use construction as well as publication.
    let mut cached = MEDIA_BACKING.lock();
    if let Some(backing) = *cached {
        return Some(backing);
    }
    let mut ring =
        UnsubmittedDecodeBuffer::new(dev, MEDIA_DEFAULT_RING_BYTES, windows.ring_gpu_addr)?;
    let mut context =
        UnsubmittedDecodeBuffer::new(dev, MEDIA_DEFAULT_CONTEXT_BYTES, windows.context_gpu_addr)?;
    let mut batch =
        UnsubmittedDecodeBuffer::new(dev, MEDIA_DEFAULT_BATCH_BYTES, windows.batch_gpu_addr)?;
    let mut result =
        UnsubmittedDecodeBuffer::new(dev, MEDIA_DEFAULT_RESULT_BYTES, windows.result_gpu_addr)?;
    let mut bitstream = UnsubmittedDecodeBuffer::new(
        dev,
        MEDIA_DEFAULT_BITSTREAM_BYTES,
        windows.bitstream_gpu_addr,
    )?;
    let mut output_surface = UnsubmittedDecodeBuffer::new(
        dev,
        MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES,
        windows.output_surface_gpu_addr,
    )?;
    let mut avc_scratch = UnsubmittedDecodeBuffer::new(
        dev,
        MEDIA_DEFAULT_AVC_SCRATCH_BYTES,
        windows.avc_scratch_gpu_addr,
    )?;
    let (ring_phys, ring_virt) = (ring.phys, ring.virt);
    let (context_phys, context_virt) = (context.phys, context.virt);
    let (batch_phys, batch_virt) = (batch.phys, batch.virt);
    let (result_phys, result_virt) = (result.phys, result.virt);
    let (bitstream_phys, bitstream_virt) = (bitstream.phys, bitstream.virt);
    let (output_surface_phys, output_surface_virt) = (output_surface.phys, output_surface.virt);
    let (avc_scratch_phys, avc_scratch_virt) = (avc_scratch.phys, avc_scratch.virt);
    if !(ring.map()
        && context.map()
        && batch.map()
        && result.map()
        && bitstream.map()
        && output_surface.map()
        && avc_scratch.map())
    {
        return None;
    }
    super::ggtt_invalidate(dev);
    let ppgtt_pml4_phys = install_media_ppgtt(&[
        crate::intel::ppgtt::PpgttRange {
            gpu: windows.batch_gpu_addr,
            phys: batch_phys,
            bytes: MEDIA_DEFAULT_BATCH_BYTES,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: windows.bitstream_gpu_addr,
            phys: bitstream_phys,
            bytes: MEDIA_DEFAULT_BITSTREAM_BYTES,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: windows.output_surface_gpu_addr,
            phys: output_surface_phys,
            bytes: MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: windows.avc_scratch_gpu_addr,
            phys: avc_scratch_phys,
            bytes: MEDIA_DEFAULT_AVC_SCRATCH_BYTES,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: windows.result_gpu_addr,
            phys: result_phys,
            bytes: MEDIA_DEFAULT_RESULT_BYTES,
        },
    ])
    .or_else(|| {
        report_decode_backing_failure("ppgtt", 0);
        None
    })?;
    let backing = MediaBitstreamBacking {
        ring_phys,
        ring_virt,
        ring_bytes: MEDIA_DEFAULT_RING_BYTES,
        context_phys,
        context_virt,
        context_bytes: MEDIA_DEFAULT_CONTEXT_BYTES,
        batch_phys,
        batch_virt,
        batch_bytes: MEDIA_DEFAULT_BATCH_BYTES,
        result_phys,
        result_virt,
        result_bytes: MEDIA_DEFAULT_RESULT_BYTES,
        bitstream_phys,
        bitstream_virt,
        bitstream_bytes: MEDIA_DEFAULT_BITSTREAM_BYTES,
        output_surface_phys,
        output_surface_virt,
        output_surface_bytes: MEDIA_DEFAULT_OUTPUT_SURFACE_BYTES,
        avc_scratch_phys,
        avc_scratch_virt,
        avc_scratch_bytes: MEDIA_DEFAULT_AVC_SCRATCH_BYTES,
        ppgtt_pml4_phys,
    };
    ring.retain();
    context.retain();
    batch.retain();
    result.retain();
    bitstream.retain();
    output_surface.retain();
    avc_scratch.retain();
    DECODE_BACKING_FAILURE_LOGGED.store(false, Ordering::Release);
    *cached = Some(backing);
    Some(backing)
}

pub(super) fn stream_encoded_to_bitstream(
    dev: crate::intel::Dev,
    engine: MediaEngineDescriptor,
    windows: MediaGpuWindowLayout,
    backing: MediaBitstreamBacking,
    encoded: &[u8],
) -> Option<MediaEncodedStreamProof> {
    if encoded.is_empty() || encoded.len() > backing.bitstream_bytes {
        return None;
    }
    let engine_wake = wake_media_engine_forcewake(dev, engine);
    let wake = snapshot_forcewake(dev);
    unsafe {
        core::ptr::copy_nonoverlapping(encoded.as_ptr(), backing.bitstream_virt, encoded.len());
        let clear_len = backing
            .bitstream_bytes
            .saturating_sub(encoded.len())
            .min(256);
        if clear_len != 0 {
            core::ptr::write_bytes(backing.bitstream_virt.add(encoded.len()), 0, clear_len);
        }
    }
    super::dma_flush(backing.bitstream_virt, encoded.len());
    Some(MediaEncodedStreamProof {
        engine_name: engine.name,
        bitstream_gpu_addr: windows.bitstream_gpu_addr,
        bitstream_phys: backing.bitstream_phys,
        bitstream_virt: backing.bitstream_virt as usize,
        bytes_written: encoded.len(),
        capacity: backing.bitstream_bytes,
        signature: byte_signature(encoded),
        forcewake_engine_ack_reg: engine_wake.ack_reg,
        forcewake_engine_ack: engine_wake.ack_value,
        forcewake_engine_awake: engine_wake.awake,
        forcewake_global_ack: wake.global_ack,
        forcewake_awake_count: wake.awake_count,
    })
}
