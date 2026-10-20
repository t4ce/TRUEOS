//! Six-row letter, digit and @/%/$ preview; missing glyphs leave empty cells.
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

use crate::ui4::{self, FrameHandle, WindowOwner};

const OWNER: WindowOwner = WindowOwner::MICROFONT_LINE_PREVIEW;
// Native screen pixels; allow thinner strokes below Lean for small glyphs.
const LINE_WIDTH_PX: f32 =
    (microfont::FontShape::Meso.line_width_px() + microfont::FontShape::Lean.line_width_px()) * 0.5;
const MIN_LINE_WIDTH_PX: f32 = 1.0;
const WIDTH_STEP_PX: f32 = 0.25;
const END_CORNER_FRACTION: f32 = 0.5;
const GLYPHS: &[u8; 78] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789   abcdefghijklmnopqrstuvwxyz@%$          ";
const COLUMNS: usize = 13;
// Font size means the complete eleven-row em cell, including descender space.
// Use the same EDID-to-active-scanout conversion as gridpaper; no guessed DPI.
fn glyph_height_pixels(step: u8) -> Result<u32, &'static str> {
    crate::intel::physical_extent_pixels_fraction(10, u32::from(step) * 5, 2)
        .map(|(_, height)| height)
        .ok_or("physical-display-size-unavailable")
}
fn size_after_tabs(step: u8, tabs: u32) -> u8 {
    ((u32::from(step) - 2 + tabs % 15) % 15 + 2) as u8
}
fn frame_extent(height: u32, line_width_px: f32) -> (u32, u32) {
    let (width, height) = super::microfont_mask_service::AtlasConfig {
        glyph_height_px: Some(height), line_width_px,
        ..super::microfont_mask_service::AtlasConfig::default()
    }.cell_extent();
    (COLUMNS as u32 * width, 6 * height + super::microfont_mask_service::PREVIEW_HEADER_HEIGHT)
}
type PaintResult = Result<usize, &'static str>;
type Reply = Signal<crate::wait::EmbassySpinRawMutex, PaintResult>;

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfont_line_preview_task() {
    if let Err(reason) = show_once().await {
        crate::log_important!(target: "microfont-line-preview";
            "microfont-line-preview: failed reason={}\n", reason);
    }
}

async fn show_once() -> Result<(), &'static str> {
    let output = ui4::OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("scanout-unavailable")?;
    let mut size_step = 2;
    let mut glyph_height = glyph_height_pixels(size_step)?;
    let (width, height) = frame_extent(glyph_height, LINE_WIDTH_PX);
    let session = ui4::begin_window_session(OWNER).map_err(|_| "session-create")?;
    let mut frame = match create_preview_frame(output, glyph_height, LINE_WIDTH_PX) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = ui4::finish_window_session(OWNER, session);
            return Err("frame-create");
        }
    };
    let window = match ui4::create_window(ui4::WindowCreate {
        owner: OWNER,
        session,
        frame,
        output,
        plane: ui4::WindowPlane::Universal(ui4::ALPHA_OVERLAY_PLANE_SLOT as u8),
        placement: ui4::WindowPlacement {
            x: (screen_width.saturating_sub(width) / 2) as i32,
            y: (screen_height.saturating_sub(height) / 2) as i32,
            width,
            height,
            z: 32,
            opacity: 255,
            visible: true,
        },
        interaction: ui4::WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = ui4::finish_window_session(OWNER, session);
            let _ = ui4::destroy_frame(frame);
            return Err("window-create");
        }
    };
    let mut width_px = LINE_WIDTH_PX;
    let lines = match render_frame(frame, width_px, glyph_height, size_step).await {
        Ok(lines) => lines,
        Err(reason) => {
            let _ = ui4::finish_window_session(OWNER, session);
            ui4::retire_frame_when_released(frame);
            return Err(reason);
        }
    };
    if ui4::publish_window_frame(OWNER, window, ui4::DamageRect::FULL).is_err() {
        let _ = ui4::finish_window_session(OWNER, session);
        ui4::retire_frame_when_released(frame);
        return Err("window-publish");
    }
    let present = GLYPHS
        .iter()
        .filter(|&&glyph| microfont::raster_line::glyph_strips(glyph as char).is_some())
        .count();
    let strips: Vec<_> = GLYPHS
        .iter()
        .filter_map(|&glyph| microfont::raster_line::glyph_strips(glyph as char))
        .flat_map(|strips| strips.iter())
        .collect();
    let join_triangles: usize = strips
        .iter()
        .map(|s| {
            let mut recipes = Vec::new();
            super::microfont_mask_service::append_strip_joins(&mut recipes, 0, s);
            recipes.len() / 3
        })
        .sum();
    let real_vertices: usize = strips.iter().map(|s| s.points.len()).sum();
    let ends = strips
        .iter()
        .flat_map(|s| [s.start, s.end])
        .filter(|&tag| tag == microfont::raster_line::Boundary::End)
        .count();
    crate::log_important!(target: "microfont-line-preview";
        "microfont-line-preview: presented window={} frame={} size={}x{} em_height_px={} cells=65 slots=78 present={} source=line-binary-v1 layout=A-M/N-Z/0-9/a-m/n-z/@%$ rows=6 columns=13 ink_pixels={} size_mm=5 sizing=eleven-row-em tab=5..40mm-step2.5 topology=line-strip-adj geometry_shader=off adjacency_removal=clip line_draws={} line_real_vertices={} adjacency_vertices={} tags=end:{},junction:{} width_px={} join_triangles_total={} join_color=ink source_mask=r8 background=transparent colors=rainbow alpha_rows=255..128 geometry_uploads=1 geometry_reuploads=0 end_corner_fraction={} aa=native-lines+analytic-patches+analytic-caps samples=1 cap_style=round radius_limit_px={} junction_trimming=off scroll_step_px=0.25 scroll_min_px={} lean_px={} meso_px={} storage=none escape=close\n",
        window.raw(), frame.raw(), width, height, glyph_height, present, lines, strips.len(), real_vertices, 2 * strips.len(), ends, 2 * strips.len() - ends, LINE_WIDTH_PX, join_triangles, END_CORNER_FRACTION, LINE_WIDTH_PX * END_CORNER_FRACTION, MIN_LINE_WIDTH_PX, microfont::FontShape::Lean.line_width_px(), microfont::FontShape::Meso.line_width_px());
    while !ui4::window_is_closed(OWNER, window) {
        let mut wheel = 0i32;
        let mut tabs = 0u32;
        for event in ui4::take_owner_input_events(OWNER) {
            match event {
                ui4::Ui4InputEvent::Pointer(event) if event.window == window => {
                    wheel = wheel.saturating_add(i32::from(event.wheel));
                }
                ui4::Ui4InputEvent::Keyboard(event) if event.window == window => {
                    use crate::r::keyboard::*;
                    if event.event.kind == KEYBOARD_OUTPUT_KIND_KEY
                        && event.event.key_code == KEYBOARD_KEY_TAB
                        && event.event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0
                        && event.event.flags & KEYBOARD_OUTPUT_FLAG_REPEAT == 0
                    { tabs += 1; }
                }
                _ => {}
            }
        }
        let next_width = width_after_scroll(width_px, wheel);
        let next_step = size_after_tabs(size_step, tabs);
        let next_height = if next_step == size_step { glyph_height } else { glyph_height_pixels(next_step)? };
        if next_width != width_px || next_step != size_step {
            // Replace the immutable front only when the new native-size render is ready.
            if let Ok(next_frame) = create_preview_frame(output, next_height, next_width) {
                match render_frame(next_frame, next_width, next_height, next_step).await {
                    Ok(_) => {
                        let committed = if next_step != size_step || frame_extent(next_height, next_width) != frame_extent(glyph_height, width_px) {
                            ui4::window_resize_state(OWNER, window).and_then(|(mut placement, epoch)| {
                                (placement.width, placement.height) = frame_extent(next_height, next_width);
                                ui4::commit_window_frame_replacement(
                                    OWNER, window, next_frame, placement, epoch, ui4::DamageRect::FULL)
                            })
                        } else {
                            ui4::commit_window_frame_refresh(OWNER, window, next_frame, ui4::DamageRect::FULL)
                        };
                        if committed.is_ok() {
                            ui4::retire_frame_when_released(frame);
                            frame = next_frame;
                            width_px = next_width;
                            size_step = next_step;
                            glyph_height = next_height;
                            let (width, height) = frame_extent(glyph_height, width_px);
                            crate::log_important!(target: "microfont-line-preview";
                                "microfont-line-preview: adjusted size_mm={}.{} em_height_px={} frame={}x{} width_px={} wheel={} tabs={} geometry_reuploads=0 storage=none\n",
                                size_step * 5 / 2, (size_step % 2) * 5, glyph_height, width, height, width_px, wheel, tabs);
                        } else {
                            ui4::retire_frame_when_released(next_frame);
                        }
                    }
                    Err(reason) => {
                        ui4::retire_frame_when_released(next_frame);
                        crate::log_important!(target: "microfont-line-preview";
                            "microfont-line-preview: render-failed reason={}\n", reason);
                    }
                }
            }
        }
        Timer::after(Duration::from_millis(16)).await;
    }
    let _ = ui4::take_owner_input_events(OWNER);
    let _ = ui4::finish_window_session(OWNER, session);
    ui4::retire_frame_when_released(frame);
    Ok(())
}

fn create_preview_frame(output: ui4::OutputId, glyph_height: u32, width_px: f32) -> Result<FrameHandle, &'static str> {
    let (width, height) = frame_extent(glyph_height, width_px);
    ui4::create_frame(ui4::FrameSpec {
        output,
        content: ui4::FrameContent::BlueprintScene,
        cadence: ui4::FrameCadence::Immutable,
        buffering: ui4::FrameBuffering::Single,
        format: ui4::ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: None,
    })
    .map_err(|_| "frame-create")
}

async fn render_frame(frame: FrameHandle, width_px: f32, glyph_height: u32, size_step: u8) -> PaintResult {
    let reply = Arc::new(Reply::new());
    loop {
        loop {
            let response = Arc::clone(&reply);
            let job = Box::new(move || response.signal(paint_once(frame, width_px, glyph_height, size_step)));
            match crate::r::blocking::try_spawn_blocking_job_with_purpose(
                job,
                "microfont-line-preview",
            ) {
                Ok(()) => break,
                Err(job) => {
                    drop(job);
                    Timer::after(Duration::from_millis(20)).await;
                }
            }
        }
        match reply.wait().await {
            Err("render-busy" | "in-flight" | "render-storage-busy" | "microfont-atlas-busy") => {
                Timer::after(Duration::from_millis(20)).await
            }
            result => return result,
        }
    }
}

/// Positive wheel steps thicken the stroke; permit 1px through Meso.
fn width_after_scroll(width_px: f32, wheel: i32) -> f32 {
    (width_px + wheel as f32 * WIDTH_STEP_PX).clamp(
        MIN_LINE_WIDTH_PX,
        microfont::FontShape::Meso.line_width_px(),
    )
}

fn paint_once(frame: FrameHandle, width_px: f32, glyph_height: u32, size_step: u8) -> PaintResult {
    use super::microfont_mask_service::{self as masks, AtlasConfig};
    let glyphs: Vec<_> = GLYPHS.iter().map(|&ch| ch as char).collect();
    let inks = preview_inks();
    let label = alloc::format!("{:.2} px   {},{:02} cm (Tab)", width_px, size_step / 4, (size_step % 4) * 25);
    masks::stamp_preview_colored_with_header(
        frame,
        AtlasConfig {
            glyph_height_px: Some(glyph_height),
            line_width_px: width_px,
            ..AtlasConfig::default()
        },
        &glyphs,
        COLUMNS,
        1, // native-pixel copy; physical config supplies the cell extent
        &inks,
        [0; 4],
        Some(&label),
    )
}

/// Rainbow varies across all named glyphs; each row independently fades alpha.
fn preview_inks() -> Vec<[u8; 4]> {
    let count = GLYPHS.iter().filter(|&&ch| ch != b' ').count();
    let mut ordinal = 0;
    let mut inks = Vec::with_capacity(GLYPHS.len());
    for row in GLYPHS.chunks(COLUMNS) {
        let letters = row.iter().filter(|&&ch| ch != b' ').count();
        let mut column = 0;
        for &ch in row {
            if ch == b' ' { inks.push([0; 4]); continue; }
            let [r, g, b] = rainbow_rgb(ordinal, count);
            let alpha = if letters <= 1 { 255 } else {
                let intervals = letters - 1;
                ((255 * (intervals - column) + 128 * column + intervals / 2) / intervals) as u8
            };
            inks.push([r, g, b, alpha]);
            column += 1;
            ordinal += 1;
        }
    }
    inks
}

fn rainbow_rgb(ordinal: usize, count: usize) -> [u8; 3] {
    let phase = ordinal * 1536 / count.max(1);
    let t = (phase % 256) as u8;
    match phase / 256 {
        0 => [255, t, 0],
        1 => [255 - t, 255, 0],
        2 => [0, 255, t],
        3 => [0, 255 - t, 255],
        4 => [t, 0, 255],
        _ => [255, 0, 255 - t],
    }
}
