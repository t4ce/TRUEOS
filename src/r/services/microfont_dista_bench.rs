//! Opt-in background experiment; pixel validation happens after timed bursts.
use super::microfont_dista_tasks::{self as tasks, Quality};
use super::microfont_dista_validate as validate;
use crate::{intel::render, ui4};
use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use microfont::FontShape;
use serde_json::{Value, json};

static RUNNING: AtomicBool = AtomicBool::new(false);
static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
enum OutputFormat { Rgba8, R8 }
impl OutputFormat {
    fn name(self) -> &'static str { match self { Self::Rgba8 => "rgba8", Self::R8 => "r8" } }
}

struct Running;
impl Drop for Running {
    fn drop(&mut self) { RUNNING.store(false, Ordering::Release); }
}

pub(crate) fn request(warm_samples: u8) -> Result<(), &'static str> {
    if !(1..=15).contains(&warm_samples) { return Err("samples must be 1..15"); }
    RUNNING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "already running")?;
    let guard = Running;
    let job = Box::new(move || {
        let _guard = guard;
        let run = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
        match run_matrix(run, warm_samples) {
            Ok(()) => record(json!({"kind":"done", "run":run, "cases":24, "validated_pairs":12})),
            Err(reason) => record(json!({"kind":"error", "run":run, "reason":reason})),
        }
    });
    crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "microfont-dista-bench")
        .map_err(|job| { drop(job); "background worker busy" })
}

fn record(value: Value) {
    crate::log_important!(target: "microfont-dista-bench";
        "microfont-dista-bench: {}\n", value);
}

fn stamp() -> u64 {
    unsafe {
        core::arch::x86_64::_mm_lfence();
        let ticks = core::arch::x86_64::_rdtsc();
        core::arch::x86_64::_mm_lfence();
        ticks
    }
}

fn nanos_since(start: u64, hz: u64) -> u64 {
    elapsed_ns(start, stamp(), hz)
}

fn elapsed_ns(start: u64, end: u64, hz: u64) -> u64 {
    ((u128::from(end.wrapping_sub(start)) * 1_000_000_000) / u128::from(hz)) as u64
}

fn run_matrix(run: u64, samples: u8) -> Result<(), &'static str> {
    if !crate::intel::shader::microfont_dista_seeds_current() {
        return Err("seed-or-shape-bake-stale");
    }
    let hz = crate::time::tsc_hz();
    if hz <= 1 { return Err("tsc-frequency-unavailable"); }
    let (device, sha) = crate::intel::shader::microfont_dista_artifact();
    record(json!({"kind":"begin", "run":run, "warm_samples":samples,
        "codepoints":"U+0020..U+007E", "slots":95, "nonblank_glyphs":94,
        "width":tasks::ATLAS_WIDTH, "height":tasks::ATLAS_HEIGHT,
        "glyph_width":tasks::GLYPH_WIDTH, "glyph_height":tasks::GLYPH_HEIGHT,
        "compiled_device":device, "source_sha256":sha, "tsc_hz":hz,
        "cpu_clock":"lfence-rdtsc-lfence", "legacy_cpu_timer_resolution_ns":1_000_000,
        "window":false, "pixel_readback_during_timing":false,
        "validation_readback":"after-timed-burst", "stage":"atlas-rebuild",
        "output_formats":["rgba8", "r8"], "render_target_cache_policy":"PAT3-UC",
        "ink_rgba":[255,255,255,255], "clear_rgba":[0,0,0,0],
        "gpu_scope":"instrumented draw and clear-through-RT-writeback; VS/raster overlap"}));
    for (quality_index, quality) in [Quality::Draft, Quality::Fine, Quality::High].into_iter().enumerate() {
        for (shape_index, shape) in [FontShape::Meso, FontShape::Lean].into_iter().enumerate() {
            // Alternate which variant goes first across presets to expose
            // order/frequency effects rather than always favoring the second.
            for direct in [quality_index % 2 != 0, quality_index % 2 == 0] {
                // A second matrix reverses each configuration's format order.
                let rgba_first = (run as usize + quality_index + shape_index + usize::from(direct)) % 2 == 0;
                let formats = if rgba_first { [OutputFormat::Rgba8, OutputFormat::R8] }
                    else { [OutputFormat::R8, OutputFormat::Rgba8] };
                let first = run_case(run, quality, shape, direct, formats[0], samples, hz)?;
                let second = run_case(run, quality, shape, direct, formats[1], samples, hz)?;
                let (mask, rgba) = if rgba_first { (second, first) } else { (first, second) };
                let audit = validate::compare(&mask, &rgba);
                record(json!({"kind":"validation", "run":run,
                    "variant":if direct { "direct" } else { "loop" },
                    "quality":quality_name(quality), "shape":shape_name(shape),
                    "stage":"atlas-rebuild", "after_timed_bursts":true,
                    "checked_pixels":audit.checked_pixels, "mismatched_pixels":audit.mismatched_pixels,
                    "format_mismatches":audit.format_mismatches,
                    "nonbinary_pixels":audit.nonbinary_pixels, "nonblank_glyphs":audit.nonblank_glyphs,
                    "space_pixels":audit.space_pixels, "unused_pixels":audit.unused_pixels,
                    "rgba_nonbinary_pixels":audit.rgba_nonbinary_pixels,
                    "rgba_channel_mismatches":audit.rgba_channel_mismatches,
                    "rgba_nonblank_glyphs":audit.rgba_nonblank_glyphs,
                    "rgba_space_pixels":audit.rgba_space_pixels, "rgba_unused_pixels":audit.rgba_unused_pixels,
                    "nonzero_pixels":audit.nonzero_pixels,
                    "coverage_sum":audit.coverage_sum, "r8_hash":audit.r8_hash, "rgba_hash":audit.rgba_hash,
                    "passed":audit.passed()}));
                if !audit.passed() { return Err("r8-rgba-validation"); }
            }
        }
    }
    Ok(())
}

/// Timings stop at a retired target. Warm observations rebuild every glyph;
/// they reuse descriptor storage and destination, not an already-built atlas.
/// Pixel snapshots happen after the timed burst, solely for format validation.
fn run_case(run: u64, quality: Quality, shape: FontShape, direct: bool,
    format: OutputFormat, samples: u8, hz: u64) -> Result<validate::Snapshot, &'static str>
{
    // Diagnostic accounting and log storage are outside the first-use timer.
    let active: usize = ('!'..='~').map(|ch| {
        microfont::raster::basic_seed(ch).unwrap().pixels.count_ones() as usize * quality.segments(shape)
            + 2 * crate::intel::shader::microfont_dista_active_links(ch)
    }).sum();
    let mut rows = Vec::with_capacity(samples as usize + 1);
    let case_start = stamp();
    let start = stamp();
    let tasks = tasks::build_atlas(quality, shape, direct);
    let task_prepare_ns = nanos_since(start, hz);
    let candidates = tasks.indices.len() / 3;
    let descriptor_vertices = tasks.descriptors.len();
    let upload_bytes = tasks.descriptors.len() * 12 + tasks.indices.len() * 4;
    let start = stamp();
    let mesh = render::create_resident_triangle_mesh(&tasks.descriptors, &tasks.indices)?;
    let upload_ns = nanos_since(start, hz);
    drop(tasks);
    let start = stamp();
    let (frame, lease, destination, mut mask) = if matches!(format, OutputFormat::R8) {
        match render::create_resident_dista_mask(tasks::ATLAS_WIDTH, tasks::ATLAS_HEIGHT) {
            Ok(mask) => (None, None, None, Some(mask)),
            Err(reason) => { let _ = render::release_resident_triangle_mesh(&mesh); return Err(reason); }
        }
    } else {
      let output = match ui4::OutputId::from_slot(0) {
        Some(output) => output,
        None => { let _ = render::release_resident_triangle_mesh(&mesh); return Err("output-unavailable"); }
      };
      let frame = match ui4::create_frame(ui4::FrameSpec {
        output,
        content: ui4::FrameContent::BlueprintScene,
        cadence: ui4::FrameCadence::Immutable,
        buffering: ui4::FrameBuffering::Single,
        format: ui4::ScanoutFormat::Rgba8888Premultiplied,
        width: tasks::ATLAS_WIDTH, height: tasks::ATLAS_HEIGHT, base_color: None,
    }) {
        Ok(frame) => frame,
        Err(_) => { let _ = render::release_resident_triangle_mesh(&mesh); return Err("frame-create"); }
      };
      let lease = match ui4::acquire_frame_buffer(frame) {
        Ok(lease) => lease,
        Err(_) => {
            let _ = render::release_resident_triangle_mesh(&mesh);
            let _ = ui4::destroy_frame(frame);
            return Err("frame-acquire");
        }
      };
      let destination = match ui4::gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(_) => {
            let _ = render::release_resident_triangle_mesh(&mesh);
            let _ = ui4::cancel_frame_buffer(lease);
            let _ = ui4::destroy_frame(frame);
            return Err("frame-surface");
        }
      };
      (Some(frame), Some(lease), Some(destination), None)
    };
    let allocation_ns = nanos_since(start, hz);
    let (destination_bytes, destination_pitch) = if let Some(mask) = &mask {
        (mask.surface().bytes, mask.surface().pitch_bytes)
    } else { let surface = destination.unwrap(); (surface.bytes, surface.pitch_bytes) };
    let mut safe_to_release = true;
    let result = (|| {
        let draws = [render::ResidentSceneDraw {
            mesh: &mesh, depth_flags: None, rgba: [255; 4], sampled_texture: None,
            fragment_contract: render::ResidentSceneFragmentContract::MicrofontDista,
            viewport_translation_px: [0.0, 0.0],
            topology: render::ResidentScenePrimitiveTopology::TriangleList, point_width_px: 0,
        }];
        for sample in 0..=samples {
            let start = stamp();
            let rendered = if let Some(mask) = &mut mask {
                render::render_resident_dista_mask(&mesh, mask)
            } else {
                render::render_resident_indexed_scene_frame_premultiplied_direct_to_surface(
                    &draws, Some([0; 4]), destination.unwrap(), false,
                )
            };
            let stopped = stamp();
            let wall_ns = elapsed_ns(start, stopped, hz);
            let first_ready_ns = elapsed_ns(case_start, stopped, hz);
            let rendered = match rendered {
                Ok(rendered) => rendered,
                Err(reason @ ("render-busy" | "in-flight" | "microfont-dista-device")) => return Err(reason),
                Err(reason) => { safe_to_release = false; return Err(reason); }
            };
            if let Some(reason) = rendered.completion_error() {
                safe_to_release = false;
                return Err(reason);
            }
            if rendered.release_fence.is_none() {
                safe_to_release = false;
                return Err("target-release-missing");
            }
            let ready = rendered.gpu_target_ready_timestamp;
            let clear_setup = rendered.gpu_clear_setup_timestamp;
            let writeback = rendered.gpu_writeback_timestamp;
            let gpu_timing_available = ready.is_some() && rendered.gpu_timestamp.is_some()
                && clear_setup.is_some() && writeback.is_some();
            rows.push(json!({"kind":"sample", "run":run,
                "variant":if direct { "direct" } else { "loop" },
                "quality":quality_name(quality), "shape":shape_name(shape), "sample":sample,
                "stage":"atlas-rebuild", "output_format":format.name(),
                "destination_bytes":destination_bytes, "destination_pitch_bytes":destination_pitch,
                "wall_ns":wall_ns,
                "first_target_ready_ns":if sample == 0 { Some(first_ready_ns) } else { None },
                "gpu_draw_ns":rendered.gpu_timestamp.map(|t|t.ns),
                "gpu_target_ready_ns":ready.map(|t|t.ns),
                "gpu_clear_setup_ns":clear_setup.map(|t|t.ns),
                "gpu_writeback_ns":writeback.map(|t|t.ns),
                "gpu_ticks":ready.map(|t|t.ticks), "gpu_hz":ready.map(|t|t.hz),
                "gpu_timing_available":gpu_timing_available,
                "cpu_prepare_us":rendered.geometry_prepare_us,
                "gpu_wait_us":rendered.gpu_poll_us,
                "submitted_triangles":candidates, "active_union_triangles":active,
                "present_copy":rendered.present_copy_performed}));
            if !gpu_timing_available {
                return Err("gpu-timestamps-unavailable");
            }
        }
        Ok(())
    })();
    let snapshot = if result.is_ok() {
        // Both renderer completion and its final release fence were observed.
        // No readback, comparison or cache management enters a timed sample.
        unsafe {
            if let Some(mask) = &mask { validate::capture_r8_retired(mask.surface()) }
            else { validate::capture_rgba_retired(destination.unwrap()) }
        }
    } else { Err("sample-incomplete") };
    let cleanup = if safe_to_release {
        let mesh_released = render::release_resident_triangle_mesh(&mesh);
        let lease_cancelled = lease.map(|lease| ui4::cancel_frame_buffer(lease).is_ok());
        let frame_destroyed = frame.map(|frame| ui4::destroy_frame(frame).is_ok());
        let mask_released = mask.take().map(render::release_resident_dista_mask);
        Some((mesh_released, lease_cancelled, frame_destroyed, mask_released))
    } else {
        // Without proof of retirement, keep both resident storage and the
        // destination lease alive. An uncertain writer must not race reuse.
        core::mem::forget(mesh);
        core::mem::forget(mask.take());
        None
    };
    record(json!({"kind":"setup", "run":run, "variant":if direct { "direct" } else { "loop" },
        "quality":quality_name(quality), "shape":shape_name(shape),
        "stage":"atlas-rebuild", "output_format":format.name(),
        "render_target_cache_policy":"PAT3-UC",
        "task_prepare_ns":task_prepare_ns, "mesh_upload_ns":upload_ns,
        "allocation_ns":allocation_ns, "descriptor_vertices":descriptor_vertices,
        "upload_bytes":upload_bytes, "destination_bytes":destination_bytes,
        "destination_pitch_bytes":destination_pitch,
        "retained_on_uncertain_completion":!safe_to_release,
        "cleanup_mesh_released":cleanup.map(|c|c.0),
        "cleanup_lease_cancelled":cleanup.and_then(|c|c.1),
        "cleanup_frame_destroyed":cleanup.and_then(|c|c.2),
        "cleanup_mask_released":cleanup.and_then(|c|c.3)}));
    // Emit after the measured burst; serial/network logging must not dominate
    // the spacing between warm samples or their TSC intervals.
    for row in rows { record(row); }
    result?;
    if cleanup.is_some_and(|c| !c.0 || c.1 == Some(false) || c.2 == Some(false) || c.3 == Some(false)) {
        return Err("cleanup-incomplete");
    }
    snapshot
}

fn quality_name(quality: Quality) -> &'static str {
    match quality { Quality::Draft => "Draft", Quality::Fine => "Fine", Quality::High => "High" }
}
fn shape_name(shape: FontShape) -> &'static str {
    match shape { FontShape::Meso => "Meso", FontShape::Lean => "Lean" }
}
