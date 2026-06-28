use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use libm::{ceilf, fabsf, floorf};
use spin::Mutex;

use crate::intel::types::{
    Rgba8, SOLID_RECT_SIZE, SPRITE_QUAD_SIZE, SolidRect, SpriteCorner, SpriteQuad, UiPlaneSlot,
    UiPresent, UiPresentPath, UiRect, UiSurfaceFormat, read_solid_rect_bytes,
    read_sprite_quad_bytes,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Ui3RenderTarget {
    Frame,
    Texture(u32),
}

#[derive(Clone, Copy, Debug)]
struct Ui3Frame {
    front_surface: crate::r::ui_surface::UiSurfaceHandle,
    back_surface: crate::r::ui_surface::UiSurfaceHandle,
    tex_id: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    active: bool,
    allow_present: bool,
    preserve_contents: bool,
    clear_rgb: u32,
    target: Ui3RenderTarget,
}

#[derive(Clone)]
struct CpuImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

static NEXT_FRAME_ID: AtomicU32 = AtomicU32::new(1);
static FRAMES: Mutex<BTreeMap<u32, Ui3Frame>> = Mutex::new(BTreeMap::new());
static OFFSCREEN: Mutex<BTreeMap<u32, CpuImage>> = Mutex::new(BTreeMap::new());
static UI3_FRAME_CREATES: AtomicU64 = AtomicU64::new(0);
static UI3_FRAME_BEGINS: AtomicU64 = AtomicU64::new(0);
static UI3_FRAME_ENDS: AtomicU64 = AtomicU64::new(0);
static UI3_FRAME_PRESENTS: AtomicU64 = AtomicU64::new(0);
static UI3_FRAME_PRESENT_FAILS: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_SOLID_CALLS: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_SOLID_BYTES: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_SPRITE_CALLS: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_SPRITE_BYTES: AtomicU64 = AtomicU64::new(0);
static UI3_TEXTURE_MISSES: AtomicU64 = AtomicU64::new(0);
static UI3_TARGET_SWITCHES: AtomicU64 = AtomicU64::new(0);
static UI3_OFFSCREEN_ALLOCS: AtomicU64 = AtomicU64::new(0);
static UI3_BEGIN_NS: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_SOLID_NS: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_SPRITE_NS: AtomicU64 = AtomicU64::new(0);
static UI3_PRESENT_FLUSH_NS: AtomicU64 = AtomicU64::new(0);
static UI3_PRESENT_SURFACE_NS: AtomicU64 = AtomicU64::new(0);
static UI3_COMMIT_NS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn create_frame(x: i32, y: i32, width: u32, height: u32, tex_id: u32) -> u32 {
    let width = width.max(1);
    let height = height.max(1);
    let Ok(front_surface) =
        crate::r::ui_surface::create_surface(width, height, UiSurfaceFormat::Xrgb8888)
    else {
        return 0;
    };
    let Ok(back_surface) =
        crate::r::ui_surface::create_surface(width, height, UiSurfaceFormat::Xrgb8888)
    else {
        crate::r::ui_surface::destroy_surface(front_surface);
        return 0;
    };
    let _ = crate::r::ui_surface::clear_surface_rgb(front_surface, 0);
    let _ = crate::r::ui_surface::clear_surface_rgb(back_surface, 0);
    let id = NEXT_FRAME_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| Some(id.wrapping_add(1).max(1)))
        .unwrap_or(1);
    FRAMES.lock().insert(
        id,
        Ui3Frame {
            front_surface,
            back_surface,
            tex_id,
            x,
            y,
            width,
            height,
            active: false,
            allow_present: true,
            preserve_contents: false,
            clear_rgb: 0,
            target: Ui3RenderTarget::Frame,
        },
    );
    let count = UI3_FRAME_CREATES.fetch_add(1, Ordering::Relaxed) + 1;
    if count <= 8 {
        crate::log!(
            "ui3/frame: create#{} frame={} tex={} pos={},{} size={}x{}\n",
            count,
            id,
            tex_id,
            x,
            y,
            width,
            height
        );
    }
    id
}

pub(crate) fn close_frame(frame_id: u32) -> bool {
    let Some(frame) = FRAMES.lock().remove(&frame_id) else {
        return false;
    };
    if frame.tex_id != 0 {
        OFFSCREEN.lock().remove(&frame.tex_id);
    }
    let front_ok = crate::r::ui_surface::destroy_surface(frame.front_surface);
    let back_ok = crate::r::ui_surface::destroy_surface(frame.back_surface);
    front_ok && back_ok
}

pub(crate) fn set_position(frame_id: u32, x: i32, y: i32) -> bool {
    let mut frames = FRAMES.lock();
    let Some(frame) = frames.get_mut(&frame_id) else {
        return false;
    };
    frame.x = x;
    frame.y = y;
    true
}

pub(crate) fn set_size(frame_id: u32, width: u32, height: u32) -> bool {
    let width = width.max(1);
    let height = height.max(1);
    let mut frames = FRAMES.lock();
    let Some(frame) = frames.get_mut(&frame_id) else {
        return false;
    };
    if frame.width == width && frame.height == height {
        return true;
    }
    let Ok(front_surface) =
        crate::r::ui_surface::create_surface(width, height, UiSurfaceFormat::Xrgb8888)
    else {
        return false;
    };
    let Ok(back_surface) =
        crate::r::ui_surface::create_surface(width, height, UiSurfaceFormat::Xrgb8888)
    else {
        crate::r::ui_surface::destroy_surface(front_surface);
        return false;
    };
    let old_front_surface = frame.front_surface;
    let old_back_surface = frame.back_surface;
    frame.front_surface = front_surface;
    frame.back_surface = back_surface;
    frame.width = width;
    frame.height = height;
    frame.active = false;
    frame.target = Ui3RenderTarget::Frame;
    let _ = crate::r::ui_surface::clear_surface_rgb(front_surface, 0);
    let _ = crate::r::ui_surface::clear_surface_rgb(back_surface, 0);
    drop(frames);
    let front_ok = crate::r::ui_surface::destroy_surface(old_front_surface);
    let back_ok = crate::r::ui_surface::destroy_surface(old_back_surface);
    front_ok && back_ok
}

pub(crate) fn request_repaint(frame_id: u32) -> bool {
    present_frame(frame_id, false)
}

pub(crate) fn begin_frame(
    frame_id: u32,
    clear_rgb: u32,
    preserve_contents: bool,
    allow_present: bool,
) -> i32 {
    let begin_start_ns = now_ns();
    let (front_surface, back_surface) = {
        let mut frames = FRAMES.lock();
        let Some(frame) = frames.get_mut(&frame_id) else {
            return -1;
        };
        if frame.active {
            return -2;
        }
        frame.active = true;
        frame.allow_present = allow_present;
        frame.preserve_contents = preserve_contents;
        frame.clear_rgb = clear_rgb & 0x00FF_FFFF;
        frame.target = Ui3RenderTarget::Frame;
        (frame.front_surface, frame.back_surface)
    };
    if preserve_contents {
        if !copy_surface_pixels(front_surface, back_surface) {
            let _ = crate::r::ui_surface::clear_surface_rgb(back_surface, clear_rgb & 0x00FF_FFFF);
        }
    } else {
        let _ = crate::r::ui_surface::clear_surface_rgb(back_surface, clear_rgb & 0x00FF_FFFF);
    }
    UI3_FRAME_BEGINS.fetch_add(1, Ordering::Relaxed);
    UI3_BEGIN_NS.fetch_add(elapsed_ns_since(begin_start_ns), Ordering::Relaxed);
    0
}

pub(crate) fn end_frame(frame_id: u32) -> i32 {
    let allow_present = {
        let mut frames = FRAMES.lock();
        let Some(frame) = frames.get_mut(&frame_id) else {
            return -1;
        };
        if !frame.active {
            return -2;
        }
        frame.active = false;
        frame.target = Ui3RenderTarget::Frame;
        frame.allow_present
    };
    if allow_present {
        if !present_frame(frame_id, true) {
            return -3;
        }
    } else if !commit_frame_without_present(frame_id) {
        return -3;
    }
    UI3_FRAME_ENDS.fetch_add(1, Ordering::Relaxed);
    0
}

pub(crate) fn set_render_target(frame_id: u32, tex_id: u32) -> i32 {
    let (target, clear, clear_rgb, width, height) = {
        let mut frames = FRAMES.lock();
        let Some(frame) = frames.get_mut(&frame_id) else {
            return -1;
        };
        let target = if tex_id == 0 || tex_id == frame.tex_id {
            Ui3RenderTarget::Frame
        } else {
            Ui3RenderTarget::Texture(tex_id)
        };
        let clear = frame.active
            && !frame.preserve_contents
            && frame.target != target
            && matches!(target, Ui3RenderTarget::Texture(_));
        if frame.target != target {
            UI3_TARGET_SWITCHES.fetch_add(1, Ordering::Relaxed);
        }
        frame.target = target;
        (target, clear, frame.clear_rgb, frame.width, frame.height)
    };

    if let Ui3RenderTarget::Texture(id) = target {
        ensure_offscreen(id, width, height, clear, clear_rgb);
    }
    0
}

pub(crate) fn draw_solid_batch(frame_id: u32, bytes: &[u8]) -> i32 {
    if bytes.len() % SOLID_RECT_SIZE != 0 {
        return -3;
    }
    let draw_start_ns = now_ns();
    UI3_DRAW_SOLID_CALLS.fetch_add(1, Ordering::Relaxed);
    UI3_DRAW_SOLID_BYTES.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    let Some(frame) = frame_snapshot(frame_id) else {
        return -1;
    };
    if !frame.active {
        return -2;
    }

    match frame.target {
        Ui3RenderTarget::Frame => {
            let Some(access) = crate::r::ui_surface::pixel_access(frame.back_surface) else {
                return -1;
            };
            let dst = unsafe { core::slice::from_raw_parts_mut(access.virt, access.byte_len) };
            raster_solid_batch(
                dst,
                access.pitch as usize,
                access.width,
                access.height,
                access.format,
                bytes,
            );
            let _ = crate::r::ui_surface::flush_surface(frame.back_surface);
        }
        Ui3RenderTarget::Texture(tex_id) => {
            let mut offscreen = OFFSCREEN.lock();
            let image = offscreen
                .entry(tex_id)
                .or_insert_with(|| new_cpu_image(frame.width, frame.height));
            raster_solid_batch(
                &mut image.rgba,
                image.width as usize * 4,
                image.width,
                image.height,
                UiSurfaceFormat::Rgba8888,
                bytes,
            );
        }
    }
    UI3_DRAW_SOLID_NS.fetch_add(elapsed_ns_since(draw_start_ns), Ordering::Relaxed);
    0
}

pub(crate) fn draw_sprite_batch(frame_id: u32, tex_id: u32, bytes: &[u8]) -> i32 {
    if bytes.len() % SPRITE_QUAD_SIZE != 0 {
        return -3;
    }
    let draw_start_ns = now_ns();
    UI3_DRAW_SPRITE_CALLS.fetch_add(1, Ordering::Relaxed);
    UI3_DRAW_SPRITE_BYTES.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    let Some(frame) = frame_snapshot(frame_id) else {
        return -1;
    };
    if !frame.active {
        return -2;
    }
    let Some(src) = texture_source(tex_id) else {
        UI3_TEXTURE_MISSES.fetch_add(1, Ordering::Relaxed);
        return 0;
    };

    match frame.target {
        Ui3RenderTarget::Frame => {
            let Some(access) = crate::r::ui_surface::pixel_access(frame.back_surface) else {
                return -1;
            };
            let dst = unsafe { core::slice::from_raw_parts_mut(access.virt, access.byte_len) };
            raster_sprite_batch(
                dst,
                access.pitch as usize,
                access.width,
                access.height,
                access.format,
                &src,
                bytes,
            );
            let _ = crate::r::ui_surface::flush_surface(frame.back_surface);
        }
        Ui3RenderTarget::Texture(dst_tex_id) => {
            let mut offscreen = OFFSCREEN.lock();
            let image = offscreen
                .entry(dst_tex_id)
                .or_insert_with(|| new_cpu_image(frame.width, frame.height));
            raster_sprite_batch(
                &mut image.rgba,
                image.width as usize * 4,
                image.width,
                image.height,
                UiSurfaceFormat::Rgba8888,
                &src,
                bytes,
            );
        }
    }
    UI3_DRAW_SPRITE_NS.fetch_add(elapsed_ns_since(draw_start_ns), Ordering::Relaxed);
    0
}

fn present_frame(frame_id: u32, swap_on_success: bool) -> bool {
    let frame = {
        let frames = FRAMES.lock();
        let Some(frame) = frames.get(&frame_id) else {
            return false;
        };
        *frame
    };
    let surface = if swap_on_success {
        frame.back_surface
    } else {
        frame.front_surface
    };
    let flush_start_ns = now_ns();
    crate::r::ui_surface::flush_surface(surface);
    UI3_PRESENT_FLUSH_NS.fetch_add(elapsed_ns_since(flush_start_ns), Ordering::Relaxed);
    let dst_x = frame.x.max(0) as u32;
    let dst_y = frame.y.max(0) as u32;
    let present = UiPresent {
        src: UiRect::new(0, 0, frame.width, frame.height),
        dst: UiRect::new(dst_x, dst_y, frame.width, frame.height),
        plane: UiPlaneSlot::Primary,
    };
    let present_start_ns = now_ns();
    let result = crate::r::ui_surface::present_surface(surface, present, "ui3-frame");
    UI3_PRESENT_SURFACE_NS.fetch_add(elapsed_ns_since(present_start_ns), Ordering::Relaxed);
    let present_count = UI3_FRAME_PRESENTS.fetch_add(1, Ordering::Relaxed) + 1;
    match result {
        Ok(path) => {
            if swap_on_success {
                swap_frame_surfaces(frame_id, frame.front_surface, frame.back_surface);
            }
            log_present_stats(present_count, frame_id, frame, path);
            true
        }
        Err(err) => {
            let fail_count = UI3_FRAME_PRESENT_FAILS.fetch_add(1, Ordering::Relaxed) + 1;
            crate::log!(
                "ui3/frame: present failed#{} frame={} err={:?} size={}x{} dst={},{}\n",
                fail_count,
                frame_id,
                err,
                frame.width,
                frame.height,
                dst_x,
                dst_y
            );
            false
        }
    }
}

fn commit_frame_without_present(frame_id: u32) -> bool {
    let commit_start_ns = now_ns();
    let frame = {
        let frames = FRAMES.lock();
        let Some(frame) = frames.get(&frame_id) else {
            return false;
        };
        *frame
    };
    let _ = crate::r::ui_surface::flush_surface(frame.back_surface);
    swap_frame_surfaces(frame_id, frame.front_surface, frame.back_surface);
    UI3_COMMIT_NS.fetch_add(elapsed_ns_since(commit_start_ns), Ordering::Relaxed);
    true
}

fn log_present_stats(count: u64, frame_id: u32, frame: Ui3Frame, path: UiPresentPath) {
    if count > 8 && count % 60 != 0 {
        return;
    }
    crate::log!(
        "ui3/frame: present#{} frame={} path={:?} size={}x{} dst={},{} begin={} end={} sprite_calls={} sprite_bytes={} sprite_ms={} sprite_avg_us={} solid_calls={} solid_bytes={} solid_ms={} solid_avg_us={} begin_ms={} present_flush_ms={} present_surface_ms={} present_avg_ms={} commit_ms={} target_switches={} offscreen_allocs={} sprite_misses={}\n",
        count,
        frame_id,
        path,
        frame.width,
        frame.height,
        frame.x.max(0),
        frame.y.max(0),
        UI3_FRAME_BEGINS.load(Ordering::Relaxed),
        UI3_FRAME_ENDS.load(Ordering::Relaxed),
        UI3_DRAW_SPRITE_CALLS.load(Ordering::Relaxed),
        UI3_DRAW_SPRITE_BYTES.load(Ordering::Relaxed),
        ns_to_ms(UI3_DRAW_SPRITE_NS.load(Ordering::Relaxed)),
        avg_ns_to_us(
            UI3_DRAW_SPRITE_NS.load(Ordering::Relaxed),
            UI3_DRAW_SPRITE_CALLS.load(Ordering::Relaxed)
        ),
        UI3_DRAW_SOLID_CALLS.load(Ordering::Relaxed),
        UI3_DRAW_SOLID_BYTES.load(Ordering::Relaxed),
        ns_to_ms(UI3_DRAW_SOLID_NS.load(Ordering::Relaxed)),
        avg_ns_to_us(
            UI3_DRAW_SOLID_NS.load(Ordering::Relaxed),
            UI3_DRAW_SOLID_CALLS.load(Ordering::Relaxed)
        ),
        ns_to_ms(UI3_BEGIN_NS.load(Ordering::Relaxed)),
        ns_to_ms(UI3_PRESENT_FLUSH_NS.load(Ordering::Relaxed)),
        ns_to_ms(UI3_PRESENT_SURFACE_NS.load(Ordering::Relaxed)),
        avg_ns_to_ms(
            UI3_PRESENT_SURFACE_NS.load(Ordering::Relaxed),
            UI3_FRAME_PRESENTS.load(Ordering::Relaxed)
        ),
        ns_to_ms(UI3_COMMIT_NS.load(Ordering::Relaxed)),
        UI3_TARGET_SWITCHES.load(Ordering::Relaxed),
        UI3_OFFSCREEN_ALLOCS.load(Ordering::Relaxed),
        UI3_TEXTURE_MISSES.load(Ordering::Relaxed)
    );
}

#[inline]
fn now_ns() -> u64 {
    crate::chronos::monotonic_nanos()
}

#[inline]
fn elapsed_ns_since(start_ns: u64) -> u64 {
    now_ns().saturating_sub(start_ns)
}

#[inline]
fn ns_to_ms(ns: u64) -> u64 {
    ns / 1_000_000
}

#[inline]
fn avg_ns_to_us(total_ns: u64, count: u64) -> u64 {
    if count == 0 {
        0
    } else {
        total_ns / count / 1_000
    }
}

#[inline]
fn avg_ns_to_ms(total_ns: u64, count: u64) -> u64 {
    if count == 0 {
        0
    } else {
        total_ns / count / 1_000_000
    }
}

fn frame_snapshot(frame_id: u32) -> Option<Ui3Frame> {
    FRAMES.lock().get(&frame_id).copied()
}

fn swap_frame_surfaces(
    frame_id: u32,
    expected_front: crate::r::ui_surface::UiSurfaceHandle,
    expected_back: crate::r::ui_surface::UiSurfaceHandle,
) {
    let mut frames = FRAMES.lock();
    let Some(frame) = frames.get_mut(&frame_id) else {
        return;
    };
    if frame.front_surface != expected_front || frame.back_surface != expected_back {
        return;
    }
    core::mem::swap(&mut frame.front_surface, &mut frame.back_surface);
}

fn copy_surface_pixels(
    src_handle: crate::r::ui_surface::UiSurfaceHandle,
    dst_handle: crate::r::ui_surface::UiSurfaceHandle,
) -> bool {
    let Some(src) = crate::r::ui_surface::pixel_access(src_handle) else {
        return false;
    };
    let Some(dst) = crate::r::ui_surface::pixel_access(dst_handle) else {
        return false;
    };
    if src.format != dst.format {
        return false;
    }
    let row_bytes = src
        .width
        .min(dst.width)
        .saturating_mul(core::mem::size_of::<u32>() as u32) as usize;
    let rows = src.height.min(dst.height) as usize;
    if row_bytes == 0 || rows == 0 {
        return false;
    }
    let src_pitch = src.pitch as usize;
    let dst_pitch = dst.pitch as usize;
    if src_pitch < row_bytes || dst_pitch < row_bytes {
        return false;
    }
    for row in 0..rows {
        let src_off = row.saturating_mul(src_pitch);
        let dst_off = row.saturating_mul(dst_pitch);
        if src_off.saturating_add(row_bytes) > src.byte_len
            || dst_off.saturating_add(row_bytes) > dst.byte_len
        {
            return false;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(src.virt.add(src_off), dst.virt.add(dst_off), row_bytes);
        }
    }
    crate::r::ui_surface::flush_surface(dst_handle);
    true
}

fn ensure_offscreen(tex_id: u32, width: u32, height: u32, clear: bool, clear_rgb: u32) {
    let mut offscreen = OFFSCREEN.lock();
    let was_present = offscreen.contains_key(&tex_id);
    let image = offscreen
        .entry(tex_id)
        .or_insert_with(|| new_cpu_image(width, height));
    if !was_present {
        UI3_OFFSCREEN_ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
    if image.width != width || image.height != height {
        *image = new_cpu_image(width, height);
        UI3_OFFSCREEN_ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
    if clear {
        clear_pixels(
            &mut image.rgba,
            image.width,
            image.height,
            image.width as usize * 4,
            clear_rgb,
        );
    }
}

fn new_cpu_image(width: u32, height: u32) -> CpuImage {
    let width = width.max(1);
    let height = height.max(1);
    let len = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    let mut rgba = Vec::new();
    rgba.resize(len, 0);
    CpuImage {
        width,
        height,
        rgba,
    }
}

fn texture_source(tex_id: u32) -> Option<CpuImage> {
    if let Some(image) = OFFSCREEN.lock().get(&tex_id).cloned() {
        return Some(image);
    }
    crate::ui3::ui3_img::image_clone(tex_id).map(|image| CpuImage {
        width: image.width,
        height: image.height,
        rgba: image.rgba,
    })
}

fn raster_solid_batch(
    dst: &mut [u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    bytes: &[u8],
) {
    let item_bytes = SOLID_RECT_SIZE;
    let mut off = 0usize;
    while off + item_bytes <= bytes.len() {
        let Some(rect) = read_solid_rect_bytes(bytes, off) else {
            break;
        };
        raster_solid_rect(dst, pitch, width, height, format, rect);
        off += item_bytes;
    }
}

fn raster_sprite_batch(
    dst: &mut [u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    src: &CpuImage,
    bytes: &[u8],
) {
    let item_bytes = SPRITE_QUAD_SIZE;
    let mut off = 0usize;
    while off + item_bytes <= bytes.len() {
        let Some(quad) = read_sprite_quad_bytes(bytes, off) else {
            break;
        };
        raster_sprite_quad(dst, pitch, width, height, format, src, quad);
        off += item_bytes;
    }
}

fn raster_solid_rect(
    dst: &mut [u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    rect: SolidRect,
) {
    if !(rect.w > 0.0 && rect.h > 0.0) {
        return;
    }
    let max_w = width.max(1) as i32 - 1;
    let max_h = height.max(1) as i32 - 1;
    let min_x = floorf(rect.x).max(0.0) as i32;
    let min_y = floorf(rect.y).max(0.0) as i32;
    let max_x = (ceilf(rect.x + rect.w) as i32 - 1).min(max_w);
    let max_y = (ceilf(rect.y + rect.h) as i32 - 1).min(max_h);
    if min_x > max_x || min_y > max_y {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            blend_pixel(dst, pitch, x as u32, y as u32, format, rect.color);
        }
    }
}

fn raster_sprite_quad(
    dst: &mut [u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    src: &CpuImage,
    quad: SpriteQuad,
) {
    let SpriteQuad {
        c0,
        c1,
        c2,
        c3,
        color,
    } = quad;
    let ex = (c1.x - c0.x, c1.y - c0.y);
    let ey = (c3.x - c0.x, c3.y - c0.y);
    let det = ex.0 * ey.1 - ex.1 * ey.0;
    if fabsf(det) < 0.00001 {
        return;
    }
    let max_w = width.max(1) as i32 - 1;
    let max_h = height.max(1) as i32 - 1;
    let min_x = floorf(c0.x.min(c1.x).min(c2.x).min(c3.x)).max(0.0) as i32;
    let min_y = floorf(c0.y.min(c1.y).min(c2.y).min(c3.y)).max(0.0) as i32;
    let max_x = (ceilf(c0.x.max(c1.x).max(c2.x).max(c3.x)) as i32).min(max_w);
    let max_y = (ceilf(c0.y.max(c1.y).max(c2.y).max(c3.y)) as i32).min(max_h);
    if min_x > max_x || min_y > max_y {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 + 0.5 - c0.x;
            let dy = y as f32 + 0.5 - c0.y;
            let s = (dx * ey.1 - dy * ey.0) / det;
            let t = (ex.0 * dy - ex.1 * dx) / det;
            if (-0.0001..=1.0001).contains(&s) && (-0.0001..=1.0001).contains(&t) {
                let u = lerp2(c0.u, c1.u, c3.u, s, t);
                let v = lerp2(c0.v, c1.v, c3.v, s, t);
                let texel = sample_rgba(src, u, v);
                blend_pixel(dst, pitch, x as u32, y as u32, format, modulate(texel, color));
            }
        }
    }
}

#[inline]
fn lerp2(origin: f32, axis_x: f32, axis_y: f32, s: f32, t: f32) -> f32 {
    origin + (axis_x - origin) * s + (axis_y - origin) * t
}

fn sample_rgba(src: &CpuImage, u: f32, v: f32) -> Rgba8 {
    let max_x = src.width.max(1) as i32 - 1;
    let max_y = src.height.max(1) as i32 - 1;
    let x = ((u.clamp(0.0, 1.0) * src.width.max(1) as f32) as i32).clamp(0, max_x);
    let y = ((v.clamp(0.0, 1.0) * src.height.max(1) as f32) as i32).clamp(0, max_y);
    let off = (y as usize)
        .saturating_mul(src.width as usize)
        .saturating_add(x as usize)
        .saturating_mul(4);
    if off + 3 >= src.rgba.len() {
        return Rgba8::new(0, 0, 0, 0);
    }
    Rgba8::new(src.rgba[off], src.rgba[off + 1], src.rgba[off + 2], src.rgba[off + 3])
}

fn modulate(src: Rgba8, tint: Rgba8) -> Rgba8 {
    Rgba8::new(
        mul_u8(src.r, tint.r),
        mul_u8(src.g, tint.g),
        mul_u8(src.b, tint.b),
        mul_u8(src.a, tint.a),
    )
}

#[inline]
fn mul_u8(a: u8, b: u8) -> u8 {
    (((a as u16) * (b as u16) + 127) / 255) as u8
}

fn clear_pixels(dst: &mut [u8], width: u32, height: u32, pitch: usize, rgb: u32) {
    let color =
        Rgba8::new(((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8, 0xFF);
    for y in 0..height {
        for x in 0..width {
            let off = (y as usize)
                .saturating_mul(pitch)
                .saturating_add(x as usize * 4);
            if off + 3 < dst.len() {
                dst[off] = color.r;
                dst[off + 1] = color.g;
                dst[off + 2] = color.b;
                dst[off + 3] = color.a;
            }
        }
    }
}

fn blend_pixel(dst: &mut [u8], pitch: usize, x: u32, y: u32, format: UiSurfaceFormat, src: Rgba8) {
    let off = (y as usize)
        .saturating_mul(pitch)
        .saturating_add(x as usize * 4);
    if off + 3 >= dst.len() || src.a == 0 {
        return;
    }
    if src.a == 255 {
        write_pixel(dst, off, format, src.r, src.g, src.b, 255);
        return;
    }
    let inv = 255u16.saturating_sub(src.a as u16);
    let (dst_r, dst_g, dst_b, dst_a) = read_pixel(dst, off, format);
    let out_r = blend_channel(src.r, src.a, dst_r, inv);
    let out_g = blend_channel(src.g, src.a, dst_g, inv);
    let out_b = blend_channel(src.b, src.a, dst_b, inv);
    let out_a = src.a as u16 + (((dst_a as u16) * inv + 127) / 255);
    write_pixel(dst, off, format, out_r, out_g, out_b, out_a.min(255) as u8);
}

#[inline]
fn blend_channel(src: u8, src_a: u8, dst: u8, inv_a: u16) -> u8 {
    ((((src as u16) * (src_a as u16)) + ((dst as u16) * inv_a) + 127) / 255) as u8
}

#[inline]
fn read_pixel(dst: &[u8], off: usize, format: UiSurfaceFormat) -> (u8, u8, u8, u8) {
    match format {
        UiSurfaceFormat::Rgba8888 => (dst[off], dst[off + 1], dst[off + 2], dst[off + 3]),
        UiSurfaceFormat::Xrgb8888 => (dst[off + 2], dst[off + 1], dst[off], 255),
        UiSurfaceFormat::Xbgr8888 => (dst[off], dst[off + 1], dst[off + 2], 255),
    }
}

#[inline]
fn write_pixel(dst: &mut [u8], off: usize, format: UiSurfaceFormat, r: u8, g: u8, b: u8, a: u8) {
    match format {
        UiSurfaceFormat::Rgba8888 => {
            dst[off] = r;
            dst[off + 1] = g;
            dst[off + 2] = b;
            dst[off + 3] = a;
        }
        UiSurfaceFormat::Xrgb8888 => {
            dst[off] = b;
            dst[off + 1] = g;
            dst[off + 2] = r;
            dst[off + 3] = 0;
        }
        UiSurfaceFormat::Xbgr8888 => {
            dst[off] = r;
            dst[off + 1] = g;
            dst[off + 2] = b;
            dst[off + 3] = 0;
        }
    }
}
