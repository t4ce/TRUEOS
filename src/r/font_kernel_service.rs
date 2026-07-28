//! Shared multi-consumer GPU-font service.
//!
//! `RetainScene` yields GPU-VM-resident Skrifa coverage that a caller may
//! restamp repeatedly. `Stamp` is the one-shot path: the worker creates the
//! same retained representation temporarily, composites it into a new
//! GPU-visible RGBA buffer, and returns that owned buffer asynchronously.
//! Gridpaper page, cell-patch, presentation, and print work use the same fair
//! hardware admission lane while retaining independent runtime state.

use alloc::{boxed::Box, collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_sync::semaphore::{FairSemaphore, Semaphore, SemaphoreReleaser};
use embassy_sync::signal::Signal;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::intel::gpu_font::{
    GpuFontFace, GpuFontJobEntry, GpuFontRetainedScene, GpuFontRetainedSceneError,
    GpuFontRetainedStyle, GpuFontRgba, GpuFontTextRequest, MAX_DYNAMIC_TEXT_CHARS,
    ensure_font_face_available, retain_gpu_font_centered_scene_at_raster,
    retain_gpu_font_scene_at_raster,
};

const FONT_KERNEL_QUEUE_CAPACITY: usize = 32;
const FONT_KERNEL_MAX_RUNS: usize = 64;
const FONT_KERNEL_LANE_RETRY_MS: u64 = 2;
const FONT_KERNEL_GPU_WAITERS: usize = 32;

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
static ONLINE: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
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
    GridPage,
    GridCellPatch,
    GridPresent,
    GridPrint,
}

impl FontKernelConsumerPath {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::RetainScene => "retain-scene",
            Self::Stamp => "stamp",
            Self::GridPage => "grid-page",
            Self::GridCellPatch => "grid-cell-patch",
            Self::GridPresent => "grid-present",
            Self::GridPrint => "grid-print",
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

/// Exclusive, FIFO admission to GPU font coverage and presentation work.
///
/// The hardware path remains deliberately single-submit, but every font
/// producer enters through this multi-waiter boundary. Dropping the lease
/// hands the lane to the oldest waiter.
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
                    // Capacity is deliberately larger than every resident
                    // Gridpaper worker plus the retained/stamp controller.
                    // Still recover if that invariant is temporarily exceeded.
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
    {
        let waited_ms = Instant::now().as_millis().saturating_sub(wait_started_ms);
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
            FontKernelConsumerPath::GridPage => {
                status.grid_page_lane_admissions =
                    status.grid_page_lane_admissions.saturating_add(1);
            }
            FontKernelConsumerPath::GridCellPatch => {
                status.grid_patch_lane_admissions =
                    status.grid_patch_lane_admissions.saturating_add(1);
            }
            FontKernelConsumerPath::GridPresent => {
                status.grid_present_lane_admissions =
                    status.grid_present_lane_admissions.saturating_add(1);
            }
            FontKernelConsumerPath::GridPrint => {
                status.grid_print_lane_admissions =
                    status.grid_print_lane_admissions.saturating_add(1);
            }
        }
    }
    FontKernelGpuLease {
        permit: Some(permit),
        consumer,
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
pub(crate) struct FontStampRequest {
    pub(crate) scene: RetainSceneRequest,
    pub(crate) foreground: GpuFontRgba,
}

/// GPU-visible RGBA output from one asynchronous stamp request.
pub(crate) struct FontStampedBuffer {
    ticket: FontKernelTicket,
    storage: crate::intel::gpgpu::GpgpuOwnedRgba8Surface,
    submits: usize,
    active_walkers: usize,
}

impl FontStampedBuffer {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
    }

    pub(crate) const fn surface(&self) -> crate::intel::gpgpu::GpgpuRgba8Surface {
        self.storage.surface()
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
        Signal<crate::wait::EmbassySpinRawMutex, Result<GpuFontRetainedScene, FontKernelError>>,
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
    pub(crate) fn try_take(&mut self) -> Option<Result<GpuFontRetainedScene, FontKernelError>> {
        self.reply.try_take()
    }

    pub(crate) async fn wait(self) -> Result<GpuFontRetainedScene, FontKernelError> {
        self.reply.wait().await
    }
}

pub(crate) struct PendingFontStamp {
    ticket: FontKernelTicket,
    reply:
        Arc<Signal<crate::wait::EmbassySpinRawMutex, Result<FontStampedBuffer, FontKernelError>>>,
}

impl PendingFontStamp {
    pub(crate) const fn ticket(&self) -> FontKernelTicket {
        self.ticket
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
    pub(crate) grid_page_lane_admissions: u64,
    pub(crate) grid_patch_lane_admissions: u64,
    pub(crate) grid_present_lane_admissions: u64,
    pub(crate) grid_print_lane_admissions: u64,
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
            grid_page_lane_admissions: 0,
            grid_patch_lane_admissions: 0,
            grid_present_lane_admissions: 0,
            grid_print_lane_admissions: 0,
            queued: 0,
        }
    }
}

enum QueuedFontRequest {
    Retain {
        ticket: FontKernelTicket,
        request: RetainSceneRequest,
        reply: Arc<
            Signal<crate::wait::EmbassySpinRawMutex, Result<GpuFontRetainedScene, FontKernelError>>,
        >,
    },
    Stamp {
        ticket: FontKernelTicket,
        request: FontStampRequest,
        reply: Arc<
            Signal<crate::wait::EmbassySpinRawMutex, Result<FontStampedBuffer, FontKernelError>>,
        >,
    },
}

impl QueuedFontRequest {
    const fn ticket(&self) -> FontKernelTicket {
        match self {
            Self::Retain { ticket, .. } | Self::Stamp { ticket, .. } => *ticket,
        }
    }

    const fn consumer(&self) -> FontKernelConsumer {
        let path = match self {
            Self::Retain { .. } => FontKernelConsumerPath::RetainScene,
            Self::Stamp { .. } => FontKernelConsumerPath::Stamp,
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
    validate_retain_request(&request.scene)?;
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

fn next_ticket() -> FontKernelTicket {
    loop {
        let current = NEXT_TICKET.fetch_add(1, Ordering::AcqRel);
        if current != 0 {
            return FontKernelTicket(current);
        }
    }
}

fn validate_retain_request(request: &RetainSceneRequest) -> Result<(), FontKernelError> {
    if request.runs.is_empty() || request.runs.len() > FONT_KERNEL_MAX_RUNS {
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
        if chars == 0 || chars > MAX_DYNAMIC_TEXT_CHARS {
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

fn set_active_stage(ticket: FontKernelTicket, stage: &'static str) {
    let mut status = STATUS.lock();
    status.active_ticket = Some(ticket);
    status.active_stage = stage;
}

fn process_retain_scene(
    ticket: FontKernelTicket,
    request: &RetainSceneRequest,
) -> Result<GpuFontRetainedScene, FontKernelError> {
    set_active_stage(ticket, "font-warm");
    ensure_font_face_available(request.font).map_err(FontKernelError::Unavailable)?;
    set_active_stage(ticket, "coverage");
    let entries = request
        .runs
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

fn process_stamp(
    ticket: FontKernelTicket,
    request: &FontStampRequest,
) -> Result<FontStampedBuffer, FontKernelError> {
    let width = request.scene.raster_width;
    let height = request.scene.raster_height;
    let scene = process_retain_scene(ticket, &request.scene)?;
    set_active_stage(ticket, "output-allocate");
    let storage = crate::intel::gpgpu::allocate_font_instance_rgba8_surface(width, height)
        .ok_or(FontKernelError::Unavailable("font-stamp-output-allocation"))?;
    let surface = storage.surface();
    // The owned RGBA allocation is zeroed and DMA-flushed before return.
    // Dispatching another GPU clear here only adds direct-RCS contention.
    set_active_stage(ticket, "instance");
    let rendered = match scene.restamp_instance(
        surface,
        GpuFontRetainedStyle::identity(request.foreground),
        false,
        0.0,
    ) {
        Ok(rendered) => rendered,
        Err(error) => {
            if matches!(error, GpuFontRetainedSceneError::SubmittedIncomplete(_)) {
                core::mem::forget(storage);
            }
            return Err(error.into());
        }
    };
    Ok(FontStampedBuffer {
        ticket,
        storage,
        submits: rendered.submits,
        active_walkers: rendered.active_walkers,
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
    matches!(
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
            "font-kernel-service: {} deferred ticket={} reason={:?} gpu_retry={} queued={} action=requeue-ticket+yield-font-lane\n",
            operation,
            ticket.raw(),
            error,
            retry,
            REQUESTS.lock().len().saturating_add(1),
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
        "font-kernel-service: online paths=retain-scene+async-stamp+grid-page+grid-cell-patch+grid-present+grid-print controller=bsp worker=leased-blocking-service-lane font_lane=fair-fifo-multi-consumer queue_capacity={} retained_storage=gpu-vm-r8 stamp_output=gpu-vm-rgba8 completion=signal\n",
        FONT_KERNEL_QUEUE_CAPACITY,
    );
    loop {
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
    fn consumer_paths_keep_independent_identity() {
        let blueprint = FontKernelConsumer::new(FontKernelConsumerPath::GridPage, 1);
        let spirit = FontKernelConsumer::new(FontKernelConsumerPath::GridPage, 2);
        let stamp = FontKernelConsumer::new(FontKernelConsumerPath::Stamp, 1);
        assert_ne!(blueprint, spirit);
        assert_ne!(blueprint, stamp);
        assert_eq!(blueprint.path.name(), "grid-page");
        assert_eq!(stamp.path.name(), "stamp");
    }
}
