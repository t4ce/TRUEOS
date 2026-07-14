//! Compact TCP scene store for renderer-independent 3D geometry placement.
//!
//! The wire/state implementation lives in `trueos-draw3d`; this module only adapts it to
//! TRUEOS's native network queues and publishes the live scene to future renderers.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use trueos_draw3d::{
    Command, FrameDecoder, ImageFormat, ProjectedMesh, RenderImage, Response, ResponseError, Scene,
    SceneStats, encode_response, project_scene,
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
static SCENE_REVISION: AtomicU64 = AtomicU64::new(1);
static SCENE_GEOMETRY_REVISION: AtomicU64 = AtomicU64::new(1);

const FRAME_PERIOD_MS: u64 = 33;
const FRAME_LOG_INTERVAL: u64 = 300;

struct SceneRenderJob {
    instance_id: u64,
    mesh_id: u64,
    color: trueos_draw3d::Rgba8,
    resident: crate::intel::render::ResidentTriangleMesh,
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
    let geometry_changed = !matches!(&command, Command::SetColor { .. });
    let mut scene = SCENE.lock();
    let outcome = scene.get_or_insert_with(Scene::default).apply(command)?;
    SCENE_REVISION.fetch_add(1, Ordering::AcqRel);
    if geometry_changed {
        SCENE_GEOMETRY_REVISION.fetch_add(1, Ordering::AcqRel);
    }
    Ok(outcome)
}

fn projected_scene() -> Vec<ProjectedMesh> {
    with_scene(|scene| project_scene(scene, 1.0))
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
    let mut frame = 0u64;
    let mut next_frame = Instant::now();
    let mut last_sync_error = None;
    let mut last_render_error = None;

    crate::log_info!(
        target: "draw3d";
        "draw3d: render engine online target_fps=30 frame_ms={} max_meshes=100 max_instances=100 max_vertices_per_mesh=1000 target=512x512\n",
        FRAME_PERIOD_MS
    );
    loop {
        next_frame += EmbassyDuration::from_millis(FRAME_PERIOD_MS);
        let revision = SCENE_REVISION.load(Ordering::Acquire);
        let geometry_revision = SCENE_GEOMETRY_REVISION.load(Ordering::Acquire);
        if revision != resident_revision {
            let had_jobs = !jobs.is_empty();
            let sync_result = if geometry_revision != resident_geometry_revision {
                sync_render_jobs(&mut jobs, projected_scene())
            } else {
                refresh_render_job_colors(&mut jobs);
                Ok(())
            };
            match sync_result {
                Ok(()) => {
                    resident_revision = revision;
                    resident_geometry_revision = geometry_revision;
                    last_sync_error = None;
                    crate::log_info!(
                        target: "draw3d";
                        "draw3d: scene resident revision={} jobs={}\n",
                        revision,
                        jobs.len()
                    );
                    if had_jobs && jobs.is_empty() {
                        let _ = crate::intel::render::submit_resident_triangle_scene_frame(&[]);
                    }
                }
                Err(error) => {
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

        if !jobs.is_empty() {
            let draws = jobs
                .iter()
                .map(|job| crate::intel::render::ResidentSceneDraw {
                    mesh: &job.resident,
                    rgba: [job.color.r, job.color.g, job.color.b, job.color.a],
                })
                .collect::<Vec<_>>();
            match crate::intel::render::submit_resident_triangle_scene_frame(&draws) {
                Ok(result) => {
                    last_render_error = None;
                    if frame == 0 || frame.is_multiple_of(FRAME_LOG_INTERVAL) {
                        crate::log_info!(
                            target: "draw3d";
                            "draw3d: frame success frame={} revision={} draws={}/{} changed_pixels={} presented={}\n",
                            frame,
                            resident_revision,
                            result.completed_draws,
                            result.requested_draws,
                            result.changed_pixels,
                            result.presented as u8
                        );
                    }
                }
                Err(error) => {
                    if last_render_error != Some(error) {
                        crate::log_warn!(
                            target: "draw3d";
                            "draw3d: frame pending frame={} jobs={} error={}\n",
                            frame,
                            jobs.len(),
                            error
                        );
                        last_render_error = Some(error);
                    }
                }
            }
        }
        frame = frame.wrapping_add(1);

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
        crate::log_warn!(target: "draw3d"; "draw3d: listener request queue full port={}\n", TCP_PORT);
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
        crate::log_warn!(
            target: "draw3d";
            "draw3d: reply queue full handle={} bytes={}\n",
            handle.0,
            len
        );
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
            Command::RequestRender => Response::RenderImage(RenderImage {
                format: ImageFormat::Jpeg,
                width: PLACEHOLDER_RENDER_WIDTH,
                height: PLACEHOLDER_RENDER_HEIGHT,
                bytes: PLACEHOLDER_RENDER_JPEG.to_vec(),
            }),
            command => match apply_command(command) {
                Ok(outcome) => Response::Applied(outcome),
                Err(error) => Response::Error(ResponseError::Apply(error)),
            },
        };

        match &response {
            Response::Applied(outcome) => crate::log_info!(
                target: "draw3d";
                "draw3d: success command={} request={} handle={} affected={} meshes={} instances={} vertices={} edges={} faces={} mesh_bytes={}\n",
                command_name,
                request_id,
                handle.0,
                outcome.affected,
                outcome.stats.mesh_count,
                outcome.stats.instance_count,
                outcome.stats.vertex_count,
                outcome.stats.edge_count,
                outcome.stats.face_count,
                outcome.stats.mesh_bytes
            ),
            Response::Stats(stats) => crate::log_info!(
                target: "draw3d";
                "draw3d: success command=get_stats request={} handle={} meshes={} instances={} vertices={} edges={} faces={} mesh_bytes={}\n",
                request_id,
                handle.0,
                stats.mesh_count,
                stats.instance_count,
                stats.vertex_count,
                stats.edge_count,
                stats.face_count,
                stats.mesh_bytes
            ),
            Response::Pong(nonce) => crate::log_info!(
                target: "draw3d";
                "draw3d: success command=ping request={} handle={} nonce={}\n",
                request_id,
                handle.0,
                nonce
            ),
            Response::RenderImage(image) => {
                let camera = with_scene(Scene::camera);
                crate::log_info!(
                    target: "draw3d";
                    "draw3d: success command=request_render request={} handle={} placeholder=1 format={:?} width={} height={} bytes={} camera_pos=({},{},{}) camera_dir=({},{},{}) camera_up=({},{},{}) near={} far={} vfov={}\n",
                    request_id,
                    handle.0,
                    image.format,
                    image.width,
                    image.height,
                    image.bytes.len(),
                    camera.position.x,
                    camera.position.y,
                    camera.position.z,
                    camera.view_direction.x,
                    camera.view_direction.y,
                    camera.view_direction.z,
                    camera.up_axis.x,
                    camera.up_axis.y,
                    camera.up_axis.z,
                    camera.near_plane,
                    camera.far_plane,
                    camera.vertical_fov
                );
            }
            Response::Error(error) => crate::log_warn!(
                target: "draw3d";
                "draw3d: command failed command={} request={} handle={} error={:?}\n",
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
        "draw3d: service ready tcp_port={} protocol_version={} max_payload={}\n",
        TCP_PORT,
        trueos_draw3d::PROTOCOL_VERSION,
        trueos_draw3d::MAX_PAYLOAD_LEN
    );

    let mut listener: Option<NetHandle> = None;
    let mut decoders: BTreeMap<u32, FrameDecoder> = BTreeMap::new();
    let mut retry_ticks: u16 = 0;

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
                    crate::log_info!(
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
                    crate::log_info!(
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
                    crate::log_warn!(target: "draw3d"; "draw3d: network error msg={}\n", msg);
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
