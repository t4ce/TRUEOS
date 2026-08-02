//! Shared multi-consumer GPU-font service.
//!
//! `RetainScene` yields GPU-VM-resident Skrifa coverage that a caller may
//! restamp repeatedly. `Stamp` is the one-shot path: the worker creates the
//! same retained representation temporarily, composites ordered font/color
//! layers into either a new GPU-visible premultiplied RGBA8 buffer or a leased
//! UI4 frame, and returns the owned buffer or exact producer-release proof
//! asynchronously. Stamp callers may preserve a canvas or request an exact
//! coverage-union crop; both obey the UHD/4K pixel and 4096-glyph soft caps.
//! The lane is deliberately local to real font retain/stamp work. Unrelated
//! GPU clients own admission through the GPU executor and GuC contexts.

use alloc::{boxed::Box, collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_sync::semaphore::{FairSemaphore, Semaphore, SemaphoreReleaser};
use embassy_sync::signal::Signal;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::intel::gpu_font::{
    GpuFontFace, GpuFontJobEntry, GpuFontRetainedScene, GpuFontRetainedSceneError,
    GpuFontRetainedStyle, GpuFontRgba, GpuFontTextRequest, MAX_DYNAMIC_TEXT_CHARS,
    ensure_font_face_available, font_face_supports_text, retain_gpu_font_centered_scene_at_raster,
    retain_gpu_font_scene_at_raster,
};

const FONT_KERNEL_QUEUE_CAPACITY: usize = 32;
const FONT_KERNEL_MAX_RUNS: usize = 64;
const FONT_KERNEL_MAX_STAMP_LAYERS: usize = 64;
pub(crate) const FONT_STAMP_MAX_EXTENT: u32 = 4096;
pub(crate) const FONT_STAMP_MAX_PIXELS: u64 = 3840 * 2160;
pub(crate) const FONT_STAMP_MAX_GLYPHS: usize = 4096;
const FONT_KERNEL_LANE_RETRY_MS: u64 = 2;
const FONT_KERNEL_GPU_RETRY_MS: u64 = 2;
const FONT_KERNEL_GPU_WAITERS: usize = 32;

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
static ONLINE: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static GPU_RETRY_DELAY_PENDING: AtomicBool = AtomicBool::new(false);
static LAST_RETAIN_PARTITION_LOG_TICKET: AtomicU64 = AtomicU64::new(0);
static LAST_STAMP_PARTITION_LOG_TICKET: AtomicU64 = AtomicU64::new(0);
static WORK_AVAILABLE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static REQUESTS: Mutex<VecDeque<QueuedFontRequest>> = Mutex::new(VecDeque::new());
static STATUS: Mutex<FontKernelServiceStatus> = Mutex::new(FontKernelServiceStatus::new());
static GPU_LANE: FairSemaphore<crate::wait::EmbassySpinRawMutex, FONT_KERNEL_GPU_WAITERS> =
    FairSemaphore::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FontKernelTicket(u64);

impl FontKernelTicket {
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontKernelConsumerPath {
    RetainScene,
    Stamp,
}

impl FontKernelConsumerPath {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::RetainScene => "retain-scene",
            Self::Stamp => "stamp",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FontKernelConsumer {
    pub(crate) path: FontKernelConsumerPath,
    pub(crate) id: u64,
}

impl FontKernelConsumer {
    pub(crate) const fn new(path: FontKernelConsumerPath, id: u64) -> Self {
        Self { path, id }
    }
}

type FontKernelGpuSemaphore =
    FairSemaphore<crate::wait::EmbassySpinRawMutex, FONT_KERNEL_GPU_WAITERS>;

/// Exclusive, FIFO admission to Font Engine direct-RCS work.
///
/// The hardware path remains deliberately single-submit. Dropping the lease
/// hands the lane to the oldest asynchronous waiter.
pub(crate) struct FontKernelGpuLease {
    permit: Option<SemaphoreReleaser<'static, FontKernelGpuSemaphore>>,
    consumer: FontKernelConsumer,
}

impl Drop for FontKernelGpuLease {
    fn drop(&mut self) {
        {
            let mut status = STATUS.lock();
            if status.active_consumer == Some(self.consumer) {
                status.active_consumer = None;
            }
        }
        drop(self.permit.take());
    }
}

pub(crate) async fn acquire_gpu_lane(consumer: FontKernelConsumer) -> FontKernelGpuLease {
    let wait_started_ms = Instant::now().as_millis();
    let permit = if let Some(permit) = GPU_LANE.try_acquire(1) {
        permit
    } else {
        {
            let mut status = STATUS.lock();
            status.lane_contentions = status.lane_contentions.saturating_add(1);
            status.lane_waiters = status.lane_waiters.saturating_add(1);
            status.lane_peak_waiters = status.lane_peak_waiters.max(status.lane_waiters);
        }
        let permit = loop {
            match GPU_LANE.acquire(1).await {
                Ok(permit) => break permit,
                Err(_) => {
                    // The semaphore has bounded waiter storage. Recover if a
                    // burst temporarily exhausts that bookkeeping capacity.
                    Timer::after(EmbassyDuration::from_millis(FONT_KERNEL_LANE_RETRY_MS)).await;
                }
            }
        };
        {
            let mut status = STATUS.lock();
            status.lane_waiters = status.lane_waiters.saturating_sub(1);
        }
        permit
    };
    record_gpu_lane_admission(consumer, Instant::now().as_millis().saturating_sub(wait_started_ms));
    FontKernelGpuLease {
        permit: Some(permit),
        consumer,
    }
}

fn record_gpu_lane_admission(consumer: FontKernelConsumer, waited_ms: u64) {
    let mut status = STATUS.lock();
    status.active_consumer = Some(consumer);
    status.lane_admissions = status.lane_admissions.saturating_add(1);
    status.lane_wait_ms = status.lane_wait_ms.saturating_add(waited_ms);
    status.lane_wait_max_ms = status.lane_wait_max_ms.max(waited_ms);
    match consumer.path {
        FontKernelConsumerPath::RetainScene => {
            status.retain_lane_admissions = status.retain_lane_admissions.saturating_add(1);
        }
        FontKernelConsumerPath::Stamp => {
            status.stamp_lane_admissions = status.stamp_lane_admissions.saturating_add(1);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontKernelError {
    QueueFull,
    InvalidRequest(&'static str),
    Unavailable(&'static str),
    SubmittedIncomplete(&'static str),
}

impl From<GpuFontRetainedSceneError> for FontKernelError {
    fn from(error: GpuFontRetainedSceneError) -> Self {
        match error {
            GpuFontRetainedSceneError::Unavailable(reason) => Self::Unavailable(reason),
            GpuFontRetainedSceneError::SubmittedIncomplete(reason) => {
                Self::SubmittedIncomplete(reason)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetainedFontPositioning {
    SceneOrigin,
    VisualBoundsCenter,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainedFontRun {
    pub(crate) text: String,
    pub(crate) position: [f32; 2],
    pub(crate) font_pixels: f32,
    pub(crate) slant: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainSceneRequest {
    pub(crate) runs: Vec<RetainedFontRun>,
    pub(crate) font: GpuFontFace,
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
    pub(crate) raster_width: u32,
    pub(crate) raster_height: u32,
    pub(crate) positioning: RetainedFontPositioning,
}

#[derive(Clone, Debug)]
pub(crate) struct FontStampLayer {
    pub(crate) scene: RetainSceneRequest,
    pub(crate) foreground: GpuFontRgba,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontStampFit {
    /// Preserve the caller's complete raster, including transparent space.
    Canvas,
    /// Crop the returned allocation to the union of all generated coverage.
    Tight,
}

#[derive(Clone, Debug)]
pub(crate) struct FontStampRequest {
    pub(crate) layers: Vec<FontStampLayer>,
    pub(crate) fit: FontStampFit,
}

/// One logical retained scene backed by bounded analytical coverage masks.
///
/// Gridpaper uses the same low-level model: keep independently admitted R8
/// masks resident, then composite them together as one draw-time layer batch.
pub(crate) struct FontKernelRetainedScene {
    masks: Vec<GpuFontRetainedScene>,
}

impl FontKernelRetainedScene {
    pub(crate) fn masks(
        &self,
    ) -> impl Iterator<Item = Option<(crate::intel::gpgpu::GpgpuMask8Surface, [i32; 2])>> + '_ {
        self.masks
            .iter()
            .map(|mask| Some((mask.mask_surface()?, mask.origin_px()?)))
    }

    pub(crate) const fn mask_count(&self) -> usize {
        self.masks.len()
    }
}

/// GPU-visible RGBA output from one asynchronous stamp request.
pub(crate) struct FontStampedBuffer {
    ticket: FontKernelTicket,
    storage: crate::intel::gpgpu::GpgpuOwnedRgba8Surface,
    origin_px: [i32; 2],
    glyphs: usize,
    submits: usize,
    active_walkers: usize,
}

/// Completion metadata for a stamp written directly into a caller-owned UI4
/// frame. The release is bound to that exact allocation and is the only token
/// accepted by the frame pool for GPU-authored publication.
pub(crate) struct FontFrameStamp {
    ticket: FontKernelTicket,
    glyphs: usize,
    submits: usize,
    clear_submits: usize,
    active_walkers: usize,
    pre_service_ms: u64,
    clear_ms: u64,
    prepare_coverage_ms: u64,
    coverage_build_ms: u64,
    coverage_audit_ms: u64,
    coverage_submits: usize,
    instance_release_ms: u64,
    total_service_ms: u64,
    release: crate::intel::gpgpu::GpgpuRgba8ReleaseFence,
}

impl FontFrameStamp {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    pub(crate) const fn glyphs(&self) -> usize {
        self.glyphs
    }

    pub(crate) const fn submits(&self) -> usize {
        self.submits
    }

    pub(crate) const fn clear_submits(&self) -> usize {
        self.clear_submits
    }

    pub(crate) const fn active_walkers(&self) -> usize {
        self.active_walkers
    }

    /// Time from FIFO insertion until the blocking service worker began. This
    /// deliberately includes FIFO backlog, GPU-lane admission, and blocking
    /// worker dispatch; it is not presented as a pure queue measurement.
    pub(crate) const fn pre_service_ms(&self) -> u64 {
        self.pre_service_ms
    }

    pub(crate) const fn clear_ms(&self) -> u64 {
        self.clear_ms
    }

    pub(crate) const fn prepare_coverage_ms(&self) -> u64 {
        self.prepare_coverage_ms
    }

    /// Outline preparation, allocation, and analytical R8 GPU generation.
    pub(crate) const fn coverage_build_ms(&self) -> u64 {
        self.coverage_build_ms
    }

    /// CPU cache flush and full-mask nonzero integrity scan.
    pub(crate) const fn coverage_audit_ms(&self) -> u64 {
        self.coverage_audit_ms
    }

    pub(crate) const fn coverage_submits(&self) -> usize {
        self.coverage_submits
    }

    pub(crate) const fn instance_release_ms(&self) -> u64 {
        self.instance_release_ms
    }

    /// Worker time from optional clear admission through exact release proof.
    /// Pre-service delay is reported separately and is not included here.
    pub(crate) const fn total_service_ms(&self) -> u64 {
        self.total_service_ms
    }

    pub(crate) const fn release(&self) -> crate::intel::gpgpu::GpgpuRgba8ReleaseFence {
        self.release
    }
}

impl FontStampedBuffer {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    pub(crate) const fn surface(&self) -> crate::intel::gpgpu::GpgpuRgba8Surface {
        self.storage.surface()
    }

    /// Logical scene coordinate represented by output pixel (0, 0).
    pub(crate) const fn origin_px(&self) -> [i32; 2] {
        self.origin_px
    }

    pub(crate) const fn glyphs(&self) -> usize {
        self.glyphs
    }

    pub(crate) const fn submits(&self) -> usize {
        self.submits
    }

    pub(crate) const fn active_walkers(&self) -> usize {
        self.active_walkers
    }

    pub(crate) fn readback_tight_rgba(&self) -> Option<Vec<u8>> {
        self.storage.readback_tight_rgba()
    }
}

pub(crate) struct PendingRetainScene {
    ticket: FontKernelTicket,
    reply: Arc<
        Signal<crate::wait::EmbassySpinRawMutex, Result<FontKernelRetainedScene, FontKernelError>>,
    >,
}

impl PendingRetainScene {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    /// Take a completed retained scene without blocking the caller.
    ///
    /// VM-facing UI4 producers use this to turn the Embassy completion into a
    /// cooperative submit/poll boundary: the guest yields while the worker
    /// owns outline preparation and GPU coverage creation.
    pub(crate) fn try_take(&mut self) -> Option<Result<FontKernelRetainedScene, FontKernelError>> {
        self.reply.try_take()
    }

    pub(crate) async fn wait(self) -> Result<FontKernelRetainedScene, FontKernelError> {
        self.reply.wait().await
    }
}

pub(crate) struct PendingFontStamp {
    ticket: FontKernelTicket,
    reply:
        Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontStampedBuffer, FontKernelError>>>,
}

pub(crate) struct PendingFontFrameStamp {
    ticket: FontKernelTicket,
    queued_ahead: usize,
    reply: Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontFrameStamp, FontKernelError>>>,
}

impl PendingFontFrameStamp {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    /// Exact number of requests already resident in the service FIFO while
    /// this request was inserted. A separately dequeued active request is not
    /// included.
    pub(crate) const fn queued_ahead(&self) -> usize {
        self.queued_ahead
    }

    /// Take a completed direct-frame stamp without blocking the caller.
    ///
    /// Blueprint publishers use this as a cooperative submit/poll boundary
    /// while retaining the exact UI4 write lease targeted by the worker.
    pub(crate) fn try_take(&mut self) -> Option<Result<FontFrameStamp, FontKernelError>> {
        self.reply.try_take()
    }

    pub(crate) async fn wait(self) -> Result<FontFrameStamp, FontKernelError> {
        self.reply.wait().await
    }
}

impl PendingFontStamp {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    /// Take a completed one-shot stamp without blocking the caller.
    ///
    /// UI4 owns the returned RGBA allocation until its compositor submission
    /// has retired, so Blueprint VM calls can cooperatively submit and poll
    /// without copying the raster through guest or CPU memory.
    pub(crate) fn try_take(&mut self) -> Option<Result<FontStampedBuffer, FontKernelError>> {
        self.reply.try_take()
    }

    pub(crate) async fn wait(self) -> Result<FontStampedBuffer, FontKernelError> {
        self.reply.wait().await
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FontKernelServiceStatus {
    pub(crate) online: bool,
    pub(crate) active_ticket: Option<FontKernelTicket>,
    pub(crate) active_stage: &'static str,
    pub(crate) active_consumer: Option<FontKernelConsumer>,
    pub(crate) submitted_retain: u64,
    pub(crate) submitted_stamp: u64,
    pub(crate) completed_retain: u64,
    pub(crate) completed_stamp: u64,
    pub(crate) failed: u64,
    pub(crate) lane_retries: u64,
    pub(crate) gpu_retries: u64,
    pub(crate) lane_waiters: usize,
    pub(crate) lane_peak_waiters: usize,
    pub(crate) lane_admissions: u64,
    pub(crate) lane_contentions: u64,
    pub(crate) lane_wait_ms: u64,
    pub(crate) lane_wait_max_ms: u64,
    pub(crate) retain_lane_admissions: u64,
    pub(crate) stamp_lane_admissions: u64,
    pub(crate) queued: usize,
}

impl FontKernelServiceStatus {
    const fn new() -> Self {
        Self {
            online: false,
            active_ticket: None,
            active_stage: "idle",
            active_consumer: None,
            submitted_retain: 0,
            submitted_stamp: 0,
            completed_retain: 0,
            completed_stamp: 0,
            failed: 0,
            lane_retries: 0,
            gpu_retries: 0,
            lane_waiters: 0,
            lane_peak_waiters: 0,
            lane_admissions: 0,
            lane_contentions: 0,
            lane_wait_ms: 0,
            lane_wait_max_ms: 0,
            retain_lane_admissions: 0,
            stamp_lane_admissions: 0,
            queued: 0,
        }
    }
}

enum QueuedFontRequest {
    Retain {
        ticket: FontKernelTicket,
        request: RetainSceneRequest,
        reply: Arc<
            Signal<
                crate::wait::EmbassySpinRawMutex,
                Result<FontKernelRetainedScene, FontKernelError>,
            >,
        >,
    },
    Stamp {
        ticket: FontKernelTicket,
        request: FontStampRequest,
        reply: Arc<
            Signal<crate::wait::EmbassySpinRawMutex, Result<FontStampedBuffer, FontKernelError>>,
        >,
    },
    FrameStamp {
        ticket: FontKernelTicket,
        request: FontStampRequest,
        destination: crate::intel::gpgpu::GpgpuRgba8Surface,
        clear_rgba: Option<u32>,
        enqueued_ms: u64,
        reply:
            Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontFrameStamp, FontKernelError>>>,
    },
}

impl QueuedFontRequest {
    const fn ticket(&self) -> FontKernelTicket {
        match self {
            Self::Retain { ticket, .. }
            | Self::Stamp { ticket, .. }
            | Self::FrameStamp { ticket, .. } => *ticket,
        }
    }

    const fn consumer(&self) -> FontKernelConsumer {
        let path = match self {
            Self::Retain { .. } => FontKernelConsumerPath::RetainScene,
            Self::Stamp { .. } | Self::FrameStamp { .. } => FontKernelConsumerPath::Stamp,
        };
        FontKernelConsumer::new(path, self.ticket().raw())
    }
}

pub(crate) fn status() -> FontKernelServiceStatus {
    let mut status = *STATUS.lock();
    status.online = ONLINE.load(Ordering::Acquire);
    status.queued = REQUESTS.lock().len();
    status
}

pub(crate) fn submit_retain_scene(
    request: RetainSceneRequest,
) -> Result<PendingRetainScene, FontKernelError> {
    validate_retain_request(&request)?;
    let ticket = next_ticket();
    let reply = Arc::new(Signal::new());
    {
        let mut queue = REQUESTS.lock();
        if queue.len() >= FONT_KERNEL_QUEUE_CAPACITY {
            return Err(FontKernelError::QueueFull);
        }
        queue.push_back(QueuedFontRequest::Retain {
            ticket,
            request,
            reply: Arc::clone(&reply),
        });
    }
    {
        let mut status = STATUS.lock();
        status.submitted_retain = status.submitted_retain.saturating_add(1);
    }
    WORK_AVAILABLE.signal(());
    Ok(PendingRetainScene { ticket, reply })
}

pub(crate) fn submit_stamp(request: FontStampRequest) -> Result<PendingFontStamp, FontKernelError> {
    validate_stamp_request(&request)?;
    let ticket = next_ticket();
    let reply = Arc::new(Signal::new());
    {
        let mut queue = REQUESTS.lock();
        if queue.len() >= FONT_KERNEL_QUEUE_CAPACITY {
            return Err(FontKernelError::QueueFull);
        }
        queue.push_back(QueuedFontRequest::Stamp {
            ticket,
            request,
            reply: Arc::clone(&reply),
        });
    }
    {
        let mut status = STATUS.lock();
        status.submitted_stamp = status.submitted_stamp.saturating_add(1);
    }
    WORK_AVAILABLE.signal(());
    Ok(PendingFontStamp { ticket, reply })
}

/// Queue a stamp directly into one caller-owned RGBA8 surface.
///
/// Only canvas-fit requests are admitted because the destination extent and
/// ownership are fixed before submission. The caller must retain its write
/// lease until the returned exact-surface release is published or discarded.
pub(crate) fn submit_frame_stamp(
    request: FontStampRequest,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<PendingFontFrameStamp, FontKernelError> {
    queue_frame_stamp(request, destination, None)
}

/// Queue a full-surface clear followed by an ordered stamp into one caller-owned
/// RGBA8 surface. Both operations execute while the same font GPU-lane lease is
/// held. A submitted-but-incomplete clear is reported as `SubmittedIncomplete`
/// so the caller can quarantine the exact destination instead of cancelling it.
pub(crate) fn submit_frame_stamp_with_clear(
    request: FontStampRequest,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    clear_rgba: u32,
) -> Result<PendingFontFrameStamp, FontKernelError> {
    queue_frame_stamp(request, destination, Some(clear_rgba))
}

fn queue_frame_stamp(
    request: FontStampRequest,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    clear_rgba: Option<u32>,
) -> Result<PendingFontFrameStamp, FontKernelError> {
    validate_stamp_request(&request)?;
    let scene = &request.layers[0].scene;
    if request.fit != FontStampFit::Canvas
        || !destination.is_valid()
        || destination.width != scene.raster_width
        || destination.height != scene.raster_height
    {
        return Err(FontKernelError::InvalidRequest("font-frame-stamp-destination"));
    }
    let ticket = next_ticket();
    let reply = Arc::new(Signal::new());
    let queued_ahead = {
        let mut queue = REQUESTS.lock();
        if queue.len() >= FONT_KERNEL_QUEUE_CAPACITY {
            return Err(FontKernelError::QueueFull);
        }
        let queued_ahead = queue.len();
        queue.push_back(QueuedFontRequest::FrameStamp {
            ticket,
            request,
            destination,
            clear_rgba,
            enqueued_ms: Instant::now().as_millis(),
            reply: Arc::clone(&reply),
        });
        queued_ahead
    };
    {
        let mut status = STATUS.lock();
        status.submitted_stamp = status.submitted_stamp.saturating_add(1);
    }
    WORK_AVAILABLE.signal(());
    Ok(PendingFontFrameStamp {
        ticket,
        queued_ahead,
        reply,
    })
}

fn next_ticket() -> FontKernelTicket {
    loop {
        let current = NEXT_TICKET.fetch_add(1, Ordering::AcqRel);
        if current != 0 {
            return FontKernelTicket(current);
        }
    }
}

fn validate_retain_request(request: &RetainSceneRequest) -> Result<(), FontKernelError> {
    validate_scene_request(request, FONT_KERNEL_MAX_RUNS)
}

fn validate_scene_request(
    request: &RetainSceneRequest,
    max_runs: usize,
) -> Result<(), FontKernelError> {
    if request.runs.is_empty() || request.runs.len() > max_runs {
        return Err(FontKernelError::InvalidRequest("font-service-run-count"));
    }
    if request.viewport_width == 0
        || request.viewport_height == 0
        || request.raster_width == 0
        || request.raster_height == 0
    {
        return Err(FontKernelError::InvalidRequest("font-service-empty-extent"));
    }
    for run in &request.runs {
        let chars = run.text.chars().count();
        let max_chars = if request.positioning == RetainedFontPositioning::SceneOrigin {
            FONT_STAMP_MAX_GLYPHS
        } else {
            MAX_DYNAMIC_TEXT_CHARS
        };
        if chars == 0 || chars > max_chars {
            return Err(FontKernelError::InvalidRequest("font-service-text-length"));
        }
        if run.text.chars().any(char::is_control)
            || !run.position[0].is_finite()
            || !run.position[1].is_finite()
            || !run.font_pixels.is_finite()
            || run.font_pixels <= 0.0
            || !run.slant.is_finite()
            || run.slant.abs() > 1.0
        {
            return Err(FontKernelError::InvalidRequest("font-service-run"));
        }
    }
    Ok(())
}

fn validate_stamp_request(request: &FontStampRequest) -> Result<(), FontKernelError> {
    if request.layers.is_empty() || request.layers.len() > FONT_KERNEL_MAX_STAMP_LAYERS {
        return Err(FontKernelError::InvalidRequest("font-stamp-layer-count"));
    }
    let first = &request.layers[0].scene;
    if first.raster_width > FONT_STAMP_MAX_EXTENT
        || first.raster_height > FONT_STAMP_MAX_EXTENT
        || u64::from(first.raster_width) * u64::from(first.raster_height) > FONT_STAMP_MAX_PIXELS
    {
        return Err(FontKernelError::InvalidRequest("font-stamp-extent-softcap"));
    }
    let mut glyphs = 0usize;
    let mut runs = 0usize;
    for layer in &request.layers {
        validate_scene_request(&layer.scene, FONT_STAMP_MAX_GLYPHS)?;
        if layer.scene.viewport_width != first.viewport_width
            || layer.scene.viewport_height != first.viewport_height
            || layer.scene.raster_width != first.raster_width
            || layer.scene.raster_height != first.raster_height
            || layer.scene.raster_width > FONT_STAMP_MAX_EXTENT
            || layer.scene.raster_height > FONT_STAMP_MAX_EXTENT
        {
            return Err(FontKernelError::InvalidRequest("font-stamp-layer-extent"));
        }
        runs = runs
            .checked_add(layer.scene.runs.len())
            .ok_or(FontKernelError::InvalidRequest("font-stamp-run-softcap"))?;
        if runs > FONT_STAMP_MAX_GLYPHS {
            return Err(FontKernelError::InvalidRequest("font-stamp-run-softcap"));
        }
        for run in &layer.scene.runs {
            glyphs = glyphs
                .checked_add(run.text.chars().count())
                .ok_or(FontKernelError::InvalidRequest("font-stamp-glyph-softcap"))?;
            if glyphs > FONT_STAMP_MAX_GLYPHS {
                return Err(FontKernelError::InvalidRequest("font-stamp-glyph-softcap"));
            }
        }
    }
    Ok(())
}

fn set_active_stage(ticket: FontKernelTicket, stage: &'static str) {
    let mut status = STATUS.lock();
    status.active_ticket = Some(ticket);
    status.active_stage = stage;
}

fn process_retain_scene(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
) -> Result<FontKernelRetainedScene, FontKernelError> {
    let glyph_runs = expand_origin_runs(ticket, request)?;
    let mut masks = Vec::new();
    process_retain_scene_partition(ticket, request, glyph_runs.as_slice(), &mut masks)?;
    Ok(FontKernelRetainedScene { masks })
}

fn expand_origin_runs(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
) -> Result<Vec<RetainedFontRun>, FontKernelError> {
    if request.positioning != RetainedFontPositioning::SceneOrigin {
        return Ok(request.runs.clone());
    }

    set_active_stage(ticket, "font-layout");
    ensure_font_face_available(request.font).map_err(FontKernelError::Unavailable)?;
    let mut glyph_runs = Vec::new();
    for run in &request.runs {
        let mut pen_x = 0.0f32;
        for ch in run.text.chars() {
            let mut glyph = String::new();
            glyph.push(ch);
            let advance = crate::graphics::font::text_advance_width(
                request.font.registry_name(),
                glyph.as_str(),
                run.font_pixels,
            )
            .map_err(FontKernelError::Unavailable)?;
            if !ch.is_whitespace() && font_face_supports_text(request.font, glyph.as_str()) {
                glyph_runs.push(RetainedFontRun {
                    text: glyph,
                    position: [run.position[0] + pen_x, run.position[1]],
                    font_pixels: run.font_pixels,
                    slant: run.slant,
                });
            }
            pen_x += advance;
        }
    }
    if glyph_runs.is_empty() {
        return Err(FontKernelError::Unavailable("font-coverage-empty"));
    }
    crate::log_info!(
        target: "global";
        "font-kernel-service: bounded glyph layout ticket={} source_runs={} glyph_entries={} positioning=scene-origin policy=per-glyph-analytical-coverage\n",
        ticket.raw(),
        request.runs.len(),
        glyph_runs.len(),
    );
    Ok(glyph_runs)
}

fn process_retain_scene_partition(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
    runs: &[RetainedFontRun],
    masks: &mut Vec<GpuFontRetainedScene>,
) -> Result<(), FontKernelError> {
    match process_retain_scene_runs(ticket, request, runs) {
        Ok(mask) => {
            masks.push(mask);
            Ok(())
        }
        Err(FontKernelError::Unavailable("font-coverage-workload"))
            if runs.len() > 1 && request.positioning == RetainedFontPositioning::SceneOrigin =>
        {
            let midpoint = runs.len() / 2;
            if ticket.raw()
                > LAST_RETAIN_PARTITION_LOG_TICKET.fetch_max(ticket.raw(), Ordering::Relaxed)
            {
                crate::log_info!(
                    target: "global";
                    "font-kernel-service: retain partition ticket={} runs={} split={}+{} reason=font-coverage-workload storage=gpu-vm-r8-layers\n",
                    ticket.raw(),
                    runs.len(),
                    midpoint,
                    runs.len().saturating_sub(midpoint),
                );
            }
            process_retain_scene_partition(ticket, request, &runs[..midpoint], masks)?;
            process_retain_scene_partition(ticket, request, &runs[midpoint..], masks)
        }
        Err(error) => Err(error),
    }
}

fn process_retain_scene_runs(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
    runs: &[RetainedFontRun],
) -> Result<GpuFontRetainedScene, FontKernelError> {
    set_active_stage(ticket, "font-warm");
    ensure_font_face_available(request.font).map_err(FontKernelError::Unavailable)?;
    set_active_stage(ticket, "coverage");
    let entries = runs
        .iter()
        .map(|run| GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine(run.text.as_str()),
            position: run.position,
            font_pixels: run.font_pixels,
            slant: run.slant,
        })
        .collect::<Vec<_>>();
    let result = match request.positioning {
        RetainedFontPositioning::SceneOrigin => retain_gpu_font_scene_at_raster(
            entries.as_slice(),
            request.font,
            request.viewport_width,
            request.viewport_height,
            request.raster_width,
            request.raster_height,
        ),
        RetainedFontPositioning::VisualBoundsCenter => retain_gpu_font_centered_scene_at_raster(
            entries.as_slice(),
            request.font,
            request.viewport_width,
            request.viewport_height,
            request.raster_width,
            request.raster_height,
        ),
    };
    result.map_err(FontKernelError::Unavailable)
}

fn collect_stamp_scenes(
    ticket: FontKernelTicket,
    layer: &FontStampLayer,
    runs: &[RetainedFontRun],
    scenes: &mut Vec<(GpuFontRetainedScene, GpuFontRgba)>,
) -> Result<(), FontKernelError> {
    let scene = match process_retain_scene_runs(ticket, &layer.scene, runs) {
        Ok(scene) => scene,
        Err(FontKernelError::Unavailable("font-coverage-workload"))
            if runs.len() > 1
                && layer.scene.positioning == RetainedFontPositioning::SceneOrigin =>
        {
            let midpoint = runs.len() / 2;
            if ticket.raw()
                > LAST_STAMP_PARTITION_LOG_TICKET.fetch_max(ticket.raw(), Ordering::Relaxed)
            {
                crate::log_info!(
                    target: "global";
                    "font-kernel-service: stamp partition ticket={} runs={} split={}+{} reason=font-coverage-workload destination=gpu-vm-rgba8\n",
                    ticket.raw(),
                    runs.len(),
                    midpoint,
                    runs.len().saturating_sub(midpoint),
                );
            }
            collect_stamp_scenes(ticket, layer, &runs[..midpoint], scenes)?;
            return collect_stamp_scenes(ticket, layer, &runs[midpoint..], scenes);
        }
        Err(error) => return Err(error),
    };
    scenes.push((scene, layer.foreground));
    Ok(())
}

fn tight_stamp_bounds(
    scenes: &[(GpuFontRetainedScene, GpuFontRgba)],
) -> Result<([i32; 2], u32, u32), FontKernelError> {
    let mut union: Option<(i64, i64, i64, i64)> = None;
    for (scene, _) in scenes {
        let origin = scene
            .origin_px()
            .ok_or(FontKernelError::Unavailable("font-stamp-mask-origin"))?;
        let mask = scene
            .mask_surface()
            .ok_or(FontKernelError::Unavailable("font-stamp-mask-surface"))?;
        let bounds = (
            i64::from(origin[0]),
            i64::from(origin[1]),
            i64::from(origin[0]) + i64::from(mask.width),
            i64::from(origin[1]) + i64::from(mask.height),
        );
        union = Some(match union {
            Some(current) => (
                current.0.min(bounds.0),
                current.1.min(bounds.1),
                current.2.max(bounds.2),
                current.3.max(bounds.3),
            ),
            None => bounds,
        });
    }
    let (left, top, right, bottom) =
        union.ok_or(FontKernelError::Unavailable("font-stamp-empty"))?;
    let width = u32::try_from(right - left)
        .map_err(|_| FontKernelError::InvalidRequest("font-stamp-extent-softcap"))?;
    let height = u32::try_from(bottom - top)
        .map_err(|_| FontKernelError::InvalidRequest("font-stamp-extent-softcap"))?;
    if width == 0
        || height == 0
        || width > FONT_STAMP_MAX_EXTENT
        || height > FONT_STAMP_MAX_EXTENT
        || u64::from(width) * u64::from(height) > FONT_STAMP_MAX_PIXELS
    {
        return Err(FontKernelError::InvalidRequest("font-stamp-extent-softcap"));
    }
    let origin = [
        i32::try_from(left)
            .map_err(|_| FontKernelError::InvalidRequest("font-stamp-origin-range"))?,
        i32::try_from(top)
            .map_err(|_| FontKernelError::InvalidRequest("font-stamp-origin-range"))?,
    ];
    Ok((origin, width, height))
}

fn prepare_stamp_scenes(
    ticket: FontKernelTicket,
    request: &FontStampRequest,
) -> Result<(Vec<(GpuFontRetainedScene, GpuFontRgba)>, usize), FontKernelError> {
    let mut scenes = Vec::new();
    let mut glyphs = 0usize;
    for layer in &request.layers {
        glyphs = layer
            .scene
            .runs
            .iter()
            .fold(glyphs, |total, run| total.saturating_add(run.text.chars().count()));
        let glyph_runs = expand_origin_runs(ticket, &layer.scene)?;
        collect_stamp_scenes(ticket, layer, glyph_runs.as_slice(), &mut scenes)?;
    }
    Ok((scenes, glyphs))
}

fn process_stamp(
    ticket: FontKernelTicket,
    request: &FontStampRequest,
) -> Result<FontStampedBuffer, FontKernelError> {
    let (scenes, glyphs) = prepare_stamp_scenes(ticket, request)?;
    let (origin_px, width, height) = match request.fit {
        FontStampFit::Canvas => {
            let scene = &request.layers[0].scene;
            ([0, 0], scene.raster_width, scene.raster_height)
        }
        FontStampFit::Tight => tight_stamp_bounds(scenes.as_slice())?,
    };
    set_active_stage(ticket, "output-allocate");
    let storage = crate::intel::gpgpu::allocate_font_instance_rgba8_surface(width, height)
        .ok_or(FontKernelError::Unavailable("font-stamp-output-allocation"))?;
    let surface = storage.surface();
    // The owned RGBA allocation is zeroed and DMA-flushed before return.
    // Dispatching another GPU clear here only adds direct-RCS contention.
    let translation = [-origin_px[0] as f32, -origin_px[1] as f32];
    let mut submits = 0usize;
    let mut active_walkers = 0usize;
    for (scene, foreground) in scenes {
        set_active_stage(ticket, "instance");
        let mut style = GpuFontRetainedStyle::identity(foreground);
        style.translation_px = translation;
        let rendered = match scene.restamp_instance(surface, style, false, 0.0) {
            Ok(rendered) => rendered,
            Err(error) => {
                let error = FontKernelError::from(error);
                if matches!(error, FontKernelError::SubmittedIncomplete(_)) {
                    core::mem::forget(storage);
                }
                return Err(error);
            }
        };
        submits = submits.saturating_add(rendered.submits);
        active_walkers = active_walkers.saturating_add(rendered.active_walkers);
    }
    Ok(FontStampedBuffer {
        ticket,
        storage,
        origin_px,
        glyphs,
        submits,
        active_walkers,
    })
}

fn validate_frame_clear_outcome(
    outcome: crate::intel::gpgpu::GpgpuSubmissionOutcome,
) -> Result<(), FontKernelError> {
    match outcome {
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Complete => Ok(()),
        crate::intel::gpgpu::GpgpuSubmissionOutcome::SubmittedIncomplete => {
            Err(FontKernelError::SubmittedIncomplete("font-frame-clear-submit-incomplete"))
        }
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Unavailable => {
            Err(FontKernelError::Unavailable("font-frame-clear-unavailable"))
        }
    }
}

fn process_frame_stamp(
    ticket: FontKernelTicket,
    request: &FontStampRequest,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    clear_rgba: Option<u32>,
    enqueued_ms: u64,
) -> Result<FontFrameStamp, FontKernelError> {
    use crate::intel::gpgpu::GpgpuSolidRect;

    let service_started_ms = Instant::now().as_millis();
    let pre_service_ms = service_started_ms.saturating_sub(enqueued_ms);
    let mut clear_submits = 0usize;
    let clear_ms = if let Some(color_rgba) = clear_rgba {
        set_active_stage(ticket, "frame-clear");
        let clear_started_ms = Instant::now().as_millis();
        let clear = GpgpuSolidRect {
            rect: destination.bounds(),
            color_rgba,
        };
        let cleared =
            crate::intel::gpgpu::font_fill_solid_rect_rgba8_scanout_result(destination, clear);
        clear_submits = cleared.stats.submits;
        let elapsed_ms = Instant::now().as_millis().saturating_sub(clear_started_ms);
        validate_frame_clear_outcome(cleared.outcome)?;
        elapsed_ms
    } else {
        0
    };

    set_active_stage(ticket, "frame-prepare-coverage");
    let prepare_started_ms = Instant::now().as_millis();
    let (scenes, glyphs) = prepare_stamp_scenes(ticket, request)?;
    let prepare_coverage_ms = Instant::now()
        .as_millis()
        .saturating_sub(prepare_started_ms);
    let coverage_build_ms = scenes
        .iter()
        .fold(0u64, |total, (scene, _)| total.saturating_add(scene.coverage_build_ms()));
    let coverage_audit_ms = scenes
        .iter()
        .fold(0u64, |total, (scene, _)| total.saturating_add(scene.coverage_audit_ms()));
    let coverage_submits = scenes
        .iter()
        .fold(0usize, |total, (scene, _)| total.saturating_add(scene.coverage_submits()));
    let scene_count = scenes.len();
    let mut submits = 0usize;
    let mut active_walkers = 0usize;
    let mut release = None;
    let instance_started_ms = Instant::now().as_millis();
    for (index, (scene, foreground)) in scenes.into_iter().enumerate() {
        set_active_stage(ticket, "frame-instance");
        let rendered = scene.restamp_instance(
            destination,
            GpuFontRetainedStyle::identity(foreground),
            index + 1 == scene_count,
            0.0,
        )?;
        submits = submits.saturating_add(rendered.submits);
        active_walkers = active_walkers.saturating_add(rendered.active_walkers);
        if rendered.release.is_some() {
            release = rendered.release;
        }
    }
    let release =
        release.ok_or(FontKernelError::Unavailable("font-frame-stamp-release-missing"))?;
    let completed_ms = Instant::now().as_millis();
    Ok(FontFrameStamp {
        ticket,
        glyphs,
        submits,
        clear_submits,
        active_walkers,
        pre_service_ms,
        clear_ms,
        prepare_coverage_ms,
        coverage_build_ms,
        coverage_audit_ms,
        coverage_submits,
        instance_release_ms: completed_ms.saturating_sub(instance_started_ms),
        total_service_ms: completed_ms.saturating_sub(service_started_ms),
        release,
    })
}

fn complete_status(ticket: FontKernelTicket, retain: bool, succeeded: bool) {
    let mut status = STATUS.lock();
    if succeeded {
        if retain {
            status.completed_retain = status.completed_retain.saturating_add(1);
        } else {
            status.completed_stamp = status.completed_stamp.saturating_add(1);
        }
    } else {
        status.failed = status.failed.saturating_add(1);
    }
    if status.active_ticket == Some(ticket) {
        status.active_ticket = None;
        status.active_stage = "idle";
    }
}

fn log_failure(ticket: FontKernelTicket, operation: &'static str, error: &FontKernelError) {
    let stage = STATUS.lock().active_stage;
    let queued = REQUESTS.lock().len();
    crate::log_warn!(
        target: "global";
        "font-kernel-service: {} failed ticket={} stage={} reason={:?} queued={} action=signal-caller+keep-service-online\n",
        operation,
        ticket.raw(),
        stage,
        error,
        queued,
    );
}

fn retryable_gpu_error(error: &FontKernelError) -> bool {
    !crate::intel::gpgpu::font_rcs_context_is_quarantined()
        && matches!(
            error,
            FontKernelError::Unavailable(
                "font-coverage-dispatch"
                    | "font-retained-identity-restamp-unavailable"
                    | "font-retained-instance-restamp-unavailable"
            )
        )
}

fn record_gpu_retry(ticket: FontKernelTicket, operation: &'static str, error: &FontKernelError) {
    let retry = {
        let mut status = STATUS.lock();
        status.gpu_retries = status.gpu_retries.saturating_add(1);
        if status.active_ticket == Some(ticket) {
            status.active_ticket = None;
            status.active_stage = "idle";
        }
        status.gpu_retries
    };
    if retry <= 8 || retry.is_multiple_of(120) {
        crate::log_info!(
            target: "render";
            "font-kernel-service: {} deferred ticket={} reason={:?} gpu_retry={} queued={} retry_ms={} action=requeue-ticket+pace-font-lane\n",
            operation,
            ticket.raw(),
            error,
            retry,
            REQUESTS.lock().len().saturating_add(1),
            FONT_KERNEL_GPU_RETRY_MS,
        );
    }
}

fn process_queued_request(request: QueuedFontRequest) {
    match request {
        QueuedFontRequest::Retain {
            ticket,
            request,
            reply,
        } => {
            set_active_stage(ticket, "dispatch");
            let result = process_retain_scene(ticket, &request);
            if let Err(error) = &result
                && retryable_gpu_error(error)
            {
                record_gpu_retry(ticket, "retain", error);
                GPU_RETRY_DELAY_PENDING.store(true, Ordering::Release);
                REQUESTS.lock().push_back(QueuedFontRequest::Retain {
                    ticket,
                    request,
                    reply,
                });
                IN_FLIGHT.store(false, Ordering::Release);
                WORK_AVAILABLE.signal(());
                return;
            }
            if let Err(error) = &result {
                log_failure(ticket, "retain", error);
            }
            complete_status(ticket, true, result.is_ok());
            crate::log_info!(
                target: "render";
                "font-kernel-service: retain complete ticket={} ok={} queued={}\n",
                ticket.raw(),
                result.is_ok() as u8,
                REQUESTS.lock().len(),
            );
            reply.signal(result);
        }
        QueuedFontRequest::Stamp {
            ticket,
            request,
            reply,
        } => {
            set_active_stage(ticket, "dispatch");
            let result = process_stamp(ticket, &request);
            if let Err(error) = &result
                && retryable_gpu_error(error)
            {
                record_gpu_retry(ticket, "stamp", error);
                GPU_RETRY_DELAY_PENDING.store(true, Ordering::Release);
                REQUESTS.lock().push_back(QueuedFontRequest::Stamp {
                    ticket,
                    request,
                    reply,
                });
                IN_FLIGHT.store(false, Ordering::Release);
                WORK_AVAILABLE.signal(());
                return;
            }
            if let Err(error) = &result {
                log_failure(ticket, "stamp", error);
            }
            complete_status(ticket, false, result.is_ok());
            crate::log_info!(
                target: "render";
                "font-kernel-service: stamp complete ticket={} ok={} queued={}\n",
                ticket.raw(),
                result.is_ok() as u8,
                REQUESTS.lock().len(),
            );
            reply.signal(result);
        }
        QueuedFontRequest::FrameStamp {
            ticket,
            request,
            destination,
            clear_rgba,
            enqueued_ms,
            reply,
        } => {
            set_active_stage(ticket, "dispatch");
            let result =
                process_frame_stamp(ticket, &request, destination, clear_rgba, enqueued_ms);
            // A destination stamp is not replayed: an earlier ordered layer
            // may already have retired into the leased frame, so retrying the
            // whole source-over sequence would composite it twice.
            if let Err(error) = &result {
                log_failure(ticket, "frame-stamp", error);
            }
            complete_status(ticket, false, result.is_ok());
            crate::log_info!(
                target: "render";
                "font-kernel-service: frame-stamp complete ticket={} ok={} queued={}\n",
                ticket.raw(),
                result.is_ok() as u8,
                REQUESTS.lock().len(),
            );
            reply.signal(result);
        }
    }
    IN_FLIGHT.store(false, Ordering::Release);
    WORK_AVAILABLE.signal(());
}

fn dispatch_to_service_lane(
    request: QueuedFontRequest,
    gpu_lane: FontKernelGpuLease,
) -> Result<(), QueuedFontRequest> {
    let shared_request = Arc::new(Mutex::new(Some(request)));
    let worker_request = Arc::clone(&shared_request);
    let job = Box::new(move || {
        let _gpu_lane = gpu_lane;
        if let Some(request) = worker_request.lock().take() {
            process_queued_request(request);
        }
    });
    match crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "font-kernel-service") {
        Ok(()) => Ok(()),
        Err(job) => {
            drop(job);
            Err(shared_request
                .lock()
                .take()
                .expect("rejected font service-lane job retained its request"))
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn font_kernel_service_task() {
    ONLINE.store(true, Ordering::Release);
    crate::log_info!(
        target: "render";
        "font-kernel-service: online paths=retain-scene+async-stamp+async-frame-stamp controller=bsp worker=leased-blocking-service-lane font_lane=fair-fifo-font-only gpu_context=kernel-gpgpu-font queue_capacity={} retained_storage=gpu-vm-r8 stamp_output=owned-or-ui4-leased-gpu-vm-rgba8 completion=signal\n",
        FONT_KERNEL_QUEUE_CAPACITY,
    );
    loop {
        if GPU_RETRY_DELAY_PENDING.swap(false, Ordering::AcqRel) {
            Timer::after(EmbassyDuration::from_millis(FONT_KERNEL_GPU_RETRY_MS)).await;
        }
        if IN_FLIGHT.load(Ordering::Acquire) {
            WORK_AVAILABLE.wait().await;
            continue;
        }
        let Some(request) = REQUESTS.lock().pop_front() else {
            WORK_AVAILABLE.wait().await;
            continue;
        };
        let ticket = request.ticket();
        set_active_stage(ticket, "lane-admission");
        let consumer = request.consumer();
        let gpu_lane = acquire_gpu_lane(consumer).await;
        IN_FLIGHT.store(true, Ordering::Release);
        if let Err(request) = dispatch_to_service_lane(request, gpu_lane) {
            IN_FLIGHT.store(false, Ordering::Release);
            REQUESTS.lock().push_front(request);
            {
                let mut status = STATUS.lock();
                status.active_stage = "lane-wait";
                status.lane_retries = status.lane_retries.saturating_add(1);
            }
            Timer::after(EmbassyDuration::from_millis(FONT_KERNEL_LANE_RETRY_MS)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RetainSceneRequest {
        RetainSceneRequest {
            runs: alloc::vec![RetainedFontRun {
                text: String::from("retained"),
                position: [20.0, 30.0],
                font_pixels: 24.0,
                slant: 0.0,
            }],
            font: GpuFontFace::Default,
            viewport_width: 256,
            viewport_height: 128,
            raster_width: 256,
            raster_height: 128,
            positioning: RetainedFontPositioning::SceneOrigin,
        }
    }

    #[test]
    fn retained_request_accepts_owned_runs() {
        assert_eq!(validate_retain_request(&request()), Ok(()));
    }

    #[test]
    fn retained_request_rejects_control_text_and_empty_extent() {
        let mut invalid_text = request();
        invalid_text.runs[0].text = String::from("bad\nrun");
        assert_eq!(
            validate_retain_request(&invalid_text),
            Err(FontKernelError::InvalidRequest("font-service-run"))
        );

        let mut invalid_extent = request();
        invalid_extent.raster_width = 0;
        assert_eq!(
            validate_retain_request(&invalid_extent),
            Err(FontKernelError::InvalidRequest("font-service-empty-extent"))
        );
    }

    #[test]
    fn stamp_contract_accepts_layers_and_enforces_glyph_and_4k_caps() {
        let mut stamp = FontStampRequest {
            layers: alloc::vec![FontStampLayer {
                scene: request(),
                foreground: GpuFontRgba::new(255, 255, 255, 255),
            }],
            fit: FontStampFit::Tight,
        };
        assert_eq!(validate_stamp_request(&stamp), Ok(()));

        stamp.layers[0].scene.runs[0].text = "x".repeat(FONT_STAMP_MAX_GLYPHS + 1);
        assert_eq!(
            validate_stamp_request(&stamp),
            Err(FontKernelError::InvalidRequest("font-service-text-length"))
        );

        stamp.layers[0].scene.runs[0].text = String::from("x");
        stamp.layers[0].scene.raster_width = FONT_STAMP_MAX_EXTENT;
        stamp.layers[0].scene.viewport_width = FONT_STAMP_MAX_EXTENT;
        stamp.layers[0].scene.raster_height = FONT_STAMP_MAX_EXTENT;
        stamp.layers[0].scene.viewport_height = FONT_STAMP_MAX_EXTENT;
        assert_eq!(
            validate_stamp_request(&stamp),
            Err(FontKernelError::InvalidRequest("font-stamp-extent-softcap"))
        );
    }

    #[test]
    fn transient_gpu_dispatch_failures_are_retried() {
        assert!(retryable_gpu_error(&FontKernelError::Unavailable("font-coverage-dispatch")));
        assert!(retryable_gpu_error(&FontKernelError::Unavailable(
            "font-retained-instance-restamp-unavailable"
        )));
        assert!(!retryable_gpu_error(&FontKernelError::Unavailable(
            "font-stamp-output-allocation"
        )));
        assert!(!retryable_gpu_error(&FontKernelError::SubmittedIncomplete(
            "font-retained-instance-submit-incomplete"
        )));
    }

    #[test]
    fn frame_clear_preserves_submission_boundary_failures() {
        use crate::intel::gpgpu::GpgpuSubmissionOutcome;

        assert_eq!(validate_frame_clear_outcome(GpgpuSubmissionOutcome::Complete), Ok(()));
        assert_eq!(
            validate_frame_clear_outcome(GpgpuSubmissionOutcome::Unavailable),
            Err(FontKernelError::Unavailable("font-frame-clear-unavailable"))
        );
        assert_eq!(
            validate_frame_clear_outcome(GpgpuSubmissionOutcome::SubmittedIncomplete),
            Err(FontKernelError::SubmittedIncomplete("font-frame-clear-submit-incomplete"))
        );
    }

    #[test]
    fn consumer_paths_keep_independent_identity() {
        let retain = FontKernelConsumer::new(FontKernelConsumerPath::RetainScene, 1);
        let other_retain = FontKernelConsumer::new(FontKernelConsumerPath::RetainScene, 2);
        let stamp = FontKernelConsumer::new(FontKernelConsumerPath::Stamp, 1);
        assert_ne!(retain, other_retain);
        assert_ne!(retain, stamp);
        assert_eq!(retain.path.name(), "retain-scene");
        assert_eq!(stamp.path.name(), "stamp");
    }
}
