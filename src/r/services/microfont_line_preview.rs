//! Five-row A–Z, 0–9 and a–z line preview; missing glyphs leave empty cells.
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

use crate::ui4::{self, FrameHandle, WindowOwner};

const OWNER: WindowOwner = WindowOwner::MICROFONT_LINE_PREVIEW;
const SCALE: u32 = 16;
// Named presets are the only source of the scroll range. Native screen pixels.
const LINE_WIDTH_PX: f32 = (microfont::FontShape::Meso.line_width_px()
    + microfont::FontShape::Lean.line_width_px()) * 0.5;
const WIDTH_STEP_PX: f32 = 0.25;
const END_CORNER_FRACTION: f32 = 0.5;
const JOIN_RGBA: [u8; 4] = [255, 96, 32, 255];
const GLYPHS: &[u8; 65] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789   abcdefghijklmnopqrstuvwxyz";
const COLUMNS: usize = 13;
const WIDTH: u32 = COLUMNS as u32 * microfont::FWIDTH as u32 * SCALE;
const HEIGHT: u32 = 5 * microfont::FHEIGHT as u32 * SCALE;
// The bool retains the destination on uncertain GPU completion.
pub(super) type PaintResult = Result<usize, (&'static str, bool)>;
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
    let session = ui4::begin_window_session(OWNER).map_err(|_| "session-create")?;
    let mut frame = match create_preview_frame(output) {
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
            x: (screen_width.saturating_sub(WIDTH) / 2) as i32,
            y: (screen_height.saturating_sub(HEIGHT) / 2) as i32,
            width: WIDTH,
            height: HEIGHT,
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
    let lines = match render_frame(frame, width_px).await {
        Ok(lines) => lines,
        Err((reason, pinned)) => {
            let _ = ui4::finish_window_session(OWNER, session);
            if !pinned { ui4::retire_frame_when_released(frame); }
            return Err(reason);
        }
    };
    if ui4::publish_window_frame(OWNER, window, ui4::DamageRect::FULL).is_err() {
        let _ = ui4::finish_window_session(OWNER, session);
        ui4::retire_frame_when_released(frame);
        return Err("window-publish");
    }
    let present = GLYPHS.iter().filter(|&&glyph| microfont::raster_line::glyph_strips(glyph as char).is_some()).count();
    let strips: Vec<_> = GLYPHS.iter().filter_map(|&glyph| microfont::raster_line::glyph_strips(glyph as char))
        .flat_map(|strips| strips.iter()).collect();
    let join_triangles: usize = strips.iter().map(|s| s.iter_points().collect::<Vec<_>>().windows(3).filter(|triple| {
        let p = triple_pixels(triple);
        corner_triangle(p[0], p[1], p[2], LINE_WIDTH_PX).is_some()
    }).count()).sum();
    let real_vertices: usize = strips.iter().map(|s| s.points.len()).sum();
    let ends = strips.iter().flat_map(|s| [s.start, s.end])
        .filter(|&tag| tag == microfont::raster_line::Boundary::End).count();
    crate::log_important!(target: "microfont-line-preview";
        "microfont-line-preview: presented window={} frame={} size={}x{} scale={} cells=62 slots=65 present={} source=line-binary-v1 layout=A-M/N-Z/0-9/a-m/n-z rows=5 columns=13 primitives={} glyph_size=96x176 topology=line-strip-adj geometry_shader=off adjacency_removal=clip line_draws={} line_real_vertices={} adjacency_vertices={} tags=end:{},junction:{} width_px={} join_triangles_total={} join_color=orange end_corner_fraction={} cap_style=round radius_limit_px={} junction_trimming=off scroll_step_px=0.25 lean_px={} meso_px={} storage=none escape=close\n",
        window.raw(), frame.raw(), WIDTH, HEIGHT, SCALE, present, lines, strips.len(), real_vertices, 2 * strips.len(), ends, 2 * strips.len() - ends, LINE_WIDTH_PX, join_triangles, END_CORNER_FRACTION, LINE_WIDTH_PX * END_CORNER_FRACTION, microfont::FontShape::Lean.line_width_px(), microfont::FontShape::Meso.line_width_px());
    while !ui4::window_is_closed(OWNER, window) {
        let mut wheel = 0i32;
        for event in ui4::take_owner_input_events(OWNER) {
            if let ui4::Ui4InputEvent::Pointer(event) = event {
                if event.window == window { wheel = wheel.saturating_add(i32::from(event.wheel)); }
            }
        }
        let next_width = width_after_scroll(width_px, wheel);
        if next_width != width_px {
            // Render a new immutable buffer while the old front stays visible.
            // Never write into the frame that the compositor may still read.
            if let Ok(next_frame) = create_preview_frame(output) {
                match render_frame(next_frame, next_width).await {
                    Ok(_) => {
                        if ui4::commit_window_frame_refresh(OWNER, window, next_frame, ui4::DamageRect::FULL).is_ok() {
                            ui4::retire_frame_when_released(frame);
                            frame = next_frame;
                            width_px = next_width;
                            crate::log_important!(target: "microfont-line-preview";
                                "microfont-line-preview: width width_px={} cap_style=round radius_limit_px={} wheel={} lean_px={} meso_px={} storage=none\n",
                                width_px, width_px * END_CORNER_FRACTION, wheel,
                                microfont::FontShape::Lean.line_width_px(), microfont::FontShape::Meso.line_width_px());
                        } else { ui4::retire_frame_when_released(next_frame); }
                    }
                    Err((reason, pinned)) => {
                        if !pinned { ui4::retire_frame_when_released(next_frame); }
                        crate::log_important!(target: "microfont-line-preview";
                            "microfont-line-preview: width-render-failed reason={} retained={}\n", reason, pinned);
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

fn create_preview_frame(output: ui4::OutputId) -> Result<FrameHandle, &'static str> {
    ui4::create_frame(ui4::FrameSpec {
        output,
        content: ui4::FrameContent::BlueprintScene,
        cadence: ui4::FrameCadence::Immutable,
        buffering: ui4::FrameBuffering::Single,
        format: ui4::ScanoutFormat::Rgba8888Premultiplied,
        width: WIDTH,
        height: HEIGHT,
        base_color: None,
    }).map_err(|_| "frame-create")
}

async fn render_frame(frame: FrameHandle, width_px: f32) -> PaintResult {
    let reply = Arc::new(Reply::new());
    loop {
        loop {
            let response = Arc::clone(&reply);
            let job = Box::new(move || response.signal(paint_once(frame, width_px)));
            match crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "microfont-line-preview") {
                Ok(()) => break,
                Err(job) => { drop(job); Timer::after(Duration::from_millis(20)).await; }
            }
        }
        match reply.wait().await {
            Err(("render-busy" | "in-flight", false)) => Timer::after(Duration::from_millis(20)).await,
            result => return result,
        }
    }
}

fn triple_pixels(triple: &[microfont::raster_line::Point]) -> [[f32; 2]; 3] {
    core::array::from_fn(|k| [(triple[k].x() as f32 + 0.5) * SCALE as f32,
        (triple[k].y() as f32 + 0.5) * SCALE as f32])
}

fn screen_to_clip(cell: usize, point: [f32; 2]) -> [f32; 3] {
    [2.0 * ((cell % COLUMNS) as f32 * 6.0 * SCALE as f32 + point[0]) / WIDTH as f32 - 1.0,
     1.0 - 2.0 * ((cell / COLUMNS) as f32 * 11.0 * SCALE as f32 + point[1]) / HEIGHT as f32, 0.5]
}

/// A bevel patch uses the shared centre and the exposed endpoint of each
/// perpendicular half-width edge. Its winding is positive after the Y flip.
/// This diagnostic computes join patches; the stroke bodies remain native.
fn corner_triangle(previous: [f32; 2], at: [f32; 2], next: [f32; 2], width: f32)
    -> Option<[[f32; 2]; 3]>
{
    let mut incoming = [at[0] - previous[0], at[1] - previous[1]];
    let mut outgoing = [next[0] - at[0], next[1] - at[1]];
    let a = libm::sqrtf(incoming[0] * incoming[0] + incoming[1] * incoming[1]);
    let b = libm::sqrtf(outgoing[0] * outgoing[0] + outgoing[1] * outgoing[1]);
    if !width.is_finite() || width <= 0.0 || a <= 1e-6 || b <= 1e-6 { return None; }
    for k in 0..2 { incoming[k] /= a; outgoing[k] /= b; }
    let turn = incoming[0] * outgoing[1] - incoming[1] * outgoing[0];
    if turn.abs() <= 1e-6 { return None; }
    let side = if turn > 0.0 { 1.0 } else { -1.0 };
    let half = width * 0.5 * side;
    let mut triangle = [at,
        [at[0] + incoming[1] * half, at[1] - incoming[0] * half],
        [at[0] + outgoing[1] * half, at[1] - outgoing[0] * half]];
    let cross = (triangle[1][0] - at[0]) * (triangle[2][1] - at[1])
        - (triangle[1][1] - at[1]) * (triangle[2][0] - at[0]);
    if cross > 0.0 { triangle.swap(1, 2); }
    Some(triangle)
}

/// Positive wheel steps thicken the stroke; clamp to the named weight presets.
fn width_after_scroll(width_px: f32, wheel: i32) -> f32 {
    (width_px + wheel as f32 * WIDTH_STEP_PX).clamp(
        microfont::FontShape::Lean.line_width_px(), microfont::FontShape::Meso.line_width_px())
}

/// Endpoint frames stay in screen pixels, independent of seed-grid resolution.
fn rounded_end_params(strip: &microfont::raster_line::Strip, cell: usize, width_px: f32) -> Option<[u32; 16]> {
    use microfont::raster_line::Boundary::End;
    if strip.start != End && strip.end != End { return None; }
    let count = strip.points.len();
    let first = strip.point(0)?;
    let last = strip.point(count-1)?;
    // Repeated seed points do not define a direction; use the nearest distinct
    // point for each endpoint without altering the authored strip or its bytes.
    let first_neighbor = (1..count).find_map(|i| {
        let p = strip.point(i)?;
        (p != first).then_some(p)
    })?;
    let last_neighbor = (0..count-1).rev().find_map(|i| {
        let p = strip.point(i)?;
        (p != last).then_some(p)
    })?;
    let pts = [first, first_neighbor, last, last_neighbor];
    let mut params = [0.0f32; 16];
    for (offset, point, neighbor) in [(0, pts[0], pts[1]), (4, pts[2], pts[3])] {
        let dx = neighbor.x() as f32 - point.x() as f32;
        let dy = neighbor.y() as f32 - point.y() as f32;
        let length = libm::sqrtf(dx*dx + dy*dy);
        if length <= 1e-6 { return None; }
        params[offset..offset+4].copy_from_slice(&[
            ((cell % COLUMNS) as f32 * 6.0 + point.x() as f32 + 0.5) * SCALE as f32,
            ((cell / COLUMNS) as f32 * 11.0 + point.y() as f32 + 0.5) * SCALE as f32, dx/length, dy/length]);
    }
    params[8..12].copy_from_slice(&[width_px*0.5, width_px*END_CORNER_FRACTION,
        if strip.start == End {1.0} else {0.0}, if strip.end == End {1.0} else {0.0}]);
    Some(params.map(f32::to_bits))
}

fn paint_once(frame: FrameHandle, width_px: f32) -> PaintResult {
    use super::microfont_raster_preview::{PrimitiveBatch, paint_primitive_batches};
    use crate::intel::render::ResidentScenePrimitiveTopology::{LineStripAdj, TriangleList};
    // Each glyph can own several independent strips; each becomes its own draw.
    let mut geometry = Vec::new();
    for (index, &glyph) in GLYPHS.iter().enumerate() {
        let Some(strips) = microfont::raster_line::glyph_strips(glyph as char) else { continue; };
        for strip in strips {
            let mut vertices = Vec::with_capacity(strip.points.len() + 2);
            for i in 0..strip.points.len() + 2 {
                let [x, y, z] = strip.adjacency_vertex(i).ok_or(("line-vertex", false))?;
                let mut vertex = screen_to_clip(index, [(x + 0.5) * SCALE as f32, (y + 0.5) * SCALE as f32]);
                vertex[2] = z;
                vertices.push(vertex);
            }
            let indices: Vec<u32> = (0..vertices.len() as u32).collect();
            geometry.push((vertices, indices, LineStripAdj, width_px, true, None, rounded_end_params(strip, index, width_px)));
        }
    }
    for (cell, &glyph) in GLYPHS.iter().enumerate() {
        let Some(strips) = microfont::raster_line::glyph_strips(glyph as char) else { continue; };
        let mut vertices = Vec::new();
        for strip in strips {
            let points: Vec<_> = strip.iter_points().collect();
            for triple in points.windows(3) {
                let p = triple_pixels(triple);
                let Some(triangle) = corner_triangle(p[0], p[1], p[2], width_px) else { continue; };
                for point in triangle { vertices.push(screen_to_clip(cell, point)); }
            }
        }
        if vertices.is_empty() { continue; }
        let indices: Vec<u32> = (0..vertices.len() as u32).collect();
        geometry.push((vertices, indices, TriangleList, 0.0, false, Some(JOIN_RGBA), None));
    }
    let batches: Vec<_> = geometry.iter().map(|(vertices, indices, topology, width, clip, rgba, rounded_ends)| PrimitiveBatch {
        vertices, indices, topology: *topology, line_width_px: *width,
        clip_adjacency: *clip, rgba: *rgba, rounded_ends: *rounded_ends,
    }).collect();
    let primitives = paint_primitive_batches(frame, &batches, [48, 100, 216, 255])?;
    Ok(primitives)
}
