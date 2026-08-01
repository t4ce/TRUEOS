//! On-demand presentation of the build-produced Helio game artifact.
//!
//! Artifact parsing and CPU-side dynamic-camera lowering are reusable and
//! scene-independent. This module owns only TRUEOS residency, its existing
//! one-GuC Render submission, and UI4 publication.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};
use embassy_time::{Duration, Instant, Timer};

const GAME_ARTIFACT: &[u8] = include_bytes!("../../assets/helio/simple-cube.trueos.intel.helio");
const OWNER: crate::ui4::WindowOwner = crate::ui4::WindowOwner::KernelApp(8);
const PLANE_SLOT: usize = crate::ui4::RGB_OVERLAY_PLANE_SLOT_2;
const MARGIN: u32 = 48;
const TRANSPARENT_CLEAR_RGBA: [u8; 4] = [0, 0, 0, 0];

const STATE_IDLE: u8 = 0;
const STATE_PHASE_MASK: u8 = 0xf0;
const STATE_ID_MASK: u8 = 0x0f;
const STATE_REQUESTED: u8 = 0x10;
const STATE_STARTING: u8 = 0x20;
const STATE_ONLINE: u8 = 0x30;

static GAME_STATE: AtomicU8 = AtomicU8::new(STATE_IDLE);
static LAST_ERROR: spin::Mutex<Option<&'static str>> = spin::Mutex::new(None);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GameState {
    Idle,
    Requested(u8),
    Starting(u8),
    Online(u8),
}

impl GameState {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Requested(_) => "requested",
            Self::Starting(_) => "starting",
            Self::Online(_) => "online",
        }
    }

    pub(crate) const fn example_id(self) -> Option<u8> {
        match self {
            Self::Idle => None,
            Self::Requested(id) | Self::Starting(id) | Self::Online(id) => Some(id),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LaunchRequest {
    Queued,
    AlreadyRequested(u8),
    AlreadyStarting(u8),
    AlreadyOnline(u8),
    Reserved,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GameStatus {
    pub(crate) state: GameState,
    pub(crate) artifact_bytes: usize,
    pub(crate) last_error: Option<&'static str>,
}

fn game_state() -> GameState {
    let state = GAME_STATE.load(Ordering::Acquire);
    let id = state & STATE_ID_MASK;
    match state & STATE_PHASE_MASK {
        STATE_REQUESTED => GameState::Requested(id),
        STATE_STARTING => GameState::Starting(id),
        STATE_ONLINE => GameState::Online(id),
        _ => GameState::Idle,
    }
}

/// Queue the embedded game for the already-resident AP1/UI runtime service.
pub(crate) fn request_launch(example_id: u8) -> LaunchRequest {
    if !(1..=4).contains(&example_id) {
        return LaunchRequest::Reserved;
    }
    match GAME_STATE.compare_exchange(
        STATE_IDLE,
        STATE_REQUESTED | example_id,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => LaunchRequest::Queued,
        Err(state) => match decode_non_idle_state(state) {
            GameState::Requested(id) => LaunchRequest::AlreadyRequested(id),
            GameState::Starting(id) => LaunchRequest::AlreadyStarting(id),
            GameState::Online(id) => LaunchRequest::AlreadyOnline(id),
            GameState::Idle => LaunchRequest::AlreadyRequested(example_id),
        },
    }
}

fn decode_non_idle_state(state: u8) -> GameState {
    let id = state & STATE_ID_MASK;
    match state & STATE_PHASE_MASK {
        STATE_REQUESTED => GameState::Requested(id),
        STATE_STARTING => GameState::Starting(id),
        STATE_ONLINE => GameState::Online(id),
        _ => GameState::Idle,
    }
}

pub(crate) fn status() -> GameStatus {
    GameStatus {
        state: game_state(),
        artifact_bytes: GAME_ARTIFACT.len(),
        last_error: *LAST_ERROR.lock(),
    }
}

#[derive(Copy, Clone, Debug)]
enum GameError {
    Artifact(trueos_helio_runtime::Error),
    Frame(crate::ui4::FramePoolError),
    Window(crate::ui4::WindowBrokerError),
    Render(&'static str),
    InvalidFrame,
}

impl GameError {
    const fn label(self) -> &'static str {
        match self {
            Self::Artifact(_) => "artifact",
            Self::Frame(_) => "frame-pool",
            Self::Window(_) => "window-broker",
            Self::Render(reason) => reason,
            Self::InvalidFrame => "invalid-frame",
        }
    }
}

impl From<crate::ui4::FramePoolError> for GameError {
    fn from(error: crate::ui4::FramePoolError) -> Self {
        Self::Frame(error)
    }
}

impl From<crate::ui4::WindowBrokerError> for GameError {
    fn from(error: crate::ui4::WindowBrokerError) -> Self {
        Self::Window(error)
    }
}

struct ResidentTriangle {
    mesh: crate::intel::render::ResidentTriangleMesh,
    rgba: [u8; 4],
}

struct FlyCamera {
    camera: trueos_helio_runtime::Camera,
    yaw: f32,
    pitch: f32,
    look_active: bool,
}

impl FlyCamera {
    fn new(camera: trueos_helio_runtime::Camera) -> Self {
        let direction = [
            camera.target[0] - camera.position[0],
            camera.target[1] - camera.position[1],
            camera.target[2] - camera.position[2],
        ];
        let length = libm::sqrtf(
            direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2],
        )
        .max(f32::EPSILON);
        Self {
            camera,
            yaw: libm::atan2f(direction[0], -direction[2]),
            pitch: libm::asinf((direction[1] / length).clamp(-1.0, 1.0)),
            look_active: false,
        }
    }

    fn apply_events(&mut self, events: &[crate::ui4::winit_input::Event]) -> bool {
        use crate::ui4::winit_input::{
            DeviceEvent, ElementState, Event, KeyCode, MouseButton, PhysicalKey, WindowEvent,
        };

        let mut changed = false;
        for event in events {
            match *event {
                Event::WindowEvent {
                    event:
                        WindowEvent::MouseInput {
                            state,
                            button: MouseButton::Left,
                        },
                    ..
                } => self.look_active = state == ElementState::Pressed,
                Event::WindowEvent {
                    event: WindowEvent::Focused(false),
                    ..
                } => self.look_active = false,
                Event::WindowEvent {
                    event:
                        WindowEvent::KeyboardInput {
                            event:
                                crate::ui4::winit_input::KeyEvent {
                                    physical_key: PhysicalKey::Code(KeyCode::Escape),
                                    state: ElementState::Pressed,
                                    ..
                                },
                        },
                    ..
                } => self.look_active = false,
                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta: (dx, dy) },
                    ..
                } if self.look_active => {
                    self.yaw += dx as f32 * 0.003;
                    self.pitch = (self.pitch - dy as f32 * 0.003).clamp(-1.5, 1.5);
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            self.update_target();
        }
        changed
    }

    fn apply_held_keys(
        &mut self,
        input: &crate::ui4::winit_input::EventLoopInput,
        window: crate::ui4::WindowId,
        dt_seconds: f32,
    ) -> bool {
        use crate::ui4::winit_input::KeyCode;

        let forward = [libm::sinf(self.yaw), 0.0, -libm::cosf(self.yaw)];
        let right = [libm::cosf(self.yaw), 0.0, libm::sinf(self.yaw)];
        let mut movement = [0.0f32; 3];
        if input.key_is_down(window, KeyCode::KeyW) {
            add_scaled(&mut movement, forward, 1.0);
        }
        if input.key_is_down(window, KeyCode::KeyS) {
            add_scaled(&mut movement, forward, -1.0);
        }
        if input.key_is_down(window, KeyCode::KeyD) {
            add_scaled(&mut movement, right, 1.0);
        }
        if input.key_is_down(window, KeyCode::KeyA) {
            add_scaled(&mut movement, right, -1.0);
        }
        if input.key_is_down(window, KeyCode::Space) {
            movement[1] += 1.0;
        }
        if input.key_is_down(window, KeyCode::ShiftLeft) {
            movement[1] -= 1.0;
        }
        let length = libm::sqrtf(
            movement[0] * movement[0] + movement[1] * movement[1] + movement[2] * movement[2],
        );
        if length <= f32::EPSILON {
            return false;
        }
        let distance = 4.0 * dt_seconds / length;
        add_scaled(&mut self.camera.position, movement, distance);
        self.update_target();
        true
    }

    fn update_target(&mut self) {
        let cos_pitch = libm::cosf(self.pitch);
        self.camera.target = [
            self.camera.position[0] + libm::sinf(self.yaw) * cos_pitch,
            self.camera.position[1] + libm::sinf(self.pitch),
            self.camera.position[2] - libm::cosf(self.yaw) * cos_pitch,
        ];
    }
}

fn add_scaled(target: &mut [f32; 3], value: [f32; 3], scale: f32) {
    for index in 0..3 {
        target[index] += value[index] * scale;
    }
}

struct GameSurface {
    session: crate::ui4::WindowSessionId,
    frame: crate::ui4::FrameHandle,
    window: Option<crate::ui4::WindowId>,
    width: u32,
    height: u32,
}

#[derive(Copy, Clone)]
struct PendingGameResize {
    frame: crate::ui4::FrameHandle,
    width: u32,
    height: u32,
}

fn create_game_frame(width: u32, height: u32) -> Result<crate::ui4::FrameHandle, GameError> {
    let (max_width, max_height) = crate::intel::render::resident_scene_target_dimensions();
    if width == 0 || height == 0 || width as usize > max_width || height as usize > max_height {
        return Err(GameError::InvalidFrame);
    }
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    Ok(crate::ui4::create_frame(crate::ui4::FrameSpec {
        output,
        content: crate::ui4::FrameContent::RenderScene3d,
        cadence: crate::ui4::FrameCadence::Streaming,
        buffering: crate::ui4::FrameBuffering::Triple,
        format: crate::ui4::ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(crate::ui4::PremultipliedRgba8::TRANSPARENT),
    })?)
}

fn create_surface() -> Result<GameSurface, GameError> {
    let (render_width, render_height) = crate::intel::render::resident_scene_target_dimensions();
    let (scanout_width, scanout_height) = crate::intel::active_scanout_dimensions()
        .unwrap_or((crate::ui4::DEFAULT_FRAME_WIDTH, crate::ui4::DEFAULT_FRAME_HEIGHT));
    let width = crate::ui4::DEFAULT_FRAME_WIDTH
        .min(scanout_width)
        .min(render_width as u32);
    let height = crate::ui4::DEFAULT_FRAME_HEIGHT
        .min(scanout_height)
        .min(render_height as u32);
    if width == 0 || height == 0 {
        return Err(GameError::InvalidFrame);
    }
    let frame = create_game_frame(width, height)?;
    let session = match crate::ui4::begin_window_session(OWNER) {
        Ok(session) => session,
        Err(error) => {
            let _ = crate::ui4::destroy_frame(frame);
            return Err(error.into());
        }
    };
    Ok(GameSurface {
        session,
        frame,
        window: None,
        width,
        height,
    })
}

fn desired_resize(surface: &GameSurface) -> Option<(u32, u32)> {
    let window = surface.window?;
    let placement = crate::ui4::window_placement(OWNER, window).ok()?;
    let target = helio_backing_extent(placement.width, placement.height);
    (target != (surface.width, surface.height)).then_some(target)
}

/// A maximized Helio scene keeps the broker's full logical extent while the
/// universal display plane scales a half-resolution backing directly. This is
/// UI4's native maximize fast path and prevents a 2560x1440 churn frame from
/// monopolizing the shared RCS lane; ordinary 768x512 windows stay exact 1:1.
const fn helio_backing_extent(logical_width: u32, logical_height: u32) -> (u32, u32) {
    if logical_width > crate::ui4::DEFAULT_FRAME_WIDTH
        && logical_height > crate::ui4::DEFAULT_FRAME_HEIGHT
    {
        (logical_width.div_ceil(2), logical_height.div_ceil(2))
    } else {
        (logical_width, logical_height)
    }
}

fn prepare_resize(
    surface: &GameSurface,
    width: u32,
    height: u32,
) -> Result<PendingGameResize, GameError> {
    surface.window.ok_or(GameError::InvalidFrame)?;
    Ok(PendingGameResize {
        frame: create_game_frame(width, height)?,
        width,
        height,
    })
}

/// Make an already GPU-authored replacement visible in one broker transition.
/// The old frame remains attached if the Ready publication fails.
fn commit_resize(
    surface: &mut GameSurface,
    replacement: PendingGameResize,
) -> Result<crate::ui4::FrameHandle, GameError> {
    let window = surface.window.ok_or(GameError::InvalidFrame)?;
    let previous = surface.frame;
    crate::ui4::replace_window_frame(OWNER, window, replacement.frame)?;
    if let Err(error) =
        crate::ui4::publish_window_frame(OWNER, window, crate::ui4::DamageRect::FULL)
    {
        let _ = crate::ui4::replace_window_frame(OWNER, window, previous);
        let _ = crate::ui4::publish_window_frame(OWNER, window, crate::ui4::DamageRect::FULL);
        return Err(error.into());
    }
    surface.frame = replacement.frame;
    surface.width = replacement.width;
    surface.height = replacement.height;
    Ok(previous)
}

fn retire_frames(frames: &mut Vec<crate::ui4::FrameHandle>) {
    frames.retain(|frame| {
        matches!(crate::ui4::destroy_frame(*frame), Err(crate::ui4::FramePoolError::Busy))
    });
}

fn make_resident_scene(
    scene: &trueos_helio_runtime::Scene,
) -> Result<Vec<ResidentTriangle>, GameError> {
    let mut resident = Vec::with_capacity(scene.triangles.len());
    for (triangle_index, triangle) in scene.triangles.iter().enumerate() {
        let args = match scene.resident_triangle_draw_indexed_indirect(triangle_index) {
            Ok(args) => args,
            Err(error) => {
                release_resident_scene(resident);
                return Err(GameError::Artifact(error));
            }
        };
        match crate::intel::render::create_resident_triangle_mesh(&triangle.vertices, &[0, 1, 2]) {
            Ok(mesh) => {
                if let Err(error) =
                    crate::intel::render::update_resident_triangle_draw_indexed_indirect(
                        &mesh,
                        args.index_count,
                        args.instance_count,
                        args.first_index,
                        args.base_vertex,
                        args.first_instance,
                    )
                {
                    let _ = crate::intel::render::release_resident_triangle_mesh(&mesh);
                    release_resident_scene(resident);
                    return Err(GameError::Render(error));
                }
                resident.push(ResidentTriangle {
                    mesh,
                    rgba: triangle.rgba,
                });
            }
            Err(error) => {
                release_resident_scene(resident);
                return Err(GameError::Render(error));
            }
        }
    }
    if resident.is_empty() {
        return Err(GameError::Render("helio-empty-scene"));
    }
    Ok(resident)
}

fn update_resident_scene(
    resident: &[ResidentTriangle],
    scene: &trueos_helio_runtime::Scene,
) -> Result<(), GameError> {
    if resident.len() != scene.triangles.len() {
        return Err(GameError::Render("helio-resize-triangle-count"));
    }
    for (triangle_index, (resident, triangle)) in resident.iter().zip(&scene.triangles).enumerate()
    {
        crate::intel::render::update_resident_triangle_mesh(
            &resident.mesh,
            &triangle.vertices,
            &[0, 1, 2],
        )
        .map_err(GameError::Render)?;
        let args = scene
            .resident_triangle_draw_indexed_indirect(triangle_index)
            .map_err(GameError::Artifact)?;
        crate::intel::render::update_resident_triangle_draw_indexed_indirect(
            &resident.mesh,
            args.index_count,
            args.instance_count,
            args.first_index,
            args.base_vertex,
            args.first_instance,
        )
        .map_err(GameError::Render)?;
    }
    Ok(())
}

fn release_resident_scene(resident: Vec<ResidentTriangle>) {
    for triangle in resident {
        if !crate::intel::render::release_resident_triangle_mesh(&triangle.mesh) {
            core::mem::forget(triangle);
        }
    }
}

fn gpu_logger_sample(
    result: &crate::intel::render::ResidentSceneFrameResult,
    frame_index: u64,
    cadence_us: u64,
    objects: usize,
    resident: &[ResidentTriangle],
    busy_retries: u64,
    incomplete_retries: u64,
) -> crate::spirit::gpu_logger::GpuLoggerSample {
    crate::spirit::gpu_logger::GpuLoggerSample {
        frame_index,
        cadence_us,
        frame_us: result.frame_us,
        geometry_us: result.geometry_us,
        prepare_us: result.geometry_prepare_us,
        retire_wait_us: result.gpu_poll_us,
        poll_iters: result.gpu_poll_iters,
        objects: u64::try_from(objects).unwrap_or(u64::MAX),
        draws: u64::try_from(result.requested_draws).unwrap_or(u64::MAX),
        triangles: resident
            .iter()
            .map(|triangle| u64::from(triangle.mesh.index_count / 3))
            .sum(),
        busy_retries,
        incomplete_retries,
    }
}

fn publish_gpu_logger_sample(sample: crate::spirit::gpu_logger::GpuLoggerSample) {
    crate::spirit::gpu_logger::publish(crate::spirit::gpu_logger::GpuLoggerSource::Helio, sample);
}

fn destroy_unpublished_surface(surface: GameSurface) {
    if let Some(window) = surface.window {
        let _ = crate::ui4::close_window(OWNER, window);
    }
    let _ = crate::ui4::finish_window_session(OWNER, surface.session);
    let _ = crate::ui4::destroy_frame(surface.frame);
}

async fn render_frame(
    frame: crate::ui4::FrameHandle,
    width: u32,
    height: u32,
    resident: &[ResidentTriangle],
    clear_rgba: [u8; 4],
) -> Result<crate::intel::render::ResidentSceneFrameResult, GameError> {
    // Render and the detached Spirit/font/compositor jobs all execute on the
    // one physical RCS0. Hold their existing fair lane through the exact
    // completion marker so a streaming game frame cannot reset or overwrite
    // another accepted context.
    let _gpu_lane = crate::r::font_kernel_service::acquire_gpu_lane(
        crate::r::font_kernel_service::FontKernelConsumer::new(
            crate::r::font_kernel_service::FontKernelConsumerPath::Helio,
            frame.raw(),
        ),
    )
    .await;
    let lease = crate::ui4::acquire_frame_buffer(frame)?;
    let destination = match crate::ui4::gpgpu_rgba_surface(lease) {
        Ok(destination)
            if destination.width == width
                && destination.height == height
                && destination.pitch_bytes >= width.saturating_mul(4) =>
        {
            destination
        }
        Ok(_) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(GameError::InvalidFrame);
        }
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    let draws = resident
        .iter()
        .map(|triangle| crate::intel::render::ResidentSceneDraw {
            mesh: &triangle.mesh,
            rgba: triangle.rgba,
            viewport_translation_px: [0.0, 0.0],
        })
        .collect::<Vec<_>>();
    let result = match crate::intel::render::
        render_resident_triangle_scene_frame_premultiplied_with_opaque_depth_direct_to_surface(
            &draws,
            Some(clear_rgba),
            destination,
            false,
        ) {
        Ok(result) => result,
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(GameError::Render(error));
        }
    };
    if result.completed_draws != result.requested_draws
        || result.completed_draws != resident.len()
        || result.rgba.is_some()
        || result.present_copy_performed
    {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(GameError::Render("helio-incomplete-direct-frame"));
    }
    let Some(release) = result.release_fence else {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(GameError::Render("helio-missing-release-fence"));
    };
    if let Err(error) = crate::ui4::publish_gpu_frame_buffer(lease, release) {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(error.into());
    }

    Ok(result)
}

fn publish_surface_window(surface: &mut GameSurface) -> Result<(), GameError> {
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((surface.width, surface.height));
    if surface.window.is_none() {
        surface.window = Some(crate::ui4::create_window(crate::ui4::WindowCreate {
            owner: OWNER,
            session: surface.session,
            frame: surface.frame,
            output,
            plane: crate::ui4::WindowPlane::Universal(PLANE_SLOT as u8),
            placement: crate::ui4::WindowPlacement {
                x: scanout_width.saturating_sub(surface.width.saturating_add(MARGIN)) as i32,
                y: MARGIN.min(scanout_height.saturating_sub(surface.height)) as i32,
                width: surface.width,
                height: surface.height,
                z: 78,
                opacity: u8::MAX,
                visible: true,
            },
            interaction: crate::ui4::WindowInteraction::APPLICATION,
        })?);
    }
    let window = surface.window.ok_or(GameError::InvalidFrame)?;
    crate::ui4::publish_window_frame(OWNER, window, crate::ui4::DamageRect::FULL)?;
    Ok(())
}

async fn render_publish(
    surface: &mut GameSurface,
    resident: &[ResidentTriangle],
    clear_rgba: [u8; 4],
) -> Result<crate::intel::render::ResidentSceneFrameResult, GameError> {
    let result =
        render_frame(surface.frame, surface.width, surface.height, resident, clear_rgba).await?;
    publish_surface_window(surface)?;
    Ok(result)
}

fn make_resident_batches(
    batches: &[trueos_helio_runtime::churn::Batch],
) -> Result<Vec<ResidentTriangle>, GameError> {
    let mut resident = Vec::with_capacity(batches.len());
    for batch in batches {
        let args = match batch.draw_indexed_indirect() {
            Ok(args) => args,
            Err(error) => {
                release_resident_scene(resident);
                return Err(GameError::Artifact(error));
            }
        };
        match crate::intel::render::create_resident_triangle_mesh(&batch.vertices, &batch.indices) {
            Ok(mesh) => {
                if let Err(error) =
                    crate::intel::render::update_resident_triangle_draw_indexed_indirect(
                        &mesh,
                        args.index_count,
                        args.instance_count,
                        args.first_index,
                        args.base_vertex,
                        args.first_instance,
                    )
                {
                    let _ = crate::intel::render::release_resident_triangle_mesh(&mesh);
                    release_resident_scene(resident);
                    return Err(GameError::Render(error));
                }
                resident.push(ResidentTriangle {
                    mesh,
                    rgba: batch.rgba,
                });
            }
            Err(error) => {
                release_resident_scene(resident);
                return Err(GameError::Render(error));
            }
        }
    }
    Ok(resident)
}

fn update_resident_batches(
    resident: &mut [ResidentTriangle],
    batches: &[trueos_helio_runtime::churn::Batch],
) -> Result<(), GameError> {
    if resident.len() < batches.len() {
        return Err(GameError::Render("helio-churn-batch-count"));
    }
    for (resident, batch) in resident.iter_mut().zip(batches) {
        crate::intel::render::update_resident_triangle_mesh(
            &resident.mesh,
            &batch.vertices,
            &batch.indices,
        )
        .map_err(GameError::Render)?;
        let args = batch.draw_indexed_indirect().map_err(GameError::Artifact)?;
        crate::intel::render::update_resident_triangle_draw_indexed_indirect(
            &resident.mesh,
            args.index_count,
            args.instance_count,
            args.first_index,
            args.base_vertex,
            args.first_instance,
        )
        .map_err(GameError::Render)?;
        resident.rgba = batch.rgba;
    }
    Ok(())
}

fn apply_churn_key_actions(
    engine: &mut trueos_helio_runtime::churn::Engine,
    window: crate::ui4::WindowId,
    events: &[crate::ui4::winit_input::Event],
) {
    use crate::ui4::winit_input::{ElementState, Event, KeyCode, PhysicalKey, WindowEvent};

    for event in events {
        let Event::WindowEvent {
            window_id,
            event:
                WindowEvent::KeyboardInput {
                    event:
                        crate::ui4::winit_input::KeyEvent {
                            physical_key: PhysicalKey::Code(key),
                            state: ElementState::Pressed,
                            ..
                        },
                },
            ..
        } = *event
        else {
            continue;
        };
        if window_id != window {
            continue;
        }
        match key {
            KeyCode::Equal | KeyCode::NumpadAdd => {
                engine.adjust_spawn_rate(1);
                crate::log_info!(
                    target: "helio";
                    "helio churn input spawn_rate={} source=ui4-winit-bridge\n",
                    engine.spawn_rate(),
                );
            }
            KeyCode::Minus | KeyCode::NumpadSubtract => {
                engine.adjust_spawn_rate(-1);
                crate::log_info!(
                    target: "helio";
                    "helio churn input spawn_rate={} source=ui4-winit-bridge\n",
                    engine.spawn_rate(),
                );
            }
            KeyCode::KeyC => {
                let collisions = engine.toggle_collisions();
                crate::log_info!(
                    target: "helio";
                    "helio churn input collisions={} action={} mode=bounded-deterministic-separation source=ui4-winit-bridge\n",
                    collisions,
                    if collisions { "burst" } else { "orbit-reset" },
                );
            }
            _ => {}
        }
    }
}

enum PortedSceneEngine {
    Battle(trueos_helio_runtime::battle::Engine),
    Pendulum(trueos_helio_runtime::pendulum_bigcloth::Engine),
}

impl PortedSceneEngine {
    fn decode(example_id: u8) -> Result<Self, trueos_helio_runtime::Error> {
        match example_id {
            3 => trueos_helio_runtime::battle::Spec::decode_artifact(GAME_ARTIFACT)
                .and_then(trueos_helio_runtime::battle::Engine::new)
                .map(Self::Battle),
            4 => trueos_helio_runtime::pendulum_bigcloth::Spec::decode_artifact(GAME_ARTIFACT)
                .and_then(trueos_helio_runtime::pendulum_bigcloth::Engine::new)
                .map(Self::Pendulum),
            _ => Err(trueos_helio_runtime::Error::Artifact),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Battle(_) => "shape-battle-royale",
            Self::Pendulum(_) => "pendulum-bigcloth",
        }
    }

    fn controls(&self) -> &'static str {
        match self {
            Self::Battle(_) => "WASD+Space+Shift,left-drag-look,+/-shape-count",
            Self::Pendulum(_) => "WASD+Space+Shift,left-drag-look,C-pause,R-reset",
        }
    }

    fn object_count(&self) -> usize {
        match self {
            Self::Battle(engine) => engine.shape_count(),
            Self::Pendulum(engine) => engine.segment_count(),
        }
    }

    fn camera(&self) -> trueos_helio_runtime::Camera {
        match self {
            Self::Battle(engine) => engine.camera(),
            Self::Pendulum(engine) => engine.camera(),
        }
    }

    fn set_camera(
        &mut self,
        camera: trueos_helio_runtime::Camera,
    ) -> Result<(), trueos_helio_runtime::Error> {
        match self {
            Self::Battle(engine) => engine.set_camera(camera),
            Self::Pendulum(engine) => engine.set_camera(camera),
        }
    }

    fn step(
        &mut self,
        aspect: f32,
    ) -> Result<&[trueos_helio_runtime::churn::Batch], trueos_helio_runtime::Error> {
        match self {
            Self::Battle(engine) => engine.step(aspect),
            Self::Pendulum(engine) => engine.step(aspect),
        }
    }

    fn batches(&self) -> &[trueos_helio_runtime::churn::Batch] {
        match self {
            Self::Battle(engine) => engine.batches(),
            Self::Pendulum(engine) => engine.batches(),
        }
    }

    fn apply_key_actions(
        &mut self,
        window: crate::ui4::WindowId,
        events: &[crate::ui4::winit_input::Event],
    ) {
        use crate::ui4::winit_input::{ElementState, Event, KeyCode, PhysicalKey, WindowEvent};

        for event in events {
            let Event::WindowEvent {
                window_id,
                event:
                    WindowEvent::KeyboardInput {
                        event:
                            crate::ui4::winit_input::KeyEvent {
                                physical_key: PhysicalKey::Code(key),
                                state: ElementState::Pressed,
                                ..
                            },
                    },
                ..
            } = *event
            else {
                continue;
            };
            if window_id != window {
                continue;
            }
            match self {
                Self::Battle(engine) => {
                    let delta = match key {
                        KeyCode::Equal | KeyCode::NumpadAdd => 1,
                        KeyCode::Minus | KeyCode::NumpadSubtract => -1,
                        _ => continue,
                    };
                    engine.adjust_shape_count(delta);
                    crate::log_info!(
                        target: "helio";
                        "helio battle input shape_count={} action=restart source=ui4-winit-bridge\n",
                        engine.shape_count(),
                    );
                }
                Self::Pendulum(engine) => match key {
                    KeyCode::KeyC => {
                        engine.toggle_enabled();
                        crate::log_info!(
                            target: "helio";
                            "helio pendulum input physics={} source=ui4-winit-bridge\n",
                            engine.enabled(),
                        );
                    }
                    KeyCode::KeyR => {
                        engine.reset();
                        crate::log_info!(
                            target: "helio";
                            "helio pendulum input action=reset source=ui4-winit-bridge\n",
                        );
                    }
                    _ => {}
                },
            }
        }
    }
}

async fn run_cube() -> Result<(), GameError> {
    let mut surface = create_surface()?;
    let initial_scene = match trueos_helio_runtime::decode_artifact_with_replay(
        GAME_ARTIFACT,
        surface.width as f32 / surface.height as f32,
        trueos_helio_runtime::Camera::helio_simple_graph(),
    ) {
        Ok(scene) => scene,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let clear_rgba = TRANSPARENT_CLEAR_RGBA;
    let triangle_count = initial_scene.triangles.len();
    let resident = match make_resident_scene(&initial_scene) {
        Ok(resident) => resident,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(error);
        }
    };
    let rendered = match render_publish(&mut surface, &resident, clear_rgba).await {
        Ok(rendered) => rendered,
        Err(error) => {
            release_resident_scene(resident);
            destroy_unpublished_surface(surface);
            return Err(error);
        }
    };
    publish_gpu_logger_sample(gpu_logger_sample(&rendered, 1, 0, 1, &resident, 0, 0));

    let window = surface.window.expect("published Helio cube window");
    let mut input = crate::ui4::winit_input::EventLoopInput::new(OWNER);
    if !input.register_window(window) {
        release_resident_scene(resident);
        destroy_unpublished_surface(surface);
        return Err(GameError::Render("helio-input-window-capacity"));
    }
    GAME_STATE.store(STATE_ONLINE | 1, Ordering::Release);
    *LAST_ERROR.lock() = None;
    crate::log_info!(
        target: "helio";
        "helio example=1 name=simple-cube online artifact_bytes={} normalized_triangles={} source_draw_indices={} draw_source={} frame={} window={} extent={}x{} plane={} background=transparent-rgba alpha=premultiplied resize=ui4-broker->render-native-or-half-scale-ring->atomic-frame-swap->direct-plane-1x-or-2x path=helioa-v1+render-ir-v1+artifact-replay-v1->uniform-color-subdraws->helio-indirect-v1->one-guc-schedule->ui4-triple-direct cpu_readback=0 cpu_frame_copy=0\n",
        GAME_ARTIFACT.len(),
        triangle_count,
        initial_scene.source_draw_indexed_indirect.index_count,
        initial_scene.draw_source.label(),
        surface.frame.raw(),
        window.raw(),
        surface.width,
        surface.height,
        PLANE_SLOT,
    );

    let mut retired_frames = Vec::new();
    let mut pending_resize: Option<PendingGameResize> = None;
    let mut frame_index = 1u64;
    loop {
        retire_frames(&mut retired_frames);
        let _ = input.poll();

        if pending_resize.is_none()
            && let Some((width, height)) = desired_resize(&surface)
        {
            match prepare_resize(&surface, width, height) {
                Ok(replacement) => {
                    let scene = match trueos_helio_runtime::decode_artifact_with_replay(
                        GAME_ARTIFACT,
                        width as f32 / height as f32,
                        trueos_helio_runtime::Camera::helio_simple_graph(),
                    ) {
                        Ok(scene) => scene,
                        Err(error) => {
                            let _ = crate::ui4::destroy_frame(replacement.frame);
                            release_resident_scene(resident);
                            destroy_unpublished_surface(surface);
                            return Err(GameError::Artifact(error));
                        }
                    };
                    if let Err(error) = update_resident_scene(&resident, &scene) {
                        let _ = crate::ui4::destroy_frame(replacement.frame);
                        release_resident_scene(resident);
                        destroy_unpublished_surface(surface);
                        return Err(error);
                    }
                    crate::log_info!(
                        target: "helio";
                        "helio resize prepared example=1 window={} old={}x{} new={}x{} replacement_frame={} action=render-before-broker-swap old_front=retain-surflive\n",
                        window.raw(),
                        surface.width,
                        surface.height,
                        width,
                        height,
                        replacement.frame.raw(),
                    );
                    pending_resize = Some(replacement);
                }
                Err(error) => {
                    crate::log_warn!(
                        target: "helio";
                        "helio resize deferred example=1 window={} old={}x{} requested={}x{} error={:?} action=retain-current-frame-and-reconcile\n",
                        window.raw(),
                        surface.width,
                        surface.height,
                        width,
                        height,
                        error,
                    );
                }
            }
        }

        let Some(replacement) = pending_resize else {
            Timer::after(Duration::from_millis(16)).await;
            continue;
        };
        let rendered = match render_frame(
            replacement.frame,
            replacement.width,
            replacement.height,
            &resident,
            clear_rgba,
        )
        .await
        {
            Ok(rendered) => rendered,
            Err(GameError::Frame(crate::ui4::FramePoolError::Busy))
            | Err(GameError::Render("helio-incomplete-direct-frame")) => {
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(error) => {
                let _ = crate::ui4::destroy_frame(replacement.frame);
                release_resident_scene(resident);
                destroy_unpublished_surface(surface);
                return Err(error);
            }
        };
        pending_resize = None;
        match commit_resize(&mut surface, replacement) {
            Ok(previous) => {
                retired_frames.push(previous);
                frame_index = frame_index.saturating_add(1);
                publish_gpu_logger_sample(gpu_logger_sample(
                    &rendered,
                    frame_index,
                    0,
                    1,
                    &resident,
                    0,
                    0,
                ));
                crate::log_info!(
                    target: "helio";
                    "helio resize committed example=1 window={} extent={}x{} frame={} action=broker-swap-after-first-guc-release old_release=surflive presentation=1:1-or-direct-plane-2x cpu_readback=0 cpu_frame_copy=0\n",
                    window.raw(),
                    surface.width,
                    surface.height,
                    surface.frame.raw(),
                );
            }
            Err(error) => {
                let _ = crate::ui4::destroy_frame(replacement.frame);
                crate::log_warn!(
                    target: "helio";
                    "helio resize commit rejected example=1 window={} requested={}x{} error={:?} action=old-front-restored\n",
                    window.raw(),
                    replacement.width,
                    replacement.height,
                    error,
                );
            }
        }
    }
}

async fn run_churn() -> Result<(), GameError> {
    let mut surface = create_surface()?;
    let initial_aspect = surface.width as f32 / surface.height as f32;
    let spec = match trueos_helio_runtime::churn::Spec::decode_artifact(GAME_ARTIFACT) {
        Ok(spec) => spec,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let clear_rgba = TRANSPARENT_CLEAR_RGBA;
    let mut engine = match trueos_helio_runtime::churn::Engine::new(spec) {
        Ok(engine) => engine,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    if let Err(error) = engine.step(initial_aspect) {
        destroy_unpublished_surface(surface);
        return Err(GameError::Artifact(error));
    }
    let mut resident = match make_resident_batches(engine.batches()) {
        Ok(resident) => resident,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(error);
        }
    };

    let mut input = crate::ui4::winit_input::EventLoopInput::new(OWNER);
    let mut fly_camera = FlyCamera::new(engine.camera());

    let mut first_frame = true;
    let mut frame_prepared = true;
    let mut frame_index = 0u64;
    let mut busy_retries = 0u64;
    let mut incomplete_retries = 0u64;
    let mut last_sample: Option<crate::spirit::gpu_logger::GpuLoggerSample> = None;
    let mut last_successful_publish: Option<Instant> = None;
    let mut retired_frames = Vec::new();
    let mut pending_resize: Option<PendingGameResize> = None;
    let result = loop {
        retire_frames(&mut retired_frames);
        if !first_frame && !frame_prepared {
            let Some(window) = surface.window else {
                break Err(GameError::InvalidFrame);
            };
            let events = input.poll();
            apply_churn_key_actions(&mut engine, window, &events);
            let camera_changed = fly_camera.apply_events(&events)
                | fly_camera.apply_held_keys(&input, window, 0.033);
            if pending_resize.is_none()
                && let Some((width, height)) = desired_resize(&surface)
            {
                match prepare_resize(&surface, width, height) {
                    Ok(replacement) => {
                        crate::log_info!(
                            target: "helio";
                            "helio resize prepared example=2 window={} old={}x{} new={}x{} replacement_frame={} action=render-before-broker-swap old_front=retain-surflive\n",
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            replacement.frame.raw(),
                        );
                        pending_resize = Some(replacement);
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "helio";
                            "helio resize deferred example=2 window={} old={}x{} requested={}x{} error={:?} action=retain-current-frame-and-reconcile\n",
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            error,
                        );
                    }
                }
            }
            let (render_width, render_height) = pending_resize
                .map_or((surface.width, surface.height), |replacement| {
                    (replacement.width, replacement.height)
                });
            let aspect = render_width as f32 / render_height as f32;
            if camera_changed {
                if let Err(error) = engine.set_camera(fly_camera.camera) {
                    break Err(GameError::Artifact(error));
                }
            }
            if let Err(error) = engine.step(aspect) {
                break Err(GameError::Artifact(error));
            }
            if let Err(error) = update_resident_batches(&mut resident, engine.batches()) {
                break Err(error);
            }
            frame_prepared = true;
        }
        let (render_frame_handle, render_width, render_height) = pending_resize
            .map_or((surface.frame, surface.width, surface.height), |replacement| {
                (replacement.frame, replacement.width, replacement.height)
            });
        match render_frame(render_frame_handle, render_width, render_height, &resident, clear_rgba)
            .await
        {
            Ok(rendered) => {
                if let Some(replacement) = pending_resize.take() {
                    match commit_resize(&mut surface, replacement) {
                        Ok(previous) => {
                            retired_frames.push(previous);
                            crate::log_info!(
                                target: "helio";
                                "helio resize committed example=2 window={} extent={}x{} frame={} action=broker-swap-after-first-guc-release old_release=surflive presentation=1:1-or-direct-plane-2x cpu_readback=0 cpu_frame_copy=0\n",
                                surface.window.map_or(0, crate::ui4::WindowId::raw),
                                surface.width,
                                surface.height,
                                surface.frame.raw(),
                            );
                        }
                        Err(error) => {
                            let _ = crate::ui4::destroy_frame(replacement.frame);
                            frame_prepared = false;
                            crate::log_warn!(
                                target: "helio";
                                "helio resize commit rejected example=2 requested={}x{} error={:?} action=old-front-restored-and-reproject\n",
                                replacement.width,
                                replacement.height,
                                error,
                            );
                            Timer::after(Duration::from_millis(16)).await;
                            continue;
                        }
                    }
                } else if let Err(error) = publish_surface_window(&mut surface) {
                    break Err(error);
                }
                frame_prepared = false;
                frame_index = frame_index.saturating_add(1);
                let published_at = Instant::now();
                let cadence_us = last_successful_publish
                    .map(|previous| published_at.saturating_duration_since(previous).as_micros())
                    .unwrap_or(0);
                last_successful_publish = Some(published_at);
                let sample = gpu_logger_sample(
                    &rendered,
                    frame_index,
                    cadence_us,
                    engine.active_objects(),
                    &resident,
                    busy_retries,
                    incomplete_retries,
                );
                publish_gpu_logger_sample(sample);
                last_sample = Some(sample);
            }
            Err(GameError::Frame(crate::ui4::FramePoolError::Busy)) => {
                // Triple-buffer ownership can legitimately lag one producer
                // tick. Keep these exact prepared bytes and retry after UI4
                // has had a display-release opportunity.
                busy_retries = busy_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.busy_retries = busy_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(GameError::Render("helio-incomplete-direct-frame")) => {
                // Keep the last complete UI4 buffer visible and retry these
                // exact resident bytes. A bounded GuC poll miss must not
                // advance the game or rewrite late-read geometry.
                incomplete_retries = incomplete_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.incomplete_retries = incomplete_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(error) => break Err(error),
        }
        if first_frame {
            GAME_STATE.store(STATE_ONLINE | 2, Ordering::Release);
            *LAST_ERROR.lock() = None;
            let window = surface.window.expect("published Helio churn window");
            if !input.register_window(window) {
                break Err(GameError::Render("helio-input-window-capacity"));
            }
            crate::log_info!(
                target: "helio";
                "helio example=2 name=churn-benchmark online artifact_bytes={} active_objects={} resident_batches={} frame={} window={} extent={}x{} plane={} background=transparent-rgba floor=none alpha=premultiplied animation_rate=1.5x lighting=churn-light-v1+24-face-batches lights=2 ambient=hemisphere controls=WASD+Space+Shift,left-drag-look,+/-,C-collision-burst producer_pacing=relative-post-work delay_ms=33 nominal_ceiling_fps=30 input=ui4-owner-broker->winit-shaped-events resize=ui4-broker->render-native-or-half-scale-ring->atomic-frame-swap->direct-plane-1x-or-2x path=helioa-v1+churn-v1+flat-light-v1->resident-batches->helio-indirect-v1->one-guc-schedule->ui4-triple-direct cpu_readback=0 cpu_frame_copy=0\n",
                GAME_ARTIFACT.len(),
                engine.active_objects(),
                resident.len(),
                surface.frame.raw(),
                window.raw(),
                surface.width,
                surface.height,
                PLANE_SLOT,
            );
            first_frame = false;
        }
        Timer::after(Duration::from_millis(33)).await;
    };
    retire_frames(&mut retired_frames);
    release_resident_scene(resident);
    destroy_unpublished_surface(surface);
    result
}

async fn run_ported_scene(example_id: u8) -> Result<(), GameError> {
    let mut surface = create_surface()?;
    let initial_aspect = surface.width as f32 / surface.height as f32;
    let mut engine = match PortedSceneEngine::decode(example_id) {
        Ok(engine) => engine,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    if let Err(error) = engine.step(initial_aspect) {
        destroy_unpublished_surface(surface);
        return Err(GameError::Artifact(error));
    }
    let mut resident = match make_resident_batches(engine.batches()) {
        Ok(resident) => resident,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(error);
        }
    };

    let mut input = crate::ui4::winit_input::EventLoopInput::new(OWNER);
    let mut fly_camera = FlyCamera::new(engine.camera());
    let mut first_frame = true;
    let mut frame_prepared = true;
    let mut frame_index = 0u64;
    let mut busy_retries = 0u64;
    let mut incomplete_retries = 0u64;
    let mut last_sample: Option<crate::spirit::gpu_logger::GpuLoggerSample> = None;
    let mut last_successful_publish: Option<Instant> = None;
    let mut retired_frames = Vec::new();
    let mut pending_resize: Option<PendingGameResize> = None;
    let result = loop {
        retire_frames(&mut retired_frames);
        if !first_frame && !frame_prepared {
            let Some(window) = surface.window else {
                break Err(GameError::InvalidFrame);
            };
            let events = input.poll();
            engine.apply_key_actions(window, &events);
            let camera_changed = fly_camera.apply_events(&events)
                | fly_camera.apply_held_keys(&input, window, 0.033);
            if pending_resize.is_none()
                && let Some((width, height)) = desired_resize(&surface)
            {
                match prepare_resize(&surface, width, height) {
                    Ok(replacement) => {
                        crate::log_info!(
                            target: "helio";
                            "helio resize prepared example={} window={} old={}x{} new={}x{} replacement_frame={} action=render-before-broker-swap old_front=retain-surflive\n",
                            example_id,
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            replacement.frame.raw(),
                        );
                        pending_resize = Some(replacement);
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "helio";
                            "helio resize deferred example={} window={} old={}x{} requested={}x{} error={:?} action=retain-current-frame-and-reconcile\n",
                            example_id,
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            error,
                        );
                    }
                }
            }
            let (render_width, render_height) = pending_resize
                .map_or((surface.width, surface.height), |replacement| {
                    (replacement.width, replacement.height)
                });
            if camera_changed && let Err(error) = engine.set_camera(fly_camera.camera) {
                break Err(GameError::Artifact(error));
            }
            if let Err(error) = engine.step(render_width as f32 / render_height as f32) {
                break Err(GameError::Artifact(error));
            }
            if let Err(error) = update_resident_batches(&mut resident, engine.batches()) {
                break Err(error);
            }
            frame_prepared = true;
        }

        let (render_frame_handle, render_width, render_height) = pending_resize
            .map_or((surface.frame, surface.width, surface.height), |replacement| {
                (replacement.frame, replacement.width, replacement.height)
            });
        match render_frame(
            render_frame_handle,
            render_width,
            render_height,
            &resident,
            TRANSPARENT_CLEAR_RGBA,
        )
        .await
        {
            Ok(rendered) => {
                if let Some(replacement) = pending_resize.take() {
                    match commit_resize(&mut surface, replacement) {
                        Ok(previous) => {
                            retired_frames.push(previous);
                            crate::log_info!(
                                target: "helio";
                                "helio resize committed example={} window={} extent={}x{} frame={} action=broker-swap-after-first-guc-release old_release=surflive presentation=1:1-or-direct-plane-2x cpu_readback=0 cpu_frame_copy=0\n",
                                example_id,
                                surface.window.map_or(0, crate::ui4::WindowId::raw),
                                surface.width,
                                surface.height,
                                surface.frame.raw(),
                            );
                        }
                        Err(error) => {
                            let _ = crate::ui4::destroy_frame(replacement.frame);
                            frame_prepared = false;
                            crate::log_warn!(
                                target: "helio";
                                "helio resize commit rejected example={} requested={}x{} error={:?} action=old-front-restored-and-reproject\n",
                                example_id,
                                replacement.width,
                                replacement.height,
                                error,
                            );
                            Timer::after(Duration::from_millis(16)).await;
                            continue;
                        }
                    }
                } else if let Err(error) = publish_surface_window(&mut surface) {
                    break Err(error);
                }
                frame_prepared = false;
                frame_index = frame_index.saturating_add(1);
                let published_at = Instant::now();
                let cadence_us = last_successful_publish
                    .map(|previous| published_at.saturating_duration_since(previous).as_micros())
                    .unwrap_or(0);
                last_successful_publish = Some(published_at);
                let sample = gpu_logger_sample(
                    &rendered,
                    frame_index,
                    cadence_us,
                    engine.object_count(),
                    &resident,
                    busy_retries,
                    incomplete_retries,
                );
                publish_gpu_logger_sample(sample);
                last_sample = Some(sample);
            }
            Err(GameError::Frame(crate::ui4::FramePoolError::Busy)) => {
                busy_retries = busy_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.busy_retries = busy_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(GameError::Render("helio-incomplete-direct-frame")) => {
                incomplete_retries = incomplete_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.incomplete_retries = incomplete_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(error) => break Err(error),
        }
        if first_frame {
            GAME_STATE.store(STATE_ONLINE | example_id, Ordering::Release);
            *LAST_ERROR.lock() = None;
            let window = surface.window.expect("published ported Helio window");
            if !input.register_window(window) {
                break Err(GameError::Render("helio-input-window-capacity"));
            }
            crate::log_info!(
                target: "helio";
                "helio example={} name={} online artifact_bytes={} objects={} resident_batches={} frame={} window={} extent={}x{} plane={} background=transparent-rgba alpha=premultiplied input=ui4-owner-broker->winit-shaped-events controls={} resize=ui4-broker->render-native-or-half-scale-ring->atomic-frame-swap->direct-plane-1x-or-2x path=helioa-v1+scene-v1->resident-batches->helio-indirect-v1->one-guc-schedule->ui4-triple-direct cpu_readback=0 cpu_frame_copy=0\n",
                example_id,
                engine.name(),
                GAME_ARTIFACT.len(),
                engine.object_count(),
                resident.len(),
                surface.frame.raw(),
                window.raw(),
                surface.width,
                surface.height,
                PLANE_SLOT,
                engine.controls(),
            );
            first_frame = false;
        }
        Timer::after(Duration::from_millis(33)).await;
    };
    if let Some(replacement) = pending_resize {
        let _ = crate::ui4::destroy_frame(replacement.frame);
    }
    retire_frames(&mut retired_frames);
    release_resident_scene(resident);
    destroy_unpublished_surface(surface);
    result
}

/// Waits for a numbered Shell2/Blueprint request and runs the selected scene
/// from the one embedded, build-produced `.helio` artifact.
#[embassy_executor::task]
pub async fn helio_game_service_task() {
    let mut last_error = None;
    loop {
        let GameState::Requested(example_id) = game_state() else {
            Timer::after(Duration::from_millis(50)).await;
            continue;
        };
        GAME_STATE.store(STATE_STARTING | example_id, Ordering::Release);
        let result = match example_id {
            1 => run_cube().await,
            2 => run_churn().await,
            3 | 4 => run_ported_scene(example_id).await,
            _ => Err(GameError::Render("helio-example-reserved")),
        };
        match result {
            Ok(()) => {}
            Err(error) => {
                GAME_STATE.store(STATE_REQUESTED | example_id, Ordering::Release);
                *LAST_ERROR.lock() = Some(error.label());
                if last_error != Some(core::mem::discriminant(&error)) {
                    crate::log_warn!(
                        target: "helio";
                        "helio example={} start pending error={:?} action=retry\n",
                        example_id,
                        error,
                    );
                    last_error = Some(core::mem::discriminant(&error));
                }
                Timer::after(Duration::from_millis(250)).await;
            }
        }
    }
}
