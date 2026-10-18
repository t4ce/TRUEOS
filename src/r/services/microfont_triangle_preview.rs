//! Selected-character inspector: 4× source stamp above Dista Meso and Lean in one column.
use super::microfont_raster_preview::{PaintResult, append_quad, paint_triangles};
use crate::ui4::{self, FrameHandle, Ui4InputEvent, WindowOwner};
use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Instant, Timer};

const OWNER: WindowOwner = WindowOwner::MICROFONT_TRIANGLE_PREVIEW;
const GLYPH_WIDTH: u32 = 96;
const GLYPH_HEIGHT: u32 = GLYPH_WIDTH * microfont::FHEIGHT as u32 / microfont::FWIDTH as u32;
const STAMP_SCALE: u32 = 4;
const HEADER_HEIGHT: u32 = microfont::FHEIGHT as u32 * STAMP_SCALE;
const WIDTH: u32 = GLYPH_WIDTH;
const HEIGHT: u32 = HEADER_HEIGHT + 2 * GLYPH_HEIGHT;
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
        // Input still drives updates; idle turns do not render another frame.
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
    // Exercise the source stamp and both Dista shapes on boot.
    // Selected-window text input replaces this initial glyph.
    let mut character = Some('A');
    let mut dirty = true;
    let mut opened = false;
    let mut pinned = false;
    let result = 'run: loop {
        if ui4::window_is_closed(OWNER, window) {
            break Ok(());
        }
        for event in ui4::take_owner_input_events(OWNER) {
            let Ui4InputEvent::Keyboard(event) = event else {
                continue;
            };
            if event.window != window {
                continue;
            }
            let key = event.event;
            use crate::r::keyboard::{
                KEYBOARD_OUTPUT_FLAG_PRESS, KEYBOARD_OUTPUT_KIND_KEY, KEYBOARD_OUTPUT_KIND_TEXT,
            };
            if key.kind != KEYBOARD_OUTPUT_KIND_TEXT
                && !(key.kind == KEYBOARD_OUTPUT_KIND_KEY
                    && key.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0)
            {
                continue;
            }
            if let Some(ch) = char::from_u32(key.codepoint).filter(|_| key.codepoint != 0) {
                if character != Some(ch) {
                    character = Some(ch);
                    dirty = true;
                }
            }
        }
        if dirty {
            // Both CPU reconstruction and synchronous GPU retirement run on a
            // blocking lane. UI4's selected-window queue supplies input only.
            loop {
                let response = Arc::clone(&reply);
                let job = Box::new(move || response.signal(paint_character(frame, character)));
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
                            "microfont-triangle-preview: opened window={} frame={} size={}x{} cadence=streaming buffering=triple updates=input-only input=selected-window escape=close\n",
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

fn paint_character(frame: FrameHandle, character: Option<char>) -> PaintResult {
    use microfont::{
        FontDetail,
        FontShape::{Lean, Meso},
        raster_tri::dista_triangles,
    };
    let started = Instant::now().as_micros();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut counts = [0usize; 2];
    if let Some(ch) = character {
        append_stamp(ch, &mut vertices, &mut indices);
        // Meso above Lean, for Basic Latin and Braille. Other glyphs get the stamp.
        for (row, shape) in [Meso, Lean].into_iter().enumerate() {
            let Some(mesh) = dista_triangles(ch, shape, FontDetail::Fine) else {
                continue;
            };
            counts[row] = mesh.indices.len() / 3;
            let y = HEADER_HEIGHT as f32 + row as f32 * GLYPH_HEIGHT as f32;
            let scale = GLYPH_WIDTH as f32 / microfont::FWIDTH as f32;
            let base = vertices.len() as u32;
            for [px, py] in mesh.vertices {
                vertices.push([
                    2.0 * (px * scale) / WIDTH as f32 - 1.0,
                    1.0 - 2.0 * (y + py * scale) / HEIGHT as f32,
                    0.5,
                ]);
            }
            for triangle in mesh.indices.chunks_exact(3) {
                indices.extend_from_slice(&[
                    base + triangle[0],
                    base + triangle[2],
                    base + triangle[1],
                ]);
            }
        }
    }
    let cpu_us = Instant::now().as_micros().saturating_sub(started);
    let result = paint_triangles(frame, &vertices, &indices)?;
    crate::log_important!(target: "microfont-triangle-preview";
        "microfont-triangle-preview: rendered frame={} codepoint={:?} size={}x{} glyph={}x{} detail=0 subdivisions=12 triangles={:?} cpu_us={} rgba8_us={} layout=dista-meso/dista-lean stamp=4x storage=none escape=close\n",
        frame.raw(), character.map(|ch| ch as u32), WIDTH, HEIGHT, GLYPH_WIDTH, GLYPH_HEIGHT, counts,
        cpu_us, Instant::now().as_micros().saturating_sub(started));
    Ok(result)
}

fn append_stamp(character: char, vertices: &mut Vec<[f32; 3]>, indices: &mut Vec<u32>) {
    let mut utf8 = [0u8; 4];
    let text = character.encode_utf8(&mut utf8);
    let (width, height) = microfont::measure_text(text);
    let mut pixels = vec![0u8; width * height];
    if microfont::stamp_text(&mut pixels, width, height, 0, 0, text, 1).is_err() {
        return;
    }
    let left = WIDTH.saturating_sub(width as u32 * STAMP_SCALE) as f32;
    for y in 0..height {
        for x in 0..width {
            if pixels[y * width + x] == 0 {
                continue;
            }
            let x = left + (x as u32 * STAMP_SCALE) as f32;
            let y = (y as u32 * STAMP_SCALE) as f32;
            let size = STAMP_SCALE as f32;
            append_quad(
                vertices,
                indices,
                WIDTH,
                HEIGHT,
                [[x, y], [x + size, y], [x + size, y + size], [x, y + size]],
            );
        }
    }
}
