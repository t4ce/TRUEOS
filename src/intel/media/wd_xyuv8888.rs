//! Boundary between display writeback and the AVC stream.
//!
//! The display backend publishes packed XYUV8888 DMA surfaces here. Gen12
//! VDEnc accepts their X:Y:U:V component order as packed A:Y:U:V YUV444 and
//! performs the 4:4:4 to 4:2:0 chroma downsample while producing AVC. This
//! module deliberately contains no CPU colour conversion, VEBOX hop, or
//! UI4/compositor dependency.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

pub(crate) const WD_WIDTH: usize = super::avc_encode_probe::FRAME_WIDTH;
pub(crate) const WD_HEIGHT: usize = super::avc_encode_probe::FRAME_HEIGHT;
pub(crate) const WD_XYUV8888_PITCH: usize = WD_WIDTH * 4;
pub(crate) const WD_XYUV8888_BYTES: usize = WD_XYUV8888_PITCH * WD_HEIGHT;

/// A completed, CPU-mapped WD XYUV8888 target.
///
/// The streaming encoder and screenshot BCS consumer both read `phys`.
/// CPU color conversion only reads the screenshot-owned destination.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct WdXyuv8888DmaSurface {
    phys: u64,
    cpu: *const u8,
    sequence: u64,
}

unsafe impl Send for WdXyuv8888DmaSurface {}
unsafe impl Sync for WdXyuv8888DmaSurface {}

impl WdXyuv8888DmaSurface {
    /// Construct a descriptor after WD completion has made the whole surface
    /// visible. The backing must remain alive and stable until every submitted
    /// encode or requested snapshot using this value has retired.
    pub(crate) unsafe fn new(
        phys: u64,
        cpu: *const u8,
        bytes: usize,
        pitch: usize,
        sequence: u64,
    ) -> Option<Self> {
        (phys != 0
            && phys.is_multiple_of(crate::intel::WARM_ALIGN as u64)
            && !cpu.is_null()
            && bytes == WD_XYUV8888_BYTES
            && pitch == WD_XYUV8888_PITCH)
            .then_some(Self {
                phys,
                cpu,
                sequence,
            })
    }

    pub(crate) const fn sequence(self) -> u64 {
        self.sequence
    }

    pub(crate) fn encoder_surface(self) -> Option<super::avc_encode_probe::AvcXyuv8888DmaSurface> {
        super::avc_encode_probe::AvcXyuv8888DmaSurface::new(self.phys, WD_XYUV8888_BYTES)
    }

    /// Validate and narrow the display backend's completion descriptor at the
    /// media ownership boundary.
    pub(crate) unsafe fn from_writeback(frame: crate::intel::WdXyuv8888Frame) -> Option<Self> {
        if frame.width as usize != WD_WIDTH || frame.height as usize != WD_HEIGHT {
            return None;
        }
        unsafe {
            Self::new(
                frame.phys,
                frame.virt,
                frame.byte_len,
                frame.pitch_bytes as usize,
                frame.sequence,
            )
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ScreenshotState {
    Idle = 0,
    Requested = 1,
    Writing = 2,
    Ready = 3,
    Reading = 4,
    Reserving = 5,
    Failed = 6,
}

// Private aliases in the BCS PPGTT only, unrelated to WD's 0xE0000000 GGTT
// address. BCS invalidates translations before every job, including VA reuse.
const SNAPSHOT_SOURCE_GPU: u64 = 0x3000_0000;
const SNAPSHOT_DESTINATION_GPU: u64 = 0x3100_0000;
const _: () = assert!(WD_XYUV8888_BYTES <= 0x0100_0000);
const _: () = assert!(SNAPSHOT_DESTINATION_GPU + WD_XYUV8888_BYTES as u64 <= 0x4000_0000);
const SNAPSHOT_ADMISSION_TIMEOUT_NS: u64 = 250_000_000;

struct OwnedScreenshot {
    phys: u64,
    cpu: *mut u8,
    sequence: u64,
    in_flight: bool,
}

unsafe impl Send for OwnedScreenshot {}

impl Drop for OwnedScreenshot {
    fn drop(&mut self) {
        if self.in_flight {
            SCREENSHOT_DISABLED.store(true, Ordering::Release);
            crate::log_error!(target: "gfx";
                "wd-snapshot: BCS destination pinned phys=0x{:X} bytes={} action=no-free\n",
                self.phys, WD_XYUV8888_BYTES);
        } else {
            crate::dma::dealloc(self.cpu, WD_XYUV8888_BYTES);
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SnapshotCopyStats {
    pub(crate) copies: u64,
    pub(crate) failures: u64,
    pub(crate) bytes: u64,
    pub(crate) prepare_us: u64,
    pub(crate) admission_us: u64,
    pub(crate) submit_us: u64,
    pub(crate) retire_us: u64,
    pub(crate) acquire_us: u64,
    pub(crate) request_to_ready_us: u64,
}

static SNAPSHOT_STATS: spin::Mutex<SnapshotCopyStats> = spin::Mutex::new(SnapshotCopyStats {
    copies: 0,
    failures: 0,
    bytes: 0,
    prepare_us: 0,
    admission_us: 0,
    submit_us: 0,
    retire_us: 0,
    acquire_us: 0,
    request_to_ready_us: 0,
});
static SCREENSHOT: spin::Mutex<Option<OwnedScreenshot>> = spin::Mutex::new(None);
static SCREENSHOT_STATE: AtomicU8 = AtomicU8::new(ScreenshotState::Idle as u8);
static SCREENSHOT_DISABLED: AtomicBool = AtomicBool::new(false);
static SCREENSHOT_REQUESTED_NS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn snapshot_copy_stats() -> SnapshotCopyStats {
    *SNAPSHOT_STATS.lock()
}

/// Cancellation before or during an await is a failed request, never Ready.
struct ScreenshotAttempt {
    ready: bool,
}
impl Drop for ScreenshotAttempt {
    fn drop(&mut self) {
        if !self.ready {
            SNAPSHOT_STATS.lock().failures += 1;
            SCREENSHOT_STATE.store(ScreenshotState::Failed as u8, Ordering::Release);
        }
    }
}

const CAPTURE_DRIVER_IDLE: u8 = 0;
const CAPTURE_DRIVER_STREAM: u8 = 1;
const CAPTURE_DRIVER_MANUAL_SHOT: u8 = 2;
static CAPTURE_DRIVER: AtomicU8 = AtomicU8::new(CAPTURE_DRIVER_IDLE);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ScreenshotRequestError {
    Busy,
    Quarantined,
}

/// Arm one best-effort snapshot. Repeated requests never queue work.
pub(crate) fn request_screenshot() -> Result<(), ScreenshotRequestError> {
    if SCREENSHOT_DISABLED.load(Ordering::Acquire) {
        return Err(ScreenshotRequestError::Quarantined);
    }
    SCREENSHOT_STATE
        .compare_exchange(
            ScreenshotState::Idle as u8,
            ScreenshotState::Reserving as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|_| ScreenshotRequestError::Busy)?;
    SCREENSHOT_REQUESTED_NS.store(crate::chronos::monotonic_nanos(), Ordering::Relaxed);
    SCREENSHOT_STATE.store(ScreenshotState::Requested as u8, Ordering::Release);
    Ok(())
}

/// Admit a new stream only if WD was idle, including exclusion against a
/// manual shot in flight. The per-frame claim below also accepts the live owner.
pub(crate) fn try_reserve_stream_capture() -> bool {
    CAPTURE_DRIVER
        .compare_exchange(
            CAPTURE_DRIVER_IDLE,
            CAPTURE_DRIVER_STREAM,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
}

/// Reserve WD for an RDP session. A manual one-frame capture already in
/// progress wins briefly; the stream preparation worker retries cooperatively.
pub(crate) fn try_claim_stream_capture() -> bool {
    let driver = CAPTURE_DRIVER.load(Ordering::Acquire);
    driver == CAPTURE_DRIVER_STREAM
        || (driver == CAPTURE_DRIVER_IDLE
            && CAPTURE_DRIVER
                .compare_exchange(
                    CAPTURE_DRIVER_IDLE,
                    CAPTURE_DRIVER_STREAM,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok())
}

pub(crate) fn release_stream_capture() {
    let _ = CAPTURE_DRIVER.compare_exchange(
        CAPTURE_DRIVER_STREAM,
        CAPTURE_DRIVER_IDLE,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
}

/// Claim one pending manual screenshot only when no RDP session owns WD.
pub(crate) fn try_claim_manual_screenshot_capture() -> bool {
    SCREENSHOT_STATE.load(Ordering::Acquire) == ScreenshotState::Requested as u8
        && CAPTURE_DRIVER
            .compare_exchange(
                CAPTURE_DRIVER_IDLE,
                CAPTURE_DRIVER_MANUAL_SHOT,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
}

pub(crate) fn release_manual_screenshot_capture() {
    let _ = CAPTURE_DRIVER.compare_exchange(
        CAPTURE_DRIVER_MANUAL_SHOT,
        CAPTURE_DRIVER_IDLE,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
}

pub(crate) fn cancel_requested_screenshot() {
    if SCREENSHOT_STATE
        .compare_exchange(
            ScreenshotState::Requested as u8,
            ScreenshotState::Failed as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        SNAPSHOT_STATS.lock().failures += 1;
    }
}

pub(crate) fn take_failed_screenshot() -> bool {
    SCREENSHOT_STATE
        .compare_exchange(
            ScreenshotState::Failed as u8,
            ScreenshotState::Idle as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
}

/// Copy one explicitly requested frame into an owned DMA allocation. Both WD
/// owners await this operation before encoding, rearming, or releasing WD.
/// No request means no allocation, submission, or wait.
pub(crate) async fn try_refresh_requested_screenshot(source: WdXyuv8888DmaSurface) -> bool {
    use crate::intel::{
        GucBcs0CopyCompletion as Completion, GucBcs0CopySubmitError as SubmitError,
    };
    use trueos_time::{Duration, Timer};

    if SCREENSHOT_STATE
        .compare_exchange(
            ScreenshotState::Requested as u8,
            ScreenshotState::Writing as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return false;
    }
    let mut attempt = ScreenshotAttempt { ready: false };
    let started = crate::chronos::monotonic_nanos();
    let Ok(mut source_lease) =
        crate::intel::pin_ui4_wd_frame_for_copy(source.phys, source.sequence)
    else {
        return false;
    };
    let Some((phys, cpu)) =
        crate::dma::alloc_with_max(WD_XYUV8888_BYTES, crate::intel::WARM_ALIGN, Some(1u64 << 39))
    else {
        return false;
    };
    let mut destination = OwnedScreenshot {
        phys,
        cpu,
        sequence: source.sequence,
        in_flight: false,
    };
    // Recycled CPU backing may contain dirty lines. Write back/invalidate them
    // before BCS writes; never flush old dirty data over a completed DMA copy.
    crate::intel::dma_flush(cpu, WD_XYUV8888_BYTES);
    let prepared = crate::chronos::monotonic_nanos();
    let surface = |phys, gpu| crate::intel::GucBcs0RgbaSurface {
        phys,
        gpu,
        bytes: WD_XYUV8888_BYTES,
        width: WD_WIDTH as u32,
        height: WD_HEIGHT as u32,
        pitch_bytes: WD_XYUV8888_PITCH as u32,
    };
    let copy = crate::intel::GucBcs0RgbaCopy {
        source: surface(source.phys, SNAPSHOT_SOURCE_GPU),
        source_x: 0,
        source_y: 0,
        destination_x: 0,
        destination_y: 0,
        width: WD_WIDTH as u32,
        height: WD_HEIGHT as u32,
    };
    let mut submit_us = 0;
    let submission = loop {
        let submit_started = crate::chronos::monotonic_nanos();
        let queued = crate::r::services::vcpy_service::queue_uncached_copies(
            surface(phys, SNAPSHOT_DESTINATION_GPU),
            &[copy],
        );
        submit_us += crate::chronos::monotonic_nanos().saturating_sub(submit_started) / 1_000;
        match queued {
            Ok(submission) => {
                destination.in_flight = true;
                source_lease.mark_submitted();
                break submission;
            }
            Err(SubmitError::Busy)
                if crate::chronos::monotonic_nanos().saturating_sub(prepared)
                    < SNAPSHOT_ADMISSION_TIMEOUT_NS =>
            {
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(error) => {
                if error == SubmitError::SubmitFailed {
                    destination.in_flight = true;
                    source_lease.mark_submitted();
                }
                crate::log_warn!(target: "gfx";
                    "wd-snapshot: BCS admission failed wd_sequence={} reason={:?} cpu_fallback=0\n",
                    source.sequence, error);
                return false;
            }
        }
    };
    let submitted = crate::chronos::monotonic_nanos();
    loop {
        match crate::r::services::vcpy_service::poll_rgba_copies(submission) {
            Completion::Complete => break,
            Completion::Pending => Timer::after(Duration::from_millis(1)).await,
            Completion::Failed | Completion::InvalidSubmission => return false,
        }
    }
    let retired = crate::chronos::monotonic_nanos();
    destination.in_flight = false;
    source_lease.mark_retired();
    drop(source_lease);
    // Ordered BCS completion precedes CPU cache acquisition and publication.
    crate::intel::dma_flush(cpu, WD_XYUV8888_BYTES);
    let acquired = crate::chronos::monotonic_nanos();
    let stats = {
        let mut stats = SNAPSHOT_STATS.lock();
        stats.copies += 1;
        stats.bytes += WD_XYUV8888_BYTES as u64;
        stats.prepare_us = prepared.saturating_sub(started) / 1_000;
        stats.admission_us = submitted.saturating_sub(prepared) / 1_000;
        stats.submit_us = submit_us;
        stats.retire_us = retired.saturating_sub(submitted) / 1_000;
        stats.acquire_us = acquired.saturating_sub(retired) / 1_000;
        stats.request_to_ready_us =
            acquired.saturating_sub(SCREENSHOT_REQUESTED_NS.load(Ordering::Acquire)) / 1_000;
        *stats
    };
    *SCREENSHOT.lock() = Some(destination);
    attempt.ready = true;
    SCREENSHOT_STATE.store(ScreenshotState::Ready as u8, Ordering::Release);
    crate::log_info!(target: "gfx";
        "wd-snapshot: copy retired wd_sequence={} engine=bcs0 bytes={} prepare_us={} admission_us={} submit_us={} retire_us={} acquire_us={} request_to_ready_us={} poll_ms=1 cpu_copy=0\n",
        source.sequence, WD_XYUV8888_BYTES, stats.prepare_us, stats.admission_us,
        stats.submit_us, stats.retire_us, stats.acquire_us, stats.request_to_ready_us);
    true
}

/// Consume and free one owned XYUV8888 snapshot after the reader returns.
pub(crate) fn with_screenshot<R>(read: impl FnOnce(u64, &[u8]) -> R) -> Option<R> {
    if SCREENSHOT_STATE
        .compare_exchange(
            ScreenshotState::Ready as u8,
            ScreenshotState::Reading as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return None;
    }
    let snapshot = SCREENSHOT.lock().take();
    let result = snapshot.as_ref().map(|snapshot| unsafe {
        read(snapshot.sequence, core::slice::from_raw_parts(snapshot.cpu, WD_XYUV8888_BYTES))
    });
    drop(snapshot);
    SCREENSHOT_STATE.store(ScreenshotState::Idle as u8, Ordering::Release);
    result
}
