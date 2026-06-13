use alloc::format;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::intel::LiveOverlayRect;
use crate::intel::gpgpu::{GpgpuCanvas3dUi2TextureFrame, GpgpuRgba8Surface};
use crate::intel::types::Rgba8;
use crate::shell2::{
    CommandSessionInputResult, MatrixTarget, ShellBackend2, matrix_target_for_backend,
    matrix_target_interrupted, print_matrix_target_line, print_shell_line,
    set_matrix_target_active, switch_matrix_target_slot,
};

const GPU_CANVAS_SLOT: &str = "Gid";
const GPU_CANVAS_FRAME_MS: u64 = 33;
const GPU_CANVAS_OVERLAY_WIDTH: u32 = 192;
const GPU_CANVAS_OVERLAY_HEIGHT: u32 = 192;
const GPU_CANVAS_FALLBACK_X: u32 = 48;
const GPU_CANVAS_FALLBACK_Y: u32 = 48;
const GPU_CANVAS_CUBE_HALF_Q16: i32 = 32_768;

#[derive(Copy, Clone)]
enum GpuCanvas3dScene {
    Cube,
    Ico,
}

impl GpuCanvas3dScene {
    fn name(self) -> &'static str {
        match self {
            Self::Cube => "cube",
            Self::Ico => "ico",
        }
    }

    fn failure_hint(self) -> &'static str {
        match self {
            Self::Cube => "plane worklist artifact",
            Self::Ico => "transform/project artifacts",
        }
    }
}

#[derive(Clone)]
struct GpuCanvasSessionState {
    id: u64,
    cancel_requested: bool,
}

#[derive(Copy, Clone)]
struct GpuCanvasBuffer {
    phys: u64,
    virt: *mut u8,
    bytes: usize,
    pitch_bytes: u32,
}

unsafe impl Send for GpuCanvasBuffer {}

static GPU_CANVAS_SESSIONS: spin::Mutex<Vec<GpuCanvasSessionState>> = spin::Mutex::new(Vec::new());
static GPU_CANVAS_BUFFER: spin::Mutex<Option<GpuCanvasBuffer>> = spin::Mutex::new(None);
static NEXT_GPU_CANVAS_SESSION_ID: AtomicU64 = AtomicU64::new(1);

fn session_start() -> u64 {
    let id = NEXT_GPU_CANVAS_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    GPU_CANVAS_SESSIONS.lock().push(GpuCanvasSessionState {
        id,
        cancel_requested: false,
    });
    id
}

fn session_finish(session_id: u64) {
    let mut sessions = GPU_CANVAS_SESSIONS.lock();
    if let Some(index) = sessions.iter().position(|session| session.id == session_id) {
        let _ = sessions.remove(index);
    }
}

fn session_alive(session_id: u64) -> bool {
    GPU_CANVAS_SESSIONS
        .lock()
        .iter()
        .any(|session| session.id == session_id)
}

fn cancel_requested(session_id: u64) -> bool {
    GPU_CANVAS_SESSIONS
        .lock()
        .iter()
        .find(|session| session.id == session_id)
        .map(|session| session.cancel_requested)
        .unwrap_or(false)
}

fn cancel_or_interrupted(session_id: u64, target: &MatrixTarget) -> bool {
    cancel_requested(session_id) || matrix_target_interrupted(target)
}

fn session_running() -> bool {
    GPU_CANVAS_SESSIONS
        .lock()
        .iter()
        .any(|session| !session.cancel_requested)
}

fn overlay_rects_intersect(a: LiveOverlayRect, b: LiveOverlayRect) -> bool {
    let ax1 = a.x.saturating_add(a.width);
    let ay1 = a.y.saturating_add(a.height);
    let bx1 = b.x.saturating_add(b.width);
    let by1 = b.y.saturating_add(b.height);
    a.x < bx1 && b.x < ax1 && a.y < by1 && b.y < ay1
}

pub(crate) fn live_overlay_preserve_rect(rects: &[LiveOverlayRect]) -> Option<LiveOverlayRect> {
    if !session_running() {
        return None;
    }
    let canvas = current_canvas_rect();
    if rects
        .iter()
        .all(|rect| !overlay_rects_intersect(canvas, *rect))
    {
        return Some(canvas);
    }

    // The cursor normally intersects the cursor-follow canvas. Preserve the
    // canvas underneath, then let the live overlay draw cursor/menu rects over it.
    Some(canvas)
}

pub(crate) fn current_canvas_rect() -> LiveOverlayRect {
    let (viewport_width, viewport_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((0, 0));
    let (x, y) = canvas_origin_from_cursor(viewport_width, viewport_height)
        .unwrap_or((GPU_CANVAS_FALLBACK_X, GPU_CANVAS_FALLBACK_Y));
    LiveOverlayRect::new(
        x,
        y,
        GPU_CANVAS_OVERLAY_WIDTH,
        GPU_CANVAS_OVERLAY_HEIGHT,
        Rgba8::new(0, 0, 0, 0),
    )
}

fn canvas_origin_from_cursor(viewport_width: u32, viewport_height: u32) -> Option<(u32, u32)> {
    if viewport_width == 0 || viewport_height == 0 {
        return None;
    }
    let cursor = crate::ui3::ui3_hid::preferred_cursor_snapshot(viewport_width, viewport_height)?;
    Some((
        centered_origin(cursor.x_px, GPU_CANVAS_OVERLAY_WIDTH, viewport_width),
        centered_origin(cursor.y_px, GPU_CANVAS_OVERLAY_HEIGHT, viewport_height),
    ))
}

fn centered_origin(center: u32, size: u32, extent: u32) -> u32 {
    if extent <= size {
        return 0;
    }
    center.saturating_sub(size / 2).min(extent - size)
}

fn canvas_buffer() -> Option<GpuCanvasBuffer> {
    {
        let state = GPU_CANVAS_BUFFER.lock();
        if let Some(buffer) = *state {
            return Some(buffer);
        }
    }

    let pitch_bytes = GPU_CANVAS_OVERLAY_WIDTH.checked_mul(core::mem::size_of::<u32>() as u32)?;
    let bytes = (pitch_bytes as usize).checked_mul(GPU_CANVAS_OVERLAY_HEIGHT as usize)?;
    let (phys, virt) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    crate::intel::dma_flush(virt, bytes);

    let buffer = GpuCanvasBuffer {
        phys,
        virt,
        bytes,
        pitch_bytes,
    };
    *GPU_CANVAS_BUFFER.lock() = Some(buffer);
    Some(buffer)
}

pub(crate) fn submit_canvas3d_cube(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
) -> Option<u64> {
    submit_canvas3d_scene(spawner, io, GpuCanvas3dScene::Cube)
}

pub(crate) fn submit_canvas3d_ico(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
) -> Option<u64> {
    submit_canvas3d_scene(spawner, io, GpuCanvas3dScene::Ico)
}

fn submit_canvas3d_scene(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    scene: GpuCanvas3dScene,
) -> Option<u64> {
    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, GPU_CANVAS_SLOT);
    let session_id = session_start();

    print_matrix_target_line(
        &target,
        format!(
            "ui3 canvas: starting canvas3d {} worker cadence_ms={}",
            scene.name(),
            GPU_CANVAS_FRAME_MS
        )
        .as_str(),
    );
    set_matrix_target_active(&target, true);

    let spawn_token = match ui3_canvas_worker_task(target.clone(), session_id, scene) {
        Ok(token) => token,
        Err(_) => {
            session_finish(session_id);
            set_matrix_target_active(&target, false);
            print_shell_line(io, "gpgpu canvas3d: worker spawn failed");
            return None;
        }
    };
    spawner.spawn(spawn_token);

    print_matrix_target_line(&target, "ui3 canvas: send `q` or `quit` in §Gid to stop");
    Some(session_id)
}

pub(crate) fn handle_session_input(
    session_id: u64,
    target: &MatrixTarget,
    submitted: &str,
) -> CommandSessionInputResult {
    if !session_alive(session_id) {
        return CommandSessionInputResult::CompleteIdle;
    }

    let cmd = submitted.trim();
    if cmd.is_empty() {
        return CommandSessionInputResult::KeepRunning;
    }

    if cmd.eq_ignore_ascii_case("q") || cmd.eq_ignore_ascii_case("quit") {
        let mut sessions = GPU_CANVAS_SESSIONS.lock();
        if let Some(session) = sessions.iter_mut().find(|session| session.id == session_id) {
            if !session.cancel_requested {
                session.cancel_requested = true;
                print_matrix_target_line(target, "ui3 canvas: stop requested");
            } else {
                print_matrix_target_line(target, "ui3 canvas: stop already requested");
            }
        }
        return CommandSessionInputResult::KeepRunning;
    }

    print_matrix_target_line(target, "ui3 canvas: running; send `q` or `quit` to stop");
    CommandSessionInputResult::KeepRunning
}

#[embassy_executor::task(pool_size = 5)]
async fn ui3_canvas_worker_task(target: MatrixTarget, session_id: u64, scene: GpuCanvas3dScene) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let mut frame = 0u32;
        loop {
            if cancel_or_interrupted(session_id, &task_target) {
                print_matrix_target_line(&task_target, "ui3 canvas: stopped before frame");
                break;
            }

            let Some(buffer) = canvas_buffer() else {
                print_matrix_target_line(&task_target, "ui3 canvas: no buffer");
                break;
            };
            let Some(surface) = GpgpuRgba8Surface::new(
                buffer.phys,
                crate::intel::GPU_VA_DISPLAY_UI3_CANVAS_BASE,
                buffer.bytes,
                GPU_CANVAS_OVERLAY_WIDTH,
                GPU_CANVAS_OVERLAY_HEIGHT,
                buffer.pitch_bytes,
            ) else {
                print_matrix_target_line(&task_target, "ui3 canvas: bad buffer surface");
                break;
            };
            unsafe {
                core::ptr::write_bytes(buffer.virt, 0, buffer.bytes);
            }
            crate::intel::dma_flush(buffer.virt, buffer.bytes);

            match render_canvas3d_frame(scene, frame, buffer, surface) {
                Some(result) => {
                    let canvas = current_canvas_rect();
                    let presented = crate::intel::present_ui3_canvas_rgba(
                        canvas,
                        buffer.virt,
                        buffer.pitch_bytes as usize,
                        "ui3-canvas",
                    ) as u32;
                    let avg_submit_ms = if result.submitted == 0 {
                        0
                    } else {
                        result.total_submit_ms / u64::from(result.submitted)
                    };
                    if frame < 3 || frame.is_multiple_of(10) {
                        print_matrix_target_line(
                            &task_target,
                            format!(
                                "ui3 canvas: frame={} scene={} ok={} submitted={} presented={} elapsed_ms={} avg_submit_ms={} visible={} angle={} surface={}x{} canvas={}x{} dst={},{}",
                                frame,
                                scene.name(),
                                result.ok as u8,
                                result.submitted,
                                presented,
                                result.elapsed_ms,
                                avg_submit_ms,
                                result.visible_points,
                                result.last_angle_deg,
                                result.primary_width,
                                result.primary_height,
                                canvas.width,
                                canvas.height,
                                canvas.x,
                                canvas.y,
                            )
                            .as_str(),
                        )
                    }
                    frame = frame.wrapping_add(1);
                }
                None => {
                    print_matrix_target_line(
                        &task_target,
                        format!(
                            "ui3 canvas: frame failed (check primary surface, iGPU claim, and {})",
                            scene.failure_hint()
                        )
                        .as_str(),
                    );
                }
            }

            if cancel_or_interrupted(session_id, &task_target) {
                print_matrix_target_line(&task_target, "ui3 canvas: stopped");
                break;
            }
            Timer::after(EmbassyDuration::from_millis(GPU_CANVAS_FRAME_MS)).await;
        }
    }
    .await;

    session_finish(session_id);
    set_matrix_target_active(&target, false);
}

fn render_canvas3d_frame(
    scene: GpuCanvas3dScene,
    frame: u32,
    buffer: GpuCanvasBuffer,
    surface: GpgpuRgba8Surface,
) -> Option<crate::intel::gpgpu::GpgpuShellCube20ProjectResult> {
    match scene {
        GpuCanvas3dScene::Cube => crate::intel::gpgpu::shell_cube6_plane_project_surface_frame(
            frame,
            GPU_CANVAS_CUBE_HALF_Q16,
            surface,
        ),
        GpuCanvas3dScene::Ico => render_canvas3d_ico_frame(frame, buffer),
    }
}

fn render_canvas3d_ico_frame(
    frame: u32,
    buffer: GpuCanvasBuffer,
) -> Option<crate::intel::gpgpu::GpgpuShellCube20ProjectResult> {
    let texture = crate::intel::gpgpu::canvas3d_ico_project_texture_frame(
        frame,
        GPU_CANVAS_OVERLAY_WIDTH,
        GPU_CANVAS_OVERLAY_HEIGHT,
    )?;
    copy_texture_rgba_to_buffer(&texture, buffer)?;
    Some(texture.result)
}

fn copy_texture_rgba_to_buffer(
    texture: &GpgpuCanvas3dUi2TextureFrame,
    buffer: GpuCanvasBuffer,
) -> Option<()> {
    if texture.width != GPU_CANVAS_OVERLAY_WIDTH || texture.height != GPU_CANVAS_OVERLAY_HEIGHT {
        return None;
    }
    let row_pixels = buffer
        .pitch_bytes
        .checked_div(core::mem::size_of::<u32>() as u32)? as usize;
    if row_pixels < texture.width as usize {
        return None;
    }

    let src_row_bytes = (texture.width as usize).checked_mul(core::mem::size_of::<u32>())?;
    let expected_bytes = src_row_bytes.checked_mul(texture.height as usize)?;
    if texture.rgba.len() < expected_bytes {
        return None;
    }

    let dst = buffer.virt.cast::<u32>();
    for y in 0..texture.height as usize {
        let src_row = y.checked_mul(src_row_bytes)?;
        let dst_row = y.checked_mul(row_pixels)?;
        for x in 0..texture.width as usize {
            let src = src_row + x * 4;
            let pixel = pack_rgba_bytes_as_bgra_u32(
                texture.rgba[src],
                texture.rgba[src + 1],
                texture.rgba[src + 2],
                texture.rgba[src + 3],
            );
            unsafe {
                core::ptr::write_volatile(dst.add(dst_row + x), pixel);
            }
        }
    }
    crate::intel::dma_flush(buffer.virt, buffer.bytes);
    Some(())
}

fn pack_rgba_bytes_as_bgra_u32(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from(a) << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}
