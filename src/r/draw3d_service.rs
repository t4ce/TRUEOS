//! Compact TCP scene store for renderer-independent 3D geometry placement.
//!
//! The wire/state implementation lives in `trueos-draw3d`; this module only adapts it to
//! TRUEOS's native network queues and publishes the live scene to future renderers.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use trueos_draw3d::{
    CameraOrbit, Command, FrameDecoder, ImageFormat, ProjectedMesh, RenderImage, Response,
    ResponseError, Scene, SceneStats, encode_response, project_scene_at,
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
static LAST_PRESENTED_FRAME: Mutex<Option<Arc<PresentedSceneFrame>>> = Mutex::new(None);
static SCENE_REVISION: AtomicU64 = AtomicU64::new(1);
static SCENE_GEOMETRY_REVISION: AtomicU64 = AtomicU64::new(1);
static SCENE_CAMERA_REVISION: AtomicU64 = AtomicU64::new(1);
static SCENE_RUNNING: AtomicBool = AtomicBool::new(false);
static LISTENER_QUEUE_FULL_COUNT: AtomicU64 = AtomicU64::new(0);
static REPLY_QUEUE_FULL_COUNT: AtomicU64 = AtomicU64::new(0);

const FRAME_PERIOD_MS: u64 = 33;
const RENDER_STALL_WARN_ATTEMPTS: u32 = 30;
const RENDER_RETRY_TRACE_INTERVAL: u32 = 300;

struct PresentedSceneFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct SceneRenderJob {
    instance_id: u64,
    mesh_id: u64,
    color: trueos_draw3d::Rgba8,
    resident: crate::intel::render::ResidentTriangleMesh,
}

fn cache_presented_frame(result: &mut crate::intel::render::ResidentSceneFrameResult) {
    let Some(rgba) = result.rgba.take() else {
        return;
    };
    *LAST_PRESENTED_FRAME.lock() = Some(Arc::new(PresentedSceneFrame {
        width: result.width,
        height: result.height,
        rgba,
    }));
}

fn request_render_image() -> RenderImage {
    let latest = LAST_PRESENTED_FRAME.lock().clone();
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
    let camera_changed = matches!(&command, Command::SetViewCamera { .. });
    let geometry_changed = !matches!(
        &command,
        Command::SetColor { .. } | Command::StartScene { .. } | Command::StopScene
    );
    let mut scene = SCENE.lock();
    let scene = scene.get_or_insert_with(Scene::default);
    let outcome = scene.apply(command)?;
    SCENE_RUNNING.store(scene.is_running(), Ordering::Release);
    if outcome.affected != 0 {
        SCENE_REVISION.fetch_add(1, Ordering::AcqRel);
        if geometry_changed {
            SCENE_GEOMETRY_REVISION.fetch_add(1, Ordering::AcqRel);
        }
        if camera_changed {
            SCENE_CAMERA_REVISION.fetch_add(1, Ordering::AcqRel);
        }
    }
    Ok(outcome)
}

fn projected_scene(orbit_angle: f32) -> Vec<ProjectedMesh> {
    with_scene(|scene| project_scene_at(scene, 1.0, orbit_angle))
}

fn scene_camera_orbit() -> Option<CameraOrbit> {
    with_scene(Scene::camera_orbit)
}

fn scene_clear_rgba() -> Option<[u8; 4]> {
    with_scene(|scene| {
        scene
            .clear_color()
            .map(|color| [color.r, color.g, color.b, color.a])
    })
}

fn refresh_render_job_colors(jobs: &mut [SceneRenderJob]) {
    with_scene(|scene| {
        for job in jobs {
            if let Some(mesh) = scene.mesh(job.mesh_id) {
                job.color = mesh.color;
            }
        }
    });
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
) -> Result<(), &'static str> {
    let mut previous = core::mem::take(old_jobs);
    let mut next = Vec::with_capacity(projected.len());
    let mut first_error = None;

    for source in projected {
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
                next.push(job);
                continue;
            }
            release_render_job(job);
        }

        match crate::intel::render::create_resident_triangle_mesh(&source.vertices, &source.indices)
        {
            Ok(resident) => next.push(SceneRenderJob {
                instance_id: source.instance_id,
                mesh_id: source.mesh_id,
                color: source.color,
                resident,
            }),
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

#[embassy_executor::task]
pub async fn draw3d_render_task() {
    let mut jobs = Vec::new();
    let mut resident_revision = 0u64;
    let mut resident_geometry_revision = 0u64;
    let mut rendered_revision = 0u64;
    let mut next_frame = Instant::now();
    let mut camera_revision = 0u64;
    let mut camera_orbit = None;
    let mut orbit_epoch = Instant::now();
    let mut last_sync_error = None;
    let mut last_render_error = None;
    let mut render_retry_count = 0u32;
    let mut residency_ready = false;
    let mut was_running = false;
    let mut clear_rgba = None;

    let (target_width, target_height) = crate::intel::render::resident_scene_target_dimensions();
    crate::log_info!(
        target: "draw3d";
        "draw3d: render engine online running=0 control=tcp-start-stop target_fps=30 frame_ms={} max_meshes=100 max_instances=100 max_vertices_per_mesh=1000 target={}x{}\n",
        FRAME_PERIOD_MS,
        target_width,
        target_height
    );
    loop {
        next_frame += EmbassyDuration::from_millis(FRAME_PERIOD_MS);
        let running = SCENE_RUNNING.load(Ordering::Acquire);
        let revision = SCENE_REVISION.load(Ordering::Acquire);
        let geometry_revision = SCENE_GEOMETRY_REVISION.load(Ordering::Acquire);
        let next_camera_revision = SCENE_CAMERA_REVISION.load(Ordering::Acquire);
        let now = Instant::now();
        if next_camera_revision != camera_revision {
            camera_revision = next_camera_revision;
            camera_orbit = scene_camera_orbit();
            orbit_epoch = now;
        }
        let orbit_moving = camera_orbit.is_some_and(|orbit| orbit.angular_speed != 0.0);
        let orbit_angle = camera_orbit.map_or(0.0, |orbit| {
            let elapsed_seconds =
                now.saturating_duration_since(orbit_epoch).as_millis() as f32 / 1_000.0;
            orbit.angular_speed * elapsed_seconds
        });
        // Do not mutate resident vertex buffers while a prior submission is
        // being retried. Once that frame retires, wall-clock evaluation makes
        // the orbit catch up without accumulating timer drift.
        let animate_frame = running && orbit_moving && render_retry_count == 0;
        let mut background_changed = false;
        if revision != resident_revision || animate_frame {
            let next_clear_rgba = scene_clear_rgba();
            background_changed = next_clear_rgba != clear_rgba;
            clear_rgba = next_clear_rgba;
            let had_jobs = !jobs.is_empty();
            let sync_result = if geometry_revision != resident_geometry_revision || animate_frame {
                sync_render_jobs(&mut jobs, projected_scene(orbit_angle))
            } else {
                refresh_render_job_colors(&mut jobs);
                Ok(())
            };
            match sync_result {
                Ok(()) => {
                    resident_revision = revision;
                    resident_geometry_revision = geometry_revision;
                    residency_ready = true;
                    last_sync_error = None;
                    render_retry_count = 0;
                    crate::log_trace!(
                        target: "draw3d";
                        "draw3d: resident revision={} jobs={} geometry_revision={}\n",
                        revision,
                        jobs.len(),
                        geometry_revision
                    );
                    if running && had_jobs && jobs.is_empty() {
                        if let Ok(mut result) =
                            crate::intel::render::submit_resident_triangle_scene_frame(
                                &[],
                                clear_rgba,
                                false,
                            )
                        {
                            cache_presented_frame(&mut result);
                        }
                    }
                }
                Err(error) => {
                    residency_ready = false;
                    if last_sync_error != Some(error) {
                        crate::log_warn!(
                            target: "draw3d";
                            "draw3d: scene residency pending revision={} jobs={} error={}\n",
                            revision,
                            jobs.len(),
                            error
                        );
                        last_sync_error = Some(error);
                    }
                }
            }
        }

        if was_running && !running {
            let cleared =
                crate::intel::render::submit_resident_triangle_scene_frame(&[], None, false)
                    .map(|result| result.presented)
                    .unwrap_or(false);
            crate::log_info!(
                target: "draw3d";
                "draw3d: scene stopped revision={} resident_jobs={} overlay_cleared={}\n",
                resident_revision,
                jobs.len(),
                cleared as u8
            );
        } else if !was_running && running {
            if (clear_rgba.is_some() || jobs.is_empty())
                && let Ok(mut result) = crate::intel::render::submit_resident_triangle_scene_frame(
                    &[],
                    clear_rgba,
                    false,
                )
            {
                cache_presented_frame(&mut result);
            }
            crate::log_info!(
                target: "draw3d";
                "draw3d: scene started revision={} resident_jobs={} clear={:?}\n",
                resident_revision,
                jobs.len(),
                clear_rgba
            );
        } else if running && background_changed {
            if let Ok(mut result) =
                crate::intel::render::submit_resident_triangle_scene_frame(&[], clear_rgba, false)
            {
                cache_presented_frame(&mut result);
            }
        }

        // The overlay persists without resubmission.  Coalesce TCP updates at
        // the 30 Hz scene tick and draw only a new revision (or retry a frame
        // which did not fully retire).  This removes continuous GPU probe
        // traffic for static scenes while retaining a 30 FPS update ceiling.
        let retry_frame = render_retry_count != 0;
        if running
            && residency_ready
            && !jobs.is_empty()
            && (rendered_revision != resident_revision || animate_frame || retry_frame)
        {
            let draws = jobs
                .iter()
                .map(|job| crate::intel::render::ResidentSceneDraw {
                    mesh: &job.resident,
                    rgba: [job.color.r, job.color.g, job.color.b, job.color.a],
                })
                .collect::<Vec<_>>();
            let diagnostic_submit = render_retry_count == RENDER_STALL_WARN_ATTEMPTS;
            match crate::intel::render::submit_resident_triangle_scene_frame(
                &draws,
                clear_rgba,
                diagnostic_submit,
            ) {
                Ok(mut result) => {
                    let complete = result.completed_draws == result.requested_draws;
                    if complete && result.presented {
                        cache_presented_frame(&mut result);
                        rendered_revision = resident_revision;
                        last_render_error = None;
                        if render_retry_count == 0 {
                            crate::log_trace!(
                                target: "draw3d";
                                "draw3d: frame presented revision={} draws={} changed_pixels={}\n",
                                resident_revision,
                                result.completed_draws,
                                result.changed_pixels
                            );
                        } else {
                            crate::log_info!(
                                target: "draw3d";
                                "draw3d: frame recovered revision={} attempts={} draws={} changed_pixels={}\n",
                                resident_revision,
                                render_retry_count.saturating_add(1),
                                result.completed_draws,
                                result.changed_pixels
                            );
                        }
                        render_retry_count = 0;
                    } else {
                        last_render_error = None;
                        render_retry_count = render_retry_count.saturating_add(1);
                        if render_retry_count == 1 {
                            crate::log_trace!(
                                target: "draw3d";
                                "draw3d: frame retry revision={} draws={}/{} reason=incomplete\n",
                                resident_revision,
                                result.completed_draws,
                                result.requested_draws
                            );
                        } else if render_retry_count == RENDER_STALL_WARN_ATTEMPTS {
                            crate::log_warn!(
                                target: "draw3d";
                                "draw3d: frame stalled revision={} attempts={} draws={}/{} action=retain-and-retry diagnostics=next-attempt\n",
                                resident_revision,
                                render_retry_count,
                                result.completed_draws,
                                result.requested_draws
                            );
                        } else if render_retry_count.is_multiple_of(RENDER_RETRY_TRACE_INTERVAL) {
                            crate::log_trace!(
                                target: "draw3d";
                                "draw3d: frame still retrying revision={} attempts={} draws={}/{}\n",
                                resident_revision,
                                render_retry_count,
                                result.completed_draws,
                                result.requested_draws
                            );
                        }
                    }
                }
                Err(error) => {
                    render_retry_count = render_retry_count.saturating_add(1);
                    if last_render_error != Some(error) {
                        crate::log_trace!(
                            target: "draw3d";
                            "draw3d: frame retry revision={} jobs={} error={}\n",
                            resident_revision,
                            jobs.len(),
                            error
                        );
                        last_render_error = Some(error);
                    }
                    if render_retry_count == RENDER_STALL_WARN_ATTEMPTS {
                        crate::log_warn!(
                            target: "draw3d";
                            "draw3d: frame stalled revision={} attempts={} jobs={} error={} action=retry diagnostics=next-attempt\n",
                            resident_revision,
                            render_retry_count,
                            jobs.len(),
                            error
                        );
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
                    if placeholder { "placeholder" } else { "presented" },
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
