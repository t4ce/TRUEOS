use alloc::{collections::VecDeque, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;
use trueos_draw3d::{
    ApplyError, Command, Face, Instance, Mesh, Response, ResponseError, Rgba8, SceneStats,
    Transform, Vec3,
};

use crate::intel::gpu_font::{GpuFontColorProgram, GpuFontRgba, GpuFontSceneMesh};
use crate::r::draw3d_client::{Draw3dClientError, Draw3dTcpClient};

const FRAME_MS: u64 = 33;
const STATS_INTERVAL_MS: u64 = 1_000;
const REQUEST_CAP: usize = 64;
const FONT_ID_BASE: u64 = 0x5348_4632_0000_0000;
const SCENE_CLEAR: Rgba8 = Rgba8::new(255, 255, 255, 255);
const AUTO_LAYOUT_TOTAL_SCALE: f32 = 0.84;
const AUTO_LAYOUT_ROW_STEP: f32 = 3.4;

static TASK_STARTED: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static REQUESTS: Mutex<VecDeque<FontSceneRequest>> = Mutex::new(VecDeque::new());
static STATUS: Mutex<FontSceneStatus> = Mutex::new(FontSceneStatus::new());

pub(crate) struct FontSceneAdd {
    pub(crate) mesh: GpuFontSceneMesh,
    pub(crate) color_program: GpuFontColorProgram,
    pub(crate) transform: Transform,
    pub(crate) auto_layout: bool,
}

enum FontSceneRequest {
    Add(Vec<FontSceneAdd>),
    SetColorProgram(GpuFontColorProgram),
    Stop,
}

struct FontSceneItem {
    mesh_id: u64,
    instance_id: u64,
    pending_mesh: Option<GpuFontSceneMesh>,
    transform: Transform,
    auto_layout: bool,
    transform_dirty: bool,
    color_program: GpuFontColorProgram,
    started_ms: u64,
    last_color: GpuFontRgba,
    installed: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct FontSceneStatus {
    pub(crate) items: usize,
    pub(crate) pending: usize,
    pub(crate) running: bool,
    pub(crate) mesh_count: u32,
    pub(crate) instance_count: u32,
    pub(crate) last_error: Option<&'static str>,
}

impl FontSceneStatus {
    const fn new() -> Self {
        Self {
            items: 0,
            pending: 0,
            running: false,
            mesh_count: 0,
            instance_count: 0,
            last_error: None,
        }
    }
}

pub(crate) fn status() -> FontSceneStatus {
    *STATUS.lock()
}

pub(crate) fn ensure_task(spawner: &Spawner) -> Result<(), &'static str> {
    if TASK_STARTED.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    match font_draw3d_task() {
        Ok(token) => {
            spawner.spawn(token);
            Ok(())
        }
        Err(_) => {
            TASK_STARTED.store(false, Ordering::Release);
            Err("draw3d-font-task-unavailable")
        }
    }
}

pub(crate) fn add(spawner: &Spawner, additions: Vec<FontSceneAdd>) -> Result<(), &'static str> {
    ensure_task(spawner)?;
    push_request(FontSceneRequest::Add(additions))
}

pub(crate) fn set_color_program(
    spawner: &Spawner,
    program: GpuFontColorProgram,
) -> Result<(), &'static str> {
    ensure_task(spawner)?;
    push_request(FontSceneRequest::SetColorProgram(program))
}

pub(crate) fn stop(spawner: &Spawner) -> Result<(), &'static str> {
    ensure_task(spawner)?;
    push_request(FontSceneRequest::Stop)
}

fn push_request(request: FontSceneRequest) -> Result<(), &'static str> {
    let mut requests = REQUESTS.lock();
    if requests.len() >= REQUEST_CAP {
        return Err("draw3d-font-queue-full");
    }
    requests.push_back(request);
    Ok(())
}

fn next_ids() -> (u64, u64) {
    let sequence = NEXT_ID.fetch_add(1, Ordering::AcqRel).max(1);
    let id = FONT_ID_BASE | sequence;
    (id, id)
}

fn rgba(color: GpuFontRgba) -> Rgba8 {
    Rgba8::new(color.r, color.g, color.b, color.a)
}

fn reflow_auto_items(items: &mut [FontSceneItem]) {
    let count = items.iter().filter(|item| item.auto_layout).count();
    if count == 0 {
        return;
    }
    let scale = AUTO_LAYOUT_TOTAL_SCALE / count as f32;
    let row_step = AUTO_LAYOUT_ROW_STEP * scale;
    let center = (count.saturating_sub(1)) as f32 * 0.5;
    let mut row = 0usize;
    for item in items.iter_mut().filter(|item| item.auto_layout) {
        let transform = Transform {
            location: Vec3::new(0.0, (center - row as f32) * row_step, 0.0),
            rotation: Vec3::ZERO,
            scale: Vec3::new(scale, scale, scale),
        };
        if item.transform != transform {
            item.transform = transform;
            item.transform_dirty = item.installed;
        }
        row += 1;
    }
    crate::log_info!(
        target: "draw3d";
        "shell2/font: auto-layout rows={} scale={:.3} row_step={:.3} centered=1\n",
        count,
        scale,
        row_step,
    );
}

fn draw3d_mesh(mesh: &GpuFontSceneMesh, color: GpuFontRgba) -> Mesh {
    let vertices = mesh
        .vertices
        .iter()
        .map(|point| Vec3::new(point[0], point[1], point[2]))
        .collect();
    let faces = mesh
        .indices
        .chunks_exact(3)
        .map(|triangle| Face::new(triangle.to_vec()))
        .collect();
    Mesh::new(vertices, Vec::new(), faces, rgba(color))
}

async fn request(
    client: &mut Option<Draw3dTcpClient>,
    command: &Command,
) -> Result<Response, Draw3dClientError> {
    if client.is_none() {
        *client = Some(Draw3dTcpClient::connect_loopback().await?);
    }
    let result = client.as_mut().unwrap().request(command).await;
    result
}

fn record_error(error: Draw3dClientError) {
    STATUS.lock().last_error = Some(error.label());
    crate::log_error!(
        target: "draw3d";
        "shell2/font: draw3d tcp error={} port={}\n",
        error.label(),
        crate::r::draw3d_service::TCP_PORT,
    );
}

fn response_error_label(error: ResponseError) -> &'static str {
    match error {
        ResponseError::Decode(_) => "service-decode-error",
        ResponseError::Apply(ApplyError::MeshMissing) => "mesh-missing",
        ResponseError::Apply(ApplyError::InstanceMissing) => "instance-missing",
        ResponseError::Apply(ApplyError::TargetExists) => "target-exists",
        ResponseError::Apply(ApplyError::MeshInUse) => "mesh-in-use",
        ResponseError::Apply(ApplyError::MeshLimit) => "mesh-limit",
        ResponseError::Apply(ApplyError::InstanceLimit) => "instance-limit",
        ResponseError::Apply(ApplyError::VertexLimit) => "vertex-limit",
        ResponseError::Apply(ApplyError::EdgeLimit) => "edge-limit",
        ResponseError::Apply(ApplyError::FaceLimit) => "face-limit",
        ResponseError::Apply(ApplyError::FaceVertexLimit) => "face-vertex-limit",
        ResponseError::Apply(ApplyError::FaceTooSmall) => "face-too-small",
        ResponseError::Apply(ApplyError::VertexIndexOutOfRange) => "vertex-index-out-of-range",
        ResponseError::Apply(ApplyError::NonFiniteVector) => "non-finite-vector",
        ResponseError::Apply(ApplyError::InvalidClipPlanes) => "invalid-clip-planes",
        ResponseError::Apply(ApplyError::InvalidFieldOfView) => "invalid-field-of-view",
        ResponseError::Apply(ApplyError::ZeroViewDirection) => "zero-view-direction",
        ResponseError::Apply(ApplyError::ZeroUpAxis) => "zero-up-axis",
        ResponseError::Apply(ApplyError::ParallelCameraAxes) => "parallel-camera-axes",
    }
}

fn record_response_error(error: ResponseError) {
    let label = response_error_label(error);
    STATUS.lock().last_error = Some(label);
    crate::log_error!(
        target: "draw3d";
        "shell2/font: draw3d service rejection={} port={}\n",
        label,
        crate::r::draw3d_service::TCP_PORT,
    );
}

fn mesh_missing(error: ResponseError) -> bool {
    matches!(error, ResponseError::Apply(ApplyError::MeshMissing))
}

fn update_status(items: &[FontSceneItem], running: bool, stats: Option<SceneStats>) {
    let mut status = STATUS.lock();
    status.items = items.len();
    status.pending = items.iter().filter(|item| !item.installed).count();
    status.running = running;
    if let Some(stats) = stats {
        status.mesh_count = stats.mesh_count;
        status.instance_count = stats.instance_count;
    }
}

async fn refresh_stats(client: &mut Option<Draw3dTcpClient>) -> Option<SceneStats> {
    match request(client, &Command::GetStats).await {
        Ok(Response::Stats(stats)) => Some(stats),
        Ok(Response::Error(error)) => {
            record_response_error(error);
            None
        }
        Ok(_) => None,
        Err(error) => {
            record_error(error);
            None
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
async fn font_draw3d_task() {
    let mut client = None;
    let mut items: Vec<FontSceneItem> = Vec::new();
    let mut running = false;
    let mut stop_requested = false;
    let mut next_stats_ms = 0u64;

    loop {
        while let Some(command) = REQUESTS.lock().pop_front() {
            match command {
                FontSceneRequest::Add(additions) => {
                    let now_ms = Instant::now().as_millis();
                    for addition in additions {
                        let (mesh_id, instance_id) = next_ids();
                        let initial = addition.color_program.sample(0);
                        items.push(FontSceneItem {
                            mesh_id,
                            instance_id,
                            pending_mesh: Some(addition.mesh),
                            transform: addition.transform,
                            auto_layout: addition.auto_layout,
                            transform_dirty: false,
                            color_program: addition.color_program,
                            started_ms: now_ms,
                            last_color: initial,
                            installed: false,
                        });
                    }
                    reflow_auto_items(items.as_mut_slice());
                    stop_requested = false;
                }
                FontSceneRequest::SetColorProgram(program) => {
                    let now_ms = Instant::now().as_millis();
                    for item in &mut items {
                        item.color_program = program;
                        item.started_ms = now_ms;
                    }
                }
                FontSceneRequest::Stop => stop_requested = true,
            }
        }

        if stop_requested {
            let mut index = 0usize;
            while index < items.len() {
                let command = Command::DeleteMesh {
                    mesh_id: items[index].mesh_id,
                    cascade: true,
                };
                match request(&mut client, &command).await {
                    Ok(Response::Applied(_)) => {
                        items.swap_remove(index);
                    }
                    Ok(Response::Error(error)) if mesh_missing(error) => {
                        items.swap_remove(index);
                    }
                    Ok(Response::Error(error)) => {
                        record_response_error(error);
                        index += 1;
                    }
                    Ok(_) => index += 1,
                    Err(error) => {
                        record_error(error);
                        break;
                    }
                }
            }
            if items.is_empty() {
                if let Some(stats) = refresh_stats(&mut client).await
                    && stats.instance_count == 0
                {
                    match request(&mut client, &Command::StopScene).await {
                        Ok(Response::Applied(_)) => running = false,
                        Ok(Response::Error(error)) => record_response_error(error),
                        Ok(_) => {}
                        Err(error) => record_error(error),
                    }
                }
                stop_requested = false;
                if let Some(client) = client.as_mut() {
                    client.disconnect();
                }
                crate::log_info!(
                    target: "draw3d";
                    "shell2/font: scene-forget complete=1 ownership=tcp meshes=0 instances=0\n",
                );
            }
        } else {
            let mut index = 0usize;
            while index < items.len() {
                if items[index].installed {
                    index += 1;
                    continue;
                }
                let Some(mesh) = items[index].pending_mesh.as_ref() else {
                    index += 1;
                    continue;
                };
                let put_mesh = Command::PutMesh {
                    mesh_id: items[index].mesh_id,
                    mesh: draw3d_mesh(mesh, items[index].last_color),
                };
                match request(&mut client, &put_mesh).await {
                    Ok(Response::Applied(_)) => {}
                    Ok(Response::Error(error)) => {
                        record_response_error(error);
                        items.swap_remove(index);
                        reflow_auto_items(items.as_mut_slice());
                        continue;
                    }
                    Ok(_) => {
                        index += 1;
                        continue;
                    }
                    Err(error) => {
                        record_error(error);
                        break;
                    }
                }
                let put_instance = Command::PutInstance {
                    instance_id: items[index].instance_id,
                    instance: Instance::new(items[index].mesh_id, items[index].transform),
                };
                match request(&mut client, &put_instance).await {
                    Ok(Response::Applied(_)) => {
                        items[index].installed = true;
                        items[index].pending_mesh = None;
                        items[index].transform_dirty = false;
                        index += 1;
                    }
                    Ok(Response::Error(error)) => {
                        record_response_error(error);
                        let cleanup = Command::DeleteMesh {
                            mesh_id: items[index].mesh_id,
                            cascade: true,
                        };
                        match request(&mut client, &cleanup).await {
                            Ok(Response::Applied(_))
                            | Ok(Response::Error(ResponseError::Apply(ApplyError::MeshMissing))) => {
                                items.swap_remove(index);
                                reflow_auto_items(items.as_mut_slice());
                            }
                            Ok(Response::Error(cleanup_error)) => {
                                record_response_error(cleanup_error);
                                index += 1;
                            }
                            Ok(_) => index += 1,
                            Err(cleanup_error) => {
                                record_error(cleanup_error);
                                break;
                            }
                        }
                    }
                    Ok(_) => index += 1,
                    Err(error) => {
                        record_error(error);
                        break;
                    }
                }
            }

            let mut index = 0usize;
            while index < items.len() {
                if !items[index].installed || !items[index].transform_dirty {
                    index += 1;
                    continue;
                }
                let command = Command::SetTransform {
                    instance_id: items[index].instance_id,
                    transform: items[index].transform,
                };
                match request(&mut client, &command).await {
                    Ok(Response::Applied(_)) => {
                        items[index].transform_dirty = false;
                        index += 1;
                    }
                    Ok(Response::Error(ResponseError::Apply(ApplyError::InstanceMissing))) => {
                        items.swap_remove(index);
                        reflow_auto_items(items.as_mut_slice());
                    }
                    Ok(Response::Error(error)) => {
                        record_response_error(error);
                        index += 1;
                    }
                    Ok(_) => index += 1,
                    Err(error) => {
                        record_error(error);
                        break;
                    }
                }
            }

            if items.iter().any(|item| item.installed) && !running {
                match request(
                    &mut client,
                    &Command::StartScene {
                        clear: Some(SCENE_CLEAR),
                    },
                )
                .await
                {
                    Ok(Response::Applied(_)) => running = true,
                    Ok(Response::Error(error)) => record_response_error(error),
                    Ok(_) => {}
                    Err(error) => record_error(error),
                }
            }

            let now_ms = Instant::now().as_millis();
            let mut index = 0usize;
            while index < items.len() {
                if !items[index].installed {
                    index += 1;
                    continue;
                }
                let color = items[index]
                    .color_program
                    .sample(now_ms.saturating_sub(items[index].started_ms));
                if color == items[index].last_color {
                    index += 1;
                    continue;
                }
                let command = Command::SetColor {
                    mesh_id: items[index].mesh_id,
                    color: rgba(color),
                };
                match request(&mut client, &command).await {
                    Ok(Response::Applied(_)) => {
                        items[index].last_color = color;
                        index += 1;
                    }
                    Ok(Response::Error(error)) if mesh_missing(error) => {
                        items.swap_remove(index);
                        reflow_auto_items(items.as_mut_slice());
                    }
                    Ok(Response::Error(error)) => {
                        record_response_error(error);
                        index += 1;
                    }
                    Ok(_) => index += 1,
                    Err(error) => {
                        record_error(error);
                        break;
                    }
                }
            }
        }

        let now_ms = Instant::now().as_millis();
        let stats = if !items.is_empty() && now_ms >= next_stats_ms {
            next_stats_ms = now_ms.saturating_add(STATS_INTERVAL_MS);
            refresh_stats(&mut client).await
        } else {
            None
        };
        update_status(items.as_slice(), running, stats);
        Timer::after(Duration::from_millis(FRAME_MS)).await;
    }
}
