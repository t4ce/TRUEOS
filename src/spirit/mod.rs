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
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_time::{Duration, Timer};
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
const SPIRIT_RETRY_MS: u64 = 50;
const SPIRIT_LAB256_STARTUP_FRAMES: u32 = 10;

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

    pub(crate) const fn is_active(self) -> bool {
        self.index() < SPIRIT_WORKER_POOL_LIMIT
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

fn submit_lab256_startup_frame(
    id: SpiritFenceId,
    frame: u32,
) -> Result<SpiritFence, SpiritSubmitError> {
    let lease = acquire_frame(id, 0.5, 0.5, SpiritBarrierSet::GPU)?;
    let target = match gpgpu_target(lease) {
        Ok(target) => target,
        Err(error) => {
            cancel_pending(lease);
            return Err(error);
        }
    };
    let produced = crate::intel::gpgpu::lab256_spirit_frame(target, frame);
    let Some(release) = produced.release else {
        cancel_pending(lease);
        return Err(if produced.submitted {
            SpiritSubmitError::GpuSubmissionFailed
        } else {
            SpiritSubmitError::HardwareNotReady
        });
    };
    if let Err(error) = release_gpgpu(lease, release) {
        cancel_pending(lease);
        return Err(error);
    }
    Ok(lease.fence)
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
}

/// The macro pool is intentionally one today. Fence/pipe capacity remains
/// four so later activation is a deliberate pool-limit and spawn-limit change.
#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn spirit_worker_task(fence_index: u8) {
    let Some(id) = SpiritFenceId::new(fence_index).filter(|id| id.is_active()) else {
        return;
    };
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: worker start fence={} pipe={} pool-active={} pipe-cap={}\n",
        fence_index,
        pipe_name(id),
        SPIRIT_WORKER_POOL_LIMIT,
        SPIRIT_FENCE_COUNT,
    );

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
    let mut rearm_retry: Option<QueuedFrame> = None;
    let mut startup_next_frame = 0u32;
    let mut startup_aborted = false;
    loop {
        if let Some(active) = inflight {
            match intel_cursor::spirit_cursor_poll(active.flip) {
                Ok(intel_cursor::SpiritCursorFlipState::Visible) => {
                    if active.completes_fence {
                        complete_frame(active.frame.lease);
                    } else {
                        finish_rearm(id);
                    }
                    retained = Some(active.frame);
                    inflight = None;
                    continue;
                }
                Ok(intel_cursor::SpiritCursorFlipState::Waiting) => {
                    Timer::after(Duration::from_millis(SPIRIT_FLIP_POLL_MS)).await;
                    continue;
                }
                Ok(intel_cursor::SpiritCursorFlipState::Interrupted) => {
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
        } else if (startup_aborted || startup_next_frame >= SPIRIT_LAB256_STARTUP_FRAMES)
            && let Some(frame) = retained
            && matches!(intel_cursor::spirit_cursor_rearm_needed(fence_index), Ok(true))
            && begin_rearm(id)
        {
            Some((frame, false))
        } else {
            None
        };

        let Some((frame, completes_fence)) = candidate else {
            if !startup_aborted && startup_next_frame < SPIRIT_LAB256_STARTUP_FRAMES {
                match submit_lab256_startup_frame(id, startup_next_frame) {
                    Ok(fence) => {
                        crate::log_info!(
                            target: "gfx";
                            "trueos-spirit: lab256 startup queued frame={}/{} fence={} sequence={} gate=gpu-only cpu-gate=0 producer-release=guc-post-sync display-release=surflive\n",
                            startup_next_frame + 1,
                            SPIRIT_LAB256_STARTUP_FRAMES,
                            fence_index,
                            fence.sequence,
                        );
                        startup_next_frame += 1;
                        continue;
                    }
                    Err(
                        error @ (SpiritSubmitError::GpuSubmissionFailed
                        | SpiritSubmitError::InvalidGpuRelease),
                    ) => {
                        startup_aborted = true;
                        crate::log_error!(
                            target: "gfx";
                            "trueos-spirit: lab256 startup aborted frame={} error={:?} action=retain-last-surflive-frame cpu-fallback=0\n",
                            startup_next_frame,
                            error,
                        );
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "gfx";
                            "trueos-spirit: lab256 startup deferred frame={} error={:?}\n",
                            startup_next_frame,
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
                inflight = Some(Inflight {
                    flip,
                    frame,
                    completes_fence,
                });
            }
            Err(
                intel_cursor::SpiritCursorError::PipeInactive
                | intel_cursor::SpiritCursorError::DbufNotReady
                | intel_cursor::SpiritCursorError::HardwareNotReady
                | intel_cursor::SpiritCursorError::FlipPending,
            ) => {
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
