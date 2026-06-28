use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use libm::{ceilf, fabsf, floorf};
use spin::Mutex;

use crate::intel::types::{
    RGB_VERTEX_SIZE, RgbVertex, Rgba8, TEX_VERTEX_SIZE, TexVertex, UiPlaneSlot, UiPresent,
    UiPresentPath, UiRect, UiSurfaceFormat, read_rgb_vertex_bytes, read_tex_vertex_bytes,
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
static UI3_DRAW_RGB_CALLS: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_RGB_BYTES: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_TEX_CALLS: AtomicU64 = AtomicU64::new(0);
static UI3_DRAW_TEX_BYTES: AtomicU64 = AtomicU64::new(0);
static UI3_TEXTURE_MISSES: AtomicU64 = AtomicU64::new(0);
static UI3_TARGET_SWITCHES: AtomicU64 = AtomicU64::new(0);
static UI3_OFFSCREEN_ALLOCS: AtomicU64 = AtomicU64::new(0);

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
    if allow_present && !present_frame(frame_id, true) {
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

pub(crate) fn draw_rgb_triangles(frame_id: u32, bytes: &[u8]) -> i32 {
    if bytes.len() % (RGB_VERTEX_SIZE * 3) != 0 {
        return -3;
    }
    UI3_DRAW_RGB_CALLS.fetch_add(1, Ordering::Relaxed);
    UI3_DRAW_RGB_BYTES.fetch_add(bytes.len() as u64, Ordering::Relaxed);
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
            raster_rgb_triangles(
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
            raster_rgb_triangles(
                &mut image.rgba,
                image.width as usize * 4,
                image.width,
                image.height,
                UiSurfaceFormat::Rgba8888,
                bytes,
            );
        }
    }
    0
}

pub(crate) fn draw_tex_triangles(frame_id: u32, tex_id: u32, bytes: &[u8]) -> i32 {
    if bytes.len() % (TEX_VERTEX_SIZE * 3) != 0 {
        return -3;
    }
    UI3_DRAW_TEX_CALLS.fetch_add(1, Ordering::Relaxed);
    UI3_DRAW_TEX_BYTES.fetch_add(bytes.len() as u64, Ordering::Relaxed);
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
            raster_tex_triangles(
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
            raster_tex_triangles(
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
    crate::r::ui_surface::flush_surface(surface);
    let dst_x = frame.x.max(0) as u32;
    let dst_y = frame.y.max(0) as u32;
    let present = UiPresent {
        src: UiRect::new(0, 0, frame.width, frame.height),
        dst: UiRect::new(dst_x, dst_y, frame.width, frame.height),
        plane: UiPlaneSlot::Primary,
    };
    let result = crate::r::ui_surface::present_surface(surface, present, "ui3-frame");
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

fn log_present_stats(count: u64, frame_id: u32, frame: Ui3Frame, path: UiPresentPath) {
    if count > 8 && count % 60 != 0 {
        return;
    }
    crate::log!(
        "ui3/frame: present#{} frame={} path={:?} size={}x{} dst={},{} begin={} end={} tex_calls={} tex_bytes={} rgb_calls={} rgb_bytes={} target_switches={} offscreen_allocs={} tex_misses={}\n",
        count,
        frame_id,
        path,
        frame.width,
        frame.height,
        frame.x.max(0),
        frame.y.max(0),
        UI3_FRAME_BEGINS.load(Ordering::Relaxed),
        UI3_FRAME_ENDS.load(Ordering::Relaxed),
        UI3_DRAW_TEX_CALLS.load(Ordering::Relaxed),
        UI3_DRAW_TEX_BYTES.load(Ordering::Relaxed),
        UI3_DRAW_RGB_CALLS.load(Ordering::Relaxed),
        UI3_DRAW_RGB_BYTES.load(Ordering::Relaxed),
        UI3_TARGET_SWITCHES.load(Ordering::Relaxed),
        UI3_OFFSCREEN_ALLOCS.load(Ordering::Relaxed),
        UI3_TEXTURE_MISSES.load(Ordering::Relaxed)
    );
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

fn raster_rgb_triangles(
    dst: &mut [u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    bytes: &[u8],
) {
    let tri_bytes = RGB_VERTEX_SIZE * 3;
    let mut off = 0usize;
    while off + tri_bytes <= bytes.len() {
        let Some(v0) = read_rgb_vertex_bytes(bytes, off) else {
            break;
        };
        let Some(v1) = read_rgb_vertex_bytes(bytes, off + RGB_VERTEX_SIZE) else {
            break;
        };
        let Some(v2) = read_rgb_vertex_bytes(bytes, off + RGB_VERTEX_SIZE * 2) else {
            break;
        };
        raster_rgb_triangle(dst, pitch, width, height, format, v0, v1, v2);
        off += tri_bytes;
    }
}

fn raster_tex_triangles(
    dst: &mut [u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    src: &CpuImage,
    bytes: &[u8],
) {
    let tri_bytes = TEX_VERTEX_SIZE * 3;
    let mut off = 0usize;
    while off + tri_bytes <= bytes.len() {
        let Some(v0) = read_tex_vertex_bytes(bytes, off) else {
            break;
        };
        let Some(v1) = read_tex_vertex_bytes(bytes, off + TEX_VERTEX_SIZE) else {
            break;
        };
        let Some(v2) = read_tex_vertex_bytes(bytes, off + TEX_VERTEX_SIZE * 2) else {
            break;
        };
        raster_tex_triangle(dst, pitch, width, height, format, src, v0, v1, v2);
        off += tri_bytes;
    }
}

fn raster_rgb_triangle(
    dst: &mut [u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    v0: RgbVertex,
    v1: RgbVertex,
    v2: RgbVertex,
) {
    let p0 = to_pixel(v0.x, v0.y, width, height);
    let p1 = to_pixel(v1.x, v1.y, width, height);
    let p2 = to_pixel(v2.x, v2.y, width, height);
    let Some((min_x, min_y, max_x, max_y, area)) = triangle_bounds(p0, p1, p2, width, height)
    else {
        return;
    };
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(p1, p2, p) / area;
            let w1 = edge(p2, p0, p) / area;
            let w2 = edge(p0, p1, p) / area;
            if inside(w0, w1, w2) {
                let color = mix_color(v0.color, v1.color, v2.color, w0, w1, w2);
                blend_pixel(dst, pitch, x as u32, y as u32, format, color);
            }
        }
    }
}

fn raster_tex_triangle(
    dst: &mut [u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    src: &CpuImage,
    v0: TexVertex,
    v1: TexVertex,
    v2: TexVertex,
) {
    let p0 = to_pixel(v0.x, v0.y, width, height);
    let p1 = to_pixel(v1.x, v1.y, width, height);
    let p2 = to_pixel(v2.x, v2.y, width, height);
    let Some((min_x, min_y, max_x, max_y, area)) = triangle_bounds(p0, p1, p2, width, height)
    else {
        return;
    };
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(p1, p2, p) / area;
            let w1 = edge(p2, p0, p) / area;
            let w2 = edge(p0, p1, p) / area;
            if inside(w0, w1, w2) {
                let u = v0.u * w0 + v1.u * w1 + v2.u * w2;
                let v = v0.v * w0 + v1.v * w1 + v2.v * w2;
                let texel = sample_rgba(src, u, v);
                let tint = mix_color(v0.color, v1.color, v2.color, w0, w1, w2);
                blend_pixel(dst, pitch, x as u32, y as u32, format, modulate(texel, tint));
            }
        }
    }
}

fn to_pixel(x: f32, y: f32, width: u32, height: u32) -> (f32, f32) {
    ((x + 1.0) * 0.5 * width.max(1) as f32, (1.0 - y) * 0.5 * height.max(1) as f32)
}

fn triangle_bounds(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    width: u32,
    height: u32,
) -> Option<(i32, i32, i32, i32, f32)> {
    let area = edge(p0, p1, p2);
    if fabsf(area) < 0.00001 {
        return None;
    }
    let max_w = width.max(1) as i32 - 1;
    let max_h = height.max(1) as i32 - 1;
    let min_x = floorf(p0.0.min(p1.0).min(p2.0)).max(0.0) as i32;
    let min_y = floorf(p0.1.min(p1.1).min(p2.1)).max(0.0) as i32;
    let max_x = (ceilf(p0.0.max(p1.0).max(p2.0)) as i32).min(max_w);
    let max_y = (ceilf(p0.1.max(p1.1).max(p2.1)) as i32).min(max_h);
    if min_x > max_x || min_y > max_y {
        return None;
    }
    Some((min_x, min_y, max_x, max_y, area))
}

#[inline]
fn edge(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    (c.0 - a.0) * (b.1 - a.1) - (c.1 - a.1) * (b.0 - a.0)
}

#[inline]
fn inside(w0: f32, w1: f32, w2: f32) -> bool {
    w0 >= -0.0001 && w1 >= -0.0001 && w2 >= -0.0001
}

fn mix_color(c0: Rgba8, c1: Rgba8, c2: Rgba8, w0: f32, w1: f32, w2: f32) -> Rgba8 {
    Rgba8::new(
        mix_u8(c0.r, c1.r, c2.r, w0, w1, w2),
        mix_u8(c0.g, c1.g, c2.g, w0, w1, w2),
        mix_u8(c0.b, c1.b, c2.b, w0, w1, w2),
        mix_u8(c0.a, c1.a, c2.a, w0, w1, w2),
    )
}

#[inline]
fn mix_u8(a: u8, b: u8, c: u8, w0: f32, w1: f32, w2: f32) -> u8 {
    ((a as f32 * w0 + b as f32 * w1 + c as f32 * w2).clamp(0.0, 255.0) + 0.5) as u8
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
