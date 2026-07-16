//! Compact TCP scene store for renderer-independent 3D geometry placement.
//!
//! The wire/state implementation lives in `trueos-draw3d`; this module only adapts it to
//! TRUEOS's native network queues and publishes the live scene to future renderers.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
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

const PROJECTED_COVERAGE_WARN_SCREEN_EQUIVALENTS: f32 = 1.75;

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
    let elapsed_seconds = now_ns.saturating_sub(epoch_ns) as f32 / 1_000_000_000.0;
    let (target_width, target_height) = crate::intel::render::resident_scene_target_dimensions();
    let aspect = target_width as f32 / target_height as f32;
    let (projected, clear_rgba) = with_scene(|scene| {
        let angle = scene
            .camera_orbit()
            .map_or(0.0, |orbit| orbit.angular_speed * elapsed_seconds);
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
            })
            .collect::<Vec<_>>();
        crate::intel::render::capture_resident_triangle_scene_frame(&draws, clear_rgba, false)
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
