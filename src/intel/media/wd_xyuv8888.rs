//! Boundary between display writeback and the AVC stream.
//!
//! The display backend publishes packed XYUV8888 DMA surfaces here. Gen12
//! VDEnc accepts their X:Y:U:V component order as packed A:Y:U:V YUV444 and
//! performs the 4:4:4 to 4:2:0 chroma downsample while producing AVC. This
//! module deliberately contains no CPU colour conversion, VEBOX hop, or
//! UI4/compositor dependency.

use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
};

pub(crate) const WD_WIDTH: usize = super::avc_encode_probe::FRAME_WIDTH;
pub(crate) const WD_HEIGHT: usize = super::avc_encode_probe::FRAME_HEIGHT;
pub(crate) const WD_XYUV8888_PITCH: usize = WD_WIDTH * 4;
pub(crate) const WD_XYUV8888_BYTES: usize = WD_XYUV8888_PITCH * WD_HEIGHT;

/// A completed, CPU-mapped WD XYUV8888 target.
///
/// `cpu` exists solely for an explicitly requested diagnostic snapshot. The
/// streaming path consumes `phys` and never copies this surface through RAM.
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
}

struct StaticScreenshotBytes(UnsafeCell<[u8; WD_XYUV8888_BYTES]>);

unsafe impl Sync for StaticScreenshotBytes {}

static SCREENSHOT: StaticScreenshotBytes =
    StaticScreenshotBytes(UnsafeCell::new([0; WD_XYUV8888_BYTES]));
static SCREENSHOT_STATE: AtomicU8 = AtomicU8::new(ScreenshotState::Idle as u8);
static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ScreenshotRequestError {
    Busy,
}

/// Arm one best-effort snapshot. Repeated requests never queue work.
pub(crate) fn request_screenshot() -> Result<(), ScreenshotRequestError> {
    let mut state = SCREENSHOT_STATE.load(Ordering::Acquire);
    loop {
        if state != ScreenshotState::Idle as u8 && state != ScreenshotState::Ready as u8 {
            return Err(ScreenshotRequestError::Busy);
        }
        match SCREENSHOT_STATE.compare_exchange_weak(
            state,
            ScreenshotState::Requested as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Ok(()),
            Err(next) => state = next,
        }
    }
}

/// Refresh the single static slot from one completed WD frame.
///
/// The WD owner should call this from its background completion worker. It is
/// a no-op unless explicitly armed, never waits for a reader, and performs no
/// pixel conversion. The source lifetime contract of `WdXyuv8888DmaSurface` must
/// cover this call.
pub(crate) fn try_refresh_requested_screenshot(source: WdXyuv8888DmaSurface) -> bool {
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

    // The streaming consumer uses an uncached PPGTT view. A diagnostic CPU
    // reader must separately discard any clean cache lines retained from an
    // older snapshot before it copies the newest display-engine writeback.
    crate::intel::dma_flush(source.cpu.cast_mut(), WD_XYUV8888_BYTES);
    unsafe {
        core::ptr::copy_nonoverlapping(
            source.cpu,
            (*SCREENSHOT.0.get()).as_mut_ptr(),
            WD_XYUV8888_BYTES,
        );
    }
    SCREENSHOT_SEQUENCE.store(source.sequence, Ordering::Release);
    SCREENSHOT_STATE.store(ScreenshotState::Ready as u8, Ordering::Release);
    true
}

/// Borrow the latest explicitly captured raw XYUV8888 image without allocating.
/// The slot is fixed at 2560x1440, pitch 10240, and has no preview consumer.
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

    let sequence = SCREENSHOT_SEQUENCE.load(Ordering::Acquire);
    let result = unsafe { read(sequence, &*SCREENSHOT.0.get()) };
    SCREENSHOT_STATE.store(ScreenshotState::Ready as u8, Ordering::Release);
    Some(result)
}
