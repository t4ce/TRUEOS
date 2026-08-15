//! TrueOS-Spirit presentation core.
//!
//! Fence 0..3 remain reserved one-to-one for Intel cursor pipe A..D, but the
//! sane initial deployment activates only fence 0. Every activated channel
//! owns a double-buffered hardware surface pair and advances independently;
//! Spirit intentionally has no multi-pipe/gang flip operation.
//!
//! Each frame also owns a two-bit producer latch. CPU and GPGPU releases are
//! upstream eligibility proofs; the selected surface is armed only when every
//! configured bit has arrived. Intel CUR_SURFLIVE is the separate downstream
//! proof that completes the public Spirit fence.

use core::ops::BitOr;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

pub(crate) mod dobby_ui;
pub(crate) mod gpu_logger;
mod intel_cursor;
mod lilly;
mod lilly_cursor;
pub(crate) mod lilly_protocol;
mod response_window;
#[allow(dead_code)]
#[path = "Spirit_VFX.rs"]
pub(crate) mod spirit_vfx;
mod window_selection;

#[allow(unused_imports)]
pub(crate) use lilly_protocol::{LillyEmotion, enqueue_emotion_words, enqueue_emotions};
pub(crate) use response_window::{enqueue_reasoning_response, spirit_response_window_service_task};
pub(crate) use window_selection::spirit_window_selection_task;

/// Architectural pipe/fence capacity kept for later activation.
pub(crate) const SPIRIT_FENCE_COUNT: usize = 4;
/// Initial Embassy pool and runtime activation limit.
pub(crate) const SPIRIT_WORKER_POOL_LIMIT: usize = 1;
const _: () = assert!(SPIRIT_WORKER_POOL_LIMIT > 0);
const _: () = assert!(SPIRIT_WORKER_POOL_LIMIT <= SPIRIT_FENCE_COUNT);

const SPIRIT_IDLE_POLL_MS: u64 = 16;
const SPIRIT_FLIP_POLL_MS: u64 = 1;
/// A cursor base update should become SURFLIVE on the next vblank. Give it
/// roughly six 60 Hz vblanks before treating the missing latch as a Spirit
/// application fault.
const SPIRIT_SURFLIVE_TIMEOUT_MS: u64 = 100;
const SPIRIT_SURFLIVE_MAX_RETRIES: u8 = 1;
const SPIRIT_GPU_POLL_MS: u64 = 1;
const SPIRIT_CURSOR_MOVE_RETRY_MS: u64 = 1;
const SPIRIT_MOVE_PORTAL_RAMP_MS: u64 = 1_000;
const SPIRIT_MOVE_PORTAL_HOLD_MS: u64 = 1_000;
const SPIRIT_MOVE_PORTAL_PRE_MS: u64 = SPIRIT_MOVE_PORTAL_RAMP_MS;
const SPIRIT_MOVE_PORTAL_POST_MS: u64 = SPIRIT_MOVE_PORTAL_HOLD_MS + SPIRIT_MOVE_PORTAL_RAMP_MS;
const SPIRIT_MOVE_PORTAL_TOTAL_MS: u64 = SPIRIT_MOVE_PORTAL_PRE_MS + SPIRIT_MOVE_PORTAL_POST_MS;
const _: () = assert!(SPIRIT_MOVE_PORTAL_TOTAL_MS == 3_000);
const SPIRIT_BOOT_MOVE_RIGHT_PIXELS: u32 = 512;
const SPIRIT_RETRY_MS: u64 = 50;
const SPIRIT_VFX_INITIAL_TRACE_FRAMES: u64 = 30;
const SPIRIT_VFX_PERIODIC_TRACE_FRAMES: u64 = 60;
const SPIRIT_VFX_TARGET_HZ: u64 = 60;
const SPIRIT_GPU_LOGGER_TARGET_HZ: u64 = 4;
const SPIRIT_PRESENT_FPS_WINDOW_MS: u64 = 500;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpiritFenceId(u8);

impl SpiritFenceId {
    pub(crate) const FENCE_0: Self = Self(0);
    pub(crate) const FENCE_1: Self = Self(1);
    pub(crate) const FENCE_2: Self = Self(2);
    pub(crate) const FENCE_3: Self = Self(3);

    pub(crate) const fn new(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::FENCE_0),
            1 => Some(Self::FENCE_1),
            2 => Some(Self::FENCE_2),
            3 => Some(Self::FENCE_3),
            _ => None,
        }
    }

    pub(crate) const fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn is_active(self) -> bool {
        ACTIVE_FENCE_MASK.load(Ordering::Acquire) & (1 << self.0) != 0
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpiritColor(u32);

#[allow(dead_code)]
impl SpiritColor {
    pub(crate) const STARTUP: Self = Self::rgba(0x52, 0xD6, 0xFF, 0xFF);

    pub(crate) const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        let alpha16 = alpha as u16;
        let premultiplied_red = ((red as u16 * alpha16) / 0xFF) as u8;
        let premultiplied_green = ((green as u16 * alpha16) / 0xFF) as u8;
        let premultiplied_blue = ((blue as u16 * alpha16) / 0xFF) as u8;
        Self(u32::from_le_bytes([
            premultiplied_blue,
            premultiplied_green,
            premultiplied_red,
            alpha,
        ]))
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct SpiritCommandStream {
    pub(crate) color: SpiritColor,
    pub(crate) x_normalized: f64,
    pub(crate) y_normalized: f64,
}

#[allow(dead_code)]
impl SpiritCommandStream {
    pub(crate) const fn centered(color: SpiritColor) -> Self {
        Self {
            color,
            x_normalized: 0.5,
            y_normalized: 0.5,
        }
    }
}

/// Per-frame producer bit set. This is a one-shot two-party latch, rather
/// than a task rendezvous barrier: hardware cannot enter a Rust `Barrier`.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpiritBarrierSet(u8);

#[allow(dead_code)]
impl SpiritBarrierSet {
    const CPU_BIT: u8 = 1 << 0;
    const GPU_BIT: u8 = 1 << 1;
    const VALID_BITS: u8 = Self::CPU_BIT | Self::GPU_BIT;

    pub(crate) const NONE: Self = Self(0);
    pub(crate) const CPU: Self = Self(Self::CPU_BIT);
    pub(crate) const GPU: Self = Self(Self::GPU_BIT);
    pub(crate) const CPU_AND_GPU: Self = Self(Self::VALID_BITS);

    pub(crate) const fn contains(self, producer: Self) -> bool {
        self.0 & producer.0 == producer.0
    }

    const fn satisfied_by(self, released: Self) -> bool {
        released.0 & self.0 == self.0
    }
}

impl BitOr for SpiritBarrierSet {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self((self.0 | rhs.0) & Self::VALID_BITS)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpiritFence {
    pub(crate) id: SpiritFenceId,
    pub(crate) sequence: u64,
}

#[allow(dead_code)]
impl SpiritFence {
    pub(crate) fn is_complete(self) -> bool {
        COMPLETED[self.id.index()].load(Ordering::Acquire) >= self.sequence
    }

    pub(crate) async fn wait(self) {
        while !self.is_complete() {
            Timer::after(Duration::from_millis(SPIRIT_FLIP_POLL_MS)).await;
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpiritSurfaceLayout {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) byte_len: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpiritFrameLease {
    fence: SpiritFence,
    surface: intel_cursor::SpiritCursorSurfaceAccess,
}

#[allow(dead_code)]
impl SpiritFrameLease {
    pub(crate) const fn fence(self) -> SpiritFence {
        self.fence
    }

    pub(crate) const fn layout(self) -> SpiritSurfaceLayout {
        SpiritSurfaceLayout {
            width: self.surface.width,
            height: self.surface.height,
            pitch_bytes: self.surface.pitch_bytes,
            byte_len: self.surface.byte_len,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SpiritSubmitError {
    InactiveFence,
    Busy,
    InvalidCommand,
    HardwareNotReady,
    StaleLease,
    ProducerNotRequired,
    ProducerAlreadyReleased,
    InvalidGpuRelease,
    GpuSubmissionFailed,
}

#[derive(Copy, Clone)]
struct QueuedFrame {
    lease: SpiritFrameLease,
    required: SpiritBarrierSet,
    released: SpiritBarrierSet,
    producer: SpiritFrameProducer,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SpiritFrameProducer {
    External,
    Vfx,
    GpuLogger(u64),
}

impl QueuedFrame {
    const fn is_ready(self) -> bool {
        self.required.satisfied_by(self.released)
    }
}

struct SpiritMailbox {
    next_sequence: u64,
    pending: Option<QueuedFrame>,
    rearming: bool,
    last_gpgpu_release_sequence: [u64; 2],
}

#[derive(Copy, Clone)]
struct SpiritPosition {
    x_normalized: f64,
    y_normalized: f64,
}

#[derive(Copy, Clone)]
struct SpiritMoveRequest {
    position: SpiritPosition,
    sequence: u64,
    portal_transition: bool,
}

impl SpiritMoveRequest {
    const CENTERED: Self = Self {
        position: SpiritPosition::CENTERED,
        sequence: 0,
        portal_transition: false,
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpiritMoveFence {
    pub(crate) id: SpiritFenceId,
    pub(crate) sequence: u64,
}

#[allow(dead_code)]
impl SpiritMoveFence {
    pub(crate) fn is_complete(self) -> bool {
        MOVE_APPLIED[self.id.index()].load(Ordering::Acquire) >= self.sequence
    }

    pub(crate) async fn wait(self) {
        while !self.is_complete() {
            Timer::after(Duration::from_millis(SPIRIT_CURSOR_MOVE_RETRY_MS)).await;
        }
    }
}

impl SpiritPosition {
    const CENTERED: Self = Self {
        x_normalized: 0.5,
        y_normalized: 0.5,
    };

    const fn cursor_frame(self) -> intel_cursor::SpiritCursorFrame {
        intel_cursor::SpiritCursorFrame {
            x_normalized: self.x_normalized,
            y_normalized: self.y_normalized,
        }
    }
}

impl SpiritMailbox {
    const fn new() -> Self {
        Self {
            next_sequence: 0,
            pending: None,
            rearming: false,
            last_gpgpu_release_sequence: [0; 2],
        }
    }
}

static MAILBOXES: [Mutex<SpiritMailbox>; SPIRIT_FENCE_COUNT] = [
    Mutex::new(SpiritMailbox::new()),
    Mutex::new(SpiritMailbox::new()),
    Mutex::new(SpiritMailbox::new()),
    Mutex::new(SpiritMailbox::new()),
];
static MOVE_STATES: [Mutex<SpiritMoveRequest>; SPIRIT_FENCE_COUNT] = [
    Mutex::new(SpiritMoveRequest::CENTERED),
    Mutex::new(SpiritMoveRequest::CENTERED),
    Mutex::new(SpiritMoveRequest::CENTERED),
    Mutex::new(SpiritMoveRequest::CENTERED),
];
static MOVE_SIGNALS: [Signal<crate::wait::EmbassySpinRawMutex, SpiritMoveRequest>;
    SPIRIT_FENCE_COUNT] = [Signal::new(), Signal::new(), Signal::new(), Signal::new()];
static MOVE_APPLIED: [AtomicU64; SPIRIT_FENCE_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static COMPLETED: [AtomicU64; SPIRIT_FENCE_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static ACTIVE_FENCE_MASK: AtomicU8 = AtomicU8::new(0);
const SPIRIT_WORKER_FENCE_UNBOUND: u8 = u8::MAX;
static WORKER_FENCE_BINDINGS: [AtomicU8; SPIRIT_FENCE_COUNT] = [
    AtomicU8::new(SPIRIT_WORKER_FENCE_UNBOUND),
    AtomicU8::new(SPIRIT_WORKER_FENCE_UNBOUND),
    AtomicU8::new(SPIRIT_WORKER_FENCE_UNBOUND),
    AtomicU8::new(SPIRIT_WORKER_FENCE_UNBOUND),
];

/// Reserve the non-visible member of one Spirit double buffer.
///
/// `NONE` is immediately eligible and therefore only suits an already-authored
/// allocation. CPU/GPU producers must request their corresponding bit before
/// touching the returned lease. If both producers touch overlapping memory,
/// the caller must serialize them; this latch controls presentation readiness,
/// not write ordering between engines.
#[allow(dead_code)]
pub(crate) fn acquire_frame(
    id: SpiritFenceId,
    required: SpiritBarrierSet,
) -> Result<SpiritFrameLease, SpiritSubmitError> {
    acquire_frame_for(id, required, SpiritFrameProducer::External)
}

fn acquire_frame_for(
    id: SpiritFenceId,
    required: SpiritBarrierSet,
    producer: SpiritFrameProducer,
) -> Result<SpiritFrameLease, SpiritSubmitError> {
    if !id.is_active() {
        return Err(SpiritSubmitError::InactiveFence);
    }

    let mut mailbox = MAILBOXES[id.index()].lock();
    if mailbox.pending.is_some() || mailbox.rearming {
        return Err(SpiritSubmitError::Busy);
    }
    let surface = intel_cursor::spirit_cursor_back_surface(id.0).map_err(map_cursor_error)?;
    mailbox.next_sequence = mailbox.next_sequence.saturating_add(1).max(1);
    let lease = SpiritFrameLease {
        fence: SpiritFence {
            id,
            sequence: mailbox.next_sequence,
        },
        surface,
    };
    mailbox.pending = Some(QueuedFrame {
        lease,
        required,
        released: SpiritBarrierSet::NONE,
        producer,
    });
    Ok(lease)
}

/// Borrow the exact CPU mapping while its CPU release bit is still locked.
/// The pre-flush invalidates stale cache lines, making GPU-then-CPU stamping a
/// valid ordered use when the caller waits for the GPU before entering here.
#[allow(dead_code)]
pub(crate) fn write_cpu<R>(
    lease: SpiritFrameLease,
    write: impl FnOnce(&mut [u8], SpiritSurfaceLayout) -> R,
) -> Result<R, SpiritSubmitError> {
    let access = {
        let mailbox = MAILBOXES[lease.fence.id.index()].lock();
        let pending = matching_pending(&mailbox, lease)?;
        require_unreleased(pending, SpiritBarrierSet::CPU)?;
        pending.lease.surface
    };
    intel_cursor::spirit_cursor_flush_cpu(access).map_err(map_cursor_error)?;
    let bytes = unsafe { core::slice::from_raw_parts_mut(access.virt, access.byte_len) };
    Ok(write(bytes, lease.layout()))
}

/// Return the exact cursor allocation in its actual hardware byte order.
///
/// The `gpu` field is its permanent display alias; Spirit's execution PPGTT
/// maps the same physical pages for GPGPU access. Spirit VFX writes
/// premultiplied B,G,R,A bytes explicitly, so colored layers do not rely on
/// the grayscale-only accident of the original Lab256 producer.
fn gpgpu_bgra_target(
    lease: SpiritFrameLease,
) -> Result<crate::intel::gpgpu::GpgpuRgba8Surface, SpiritSubmitError> {
    let mailbox = MAILBOXES[lease.fence.id.index()].lock();
    let pending = matching_pending(&mailbox, lease)?;
    require_unreleased(pending, SpiritBarrierSet::GPU)?;
    crate::intel::gpgpu::GpgpuRgba8Surface::new_bgra(
        lease.surface.phys,
        lease.surface.cursor_gpu,
        lease.surface.byte_len,
        lease.surface.width,
        lease.surface.height,
        lease.surface.pitch_bytes,
    )
    .ok_or(SpiritSubmitError::HardwareNotReady)
}

/// Publish all CPU writes and release the CPU producer bit.
#[allow(dead_code)]
pub(crate) fn release_cpu(lease: SpiritFrameLease) -> Result<(), SpiritSubmitError> {
    {
        let mailbox = MAILBOXES[lease.fence.id.index()].lock();
        let pending = matching_pending(&mailbox, lease)?;
        require_unreleased(pending, SpiritBarrierSet::CPU)?;
    }
    intel_cursor::spirit_cursor_flush_cpu(lease.surface).map_err(map_cursor_error)?;
    let mut mailbox = MAILBOXES[lease.fence.id.index()].lock();
    let pending = matching_pending_mut(&mut mailbox, lease)?;
    require_unreleased(pending, SpiritBarrierSet::CPU)?;
    pending.released = pending.released | SpiritBarrierSet::CPU;
    Ok(())
}

/// Release the GPU producer bit with Spirit's GPGPU lane exact-allocation
/// completion proof. This remains distinct from CUR_SURFLIVE: the former makes
/// a frame eligible to arm, while the latter retires the public Spirit fence.
fn release_gpgpu(
    lease: SpiritFrameLease,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
) -> Result<(), SpiritSubmitError> {
    if !release.matches(lease.surface.phys, lease.surface.byte_len) {
        return Err(SpiritSubmitError::InvalidGpuRelease);
    }
    let mut mailbox = MAILBOXES[lease.fence.id.index()].lock();
    let surface_index = lease.surface.surface as usize;
    if surface_index >= mailbox.last_gpgpu_release_sequence.len()
        || release.sequence() <= mailbox.last_gpgpu_release_sequence[surface_index]
    {
        return Err(SpiritSubmitError::InvalidGpuRelease);
    }
    let pending = matching_pending_mut(&mut mailbox, lease)?;
    require_unreleased(pending, SpiritBarrierSet::GPU)?;
    pending.released = pending.released | SpiritBarrierSet::GPU;
    mailbox.last_gpgpu_release_sequence[surface_index] = release.sequence();
    Ok(())
}

fn submit_spirit_vfx_frame(
    id: SpiritFenceId,
    present_fps: u32,
    source_frame: lilly::LillyResidentFrame,
) -> Result<GpuInflight, SpiritSubmitError> {
    let lease = acquire_frame_for(id, SpiritBarrierSet::GPU, SpiritFrameProducer::Vfx)?;
    let target = match gpgpu_bgra_target(lease) {
        Ok(target) => target,
        Err(error) => {
            cancel_pending(lease);
            return Err(error);
        }
    };
    let Some(source) = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        source_frame.phys,
        source_frame.gpu,
        source_frame.bytes,
        source_frame.width,
        source_frame.height,
        source_frame.pitch_bytes,
    ) else {
        cancel_pending(lease);
        return Err(SpiritSubmitError::HardwareNotReady);
    };
    let snapshot = spirit_vfx::gpu_snapshot();
    let control = crate::intel::gpgpu::SpiritVfxControl {
        revision: snapshot.revision,
        background_mode: snapshot.background_mode,
        clock_seconds_of_day: snapshot.clock_seconds_of_day,
        background_phase_override: snapshot.background_phase_override,
        background_opacity: snapshot.opacity,
        background_speed: snapshot.speed,
        background_intensity: snapshot.intensity,
        background_color_a: snapshot.color_a,
        background_color_b: snapshot.color_b,
        position_x: snapshot.position_x,
        position_y: snapshot.position_y,
        rotation_radians: snapshot.rotation_radians,
        alpha_cutoff: snapshot.alpha_cutoff,
        edge_fade_pixels: snapshot.edge_fade_pixels,
        sampling: snapshot.sampling,
        shader_mode: snapshot.shader_mode,
        shader_parameters: snapshot.shader_parameters,
        fx_color_a: snapshot.fx_color_a,
        fx_color_b: snapshot.fx_color_b,
    };
    let Some(submission) =
        crate::intel::gpgpu::submit_spirit_vfx_frame(target, source, control, present_fps)
    else {
        cancel_pending(lease);
        return Err(SpiritSubmitError::HardwareNotReady);
    };
    Ok(GpuInflight {
        lease,
        submission,
        polls: 0,
    })
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GpuLoggerFramePath {
    GpuWorklist,
    CpuDirectFallback,
}

#[derive(Copy, Clone, Debug)]
struct GpuLoggerFrameSubmission {
    fence: SpiritFence,
    path: GpuLoggerFramePath,
    rects: usize,
    gpu_submits: usize,
    gpu_submit_ms: u64,
    release_sequence: u64,
}

fn submit_gpu_logger_cpu_fallback(
    id: SpiritFenceId,
    snapshot: gpu_logger::ActiveSnapshot,
) -> Result<GpuLoggerFrameSubmission, SpiritSubmitError> {
    let lease = acquire_frame_for(
        id,
        SpiritBarrierSet::CPU,
        SpiritFrameProducer::GpuLogger(snapshot.generation),
    )?;
    if let Err(error) = write_cpu(lease, |pixels, layout| {
        gpu_logger::render_bgra(pixels, layout, snapshot);
    })
    .and_then(|_| release_cpu(lease))
    {
        cancel_pending(lease);
        return Err(error);
    }
    Ok(GpuLoggerFrameSubmission {
        fence: lease.fence,
        path: GpuLoggerFramePath::CpuDirectFallback,
        rects: 0,
        gpu_submits: 0,
        gpu_submit_ms: 0,
        release_sequence: 0,
    })
}

/// Author one opaque diagnostic panel into Spirit's exact hidden cursor
/// member. The worklist and final cache-release packet both retire before the
/// common mailbox/arm path can expose the allocation. If GPU admission has
/// not crossed the hardware boundary, retain the debug surface's availability
/// with a direct CPU raster fallback; a submitted-incomplete job is never
/// presented as valid.
fn submit_gpu_logger_frame(
    id: SpiritFenceId,
    snapshot: gpu_logger::ActiveSnapshot,
) -> Result<GpuLoggerFrameSubmission, SpiritSubmitError> {
    use crate::intel::gpgpu::GpgpuSubmissionOutcome;

    let lease = acquire_frame_for(
        id,
        SpiritBarrierSet::GPU,
        SpiritFrameProducer::GpuLogger(snapshot.generation),
    )?;
    let target = match gpgpu_bgra_target(lease) {
        Ok(target) => target,
        Err(error) => {
            cancel_pending(lease);
            return Err(error);
        }
    };
    let layers = gpu_logger::build_gpu_rect_layers(snapshot);
    let mut gpu_submits = 0usize;
    let mut gpu_submit_ms = 0u64;
    // Workgroups inside one fill worklist are unordered. Keep intentional
    // overlaps in distinct, fully-retired passes: clear -> disjoint bases ->
    // foreground. This prevents the clear or row cards racing the text.
    for layer in [layers.clear(), layers.bases(), layers.foreground()] {
        let filled = crate::intel::gpgpu::fill_solid_rects_rgba8_scanout_result(target, layer);
        match filled.outcome {
            GpgpuSubmissionOutcome::Complete => {
                gpu_submits = gpu_submits.saturating_add(filled.stats.submits);
                gpu_submit_ms = gpu_submit_ms.saturating_add(filled.stats.submit_ms);
            }
            GpgpuSubmissionOutcome::Unavailable => {
                cancel_pending(lease);
                return submit_gpu_logger_cpu_fallback(id, snapshot);
            }
            GpgpuSubmissionOutcome::SubmittedIncomplete => {
                // The hidden member cannot be reused while a late GPU writer
                // might still target it. Preserve the current visible frame.
                return Err(SpiritSubmitError::GpuSubmissionFailed);
            }
        }
    }
    let finalized = crate::intel::gpgpu::release_rgba8_surface_for_scanout(target);
    if !finalized.ok {
        if finalized.submitted {
            return Err(SpiritSubmitError::GpuSubmissionFailed);
        }
        cancel_pending(lease);
        return submit_gpu_logger_cpu_fallback(id, snapshot);
    }
    let Some(release) = finalized.release else {
        cancel_pending(lease);
        return Err(SpiritSubmitError::InvalidGpuRelease);
    };
    let release_sequence = release.sequence();
    if let Err(error) = release_gpgpu(lease, release) {
        cancel_pending(lease);
        return Err(error);
    }
    Ok(GpuLoggerFrameSubmission {
        fence: lease.fence,
        path: GpuLoggerFramePath::GpuWorklist,
        rects: layers.rects.len(),
        gpu_submits: gpu_submits.saturating_add(1),
        gpu_submit_ms: gpu_submit_ms.saturating_add(finalized.submit_ms),
        release_sequence,
    })
}

/// Queue an absolute Spirit movement without coupling it to either UI4's
/// software cursors or the 60 Hz VFX producer. Repeated calls are latest-wins;
/// the returned fence proves that the dedicated task programmed this request
/// or a newer superseding CUR_POS state.
pub(crate) fn move_to(
    id: SpiritFenceId,
    x_normalized: f64,
    y_normalized: f64,
) -> Result<SpiritMoveFence, SpiritSubmitError> {
    if !x_normalized.is_finite() || !y_normalized.is_finite() {
        return Err(SpiritSubmitError::InvalidCommand);
    }
    Ok(queue_move(
        id,
        SpiritPosition {
            x_normalized: x_normalized.clamp(0.0, 1.0),
            y_normalized: y_normalized.clamp(0.0, 1.0),
        },
    ))
}

/// High-level single-pool helper: move the one active Spirit instance without
/// exposing its current pipe/fence assignment to the caller.
#[allow(dead_code)]
pub(crate) fn move_spirit_to(
    x_normalized: f64,
    y_normalized: f64,
) -> Result<SpiritMoveFence, SpiritSubmitError> {
    move_to(
        active_spirit_fence().ok_or(SpiritSubmitError::InactiveFence)?,
        x_normalized,
        y_normalized,
    )
}

/// Queue a movement relative to Spirit's latest requested position.
#[allow(dead_code)]
pub(crate) fn move_by(
    id: SpiritFenceId,
    delta_x_normalized: f64,
    delta_y_normalized: f64,
) -> Result<SpiritMoveFence, SpiritSubmitError> {
    if !delta_x_normalized.is_finite() || !delta_y_normalized.is_finite() {
        return Err(SpiritSubmitError::InvalidCommand);
    }
    let mut state = MOVE_STATES[id.index()].lock();
    state.position = SpiritPosition {
        x_normalized: (state.position.x_normalized + delta_x_normalized).clamp(0.0, 1.0),
        y_normalized: (state.position.y_normalized + delta_y_normalized).clamp(0.0, 1.0),
    };
    state.sequence = state.sequence.saturating_add(1).max(1);
    state.portal_transition = true;
    let request = *state;
    drop(state);
    Ok(publish_move(id, request))
}

#[allow(dead_code)]
pub(crate) fn move_spirit_by(
    delta_x_normalized: f64,
    delta_y_normalized: f64,
) -> Result<SpiritMoveFence, SpiritSubmitError> {
    move_by(
        active_spirit_fence().ok_or(SpiritSubmitError::InactiveFence)?,
        delta_x_normalized,
        delta_y_normalized,
    )
}

#[allow(dead_code)]
pub(crate) fn set_position(
    id: SpiritFenceId,
    x_normalized: f64,
    y_normalized: f64,
) -> Result<(), SpiritSubmitError> {
    move_to(id, x_normalized, y_normalized).map(|_| ())
}

fn queue_move(id: SpiritFenceId, position: SpiritPosition) -> SpiritMoveFence {
    let mut state = MOVE_STATES[id.index()].lock();
    state.position = position;
    state.sequence = state.sequence.saturating_add(1).max(1);
    state.portal_transition = true;
    let request = *state;
    drop(state);
    publish_move(id, request)
}

fn publish_move(id: SpiritFenceId, request: SpiritMoveRequest) -> SpiritMoveFence {
    let fence = SpiritMoveFence {
        id,
        sequence: request.sequence,
    };
    MOVE_SIGNALS[id.index()].signal(request);
    fence
}

fn resignal_current_move(id: SpiritFenceId) {
    let mut request = *MOVE_STATES[id.index()].lock();
    request.portal_transition = MOVE_APPLIED[id.index()].load(Ordering::Acquire) < request.sequence;
    MOVE_SIGNALS[id.index()].signal(request);
}

fn active_spirit_fence() -> Option<SpiritFenceId> {
    let active = ACTIVE_FENCE_MASK.load(Ordering::Acquire);
    (0..SPIRIT_FENCE_COUNT)
        .find_map(|index| (active & (1 << index) != 0).then(|| SpiritFenceId(index as u8)))
}

/// Current naive convenience path: CPU-author a solid circle and release it.
#[allow(dead_code)]
pub(crate) fn submit(
    id: SpiritFenceId,
    stream: SpiritCommandStream,
) -> Result<SpiritFence, SpiritSubmitError> {
    if !stream.x_normalized.is_finite() || !stream.y_normalized.is_finite() {
        return Err(SpiritSubmitError::InvalidCommand);
    }
    let lease = acquire_frame(id, SpiritBarrierSet::CPU)?;
    queue_move(
        id,
        SpiritPosition {
            x_normalized: stream.x_normalized.clamp(0.0, 1.0),
            y_normalized: stream.y_normalized.clamp(0.0, 1.0),
        },
    );
    if let Err(error) = intel_cursor::spirit_cursor_draw_solid_circle(lease.surface, stream.color.0)
        .map_err(map_cursor_error)
        .and_then(|_| release_cpu(lease))
    {
        cancel_pending(lease);
        return Err(error);
    }
    Ok(lease.fence)
}

#[derive(Copy, Clone)]
struct Inflight {
    flip: intel_cursor::SpiritCursorFlip,
    frame: QueuedFrame,
    completes_fence: bool,
    polls: u32,
    attempt_started: Instant,
    retries: u8,
}

struct GpuInflight {
    lease: SpiritFrameLease,
    submission: crate::intel::gpgpu::SpiritVfxSubmission,
    polls: u32,
}

/// Low-cost display-rate feedback for the shader status dot. Only completed
/// cursor-plane SURFLIVE transitions count; GuC admission and producer-marker
/// retirement do not. Fixed half-second windows are intentional here: this is
/// a presentation health hint, not frame-time instrumentation.
struct SpiritPresentationRate {
    window_started: Option<Instant>,
    visible_frames: u32,
    estimate_fps: u32,
}

impl SpiritPresentationRate {
    const fn new() -> Self {
        Self {
            window_started: None,
            visible_frames: 0,
            estimate_fps: SPIRIT_VFX_TARGET_HZ as u32,
        }
    }

    fn begin(&mut self, now: Instant) {
        self.window_started = Some(now);
        self.visible_frames = 0;
    }

    const fn estimate_fps(&self) -> u32 {
        self.estimate_fps
    }

    fn observe_surflive(&mut self, now: Instant) {
        let Some(started) = self.window_started else {
            self.begin(now);
            return;
        };
        self.visible_frames = self.visible_frames.saturating_add(1);
        let elapsed_ms = now.saturating_duration_since(started).as_millis();
        if elapsed_ms < SPIRIT_PRESENT_FPS_WINDOW_MS {
            return;
        }
        let rounded_fps = (u64::from(self.visible_frames) * 1_000 + elapsed_ms / 2) / elapsed_ms;
        self.estimate_fps = u32::try_from(rounded_fps).unwrap_or(u32::MAX);
        self.window_started = Some(now);
        self.visible_frames = 0;
    }
}

/// Independent Spirit motion executor. It consumes only Spirit's own
/// latest-state signal and is deliberately absent from the VFX frame loop and
/// UI4's software-cursor input/presentation path.
#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn spirit_cursor_task(worker_index: u8) {
    if worker_index as usize >= SPIRIT_WORKER_POOL_LIMIT {
        return;
    }
    let id = bound_spirit_fence(worker_index).await;
    let mut request = MOVE_SIGNALS[id.index()]
        .try_take()
        .unwrap_or_else(|| *MOVE_STATES[id.index()].lock());
    let mut deferred = 0u32;
    let mut lilly_cursor_failures = 0u32;
    if let Err(error) = lilly_cursor::register_once() {
        lilly_cursor_failures = 1;
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: Lilly vcursor registration deferred tag=Spirit/Lilly failures={} error={:?}\n",
            lilly_cursor_failures,
            error,
        );
    }
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: cursor task online worker={} fence={} pipe={} carrier_slot={} expected_carrier_slot={} execution=exclusive-latest-state register_owner=cur-pos frame_loop=decoupled ui4_cursor_path=Spirit/Lilly-vcursor-tagged\n",
        worker_index,
        id.index(),
        pipe_name(id),
        crate::percpu::current_slot(),
        crate::workers::AP1_UI_SERVICE_SLOT,
    );

    loop {
        while let Some(latest) = MOVE_SIGNALS[id.index()].try_take() {
            request = latest;
        }

        // Sequence zero is the boot-time centered state, not a caller-issued
        // move. Every real request gets the fixed portal transition while
        // retaining the API's existing latest-wins destination semantics.
        let portal_transition = request.portal_transition && request.sequence != 0;
        if portal_transition {
            spirit_vfx::set_move_portal_transition(true);
            Timer::after(Duration::from_millis(SPIRIT_MOVE_PORTAL_PRE_MS)).await;
            while let Some(latest) = MOVE_SIGNALS[id.index()].try_take() {
                request = latest;
            }
        }

        let applied = loop {
            while let Some(latest) = MOVE_SIGNALS[id.index()].try_take() {
                request = latest;
            }
            match intel_cursor::spirit_cursor_move(id.0, request.position.cursor_frame()) {
                Ok((x, y)) => {
                    deferred = 0;
                    MOVE_APPLIED[id.index()].store(request.sequence, Ordering::Release);
                    if request.sequence != 0
                        && let Some((screen_width, screen_height)) =
                            crate::intel::complete_scanout_pipeline_dimensions(id.index())
                    {
                        match lilly_cursor::queue_initial_outline_once(
                            x,
                            y,
                            intel_cursor::SPIRIT_CURSOR_DIM,
                            screen_width,
                            screen_height,
                        ) {
                            Ok(_) => lilly_cursor_failures = 0,
                            Err(error) => {
                                lilly_cursor_failures = lilly_cursor_failures.saturating_add(1);
                                if lilly_cursor_failures == 1
                                    || lilly_cursor_failures.is_power_of_two()
                                {
                                    crate::log_warn!(
                                        target: "gfx";
                                        "trueos-spirit: Lilly vcursor outline deferred tag=Spirit/Lilly move_sequence={} failures={} error={:?}\n",
                                        request.sequence,
                                        lilly_cursor_failures,
                                        error,
                                    );
                                }
                            }
                        }
                    }
                    if request.sequence <= 30 || request.sequence.is_multiple_of(600) {
                        crate::log_trace!(
                            target: "gfx";
                            "trueos-spirit: cursor move applied fence={} pipe={} move_sequence={} pos={}x{} normalized={:.5},{:.5} owner=spirit-cursor-task transition={} pre_ms={} post_ms={}\n",
                            id.index(),
                            pipe_name(id),
                            request.sequence,
                            x,
                            y,
                            request.position.x_normalized,
                            request.position.y_normalized,
                            portal_transition,
                            u64::from(portal_transition) * SPIRIT_MOVE_PORTAL_PRE_MS,
                            u64::from(portal_transition) * SPIRIT_MOVE_PORTAL_POST_MS,
                        );
                    }
                    break true;
                }
                Err(
                    error @ (intel_cursor::SpiritCursorError::HardwareNotReady
                    | intel_cursor::SpiritCursorError::PipeInactive
                    | intel_cursor::SpiritCursorError::DbufNotReady),
                ) => {
                    deferred = deferred.saturating_add(1);
                    if deferred == 1 || deferred.is_multiple_of(1_000) {
                        crate::log_warn!(
                            target: "gfx";
                            "trueos-spirit: cursor move deferred fence={} pipe={} move_sequence={} retries={} error={:?}\n",
                            id.index(),
                            pipe_name(id),
                            request.sequence,
                            deferred,
                            error,
                        );
                    }
                    Timer::after(Duration::from_millis(SPIRIT_CURSOR_MOVE_RETRY_MS)).await;
                }
                Err(error) => {
                    crate::log_error!(
                        target: "gfx";
                        "trueos-spirit: cursor move rejected fence={} move_sequence={} error={:?} action=drop-invalid-request\n",
                        id.index(),
                        request.sequence,
                        error,
                    );
                    break false;
                }
            }
        };

        if portal_transition && applied {
            Timer::after(Duration::from_millis(SPIRIT_MOVE_PORTAL_POST_MS)).await;
        }
        if portal_transition {
            spirit_vfx::set_move_portal_transition(false);
        }
        request = MOVE_SIGNALS[id.index()].wait().await;
    }
}

/// The macro pool is intentionally one today. Fence/pipe capacity remains
/// four so later activation is a deliberate pool-limit and spawn-limit change.
#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn spirit_worker_task(worker_index: u8) {
    if worker_index as usize >= SPIRIT_WORKER_POOL_LIMIT {
        return;
    }

    let id = selected_spirit_fence().await;
    let bit = 1u8 << id.0;
    if ACTIVE_FENCE_MASK.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
        crate::log_error!(
            target: "gfx";
            "trueos-spirit: worker={} rejected pipe={} reason=cursor-fence-already-owned\n",
            worker_index,
            pipe_name(id),
        );
        return;
    }
    WORKER_FENCE_BINDINGS[worker_index as usize].store(id.0, Ordering::Release);
    let lilly_ready = worker_index != 0 || lilly::prepare_resident_once();
    crate::log!(
        "trueos-spirit: worker={} bound fence={} pipe={} carrier_slot={} expected_carrier_slot={} selection=complete-scanout-1to1-cursor-bank pool-active={} first_job=lilly-resident-assets lilly_ready={} route=guc-spirit-vfx-optional-background+sprite->spirit-cursor-backbuffer->cur-base default=clean-lilly/one-walker producer_release=guc-post-sync display_release=cursor-surflive gpu_admission=guc-context mode=continuous initial_trace_frames={} target_hz={} ui4_publish=0\n",
        worker_index,
        id.index(),
        pipe_name(id),
        crate::percpu::current_slot(),
        crate::workers::AP1_UI_SERVICE_SLOT,
        SPIRIT_WORKER_POOL_LIMIT,
        lilly_ready as u8,
        SPIRIT_VFX_INITIAL_TRACE_FRAMES,
        SPIRIT_VFX_TARGET_HZ,
    );
    spirit_cursor_worker_loop(id).await;
}

async fn selected_spirit_fence() -> SpiritFenceId {
    loop {
        if let Some(slot) = crate::intel::complete_scanout_pipeline_slot()
            && let Ok(fence_index) = u8::try_from(slot)
            && let Some(id) = SpiritFenceId::new(fence_index)
        {
            return id;
        }
        Timer::after(Duration::from_millis(SPIRIT_RETRY_MS)).await;
    }
}

async fn bound_spirit_fence(worker_index: u8) -> SpiritFenceId {
    loop {
        let fence = WORKER_FENCE_BINDINGS[worker_index as usize].load(Ordering::Acquire);
        if let Some(id) = SpiritFenceId::new(fence) {
            return id;
        }
        Timer::after(Duration::from_millis(SPIRIT_CURSOR_MOVE_RETRY_MS)).await;
    }
}

async fn spirit_cursor_worker_loop(id: SpiritFenceId) {
    if !id.is_active() {
        return;
    }
    let fence_index = id.0;

    while let Err(error) = intel_cursor::spirit_cursor_prepare(fence_index) {
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: fence={} double-buffer prepare deferred error={:?}\n",
            fence_index,
            error,
        );
        Timer::after(Duration::from_millis(SPIRIT_RETRY_MS)).await;
    }

    let mut retained_normal: Option<QueuedFrame> = None;
    let mut retained_gpu_logger: Option<QueuedFrame> = None;
    let mut inflight: Option<Inflight> = None;
    let mut gpu_inflight: Option<GpuInflight> = None;
    let mut rearm_retry: Option<QueuedFrame> = None;
    let mut stream_queued_frames = 0u64;
    let mut stream_next_deadline = Instant::now();
    let mut stream_cadence_phase = 0u64;
    let mut gpu_logger_cadence_phase = 0u64;
    let mut presentation_rate = SpiritPresentationRate::new();
    let mut stream_aborted = false;
    let mut boot_move_queued = false;
    let mut arm_deferred_polls = 0u32;
    let mut package_next_boundary = Instant::now();
    let mut package_started = Instant::now();
    let mut package_active: Option<lilly_protocol::LillyScheduledAnimation> = None;
    let mut package_reader_failures = 0u32;
    let mut gpu_logger_pause_started: Option<Instant> = None;
    let mut gpu_logger_next_deadline = Instant::now();
    let mut gpu_logger_frames = 0u64;
    loop {
        let package_now = Instant::now();
        let gpu_logger_snapshot = gpu_logger::active_snapshot();
        match (gpu_logger_pause_started, gpu_logger_snapshot) {
            (None, Some(snapshot)) => {
                gpu_logger_pause_started = Some(package_now);
                gpu_logger_next_deadline = package_now;
                gpu_logger_cadence_phase = 0;
                gpu_logger_frames = 0;
                crate::log_info!(
                    target: "gfx";
                    "trueos-spirit: gpu-logger override entering source={:?} generation={} ttl_remaining_ms={} extent=256x256 carrier=hardware-cursor producer=direct-gpu-worklist normal_sprite_vfx=suppressed ui4_publish=0 composition=0\n",
                    snapshot.source,
                    snapshot.generation,
                    snapshot.remaining_ms,
                );
            }
            (Some(paused_at), None) => {
                let paused = package_now.saturating_duration_since(paused_at);
                package_next_boundary = package_next_boundary + paused;
                package_started = package_started + paused;
                stream_next_deadline = stream_next_deadline + paused;
                presentation_rate.begin(package_now);
                gpu_logger_pause_started = None;
                crate::log_info!(
                    target: "gfx";
                    "trueos-spirit: gpu-logger override released frames={} paused_ms={} action=resume-lilly-sprite-vfx state=preserved cadence=deadline-shifted\n",
                    gpu_logger_frames,
                    paused.as_millis(),
                );
            }
            _ => {}
        }
        if gpu_logger_snapshot.is_none()
            && id == SpiritFenceId::FENCE_0
            && package_now >= package_next_boundary
        {
            match lilly_protocol::next_animation() {
                Ok(scheduled) => {
                    package_reader_failures = 0;
                    package_next_boundary = package_now
                        + Duration::from_millis(scheduled.boundary_ms.max(SPIRIT_IDLE_POLL_MS));
                    package_started = package_now;
                    package_active = Some(scheduled);
                }
                Err(error) => {
                    package_reader_failures = package_reader_failures.saturating_add(1);
                    if package_reader_failures == 1 || package_reader_failures.is_multiple_of(100) {
                        crate::log_warn!(
                            target: "gfx";
                            "trueos-spirit: package reader deferred failures={} error={:?}\n",
                            package_reader_failures,
                            error,
                        );
                    }
                    package_next_boundary = package_now + Duration::from_millis(SPIRIT_RETRY_MS);
                }
            }
        }

        if let Some(mut producing) = gpu_inflight.take() {
            match crate::intel::gpgpu::poll_spirit_vfx_submission(producing.submission) {
                crate::intel::gpgpu::SpiritVfxCompletion::Pending => {
                    producing.polls = producing.polls.saturating_add(1);
                    gpu_inflight = Some(producing);
                    Timer::after(Duration::from_millis(SPIRIT_GPU_POLL_MS)).await;
                    continue;
                }
                crate::intel::gpgpu::SpiritVfxCompletion::Complete(release) => {
                    if let Err(error) = release_gpgpu(producing.lease, release) {
                        cancel_pending(producing.lease);
                        gpu_inflight = None;
                        stream_aborted = true;
                        crate::log_error!(
                            target: "gfx";
                            "trueos-spirit: vfx producer release rejected frame={} shader_frame={} tag={} fence={} sequence={} polls={} error={:?} action=abort-stream\n",
                            stream_queued_frames,
                            producing.submission.frame(),
                            producing.submission.tag(),
                            fence_index,
                            producing.lease.fence.sequence,
                            producing.polls,
                            error,
                        );
                        continue;
                    }
                    if spirit_should_trace_frame(stream_queued_frames) {
                        crate::log_info!(
                            target: "gfx";
                            "trueos-spirit: vfx producer released frame={} shader_frame={} tag={} fence={} sequence={} polls={} gate=gpu-only next=cursor-arm\n",
                            stream_queued_frames,
                            producing.submission.frame(),
                            producing.submission.tag(),
                            fence_index,
                            producing.lease.fence.sequence,
                            producing.polls,
                        );
                    }
                    gpu_inflight = None;
                    continue;
                }
                completion @ (crate::intel::gpgpu::SpiritVfxCompletion::Failed
                | crate::intel::gpgpu::SpiritVfxCompletion::InvalidSubmission) => {
                    cancel_pending(producing.lease);
                    gpu_inflight = None;
                    stream_aborted = true;
                    crate::log_error!(
                        target: "gfx";
                        "trueos-spirit: vfx producer failed frame={} shader_frame={} tag={} fence={} sequence={} polls={} completion={:?} action=abort-stream-retain-last-surflive\n",
                        stream_queued_frames,
                        producing.submission.frame(),
                        producing.submission.tag(),
                        fence_index,
                        producing.lease.fence.sequence,
                        producing.polls,
                        completion,
                    );
                    continue;
                }
            }
        }

        if let Some(mut active) = inflight {
            match intel_cursor::spirit_cursor_poll(active.flip) {
                Ok(intel_cursor::SpiritCursorFlipState::Visible { ctl, base, live }) => {
                    if active.completes_fence {
                        complete_frame(active.frame.lease);
                        if active.frame.producer == SpiritFrameProducer::Vfx {
                            presentation_rate.observe_surflive(Instant::now());
                        }
                        if !boot_move_queued {
                            boot_move_queued = true;
                            match crate::intel::complete_scanout_pipeline_dimensions(id.index()) {
                                Some((width, _)) if width > intel_cursor::SPIRIT_CURSOR_DIM => {
                                    let movable_width = width - intel_cursor::SPIRIT_CURSOR_DIM;
                                    let delta_x = f64::from(SPIRIT_BOOT_MOVE_RIGHT_PIXELS)
                                        / f64::from(movable_width);
                                    match move_by(id, delta_x, 0.0) {
                                        Ok(move_fence) => crate::log_info!(
                                            target: "gfx";
                                            "trueos-spirit: boot move queued fence={} pipe={} move_sequence={} direction=right pixels={} delta_normalized={:.8} trigger=first-cursor-surflive transition=portal-350+150ms\n",
                                            id.index(),
                                            pipe_name(id),
                                            move_fence.sequence,
                                            SPIRIT_BOOT_MOVE_RIGHT_PIXELS,
                                            delta_x,
                                        ),
                                        Err(error) => crate::log_warn!(
                                            target: "gfx";
                                            "trueos-spirit: boot move skipped fence={} pipe={} pixels={} error={:?}\n",
                                            id.index(),
                                            pipe_name(id),
                                            SPIRIT_BOOT_MOVE_RIGHT_PIXELS,
                                            error,
                                        ),
                                    }
                                }
                                dimensions => crate::log_warn!(
                                    target: "gfx";
                                    "trueos-spirit: boot move skipped fence={} pipe={} pixels={} dimensions={:?}\n",
                                    id.index(),
                                    pipe_name(id),
                                    SPIRIT_BOOT_MOVE_RIGHT_PIXELS,
                                    dimensions,
                                ),
                            }
                        }
                        if active.frame.producer == SpiritFrameProducer::Vfx
                            && spirit_should_trace_frame(stream_queued_frames)
                        {
                            crate::log_info!(
                                target: "gfx";
                                "trueos-spirit: cursor SURFLIVE proven frame={} fence={} sequence={} pipe={} buffer={} ctl=0x{:08X} base=0x{:08X} live=0x{:08X} present_fps={} sample_window_ms={} boundary=cursor-display-live mode=continuous ui4_publish=0\n",
                                stream_queued_frames,
                                fence_index,
                                active.frame.lease.fence.sequence,
                                pipe_name(id),
                                active.frame.lease.surface.surface,
                                ctl,
                                base,
                                live,
                                presentation_rate.estimate_fps(),
                                SPIRIT_PRESENT_FPS_WINDOW_MS,
                            );
                            if stream_queued_frames == SPIRIT_VFX_INITIAL_TRACE_FRAMES {
                                crate::log!(
                                    "trueos-spirit: vfx initial window complete frames={} target_hz={} present_fps={} sample_window_ms={} action=continue-streaming final=cursor-plane-surflive pipe={} buffer={}\n",
                                    stream_queued_frames,
                                    SPIRIT_VFX_TARGET_HZ,
                                    presentation_rate.estimate_fps(),
                                    SPIRIT_PRESENT_FPS_WINDOW_MS,
                                    pipe_name(id),
                                    active.frame.lease.surface.surface,
                                );
                            }
                        }
                    } else {
                        finish_rearm(id);
                    }
                    match active.frame.producer {
                        SpiritFrameProducer::GpuLogger(_) => {
                            retained_gpu_logger = Some(active.frame);
                        }
                        SpiritFrameProducer::External | SpiritFrameProducer::Vfx => {
                            retained_normal = Some(active.frame);
                        }
                    }
                    inflight = None;
                    continue;
                }
                Ok(intel_cursor::SpiritCursorFlipState::Waiting { ctl, base, live }) => {
                    active.polls = active.polls.saturating_add(1);
                    let now = Instant::now();
                    let wait_ms = now
                        .saturating_duration_since(active.attempt_started)
                        .as_millis();
                    if wait_ms >= SPIRIT_SURFLIVE_TIMEOUT_MS {
                        if active.retries < SPIRIT_SURFLIVE_MAX_RETRIES {
                            crate::log_warn!(
                                target: "gfx";
                                "trueos-spirit: cursor SURFLIVE timeout frame={} fence={} sequence={} pipe={} buffer={} attempt={} polls={} wait_ms={} timeout_ms={} ctl=0x{:08X} base=0x{:08X} expected=0x{:08X} live=0x{:08X} action=reprogram-cursor-once\n",
                                stream_queued_frames,
                                fence_index,
                                active.frame.lease.fence.sequence,
                                pipe_name(id),
                                active.frame.lease.surface.surface,
                                active.retries.saturating_add(1),
                                active.polls,
                                wait_ms,
                                SPIRIT_SURFLIVE_TIMEOUT_MS,
                                ctl,
                                base,
                                active.frame.lease.surface.cursor_gpu,
                                live,
                            );
                            match intel_cursor::spirit_cursor_retry_arm(active.flip) {
                                Ok(()) => {
                                    active.polls = 0;
                                    active.attempt_started = now;
                                    active.retries = active.retries.saturating_add(1);
                                    inflight = Some(active);
                                    Timer::after(Duration::from_millis(SPIRIT_FLIP_POLL_MS)).await;
                                    continue;
                                }
                                Err(error) => {
                                    stop_failed_cursor_flip(id, active);
                                    crate::log_error!(
                                        target: "gfx";
                                        "trueos-spirit: cursor SURFLIVE retry failed frame={} fence={} sequence={} pipe={} buffer={} retries={} ctl=0x{:08X} base=0x{:08X} expected=0x{:08X} live=0x{:08X} error={:?} action=stop-spirit-stream-no-more-retries\n",
                                        stream_queued_frames,
                                        fence_index,
                                        active.frame.lease.fence.sequence,
                                        pipe_name(id),
                                        active.frame.lease.surface.surface,
                                        active.retries.saturating_add(1),
                                        ctl,
                                        base,
                                        active.frame.lease.surface.cursor_gpu,
                                        live,
                                        error,
                                    );
                                    return;
                                }
                            }
                        }

                        stop_failed_cursor_flip(id, active);
                        crate::log_error!(
                            target: "gfx";
                            "trueos-spirit: cursor SURFLIVE terminal failure frame={} fence={} sequence={} pipe={} buffer={} attempts={} polls={} wait_ms={} timeout_ms={} ctl=0x{:08X} base=0x{:08X} expected=0x{:08X} live=0x{:08X} action=stop-spirit-stream-no-more-retries\n",
                            stream_queued_frames,
                            fence_index,
                            active.frame.lease.fence.sequence,
                            pipe_name(id),
                            active.frame.lease.surface.surface,
                            active.retries.saturating_add(1),
                            active.polls,
                            wait_ms,
                            SPIRIT_SURFLIVE_TIMEOUT_MS,
                            ctl,
                            base,
                            active.frame.lease.surface.cursor_gpu,
                            live,
                        );
                        return;
                    }
                    if spirit_should_trace_frame(stream_queued_frames)
                        && (active.polls == 1 || active.polls.is_multiple_of(1_000))
                    {
                        crate::log_trace!(
                            target: "gfx";
                            "trueos-spirit: cursor SURFLIVE waiting fence={} sequence={} pipe={} buffer={} polls={} ctl=0x{:08X} base=0x{:08X} expected=0x{:08X} live=0x{:08X}\n",
                            fence_index,
                            active.frame.lease.fence.sequence,
                            pipe_name(id),
                            active.frame.lease.surface.surface,
                            active.polls,
                            ctl,
                            base,
                            active.frame.lease.surface.cursor_gpu,
                            live,
                        );
                    }
                    inflight = Some(active);
                    Timer::after(Duration::from_millis(SPIRIT_FLIP_POLL_MS)).await;
                    continue;
                }
                Ok(intel_cursor::SpiritCursorFlipState::Interrupted { ctl, base, live }) => {
                    crate::log_warn!(
                        target: "gfx";
                        "trueos-spirit: cursor flip interrupted fence={} sequence={} pipe={} ctl=0x{:08X} base=0x{:08X} expected=0x{:08X} live=0x{:08X}\n",
                        fence_index,
                        active.frame.lease.fence.sequence,
                        pipe_name(id),
                        ctl,
                        base,
                        active.frame.lease.surface.cursor_gpu,
                        live,
                    );
                    if !active.completes_fence {
                        rearm_retry = Some(active.frame);
                    }
                    inflight = None;
                    Timer::after(Duration::from_millis(SPIRIT_IDLE_POLL_MS)).await;
                    continue;
                }
                Err(error) => {
                    crate::log_warn!(
                        target: "gfx";
                        "trueos-spirit: fence={} live poll deferred error={:?}\n",
                        fence_index,
                        error,
                    );
                    Timer::after(Duration::from_millis(SPIRIT_RETRY_MS)).await;
                    continue;
                }
            }
        }

        let candidate = if let Some(frame) = ready_pending(id) {
            Some((frame, true))
        } else if let Some(frame) = rearm_retry.take() {
            Some((frame, false))
        } else if (stream_aborted || gpu_logger_snapshot.is_some())
            && let Some(frame) = (if gpu_logger_snapshot.is_some() {
                retained_gpu_logger
            } else {
                retained_normal
            })
            && matches!(intel_cursor::spirit_cursor_rearm_needed(fence_index), Ok(true))
            && begin_rearm(id)
        {
            Some((frame, false))
        } else {
            None
        };

        let Some((frame, completes_fence)) = candidate else {
            if let Some(snapshot) = gpu_logger_snapshot {
                if Instant::now() < gpu_logger_next_deadline {
                    Timer::at(gpu_logger_next_deadline).await;
                    continue;
                }
                match submit_gpu_logger_frame(id, snapshot) {
                    Ok(submitted) => {
                        let period = spirit_gpu_logger_frame_period(&mut gpu_logger_cadence_phase);
                        let scheduled = gpu_logger_next_deadline + period;
                        let now = Instant::now();
                        gpu_logger_next_deadline = if now > scheduled {
                            now + period
                        } else {
                            scheduled
                        };
                        gpu_logger_frames = gpu_logger_frames.saturating_add(1);
                        if gpu_logger_frames == 1 || gpu_logger_frames.is_multiple_of(16) {
                            crate::log_info!(
                                target: "gfx";
                                "trueos-spirit: gpu-logger frame={} source={:?} generation={} fence={} sequence={} path={:?} rects={} gpu_submits={} gpu_submit_ms={} release={} sample_frame={} cadence_us={} fps={} frame_us={} geometry_us={} prepare_us={} retire_wait_us={} objects={} draws={} triangles={} retries={}/{} carrier=hardware-cursor display_release=surflive ui4_publish=0 composition=0\n",
                                gpu_logger_frames,
                                snapshot.source,
                                snapshot.generation,
                                fence_index,
                                submitted.fence.sequence,
                                submitted.path,
                                submitted.rects,
                                submitted.gpu_submits,
                                submitted.gpu_submit_ms,
                                submitted.release_sequence,
                                snapshot.sample.frame_index,
                                snapshot.sample.cadence_us,
                                gpu_logger::fps_from_cadence_us(snapshot.sample.cadence_us),
                                snapshot.sample.frame_us,
                                snapshot.sample.geometry_us,
                                snapshot.sample.prepare_us,
                                snapshot.sample.retire_wait_us,
                                snapshot.sample.objects,
                                snapshot.sample.draws,
                                snapshot.sample.triangles,
                                snapshot.sample.busy_retries,
                                snapshot.sample.incomplete_retries,
                            );
                        }
                        continue;
                    }
                    Err(SpiritSubmitError::GpuSubmissionFailed) => {
                        crate::log_error!(
                            target: "gfx";
                            "trueos-spirit: gpu-logger producer incomplete source={:?} generation={} fence={} frame={} action=preserve-current-surflive-and-stop-worker\n",
                            snapshot.source,
                            snapshot.generation,
                            fence_index,
                            gpu_logger_frames.saturating_add(1),
                        );
                        return;
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "gfx";
                            "trueos-spirit: gpu-logger admission deferred source={:?} generation={} fence={} frame={} error={:?}\n",
                            snapshot.source,
                            snapshot.generation,
                            fence_index,
                            gpu_logger_frames.saturating_add(1),
                            error,
                        );
                    }
                }
                Timer::after(Duration::from_millis(SPIRIT_RETRY_MS)).await;
                continue;
            }
            if !stream_aborted {
                if Instant::now() < stream_next_deadline {
                    Timer::at(stream_next_deadline).await;
                    continue;
                }
                let source_frame = package_active.and_then(|scheduled| {
                    let elapsed_ms = Instant::now()
                        .saturating_duration_since(package_started)
                        .as_millis();
                    scheduled
                        .frame_at_elapsed(elapsed_ms)
                        .map(|part| part.surface)
                });
                let Some(source_frame) = source_frame else {
                    Timer::after(Duration::from_millis(SPIRIT_IDLE_POLL_MS)).await;
                    continue;
                };
                match submit_spirit_vfx_frame(id, presentation_rate.estimate_fps(), source_frame) {
                    Ok(producing) => {
                        let period = spirit_vfx_frame_period(&mut stream_cadence_phase);
                        let scheduled = stream_next_deadline + period;
                        let now = Instant::now();
                        stream_next_deadline = if now > scheduled {
                            now + period
                        } else {
                            scheduled
                        };
                        stream_queued_frames = stream_queued_frames.saturating_add(1);
                        if spirit_should_trace_frame(stream_queued_frames) {
                            crate::log_info!(
                                target: "gfx";
                                "trueos-spirit: vfx stream submitted frame={} shader_frame={} tag={} fence={} sequence={} target_hz={} present_fps={} sample_window_ms={} cadence=deadline-paced/no-catch-up issuer=one-shot completion=tag-poll/yield gate=guc-context producer-release=guc-post-sync display-release=surflive mode=continuous artifacts=optional-background+sprite default=clean-lilly\n",
                                stream_queued_frames,
                                producing.submission.frame(),
                                producing.submission.tag(),
                                fence_index,
                                producing.lease.fence.sequence,
                                SPIRIT_VFX_TARGET_HZ,
                                presentation_rate.estimate_fps(),
                                SPIRIT_PRESENT_FPS_WINDOW_MS,
                            );
                        }
                        gpu_inflight = Some(producing);
                        continue;
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "gfx";
                            "trueos-spirit: vfx stream admission deferred frame={} error={:?}\n",
                            stream_queued_frames.saturating_add(1),
                            error,
                        );
                    }
                }
                Timer::after(Duration::from_millis(SPIRIT_RETRY_MS)).await;
                continue;
            }
            Timer::after(Duration::from_millis(SPIRIT_IDLE_POLL_MS)).await;
            continue;
        };
        match intel_cursor::spirit_cursor_arm(frame.lease.surface) {
            Ok(flip) => {
                arm_deferred_polls = 0;
                if !completes_fence {
                    resignal_current_move(id);
                }
                inflight = Some(Inflight {
                    flip,
                    frame,
                    completes_fence,
                    polls: 0,
                    attempt_started: Instant::now(),
                    retries: 0,
                });
            }
            Err(
                error @ (intel_cursor::SpiritCursorError::PipeInactive
                | intel_cursor::SpiritCursorError::DbufNotReady
                | intel_cursor::SpiritCursorError::HardwareNotReady
                | intel_cursor::SpiritCursorError::FlipPending),
            ) => {
                arm_deferred_polls = arm_deferred_polls.saturating_add(1);
                if arm_deferred_polls == 1 || arm_deferred_polls.is_multiple_of(300) {
                    crate::log_warn!(
                        target: "gfx";
                        "trueos-spirit: cursor arm waiting fence={} sequence={} selected_pipe={} complete_scanout_slot={:?} retries={} error={:?}\n",
                        fence_index,
                        frame.lease.fence.sequence,
                        pipe_name(id),
                        crate::intel::complete_scanout_pipeline_slot(),
                        arm_deferred_polls,
                        error,
                    );
                }
                if !completes_fence {
                    rearm_retry = Some(frame);
                }
                Timer::after(Duration::from_millis(SPIRIT_IDLE_POLL_MS)).await;
            }
            Err(error) => {
                if !completes_fence {
                    rearm_retry = Some(frame);
                }
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: fence={} arm deferred error={:?}\n",
                    fence_index,
                    error,
                );
                Timer::after(Duration::from_millis(SPIRIT_RETRY_MS)).await;
            }
        }
    }
}

fn stop_failed_cursor_flip(id: SpiritFenceId, active: Inflight) {
    intel_cursor::spirit_cursor_abandon(active.flip);
    if active.completes_fence {
        cancel_pending(active.frame.lease);
    } else {
        finish_rearm(id);
    }
    ACTIVE_FENCE_MASK.fetch_and(!(1u8 << id.0), Ordering::AcqRel);
}

fn matching_pending(
    mailbox: &SpiritMailbox,
    lease: SpiritFrameLease,
) -> Result<&QueuedFrame, SpiritSubmitError> {
    mailbox
        .pending
        .as_ref()
        .filter(|pending| pending.lease == lease)
        .ok_or(SpiritSubmitError::StaleLease)
}

fn matching_pending_mut(
    mailbox: &mut SpiritMailbox,
    lease: SpiritFrameLease,
) -> Result<&mut QueuedFrame, SpiritSubmitError> {
    mailbox
        .pending
        .as_mut()
        .filter(|pending| pending.lease == lease)
        .ok_or(SpiritSubmitError::StaleLease)
}

fn require_unreleased(
    pending: &QueuedFrame,
    producer: SpiritBarrierSet,
) -> Result<(), SpiritSubmitError> {
    if !pending.required.contains(producer) {
        return Err(SpiritSubmitError::ProducerNotRequired);
    }
    if pending.released.contains(producer) {
        return Err(SpiritSubmitError::ProducerAlreadyReleased);
    }
    Ok(())
}

fn ready_pending(id: SpiritFenceId) -> Option<QueuedFrame> {
    let mailbox = MAILBOXES[id.index()].lock();
    mailbox.pending.filter(|pending| pending.is_ready())
}

fn complete_frame(lease: SpiritFrameLease) {
    let mut mailbox = MAILBOXES[lease.fence.id.index()].lock();
    if mailbox.pending.map(|pending| pending.lease) == Some(lease) {
        mailbox.pending = None;
        COMPLETED[lease.fence.id.index()].store(lease.fence.sequence, Ordering::Release);
    }
}

fn cancel_pending(lease: SpiritFrameLease) {
    let mut mailbox = MAILBOXES[lease.fence.id.index()].lock();
    if mailbox.pending.map(|pending| pending.lease) == Some(lease) {
        mailbox.pending = None;
    }
}

fn begin_rearm(id: SpiritFenceId) -> bool {
    let mut mailbox = MAILBOXES[id.index()].lock();
    if mailbox.pending.is_some() || mailbox.rearming {
        return false;
    }
    mailbox.rearming = true;
    true
}

fn finish_rearm(id: SpiritFenceId) {
    MAILBOXES[id.index()].lock().rearming = false;
}

fn spirit_vfx_frame_period(phase: &mut u64) -> Duration {
    spirit_frame_period(phase, SPIRIT_VFX_TARGET_HZ)
}

fn spirit_gpu_logger_frame_period(phase: &mut u64) -> Duration {
    spirit_frame_period(phase, SPIRIT_GPU_LOGGER_TARGET_HZ)
}

fn spirit_frame_period(phase: &mut u64, hz: u64) -> Duration {
    let mut ticks = embassy_time::TICK_HZ / hz;
    *phase = phase.saturating_add(embassy_time::TICK_HZ % hz);
    if *phase >= hz {
        *phase -= hz;
        ticks = ticks.saturating_add(1);
    }
    Duration::from_ticks(ticks.max(1))
}

fn spirit_should_trace_frame(frame: u64) -> bool {
    frame <= SPIRIT_VFX_INITIAL_TRACE_FRAMES
        || frame.is_multiple_of(SPIRIT_VFX_PERIODIC_TRACE_FRAMES)
}

fn map_cursor_error(error: intel_cursor::SpiritCursorError) -> SpiritSubmitError {
    match error {
        intel_cursor::SpiritCursorError::FlipPending => SpiritSubmitError::Busy,
        intel_cursor::SpiritCursorError::InvalidChannel
        | intel_cursor::SpiritCursorError::InvalidFrame => SpiritSubmitError::InvalidCommand,
        intel_cursor::SpiritCursorError::HardwareNotReady
        | intel_cursor::SpiritCursorError::PipeInactive
        | intel_cursor::SpiritCursorError::DbufNotReady
        | intel_cursor::SpiritCursorError::AllocationFailed
        | intel_cursor::SpiritCursorError::MappingFailed => SpiritSubmitError::HardwareNotReady,
    }
}

pub(crate) fn hardware_ready() -> bool {
    crate::intel::has_claimed_device() && crate::intel::gen12_integrated_pat_ready()
}

const fn pipe_name(id: SpiritFenceId) -> &'static str {
    match id.0 {
        0 => "pipe-a",
        1 => "pipe-b",
        2 => "pipe-c",
        _ => "pipe-d",
    }
}
