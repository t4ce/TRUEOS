//! Experimental Blueprint transport for Solara's text-only UI4 producer.
//!
//! The kernel derives window ownership from the active Blueprint VM. Callers
//! provide text and placement only; frame handles and writable surfaces never
//! cross the ABI boundary.

use alloc::{string::String, vec::Vec};
use spin::Mutex;

use crate::intel::gpu_font::{
    GpuFontFace, GpuFontJob, GpuFontJobEntry, GpuFontTextRequest, MAX_DYNAMIC_TEXT_CHARS,
    recycle_font_job_readback, render_font_job_readback_once,
};

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, WindowCreate, WindowId,
    WindowOwner, WindowPlacement, WindowSessionId, acquire_frame_buffer, begin_window_session,
    cancel_frame_buffer, create_frame, create_window, destroy_frame, finish_window_session,
    publish_frame_buffer, publish_window_frame, writable_rgba_view,
};

const MAX_SURFACES: usize = 32;
const MAX_FRAME_WIDTH: u32 = 2_560;
const MAX_FRAME_HEIGHT: u32 = 1_440;
const MAX_TEXT_ROWS: usize = 64;
const MAX_TEXT_ROW_BYTES: usize = 1_024;

const ERROR_INVALID: i32 = -1;
const ERROR_CONTEXT: i32 = -2;
const ERROR_NOT_FOUND: i32 = -3;
const ERROR_STATE: i32 = -4;
const ERROR_FONT: i32 = -5;
const ERROR_UI4: i32 = -6;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4SolaraFontSize {
    pub native_scale: u32,
    pub target_pixels: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosUi4SolaraTextRow {
    pub text_ptr: *const u8,
    pub text_len: usize,
    pub x: f32,
    pub y: f32,
}

struct SolaraTextSurface {
    owner: WindowOwner,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    width: u32,
    height: u32,
    write_lease: Option<FrameWriteLease>,
}

static SURFACES: Mutex<Vec<SolaraTextSurface>> = Mutex::new(Vec::new());
static RETIRED_FRAMES: Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());

/// Enumerate the kernel font service's native render-target sizes.
///
/// A null output with zero capacity is a size query. The return value is the
/// total number of supported entries, even when the provided output is short.
pub unsafe extern "C" fn trueos_cabi_ui4_solara_font_sizes(
    out: *mut TrueosUi4SolaraFontSize,
    out_cap: usize,
) -> isize {
    if out.is_null() && out_cap != 0 {
        return ERROR_INVALID as isize;
    }

    let count = crate::intel::render::font_native_scale_count() as usize;
    for (slot, native_scale) in (1..=count as u32).take(out_cap.min(count)).enumerate() {
        let Some(target_pixels) =
            crate::intel::render::font_native_scale_target_pixels(native_scale)
        else {
            return ERROR_FONT as isize;
        };
        // SAFETY: the Blueprint caller supplied capacity for this slot.
        unsafe {
            out.add(slot).write(TrueosUi4SolaraFontSize {
                native_scale,
                target_pixels,
            });
        }
    }
    count as isize
}

/// Create one dirty-cadence UI4 frame and broker window for the active VM.
pub extern "C" fn trueos_cabi_ui4_solara_frame_open(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> u32 {
    reap_retired_frames();
    if width == 0 || height == 0 || width > MAX_FRAME_WIDTH || height > MAX_FRAME_HEIGHT {
        return 0;
    }
    let Some(owner) = blueprint_owner() else {
        return 0;
    };

    let old = {
        let mut surfaces = SURFACES.lock();
        surfaces
            .iter()
            .position(|surface| surface.owner == owner)
            .map(|slot| surfaces.remove(slot))
    };
    if let Some(old) = old {
        release_surface(old);
    }

    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let frame = match create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        cadence: FrameCadence::Dirty,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    }) {
        Ok(frame) => frame,
        Err(error) => {
            crate::log_error!(target: "ui4/solara-text"; "frame open allocation failed owner={:?} error={:?}\n", owner, error);
            return 0;
        }
    };
    let session = match begin_window_session(owner) {
        Ok(session) => session,
        Err(error) => {
            let _ = destroy_frame(frame);
            crate::log_error!(target: "ui4/solara-text"; "frame open session failed owner={:?} error={:?}\n", owner, error);
            return 0;
        }
    };
    let window = match create_window(WindowCreate {
        owner,
        session,
        frame,
        output,
        placement: WindowPlacement {
            x,
            y,
            width,
            height,
            z: 40,
            opacity: u8::MAX,
            visible: true,
        },
    }) {
        Ok(window) => window,
        Err(error) => {
            let _ = finish_window_session(owner, session);
            let _ = destroy_frame(frame);
            crate::log_error!(target: "ui4/solara-text"; "frame open window failed owner={:?} error={:?}\n", owner, error);
            return 0;
        }
    };

    let mut surfaces = SURFACES.lock();
    if surfaces.len() >= MAX_SURFACES {
        drop(surfaces);
        release_surface(SolaraTextSurface {
            owner,
            session,
            frame,
            window,
            width,
            height,
            write_lease: None,
        });
        return 0;
    }
    surfaces.push(SolaraTextSurface {
        owner,
        session,
        frame,
        window,
        width,
        height,
        write_lease: None,
    });
    crate::log_info!(target: "ui4/solara-text"; "frame open owner={:?} window={} extent={}x{} cadence=dirty buffers=2\n", owner, window.raw(), width, height);
    window.raw()
}

/// Acquire and clear the non-front UI4 buffer for a new Solara paint pass.
pub extern "C" fn trueos_cabi_ui4_solara_frame_begin(window_id: u32, clear_rgba: u32) -> i32 {
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease.is_some() {
        return ERROR_STATE;
    }
    let lease = match acquire_frame_buffer(surface.frame) {
        Ok(lease) => lease,
        Err(_) => return ERROR_UI4,
    };
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return ERROR_UI4;
        }
    };
    let [r, g, b, a] = clear_rgba.to_le_bytes();
    let pixel = PremultipliedRgba8::from_straight_rgba(r, g, b, a).to_native_bytes();
    // SAFETY: the write lease makes this entire UI4 allocation producer-owned.
    let bytes = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    for chunk in bytes.chunks_exact_mut(4) {
        chunk.copy_from_slice(&pixel);
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    surface.write_lease = Some(lease);
    0
}

/// Render one positioned text-row job through the kernel font service and
/// blend its coverage into the currently acquired UI4 frame.
pub unsafe extern "C" fn trueos_cabi_ui4_solara_text_rows(
    window_id: u32,
    font_id: u32,
    native_scale: u32,
    dst_x: i32,
    dst_y: i32,
    rgba: u32,
    rows: *const TrueosUi4SolaraTextRow,
    row_count: usize,
) -> i32 {
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let Some(font) = GpuFontFace::from_id(font_id) else {
        return ERROR_FONT;
    };
    if crate::intel::render::font_native_scale_target_pixels(native_scale).is_none()
        || rows.is_null()
        || row_count == 0
        || row_count > MAX_TEXT_ROWS
    {
        return ERROR_INVALID;
    }

    // Copy all guest text before entering the font renderer. The job entries
    // below then borrow only kernel-owned, validated UTF-8 strings.
    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut strings = Vec::<String>::with_capacity(row_count);
    let mut positions = Vec::<[f32; 2]>::with_capacity(row_count);
    for row in input {
        if row.text_ptr.is_null()
            || row.text_len == 0
            || row.text_len > MAX_TEXT_ROW_BYTES
            || !row.x.is_finite()
            || !row.y.is_finite()
        {
            return ERROR_INVALID;
        }
        let bytes = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        let Ok(text) = core::str::from_utf8(bytes) else {
            return ERROR_INVALID;
        };
        if text.chars().count() > MAX_DYNAMIC_TEXT_CHARS {
            return ERROR_INVALID;
        }
        strings.push(String::from(text));
        positions.push([row.x, row.y]);
    }
    let entries: Vec<_> = strings
        .iter()
        .zip(positions.iter())
        .map(|(text, position)| GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine(text.as_str()),
            position: *position,
        })
        .collect();
    let readback = match render_font_job_readback_once(GpuFontJob {
        entries: entries.as_slice(),
        font,
        native_scale,
    }) {
        Ok(readback) => readback,
        Err(reason) => {
            crate::log_warn!(target: "ui4/solara-text"; "text render rejected owner={:?} window={} reason={}\n", owner, window_id, reason);
            return ERROR_FONT;
        }
    };

    let result = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            recycle_font_job_readback(readback);
            return ERROR_NOT_FOUND;
        };
        let Some(lease) = surface.write_lease else {
            recycle_font_job_readback(readback);
            return ERROR_STATE;
        };
        match writable_rgba_view(lease) {
            Ok(view) => {
                blend_font_coverage(view, &readback, dst_x, dst_y, rgba);
                crate::intel::dma_flush(view.virt, view.byte_len);
                0
            }
            Err(_) => ERROR_UI4,
        }
    };
    recycle_font_job_readback(readback);
    result
}

/// Publish the completed dirty buffer and its window damage.
pub extern "C" fn trueos_cabi_ui4_solara_frame_publish(
    window_id: u32,
    damage_x: u32,
    damage_y: u32,
    damage_width: u32,
    damage_height: u32,
) -> i32 {
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    if damage_width == 0 || damage_height == 0 {
        return ERROR_INVALID;
    }
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if damage_x >= surface.width || damage_y >= surface.height {
        return ERROR_INVALID;
    }
    let damage = DamageRect {
        x: damage_x,
        y: damage_y,
        width: damage_width.min(surface.width - damage_x),
        height: damage_height.min(surface.height - damage_y),
    };
    let Some(lease) = surface.write_lease.take() else {
        return ERROR_STATE;
    };
    if publish_frame_buffer(lease).is_err() {
        return ERROR_UI4;
    }
    if damage.width == 0
        || damage.height == 0
        || publish_window_frame(owner, surface.window, damage).is_err()
    {
        return ERROR_UI4;
    }
    0
}

pub extern "C" fn trueos_cabi_ui4_solara_frame_close(window_id: u32) -> i32 {
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let surface = {
        let mut surfaces = SURFACES.lock();
        let Some(slot) = surfaces
            .iter()
            .position(|surface| surface.owner == owner && surface.window.raw() == window_id)
        else {
            return ERROR_NOT_FOUND;
        };
        surfaces.remove(slot)
    };
    release_surface(surface);
    0
}

fn blueprint_owner() -> Option<WindowOwner> {
    crate::hv::current_guest_execution_context_vm_id().map(WindowOwner::Vm)
}

fn surface_mut(
    surfaces: &mut [SolaraTextSurface],
    owner: WindowOwner,
    window_id: u32,
) -> Option<&mut SolaraTextSurface> {
    surfaces
        .iter_mut()
        .find(|surface| surface.owner == owner && surface.window.raw() == window_id)
}

fn release_surface(mut surface: SolaraTextSurface) {
    if let Some(lease) = surface.write_lease.take() {
        let _ = cancel_frame_buffer(lease);
    }
    let _ = finish_window_session(surface.owner, surface.session);
    if let Err(error) = destroy_frame(surface.frame) {
        if error == FramePoolError::Busy {
            RETIRED_FRAMES.lock().push(surface.frame);
        }
        crate::log_warn!(target: "ui4/solara-text"; "frame close deferred owner={:?} window={} error={:?}\n", surface.owner, surface.window.raw(), error);
    }
}

fn reap_retired_frames() {
    RETIRED_FRAMES
        .lock()
        .retain(|frame| matches!(destroy_frame(*frame), Err(FramePoolError::Busy)));
}

fn blend_font_coverage(
    destination: super::FrameRgbaView,
    source: &crate::intel::render::FontRenderTargetReadback,
    dst_x: i32,
    dst_y: i32,
    rgba: u32,
) {
    let [red, green, blue, color_alpha] = rgba.to_le_bytes();
    let destination_len = (destination.pitch as usize)
        .saturating_mul(destination.height as usize)
        .min(destination.byte_len);
    // SAFETY: the caller holds the unique UI4 write lease for this view.
    let destination_pixels =
        unsafe { core::slice::from_raw_parts_mut(destination.virt, destination_len) };
    let source_pitch = source.width as usize * 4;

    for source_y in 0..source.height as usize {
        let target_y = dst_y.saturating_add(source_y as i32);
        if target_y < 0 || target_y >= destination.height as i32 {
            continue;
        }
        for source_x in 0..source.width as usize {
            let target_x = dst_x.saturating_add(source_x as i32);
            if target_x < 0 || target_x >= destination.width as i32 {
                continue;
            }
            let source_offset = source_y * source_pitch + source_x * 4;
            let coverage = source.pixels[source_offset + 3];
            if coverage == 0 {
                continue;
            }
            let source_alpha = mul_div_255(coverage, color_alpha);
            let inverse_alpha = u8::MAX - source_alpha;
            let target_offset =
                target_y as usize * destination.pitch as usize + target_x as usize * 4;
            if target_offset + 4 > destination_pixels.len() {
                continue;
            }
            let target = &mut destination_pixels[target_offset..target_offset + 4];
            target[0] = mul_div_255(red, source_alpha)
                .saturating_add(mul_div_255(target[0], inverse_alpha));
            target[1] = mul_div_255(green, source_alpha)
                .saturating_add(mul_div_255(target[1], inverse_alpha));
            target[2] = mul_div_255(blue, source_alpha)
                .saturating_add(mul_div_255(target[2], inverse_alpha));
            target[3] = source_alpha.saturating_add(mul_div_255(target[3], inverse_alpha));
        }
    }
}

const fn mul_div_255(left: u8, right: u8) -> u8 {
    ((left as u16 * right as u16 + 127) / 255) as u8
}
