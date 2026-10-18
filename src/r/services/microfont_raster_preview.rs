//! One boot-time Block Elements quad table, retained in a service-owned UI4 frame.
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

use crate::ui4::{self, FrameHandle, WindowOwner};

const OWNER: WindowOwner = WindowOwner::MICROFONT_RASTER_PREVIEW;
const SCALE: u32 = 4;
const WIDTH: u32 = (16 * 2 - 1) * microfont::FWIDTH as u32 * SCALE;
const HEIGHT: u32 = 2 * microfont::FHEIGHT as u32 * SCALE;
// The bool retains the destination on uncertain GPU completion.
type PaintResult = Result<usize, (&'static str, bool)>;
type Reply = Signal<crate::wait::EmbassySpinRawMutex, PaintResult>;

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfont_raster_preview_task() {
    if let Err(reason) = show_once().await {
        crate::log_important!(target: "microfont-raster-preview";
            "microfont-raster-preview: failed reason={}\n", reason);
    }
}

async fn show_once() -> Result<(), &'static str> {
    let output = ui4::OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("scanout-unavailable")?;
    let session = ui4::begin_window_session(OWNER).map_err(|_| "session-create")?;
    let frame = match ui4::create_frame(ui4::FrameSpec {
        output,
        content: ui4::FrameContent::FontScene2d,
        cadence: ui4::FrameCadence::Immutable,
        buffering: ui4::FrameBuffering::Single,
        format: ui4::ScanoutFormat::Rgba8888Premultiplied,
        width: WIDTH,
        height: HEIGHT,
        base_color: Some(ui4::PremultipliedRgba8::from_straight_rgba(255, 255, 255, 255)),
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
            y: (screen_height.saturating_sub(HEIGHT) / 2) as i32,
            width: WIDTH,
            height: HEIGHT,
            z: 31,
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
    // Synchronous GPU retirement polling belongs on a blocking worker.
    let reply = Arc::new(Reply::new());
    loop {
        let response = Arc::clone(&reply);
        let job = Box::new(move || response.signal(paint_once(frame)));
        match super::super::blocking::try_spawn_blocking_job_with_purpose(
            job,
            "microfont-raster-preview",
        ) {
            Ok(()) => break,
            Err(job) => {
                drop(job);
                Timer::after(Duration::from_millis(20)).await;
            }
        }
    }
    let quads = match reply.wait().await {
        Ok(quads) => quads,
        Err((reason, pinned)) => {
            let _ = ui4::finish_window_session(OWNER, session);
            if !pinned {
                ui4::retire_frame_when_released(frame);
            }
            return Err(reason);
        }
    };
    if ui4::publish_window_frame(OWNER, window, ui4::DamageRect::FULL).is_err() {
        let _ = ui4::finish_window_session(OWNER, session);
        ui4::retire_frame_when_released(frame);
        return Err("window-publish");
    }
    crate::log_important!(target: "microfont-raster-preview";
        "microfont-raster-preview: presented window={} frame={} size={}x{} scale={} glyphs=32 quads={} rows=2 shades=included renders=1 storage=none escape=close\n",
        window.raw(), frame.raw(), WIDTH, HEIGHT, SCALE, quads);
    while !ui4::window_is_closed(OWNER, window) {
        Timer::after(Duration::from_millis(50)).await;
    }
    let _ = ui4::finish_window_session(OWNER, session);
    ui4::retire_frame_when_released(frame);
    Ok(())
}

fn paint_once(frame: FrameHandle) -> PaintResult {
    use crate::intel::gpgpu::{self, GpgpuRect, GpgpuSolidRect, GpgpuSubmissionOutcome};
    let lease = ui4::acquire_frame_buffer(frame).map_err(|_| ("frame-acquire", false))?;
    let destination = match ui4::gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(_) => {
            let _ = ui4::cancel_frame_buffer(lease);
            return Err(("frame-surface", false));
        }
    };
    // UI4 initialized the white background before granting this lease. Keep
    // the batch non-overlapping so its rectangle walkers can run in parallel.
    let mut rects = Vec::with_capacity(128);
    for (index, &(ch, _)) in microfont::BLOCK_SYMBOLS.iter().enumerate() {
        let Some(quads) = microfont::raster::block_quad_slice(ch) else {
            continue;
        };
        let x = (index % 16) as u32 * 2 * microfont::FWIDTH as u32 * SCALE;
        let y = (index / 16) as u32 * microfont::FHEIGHT as u32 * SCALE;
        for quad in quads {
            let [left, top] = quad.top_left.map(u32::from);
            let [right, bottom] = quad.bottom_right.map(u32::from);
            rects.push(GpgpuSolidRect {
                rect: GpgpuRect::new(
                    (x + left * SCALE) as i32,
                    (y + top * SCALE) as i32,
                    (right - left) * SCALE,
                    (bottom - top) * SCALE,
                ),
                color_rgba: u32::from_le_bytes([48, 100, 216, 255]),
            });
        }
    }
    let result = gpgpu::fill_solid_rects_rgba8_scanout_result(destination, &rects);
    match result.outcome {
        GpgpuSubmissionOutcome::Complete => {}
        GpgpuSubmissionOutcome::Unavailable => {
            let _ = ui4::cancel_frame_buffer(lease);
            return Err(("rectangles-unavailable", false));
        }
        GpgpuSubmissionOutcome::SubmittedIncomplete => return Err(("rectangles-incomplete", true)),
    }
    let finalizer = gpgpu::release_rgba8_surface_for_scanout(destination);
    let Some(release) = finalizer.release else {
        if !finalizer.submitted {
            let _ = ui4::cancel_frame_buffer(lease);
        }
        return Err(("scanout-release", finalizer.submitted));
    };
    if let Err(error) = ui4::publish_gpu_font_frame_buffer(lease, release) {
        crate::log_important!(target: "microfont-raster-preview";
            "microfont-raster-preview: frame publication rejected error={:?}\n", error);
        let _ = ui4::cancel_frame_buffer(lease);
        return Err(("frame-publish", false));
    }
    Ok(rects.len())
}
