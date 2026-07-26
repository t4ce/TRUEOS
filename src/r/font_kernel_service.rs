//! Two-path kernel font service.
//!
//! `RetainScene` yields GPU-VM-resident Skrifa coverage that a caller may
//! restamp repeatedly. `Stamp` is the one-shot path: the worker creates the
//! same retained representation temporarily, composites it into a new
//! GPU-visible RGBA buffer, and returns that owned buffer asynchronously.

use alloc::{collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_sync::signal::Signal;
use spin::Mutex;

use crate::intel::gpu_font::{
    GpuFontFace, GpuFontJobEntry, GpuFontRetainedScene, GpuFontRetainedSceneError,
    GpuFontRetainedStyle, GpuFontRgba, GpuFontTextRequest, MAX_DYNAMIC_TEXT_CHARS,
    ensure_font_face_available, retain_gpu_font_centered_scene_at_raster,
    retain_gpu_font_scene_at_raster,
};

const FONT_KERNEL_QUEUE_CAPACITY: usize = 32;
const FONT_KERNEL_MAX_RUNS: usize = 64;

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
static ONLINE: AtomicBool = AtomicBool::new(false);
static WORK_AVAILABLE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static REQUESTS: Mutex<VecDeque<QueuedFontRequest>> = Mutex::new(VecDeque::new());
static STATUS: Mutex<FontKernelServiceStatus> = Mutex::new(FontKernelServiceStatus::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FontKernelTicket(u64);

impl FontKernelTicket {
    pub(crate) const fn raw(self) -> u64 {
        self.0
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
    pub(crate) submitted_retain: u64,
    pub(crate) submitted_stamp: u64,
    pub(crate) completed_retain: u64,
    pub(crate) completed_stamp: u64,
    pub(crate) failed: u64,
    pub(crate) queued: usize,
}

impl FontKernelServiceStatus {
    const fn new() -> Self {
        Self {
            online: false,
            active_ticket: None,
            active_stage: "idle",
            submitted_retain: 0,
            submitted_stamp: 0,
            completed_retain: 0,
            completed_stamp: 0,
            failed: 0,
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
    request: RetainSceneRequest,
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
    request: FontStampRequest,
) -> Result<FontStampedBuffer, FontKernelError> {
    let width = request.scene.raster_width;
    let height = request.scene.raster_height;
    let scene = process_retain_scene(ticket, request.scene)?;
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

#[embassy_executor::task]
pub(crate) async fn font_kernel_service_task() {
    ONLINE.store(true, Ordering::Release);
    crate::log_info!(
        target: "render";
        "font-kernel-service: online paths=retain-scene+async-stamp queue_capacity={} retained_storage=gpu-vm-r8 stamp_output=gpu-vm-rgba8 completion=signal\n",
        FONT_KERNEL_QUEUE_CAPACITY,
    );
    loop {
        while let Some(request) = REQUESTS.lock().pop_front() {
            match request {
                QueuedFontRequest::Retain {
                    ticket,
                    request,
                    reply,
                } => {
                    set_active_stage(ticket, "dispatch");
                    let result = process_retain_scene(ticket, request);
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
                    let result = process_stamp(ticket, request);
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
        }
        WORK_AVAILABLE.wait().await;
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
}
