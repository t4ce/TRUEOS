//! Full Meso atlas: retained GPU coverage, one CPU copy for each selected view.
use super::{microfont_dista_copy, microfont_dista_tasks as tasks, microfont_dista_view::Config};
use super::microfont_raster_preview::PaintResult;
use crate::{intel::render, ui4::{self, FrameHandle, WindowOwner}};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration, Timer};

const OWNER: WindowOwner = WindowOwner::MICROFONT_TRIANGLE_PREVIEW;
const MAX_CACHED_MASKS: usize = 6;
type Reply = Signal<crate::wait::EmbassySpinRawMutex, PaintResult>;

struct CachedMask {
    key: (usize, u32),
    mask: render::ResidentDistaMask,
    triangles: usize,
}
#[derive(Default)]
struct AtlasCache { entries: Vec<CachedMask> }

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfont_triangle_preview_task() {
    if let Err(reason) = inspect().await {
        crate::log_important!(target: "microfont-triangle-preview";
            "microfont-triangle-preview: failed reason={} cpu-fallback=off\n", reason);
    }
}

fn create_frame(output: ui4::OutputId, config: Config) -> Result<FrameHandle, &'static str> {
    ui4::create_frame(ui4::FrameSpec {
        output, content: ui4::FrameContent::BlueprintScene,
        cadence: ui4::FrameCadence::Immutable, buffering: ui4::FrameBuffering::Single,
        format: ui4::ScanoutFormat::Rgba8888Premultiplied,
        width: config.width(), height: config.height(), base_color: None,
    }).map_err(|_| "frame-create")
}

async fn paint_when_admitted(
    frame: FrameHandle, config: Config, cache: &Arc<Mutex<AtlasCache>>,
    window: ui4::WindowId,
) -> PaintResult {
    let reply = Arc::new(Reply::new());
    loop {
        if ui4::window_is_closed(OWNER, window) { return Err(("window-closed", false)); }
        let response = Arc::clone(&reply);
        let cache = Arc::clone(cache);
        let job = Box::new(move || response.signal(paint_atlas(frame, config, &mut cache.lock())));
        match crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "microfont-triangle-preview") {
            Ok(()) => match reply.wait().await {
                Err(("render-busy" | "in-flight" | "render-storage-busy" | "frame-busy", false)) => {},
                result => return result,
            },
            Err(job) => drop(job),
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}

async fn inspect() -> Result<(), &'static str> {
    let output = ui4::OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) = crate::intel::active_scanout_dimensions().ok_or("scanout-unavailable")?;
    let session = ui4::begin_window_session(OWNER).map_err(|_| "session-create")?;
    let mut config = Config::default();
    let mut frame = match create_frame(output, config) {
        Ok(frame) => frame,
        Err(reason) => { let _ = ui4::finish_window_session(OWNER, session); return Err(reason); }
    };
    let window = match ui4::create_window(ui4::WindowCreate {
        owner: OWNER, session, frame, output,
        plane: ui4::WindowPlane::Universal(ui4::ALPHA_OVERLAY_PLANE_SLOT as u8),
        placement: ui4::WindowPlacement {
            x: (screen_width.saturating_sub(config.width()) / 2).min(32) as i32,
            y: (screen_height.saturating_sub(config.height()) / 2) as i32,
            width: config.width(), height: config.height(), z: 32, opacity: 255, visible: true,
        },
        interaction: ui4::WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = ui4::finish_window_session(OWNER, session);
            ui4::retire_frame_when_released(frame);
            return Err("window-create");
        }
    };
    let cache = Arc::new(Mutex::new(AtlasCache::default()));
    let mut shown = None;
    let result = loop {
        if ui4::window_is_closed(OWNER, window) { break Ok(()); }
        for input in ui4::take_owner_input_events(OWNER) {
            if let ui4::Ui4InputEvent::Keyboard(event) = input
                && event.window == window
                && let Some(character) = comparison_key(event.event)
            { config.apply_key(character); }
        }
        if shown != Some(config) {
            let replacement = shown.is_some();
            let next = if replacement {
                match create_frame(output, config) { Ok(next) => next, Err(reason) => break Err(reason) }
            } else { frame };
            let epoch = match ui4::window_resize_state(OWNER, window) {
                Ok(state) => state,
                Err(_) => {
                    if replacement { ui4::retire_frame_when_released(next); }
                    break Err("window-state");
                }
            };
            if let Err((reason, _)) = paint_when_admitted(next, config, &cache, window).await {
                if replacement { ui4::retire_frame_when_released(next); }
                if reason == "window-closed" { break Ok(()); }
                break Err(reason);
            }
            if ui4::window_is_closed(OWNER, window) {
                if replacement { ui4::retire_frame_when_released(next); }
                break Ok(());
            }
            let publication = if replacement {
                let placement = ui4::WindowPlacement {
                    width: config.width(), height: config.height(), ..epoch.0
                };
                ui4::commit_window_frame_replacement(OWNER, window, next, placement, epoch.1, ui4::DamageRect::FULL)
            } else { ui4::publish_window_frame(OWNER, window, ui4::DamageRect::FULL) };
            if publication.is_err() {
                if replacement { ui4::retire_frame_when_released(next); }
                break Err("window-publish");
            }
            if replacement { ui4::retire_frame_when_released(frame); frame = next; }
            shown = Some(config);
            crate::log_important!(target: "microfont-triangle-preview";
                "microfont-triangle-preview: opened window={} frame={} atlas=U+0020..U+007E shape=meso size={}x{} glyph={}x{} segments={} max_error_px={} aa={} cadence=immutable buffering=single updates=on-view-change controls=Q:quality,A:AA,1-5:size escape=close\n",
                window.raw(), frame.raw(), config.width(), config.height(),
                6 * config.scale(), 11 * config.scale(), config.segments(), config.max_error_px(),
                if config.aa { "2x2-box" } else { "off" });
        }
        Timer::after(Duration::from_millis(20)).await;
    };
    let _ = ui4::finish_window_session(OWNER, session);
    ui4::retire_frame_when_released(frame);
    // Every worker replied after producer retirement. Cached masks are immutable.
    drop(cache);
    result
}

fn comparison_key(event: crate::r::keyboard::TrueosKeyboardOutputEvent) -> Option<char> {
    use crate::r::keyboard::*;
    if event.kind != KEYBOARD_OUTPUT_KIND_TEXT || event.flags & KEYBOARD_OUTPUT_FLAG_PRESS == 0
        || event.flags & KEYBOARD_OUTPUT_FLAG_REPEAT != 0 || event.modifiers & 0xDD != 0
    { return None; }
    char::from_u32(event.codepoint)
}

fn stamp() -> u64 {
    unsafe {
        core::arch::x86_64::_mm_lfence();
        let tick = core::arch::x86_64::_rdtsc();
        core::arch::x86_64::_mm_lfence();
        tick
    }
}
fn elapsed_ns(start: u64) -> u64 {
    ((u128::from(stamp().wrapping_sub(start)) * 1_000_000_000)
        / u128::from(crate::time::tsc_hz().max(1))) as u64
}

fn paint_atlas(frame: FrameHandle, config: Config, cache: &mut AtlasCache) -> PaintResult {
    if !crate::intel::shader::microfont_dista_seeds_current() {
        return Err(("seed-or-shape-bake-stale", false));
    }
    let key = config.source_key();
    let cached = cache.entries.iter().position(|entry| entry.key == key);
    let mut gpu = None;
    let mut generate_ns = 0;
    if let Some(index) = cached {
        let entry = cache.entries.remove(index);
        cache.entries.push(entry);
    } else {
        let start = stamp();
        let upload = tasks::build_atlas_segments(config.segments(), microfont::FontShape::Meso, true);
        let triangles = upload.indices.len() / 3;
        let mesh = render::create_resident_triangle_mesh(&upload.descriptors, &upload.indices)
            .map_err(|reason| (reason, false))?;
        drop(upload);
        let mut mask = match render::create_resident_dista_mask(
            config.width() * config.sample_scale(), config.height() * config.sample_scale(),
        ) {
            Ok(mask) => mask,
            Err(reason) => { let _ = render::release_resident_triangle_mesh(&mesh); return Err((reason, false)); }
        };
        let result = match render::render_resident_dista_mask(&mesh, &mut mask) {
            Ok(result) => result,
            Err(reason) => {
                // Dedicated encoder errors precede submission; accepted
                // submissions without retirement return a completed=false result.
                let _ = render::release_resident_triangle_mesh(&mesh);
                let _ = render::release_resident_dista_mask(mask);
                return Err((reason, false));
            }
        };
        if let Some(reason) = result.completion_error() {
            core::mem::forget(mesh);
            // Mask Drop quarantines its alias and backing. UI4 is untouched.
            drop(mask);
            return Err((reason, false));
        }
        if result.release_fence.is_none() {
            core::mem::forget(mesh);
            core::mem::forget(mask);
            return Err(("mask-release-missing", false));
        }
        gpu = Some((result.gpu_timestamp, result.gpu_target_ready_timestamp));
        if !render::release_resident_triangle_mesh(&mesh) {
            drop(mask);
            return Err(("mesh-release", false));
        }
        generate_ns = elapsed_ns(start);
        if cache.entries.len() == MAX_CACHED_MASKS {
            let oldest = cache.entries.remove(0);
            if !render::release_resident_dista_mask(oldest.mask) {
                return Err(("mask-cache-release", false));
            }
        }
        cache.entries.push(CachedMask { key, mask, triangles });
    }
    let entry = cache.entries.last().ok_or(("mask-cache-empty", false))?;
    let lease = ui4::acquire_frame_buffer(frame).map_err(|_| ("frame-busy", false))?;
    let copied = (|| {
        let view = ui4::writable_rgba_view(lease).map_err(|_| "frame-view")?;
        let start = stamp();
        // Source has no further writers; destination lease owns every RGBA byte.
        let stats = unsafe { microfont_dista_copy::copy_retired_mask_to_rgba(
            entry.mask.surface(), view, config.sample_scale(),
        ) }?;
        let cpu_copy_ns = elapsed_ns(start);
        ui4::mark_frame_buffer_cpu_authored(lease).map_err(|_| "frame-cpu-authored")?;
        ui4::mark_frame_buffer_fully_opaque(lease).map_err(|_| "frame-opaque")?;
        ui4::publish_frame_buffer(lease).map_err(|_| "frame-publish")?;
        let (_, sha) = crate::intel::shader::microfont_dista_artifact();
        crate::log_important!(target: "microfont-triangle-preview";
            "microfont-triangle-preview: atlas-copy frame={} slots=95 nonblank=94 shape=meso glyph={}x{} segments={} aa={} source={}x{} source_bytes={} cache_hit={} gpu_generations={} cpu_copies=1 cpu_geometry=0 submitted_triangles={} generation_wall_ns={} gpu_timing={:?} cpu_copy_ns={} pixels={} samples={} partial_coverage_pixels={} source_sha256={}\n",
            frame.raw(), 6 * config.scale(), 11 * config.scale(), config.segments(), config.sample_scale(),
            entry.mask.surface().width, entry.mask.surface().height, entry.mask.surface().bytes,
            cached.is_some(), usize::from(cached.is_none()), entry.triangles, generate_ns, gpu,
            cpu_copy_ns, stats.output_pixels, stats.source_samples, stats.partial_coverage_pixels, sha);
        Ok(entry.triangles)
    })();
    copied.map_err(|reason| { let _ = ui4::cancel_frame_buffer(lease); (reason, false) })
}
