use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crc32fast::Hasher as Crc32;
use embassy_time::{Duration as EmbassyDuration, Timer};
use miniz_oxide::deflate::compress_to_vec_zlib;
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

struct EncodedScreenshotBuffer {
    bytes: Mutex<Vec<u8>>,
}

impl EncodedScreenshotBuffer {
    const fn new() -> Self {
        Self {
            bytes: Mutex::new(Vec::new()),
        }
    }

    fn replace(&self, data: &[u8]) {
        let mut guard = self.bytes.lock();
        guard.clear();
        guard.extend_from_slice(data);
    }

    fn copy_out(&self, out_ptr: *mut u8, out_cap: usize) -> isize {
        let guard = self.bytes.lock();
        if guard.is_empty() {
            return -1;
        }
        if out_ptr.is_null() || out_cap == 0 {
            return guard.len() as isize;
        }
        if out_cap < guard.len() {
            return guard.len() as isize;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(guard.as_ptr(), out_ptr, guard.len());
        }
        guard.len() as isize
    }
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
        crate::r::ui2::request_full_recompose("screenshot-capture");
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

    pub async fn next_frame(&self, seen_seq: u64, poll_ms: u64) -> (u64, ImageBuffer) {
        self.core.arm_capture();
        let delay_ms = poll_ms.max(1);
        loop {
            if let Some(next) = self.core.copy_if_newer(seen_seq) {
                return next;
            }

            Timer::after(EmbassyDuration::from_millis(delay_ms)).await;
        }
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

fn push_be_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn append_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    push_be_u32(out, data.len() as u32);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);

    let mut crc = Crc32::new();
    crc.update(kind);
    crc.update(data);
    push_be_u32(out, crc.finalize());
}

fn base64_encode(bytes: &[u8]) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    let mut i = 0usize;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(TABLE[((n >> 18) & 0x3F) as usize]);
        out.push(TABLE[((n >> 12) & 0x3F) as usize]);
        out.push(TABLE[((n >> 6) & 0x3F) as usize]);
        out.push(TABLE[(n & 0x3F) as usize]);
        i += 3;
    }

    match bytes.len().saturating_sub(i) {
        1 => {
            let n = (bytes[i] as u32) << 16;
            out.push(TABLE[((n >> 18) & 0x3F) as usize]);
            out.push(TABLE[((n >> 12) & 0x3F) as usize]);
            out.push(b'=');
            out.push(b'=');
        }
        2 => {
            let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
            out.push(TABLE[((n >> 18) & 0x3F) as usize]);
            out.push(TABLE[((n >> 12) & 0x3F) as usize]);
            out.push(TABLE[((n >> 6) & 0x3F) as usize]);
            out.push(b'=');
        }
        _ => {}
    }

    out
}

fn encode_png_data_url(image: &ImageBuffer) -> Option<Vec<u8>> {
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 {
        return None;
    }

    let pixel_count = width.saturating_mul(height);
    if image.pixels.len() < pixel_count {
        return None;
    }

    let mut filtered = Vec::with_capacity(height.saturating_mul(1 + width.saturating_mul(3)));
    for y in 0..height {
        let row = &image.pixels[y * width..(y + 1) * width];
        filtered.push(0);
        for &pixel in row {
            filtered.push(((pixel >> 16) & 0xFF) as u8);
            filtered.push(((pixel >> 8) & 0xFF) as u8);
            filtered.push((pixel & 0xFF) as u8);
        }
    }

    let compressed = compress_to_vec_zlib(&filtered, 6);

    let mut png = Vec::with_capacity(8 + 25 + 12 + compressed.len() + 12);
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = Vec::with_capacity(13);
    push_be_u32(&mut ihdr, image.width);
    push_be_u32(&mut ihdr, image.height);
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    append_png_chunk(&mut png, b"IHDR", &ihdr);
    append_png_chunk(&mut png, b"IDAT", &compressed);
    append_png_chunk(&mut png, b"IEND", &[]);

    let mut out = b"data:image/png;base64,".to_vec();
    out.extend_from_slice(&base64_encode(&png));
    Some(out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_capture_screenshot_data_url(
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() || out_cap == 0 {
        let Some(image) = next_frame_blocking(2000) else {
            return -1;
        };
        let Some(data_url) = encode_png_data_url(&image) else {
            return -1;
        };
        ENCODED_SCREENSHOT_BUFFER.replace(data_url.as_slice());
        return data_url.len() as isize;
    }

    ENCODED_SCREENSHOT_BUFFER.copy_out(out_ptr, out_cap)
}

static LAST_SCREENSHOT_BUFFER: LastScreenshotBuffer = LastScreenshotBuffer::new();
static ENCODED_SCREENSHOT_BUFFER: EncodedScreenshotBuffer = EncodedScreenshotBuffer::new();
static VIRGL_SCREENSHOT_AWAIT: ScreenshotAwait = ScreenshotAwait::new(&LAST_SCREENSHOT_BUFFER);

pub fn virgl_screenshot_await() -> &'static ScreenshotAwait {
    &VIRGL_SCREENSHOT_AWAIT
}

pub(crate) fn screenshot_capture_armed() -> bool {
    LAST_SCREENSHOT_BUFFER.is_capture_armed()
}

pub(crate) fn publish_screenshot_image_buffer(width: u32, height: u32, pixels: &[u32]) -> u64 {
    LAST_SCREENSHOT_BUFFER.publish_copy(width, height, pixels)
}

pub(crate) fn publish_screenshot_rgba_buffer(width: u32, height: u32, rgba: &[u8]) -> u64 {
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

pub(crate) fn virgl_screenshot_capture_armed() -> bool {
    screenshot_capture_armed()
}

pub(crate) fn publish_virgl_image_buffer(width: u32, height: u32, pixels: &[u32]) -> u64 {
    publish_screenshot_image_buffer(width, height, pixels)
}
