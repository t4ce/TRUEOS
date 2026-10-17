//! Produce four studio and four Han inspection PNGs once per boot.
//! No font warming, atlas publication, cache, or Shell3 changes.
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

#[derive(Clone, Copy)]
struct Stamp {
    mesh: &'static [u8],
    path: &'static str,
    width: u32,
    height: u32,
    scale: u32,
    tile_width: u32,
    tile_height: u32,
    detail: u32,
    glyphs: usize,
    part: u32,
    recipe: &'static str,
}
include!("../../intel/gpu_font/moon/config.rs");
mod han {
    use super::Stamp;
    include!("../../intel/gpu_font/moon/han/config.rs");
}
type Reply = Signal<crate::wait::EmbassySpinRawMutex, Result<Vec<u8>, &'static str>>;

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfonttothemoon_task() {
    for stamp in STAMPS.into_iter().chain(han::STAMPS) {
        run_stamp(stamp).await;
    }
}

async fn run_stamp(stamp: Stamp) {
    crate::log_important!(target: "microfonttothemoon";
        "microfonttothemoon: start part={}/4 glyphs={} scale={} cell={}x{} tile={}x{} image={}x{} detail={} destination={} recipe={}\n", stamp.part, stamp.glyphs, stamp.scale, 6 * stamp.scale, 11 * stamp.scale, stamp.tile_width, stamp.tile_height, stamp.width, stamp.height, stamp.detail, stamp.path, stamp.recipe);
    let reply = Arc::new(Reply::new());
    let png = loop {
        // Queue admission may wait; accepted GPU work is never replayed.
        loop {
            let worker_reply = Arc::clone(&reply);
            let job = Box::new(move || {
                let result = render_stamp(stamp).and_then(|rgba| {
                    crate::graphics::encoder::png::encode_rgba8_png(
                        stamp.width,
                        stamp.height,
                        &rgba,
                        stamp.width as usize * 4,
                    )
                    .map_err(|_| "png-encode-failed")
                });
                worker_reply.signal(result);
            });
            match crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "microfonttothemoon")
            {
                Ok(()) => break,
                Err(job) => {
                    drop(job);
                    Timer::after(Duration::from_millis(250)).await;
                }
            }
        }
        match reply.wait().await {
            Ok(png) => break png,
            Err("render-storage-busy") => {
                Timer::after(Duration::from_millis(250)).await;
                continue;
            }
            Err(reason) => {
                crate::log_important!(target: "microfonttothemoon";
                "microfonttothemoon: failed path={} reason={} render-replay=0\n", stamp.path, reason);
                return;
            }
        }
    };
    // Retain only the finished PNG while a writable disk becomes available.
    // The filesystem future belongs to the BSP, never the blocking GPU lane.
    loop {
        if let Some(disk) = crate::ui4::writable_capture_root_handle() {
            match crate::r::fs::trueosfs::file_in_typed_async(
                disk,
                stamp.path,
                &png,
                infer::ContentTypeId::PNG,
            )
            .await
            {
                Ok(true) => {
                    crate::log_important!(target: "microfonttothemoon";
                        "microfonttothemoon: saved path=trueosfs:/{} disk_id={} png_bytes={} image={}x{} render-replay=0\n",
                        stamp.path, disk.id().raw(), png.len(), stamp.width, stamp.height);
                    return;
                }
                result => {
                    crate::log_important!(target: "microfonttothemoon";
                        "microfonttothemoon: save deferred path={} result={:?} retry_ms=1000\n", stamp.path, result);
                }
            }
        }
        Timer::after(Duration::from_millis(1000)).await;
    }
}

/// The studio's pure tessellator generates this immutable geometry at the
/// configured detail. Only vertices/indices are embedded; no raster is cached.
/// The boot worker uploads it once and submits one grayscale triangle scene.
fn render_stamp(stamp: Stamp) -> Result<Vec<u8>, &'static str> {
    use crate::intel::render::{
        ResidentSceneDraw, ResidentSceneFragmentContract, ResidentScenePrimitiveTopology,
    };
    // Worker time through completed RGBA8 readback: includes mesh preparation
    // and upload, but excludes queue admission, PNG encoding and disk writes.
    let generation_started_ns = crate::chronos::monotonic_nanos();
    let data = stamp.mesh;
    fn word(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
        let data = bytes.get(offset..offset + 4).ok_or("mesh-truncated")?;
        Ok(u32::from_le_bytes(data.try_into().map_err(|_| "mesh-word")?))
    }
    if data.get(..8) != Some(b"MOONMSH1") {
        return Err("mesh-version");
    }
    let vertex_count = word(data, 8)? as usize;
    let index_count = word(data, 12)? as usize;
    let index_offset = 16 + vertex_count * 12;
    if data.len() != index_offset + index_count * 4 || !index_count.is_multiple_of(3) {
        return Err("mesh-length");
    }
    let mut vertices = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        let offset = 16 + i * 12;
        vertices.push([
            f32::from_bits(word(data, offset)?),
            f32::from_bits(word(data, offset + 4)?),
            f32::from_bits(word(data, offset + 8)?),
        ]);
    }
    let mut indices = Vec::with_capacity(index_count);
    for i in 0..index_count {
        indices.push(word(data, index_offset + i * 4)?);
    }
    let mesh = crate::intel::render::create_resident_triangle_mesh(&vertices, &indices)?;
    drop(vertices);
    drop(indices);
    let draw = ResidentSceneDraw {
        mesh: &mesh,
        depth_flags: None,
        rgba: [255; 4],
        sampled_texture: None,
        fragment_contract: ResidentSceneFragmentContract::ConstantRgba,
        viewport_translation_px: [0.0; 2],
        topology: ResidentScenePrimitiveTopology::TriangleList,
        point_width_px: 0,
    };
    let result = crate::intel::render::capture_resident_triangle_scene_frame_premultiplied_at_extent_msaa4_with_coverage(
        &[draw], &[], Some([0, 0, 0, 255]), stamp.width, stamp.height, false,
    );
    let rgba8_ready_ns = crate::chronos::monotonic_nanos();
    // The renderer owns incomplete GPU backing until exact retirement. Do not
    // release the source after an uncertain submission; never replay this job.
    match result {
        Ok(frame) if frame.frame_complete && frame.completed_draws == 1 => {
            let released = crate::intel::render::release_resident_triangle_mesh(&mesh);
            if !released {
                return Err("mesh-release-unproven");
            }
            let rgba = frame.rgba.ok_or("readback-missing")?;
            crate::log_important!(target: "microfonttothemoon";
                "microfonttothemoon: rendered path={} part={}/4 detail={} field_spacing={} vertices={} triangles={} rgba8_bytes={} generation_us={} geometry_us={} frame_us={}\n",
                stamp.path, stamp.part, stamp.detail, 1.0 / stamp.detail as f32,
                vertex_count, index_count / 3, rgba.len(),
                rgba8_ready_ns.saturating_sub(generation_started_ns) / 1_000,
                frame.geometry_us, frame.frame_us);
            Ok(rgba)
        }
        Err("render-storage-busy" | "render-busy") => {
            if !crate::intel::render::release_resident_triangle_mesh(&mesh) {
                return Err("mesh-release-unproven");
            }
            Err("render-storage-busy")
        }
        Ok(_) | Err(_) => {
            core::mem::forget(mesh);
            Err("render-incomplete-no-replay")
        }
    }
}
