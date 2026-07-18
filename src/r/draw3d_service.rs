//! Compact TCP scene store for renderer-independent 3D geometry placement.
//!
//! The wire/state implementation lives in `trueos-draw3d`; this module only adapts it to
//! TRUEOS's native network queues and publishes the live scene through one
//! fixed UI4 surface without exposing scene state to the compositor.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use trueos_draw3d::{
    Command, FrameDecoder, ImageFormat, ProjectedMesh, RenderImage, Response, ResponseError, Scene,
    SceneStats, encode_response, project_scene_with_camera,
};

use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};

pub const TCP_PORT: u16 = crate::allports::services::DRAW3D_TCP_PORT;
const OWNER: &str = "draw3d-service";
const EVENT_BATCH: usize = 64;
const COMMAND_QUEUE_DEPTH: usize = 512;
const EVENT_QUEUE_DEPTH: usize = 512;
const PLACEHOLDER_RENDER_JPEG: &[u8] = include_bytes!("../../logo.jpg");
const PLACEHOLDER_RENDER_WIDTH: u32 = 3_840;
const PLACEHOLDER_RENDER_HEIGHT: u32 = 2_160;

static SCENE: Mutex<Option<Scene>> = Mutex::new(None);
static LAST_SCREENSHOT_FRAME: Mutex<Option<Arc<CapturedSceneFrame>>> = Mutex::new(None);
static SCENE_REVISION: AtomicU64 = AtomicU64::new(1);
static SCENE_CAMERA_EPOCH_NS: AtomicU64 = AtomicU64::new(0);
static LISTENER_QUEUE_FULL_COUNT: AtomicU64 = AtomicU64::new(0);
static REPLY_QUEUE_FULL_COUNT: AtomicU64 = AtomicU64::new(0);
static UI4_GPU_DIRECT_FRAME_LOGGED: AtomicBool = AtomicBool::new(false);

const PROJECTED_COVERAGE_WARN_SCREEN_EQUIVALENTS: f32 = 1.75;
const UI4_FRAME_PERIOD_US: u64 = 16_667;
const UI4_OWNER: crate::ui4::WindowOwner = crate::ui4::WindowOwner::KernelApp(3);
const UI4_PLANE_SLOT: usize = crate::ui4::RGB_OVERLAY_PLANE_SLOT_3;
const _: () = assert!(UI4_PLANE_SLOT == 3);

struct CapturedSceneFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct SceneRenderJob {
    instance_id: u64,
    mesh_id: u64,
    color: trueos_draw3d::Rgba8,
    triangle_count: usize,
    projected_coverage: f32,
    pressure_warned: bool,
    resident: crate::intel::render::ResidentTriangleMesh,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Draw3dUi4Error {
    Frame(crate::ui4::FramePoolError),
    Window(crate::ui4::WindowBrokerError),
    Render(&'static str),
    InvalidFrame,
}

impl From<crate::ui4::FramePoolError> for Draw3dUi4Error {
    fn from(error: crate::ui4::FramePoolError) -> Self {
        Self::Frame(error)
    }
}

impl From<crate::ui4::WindowBrokerError> for Draw3dUi4Error {
    fn from(error: crate::ui4::WindowBrokerError) -> Self {
        Self::Window(error)
    }
}

struct Draw3dUi4Surface {
    session: crate::ui4::WindowSessionId,
    frame: crate::ui4::FrameHandle,
    window: Option<crate::ui4::WindowId>,
    width: u32,
    height: u32,
}

fn initialize_ui4_surface() -> Result<Draw3dUi4Surface, Draw3dUi4Error> {
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let (max_width, max_height) = crate::intel::render::resident_scene_target_dimensions();
    let (scanout_width, scanout_height) = crate::intel::active_scanout_dimensions()
        .unwrap_or((crate::ui4::DEFAULT_FRAME_WIDTH, crate::ui4::DEFAULT_FRAME_HEIGHT));
    let width = scanout_width.min(max_width as u32);
    let height = scanout_height.min(max_height as u32);
    let frame = crate::ui4::create_frame(crate::ui4::FrameSpec {
        output,
        content: crate::ui4::FrameContent::RenderScene3d,
        cadence: crate::ui4::FrameCadence::Streaming,
        format: crate::ui4::ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(crate::ui4::PremultipliedRgba8::TRANSPARENT),
    })?;
    let session = match crate::ui4::begin_window_session(UI4_OWNER) {
        Ok(session) => session,
        Err(error) => {
            let _ = crate::ui4::destroy_frame(frame);
            return Err(error.into());
        }
    };
    Ok(Draw3dUi4Surface {
        session,
        frame,
        window: None,
        width,
        height,
    })
}

fn publish_ui4_scene_frame(
    surface: &mut Draw3dUi4Surface,
    lease: crate::ui4::FrameWriteLease,
    release: crate::intel::render::ResidentSceneReleaseFence,
) -> Result<(), Draw3dUi4Error> {
    if let Err(error) = crate::ui4::publish_gpu_frame_buffer(lease, release) {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(error.into());
    }
    if surface.window.is_none() {
        let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
        let (scanout_width, scanout_height) =
            crate::intel::active_scanout_dimensions().unwrap_or((surface.width, surface.height));
        surface.window = Some(crate::ui4::create_window(crate::ui4::WindowCreate {
            owner: UI4_OWNER,
            session: surface.session,
            frame: surface.frame,
            output,
            plane: crate::ui4::WindowPlane::Universal(UI4_PLANE_SLOT as u8),
            placement: crate::ui4::WindowPlacement {
                x: (scanout_width.saturating_sub(surface.width) / 2) as i32,
                y: (scanout_height.saturating_sub(surface.height) / 2) as i32,
                width: surface.width,
                height: surface.height,
                z: 80,
                opacity: u8::MAX,
                visible: true,
            },
        })?);
    }
    crate::ui4::publish_window_frame(
        UI4_OWNER,
        surface.window.ok_or(Draw3dUi4Error::InvalidFrame)?,
        crate::ui4::DamageRect::FULL,
    )?;
    Ok(())
}

fn render_and_publish_ui4_scene_frame(
    surface: &mut Draw3dUi4Surface,
    draws: &[crate::intel::render::ResidentSceneDraw<'_>],
    clear_rgba: Option<[u8; 4]>,
    revision: u64,
) -> Result<crate::intel::render::ResidentSceneFrameResult, Draw3dUi4Error> {
    let lease = crate::ui4::acquire_frame_buffer(surface.frame)?;
    let destination = match crate::ui4::gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    if destination.width != surface.width
        || destination.height != surface.height
        || destination.pitch_bytes < surface.width.saturating_mul(4)
    {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(Draw3dUi4Error::InvalidFrame);
    }
    let result = match crate::intel::render::
        render_resident_triangle_scene_frame_premultiplied_with_opaque_depth_direct_to_surface(
            draws,
            clear_rgba,
            destination,
            false,
        ) {
        Ok(result) => result,
        Err(reason) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(Draw3dUi4Error::Render(reason));
        }
    };
    if result.completed_draws != result.requested_draws
        || result.rgba.is_some()
        || SCENE_REVISION.load(Ordering::Acquire) != revision
        || !with_scene(Scene::is_running)
    {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(Draw3dUi4Error::Render("incomplete-or-stale-gpu-frame"));
    }
    if result.present_copy_performed {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(Draw3dUi4Error::Render("direct-frame-copied"));
    }
    let Some(release) = result.release_fence else {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(Draw3dUi4Error::Render("missing-render-release-fence"));
    };
    publish_ui4_scene_frame(surface, lease, release)?;
    if !UI4_GPU_DIRECT_FRAME_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(
            target: "draw3d";
            "draw3d: live frame path=one-guc-scene-submit-to-ui4-triple-buffer target_gpu=0x{:X} size={}x{} pitch={} buffers=3 publish_after_guc_release=1 release_sequence={} render_release=rt-tile-flush+separate-qword-cookie render_target_cache_policy=ppgtt-pat3-uc+leaf-readback display_cache_policy=ggtt-pte-readback compositor_expected=direct-scanout-vblank-flip cpu_surface_sweep=0 cpu_readback=0 cpu_frame_copy=0\n",
            destination.gpu,
            destination.width,
            destination.height,
            destination.pitch_bytes,
            release.sequence(),
        );
    }
    Ok(result)
}

fn projected_draw_pressure(source: &ProjectedMesh) -> (usize, f32) {
    let mut coverage = 0.0f32;
    let mut triangles = 0usize;
    for triangle in source.indices.chunks_exact(3) {
        let Some(a) = source.vertices.get(triangle[0] as usize) else {
            continue;
        };
        let Some(b) = source.vertices.get(triangle[1] as usize) else {
            continue;
        };
        let Some(c) = source.vertices.get(triangle[2] as usize) else {
            continue;
        };
        let twice_area = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
        if !twice_area.is_finite() || twice_area <= 1.0e-9 {
            continue;
        }
        triangles += 1;
        // NDC spans four square units. Dividing triangle area by four
        // produces a screen-equivalent coverage estimate. Cap individual
        // triangles because X/Y clipping is GPU-owned and off-screen points
        // can otherwise dominate this diagnostic heuristic.
        coverage += (twice_area * 0.125).min(1.0);
    }
    (triangles, coverage)
}

fn warn_projected_draw_pressure(
    instance_id: u64,
    mesh_id: u64,
    triangle_count: usize,
    projected_coverage: f32,
) {
    crate::log_warn!(
        target: "draw3d";
        "draw3d: projected draw pressure instance={} mesh={} triangles={} screen_equivalents={:.2} guard=advisory potential_reason=high-screen-coverage-and-overdraw-in-one-gpu-submit action=split-mesh-into-smaller-resident-draws\n",
        instance_id,
        mesh_id,
        triangle_count,
        projected_coverage
    );
}

fn cache_screenshot_frame(result: &mut crate::intel::render::ResidentSceneFrameResult) {
    let Some(rgba) = result.rgba.take() else {
        return;
    };
    *LAST_SCREENSHOT_FRAME.lock() = Some(Arc::new(CapturedSceneFrame {
        width: result.width,
        height: result.height,
        rgba,
    }));
}

fn request_render_image() -> RenderImage {
    let request_started_ns = crate::chronos::monotonic_nanos();
    let captured = capture_screenshot_view();
    let capture_complete_ns = crate::chronos::monotonic_nanos();
    // A failed fresh capture must not discard the last complete screenshot.
    // Permanent reset explicitly clears both this cache and presentation
    // eligibility, so returning the cache here cannot cross scene lifetimes.
    let latest = LAST_SCREENSHOT_FRAME.lock().clone();
    if let Some(frame) = latest {
        let stride = usize::try_from(frame.width)
            .ok()
            .and_then(|width| width.checked_mul(4));
        if let Some(stride) = stride {
            match crate::graphics::encoder::png::encode_rgba8_png(
                frame.width,
                frame.height,
                frame.rgba.as_slice(),
                stride,
            ) {
                Ok(bytes) => {
                    crate::log_info!(
                        target: "draw3d";
                        "draw3d: screenshot profile capture_us={} encode_us={} total_us={} fresh={} format=png size={}x{} bytes={}\n",
                        capture_complete_ns.saturating_sub(request_started_ns) / 1_000,
                        crate::chronos::monotonic_nanos().saturating_sub(capture_complete_ns) / 1_000,
                        crate::chronos::monotonic_nanos().saturating_sub(request_started_ns) / 1_000,
                        captured as u8,
                        frame.width,
                        frame.height,
                        bytes.len(),
                    );
                    return RenderImage {
                        format: ImageFormat::Png,
                        width: frame.width,
                        height: frame.height,
                        bytes,
                    };
                }
                Err(error) => crate::log_warn!(
                    target: "draw3d";
                    "draw3d: cached frame PNG encode failed error={:?} rgba_bytes={}\n",
                    error,
                    frame.rgba.len()
                ),
            }
        }
    }

    RenderImage {
        format: ImageFormat::Jpeg,
        width: PLACEHOLDER_RENDER_WIDTH,
        height: PLACEHOLDER_RENDER_HEIGHT,
        bytes: PLACEHOLDER_RENDER_JPEG.to_vec(),
    }
}

/// Read the current shared scene without copying mesh buffers.
///
/// The closure must remain short: command application and future render readers share this lock.
pub fn with_scene<R>(read: impl FnOnce(&Scene) -> R) -> R {
    let mut scene = SCENE.lock();
    let scene = scene.get_or_insert_with(Scene::default);
    read(scene)
}

pub fn scene_stats() -> SceneStats {
    with_scene(Scene::stats)
}

fn apply_command(
    command: Command,
) -> Result<trueos_draw3d::ApplyOutcome, trueos_draw3d::ApplyError> {
    let permanent_stop = matches!(&command, Command::StopScene { permanent: true });
    let camera_changed = matches!(&command, Command::SetViewCamera { .. });
    let mut scene = SCENE.lock();
    let scene = scene.get_or_insert_with(Scene::default);
    let outcome = scene.apply(command)?;
    if permanent_stop {
        *LAST_SCREENSHOT_FRAME.lock() = None;
    }
    if outcome.affected != 0 {
        SCENE_REVISION.fetch_add(1, Ordering::AcqRel);
        if camera_changed {
            SCENE_CAMERA_EPOCH_NS.store(crate::chronos::monotonic_nanos(), Ordering::Release);
        }
    }
    Ok(outcome)
}

fn release_render_job(job: SceneRenderJob) {
    if !crate::intel::render::release_resident_triangle_mesh(&job.resident) {
        crate::log_warn!(
            target: "draw3d";
            "draw3d: resident release quarantined instance={} mesh={}\n",
            job.instance_id,
            job.mesh_id
        );
        core::mem::forget(job);
    }
}

fn sync_render_jobs(
    old_jobs: &mut Vec<SceneRenderJob>,
    projected: Vec<ProjectedMesh>,
    warn_pressure: bool,
) -> Result<(), &'static str> {
    let mut previous = core::mem::take(old_jobs);
    let mut next = Vec::with_capacity(projected.len());
    let mut first_error = None;

    for source in projected {
        let (triangle_count, projected_coverage) = projected_draw_pressure(&source);
        let old_position = previous
            .iter()
            .position(|job| job.instance_id == source.instance_id);
        let reusable = old_position.map(|position| previous.swap_remove(position));
        if let Some(mut job) = reusable {
            if crate::intel::render::update_resident_triangle_mesh(
                &job.resident,
                &source.vertices,
                &source.indices,
            )
            .is_ok()
            {
                job.mesh_id = source.mesh_id;
                job.color = source.color;
                job.triangle_count = triangle_count;
                job.projected_coverage = projected_coverage;
                if warn_pressure
                    && !job.pressure_warned
                    && projected_coverage >= PROJECTED_COVERAGE_WARN_SCREEN_EQUIVALENTS
                {
                    warn_projected_draw_pressure(
                        job.instance_id,
                        job.mesh_id,
                        triangle_count,
                        projected_coverage,
                    );
                    job.pressure_warned = true;
                }
                next.push(job);
                continue;
            }
            release_render_job(job);
        }

        match crate::intel::render::create_resident_triangle_mesh(&source.vertices, &source.indices)
        {
            Ok(resident) => {
                let pressure_warned = warn_pressure
                    && projected_coverage >= PROJECTED_COVERAGE_WARN_SCREEN_EQUIVALENTS;
                if pressure_warned {
                    warn_projected_draw_pressure(
                        source.instance_id,
                        source.mesh_id,
                        triangle_count,
                        projected_coverage,
                    );
                }
                next.push(SceneRenderJob {
                    instance_id: source.instance_id,
                    mesh_id: source.mesh_id,
                    color: source.color,
                    triangle_count,
                    projected_coverage,
                    pressure_warned,
                    resident,
                });
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    for stale in previous {
        release_render_job(stale);
    }
    *old_jobs = next;
    first_error.map_or(Ok(()), Err)
}

fn capture_screenshot_view() -> bool {
    // Screenshot rendering is off-screen and never changes scanout.
    let capture_revision = SCENE_REVISION.load(Ordering::Acquire);
    let now_ns = crate::chronos::monotonic_nanos();
    let epoch_ns = SCENE_CAMERA_EPOCH_NS.load(Ordering::Acquire);
    let elapsed_seconds = now_ns.saturating_sub(epoch_ns) as f64 / 1_000_000_000.0;
    let (target_width, target_height) = crate::intel::render::resident_scene_target_dimensions();
    let aspect = target_width as f32 / target_height as f32;
    let (projected, clear_rgba) = with_scene(|scene| {
        let angle = scene
            .camera_orbit()
            .map_or(0.0, |orbit| (f64::from(orbit.angular_speed) * elapsed_seconds) as f32);
        let camera = scene.camera_at(angle);
        let clear = scene
            .clear_color()
            .map(|color| [color.r, color.g, color.b, color.a]);
        (project_scene_with_camera(scene, aspect, camera), clear)
    });

    let mut jobs = Vec::new();
    if let Err(error) = sync_render_jobs(&mut jobs, projected, false) {
        for job in jobs {
            release_render_job(job);
        }
        crate::log_warn!(
            target: "draw3d";
            "draw3d: screenshot residency failed error={}\n",
            error
        );
        return false;
    }

    let capture = {
        let draws = jobs
            .iter()
            .map(|job| crate::intel::render::ResidentSceneDraw {
                mesh: &job.resident,
                rgba: [job.color.r, job.color.g, job.color.b, job.color.a],
                viewport_translation_px: [0.0, 0.0],
            })
            .collect::<Vec<_>>();
        crate::intel::render::capture_resident_triangle_scene_frame_with_opaque_depth_msaa4(
            &draws, clear_rgba, false,
        )
    };
    for job in jobs {
        release_render_job(job);
    }

    match capture {
        Ok(mut result)
            if result.completed_draws == result.requested_draws && result.rgba.is_some() =>
        {
            if SCENE_REVISION.load(Ordering::Acquire) != capture_revision {
                crate::log_warn!(
                    target: "draw3d";
                    "draw3d: screenshot capture discarded revision={} potential_reason=scene-mutated-during-capture\n",
                    capture_revision,
                );
                return false;
            }
            cache_screenshot_frame(&mut result);
            true
        }
        Ok(result) => {
            crate::log_warn!(
                target: "draw3d";
                "draw3d: screenshot capture incomplete draws={}/{}\n",
                result.completed_draws,
                result.requested_draws
            );
            false
        }
        Err(error) => {
            crate::log_warn!(
                target: "draw3d";
                "draw3d: screenshot capture failed error={}\n",
                error
            );
            false
        }
    }
}

/// AP1 UI carrier for the TCP-owned scene.
///
/// The UI4 allocation exists for the complete service lifetime, but geometry
/// submission remains strictly gated by the protocol's `StartScene` state.
/// Static scenes render once per revision; a moving orbit submits at the
/// 60 Hz ceiling into an acquired buffer that is published only after the
/// scene-level GuC fence retires.
#[embassy_executor::task]
pub async fn draw3d_ui4_render_task() {
    crate::intel::wait_hw_logo_sequence_done().await;
    let mut last_init_error = None;
    let mut surface = loop {
        match initialize_ui4_surface() {
            Ok(surface) => break surface,
            Err(error) => {
                if last_init_error != Some(error) {
                    crate::log_warn!(
                        target: "draw3d";
                        "draw3d: UI4 surface pending error={:?} action=retry\n",
                        error
                    );
                    last_init_error = Some(error);
                }
                Timer::after(EmbassyDuration::from_millis(250)).await;
            }
        }
    };
    let buffers = crate::ui4::frame_snapshot(surface.frame)
        .map(|snapshot| snapshot.buffer_count)
        .unwrap_or(0);
    crate::log_info!(
        target: "draw3d";
        "draw3d: UI4 rewire surface ready owner=kernel-app-3 session={} frame={} window=deferred-until-first-scene output=D01 plane_slot={} format=rgba8-premultiplied cadence=streaming buffers={} extent={}x{} ui4_consumers=draw3d-only frame_addresses=3 target_hz=60 producer=one-guc-scene-submit presentation=triple-buffer-direct-scanout per_frame_compositor_jobs=0 per_frame_display_flips=1 cpu_readback=0 cpu_frame_copy=0 scene_running=0\n",
        surface.session.raw(),
        surface.frame.raw(),
        UI4_PLANE_SLOT,
        buffers,
        surface.width,
        surface.height,
    );

    let mut jobs = Vec::new();
    let mut rendered_revision = 0u64;
    let mut was_running = false;
    let mut retry_frame = false;
    let mut last_sync_error = None;
    let mut last_render_error = None;
    let mut next_frame = Instant::now();
    let mut submitted_frames = 0u64;

    loop {
        next_frame += EmbassyDuration::from_micros(UI4_FRAME_PERIOD_US);
        let revision = SCENE_REVISION.load(Ordering::Acquire);
        let (running, orbit_moving) = with_scene(|scene| {
            (
                scene.is_running(),
                scene
                    .camera_orbit()
                    .is_some_and(|orbit| orbit.angular_speed != 0.0),
            )
        });
        if was_running && !running {
            rendered_revision = 0;
            retry_frame = false;
            crate::log_info!(
                target: "draw3d";
                "draw3d: UI4 scene stopped revision={} action=freeze-last-presented-frame guc_submits=0\n",
                revision
            );
        } else if !was_running && running {
            retry_frame = true;
            crate::log_info!(
                target: "draw3d";
                "draw3d: UI4 scene started revision={} action=render-publish-flip-triple-buffer-slot3 target_hz=60\n",
                revision
            );
        }

        if running && (revision != rendered_revision || orbit_moving || retry_frame) {
            let now_ns = crate::chronos::monotonic_nanos();
            let epoch_ns = SCENE_CAMERA_EPOCH_NS.load(Ordering::Acquire);
            let elapsed_seconds = now_ns.saturating_sub(epoch_ns) as f64 / 1_000_000_000.0;
            if surface.width == 0 || surface.height == 0 {
                Timer::at(next_frame).await;
                continue;
            }
            let aspect = surface.width as f32 / surface.height as f32;
            let (projected, clear_rgba) = with_scene(|scene| {
                let angle = scene
                    .camera_orbit()
                    .map_or(0.0, |orbit| (f64::from(orbit.angular_speed) * elapsed_seconds) as f32);
                let camera = scene.camera_at(angle);
                let clear = scene
                    .clear_color()
                    .map(|color| [color.r, color.g, color.b, color.a]);
                (project_scene_with_camera(scene, aspect, camera), clear)
            });
            match sync_render_jobs(&mut jobs, projected, true) {
                Ok(()) => {
                    last_sync_error = None;
                    let draws = jobs
                        .iter()
                        .map(|job| crate::intel::render::ResidentSceneDraw {
                            mesh: &job.resident,
                            rgba: [job.color.r, job.color.g, job.color.b, job.color.a],
                            viewport_translation_px: [0.0, 0.0],
                        })
                        .collect::<Vec<_>>();
                    match render_and_publish_ui4_scene_frame(
                        &mut surface,
                        &draws,
                        clear_rgba,
                        revision,
                    ) {
                        Ok(result) => {
                            rendered_revision = revision;
                            retry_frame = false;
                            last_render_error = None;
                            submitted_frames = submitted_frames.saturating_add(1);
                            if submitted_frames <= 8 || submitted_frames.is_multiple_of(120) {
                                crate::log_trace!(
                                    target: "draw3d";
                                    "draw3d: UI4 triple frame updated seq={} revision={} draws={} changed_pixels={} render={}x{} frame_us={} geometry_us={} resolve_us={} present_copy_us={} guc_scene_submits=1 ui4_publish=1 ui4_compositor_jobs=0 ui4_display_flip=1 cpu_readback=0 cpu_frame_copy=0 plane_slot={}\n",
                                    submitted_frames,
                                    revision,
                                    result.completed_draws,
                                    result.changed_pixels,
                                    result.width,
                                    result.height,
                                    result.frame_us,
                                    result.geometry_us,
                                    result.resolve_us,
                                    result.present_copy_us,
                                    UI4_PLANE_SLOT,
                                );
                            }
                        }
                        Err(error) => {
                            retry_frame = true;
                            if last_render_error != Some(error) {
                                crate::log_warn!(
                                    target: "draw3d";
                                    "draw3d: UI4 GPU-direct render pending revision={} jobs={} error={:?} action=retain-front-and-retry\n",
                                    revision,
                                    jobs.len(),
                                    error,
                                );
                                last_render_error = Some(error);
                            }
                        }
                    }
                }
                Err(reason) => {
                    retry_frame = true;
                    if last_sync_error != Some(reason) {
                        crate::log_warn!(
                            target: "draw3d";
                            "draw3d: UI4 residency pending revision={} jobs={} reason={} action=retry\n",
                            revision,
                            jobs.len(),
                            reason
                        );
                        last_sync_error = Some(reason);
                    }
                }
            }
        }
        was_running = running;

        if next_frame <= Instant::now() {
            next_frame = Instant::now();
        }
        Timer::at(next_frame).await;
    }
}

fn request_listener(commands: &NetQueue<NetCommand>) -> bool {
    if commands
        .push(NetCommand::OpenTcpListen { port: TCP_PORT })
        .is_err()
    {
        let occurrences = LISTENER_QUEUE_FULL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrences == 1 {
            crate::log_warn!(
                target: "draw3d";
                "draw3d: listener queue full port={} action=retry\n",
                TCP_PORT
            );
        } else if occurrences >= 64 && occurrences.is_power_of_two() {
            crate::log_trace!(
                target: "draw3d";
                "draw3d: listener queue still full port={} occurrences={}\n",
                TCP_PORT,
                occurrences
            );
        }
        false
    } else {
        true
    }
}

fn send_reply(commands: &NetQueue<NetCommand>, handle: NetHandle, bytes: Vec<u8>) {
    let len = bytes.len();
    if commands
        .push(NetCommand::SendTcp {
            handle,
            data: bytes,
        })
        .is_err()
    {
        let occurrences = REPLY_QUEUE_FULL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if occurrences == 1 {
            crate::log_warn!(
                target: "draw3d";
                "draw3d: reply queue full handle={} bytes={} action=drop-reply\n",
                handle.0,
                len
            );
        } else if occurrences >= 64 && occurrences.is_power_of_two() {
            crate::log_trace!(
                target: "draw3d";
                "draw3d: reply queue still full occurrences={}\n",
                occurrences
            );
        }
    }
}

fn close_connection(commands: &NetQueue<NetCommand>, handle: NetHandle) {
    let _ = commands.push(NetCommand::Close { handle });
}

fn process_data(
    commands: &NetQueue<NetCommand>,
    decoders: &mut BTreeMap<u32, FrameDecoder>,
    handle: NetHandle,
    data: &[u8],
) {
    let decoder = decoders.entry(handle.0).or_default();
    if let Err(error) = decoder.push(data) {
        crate::log_warn!(
            target: "draw3d";
            "draw3d: protocol buffer rejected handle={} bytes={} error={:?}\n",
            handle.0,
            data.len(),
            error
        );
        decoders.remove(&handle.0);
        close_connection(commands, handle);
        return;
    }

    loop {
        let request = match decoder.next_request() {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(error) => {
                crate::log_warn!(
                    target: "draw3d";
                    "draw3d: invalid frame handle={} error={:?}\n",
                    handle.0,
                    error
                );
                decoders.remove(&handle.0);
                close_connection(commands, handle);
                return;
            }
        };

        let request_id = request.request_id;
        let opcode = request.opcode;
        let command = match request.command {
            Ok(command) => command,
            Err(error) => {
                crate::log_warn!(
                    target: "draw3d";
                    "draw3d: decode failed handle={} request={} opcode=0x{:02x} error={:?}\n",
                    handle.0,
                    request_id,
                    opcode as u8,
                    error
                );
                send_reply(
                    commands,
                    handle,
                    encode_response(
                        opcode,
                        request_id,
                        &Response::Error(ResponseError::Decode(error)),
                    ),
                );
                continue;
            }
        };

        let command_name = command.name();
        let response = match command {
            Command::Ping { nonce } => Response::Pong(nonce),
            Command::GetStats => Response::Stats(scene_stats()),
            Command::RequestRender => Response::RenderImage(request_render_image()),
            command => match apply_command(command) {
                Ok(outcome) => Response::Applied(outcome),
                Err(error) => Response::Error(ResponseError::Apply(error)),
            },
        };

        match &response {
            Response::Applied(outcome) => crate::log_trace!(
                target: "draw3d";
                "draw3d: command={} request={} handle={} affected={} scene=[meshes:{} instances:{} vertices:{} faces:{} bytes:{}]\n",
                command_name,
                request_id,
                handle.0,
                outcome.affected,
                outcome.stats.mesh_count,
                outcome.stats.instance_count,
                outcome.stats.vertex_count,
                outcome.stats.face_count,
                outcome.stats.mesh_bytes
            ),
            Response::Stats(stats) => crate::log_trace!(
                target: "draw3d";
                "draw3d: command=get_stats request={} handle={} scene=[meshes:{} instances:{} vertices:{} faces:{} bytes:{}]\n",
                request_id,
                handle.0,
                stats.mesh_count,
                stats.instance_count,
                stats.vertex_count,
                stats.face_count,
                stats.mesh_bytes
            ),
            Response::Pong(nonce) => crate::log_trace!(
                target: "draw3d";
                "draw3d: command=ping request={} handle={} nonce={}\n",
                request_id,
                handle.0,
                nonce
            ),
            Response::RenderImage(image) => {
                let placeholder = image.format == ImageFormat::Jpeg;
                crate::log_trace!(
                    target: "draw3d";
                    "draw3d: command=request_render request={} handle={} source={} format={:?} size={}x{} bytes={}\n",
                    request_id,
                    handle.0,
                    if placeholder { "placeholder" } else { "offscreen-capture" },
                    image.format,
                    image.width,
                    image.height,
                    image.bytes.len()
                );
            }
            // Command rejections are part of the protocol contract and are
            // returned to the caller; they are request trace, not a service
            // health transition.
            Response::Error(error) => crate::log_trace!(
                target: "draw3d";
                "draw3d: command rejected command={} request={} handle={} error={:?}\n",
                command_name,
                request_id,
                handle.0,
                error
            ),
        }

        send_reply(commands, handle, encode_response(opcode, request_id, &response));
    }
}

#[embassy_executor::task]
pub async fn draw3d_service_task() {
    let commands = NetQueue::new_leaked("draw3d-cmd", COMMAND_QUEUE_DEPTH);
    let events = NetQueue::new_leaked("draw3d-evt", EVENT_QUEUE_DEPTH);
    register_app_queues(OWNER, commands, events);
    let _ = with_scene(|scene| scene.stats());

    let mut listener_pending = request_listener(commands);
    crate::log_info!(
        target: "draw3d";
        "draw3d: service ready tcp_port={} protocol_version={} max_payload={} scene_running=0\n",
        TCP_PORT,
        trueos_draw3d::PROTOCOL_VERSION,
        trueos_draw3d::MAX_PAYLOAD_LEN
    );

    let mut listener: Option<NetHandle> = None;
    let mut decoders: BTreeMap<u32, FrameDecoder> = BTreeMap::new();
    let mut retry_ticks: u16 = 0;
    let mut last_network_error = None;
    let mut network_error_repeats = 0u64;

    loop {
        for event in events.drain(EVENT_BATCH) {
            match event {
                NetEvent::Opened { handle, kind } if kind == SocketKind::Tcp => {
                    listener_pending = false;
                    listener = Some(handle);
                    crate::log_info!(
                        target: "draw3d";
                        "draw3d: listening tcp_port={} handle={}\n",
                        TCP_PORT,
                        handle.0
                    );
                }
                NetEvent::TcpEstablished {
                    handle,
                    peer,
                    peer6,
                } => {
                    if listener == Some(handle) {
                        listener = None;
                        listener_pending = request_listener(commands);
                    }
                    decoders.entry(handle.0).or_default();
                    crate::log_trace!(
                        target: "draw3d";
                        "draw3d: client connected handle={} peer={:?} peer6={:?}\n",
                        handle.0,
                        peer,
                        peer6
                    );
                }
                NetEvent::TcpData { handle, data } => {
                    // Some peers can deliver data in the same poll as establishment.
                    if listener == Some(handle) {
                        listener = None;
                        listener_pending = request_listener(commands);
                    }
                    process_data(commands, &mut decoders, handle, &data);
                }
                NetEvent::Closed { handle } => {
                    let was_listener = listener == Some(handle);
                    if was_listener {
                        listener = None;
                    }
                    decoders.remove(&handle.0);
                    crate::log_trace!(
                        target: "draw3d";
                        "draw3d: connection closed handle={} listener={}\n",
                        handle.0,
                        was_listener as u8
                    );
                    if was_listener {
                        listener_pending = request_listener(commands);
                    }
                }
                NetEvent::Error { msg } => {
                    if listener.is_none() {
                        listener_pending = false;
                    }
                    if last_network_error == Some(msg) {
                        network_error_repeats = network_error_repeats.saturating_add(1);
                        if network_error_repeats >= 64 && network_error_repeats.is_power_of_two() {
                            crate::log_trace!(
                                target: "draw3d";
                                "draw3d: network error persists msg={} repeats={}\n",
                                msg,
                                network_error_repeats
                            );
                        }
                    } else {
                        last_network_error = Some(msg);
                        network_error_repeats = 1;
                        crate::log_warn!(
                            target: "draw3d";
                            "draw3d: network state error msg={} action=listener-retry\n",
                            msg
                        );
                    }
                }
                NetEvent::Opened { .. }
                | NetEvent::TcpSent { .. }
                | NetEvent::UdpPacket { .. }
                | NetEvent::UdpPacketV6 { .. }
                | NetEvent::IpPacket { .. }
                | NetEvent::IcmpReply { .. }
                | NetEvent::IcmpReplyV6 { .. } => {}
            }
        }

        retry_ticks = retry_ticks.wrapping_add(1);
        if retry_ticks >= 1_000 {
            retry_ticks = 0;
            if listener.is_none() && !listener_pending {
                listener_pending = request_listener(commands);
            }
        }

        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}
