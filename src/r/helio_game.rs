//! On-demand presentation of the build-produced Helio game artifact.
//!
//! Artifact parsing and CPU-side dynamic-camera lowering are reusable and
//! scene-independent. This module owns only TRUEOS residency, its existing
//! one-GuC Render submission, and UI4 publication.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};
use embassy_time::{Duration, Timer};

const GAME_ARTIFACT: &[u8] = include_bytes!("../../assets/helio/simple-cube.trueos.intel.helio");
const OWNER: crate::ui4::WindowOwner = crate::ui4::WindowOwner::KernelApp(8);
const PLANE_SLOT: usize = crate::ui4::RGB_OVERLAY_PLANE_SLOT_2;
const MARGIN: u32 = 48;

const STATE_IDLE: u8 = 0;
const STATE_REQUESTED: u8 = 1;
const STATE_STARTING: u8 = 2;
const STATE_ONLINE: u8 = 3;

static GAME_STATE: AtomicU8 = AtomicU8::new(STATE_IDLE);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GameState {
    Idle,
    Requested,
    Starting,
    Online,
}

impl GameState {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Requested => "requested",
            Self::Starting => "starting",
            Self::Online => "online",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LaunchRequest {
    Queued,
    AlreadyRequested,
    AlreadyStarting,
    AlreadyOnline,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GameStatus {
    pub(crate) state: GameState,
    pub(crate) artifact_bytes: usize,
}

fn game_state() -> GameState {
    match GAME_STATE.load(Ordering::Acquire) {
        STATE_REQUESTED => GameState::Requested,
        STATE_STARTING => GameState::Starting,
        STATE_ONLINE => GameState::Online,
        _ => GameState::Idle,
    }
}

/// Queue the embedded game for the already-resident AP1/UI runtime service.
pub(crate) fn request_launch() -> LaunchRequest {
    match GAME_STATE.compare_exchange(
        STATE_IDLE,
        STATE_REQUESTED,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => LaunchRequest::Queued,
        Err(STATE_REQUESTED) => LaunchRequest::AlreadyRequested,
        Err(STATE_STARTING) => LaunchRequest::AlreadyStarting,
        Err(STATE_ONLINE) => LaunchRequest::AlreadyOnline,
        Err(_) => LaunchRequest::AlreadyRequested,
    }
}

pub(crate) fn status() -> GameStatus {
    GameStatus {
        state: game_state(),
        artifact_bytes: GAME_ARTIFACT.len(),
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

#[derive(Copy, Clone)]
struct GameSurface {
    session: crate::ui4::WindowSessionId,
    frame: crate::ui4::FrameHandle,
    width: u32,
    height: u32,
}

fn create_surface() -> Result<GameSurface, GameError> {
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
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
    let frame = crate::ui4::create_frame(crate::ui4::FrameSpec {
        output,
        content: crate::ui4::FrameContent::RenderScene3d,
        cadence: crate::ui4::FrameCadence::Streaming,
        buffering: crate::ui4::FrameBuffering::Triple,
        format: crate::ui4::ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(crate::ui4::PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
    })?;
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
        width,
        height,
    })
}

fn make_resident_scene(
    scene: &trueos_helio_runtime::Scene,
) -> Result<Vec<ResidentTriangle>, GameError> {
    let mut resident = Vec::with_capacity(scene.triangles.len());
    for triangle in &scene.triangles {
        match crate::intel::render::create_resident_triangle_mesh(&triangle.vertices, &[0, 1, 2]) {
            Ok(mesh) => resident.push(ResidentTriangle {
                mesh,
                rgba: triangle.rgba,
            }),
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

fn release_resident_scene(resident: Vec<ResidentTriangle>) {
    for triangle in resident {
        if !crate::intel::render::release_resident_triangle_mesh(&triangle.mesh) {
            core::mem::forget(triangle);
        }
    }
}

fn destroy_unpublished_surface(surface: GameSurface) {
    let _ = crate::ui4::finish_window_session(OWNER, surface.session);
    let _ = crate::ui4::destroy_frame(surface.frame);
}

fn render_publish_once(
    surface: GameSurface,
    resident: &[ResidentTriangle],
    clear_rgba: [u8; 4],
) -> Result<(), GameError> {
    let lease = crate::ui4::acquire_frame_buffer(surface.frame)?;
    let destination = match crate::ui4::gpgpu_rgba_surface(lease) {
        Ok(destination)
            if destination.width == surface.width
                && destination.height == surface.height
                && destination.pitch_bytes >= surface.width.saturating_mul(4) =>
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

    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((surface.width, surface.height));
    let window = crate::ui4::create_window(crate::ui4::WindowCreate {
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
    })?;
    crate::ui4::publish_window_frame(OWNER, window, crate::ui4::DamageRect::FULL)?;
    crate::log_info!(
        target: "helio";
        "helio game online artifact_bytes={} normalized_triangles={} frame={} window={} extent={}x{} plane={} path=helioa-v1+render-ir-v1->resident-triangles->one-guc-render->ui4-triple-direct cpu_readback=0 cpu_frame_copy=0\n",
        GAME_ARTIFACT.len(),
        resident.len(),
        surface.frame.raw(),
        window.raw(),
        surface.width,
        surface.height,
        PLANE_SLOT,
    );
    Ok(())
}

/// Waits for the Shell2/Blueprint launch request, loads the build-produced
/// `.helio`, resolves its two dynamic slots, uploads its normalized draw once,
/// and publishes it as a UI4 RenderScene3d window.
#[embassy_executor::task]
pub async fn helio_game_service_task() {
    let mut last_error = None;
    loop {
        if game_state() != GameState::Requested {
            Timer::after(Duration::from_millis(50)).await;
            continue;
        }
        GAME_STATE.store(STATE_STARTING, Ordering::Release);
        let result = (|| {
            let surface = create_surface()?;
            let scene = match trueos_helio_runtime::decode_artifact(
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
            let resident = match make_resident_scene(&scene) {
                Ok(resident) => resident,
                Err(error) => {
                    destroy_unpublished_surface(surface);
                    return Err(error);
                }
            };
            match render_publish_once(surface, &resident, scene.clear_rgba) {
                Ok(()) => Ok(resident),
                Err(error) => {
                    release_resident_scene(resident);
                    destroy_unpublished_surface(surface);
                    Err(error)
                }
            }
        })();
        match result {
            Ok(resident) => {
                GAME_STATE.store(STATE_ONLINE, Ordering::Release);
                // Window, frame ring, and resident Render mappings intentionally
                // remain owned by this static service for the life of the OS.
                core::mem::forget(resident);
                core::future::pending::<()>().await;
            }
            Err(error) => {
                GAME_STATE.store(STATE_REQUESTED, Ordering::Release);
                if last_error != Some(core::mem::discriminant(&error)) {
                    crate::log_warn!(
                        target: "helio";
                        "helio game start pending error={:?} action=retry\n",
                        error,
                    );
                    last_error = Some(core::mem::discriminant(&error));
                }
                Timer::after(Duration::from_millis(250)).await;
            }
        }
    }
}
