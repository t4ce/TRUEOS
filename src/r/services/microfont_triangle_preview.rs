//! One stamped Dista specimen: GPU-generated Meso above Lean, with CPU fallback.
use super::microfont_raster_preview::{PaintResult, paint_triangles, paint_triangles_with_contract};
use crate::ui4::{self, FrameHandle, WindowOwner};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Instant, Timer};

const OWNER: WindowOwner = WindowOwner::MICROFONT_TRIANGLE_PREVIEW;
use super::microfont_dista_tasks as tasks;
use tasks::{GLYPH_WIDTH, GLYPH_HEIGHT, WIDTH, HEIGHT};
// Boundary-error presets: Draft ≤0.25px, Fine ≤0.125px, High ≤0.0625px.
// Geometry is generated once; changing this setting requires a new specimen.
const QUALITY: tasks::Quality = [tasks::Quality::Draft, tasks::Quality::Fine, tasks::Quality::High][1];
type Reply = Signal<crate::wait::EmbassySpinRawMutex, PaintResult>;

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfont_triangle_preview_task() {
    if let Err(reason) = inspect().await {
        crate::log_important!(target: "microfont-triangle-preview";
            "microfont-triangle-preview: failed reason={}\n", reason);
    }
}

async fn inspect() -> Result<(), &'static str> {
    let output = ui4::OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("scanout-unavailable")?;
    let session = ui4::begin_window_session(OWNER).map_err(|_| "session-create")?;
    let frame = match ui4::create_frame(ui4::FrameSpec {
        output,
        content: ui4::FrameContent::BlueprintScene,
        // UI4's resident-triangle direct scanout accepts Streaming/Triple.
        // Publish once and retain the surface until the window closes.
        cadence: ui4::FrameCadence::Streaming,
        buffering: ui4::FrameBuffering::Triple,
        format: ui4::ScanoutFormat::Rgba8888Premultiplied,
        width: WIDTH,
        height: HEIGHT,
        base_color: None,
    }) {
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
            x: (screen_width.saturating_sub(WIDTH) / 2)
                .saturating_add(400)
                .min(screen_width.saturating_sub(WIDTH)) as i32,
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
    let reply = Arc::new(Reply::new());
    let mut dirty = true;
    let mut opened = false;
    let mut pinned = false;
    let result = 'run: loop {
        if ui4::window_is_closed(OWNER, window) {
            break Ok(());
        }
        // Drain owner events without interpreting text as glyph selection.
        let _ = ui4::take_owner_input_events(OWNER);
        if dirty {
            // Upload/retirement and the explicit CPU fallback use a blocking lane.
            loop {
                let response = Arc::clone(&reply);
                let job = Box::new(move || response.signal(paint_specimen(frame)));
                match crate::r::blocking::try_spawn_blocking_job_with_purpose(
                    job,
                    "microfont-triangle-preview",
                ) {
                    Ok(()) => break,
                    Err(job) => {
                        drop(job);
                        if ui4::window_is_closed(OWNER, window) {
                            break 'run Ok(());
                        }
                        Timer::after(Duration::from_millis(20)).await;
                    }
                }
            }
            match reply.wait().await {
                Ok(_) => {
                    if ui4::window_is_closed(OWNER, window) {
                        break Ok(());
                    }
                    if ui4::publish_window_frame(OWNER, window, ui4::DamageRect::FULL).is_err() {
                        break Err("window-publish");
                    }
                    if !opened {
                        crate::log_important!(target: "microfont-triangle-preview";
                            "microfont-triangle-preview: opened window={} frame={} size={}x{} cadence=streaming buffering=triple text=ABC123/Yy% rows=meso/lean updates=stamp-once typing=off escape=close\n",
                            window.raw(), frame.raw(), WIDTH, HEIGHT);
                        opened = true;
                    }
                    dirty = false;
                }
                Err(("render-busy" | "in-flight" | "frame-busy", false)) => {}
                Err((reason, retained)) => {
                    pinned = retained;
                    break Err(reason);
                }
            }
        }
        Timer::after(Duration::from_millis(20)).await;
    };
    let _ = ui4::finish_window_session(OWNER, session);
    if !pinned {
        ui4::retire_frame_when_released(frame);
    }
    result
}

fn paint_specimen(frame: FrameHandle) -> PaintResult {
    // The native VS bakes seed tables. Detect edits made since its last bake
    // instead of displaying stale letter designs silently.
    let seeds_current = crate::intel::shader::microfont_dista_seeds_current();
    if seeds_current {
        let started = Instant::now().as_micros();
        let tasks = tasks::build(QUALITY);
        let prepare_us = Instant::now().as_micros().saturating_sub(started);
        let result = paint_triangles_with_contract(
            frame, &tasks.descriptors, &tasks.indices,
            crate::intel::render::ResidentSceneFragmentContract::MicrofontDista,
        );
        match result {
            Ok(triangles) => {
                let sites: usize = tasks::TEXT.chars().map(|ch|
                    microfont::raster::basic_seed(ch).unwrap().pixels.count_ones() as usize).sum();
                let links: usize = tasks::TEXT.chars().map(
                    crate::intel::shader::microfont_dista_active_links).sum();
                let active = [microfont::FontShape::Meso, microfont::FontShape::Lean].map(|shape|
                    sites * QUALITY.segments(shape) + links * 2);
                let (device, source_sha) = crate::intel::shader::microfont_dista_artifact();
                crate::log_important!(target: "microfont-triangle-preview";
                    "microfont-triangle-preview: stamped frame={} text={} size={}x{} glyph={}x{} quality={:?} error_micropixels={} segments={}/{} candidate_triangles={:?} active_union_triangles={:?} descriptor_vertices={} cpu_task_prepare_us={} total_us={} compiled_device=0x{:04x} source_sha256={} geometry=glsl-vertex envelope=opaque-union cpu_triangulation=0\n",
                    frame.raw(), tasks::TEXT, WIDTH, HEIGHT, GLYPH_WIDTH, GLYPH_HEIGHT, QUALITY,
                    QUALITY.error_micropixels(), QUALITY.segments(microfont::FontShape::Meso),
                    QUALITY.segments(microfont::FontShape::Lean), tasks.row_candidates,
                    active, tasks.descriptors.len(), prepare_us, Instant::now().as_micros().saturating_sub(started),
                    device, source_sha);
                return Ok(triangles);
            }
            // Device rejection occurs before submission; the helper releases
            // the descriptor mesh and cancels its destination lease.
            Err(("microfont-dista-device", false)) => {}
            other => return other,
        }
    }
    crate::log_important!(target: "microfont-triangle-preview";
        "microfont-triangle-preview: CPU fallback reason={}\n",
        if seeds_current { "unsupported-device" } else { "seed-or-shape-bake-stale" });
    paint_cpu_fallback(frame)
}

fn paint_cpu_fallback(frame: FrameHandle) -> PaintResult {
    let started = Instant::now().as_micros();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut counts = [0usize; 2];
    let scale = GLYPH_WIDTH as f32 / microfont::FWIDTH as f32;
    for (row, shape) in [microfont::FontShape::Meso, microfont::FontShape::Lean].into_iter().enumerate() {
        for (column, ch) in tasks::TEXT.chars().enumerate() {
            let Some(mesh) = microfont::raster_tri::dista_triangles(ch, shape, QUALITY.cpu_detail()) else { continue; };
            counts[row] += mesh.indices.len() / 3;
            let base = vertices.len() as u32;
            for [px, py] in mesh.vertices {
                vertices.push([
                    2.0 * (column as f32 * GLYPH_WIDTH as f32 + px * scale) / WIDTH as f32 - 1.0,
                    1.0 - 2.0 * (row as f32 * GLYPH_HEIGHT as f32 + py * scale) / HEIGHT as f32,
                    0.5,
                ]);
            }
            for triangle in mesh.indices.chunks_exact(3) {
                indices.extend_from_slice(&[base + triangle[0], base + triangle[2], base + triangle[1]]);
            }
        }
    }
    let cpu_us = Instant::now().as_micros().saturating_sub(started);
    let result = paint_triangles(frame, &vertices, &indices)?;
    crate::log_important!(target: "microfont-triangle-preview";
        "microfont-triangle-preview: stamped frame={} text={} geometry=cpu-fallback triangles={:?} cpu_us={} total_us={} typing=off\n",
        frame.raw(), tasks::TEXT, counts, cpu_us, Instant::now().as_micros().saturating_sub(started));
    Ok(result)
}
