//! One boot-time frame: two native line primitives, no font tessellation.
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

use crate::ui4::{self, FrameHandle, WindowOwner};

const OWNER: WindowOwner = WindowOwner::MICROFONT_LINE_PREVIEW;
const SCALE: u32 = 16;
const WIDTH: u32 = 2 * microfont::FWIDTH as u32 * SCALE;
const HEIGHT: u32 = microfont::FHEIGHT as u32 * SCALE;
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
            y: (screen_height.saturating_sub(HEIGHT) / 2) as i32,
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
    let lines = loop {
        loop {
            let response = Arc::clone(&reply);
            let job = Box::new(move || response.signal(paint_once(frame)));
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
            Ok(lines) => break lines,
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
    crate::log_important!(target: "microfont-line-preview";
        "microfont-line-preview: presented window={} frame={} size={}x{} scale={} glyphs=2 lines={} glyph_size=96x176 topology=line-list width=default renders=1 storage=none escape=close\n",
        window.raw(), frame.raw(), WIDTH, HEIGHT, SCALE, lines);
    while !ui4::window_is_closed(OWNER, window) {
        Timer::after(Duration::from_millis(50)).await;
    }
    let _ = ui4::finish_window_session(OWNER, session);
    ui4::retire_frame_when_released(frame);
    Ok(())
}

fn paint_once(frame: FrameHandle) -> PaintResult {
    let mut vertices = Vec::with_capacity(4);
    for (index, ch) in ['|', '/'].into_iter().enumerate() {
        let points = microfont::raster_line::endpoints(ch).ok_or(("line-seed", false))?;
        for [x, y] in points {
            vertices.push([
                2.0 * ((index as f32 * 6.0 + x) * SCALE as f32) / WIDTH as f32 - 1.0,
                1.0 - 2.0 * (y * SCALE as f32) / HEIGHT as f32,
                0.5,
            ]);
        }
    }
    super::microfont_raster_preview::paint_primitives(
        frame, &vertices, &[0, 1, 2, 3],
        crate::intel::render::ResidentScenePrimitiveTopology::LineList,
    )
}
