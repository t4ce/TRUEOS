use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

#[derive(Clone, Debug, Default)]
pub struct ImageBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

pub struct ScreenshotAwait {
    core: &'static LastScreenshotBuffer,
}

#[derive(Debug, Default)]
struct ImageBufferSlot {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    valid: bool,
}

impl ImageBufferSlot {
    const fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            valid: false,
        }
    }

    fn copy_out(&self) -> Option<ImageBuffer> {
        if !self.valid {
            return None;
        }

        Some(ImageBuffer {
            width: self.width,
            height: self.height,
            pixels: self.pixels.clone(),
        })
    }

    fn publish_copy(&mut self, width: u32, height: u32, pixels: &[u32]) {
        self.width = width;
        self.height = height;

        let need = (width as usize).saturating_mul(height as usize);
        self.pixels.resize(need, 0);

        let copy_len = need.min(pixels.len());
        self.pixels[..copy_len].copy_from_slice(&pixels[..copy_len]);
        if copy_len < need {
            self.pixels[copy_len..need].fill(0);
        }

        self.valid = need != 0;
    }
}

struct LastScreenshotBuffer {
    capture_armed: AtomicBool,
    seq: AtomicU64,
    slot: Mutex<ImageBufferSlot>,
}

impl LastScreenshotBuffer {
    const fn new() -> Self {
        Self {
            capture_armed: AtomicBool::new(false),
            seq: AtomicU64::new(0),
            slot: Mutex::new(ImageBufferSlot::new()),
        }
    }

    fn arm_capture(&self) {
        self.capture_armed.store(true, Ordering::Release);
    }

    fn is_capture_armed(&self) -> bool {
        self.capture_armed.load(Ordering::Acquire)
    }

    fn published_seq(&self) -> u64 {
        self.seq.load(Ordering::Acquire)
    }

    fn publish_copy(&self, width: u32, height: u32, pixels: &[u32]) -> u64 {
        if !self.capture_armed.swap(false, Ordering::AcqRel) {
            return self.published_seq();
        }

        {
            let mut guard = self.slot.lock();
            guard.publish_copy(width, height, pixels);
        }

        self.seq.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }

    fn copy_if_newer(&self, seen_seq: u64) -> Option<(u64, ImageBuffer)> {
        let seq = self.published_seq();
        if seq <= seen_seq {
            return None;
        }

        let guard = self.slot.lock();
        let image = guard.copy_out()?;
        Some((seq, image))
    }
}

impl ScreenshotAwait {
    const fn new(core: &'static LastScreenshotBuffer) -> Self {
        Self { core }
    }
}

fn next_frame_blocking(timeout_ms: u64) -> Option<ImageBuffer> {
    let seen_seq = VIRGL_SCREENSHOT_AWAIT.core.published_seq();
    VIRGL_SCREENSHOT_AWAIT.core.arm_capture();

    let mut out: Option<ImageBuffer> = None;
    let ok = crate::wait::spin_until_timeout(timeout_ms.max(1), || {
        if let Some((_seq, image)) = VIRGL_SCREENSHOT_AWAIT.core.copy_if_newer(seen_seq) {
            out = Some(image);
            return true;
        }
        false
    });
    if ok { out } else { None }
}

static LAST_SCREENSHOT_BUFFER: LastScreenshotBuffer = LastScreenshotBuffer::new();
static VIRGL_SCREENSHOT_AWAIT: ScreenshotAwait = ScreenshotAwait::new(&LAST_SCREENSHOT_BUFFER);

pub(crate) fn screenshot_capture_armed() -> bool {
    crate::allcaps::gfx::SCREENSHOT_CAPTURE_ENABLED && LAST_SCREENSHOT_BUFFER.is_capture_armed()
}

pub(crate) fn publish_screenshot_rgba_buffer(width: u32, height: u32, rgba: &[u8]) -> u64 {
    if !crate::allcaps::gfx::SCREENSHOT_CAPTURE_ENABLED {
        return 0;
    }

    let need_pixels = (width as usize).saturating_mul(height as usize);
    if need_pixels == 0 {
        return LAST_SCREENSHOT_BUFFER.publish_copy(width, height, &[]);
    }

    let mut pixels = Vec::with_capacity(need_pixels);
    for chunk in rgba.chunks_exact(4).take(need_pixels) {
        pixels.push(((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32));
    }
    if pixels.len() < need_pixels {
        pixels.resize(need_pixels, 0);
    }
    LAST_SCREENSHOT_BUFFER.publish_copy(width, height, pixels.as_slice())
}
