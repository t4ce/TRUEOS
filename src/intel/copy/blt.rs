use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::Mutex;

const BCS0_RING_BASE: usize = 0x22000;
const RING_HEAD: usize = 0x34;
const RING_TAIL: usize = 0x30;
const RING_START: usize = 0x38;
const RING_CTL: usize = 0x3C;
const RING_HWS_PGA: usize = 0x80;
const RING_ACTHD: usize = 0x74;
const RING_IPEIR: usize = 0x64;
const RING_IPEHR: usize = 0x68;
const RING_EIR: usize = 0xB0;
const RING_MI_MODE: usize = 0x9C;
const RING_MODE_GEN7: usize = 0x29C;
const RING_CONTEXT_CONTROL: usize = 0x244;
const RING_CONTEXT_CONTROL_REF: usize = 0x5A0;
const RING_EXECLIST_CONTROL: usize = 0x550;
const RING_EXECLIST_STATUS_LO: usize = 0x234;
const RING_EXECLIST_STATUS_HI: usize = 0x238;
const RING_EXECLIST_SQ_LO: usize = 0x510;
const RING_EXECLIST_SQ_HI: usize = 0x514;

const RING_VALID: u32 = 1;
const EL_CTRL_LOAD: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const CTX_DESC_FORCE_RESTORE: u32 = 1 << 2;
const CTX_DESC_VALID: u32 = 1 << 0;
const CTX_DESC_PPGTT_ENABLE: u32 = 1 << 5;
const CTX_DESC_PRIVILEGE: u32 = 1 << 8;
const CTX_DESC_PRIORITY_NORMAL: u32 = 1 << 9;
const CTX_DESC_ADDRESSING_MODE_SHIFT: u32 = 3;
const INTEL_LEGACY_64B_CONTEXT: u32 = 3;
const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;
const STOP_RING: u32 = 1 << 8;
const MODE_IDLE: u32 = 1 << 9;

const MI_NOOP: u32 = 0;
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_LRI_CS_MMIO: u32 = 1 << 19;
const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_BATCH_PPGTT: u32 = 0;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_STORE_DATA_IMM_GGTT_DW1: u32 = 0x1040_0002;
const MI_FLUSH_DW: u32 = (0x26 << 23) | 3;
const MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE: u32 = 1 << 14;
const MI_ARB_CHECK: u32 = 0x0500_0005;

const DIRECT_BLT_RING_BYTES: usize = 4096;
const DIRECT_BLT_CONTEXT_BYTES: usize = 22 * 4096;
const DIRECT_BLT_BATCH_BYTES: usize = 4096;
const DIRECT_BLT_RESULT_BYTES: usize = 4096;
const DIRECT_BLT_COPY_BYTES: usize = 4096;
// Match the direct-RCS address envelope so BCS can consume the same stable,
// allocation-owned producer and compositor aliases. Reusing one small alias
// for different physical surfaces would require an explicit Gen12 TLB
// invalidation between jobs and is not a safe frame contract.
const DIRECT_BLT_PPGTT_PT_COUNT: usize = 512;
const DIRECT_BLT_PPGTT_BYTES: usize = (3 + DIRECT_BLT_PPGTT_PT_COUNT) * 4096;
const DIRECT_BLT_PPGTT_LIMIT_BYTES: u64 = DIRECT_BLT_PPGTT_PT_COUNT as u64 * 512 * 4096;
const DIRECT_BLT_LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();
const DIRECT_BLT_GPU_VA_RING_BASE: u64 = 0x00B0_0000;
const DIRECT_BLT_GPU_VA_CONTEXT_BASE: u64 = 0x00B1_0000;
const DIRECT_BLT_GPU_VA_BATCH_BASE: u64 = 0x00B4_0000;
const DIRECT_BLT_GPU_VA_RESULT_BASE: u64 = 0x00B5_0000;
const DIRECT_BLT_GPU_VA_SRC_BASE: u64 = 0x00B6_0000;
const DIRECT_BLT_GPU_VA_DST_BASE: u64 = 0x00B7_0000;
const GUC_BLT_UI4_MAX_COPIES: usize = 64;
const GUC_BLT_UI4_TIMEOUT_MS: u64 = 250;
const DIRECT_BLT_SMOKE_MARKER: u32 = 0xC0DE_BC50;
const DIRECT_BLT_SMOKE_POLL_ITERS: usize = 262_144;
const DIRECT_BLT_COPY_WIDTH: u32 = 4;
const DIRECT_BLT_COPY_HEIGHT: u32 = 4;
const DIRECT_BLT_COPY_PITCH_BYTES: u32 = DIRECT_BLT_COPY_WIDTH * core::mem::size_of::<u32>() as u32;
const DIRECT_BLT_SRC_BASE_PATTERN: u32 = 0xB17C_0000;
const DIRECT_BLT_DST_POISON_BASE: u32 = 0xD57D_0000;
const XY_FAST_COPY_BLT_CMD: u32 = (2 << 29) | (0x42 << 22) | 8;
const XY_FAST_COPY_COLOR_DEPTH_32: u32 = 3 << 24;
const XY_FAST_COPY_DST_SYSTEM_MEM: u32 = 1 << 28;
const XY_FAST_COPY_SRC_SYSTEM_MEM: u32 = 1 << 29;

static DIRECT_BLT_STATE: Mutex<Option<DirectBltState>> = Mutex::new(None);
static DIRECT_BLT_SUBMIT_LOCK: Mutex<()> = Mutex::new(());
static DIRECT_BLT_SMOKE_RAN: AtomicBool = AtomicBool::new(false);
static DIRECT_BLT_SUBMIT_COUNTER: AtomicU32 = AtomicU32::new(1);
static GUC_BLT_PROBE: Mutex<GucBltProbeRuntime> = Mutex::new(GucBltProbeRuntime::new());
static GUC_BLT_UI4: Mutex<GucBltUi4Runtime> = Mutex::new(GucBltUi4Runtime::new());
static GUC_BLT_RING: Mutex<GucBltRingRuntime> = Mutex::new(GucBltRingRuntime::new());
static GUC_BLT_LANE_BUSY: AtomicBool = AtomicBool::new(false);
static GUC_BLT_LANE_QUARANTINED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
struct DirectBltState {
    ring_phys: u64,
    ring_virt: *mut u8,
    context_phys: u64,
    context_virt: *mut u8,
    batch_phys: u64,
    batch_virt: *mut u8,
    result_phys: u64,
    result_virt: *mut u8,
    src_phys: u64,
    src_virt: *mut u8,
    dst_phys: u64,
    dst_virt: *mut u8,
    ppgtt_phys: u64,
    ppgtt_virt: *mut u8,
}

unsafe impl Send for DirectBltState {}
unsafe impl Sync for DirectBltState {}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GucBcs0FastCopyProbe {
    pub(crate) forcewake: bool,
    pub(crate) ggtt: bool,
    pub(crate) ppgtt: bool,
    pub(crate) batch: bool,
    pub(crate) submitted: bool,
    pub(crate) pending: bool,
    pub(crate) retired: bool,
    pub(crate) context_saved: bool,
    pub(crate) timeline_retired: bool,
    pub(crate) copy_ok: bool,
    pub(crate) src_preserved: bool,
    pub(crate) marker: u32,
    pub(crate) saved_head: u32,
    pub(crate) published_tail: u32,
    pub(crate) retire_ms: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CopyEngineActivitySnapshot {
    pub(crate) available: bool,
    pub(crate) guc_lane_busy: bool,
    pub(crate) guc_lane_quarantined: bool,
    pub(crate) guc_context_initialized: bool,
    pub(crate) guc_saved_head: u32,
    pub(crate) guc_published_tail: u32,
    pub(crate) submit_counter: u32,
    pub(crate) head: u32,
    pub(crate) tail: u32,
    pub(crate) start: u32,
    pub(crate) ctl: u32,
    pub(crate) acthd: u32,
    pub(crate) mi_mode: u32,
    pub(crate) mode: u32,
    pub(crate) context_control: u32,
    pub(crate) execlist_control: u32,
    pub(crate) execlist_status_lo: u32,
    pub(crate) execlist_status_hi: u32,
    pub(crate) ipeir: u32,
    pub(crate) ipehr: u32,
    pub(crate) eir: u32,
}

pub(crate) fn activity_snapshot() -> CopyEngineActivitySnapshot {
    let guc_lane_busy = GUC_BLT_LANE_BUSY.load(Ordering::Acquire);
    let guc_lane_quarantined = GUC_BLT_LANE_QUARANTINED.load(Ordering::Acquire);
    let ring_runtime = GUC_BLT_RING.lock();
    let guc_context_initialized = ring_runtime.context_initialized;
    let guc_saved_head = ring_runtime.last_saved_head;
    let guc_published_tail = ring_runtime.published_tail_bytes as u32;
    drop(ring_runtime);
    let submit_counter = DIRECT_BLT_SUBMIT_COUNTER.load(Ordering::Relaxed);
    let Some(dev) = crate::intel::claimed_device() else {
        return CopyEngineActivitySnapshot {
            guc_lane_busy,
            guc_lane_quarantined,
            guc_context_initialized,
            guc_saved_head,
            guc_published_tail,
            submit_counter,
            ..CopyEngineActivitySnapshot::default()
        };
    };

    CopyEngineActivitySnapshot {
        available: true,
        guc_lane_busy,
        guc_lane_quarantined,
        guc_context_initialized,
        guc_saved_head,
        guc_published_tail,
        submit_counter,
        head: super::mmio_read(dev, BCS0_RING_BASE + RING_HEAD),
        tail: super::mmio_read(dev, BCS0_RING_BASE + RING_TAIL),
        start: super::mmio_read(dev, BCS0_RING_BASE + RING_START),
        ctl: super::mmio_read(dev, BCS0_RING_BASE + RING_CTL),
        acthd: super::mmio_read(dev, BCS0_RING_BASE + RING_ACTHD),
        mi_mode: super::mmio_read(dev, BCS0_RING_BASE + RING_MI_MODE),
        mode: super::mmio_read(dev, BCS0_RING_BASE + RING_MODE_GEN7),
        context_control: super::mmio_read(dev, BCS0_RING_BASE + RING_CONTEXT_CONTROL),
        execlist_control: super::mmio_read(dev, BCS0_RING_BASE + RING_EXECLIST_CONTROL),
        execlist_status_lo: super::mmio_read(dev, BCS0_RING_BASE + RING_EXECLIST_STATUS_LO),
        execlist_status_hi: super::mmio_read(dev, BCS0_RING_BASE + RING_EXECLIST_STATUS_HI),
        ipeir: super::mmio_read(dev, BCS0_RING_BASE + RING_IPEIR),
        ipehr: super::mmio_read(dev, BCS0_RING_BASE + RING_IPEHR),
        eir: super::mmio_read(dev, BCS0_RING_BASE + RING_EIR),
    }
}

impl GucBcs0FastCopyProbe {
    pub(crate) const fn passed(self) -> bool {
        self.submitted
            && self.retired
            && self.context_saved
            && self.timeline_retired
            && self.copy_ok
            && self.src_preserved
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucBcs0RgbaSurface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucBcs0RgbaCopy {
    pub(crate) source: GucBcs0RgbaSurface,
    pub(crate) source_x: u32,
    pub(crate) source_y: u32,
    pub(crate) destination_x: u32,
    pub(crate) destination_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GucBcs0CopySubmission {
    sequence: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GucBcs0CopySubmitError {
    Unavailable,
    Busy,
    InvalidRequest,
    SubmitFailed,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GucBcs0CopyCompletion {
    Pending,
    Complete,
    Failed,
    InvalidSubmission,
}

struct GucBltUi4Runtime {
    pending: Option<crate::gpu::executor::KernelSubmission>,
    sequence: u32,
    expected_marker: u32,
    started_tick: u64,
    copies: usize,
    bytes: u64,
}

impl GucBltUi4Runtime {
    const fn new() -> Self {
        Self {
            pending: None,
            sequence: 0,
            expected_marker: 0,
            started_tick: 0,
            copies: 0,
            bytes: 0,
        }
    }
}

struct GucBltRingRuntime {
    published_tail_bytes: usize,
    context_initialized: bool,
    save_deferrals: u64,
    last_saved_head: u32,
}

impl GucBltRingRuntime {
    const fn new() -> Self {
        Self {
            published_tail_bytes: 0,
            context_initialized: false,
            save_deferrals: 0,
            last_saved_head: 0,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GucBltPrepareError {
    Busy,
    Invalid,
    Quarantined,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct GucBltContextSaveStatus {
    observed: bool,
    saved_head: u32,
    published_tail_bytes: usize,
}

struct GucBltProbeRuntime {
    pending: Option<crate::gpu::executor::KernelSubmission>,
    started_tick: u64,
    src_before: [u32; 4],
    dst_before: [u32; 4],
    completed: Option<GucBcs0FastCopyProbe>,
}

impl GucBltProbeRuntime {
    const fn new() -> Self {
        Self {
            pending: None,
            started_tick: 0,
            src_before: [0; 4],
            dst_before: [0; 4],
            completed: None,
        }
    }
}

/// Submit one 4x4 XY_FAST_COPY_BLT through the mediated GuC BCS0 lane.
///
/// This intentionally has no direct-execlist fallback. If the marker does not
/// retire during this call, the exact executor token and all backing storage
/// remain live; a later probe call only observes that same submission.
pub(crate) fn submit_guc_bcs0_fast_copy_probe_now() -> GucBcs0FastCopyProbe {
    if !guc_blt_state_reuse_permitted(&GUC_BLT_LANE_QUARANTINED) {
        return GucBcs0FastCopyProbe::default();
    }
    let Some(dev) = super::claimed_device() else {
        return GucBcs0FastCopyProbe::default();
    };
    let Some(state) = direct_blt_state_once() else {
        return GucBcs0FastCopyProbe::default();
    };
    let mut runtime = GUC_BLT_PROBE.lock();
    if let Some(completed) = runtime.completed {
        return completed;
    }
    if runtime.pending.is_some() {
        return guc_blt_poll_and_retire(state, &mut runtime);
    }
    if GUC_BLT_LANE_BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return GucBcs0FastCopyProbe::default();
    }
    if !guc_blt_context_save_status(state).observed {
        // No storage has been touched. Let a later call retry the
        // context-specific saved-HEAD gate instead of overwriting the batch,
        // PPGTT, or ring while GuC still owns them.
        GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
        return GucBcs0FastCopyProbe::default();
    }

    let forcewake = direct_blt_forcewake(dev);
    let ggtt = forcewake && direct_blt_map_state(dev, state);
    let ppgtt = ggtt && direct_blt_init_ppgtt(state);
    let (src_before, dst_before) = if ppgtt {
        direct_blt_seed_fast_copy_buffers(state)
    } else {
        ([0; 4], [0; 4])
    };
    let batch = ppgtt && direct_blt_encode_fast_copy_batch(state);
    let old_tail = match batch.then(|| guc_blt_prepare_context(state)) {
        Some(Ok(old_tail)) => old_tail,
        Some(Err(GucBltPrepareError::Quarantined)) => {
            return GucBcs0FastCopyProbe {
                forcewake,
                ggtt,
                ppgtt,
                batch,
                ..GucBcs0FastCopyProbe::default()
            };
        }
        Some(Err(GucBltPrepareError::Busy | GucBltPrepareError::Invalid)) | None => {
            GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
            return GucBcs0FastCopyProbe {
                forcewake,
                ggtt,
                ppgtt,
                batch,
                ..GucBcs0FastCopyProbe::default()
            };
        }
    };

    let (hwlrca_lo, hwlrca_hi) = guc_blt_context_descriptor(DIRECT_BLT_GPU_VA_CONTEXT_BASE);
    let descriptor = crate::gpu::physical::PhysicalContextDescriptor {
        engine: crate::gpu::physical::PhysicalEngineId::BCS0,
        hwlrca_lo,
        hwlrca_hi,
        gpuvm_root_phys: state.ppgtt_phys,
    };
    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);
    let submission = match crate::gpu::executor::submit_kernel_context(
        crate::gpu::vgpu::KernelClient::Ui4Blitter,
        descriptor,
    ) {
        Ok(submission) => submission,
        Err(crate::gpu::vgpu::VgpuError::Busy) => {
            guc_blt_rollback_context_tail(state, old_tail);
            GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
            return GucBcs0FastCopyProbe {
                forcewake,
                ggtt,
                ppgtt,
                batch,
                ..GucBcs0FastCopyProbe::default()
            };
        }
        Err(error) => {
            // A transport error after publishing the LRC tail can be
            // ambiguous: GuC may have consumed the H2G request even when the
            // host did not observe its response. Do not roll back or reuse.
            let isolation = guc_blt_quarantine("submit-ambiguous");
            crate::log!(
                "intel/blt: guc-bcs0-fast-copy submitted=0 error={:?} path=guc direct_elsp=0 legacy_fallback=0 action=quarantine-ui4-blitter device_found={} contexts_disabled={} contexts_retained={} storage_released=0\n",
                error,
                isolation.device_found as u8,
                isolation.contexts_disabled,
                isolation.contexts_retained,
            );
            return GucBcs0FastCopyProbe {
                forcewake,
                ggtt,
                ppgtt,
                batch,
                ..GucBcs0FastCopyProbe::default()
            };
        }
    };
    runtime.pending = Some(submission);
    runtime.started_tick = direct_blt_now_tick();
    runtime.src_before = src_before;
    runtime.dst_before = dst_before;
    guc_blt_poll_and_retire(state, &mut runtime)
}

fn guc_blt_saved_head_reached_tail(saved_head: u32, published_tail_bytes: usize) -> bool {
    (saved_head & (DIRECT_BLT_RING_BYTES as u32 - 1)) == published_tail_bytes as u32
}

fn guc_blt_observe_context_save(
    state: DirectBltState,
    runtime: &mut GucBltRingRuntime,
) -> GucBltContextSaveStatus {
    if !runtime.context_initialized {
        return GucBltContextSaveStatus {
            observed: true,
            saved_head: runtime.last_saved_head,
            published_tail_bytes: runtime.published_tail_bytes,
        };
    }

    let saved_head = guc_blt_read_lrc_ring_head(state);
    runtime.last_saved_head = saved_head;
    let observed = guc_blt_saved_head_reached_tail(saved_head, runtime.published_tail_bytes);
    if observed {
        if runtime.save_deferrals != 0 {
            crate::log_info!(target: "gfx";
                "intel/blt: guc-bcs0 context-save observed=1 saved_head={} published_tail={} deferrals={} action=storage-owned-by-cpu\n",
                saved_head & (DIRECT_BLT_RING_BYTES as u32 - 1),
                runtime.published_tail_bytes,
                runtime.save_deferrals,
            );
        }
        runtime.save_deferrals = 0;
    } else {
        runtime.save_deferrals = runtime.save_deferrals.saturating_add(1);
        if runtime.save_deferrals == 1 || runtime.save_deferrals.is_power_of_two() {
            crate::log_info!(target: "gfx";
                "intel/blt: guc-bcs0 context-save observed=0 saved_head={} published_tail={} deferrals={} ownership=wait-guc-context-save action=defer-storage-reuse\n",
                saved_head & (DIRECT_BLT_RING_BYTES as u32 - 1),
                runtime.published_tail_bytes,
                runtime.save_deferrals,
            );
        }
    }
    GucBltContextSaveStatus {
        observed,
        saved_head,
        published_tail_bytes: runtime.published_tail_bytes,
    }
}

fn guc_blt_context_save_status(state: DirectBltState) -> GucBltContextSaveStatus {
    let mut runtime = GUC_BLT_RING.lock();
    guc_blt_observe_context_save(state, &mut runtime)
}

fn guc_blt_prepare_context(state: DirectBltState) -> Result<usize, GucBltPrepareError> {
    if !guc_blt_state_reuse_permitted(&GUC_BLT_LANE_QUARANTINED) {
        return Err(GucBltPrepareError::Quarantined);
    }
    let mut runtime = GUC_BLT_RING.lock();
    if !guc_blt_observe_context_save(state, &mut runtime).observed {
        return Err(GucBltPrepareError::Busy);
    }
    let old_tail_bytes = runtime.published_tail_bytes;
    let Some(ring_ctl) = direct_blt_ring_ctl_value(DIRECT_BLT_RING_BYTES) else {
        return Err(GucBltPrepareError::Invalid);
    };
    let ring_tail_bytes = guc_blt_append_ring_batch_start(state, old_tail_bytes);
    if !runtime.context_initialized {
        if !direct_blt_init_context_image(
            state,
            DIRECT_BLT_GPU_VA_RING_BASE as u32,
            ring_tail_bytes as u32,
            ring_ctl,
        ) {
            return Err(GucBltPrepareError::Invalid);
        }
        runtime.context_initialized = true;
    } else if !guc_blt_write_lrc_ring_tail(state, ring_tail_bytes as u32) {
        return Err(GucBltPrepareError::Invalid);
    }
    runtime.published_tail_bytes = ring_tail_bytes;
    Ok(old_tail_bytes)
}

fn guc_blt_rollback_context_tail(state: DirectBltState, old_tail_bytes: usize) {
    let mut runtime = GUC_BLT_RING.lock();
    runtime.published_tail_bytes = old_tail_bytes;
    let _ = guc_blt_write_lrc_ring_tail(state, old_tail_bytes as u32);
}

fn guc_blt_poll_and_retire(
    state: DirectBltState,
    runtime: &mut GucBltProbeRuntime,
) -> GucBcs0FastCopyProbe {
    let observed = guc_blt_poll_result(state, DIRECT_BLT_SMOKE_MARKER);
    let marker_observed = observed == DIRECT_BLT_SMOKE_MARKER;
    let save_status = if marker_observed {
        guc_blt_context_save_status(state)
    } else {
        let ring = GUC_BLT_RING.lock();
        GucBltContextSaveStatus {
            observed: false,
            saved_head: ring.last_saved_head,
            published_tail_bytes: ring.published_tail_bytes,
        }
    };
    let retired = marker_observed && save_status.observed;
    let mut timeline_retired = false;
    if retired {
        if let Some(submission) = runtime.pending {
            timeline_retired =
                crate::gpu::executor::complete_kernel_submission(submission, true).is_some();
            if timeline_retired {
                runtime.pending.take();
                GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
            } else {
                let _ = guc_blt_quarantine("timeline-retire-failed");
            }
        }
    }
    let (src_after, dst_after) = direct_blt_read_fast_copy_buffers(state);
    let src_preserved = src_after == runtime.src_before;
    let copy_ok = dst_after == runtime.src_before;
    let report = GucBcs0FastCopyProbe {
        forcewake: true,
        ggtt: true,
        ppgtt: true,
        batch: true,
        submitted: true,
        pending: runtime.pending.is_some(),
        retired,
        context_saved: save_status.observed,
        timeline_retired,
        copy_ok,
        src_preserved,
        marker: observed,
        saved_head: save_status.saved_head,
        published_tail: save_status.published_tail_bytes as u32,
        retire_ms: direct_blt_elapsed_ms_since(runtime.started_tick),
    };
    crate::log_info!(
        target: "gfx";
        "intel/blt: guc-bcs0-fast-copy forcewake={} ggtt={} ppgtt={} batch={} submitted={} pending={} marker_observed={} context_saved={} retired={} timeline_retired={} copy_ok={} src_preserved={} dst_before_poisoned={} retire_ms={} observed=0x{:08X} expected=0x{:08X} saved_head={} published_tail={} engine=bcs0 class=3 instance=0 rect={}x{} pitch={} path=guc direct_elsp=0 legacy_fallback=0 cmd=xy-fast-copy-blt\n",
        report.forcewake as u8,
        report.ggtt as u8,
        report.ppgtt as u8,
        report.batch as u8,
        report.submitted as u8,
        report.pending as u8,
        marker_observed as u8,
        report.context_saved as u8,
        report.retired as u8,
        report.timeline_retired as u8,
        report.copy_ok as u8,
        report.src_preserved as u8,
        (runtime.dst_before == direct_blt_fast_copy_dst_expected_before()) as u8,
        report.retire_ms,
        report.marker,
        DIRECT_BLT_SMOKE_MARKER,
        report.saved_head & (DIRECT_BLT_RING_BYTES as u32 - 1),
        report.published_tail,
        DIRECT_BLT_COPY_WIDTH,
        DIRECT_BLT_COPY_HEIGHT,
        DIRECT_BLT_COPY_PITCH_BYTES,
    );
    if retired && timeline_retired {
        runtime.completed = Some(report);
    }
    report
}

/// Queue one ordered set of unscaled RGBA rectangle copies through GuC BCS0.
///
/// Source and destination GPU addresses are stable allocation-owned aliases.
/// The caller retains those allocations until
/// `poll_guc_bcs0_rgba_copies` reports completion.
pub(crate) fn queue_guc_bcs0_rgba_copies(
    destination: GucBcs0RgbaSurface,
    copies: &[GucBcs0RgbaCopy],
) -> Result<GucBcs0CopySubmission, GucBcs0CopySubmitError> {
    let Some(dev) = super::claimed_device() else {
        return Err(GucBcs0CopySubmitError::Unavailable);
    };
    let Some(state) = direct_blt_state_once() else {
        return Err(GucBcs0CopySubmitError::Unavailable);
    };
    if !guc_blt_valid_surface(destination)
        || copies.is_empty()
        || copies.len() > GUC_BLT_UI4_MAX_COPIES
    {
        return Err(GucBcs0CopySubmitError::InvalidRequest);
    }
    if GUC_BLT_UI4.lock().pending.is_some()
        || GUC_BLT_LANE_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return Err(GucBcs0CopySubmitError::Busy);
    }

    let prepared = (|| {
        let forcewake = direct_blt_forcewake(dev);
        let ggtt = forcewake && direct_blt_map_state(dev, state);
        let ppgtt = ggtt && direct_blt_init_ppgtt(state);
        if !ppgtt || !guc_blt_map_ui4_surfaces(state, destination, copies) {
            return None;
        }
        let sequence = DIRECT_BLT_SUBMIT_COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        let marker = 0xBC50_0000 | (sequence & 0xFFFF);
        let (copy_count, copied_bytes) =
            guc_blt_encode_ui4_copy_batch(state, destination, copies, marker)?;
        let old_tail = guc_blt_prepare_context(state)?;
        Some((sequence, marker, copy_count, copied_bytes, old_tail))
    })();
    let Some((sequence, marker, copy_count, copied_bytes, old_tail)) = prepared else {
        GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
        return Err(GucBcs0CopySubmitError::InvalidRequest);
    };

    let (hwlrca_lo, hwlrca_hi) = guc_blt_context_descriptor(DIRECT_BLT_GPU_VA_CONTEXT_BASE);
    let descriptor = crate::gpu::physical::PhysicalContextDescriptor {
        engine: crate::gpu::physical::PhysicalEngineId::BCS0,
        hwlrca_lo,
        hwlrca_hi,
        gpuvm_root_phys: state.ppgtt_phys,
    };
    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);
    let submission = match crate::gpu::executor::submit_kernel_context(
        crate::gpu::vgpu::KernelClient::Ui4Blitter,
        descriptor,
    ) {
        Ok(submission) => submission,
        Err(crate::gpu::vgpu::VgpuError::Busy) => {
            guc_blt_rollback_context_tail(state, old_tail);
            GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
            return Err(GucBcs0CopySubmitError::Busy);
        }
        Err(error) => {
            guc_blt_rollback_context_tail(state, old_tail);
            GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
            crate::log_error!(target: "gfx";
                "intel/blt: ui4-bcs0 submitted=0 error={:?} copies={} path=guc direct_elsp=0 legacy_fallback=0\n",
                error, copy_count,
            );
            return Err(GucBcs0CopySubmitError::SubmitFailed);
        }
    };
    let mut runtime = GUC_BLT_UI4.lock();
    runtime.pending = Some(submission);
    runtime.sequence = sequence;
    runtime.expected_marker = marker;
    runtime.started_tick = direct_blt_now_tick();
    runtime.copies = copy_count;
    runtime.bytes = copied_bytes;
    crate::log_trace!(target: "ui4";
        "ui4/blt: queued sequence={} engine=bcs0 path=guc copies={} bytes={} marker=0x{:08X} destination={}x{} pitch=0x{:X} direct_elsp=0 legacy_fallback=0\n",
        sequence, copy_count, copied_bytes, marker, destination.width,
        destination.height, destination.pitch_bytes,
    );
    Ok(GucBcs0CopySubmission { sequence })
}

pub(crate) fn poll_guc_bcs0_rgba_copies(
    submission: GucBcs0CopySubmission,
) -> GucBcs0CopyCompletion {
    let Some(state) = direct_blt_state_once() else {
        return GucBcs0CopyCompletion::Failed;
    };
    let mut runtime = GUC_BLT_UI4.lock();
    if runtime.pending.is_none() || runtime.sequence != submission.sequence {
        return GucBcs0CopyCompletion::InvalidSubmission;
    }
    super::dma_flush(state.result_virt, core::mem::size_of::<u32>());
    let observed = unsafe { core::ptr::read_volatile(state.result_virt.cast::<u32>()) };
    if observed != runtime.expected_marker {
        if direct_blt_elapsed_ms_since(runtime.started_tick) < GUC_BLT_UI4_TIMEOUT_MS {
            return GucBcs0CopyCompletion::Pending;
        }
        if let Some(pending) = runtime.pending.take() {
            let _ = crate::gpu::executor::complete_kernel_submission(pending, false);
        }
        GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
        crate::log_error!(target: "gfx";
            "intel/blt: ui4-bcs0 timeout sequence={} copies={} bytes={} observed=0x{:08X} expected=0x{:08X} timeout_ms={}\n",
            runtime.sequence, runtime.copies, runtime.bytes, observed,
            runtime.expected_marker, GUC_BLT_UI4_TIMEOUT_MS,
        );
        return GucBcs0CopyCompletion::Failed;
    }
    let Some(pending) = runtime.pending.take() else {
        return GucBcs0CopyCompletion::InvalidSubmission;
    };
    let timeline_retired =
        crate::gpu::executor::complete_kernel_submission(pending, true).is_some();
    GUC_BLT_LANE_BUSY.store(false, Ordering::Release);
    if !timeline_retired {
        return GucBcs0CopyCompletion::Failed;
    }
    crate::log_trace!(target: "ui4";
        "ui4/blt: retired sequence={} engine=bcs0 path=guc copies={} bytes={} marker=0x{:08X} elapsed_ms={} timeline_retired=1\n",
        runtime.sequence, runtime.copies, runtime.bytes, observed,
        direct_blt_elapsed_ms_since(runtime.started_tick),
    );
    GucBcs0CopyCompletion::Complete
}

fn guc_blt_poll_result(state: DirectBltState, expected: u32) -> u32 {
    let mut observed = 0;
    for _ in 0..DIRECT_BLT_SMOKE_POLL_ITERS {
        super::dma_flush(state.result_virt, core::mem::size_of::<u32>());
        observed = unsafe { core::ptr::read_volatile(state.result_virt as *const u32) };
        if observed == expected {
            break;
        }
        core::hint::spin_loop();
    }
    observed
}

/// Keep the complete GuC/vGPU descriptor stable across submissions so the
/// broker reuses one registered context while the saved ring tail advances.
fn guc_blt_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let descriptor = base
        | CTX_DESC_VALID
        | CTX_DESC_PRIVILEGE
        | (INTEL_LEGACY_64B_CONTEXT << CTX_DESC_ADDRESSING_MODE_SHIFT);
    (descriptor, (context_gpu_addr >> 32) as u32)
}

pub(crate) fn submit_bcs0_mi_smoke_once() -> bool {
    if DIRECT_BLT_SMOKE_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gfx";
            "intel/blt: bcs0-mi-smoke skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_blt_state_once() else {
        crate::log_info!(
            target: "gfx";
            "intel/blt: bcs0-mi-smoke failed rung=alloc\n"
        );
        return false;
    };

    let forcewake_ok = direct_blt_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_blt_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_blt_init_ppgtt(state);
    let (src_before, dst_before) = if ppgtt_ok {
        direct_blt_seed_fast_copy_buffers(state)
    } else {
        ([0; 4], [0; 4])
    };
    let batch_ok = ppgtt_ok && direct_blt_encode_fast_copy_batch(state);
    let submit_start_tick = direct_blt_now_tick();
    let submitted = batch_ok && direct_blt_submit_batch(dev, state);
    let observed = if submitted {
        direct_blt_poll_result(state, DIRECT_BLT_SMOKE_MARKER)
    } else {
        0
    };
    let retire_ms = if submitted {
        direct_blt_elapsed_ms_since(submit_start_tick)
    } else {
        0
    };
    let retired = observed == DIRECT_BLT_SMOKE_MARKER;
    let (src_after, dst_after) = direct_blt_read_fast_copy_buffers(state);
    let src_preserved = src_after == src_before;
    let dst_before_poisoned = dst_before == direct_blt_fast_copy_dst_expected_before();
    let copy_ok = dst_after == src_before;

    crate::log_info!(
        target: "gfx";
        "intel/blt: bcs0-fast-copy forcewake={} ggtt={} ppgtt={} batch={} submitted={} retired={} copy_ok={} src_preserved={} dst_before_poisoned={} retire_ms={} observed=0x{:08X} expected=0x{:08X} src_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] dst_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] src_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] dst_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] engine_base=0x{:X} ring_gpu=0x{:X} ctx_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} src_gpu=0x{:X} dst_gpu=0x{:X} rect={}x{} pitch={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} el=0x{:08X}:0x{:08X} path=direct-execlist no_guc_submit=1 cmd=xy-fast-copy-blt\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        copy_ok as u8,
        src_preserved as u8,
        dst_before_poisoned as u8,
        retire_ms,
        observed,
        DIRECT_BLT_SMOKE_MARKER,
        src_before[0],
        src_before[1],
        src_before[2],
        src_before[3],
        dst_before[0],
        dst_before[1],
        dst_before[2],
        dst_before[3],
        src_after[0],
        src_after[1],
        src_after[2],
        src_after[3],
        dst_after[0],
        dst_after[1],
        dst_after[2],
        dst_after[3],
        BCS0_RING_BASE,
        DIRECT_BLT_GPU_VA_RING_BASE,
        DIRECT_BLT_GPU_VA_CONTEXT_BASE,
        DIRECT_BLT_GPU_VA_BATCH_BASE,
        DIRECT_BLT_GPU_VA_RESULT_BASE,
        DIRECT_BLT_GPU_VA_SRC_BASE,
        DIRECT_BLT_GPU_VA_DST_BASE,
        DIRECT_BLT_COPY_WIDTH,
        DIRECT_BLT_COPY_HEIGHT,
        DIRECT_BLT_COPY_PITCH_BYTES,
        super::mmio_read(dev, BCS0_RING_BASE + RING_HEAD),
        super::mmio_read(dev, BCS0_RING_BASE + RING_TAIL),
        super::mmio_read(dev, BCS0_RING_BASE + RING_ACTHD),
        super::mmio_read(dev, BCS0_RING_BASE + RING_IPEIR),
        super::mmio_read(dev, BCS0_RING_BASE + RING_IPEHR),
        super::mmio_read(dev, BCS0_RING_BASE + RING_EIR),
        super::mmio_read(dev, BCS0_RING_BASE + RING_EXECLIST_STATUS_LO),
        super::mmio_read(dev, BCS0_RING_BASE + RING_EXECLIST_STATUS_HI),
    );

    retired && copy_ok
}

fn direct_blt_state_once() -> Option<DirectBltState> {
    if let Some(state) = *DIRECT_BLT_STATE.lock() {
        return Some(state);
    }

    let (ring_phys, ring_virt) = crate::dma::alloc(DIRECT_BLT_RING_BYTES, super::WARM_ALIGN)?;
    let (context_phys, context_virt) =
        crate::dma::alloc(DIRECT_BLT_CONTEXT_BYTES, super::WARM_ALIGN)?;
    let (batch_phys, batch_virt) = crate::dma::alloc(DIRECT_BLT_BATCH_BYTES, super::WARM_ALIGN)?;
    let (result_phys, result_virt) = crate::dma::alloc(DIRECT_BLT_RESULT_BYTES, super::WARM_ALIGN)?;
    let (src_phys, src_virt) = crate::dma::alloc(DIRECT_BLT_COPY_BYTES, super::WARM_ALIGN)?;
    let (dst_phys, dst_virt) = crate::dma::alloc(DIRECT_BLT_COPY_BYTES, super::WARM_ALIGN)?;
    let (ppgtt_phys, ppgtt_virt) = crate::dma::alloc(DIRECT_BLT_PPGTT_BYTES, super::WARM_ALIGN)?;

    unsafe {
        core::ptr::write_bytes(ring_virt, 0, DIRECT_BLT_RING_BYTES);
        core::ptr::write_bytes(context_virt, 0, DIRECT_BLT_CONTEXT_BYTES);
        core::ptr::write_bytes(batch_virt, 0, DIRECT_BLT_BATCH_BYTES);
        core::ptr::write_bytes(result_virt, 0, DIRECT_BLT_RESULT_BYTES);
        core::ptr::write_bytes(src_virt, 0, DIRECT_BLT_COPY_BYTES);
        core::ptr::write_bytes(dst_virt, 0, DIRECT_BLT_COPY_BYTES);
        core::ptr::write_bytes(ppgtt_virt, 0, DIRECT_BLT_PPGTT_BYTES);
    }

    let state = DirectBltState {
        ring_phys,
        ring_virt,
        context_phys,
        context_virt,
        batch_phys,
        batch_virt,
        result_phys,
        result_virt,
        src_phys,
        src_virt,
        dst_phys,
        dst_virt,
        ppgtt_phys,
        ppgtt_virt,
    };
    *DIRECT_BLT_STATE.lock() = Some(state);
    Some(state)
}

fn direct_blt_map_state(dev: super::Dev, state: DirectBltState) -> bool {
    let mapped =
        super::map_ggtt(dev, state.ring_phys, DIRECT_BLT_RING_BYTES, DIRECT_BLT_GPU_VA_RING_BASE)
            && super::map_ggtt(
                dev,
                state.context_phys,
                DIRECT_BLT_CONTEXT_BYTES,
                DIRECT_BLT_GPU_VA_CONTEXT_BASE,
            )
            && super::map_ggtt(
                dev,
                state.batch_phys,
                DIRECT_BLT_BATCH_BYTES,
                DIRECT_BLT_GPU_VA_BATCH_BASE,
            )
            && super::map_ggtt(
                dev,
                state.result_phys,
                DIRECT_BLT_RESULT_BYTES,
                DIRECT_BLT_GPU_VA_RESULT_BASE,
            )
            && super::map_ggtt(
                dev,
                state.src_phys,
                DIRECT_BLT_COPY_BYTES,
                DIRECT_BLT_GPU_VA_SRC_BASE,
            )
            && super::map_ggtt(
                dev,
                state.dst_phys,
                DIRECT_BLT_COPY_BYTES,
                DIRECT_BLT_GPU_VA_DST_BASE,
            );
    if mapped {
        super::ggtt_invalidate(dev);
    }
    mapped
}

fn direct_blt_init_ppgtt(state: DirectBltState) -> bool {
    let pml4_off = 0usize;
    let pdp_off = 4096usize;
    let pd_off = 8192usize;
    let pt_off = 12288usize;
    let pte_present_rw = super::GEN8_PAGE_PRESENT | (1 << 1);
    let pde_present_rw_uc = pte_present_rw | (1 << 3) | (1 << 4);

    unsafe {
        core::ptr::write_bytes(state.ppgtt_virt, 0, DIRECT_BLT_PPGTT_BYTES);
        let pml4 = state.ppgtt_virt.add(pml4_off) as *mut u64;
        let pdp = state.ppgtt_virt.add(pdp_off) as *mut u64;
        let pd = state.ppgtt_virt.add(pd_off) as *mut u64;
        core::ptr::write_volatile(pml4, (state.ppgtt_phys + pdp_off as u64) | pde_present_rw_uc);
        core::ptr::write_volatile(pdp, (state.ppgtt_phys + pd_off as u64) | pde_present_rw_uc);
        for index in 0..DIRECT_BLT_PPGTT_PT_COUNT {
            let pt_phys = state.ppgtt_phys + pt_off as u64 + (index as u64) * 4096;
            core::ptr::write_volatile(pd.add(index), pt_phys | pde_present_rw_uc);
        }
    }

    let ok = direct_blt_map_ppgtt_region(
        state,
        DIRECT_BLT_GPU_VA_RING_BASE,
        state.ring_phys,
        DIRECT_BLT_RING_BYTES,
        pte_present_rw,
    ) && direct_blt_map_ppgtt_region(
        state,
        DIRECT_BLT_GPU_VA_CONTEXT_BASE,
        state.context_phys,
        DIRECT_BLT_CONTEXT_BYTES,
        pte_present_rw,
    ) && direct_blt_map_ppgtt_region(
        state,
        DIRECT_BLT_GPU_VA_BATCH_BASE,
        state.batch_phys,
        DIRECT_BLT_BATCH_BYTES,
        pte_present_rw,
    ) && direct_blt_map_ppgtt_region(
        state,
        DIRECT_BLT_GPU_VA_RESULT_BASE,
        state.result_phys,
        DIRECT_BLT_RESULT_BYTES,
        pte_present_rw,
    ) && direct_blt_map_ppgtt_region(
        state,
        DIRECT_BLT_GPU_VA_SRC_BASE,
        state.src_phys,
        DIRECT_BLT_COPY_BYTES,
        pte_present_rw,
    ) && direct_blt_map_ppgtt_region(
        state,
        DIRECT_BLT_GPU_VA_DST_BASE,
        state.dst_phys,
        DIRECT_BLT_COPY_BYTES,
        pte_present_rw,
    );

    super::dma_flush(state.ppgtt_virt, DIRECT_BLT_PPGTT_BYTES);
    ok
}

fn direct_blt_map_ppgtt_region(
    state: DirectBltState,
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
        if pd_index >= DIRECT_BLT_PPGTT_PT_COUNT {
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

fn direct_blt_forcewake(dev: super::Dev) -> bool {
    // BCS consumes the retained physical-GT contract established at boot.
    // Repairing shared forcewake from a client submission can transition the
    // GT underneath an unrelated live GuC context.
    super::physical_gt_ready(dev)
}

fn direct_blt_encode_fast_copy_batch(state: DirectBltState) -> bool {
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_BLT_BATCH_BYTES);
        core::ptr::write_bytes(state.ring_virt, 0, DIRECT_BLT_RING_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_BLT_RESULT_BYTES);

        let result = state.result_virt as *mut u32;
        core::ptr::write_volatile(result, 0);

        let batch = state.batch_virt as *mut u32;
        core::ptr::write_volatile(batch, XY_FAST_COPY_BLT_CMD);
        core::ptr::write_volatile(
            batch.add(1),
            DIRECT_BLT_COPY_PITCH_BYTES
                | XY_FAST_COPY_COLOR_DEPTH_32
                | XY_FAST_COPY_DST_SYSTEM_MEM
                | XY_FAST_COPY_SRC_SYSTEM_MEM,
        );
        core::ptr::write_volatile(batch.add(2), 0);
        core::ptr::write_volatile(
            batch.add(3),
            DIRECT_BLT_COPY_WIDTH | (DIRECT_BLT_COPY_HEIGHT << 16),
        );
        core::ptr::write_volatile(batch.add(4), DIRECT_BLT_GPU_VA_DST_BASE as u32);
        core::ptr::write_volatile(batch.add(5), (DIRECT_BLT_GPU_VA_DST_BASE >> 32) as u32);
        core::ptr::write_volatile(batch.add(6), 0);
        core::ptr::write_volatile(batch.add(7), DIRECT_BLT_COPY_PITCH_BYTES);
        core::ptr::write_volatile(batch.add(8), DIRECT_BLT_GPU_VA_SRC_BASE as u32);
        core::ptr::write_volatile(batch.add(9), (DIRECT_BLT_GPU_VA_SRC_BASE >> 32) as u32);
        core::ptr::write_volatile(batch.add(10), MI_STORE_DATA_IMM_GGTT_DW1);
        core::ptr::write_volatile(batch.add(11), DIRECT_BLT_GPU_VA_RESULT_BASE as u32);
        core::ptr::write_volatile(batch.add(12), (DIRECT_BLT_GPU_VA_RESULT_BASE >> 32) as u32);
        core::ptr::write_volatile(batch.add(13), DIRECT_BLT_SMOKE_MARKER);
        core::ptr::write_volatile(batch.add(14), MI_ARB_CHECK);
        core::ptr::write_volatile(batch.add(15), MI_BATCH_BUFFER_END);
    }
    super::dma_flush(state.batch_virt, 16 * core::mem::size_of::<u32>());
    super::dma_flush(state.result_virt, DIRECT_BLT_RESULT_BYTES);
    true
}

fn guc_blt_valid_surface(surface: GucBcs0RgbaSurface) -> bool {
    if surface.phys == 0
        || !surface.phys.is_multiple_of(super::WARM_ALIGN as u64)
        || surface.gpu == 0
        || !surface.gpu.is_multiple_of(super::WARM_ALIGN as u64)
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < surface.width.saturating_mul(4)
        || !surface.pitch_bytes.is_multiple_of(4)
        || surface.pitch_bytes >= (1 << 18)
    {
        return false;
    }
    let allocation_valid = (surface.height as usize)
        .checked_sub(1)
        .and_then(|row| row.checked_mul(surface.pitch_bytes as usize))
        .and_then(|last_row| last_row.checked_add(surface.width as usize * 4))
        .is_some_and(|required| required <= surface.bytes);
    let gpu_range_valid = u64::try_from(surface.bytes)
        .ok()
        .and_then(|bytes| surface.gpu.checked_add(bytes))
        .is_some_and(|end| end <= DIRECT_BLT_PPGTT_LIMIT_BYTES);
    allocation_valid && gpu_range_valid
}

fn guc_blt_valid_copy(destination: GucBcs0RgbaSurface, copy: GucBcs0RgbaCopy) -> bool {
    guc_blt_valid_surface(copy.source)
        && !guc_blt_physical_ranges_overlap(destination, copy.source)
        && copy.width != 0
        && copy.height != 0
        && copy
            .source_x
            .checked_add(copy.width)
            .is_some_and(|right| right <= copy.source.width && right <= u16::MAX as u32)
        && copy
            .source_y
            .checked_add(copy.height)
            .is_some_and(|bottom| bottom <= copy.source.height && bottom <= u16::MAX as u32)
        && copy
            .destination_x
            .checked_add(copy.width)
            .is_some_and(|right| right <= destination.width && right <= u16::MAX as u32)
        && copy
            .destination_y
            .checked_add(copy.height)
            .is_some_and(|bottom| bottom <= destination.height && bottom <= u16::MAX as u32)
}

fn guc_blt_map_ui4_surfaces(
    state: DirectBltState,
    destination: GucBcs0RgbaSurface,
    copies: &[GucBcs0RgbaCopy],
) -> bool {
    let pte_present_rw_wb = super::GEN8_PAGE_PRESENT | (1 << 1);
    // PAT3 is the system-memory UC contract used by render-to-scanout targets.
    // BCS completion therefore releases into memory visible to the display,
    // rather than requiring a post-copy CPU sweep of the destination.
    let pte_present_rw_scanout_uc = pte_present_rw_wb | (1 << 3) | (1 << 4);
    if !direct_blt_map_ppgtt_region(
        state,
        destination.gpu,
        destination.phys,
        destination.bytes,
        pte_present_rw_scanout_uc,
    ) {
        return false;
    }

    for copy in copies.iter().copied() {
        if !guc_blt_valid_copy(destination, copy) {
            return false;
        }
        if !direct_blt_map_ppgtt_region(
            state,
            copy.source.gpu,
            copy.source.phys,
            copy.source.bytes,
            pte_present_rw_wb,
        ) {
            return false;
        }
    }
    super::dma_flush(state.ppgtt_virt, DIRECT_BLT_PPGTT_BYTES);
    true
}

fn guc_blt_encode_ui4_copy_batch(
    state: DirectBltState,
    destination: GucBcs0RgbaSurface,
    copies: &[GucBcs0RgbaCopy],
    marker: u32,
) -> Option<(usize, u64)> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            state.batch_virt.cast::<u32>(),
            DIRECT_BLT_BATCH_BYTES / core::mem::size_of::<u32>(),
        )
    };
    batch.fill(0);
    unsafe {
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_BLT_RESULT_BYTES);
    }
    let mut cursor = 0usize;
    let mut copied_bytes = 0u64;
    for copy in copies.iter().copied() {
        if !guc_blt_valid_copy(destination, copy) || cursor.saturating_add(10) > batch.len() {
            return None;
        }
        let destination_right = copy.destination_x.checked_add(copy.width)?;
        let destination_bottom = copy.destination_y.checked_add(copy.height)?;
        batch[cursor] = XY_FAST_COPY_BLT_CMD;
        batch[cursor + 1] = destination.pitch_bytes
            | XY_FAST_COPY_COLOR_DEPTH_32
            | XY_FAST_COPY_DST_SYSTEM_MEM
            | XY_FAST_COPY_SRC_SYSTEM_MEM;
        batch[cursor + 2] = copy.destination_x | (copy.destination_y << 16);
        batch[cursor + 3] = destination_right | (destination_bottom << 16);
        batch[cursor + 4] = destination.gpu as u32;
        batch[cursor + 5] = (destination.gpu >> 32) as u32;
        batch[cursor + 6] = copy.source_x | (copy.source_y << 16);
        batch[cursor + 7] = copy.source.pitch_bytes;
        batch[cursor + 8] = copy.source.gpu as u32;
        batch[cursor + 9] = (copy.source.gpu >> 32) as u32;
        cursor += 10;
        copied_bytes = copied_bytes.saturating_add(
            u64::from(copy.width)
                .saturating_mul(u64::from(copy.height))
                .saturating_mul(4),
        );
    }

    // MI_FLUSH_DW's post-sync immediate is the completion cookie. Unlike a
    // later plain store, it is ordered after the preceding BCS writes. The
    // compositor will not stage PLANE_SURF until this exact cookie retires.
    if cursor.saturating_add(8) > batch.len() {
        return None;
    }
    batch[cursor] = MI_FLUSH_DW | MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE;
    batch[cursor + 1] = DIRECT_BLT_GPU_VA_RESULT_BASE as u32;
    batch[cursor + 2] = (DIRECT_BLT_GPU_VA_RESULT_BASE >> 32) as u32;
    batch[cursor + 3] = marker;
    batch[cursor + 4] = 0;
    batch[cursor + 5] = MI_ARB_CHECK;
    batch[cursor + 6] = MI_BATCH_BUFFER_END;
    batch[cursor + 7] = MI_NOOP;
    cursor += 8;
    super::dma_flush(state.batch_virt, cursor.saturating_mul(core::mem::size_of::<u32>()));
    super::dma_flush(state.result_virt, core::mem::size_of::<u32>());
    Some((copies.len(), copied_bytes))
}

fn guc_blt_physical_ranges_overlap(left: GucBcs0RgbaSurface, right: GucBcs0RgbaSurface) -> bool {
    let left_end = u64::try_from(left.bytes)
        .ok()
        .and_then(|bytes| left.phys.checked_add(bytes));
    let right_end = u64::try_from(right.bytes)
        .ok()
        .and_then(|bytes| right.phys.checked_add(bytes));
    match (left_end, right_end) {
        (Some(left_end), Some(right_end)) => left.phys < right_end && right.phys < left_end,
        _ => true,
    }
}

fn direct_blt_seed_fast_copy_buffers(state: DirectBltState) -> ([u32; 4], [u32; 4]) {
    let expected_src = direct_blt_fast_copy_src_expected();
    let expected_dst = direct_blt_fast_copy_dst_expected_before();
    unsafe {
        core::ptr::write_bytes(state.src_virt, 0, DIRECT_BLT_COPY_BYTES);
        core::ptr::write_bytes(state.dst_virt, 0, DIRECT_BLT_COPY_BYTES);
        let src = state.src_virt as *mut u32;
        let dst = state.dst_virt as *mut u32;
        let pixels = (DIRECT_BLT_COPY_WIDTH * DIRECT_BLT_COPY_HEIGHT) as usize;
        for index in 0..pixels {
            core::ptr::write_volatile(src.add(index), direct_blt_src_pixel(index));
            core::ptr::write_volatile(dst.add(index), direct_blt_dst_poison_pixel(index));
        }
    }
    super::dma_flush(state.src_virt, DIRECT_BLT_COPY_BYTES);
    super::dma_flush(state.dst_virt, DIRECT_BLT_COPY_BYTES);
    (expected_src, expected_dst)
}

fn direct_blt_read_fast_copy_buffers(state: DirectBltState) -> ([u32; 4], [u32; 4]) {
    super::dma_flush(state.src_virt, DIRECT_BLT_COPY_BYTES);
    super::dma_flush(state.dst_virt, DIRECT_BLT_COPY_BYTES);
    let mut src_values = [0u32; 4];
    let mut dst_values = [0u32; 4];
    unsafe {
        let src = state.src_virt as *const u32;
        let dst = state.dst_virt as *const u32;
        for index in 0..4usize {
            src_values[index] = core::ptr::read_volatile(src.add(index));
            dst_values[index] = core::ptr::read_volatile(dst.add(index));
        }
    }
    (src_values, dst_values)
}

fn direct_blt_fast_copy_src_expected() -> [u32; 4] {
    [
        direct_blt_src_pixel(0),
        direct_blt_src_pixel(1),
        direct_blt_src_pixel(2),
        direct_blt_src_pixel(3),
    ]
}

fn direct_blt_fast_copy_dst_expected_before() -> [u32; 4] {
    [
        direct_blt_dst_poison_pixel(0),
        direct_blt_dst_poison_pixel(1),
        direct_blt_dst_poison_pixel(2),
        direct_blt_dst_poison_pixel(3),
    ]
}

fn direct_blt_src_pixel(index: usize) -> u32 {
    DIRECT_BLT_SRC_BASE_PATTERN | (index as u32)
}

fn direct_blt_dst_poison_pixel(index: usize) -> u32 {
    DIRECT_BLT_DST_POISON_BASE | (index as u32)
}

fn direct_blt_submit_batch(dev: super::Dev, state: DirectBltState) -> bool {
    let Some(ring_tail_bytes) = direct_blt_build_ring_batch_start(state) else {
        return false;
    };
    let Some(ring_ctl) = direct_blt_ring_ctl_value(DIRECT_BLT_RING_BYTES) else {
        return false;
    };
    if !direct_blt_init_context_image(
        state,
        DIRECT_BLT_GPU_VA_RING_BASE as u32,
        ring_tail_bytes as u32,
        ring_ctl,
    ) {
        return false;
    }

    let _submit_guard = DIRECT_BLT_SUBMIT_LOCK.lock();
    direct_blt_wait_idleish(dev);

    let pphwsp_gpu = (DIRECT_BLT_GPU_VA_CONTEXT_BASE & !0xFFF) as u32;
    direct_blt_init_csb_pointers(dev, state.context_virt);
    super::mmio_write(
        dev,
        BCS0_RING_BASE + RING_MODE_GEN7,
        direct_blt_masked_bit_enable(GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    let ctx_ctl = direct_blt_ctx_control_value(false);
    super::mmio_write(dev, BCS0_RING_BASE + RING_CONTEXT_CONTROL, ctx_ctl);
    super::mmio_write(dev, BCS0_RING_BASE + RING_CONTEXT_CONTROL_REF, ctx_ctl);
    super::mmio_write(dev, BCS0_RING_BASE + RING_MI_MODE, direct_blt_masked_bit_disable(STOP_RING));
    super::mmio_write(dev, BCS0_RING_BASE + RING_HWS_PGA, pphwsp_gpu);
    super::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);

    let (context_desc_lo, context_desc_hi) =
        direct_blt_context_descriptor(DIRECT_BLT_GPU_VA_CONTEXT_BASE);
    direct_blt_execlist_submit_port_push(dev, context_desc_lo, context_desc_hi, 0, 0);
    super::mmio_write(dev, BCS0_RING_BASE + RING_EXECLIST_CONTROL, EL_CTRL_LOAD);
    super::mmio_write(dev, BCS0_RING_BASE + RING_TAIL, ring_tail_bytes as u32);
    true
}

fn direct_blt_build_ring_batch_start(state: DirectBltState) -> Option<usize> {
    unsafe {
        let dwords = core::slice::from_raw_parts_mut(state.ring_virt as *mut u32, 8);
        dwords[0] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_PPGTT;
        dwords[1] = DIRECT_BLT_GPU_VA_BATCH_BASE as u32;
        dwords[2] = (DIRECT_BLT_GPU_VA_BATCH_BASE >> 32) as u32;
        dwords[3] = MI_ARB_CHECK;
        dwords[4] = MI_NOOP;
        dwords[5] = MI_NOOP;
        dwords[6] = MI_NOOP;
        dwords[7] = MI_NOOP;
    }
    let tail_bytes = 4 * core::mem::size_of::<u32>();
    super::dma_flush(state.ring_virt, DIRECT_BLT_RING_BYTES);
    Some(tail_bytes)
}

fn guc_blt_append_ring_batch_start(state: DirectBltState, tail_bytes: usize) -> usize {
    const ENTRY_DWORDS: usize = 4;
    debug_assert_eq!(tail_bytes % (ENTRY_DWORDS * core::mem::size_of::<u32>()), 0);
    debug_assert!(tail_bytes < DIRECT_BLT_RING_BYTES);
    let start = tail_bytes / core::mem::size_of::<u32>();
    unsafe {
        let dwords = state.ring_virt.cast::<u32>();
        core::ptr::write_volatile(dwords.add(start), MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_PPGTT);
        core::ptr::write_volatile(dwords.add(start + 1), DIRECT_BLT_GPU_VA_BATCH_BASE as u32);
        core::ptr::write_volatile(
            dwords.add(start + 2),
            (DIRECT_BLT_GPU_VA_BATCH_BASE >> 32) as u32,
        );
        core::ptr::write_volatile(dwords.add(start + 3), MI_NOOP);
        super::dma_flush(
            state.ring_virt.add(tail_bytes),
            ENTRY_DWORDS * core::mem::size_of::<u32>(),
        );
    }
    (tail_bytes + ENTRY_DWORDS * core::mem::size_of::<u32>()) % DIRECT_BLT_RING_BYTES
}

fn direct_blt_poll_result(state: DirectBltState, expected: u32) -> u32 {
    let mut observed = 0;
    for _ in 0..DIRECT_BLT_SMOKE_POLL_ITERS {
        super::dma_flush(state.result_virt, DIRECT_BLT_RESULT_BYTES);
        observed = unsafe { core::ptr::read_volatile(state.result_virt as *const u32) };
        if observed == expected {
            break;
        }
        core::hint::spin_loop();
    }
    observed
}

fn direct_blt_init_context_image(
    state: DirectBltState,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
) -> bool {
    let total_dwords = DIRECT_BLT_CONTEXT_BYTES / core::mem::size_of::<u32>();
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(state.context_virt as *mut u32, total_dwords) };
    dwords.fill(0);
    let lrc = &mut dwords[DIRECT_BLT_LRC_STATE_OFFSET_DWORDS..];
    if lrc.len() < 192 {
        return false;
    }

    let ring_base = BCS0_RING_BASE as u32;
    let mut idx = 0usize;
    lrc[idx] = MI_NOOP;
    idx += 1;
    lrc[idx] = direct_blt_mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = ring_base + RING_CONTEXT_CONTROL as u32;
    lrc[idx + 1] = direct_blt_ctx_control_value(false);
    lrc[idx + 2] = ring_base + RING_HEAD as u32;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = ring_base + RING_TAIL as u32;
    lrc[idx + 5] = ring_tail;
    lrc[idx + 6] = ring_base + RING_START as u32;
    lrc[idx + 7] = ring_start;
    lrc[idx + 8] = ring_base + RING_CTL as u32;
    lrc[idx + 9] = ring_ctl;
    lrc[idx + 10] = ring_base + 0x168;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = ring_base + 0x140;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = ring_base + 0x110;
    lrc[idx + 15] = 0;
    lrc[idx + 16] = ring_base + 0x1C0;
    lrc[idx + 17] = 0;
    lrc[idx + 18] = ring_base + 0x1C4;
    lrc[idx + 19] = 0;
    lrc[idx + 20] = ring_base + 0x1C8;
    lrc[idx + 21] = 0;
    lrc[idx + 22] = ring_base + 0x180;
    lrc[idx + 23] = 0;
    lrc[idx + 24] = ring_base + 0x2B4;
    lrc[idx + 25] = 0;
    lrc[idx + 26] = ring_base + 0x5A8;
    lrc[idx + 27] = 0;
    lrc[idx + 28] = ring_base + 0x5AC;
    lrc[idx + 29] = 0;
    idx += 30;
    direct_blt_push_nops(lrc, &mut idx, 5);

    lrc[idx] = direct_blt_mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    for (offset, value) in [
        (0x3A8u32, 0),
        (0x28C, 0),
        (0x288, 0),
        (0x284, 0),
        (0x280, 0),
        (0x27C, 0),
        (0x278, 0),
        (0x274, (state.ppgtt_phys >> 32) as u32),
        (0x270, state.ppgtt_phys as u32),
    ] {
        lrc[idx] = ring_base + offset;
        lrc[idx + 1] = value;
        idx += 2;
    }

    lrc[idx] = direct_blt_mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = ring_base + 0x1B0;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = ring_base + 0x5A8;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = ring_base + 0x5AC;
    lrc[idx + 5] = 0;
    idx += 6;
    direct_blt_push_nops(lrc, &mut idx, 6);

    lrc[idx] = direct_blt_mi_lri_cmd(1, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = ring_base + 0xC8;
    lrc[idx + 1] = 0x7FFF_FFFF;
    idx += 2;
    direct_blt_push_nops(lrc, &mut idx, 13);

    lrc[idx] = direct_blt_mi_lri_cmd(4, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = ring_base + 0x28;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = ring_base + RING_MI_MODE as u32;
    lrc[idx + 3] = direct_blt_masked_bit_disable(STOP_RING);
    lrc[idx + 4] = ring_base + RING_IPEHR as u32;
    lrc[idx + 5] = 0;
    lrc[idx + 6] = ring_base + 0x84;
    lrc[idx + 7] = 0;
    idx += 8;
    direct_blt_push_nops(lrc, &mut idx, 8);

    const CTX_RING_TAIL_DW: usize = 7;
    const CTX_RING_START_DW: usize = 9;
    const CTX_RING_CTL_DW: usize = 11;
    lrc[CTX_RING_TAIL_DW] = ring_tail;
    lrc[CTX_RING_START_DW] = ring_start;
    lrc[CTX_RING_CTL_DW] = ring_ctl;
    lrc[idx] = MI_BATCH_BUFFER_END | 1;

    super::dma_flush(state.context_virt, DIRECT_BLT_CONTEXT_BYTES);
    true
}

fn guc_blt_write_lrc_ring_tail(state: DirectBltState, ring_tail: u32) {
    const LRC_RING_TAIL_VALUE_DW: usize = 7;
    let total_dwords = DIRECT_BLT_CONTEXT_BYTES / core::mem::size_of::<u32>();
    if total_dwords <= DIRECT_BLT_LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW {
        return;
    }
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(state.context_virt.cast::<u32>(), total_dwords) };
    dwords[DIRECT_BLT_LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW] = ring_tail;
    super::dma_flush(state.context_virt, DIRECT_BLT_CONTEXT_BYTES);
}

fn direct_blt_init_csb_pointers(dev: super::Dev, hwsp_virt: *mut u8) {
    const GEN12_HWSP_CSB_WRITE_OFFSET: usize = 0xBC;
    const GEN12_CSB_RESET_VALUE: u32 = 11;
    const GEN12_HWSP_CSB_BUF0_OFFSET: usize = 0x40;
    const GEN12_CSB_ENTRIES: usize = 12;
    let csb_init: u32 = 0xFFFF_0000 | (GEN12_CSB_RESET_VALUE << 8) | GEN12_CSB_RESET_VALUE;
    super::mmio_write(dev, BCS0_RING_BASE + 0x3A0, csb_init);
    let _ = super::mmio_read(dev, BCS0_RING_BASE + 0x3A0);
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
    super::mmio_write(dev, BCS0_RING_BASE + 0x3A0, csb_init);
    let _ = super::mmio_read(dev, BCS0_RING_BASE + 0x3A0);
}

fn direct_blt_wait_idleish(dev: super::Dev) {
    for _ in 0..200_000u32 {
        let el = super::mmio_read(dev, BCS0_RING_BASE + RING_EXECLIST_STATUS_LO);
        if (el >> 30) == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    super::mmio_write(dev, BCS0_RING_BASE + RING_MI_MODE, STOP_RING | (STOP_RING << 16));
    for _ in 0..50_000u32 {
        if super::mmio_read(dev, BCS0_RING_BASE + RING_MI_MODE) & MODE_IDLE != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    super::mmio_write(dev, BCS0_RING_BASE + RING_MI_MODE, STOP_RING << 16);
}

fn direct_blt_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let desc = base
        | CTX_DESC_VALID
        | CTX_DESC_PPGTT_ENABLE
        | CTX_DESC_FORCE_RESTORE
        | CTX_DESC_PRIVILEGE
        | CTX_DESC_PRIORITY_NORMAL
        | (INTEL_LEGACY_64B_CONTEXT << CTX_DESC_ADDRESSING_MODE_SHIFT);
    let submit_id = DIRECT_BLT_SUBMIT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let base_context_id = (((context_gpu_addr >> 12) as u32) & 0x3FF).max(1);
    let sw_context_id = (((submit_id & 0x3FF) << 1) ^ base_context_id).max(1) & 0x7FF;
    let desc_hi = ((context_gpu_addr >> 32) as u32) | (sw_context_id << 7);
    (desc, desc_hi)
}

fn direct_blt_execlist_submit_port_push(
    dev: super::Dev,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    super::mmio_write(dev, BCS0_RING_BASE + RING_EXECLIST_SQ_LO, context0_lo);
    super::mmio_write(dev, BCS0_RING_BASE + RING_EXECLIST_SQ_HI, context0_hi);
    super::mmio_write(dev, BCS0_RING_BASE + RING_EXECLIST_SQ_LO + 8, context1_lo);
    super::mmio_write(dev, BCS0_RING_BASE + RING_EXECLIST_SQ_HI + 8, context1_hi);
}

fn direct_blt_ring_ctl_value(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | RING_VALID)
}

fn direct_blt_ctx_control_value(inhibit_restore: bool) -> u32 {
    let mut ctl = direct_blt_masked_bits_update(
        CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT,
    );
    if inhibit_restore {
        ctl |= CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT;
    }
    ctl
}

fn direct_blt_mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | num_regs.saturating_mul(2).saturating_sub(1)
}

fn direct_blt_push_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}

fn direct_blt_masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

fn direct_blt_masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

fn direct_blt_masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

fn direct_blt_now_tick() -> u64 {
    embassy_time_driver::now()
}

fn direct_blt_elapsed_ms_since(start_tick: u64) -> u64 {
    let elapsed = direct_blt_now_tick().saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        elapsed.saturating_mul(1000) / hz
    }
}
