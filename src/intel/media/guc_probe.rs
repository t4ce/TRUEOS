//! One-shot GuC/VCS0 transport proof.
//!
//! This deliberately contains no codec packets. It proves that TRUEOS can
//! register a VCS0 LRC with GuC, schedule it, observe ordered PPGTT marker
//! writes, and safely tear the context down. AVC encode remains gated until a
//! separate patch supplies the complete VDEnc/MFX resource and command graph.

use core::sync::atomic::{AtomicU8, Ordering};

use spin::Mutex;

use super::engine as media;

const PROBE_RING_GPU: u64 = 0x1100_0000;
const PROBE_CONTEXT_GPU: u64 = 0x1101_0000;
const PROBE_BATCH_GPU: u64 = 0x1108_0000;
const PROBE_RESULT_GPU: u64 = 0x1110_0000;
const PROBE_RING_BYTES: usize = 16 * 1024;
const PROBE_CONTEXT_BYTES: usize = 22 * 4096;
const PROBE_BATCH_BYTES: usize = 4096;
const PROBE_RESULT_BYTES: usize = 4096;
const PROBE_TIMEOUT_NS: u64 = 50_000_000;
const PROBE_POLL_LIMIT: u32 = 1_000_000;

const KICKOFF_MARKER: u32 = 0x5643_5001;
const PRESUBMIT_MARKER: u32 = 0x5643_5002;
const POSTSUBMIT_MARKER: u32 = 0x5643_5003;
const COMPLETE_MARKER: u32 = 0x5643_5004;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum GucVcs0ProbeState {
    NotRun = 0,
    Deferred = 1,
    Preparing = 2,
    Submitted = 3,
    Passed = 4,
    Failed = 5,
    Quarantined = 6,
}

impl GucVcs0ProbeState {
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
pub(crate) enum GucVcs0ProbeFailure {
    None,
    DeviceUnavailable,
    Vcs0Unavailable,
    GucTransportUnavailable,
    LaneBusy,
    LaneQuarantined,
    ForcewakeUnavailable,
    BackingAllocation,
    BatchBuild,
    ContextBuild,
    RegisterRejected,
    SubmitRejected,
    CompletionTimeout,
    MarkerMismatch,
    ContextTeardown,
}

impl GucVcs0ProbeFailure {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DeviceUnavailable => "device-unavailable",
            Self::Vcs0Unavailable => "vcs0-unavailable",
            Self::GucTransportUnavailable => "guc-transport-unavailable",
            Self::LaneBusy => "vcs0-lane-busy",
            Self::LaneQuarantined => "vcs0-lane-quarantined",
            Self::ForcewakeUnavailable => "vcs0-forcewake-unavailable",
            Self::BackingAllocation => "probe-backing-allocation",
            Self::BatchBuild => "marker-batch-build",
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
pub(crate) struct GucVcs0ProbeReport {
    pub(crate) state: GucVcs0ProbeState,
    pub(crate) failure: GucVcs0ProbeFailure,
    pub(crate) forcewake: bool,
    pub(crate) backing_ready: bool,
    pub(crate) batch_ready: bool,
    pub(crate) context_ready: bool,
    pub(crate) registered: bool,
    pub(crate) submitted: bool,
    pub(crate) retired: bool,
    pub(crate) context_destroyed: bool,
    pub(crate) serial: u64,
    pub(crate) hwlrca_lo: u32,
    pub(crate) hwlrca_hi: u32,
    pub(crate) kickoff: u32,
    pub(crate) presubmit: u32,
    pub(crate) postsubmit: u32,
    pub(crate) complete: u32,
    pub(crate) poll_iters: u32,
    pub(crate) elapsed_us: u64,
}

impl GucVcs0ProbeReport {
    const EMPTY: Self = Self {
        state: GucVcs0ProbeState::NotRun,
        failure: GucVcs0ProbeFailure::None,
        forcewake: false,
        backing_ready: false,
        batch_ready: false,
        context_ready: false,
        registered: false,
        submitted: false,
        retired: false,
        context_destroyed: false,
        serial: 0,
        hwlrca_lo: 0,
        hwlrca_hi: 0,
        kickoff: 0,
        presubmit: 0,
        postsubmit: 0,
        complete: 0,
        poll_iters: 0,
        elapsed_us: 0,
    };
}

struct ProbeBacking {
    ring_virt: *mut u8,
    context_virt: *mut u8,
    batch_virt: *mut u8,
    result_virt: *mut u8,
    ppgtt: crate::intel::ppgtt::SparsePpgtt,
}

unsafe impl Send for ProbeBacking {}

static STATE: AtomicU8 = AtomicU8::new(GucVcs0ProbeState::NotRun as u8);
static REPORT: Mutex<GucVcs0ProbeReport> = Mutex::new(GucVcs0ProbeReport::EMPTY);
static BACKING: Mutex<Option<ProbeBacking>> = Mutex::new(None);

pub(crate) fn passed() -> bool {
    STATE.load(Ordering::Acquire) == GucVcs0ProbeState::Passed as u8
}

pub(crate) fn snapshot() -> GucVcs0ProbeReport {
    *REPORT.lock()
}

/// Run the first real GuC-owned VCS0 workload. Deferred results are retryable;
/// once preparation starts, every outcome is terminal for this boot.
pub(crate) fn run_once() -> GucVcs0ProbeReport {
    let current = GucVcs0ProbeState::from_raw(STATE.load(Ordering::Acquire));
    if current != GucVcs0ProbeState::NotRun {
        return snapshot();
    }

    let Some(dev) = crate::intel::claimed_device() else {
        return deferred(GucVcs0ProbeFailure::DeviceUnavailable);
    };
    let (engine, _) = media::default_decode_engine_and_window();
    if engine.id.instance != 0 || !engine.capabilities.decode {
        return deferred(GucVcs0ProbeFailure::Vcs0Unavailable);
    }
    if !crate::intel::guc_submission::INTEL_GUC_SCHEDULER.ready() {
        return deferred(GucVcs0ProbeFailure::GucTransportUnavailable);
    }

    let lane = match media::try_acquire_vcs0_lane() {
        Ok(lane) => lane,
        Err(media::MediaVcs0LaneAcquireError::Busy) => {
            if STATE.load(Ordering::Acquire) != GucVcs0ProbeState::NotRun as u8 {
                return snapshot();
            }
            return deferred(GucVcs0ProbeFailure::LaneBusy);
        }
        Err(media::MediaVcs0LaneAcquireError::Quarantined) => {
            if STATE.load(Ordering::Acquire) != GucVcs0ProbeState::NotRun as u8 {
                return snapshot();
            }
            return deferred(GucVcs0ProbeFailure::LaneQuarantined);
        }
    };
    if STATE
        .compare_exchange(
            GucVcs0ProbeState::NotRun as u8,
            GucVcs0ProbeState::Preparing as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return snapshot();
    }

    let started_ns = crate::chronos::monotonic_nanos();
    let mut report = GucVcs0ProbeReport {
        state: GucVcs0ProbeState::Preparing,
        ..GucVcs0ProbeReport::EMPTY
    };
    publish(report);

    report.forcewake = media::wake_media_engine_for_guc(dev, engine);
    if !report.forcewake {
        return fail(report, GucVcs0ProbeFailure::ForcewakeUnavailable, started_ns);
    }

    let mut backing_slot = BACKING.lock();
    if backing_slot.is_none() {
        *backing_slot = build_backing(dev);
    }
    let Some(backing) = backing_slot.as_ref() else {
        return fail(report, GucVcs0ProbeFailure::BackingAllocation, started_ns);
    };
    report.backing_ready = true;

    unsafe {
        core::ptr::write_bytes(backing.ring_virt, 0, PROBE_RING_BYTES);
        core::ptr::write_bytes(backing.context_virt, 0, PROBE_CONTEXT_BYTES);
        core::ptr::write_bytes(backing.batch_virt, 0, PROBE_BATCH_BYTES);
        core::ptr::write_bytes(backing.result_virt, 0, PROBE_RESULT_BYTES);
    }
    let Some(batch_bytes) = build_marker_batch(backing.batch_virt) else {
        return fail(report, GucVcs0ProbeFailure::BatchBuild, started_ns);
    };
    report.batch_ready = true;

    let Some(ring_tail_bytes) = media::build_ring_batch_start_words(
        backing.ring_virt,
        PROBE_RING_BYTES,
        0,
        PROBE_RESULT_GPU,
        KICKOFF_MARKER,
        PROBE_BATCH_GPU,
    ) else {
        return fail(report, GucVcs0ProbeFailure::ContextBuild, started_ns);
    };
    let Some(ring_ctl) = media::ring_ctl_value_for_size(PROBE_RING_BYTES) else {
        return fail(report, GucVcs0ProbeFailure::ContextBuild, started_ns);
    };
    if !media::init_gen12_video_context_image(
        backing.context_virt,
        PROBE_CONTEXT_BYTES,
        engine.ring_base,
        0,
        PROBE_RING_GPU as u32,
        ring_tail_bytes as u32,
        ring_ctl,
        PROBE_CONTEXT_GPU as u32,
        backing.ppgtt.pml4_phys(),
        false,
    ) {
        return fail(report, GucVcs0ProbeFailure::ContextBuild, started_ns);
    }
    report.context_ready = true;

    crate::intel::dma_flush(backing.batch_virt, batch_bytes);
    crate::intel::dma_flush(backing.ring_virt, ring_tail_bytes);
    crate::intel::dma_flush(backing.context_virt, PROBE_CONTEXT_BYTES);
    crate::intel::dma_flush(backing.result_virt, PROBE_RESULT_BYTES);
    crate::intel::ggtt_invalidate(dev);
    core::sync::atomic::fence(Ordering::SeqCst);

    let (hwlrca_lo, hwlrca_hi) = media::build_media_guc_context_descriptor(PROBE_CONTEXT_GPU);
    report.hwlrca_lo = hwlrca_lo;
    report.hwlrca_hi = hwlrca_hi;
    let token = match crate::intel::guc_submission::INTEL_GUC_SCHEDULER.register(
        dev,
        crate::gpu::physical::EngineClass::VideoDecode,
        hwlrca_lo,
        hwlrca_hi,
    ) {
        Ok(token) => token,
        Err(_) => {
            return fail(report, GucVcs0ProbeFailure::RegisterRejected, started_ns);
        }
    };
    report.registered = true;

    let submission = match crate::intel::guc_submission::INTEL_GUC_SCHEDULER.submit(dev, token) {
        Ok(submission) => submission,
        Err(_) => {
            report.context_destroyed = crate::intel::guc_submission::INTEL_GUC_SCHEDULER
                .destroy(dev, token)
                .is_ok();
            if !report.context_destroyed {
                return quarantine(lane, report, GucVcs0ProbeFailure::ContextTeardown, started_ns);
            }
            return fail(report, GucVcs0ProbeFailure::SubmitRejected, started_ns);
        }
    };
    report.state = GucVcs0ProbeState::Submitted;
    report.submitted = true;
    report.serial = submission.serial;
    publish(report);

    let deadline = started_ns.saturating_add(PROBE_TIMEOUT_NS);
    while report.poll_iters < PROBE_POLL_LIMIT {
        crate::intel::dma_flush(backing.result_virt, 4 * 8);
        report.complete =
            media::read_result_dword(backing.result_virt, media::MEDIA_RESULT_COMPLETE_SLOT);
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

    crate::intel::dma_flush(backing.result_virt, 4 * 8);
    report.kickoff =
        media::read_result_dword(backing.result_virt, media::MEDIA_RESULT_KICKOFF_SLOT);
    report.presubmit =
        media::read_result_dword(backing.result_virt, media::MEDIA_RESULT_PRESUBMIT_SLOT);
    report.postsubmit =
        media::read_result_dword(backing.result_virt, media::MEDIA_RESULT_POSTSUBMIT_SLOT);
    report.complete =
        media::read_result_dword(backing.result_virt, media::MEDIA_RESULT_COMPLETE_SLOT);
    if !report.retired {
        return quarantine(lane, report, GucVcs0ProbeFailure::CompletionTimeout, started_ns);
    }

    report.context_destroyed = crate::intel::guc_submission::INTEL_GUC_SCHEDULER
        .destroy(dev, token)
        .is_ok();
    if !report.context_destroyed {
        return quarantine(lane, report, GucVcs0ProbeFailure::ContextTeardown, started_ns);
    }

    if report.kickoff != KICKOFF_MARKER
        || report.presubmit != PRESUBMIT_MARKER
        || report.postsubmit != POSTSUBMIT_MARKER
        || report.complete != COMPLETE_MARKER
    {
        return fail(report, GucVcs0ProbeFailure::MarkerMismatch, started_ns);
    }

    report.state = GucVcs0ProbeState::Passed;
    report.failure = GucVcs0ProbeFailure::None;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn build_backing(dev: crate::intel::Dev) -> Option<ProbeBacking> {
    let (ring_phys, ring_virt) = crate::dma::alloc(PROBE_RING_BYTES, crate::intel::WARM_ALIGN)?;
    let (context_phys, context_virt) =
        crate::dma::alloc(PROBE_CONTEXT_BYTES, crate::intel::WARM_ALIGN)?;
    let (batch_phys, batch_virt) = crate::dma::alloc(PROBE_BATCH_BYTES, crate::intel::WARM_ALIGN)?;
    let (result_phys, result_virt) =
        crate::dma::alloc(PROBE_RESULT_BYTES, crate::intel::WARM_ALIGN)?;

    if !crate::intel::map_ggtt(dev, ring_phys, PROBE_RING_BYTES, PROBE_RING_GPU)
        || !crate::intel::map_ggtt(dev, context_phys, PROBE_CONTEXT_BYTES, PROBE_CONTEXT_GPU)
        || !crate::intel::map_ggtt(dev, batch_phys, PROBE_BATCH_BYTES, PROBE_BATCH_GPU)
        || !crate::intel::map_ggtt(dev, result_phys, PROBE_RESULT_BYTES, PROBE_RESULT_GPU)
    {
        return None;
    }
    crate::intel::ggtt_invalidate(dev);
    let ppgtt = crate::intel::ppgtt::build_sparse_ppgtt_for_ranges(&[
        crate::intel::ppgtt::PpgttRange {
            gpu: PROBE_BATCH_GPU,
            phys: batch_phys,
            bytes: PROBE_BATCH_BYTES,
        },
        crate::intel::ppgtt::PpgttRange {
            gpu: PROBE_RESULT_GPU,
            phys: result_phys,
            bytes: PROBE_RESULT_BYTES,
        },
    ])?;
    Some(ProbeBacking {
        ring_virt,
        context_virt,
        batch_virt,
        result_virt,
        ppgtt,
    })
}

fn build_marker_batch(batch_virt: *mut u8) -> Option<usize> {
    let batch = unsafe {
        core::slice::from_raw_parts_mut(
            batch_virt.cast::<u32>(),
            PROBE_BATCH_BYTES / core::mem::size_of::<u32>(),
        )
    };
    let mut idx = 0usize;
    if !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        PROBE_RESULT_GPU + media::MEDIA_RESULT_PRESUBMIT_SLOT,
        PRESUBMIT_MARKER,
    ) || !media::emit_store_dword_ppgtt(
        batch,
        &mut idx,
        PROBE_RESULT_GPU + media::MEDIA_RESULT_POSTSUBMIT_SLOT,
        POSTSUBMIT_MARKER,
    ) {
        return None;
    }
    let flush = media::begin_batch_packet(
        batch,
        &mut idx,
        5,
        media::MI_FLUSH_DW
            | media::MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE
            | media::MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE,
    )?;
    batch[flush + 1] = (PROBE_RESULT_GPU + media::MEDIA_RESULT_COMPLETE_SLOT) as u32;
    batch[flush + 2] = ((PROBE_RESULT_GPU + media::MEDIA_RESULT_COMPLETE_SLOT) >> 32) as u32;
    batch[flush + 3] = COMPLETE_MARKER;
    batch[flush + 4] = 0;
    if idx.saturating_add(3) > batch.len() {
        return None;
    }
    batch[idx] = media::MI_ARB_CHECK;
    batch[idx + 1] = media::MI_BATCH_BUFFER_END;
    batch[idx + 2] = media::MI_NOOP;
    Some((idx + 3) * core::mem::size_of::<u32>())
}

fn deferred(failure: GucVcs0ProbeFailure) -> GucVcs0ProbeReport {
    let report = GucVcs0ProbeReport {
        state: GucVcs0ProbeState::Deferred,
        failure,
        ..GucVcs0ProbeReport::EMPTY
    };
    *REPORT.lock() = report;
    report
}

fn fail(
    mut report: GucVcs0ProbeReport,
    failure: GucVcs0ProbeFailure,
    started_ns: u64,
) -> GucVcs0ProbeReport {
    report.state = GucVcs0ProbeState::Failed;
    report.failure = failure;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn quarantine(
    lane: media::MediaVcs0LaneGuard,
    mut report: GucVcs0ProbeReport,
    failure: GucVcs0ProbeFailure,
    started_ns: u64,
) -> GucVcs0ProbeReport {
    lane.quarantine();
    report.state = GucVcs0ProbeState::Quarantined;
    report.failure = failure;
    report.elapsed_us = elapsed_us(started_ns);
    publish(report);
    report
}

fn publish(report: GucVcs0ProbeReport) {
    *REPORT.lock() = report;
    STATE.store(report.state as u8, Ordering::Release);
}

fn elapsed_us(started_ns: u64) -> u64 {
    crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000
}
