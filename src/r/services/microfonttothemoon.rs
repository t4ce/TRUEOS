//! Runtime CPU Microfont construction, GPU RGBA8 capture and one tar/LZ4 export.
use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use embassy_sync::signal::Signal;
use trueos_time::{Duration, Timer};

mod mesh;
mod source;
const RECIPE_JSON: &str = include_str!("../../../tools/microfonttothemoon/recipe.json");
const ARCHIVE_PATH: &str = "microfonttothemoon-8x.tar.lz4";
const ARCHIVE_LIMIT: usize = 128 * 1024 * 1024;
// Temporary diagnostic: run generation and encoding without filesystem writes.
const STORE_ARCHIVE: bool = false;
#[derive(Clone, Copy)]
struct Stamp {
    han: bool,
    part: usize,
}
impl Stamp {
    fn path(self) -> String {
        format!("microfonttothemoon{}-8x-{}.png", if self.han { "-han" } else { "" }, self.part + 1)
    }
}
type Reply = Signal<crate::wait::EmbassySpinRawMutex, Result<Vec<u8>, &'static str>>;

fn settings() -> Result<source::Settings, &'static str> {
    let v: serde_json::Value = serde_json::from_str(RECIPE_JSON).map_err(|_| "recipe-json")?;
    let number = |key: &str| {
        v[key]
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or("recipe-number")
    };
    // This CPU port implements the currently selected neutral membrane model.
    // Refuse unsupported settings rather than quietly rendering another body.
    if v["model"].as_str() != Some("membrane")
        || v["route"].as_str() != Some("field")
        || number("bias")? != 0.0
        || number("diagAssist")? != 0.0
        || number("diagSnap")? != 0.0
        || v["cross"].as_bool() != Some(false)
    {
        return Err("unsupported-runtime-recipe");
    }
    let detail = v["detail"].as_u64().ok_or("recipe-detail")? as usize;
    let reach = number("reach")?;
    let long_reach_value = number("long_reach_value")?;
    if ![6, 12, 24, 40].contains(&detail)
        || !(0.1..=6.0).contains(&reach)
        || !(reach..=6.0).contains(&long_reach_value)
    {
        return Err("recipe-range");
    }
    let mut long_reach = Vec::new();
    for value in v["long_reach"].as_array().ok_or("reach-list")? {
        let mut chars = value.as_str().ok_or("reach-character")?.chars();
        let character = chars.next().ok_or("reach-character")?;
        if chars.next().is_some() {
            return Err("reach-character");
        }
        long_reach.push(character);
    }
    Ok(source::Settings {
        mesh: mesh::Recipe {
            sigma: number("sigma")?,
            level: number("level")?,
            balance: number("balance")?,
            detail,
            prune: v["prune"].as_bool().ok_or("recipe-prune")?,
        },
        reach,
        long_reach_value,
        long_reach,
    })
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfonttothemoon_task() {
    let settings = match settings() {
        Ok(s) => Arc::new(s),
        Err(reason) => {
            crate::log_important!(target:"microfonttothemoon";"microfonttothemoon: failed reason={}\n",reason);
            return;
        }
    };
    crate::log_important!(target: "microfonttothemoon";
        "microfonttothemoon: runtime-recipe model=membrane reach={} long_reach_value={} long_reach={:?} sigma={} level={} balance={} detail={} output=tar.lz4\n",
        settings.reach, settings.long_reach_value, settings.long_reach,
        settings.mesh.sigma, settings.mesh.level, settings.mesh.balance, settings.mesh.detail);
    let mut images = Vec::new();
    for han in [false, true] {
        for part in 0..4 {
            let stamp = Stamp { han, part };
            let Some(png) = run_stamp(stamp, Arc::clone(&settings)).await else {
                return;
            };
            images.push((stamp.path(), png));
        }
    }
    // Pack/compress on the blocking lane too; only the one final write uses BSP.
    let reply = Arc::new(Reply::new());
    let images = Arc::new(images);
    loop {
        let response = Arc::clone(&reply);
        let inputs = Arc::clone(&images);
        let job = Box::new(move || {
            let entries: Vec<_> = inputs
                .iter()
                .map(|(name, bytes)| crate::r::tar::SourceEntry {
                    name,
                    bytes,
                    content_type_raw: Some(infer::ContentTypeId::PNG.raw()),
                })
                .collect();
            let result = crate::r::tar::pack(&entries, ARCHIVE_LIMIT)
                .map_err(|_| "tar-pack")
                .map(|tar| crate::r::lz4::compress_frame_cpu(&tar));
            response.signal(result);
        });
        match crate::r::blocking::try_spawn_blocking_job_with_purpose(
            job,
            "microfonttothemoon/archive",
        ) {
            Ok(()) => break,
            Err(job) => {
                drop(job);
                Timer::after(Duration::from_millis(250)).await;
            }
        }
    }
    let archive = match reply.wait().await {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log_important!(target:"microfonttothemoon";"microfonttothemoon: archive failed reason={}\n",reason);
            return;
        }
    };
    drop(images);
    if !STORE_ARCHIVE {
        crate::log_important!(target:"microfonttothemoon";
            "microfonttothemoon: complete storage=disabled path={} archive_bytes={} png_files=8\n",
            ARCHIVE_PATH, archive.len());
        return;
    }
    loop {
        if let Some(disk) = crate::ui4::writable_capture_root_handle() {
            match crate::r::fs::trueosfs::file_in_typed_async(
                disk,
                ARCHIVE_PATH,
                &archive,
                infer::ContentTypeId::LZ4,
            )
            .await
            {
                Ok(true) => {
                    crate::log_important!(target:"microfonttothemoon";
                        "microfonttothemoon: saved path=trueosfs:/{} disk_id={} archive_bytes={} png_files=8 render-replay=0\n",ARCHIVE_PATH,disk.id().raw(),archive.len());
                    return;
                }
                result => crate::log_important!(target:"microfonttothemoon";
                    "microfonttothemoon: archive save deferred path={} result={:?} retry_ms=1000\n",ARCHIVE_PATH,result),
            }
        }
        Timer::after(Duration::from_millis(1000)).await;
    }
}

async fn run_stamp(stamp: Stamp, settings: Arc<source::Settings>) -> Option<Vec<u8>> {
    crate::log_important!(target:"microfonttothemoon";
        "microfonttothemoon: start path={} part={}/4 scale=8 image={}x{} detail={} mesh=runtime-cpu source=microfont-crate\n",
        stamp.path(),stamp.part+1,source::WIDTH,source::HEIGHT,settings.mesh.detail);
    let reply = Arc::new(Reply::new());
    loop {
        loop {
            let response = Arc::clone(&reply);
            let settings = Arc::clone(&settings);
            let job = Box::new(move || {
                let result = render_stamp(stamp, &settings).and_then(|rgba| {
                    crate::graphics::encoder::png::encode_rgba8_png(
                        source::WIDTH,
                        source::HEIGHT,
                        &rgba,
                        source::WIDTH as usize * 4,
                    )
                    .map_err(|_| "png-encode-failed")
                });
                response.signal(result);
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
            Ok(png) => return Some(png),
            Err("render-storage-busy") => Timer::after(Duration::from_millis(250)).await,
            Err(reason) => {
                crate::log_important!(target:"microfonttothemoon";
                    "microfonttothemoon: failed path={} reason={} render-replay=0\n",stamp.path(),reason);
                return None;
            }
        }
    }
}

/// CPU construction from crate masks, GPU rasterization and completed readback.
/// No precomputed mesh bytes are embedded or loaded.
fn render_stamp(stamp: Stamp, settings: &source::Settings) -> Result<Vec<u8>, &'static str> {
    use crate::intel::render::{
        ResidentSceneDraw, ResidentSceneFragmentContract, ResidentScenePrimitiveTopology,
    };
    // Worker time through completed RGBA8 readback: includes mesh preparation
    // and upload; excludes queue admission, PNG/archive encoding and disk writes.
    let generation_started_ns = crate::chronos::monotonic_nanos();
    let built = source::build_part(stamp.han, stamp.part, settings)?;
    let cpu_mesh_us =
        crate::chronos::monotonic_nanos().saturating_sub(generation_started_ns) / 1_000;
    let glyph_count = built.glyph_count;
    let long_reach_count = built.long_reach_count;
    let vertices = built.vertices;
    let indices = built.indices;
    let vertex_count = vertices.len();
    let index_count = indices.len();
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
        &[draw], &[], Some([0, 0, 0, 255]), source::WIDTH, source::HEIGHT, false,
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
                "microfonttothemoon: rendered path={} part={}/4 detail={} field_spacing={} glyphs={} long_reach_glyphs={} vertices={} triangles={} rgba8_bytes={} cpu_mesh_us={} generation_us={} geometry_us={} frame_us={}\n",
                stamp.path(), stamp.part + 1, settings.mesh.detail, 1.0 / settings.mesh.detail as f32,
                glyph_count, long_reach_count,
                vertex_count, index_count / 3, rgba.len(), cpu_mesh_us,
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
