//! One boot-time Box Drawing quad table, retained in a service-owned UI4 frame.
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

use crate::ui4::{self, FrameHandle, WindowOwner};

const OWNER: WindowOwner = WindowOwner::BOXDRAW_RASTER_PREVIEW;
const SCALE: u32 = 4;
const WIDTH: u32 = (16 * 2 - 1) * microfont::FWIDTH as u32 * SCALE;
const HEIGHT: u32 = 8 * microfont::FHEIGHT as u32 * SCALE;
// The bool retains the destination on uncertain GPU completion.
type PaintResult = Result<usize, (&'static str, bool)>;
type Reply = Signal<crate::wait::EmbassySpinRawMutex, PaintResult>;

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn boxdraw_raster_preview_task() {
    if let Err(reason) = show_once().await {
        crate::log_important!(target: "boxdraw-raster-preview";
            "boxdraw-raster-preview: failed reason={}\n", reason);
    }
}

async fn show_once() -> Result<(), &'static str> {
    let output = ui4::OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("scanout-unavailable")?;
    let session = ui4::begin_window_session(OWNER).map_err(|_| "session-create")?;
    let frame = match ui4::create_frame(ui4::FrameSpec {
        output,
        content: ui4::FrameContent::BlueprintScene,
        cadence: ui4::FrameCadence::Immutable,
        buffering: ui4::FrameBuffering::Single,
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
            x: (screen_width.saturating_sub(WIDTH) / 2) as i32,
            y: (screen_height.saturating_sub(2 * microfont::FHEIGHT as u32 * SCALE) / 2)
                .saturating_sub(HEIGHT + 16) as i32,
            width: WIDTH,
            height: HEIGHT,
            z: 32,
            opacity: 255,
            visible: true,
        },
        interaction: ui4::WindowInteraction::MOVABLE_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = ui4::finish_window_session(OWNER, session);
            let _ = ui4::destroy_frame(frame);
            return Err("window-create");
        }
    };
    // Retry only queue admission and explicit pre-submission render backpressure.
    let reply = Arc::new(Reply::new());
    let quads = loop {
        loop {
            let response = Arc::clone(&reply);
            let job = Box::new(move || response.signal(paint_once(frame)));
            match crate::r::blocking::try_spawn_blocking_job_with_purpose(
                job,
                "boxdraw-raster-preview",
            ) {
                Ok(()) => break,
                Err(job) => {
                    drop(job);
                    Timer::after(Duration::from_millis(20)).await;
                }
            }
        }
        match reply.wait().await {
            Ok(quads) => break quads,
            Err(("render-busy" | "in-flight", false)) => {
                Timer::after(Duration::from_millis(20)).await;
            }
            Err((reason, pinned)) => {
                let _ = ui4::finish_window_session(OWNER, session);
                if !pinned {
                    ui4::retire_frame_when_released(frame);
                }
                return Err(reason);
            }
        }
    };
    if ui4::publish_window_frame(OWNER, window, ui4::DamageRect::FULL).is_err() {
        let _ = ui4::finish_window_session(OWNER, session);
        ui4::retire_frame_when_released(frame);
        return Err("window-publish");
    }
    crate::log_important!(target: "boxdraw-raster-preview";
        "boxdraw-raster-preview: presented window={} frame={} size={}x{} scale={} glyphs=124 quads={} rows=8 rounded-corners=blank renders=1 storage=none escape=close\n",
        window.raw(), frame.raw(), WIDTH, HEIGHT, SCALE, quads);
    while !ui4::window_is_closed(OWNER, window) {
        Timer::after(Duration::from_millis(50)).await;
    }
    let _ = ui4::finish_window_session(OWNER, session);
    ui4::retire_frame_when_released(frame);
    Ok(())
}

fn paint_once(frame: FrameHandle) -> PaintResult {
    use crate::intel::render::{
        self, ResidentSceneDraw, ResidentSceneFragmentContract, ResidentScenePrimitiveTopology,
    };
    let mut vertices = Vec::with_capacity(microfont::raster::BOX_DRAW_QUAD_COUNT * 4);
    let mut indices = Vec::with_capacity(microfont::raster::BOX_DRAW_QUAD_COUNT * 6);
    for (index, &(ch, _)) in microfont::BOX_DRAW_SYMBOLS.iter().enumerate() {
        let Some(quads) = microfont::raster::boxdraw_quad_slice(ch) else {
            continue;
        };
        let x = (index % 16) as f32 * 2.0 * microfont::FWIDTH as f32 * SCALE as f32;
        let y = (index / 16) as f32 * microfont::FHEIGHT as f32 * SCALE as f32;
        for quad in quads {
            let base = vertices.len() as u32;
            for [qx, qy] in quad.vertices {
                vertices.push([
                    2.0 * (x + qx * SCALE as f32) / WIDTH as f32 - 1.0,
                    1.0 - 2.0 * (y + qy * SCALE as f32) / HEIGHT as f32,
                    0.5,
                ]);
            }
            indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    let quads = vertices.len() / 4;
    let mesh = render::create_resident_triangle_mesh(&vertices, &indices)
        .map_err(|reason| (reason, false))?;
    let lease = match ui4::acquire_frame_buffer(frame) {
        Ok(lease) => lease,
        Err(_) => {
            let _ = render::release_resident_triangle_mesh(&mesh);
            return Err(("frame-acquire", false));
        }
    };
    let destination = match ui4::gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(_) => {
            let _ = render::release_resident_triangle_mesh(&mesh);
            let _ = ui4::cancel_frame_buffer(lease);
            return Err(("frame-surface", false));
        }
    };
    let draws = [ResidentSceneDraw {
        mesh: &mesh,
        depth_flags: None,
        rgba: [48, 100, 216, 255],
        sampled_texture: None,
        fragment_contract: ResidentSceneFragmentContract::ConstantRgba,
        viewport_translation_px: [0.0, 0.0],
        topology: ResidentScenePrimitiveTopology::TriangleList,
        point_width_px: 0,
    }];
    let result = render::render_resident_indexed_scene_frame_premultiplied_direct_to_surface(
        &draws,
        Some([255, 255, 255, 255]),
        destination,
        false,
    );
    let result = match result {
        Ok(result) => result,
        Err("render-busy") => {
            let _ = render::release_resident_triangle_mesh(&mesh);
            let _ = ui4::cancel_frame_buffer(lease);
            return Err(("render-busy", false));
        }
        Err(reason) => {
            // No proof of retirement: keep both the mesh and write lease alive.
            core::mem::forget(mesh);
            return Err((reason, true));
        }
    };
    if let Some(reason) = result.completion_error() {
        core::mem::forget(mesh);
        return Err((reason, true));
    }
    let Some(release) = result.release_fence else {
        core::mem::forget(mesh);
        return Err(("scanout-release", true));
    };
    let _ = render::release_resident_triangle_mesh(&mesh);
    if let Err(error) = ui4::publish_resident_scene_frame_buffer(lease, release) {
        crate::log_important!(target: "boxdraw-raster-preview";
            "boxdraw-raster-preview: frame publication rejected error={:?}\n", error);
        let _ = ui4::cancel_frame_buffer(lease);
        return Err(("frame-publish", false));
    }
    Ok(quads)
}
