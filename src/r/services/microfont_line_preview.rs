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
        super::microfont_mask_service::corner_triangle(p[0], p[1], p[2], LINE_WIDTH_PX).is_some()
    }).count()).sum();
    let real_vertices: usize = strips.iter().map(|s| s.points.len()).sum();
    let ends = strips.iter().flat_map(|s| [s.start, s.end])
        .filter(|&tag| tag == microfont::raster_line::Boundary::End).count();
    crate::log_important!(target: "microfont-line-preview";
        "microfont-line-preview: presented window={} frame={} size={}x{} scale={} cells=62 slots=65 present={} source=line-binary-v1 layout=A-M/N-Z/0-9/a-m/n-z rows=5 columns=13 ink_pixels={} glyph_size=96x176 topology=line-strip-adj geometry_shader=off adjacency_removal=clip line_draws={} line_real_vertices={} adjacency_vertices={} tags=end:{},junction:{} width_px={} join_triangles_total={} join_color=ink source_mask=r8 geometry_uploads=1 geometry_reuploads=0 end_corner_fraction={} aa=native-lines+analytic-patches+analytic-caps samples=1 cap_style=round radius_limit_px={} junction_trimming=off scroll_step_px=0.25 lean_px={} meso_px={} storage=none escape=close\n",
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
                                "microfont-line-preview: width width_px={} aa=native-lines+analytic-patches+analytic-caps samples=1 cap_style=round radius_limit_px={} wheel={} lean_px={} meso_px={} storage=none\n",
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
            Err(("render-busy" | "in-flight" | "render-storage-busy" | "microfont-atlas-busy", false)) => Timer::after(Duration::from_millis(20)).await,
            result => return result,
        }
    }
}

fn triple_pixels(triple: &[microfont::raster_line::Point]) -> [[f32; 2]; 3] {
    core::array::from_fn(|k| [(triple[k].x() as f32 + 0.5) * SCALE as f32,
        (triple[k].y() as f32 + 0.5) * SCALE as f32])
}

/// Positive wheel steps thicken the stroke; clamp to the named weight presets.
fn width_after_scroll(width_px: f32, wheel: i32) -> f32 {
    (width_px + wheel as f32 * WIDTH_STEP_PX).clamp(
        microfont::FontShape::Lean.line_width_px(), microfont::FontShape::Meso.line_width_px())
}

fn paint_once(frame: FrameHandle, width_px: f32) -> PaintResult {
    use super::microfont_mask_service::{self as masks, AtlasConfig};
    let glyphs:Vec<_>=GLYPHS.iter().map(|&ch|ch as char).collect();
    masks::stamp_preview(frame,AtlasConfig { glyph_scale:SCALE,line_width_px:width_px },
        &glyphs,COLUMNS,SCALE,[48,100,216,255],[255;4]).map_err(|reason|(reason,false))
}
