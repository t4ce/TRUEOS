//! TrueOS-Spirit presentation core.
//!
//! Fence 0..3 remain reserved one-to-one for Intel cursor pipe A..D, but the
//! sane initial deployment activates only fence 0. Every activated channel
//! owns a double-buffered hardware surface pair and advances independently;
//! Spirit intentionally has no multi-pipe/gang flip operation.
//!
//! Each frame also owns a two-bit producer latch. CPU and 3D-GPU releases are
//! upstream eligibility proofs; the selected surface is armed only when every
//! configured bit has arrived. Intel CUR_SURFLIVE is the separate downstream
//! proof that completes the public Spirit fence.

use core::ops::BitOr;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

mod intel_cursor;

/// Architectural pipe/fence capacity kept for later activation.
pub(crate) const SPIRIT_FENCE_COUNT: usize = 4;
/// Initial Embassy pool and runtime activation limit.
pub(crate) const SPIRIT_WORKER_POOL_LIMIT: usize = 1;
const _: () = assert!(SPIRIT_WORKER_POOL_LIMIT > 0);
const _: () = assert!(SPIRIT_WORKER_POOL_LIMIT <= SPIRIT_FENCE_COUNT);

const SPIRIT_IDLE_POLL_MS: u64 = 16;
const SPIRIT_FLIP_POLL_MS: u64 = 1;
const SPIRIT_GPU_POLL_MS: u64 = 1;
const SPIRIT_RETRY_MS: u64 = 50;
const SPIRIT_LAB256_INITIAL_TRACE_FRAMES: u64 = 30;
const SPIRIT_LAB256_PERIODIC_TRACE_FRAMES: u64 = 60;
const SPIRIT_LAB256_TARGET_HZ: u64 = 60;
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
    x_normalized: f64,
    y_normalized: f64,
    required: SpiritBarrierSet,
    released: SpiritBarrierSet,
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
    last_gpu_release_sequence: [u64; 2],
    last_gpgpu_release_sequence: [u64; 2],
}

impl SpiritMailbox {
    const fn new() -> Self {
        Self {
            next_sequence: 0,
            pending: None,
            rearming: false,
            last_gpu_release_sequence: [0; 2],
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
static COMPLETED: [AtomicU64; SPIRIT_FENCE_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static ACTIVE_FENCE_MASK: AtomicU8 = AtomicU8::new(0);

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
    x_normalized: f64,
    y_normalized: f64,
    required: SpiritBarrierSet,
) -> Result<SpiritFrameLease, SpiritSubmitError> {
    if !id.is_active() {
        return Err(SpiritSubmitError::InactiveFence);
    }
    if !x_normalized.is_finite() || !y_normalized.is_finite() {
        return Err(SpiritSubmitError::InvalidCommand);
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
        x_normalized,
        y_normalized,
        required,
        released: SpiritBarrierSet::NONE,
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

/// Return the exact allocation as a 3D render destination.
///
/// The `gpu` field carries the permanent display GGTT alias for identity. The
/// direct Intel 3D path keys on `phys` and installs its own persistent render
/// alias; no staging allocation or presentation copy is introduced here. Its
/// render-target state uses B8G8R8A8 storage so logical shader RGBA lands in
/// the ARGB cursor plane's required little-endian BGRA byte order.
#[allow(dead_code)]
pub(crate) fn render_3d_target(
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

/// Return the exact cursor allocation as a raw premultiplied-RGBA8 compute
/// destination. Unlike the formatted 3D target, a GPGPU pointer writes bytes
/// directly and therefore retains the Lab256 RGBA storage contract.
fn gpgpu_target(
    lease: SpiritFrameLease,
) -> Result<crate::intel::gpgpu::GpgpuRgba8Surface, SpiritSubmitError> {
    let mailbox = MAILBOXES[lease.fence.id.index()].lock();
    let pending = matching_pending(&mailbox, lease)?;
    require_unreleased(pending, SpiritBarrierSet::GPU)?;
    crate::intel::gpgpu::GpgpuRgba8Surface::new(
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

/// Release the 3D producer bit only with the renderer's exact-allocation,
/// cache-drained completion proof. The sequence must also be newer than the
/// last proof accepted for this physical member, so an old proof cannot be
/// replayed when the double buffer cycles back to the same allocation.
#[allow(dead_code)]
pub(crate) fn release_gpu(
    lease: SpiritFrameLease,
    release: crate::intel::render::ResidentSceneReleaseFence,
) -> Result<(), SpiritSubmitError> {
    if !release.matches(lease.surface.phys, lease.surface.byte_len) {
        return Err(SpiritSubmitError::InvalidGpuRelease);
    }
    let mut mailbox = MAILBOXES[lease.fence.id.index()].lock();
    let surface_index = lease.surface.surface as usize;
    if surface_index >= mailbox.last_gpu_release_sequence.len()
        || release.sequence() <= mailbox.last_gpu_release_sequence[surface_index]
    {
        return Err(SpiritSubmitError::InvalidGpuRelease);
    }
    let pending = matching_pending_mut(&mut mailbox, lease)?;
    require_unreleased(pending, SpiritBarrierSet::GPU)?;
    pending.released = pending.released | SpiritBarrierSet::GPU;
    mailbox.last_gpu_release_sequence[surface_index] = release.sequence();
    Ok(())
}

/// Release the same GPU producer bit with the GPGPU lane's exact-allocation
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

fn submit_lab256_frame(
    id: SpiritFenceId,
    present_fps: u32,
) -> Result<GpuInflight, SpiritSubmitError> {
    let lease = acquire_frame(id, 0.5, 0.5, SpiritBarrierSet::GPU)?;
    let target = match gpgpu_target(lease) {
        Ok(target) => target,
        Err(error) => {
            cancel_pending(lease);
            return Err(error);
        }
    };
    let pointer_xy = spirit_lab256_pointer_snapshot();
    let Some(submission) =
        crate::intel::gpgpu::submit_lab256_spirit_frame(target, present_fps, pointer_xy)
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

fn spirit_lab256_pointer_snapshot() -> Option<(u16, u16)> {
    let (_, x_normalized, y_normalized, _) =
        crate::r::cursor::preferred_physical_cursor_snapshot_with_slot_buttons()?;
    Some((
        spirit_lab256_normalized_coord(x_normalized),
        spirit_lab256_normalized_coord(y_normalized),
    ))
}

fn spirit_lab256_normalized_coord(value: f64) -> u16 {
    let finite = if value.is_finite() { value } else { 0.5 };
    (finite.clamp(0.0, 1.0) * 255.0 + 0.5) as u16
}

/// Current naive convenience path: CPU-author a solid circle and release it.
#[allow(dead_code)]
pub(crate) fn submit(
    id: SpiritFenceId,
    stream: SpiritCommandStream,
) -> Result<SpiritFence, SpiritSubmitError> {
    let lease = acquire_frame(id, stream.x_normalized, stream.y_normalized, SpiritBarrierSet::CPU)?;
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
}

#[derive(Copy, Clone)]
struct GpuInflight {
    lease: SpiritFrameLease,
    submission: crate::intel::gpgpu::Lab256SpiritSubmission,
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
            estimate_fps: SPIRIT_LAB256_TARGET_HZ as u32,
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

/// The macro pool is intentionally one today. Fence/pipe capacity remains
/// four so later activation is a deliberate pool-limit and spawn-limit change.
#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn spirit_worker_task(worker_index: u8) {
    if worker_index as usize >= SPIRIT_WORKER_POOL_LIMIT {
        return;
    }

    let id = loop {
        if let Some(slot) = crate::intel::complete_scanout_pipeline_slot()
            && let Ok(fence_index) = u8::try_from(slot)
            && let Some(id) = SpiritFenceId::new(fence_index)
        {
            break id;
        }
        Timer::after(Duration::from_millis(SPIRIT_RETRY_MS)).await;
    };
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
    crate::log!(
        "trueos-spirit: worker={} bound fence={} pipe={} carrier_slot={} expected_carrier_slot={} selection=complete-scanout-1to1-cursor-bank pool-active={} route=guc-lab256->spirit-cursor-backbuffer->cur-base producer_release=guc-post-sync display_release=cursor-surflive mode=continuous initial_trace_frames={} target_hz={} ui4_publish=0\n",
        worker_index,
        id.index(),
        pipe_name(id),
        crate::percpu::current_slot(),
        crate::workers::AP1_UI_SERVICE_SLOT,
        SPIRIT_WORKER_POOL_LIMIT,
        SPIRIT_LAB256_INITIAL_TRACE_FRAMES,
        SPIRIT_LAB256_TARGET_HZ,
    );
    spirit_cursor_worker_loop(id).await;
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

    let mut retained: Option<QueuedFrame> = None;
    let mut inflight: Option<Inflight> = None;
    let mut gpu_inflight: Option<GpuInflight> = None;
    let mut rearm_retry: Option<QueuedFrame> = None;
    let mut stream_queued_frames = 0u64;
    let mut stream_next_deadline = Instant::now();
    let mut stream_cadence_phase = 0u64;
    let mut presentation_rate = SpiritPresentationRate::new();
    let mut stream_aborted = false;
    let mut arm_deferred_polls = 0u32;
    loop {
        if let Some(mut producing) = gpu_inflight {
            match crate::intel::gpgpu::poll_lab256_spirit_submission(producing.submission) {
                crate::intel::gpgpu::Lab256SpiritCompletion::Pending => {
                    producing.polls = producing.polls.saturating_add(1);
                    gpu_inflight = Some(producing);
                    Timer::after(Duration::from_millis(SPIRIT_GPU_POLL_MS)).await;
                    continue;
                }
                crate::intel::gpgpu::Lab256SpiritCompletion::Complete(release) => {
                    if let Err(error) = release_gpgpu(producing.lease, release) {
                        cancel_pending(producing.lease);
                        gpu_inflight = None;
                        stream_aborted = true;
                        crate::log_error!(
                            target: "gfx";
                            "trueos-spirit: lab256 producer release rejected frame={} shader_frame={} tag={} fence={} sequence={} polls={} error={:?} action=abort-stream\n",
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
                            "trueos-spirit: lab256 producer released frame={} shader_frame={} tag={} fence={} sequence={} polls={} gate=gpu-only next=cursor-arm\n",
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
                completion @ (crate::intel::gpgpu::Lab256SpiritCompletion::Failed
                | crate::intel::gpgpu::Lab256SpiritCompletion::InvalidSubmission) => {
                    cancel_pending(producing.lease);
                    gpu_inflight = None;
                    stream_aborted = true;
                    crate::log_error!(
                        target: "gfx";
                        "trueos-spirit: lab256 producer failed frame={} shader_frame={} tag={} fence={} sequence={} polls={} completion={:?} action=abort-stream-retain-last-surflive\n",
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
                        presentation_rate.observe_surflive(Instant::now());
                        if spirit_should_trace_frame(stream_queued_frames) {
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
                            if stream_queued_frames == SPIRIT_LAB256_INITIAL_TRACE_FRAMES {
                                crate::log!(
                                    "trueos-spirit: lab256 initial window complete frames={} target_hz={} present_fps={} sample_window_ms={} action=continue-streaming final=cursor-plane-surflive pipe={} buffer={}\n",
                                    stream_queued_frames,
                                    SPIRIT_LAB256_TARGET_HZ,
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
                    retained = Some(active.frame);
                    inflight = None;
                    continue;
                }
                Ok(intel_cursor::SpiritCursorFlipState::Waiting { ctl, base, live }) => {
                    active.polls = active.polls.saturating_add(1);
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
        } else if stream_aborted
            && let Some(frame) = retained
            && matches!(intel_cursor::spirit_cursor_rearm_needed(fence_index), Ok(true))
            && begin_rearm(id)
        {
            Some((frame, false))
        } else {
            None
        };

        let Some((frame, completes_fence)) = candidate else {
            if !stream_aborted {
                if Instant::now() < stream_next_deadline {
                    Timer::at(stream_next_deadline).await;
                    continue;
                }
                match submit_lab256_frame(id, presentation_rate.estimate_fps()) {
                    Ok(producing) => {
                        let period = spirit_lab256_frame_period(&mut stream_cadence_phase);
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
                                "trueos-spirit: lab256 stream submitted frame={} shader_frame={} tag={} fence={} sequence={} target_hz={} present_fps={} sample_window_ms={} cadence=deadline-paced/no-catch-up issuer=one-shot completion=tag-poll/yield gate=gpu-only cpu-gate=0 producer-release=guc-post-sync display-release=surflive mode=continuous\n",
                                stream_queued_frames,
                                producing.submission.frame(),
                                producing.submission.tag(),
                                fence_index,
                                producing.lease.fence.sequence,
                                SPIRIT_LAB256_TARGET_HZ,
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
                            "trueos-spirit: lab256 stream admission deferred frame={} error={:?}\n",
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
        let cursor_frame = intel_cursor::SpiritCursorFrame {
            x_normalized: frame.x_normalized,
            y_normalized: frame.y_normalized,
        };
        match intel_cursor::spirit_cursor_arm(frame.lease.surface, cursor_frame) {
            Ok(flip) => {
                arm_deferred_polls = 0;
                inflight = Some(Inflight {
                    flip,
                    frame,
                    completes_fence,
                    polls: 0,
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

fn spirit_lab256_frame_period(phase: &mut u64) -> Duration {
    let hz = SPIRIT_LAB256_TARGET_HZ;
    let mut ticks = embassy_time::TICK_HZ / hz;
    *phase = phase.saturating_add(embassy_time::TICK_HZ % hz);
    if *phase >= hz {
        *phase -= hz;
        ticks = ticks.saturating_add(1);
    }
    Duration::from_ticks(ticks.max(1))
}

fn spirit_should_trace_frame(frame: u64) -> bool {
    frame <= SPIRIT_LAB256_INITIAL_TRACE_FRAMES
        || frame.is_multiple_of(SPIRIT_LAB256_PERIODIC_TRACE_FRAMES)
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
