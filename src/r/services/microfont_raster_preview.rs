//! One boot-time frame: Box Drawing, Block Elements and Braille at their original scale.
use alloc::{boxed::Box, sync::Arc};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

use crate::ui4::{self, FrameHandle, WindowOwner};

const OWNER: WindowOwner = WindowOwner::MICROFONT_RASTER_PREVIEW;
const SCALE: u32 = 4;
const WIDTH: u32 = (16 * 2 - 1) * microfont::FWIDTH as u32 * SCALE;
const BOX_HEIGHT: u32 = 8 * microfont::FHEIGHT as u32 * SCALE;
const HEIGHT: u32 = BOX_HEIGHT + 18 * microfont::FHEIGHT as u32 * SCALE;
type PaintResult = Result<usize, &'static str>;
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
    // Retry only queue admission and explicit pre-submission render backpressure.
    let reply = Arc::new(Reply::new());
    let quads = loop {
        loop {
            let response = Arc::clone(&reply);
            let job = Box::new(move || response.signal(paint_once(frame)));
            match crate::r::blocking::try_spawn_blocking_job_with_purpose(
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
        match reply.wait().await {
            Ok(quads) => break quads,
            Err("render-busy" | "in-flight" | "render-storage-busy" | "microfont-atlas-busy") => {
                Timer::after(Duration::from_millis(20)).await;
            }
            Err(reason) => {
                let _ = ui4::finish_window_session(OWNER, session);
                ui4::retire_frame_when_released(frame);
                return Err(reason);
            }
        }
    };
    if ui4::publish_window_frame(OWNER, window, ui4::DamageRect::FULL).is_err() {
        let _ = ui4::finish_window_session(OWNER, session);
        ui4::retire_frame_when_released(frame);
        return Err("window-publish");
    }
    crate::log_important!(target: "microfont-raster-preview";
        "microfont-raster-preview: presented window={} frame={} size={}x{} scale={} glyphs=416 ink_pixels={} source=shared-r8-atlas rows=8+2+16 rounded-corners=blank renders=1 storage=none escape=close\n",
        window.raw(), frame.raw(), WIDTH, HEIGHT, SCALE, quads);
    while !ui4::window_is_closed(OWNER, window) {
        Timer::after(Duration::from_millis(50)).await;
    }
    let _ = ui4::finish_window_session(OWNER, session);
    ui4::retire_frame_when_released(frame);
    Ok(())
}

fn paint_once(frame: FrameHandle) -> PaintResult {
    use super::microfont_mask_service::{self as masks, AtlasConfig};
    // Preserve the existing spaced quad specimen; pixels come from the same
    // warmed atlas as the line specimen, sampled at its original 4x size.
    let columns = 31;
    let mut glyphs = alloc::vec!['\0';columns*26];
    for (index, &ch) in microfont::BOX_DRAW_CHARACTERS.iter().enumerate() {
        glyphs[(index / 16) * columns + (index % 16) * 2] = ch;
    }
    for (index, &ch) in microfont::BLOCK_CHARACTERS.iter().enumerate() {
        glyphs[(8 + index / 16) * columns + (index % 16) * 2] = ch;
    }
    for index in 0..256 {
        glyphs[(10 + index / 16) * columns + (index % 16) * 2] =
            char::from_u32(0x2800 + index as u32).unwrap();
    }
    masks::stamp_preview(
        frame,
        AtlasConfig::default(),
        &glyphs,
        columns,
        SCALE,
        [48, 100, 216, 255],
        [255; 4],
    )
}
