use alloc::format;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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
const GPU_CANVAS_INITIAL_DIM: u32 = 192;
const GPU_CANVAS_MIN_DIM: u32 = 192;
const GPU_CANVAS_MAX_DIM: u32 = 1440;
const GPU_CANVAS_CUBE_SAFE_MAX_DIM: u32 = 576;
const GPU_CANVAS_WHEEL_STEP_PX: i32 = 64;
const GPU_CANVAS_FALLBACK_X: u32 = 48;
const GPU_CANVAS_FALLBACK_Y: u32 = 48;
const GPU_CANVAS_CURSOR_EVENT_BATCH_CAP: usize = 64;

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

    fn max_dim(self, capacity_width: u32, capacity_height: u32) -> u32 {
        let capacity = max_canvas_dim_for_viewport(capacity_width, capacity_height);
        match self {
            Self::Cube => capacity.min(GPU_CANVAS_CUBE_SAFE_MAX_DIM),
            Self::Ico => capacity,
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
    width: u32,
    height: u32,
    pitch_bytes: u32,
}

unsafe impl Send for GpuCanvasBuffer {}

static GPU_CANVAS_SESSIONS: spin::Mutex<Vec<GpuCanvasSessionState>> = spin::Mutex::new(Vec::new());
static GPU_CANVAS_BUFFER: spin::Mutex<Option<GpuCanvasBuffer>> = spin::Mutex::new(None);
static GPU_CANVAS_DIM: AtomicU32 = AtomicU32::new(GPU_CANVAS_INITIAL_DIM);
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
    let dim = current_canvas_dim_for_viewport(viewport_width, viewport_height);
    let (x, y) = canvas_origin_from_cursor(viewport_width, viewport_height)
        .unwrap_or((GPU_CANVAS_FALLBACK_X, GPU_CANVAS_FALLBACK_Y));
    LiveOverlayRect::new(x, y, dim, dim, Rgba8::new(0, 0, 0, 0))
}

fn canvas_origin_from_cursor(viewport_width: u32, viewport_height: u32) -> Option<(u32, u32)> {
    if viewport_width == 0 || viewport_height == 0 {
        return None;
    }
    let cursor = crate::ui3::ui3_hid::preferred_cursor_snapshot(viewport_width, viewport_height)?;
    let dim = current_canvas_dim_for_viewport(viewport_width, viewport_height);
    Some((
        centered_origin(cursor.x_px, dim, viewport_width),
        centered_origin(cursor.y_px, dim, viewport_height),
    ))
}

fn centered_origin(center: u32, size: u32, extent: u32) -> u32 {
    if extent <= size {
        return 0;
    }
    center.saturating_sub(size / 2).min(extent - size)
}

fn current_canvas_dim_for_viewport(viewport_width: u32, viewport_height: u32) -> u32 {
    let max_dim = max_canvas_dim_for_viewport(viewport_width, viewport_height);
    let min_dim = GPU_CANVAS_MIN_DIM.min(max_dim).max(1);
    GPU_CANVAS_DIM
        .load(Ordering::Acquire)
        .clamp(min_dim, max_dim)
}

fn max_canvas_dim_for_viewport(viewport_width: u32, viewport_height: u32) -> u32 {
    viewport_width
        .min(viewport_height)
        .min(GPU_CANVAS_MAX_DIM)
        .max(1)
}

fn canvas_buffer_capacity_dim() -> u32 {
    let (viewport_width, viewport_height) = crate::intel::active_scanout_dimensions()
        .unwrap_or((GPU_CANVAS_INITIAL_DIM, GPU_CANVAS_INITIAL_DIM));
    max_canvas_dim_for_viewport(viewport_width, viewport_height)
}

fn canvas_buffer() -> Option<GpuCanvasBuffer> {
    let capacity_dim = canvas_buffer_capacity_dim();
    {
        let state = GPU_CANVAS_BUFFER.lock();
        if let Some(buffer) = *state {
            if buffer.width >= capacity_dim && buffer.height >= capacity_dim {
                return Some(buffer);
            }
        }
    }

    let pitch_bytes = capacity_dim.checked_mul(core::mem::size_of::<u32>() as u32)?;
    let bytes = (pitch_bytes as usize).checked_mul(capacity_dim as usize)?;
    let (phys, virt) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    crate::intel::dma_flush(virt, bytes);

    let buffer = GpuCanvasBuffer {
        phys,
        virt,
        bytes,
        width: capacity_dim,
        height: capacity_dim,
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
        let mut cursor_drain = crate::ui3::ui3_hid::Ui3CursorEventDrain::default();
        let (start_width, start_height) =
            crate::intel::active_scanout_dimensions().unwrap_or((GPU_CANVAS_INITIAL_DIM, GPU_CANVAS_INITIAL_DIM));
        let mut last_retired_dim = current_canvas_dim_for_viewport(start_width, start_height);
        prime_canvas_cursor_drain(&mut cursor_drain);
        loop {
            if cancel_or_interrupted(session_id, &task_target) {
                print_matrix_target_line(&task_target, "ui3 canvas: stopped before frame");
                break;
            }

            let wheel_delta = drain_canvas_wheel_delta(&mut cursor_drain);
            let Some(buffer) = canvas_buffer() else {
                print_matrix_target_line(&task_target, "ui3 canvas: no buffer");
                break;
            };
            let scene_max_dim = scene.max_dim(buffer.width, buffer.height);
            let canvas_dim =
                apply_canvas_wheel_delta(wheel_delta, scene_max_dim, scene_max_dim);
            let Some(surface) = GpgpuRgba8Surface::new(
                buffer.phys,
                crate::intel::GPU_VA_DISPLAY_UI3_CANVAS_BASE,
                buffer.bytes,
                canvas_dim,
                canvas_dim,
                buffer.pitch_bytes,
            ) else {
                print_matrix_target_line(&task_target, "ui3 canvas: bad buffer surface");
                break;
            };
            clear_canvas_buffer_region(buffer, canvas_dim, canvas_dim);

            match render_canvas3d_frame(scene, frame, buffer, surface) {
                Some(result) => {
                    let render_dim = canvas_dim;
                    let rollback_dim = if result.ok {
                        last_retired_dim = render_dim;
                        0
                    } else if render_dim > last_retired_dim {
                        GPU_CANVAS_DIM.store(last_retired_dim, Ordering::Release);
                        last_retired_dim
                    } else {
                        0
                    };
                    let canvas = current_canvas_rect();
                    let presented = if result.ok {
                        crate::ui3::ui3_orbits::submit_canvas_rgba(
                            canvas,
                            buffer.virt,
                            buffer.pitch_bytes as usize,
                            "ui3-canvas",
                        ) as u32
                    } else {
                        0
                    };
                    let avg_submit_ms = if result.submitted == 0 {
                        0
                    } else {
                        result.total_submit_ms / u64::from(result.submitted)
                    };
                    if frame < 3 || frame.is_multiple_of(10) {
                        print_matrix_target_line(
                            &task_target,
                            format!(
                                "ui3 canvas: frame={} scene={} ok={} submitted={} presented={} elapsed_ms={} avg_submit_ms={} visible={} angle={} render={}x{} surface={}x{} canvas={}x{} cap={}x{} max_dim={} dst={},{} wheel_delta={} groups={}x{}x{} pre=0x{:08X} post=0x{:08X} rollback={}",
                                frame,
                                scene.name(),
                                result.ok as u8,
                                result.submitted,
                                presented,
                                result.elapsed_ms,
                                avg_submit_ms,
                                result.visible_points,
                                result.last_angle_deg,
                                render_dim,
                                render_dim,
                                result.primary_width,
                                result.primary_height,
                                canvas.width,
                                canvas.height,
                                buffer.width,
                                buffer.height,
                                scene_max_dim,
                                canvas.x,
                                canvas.y,
                                wheel_delta,
                                result.work_group_x,
                                result.work_group_y,
                                result.work_group_z,
                                result.pre_marker,
                                result.post_marker,
                                rollback_dim,
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
            crate::intel::gpgpu::CANVAS3D_CUBE_LIVE_HALF_Q16,
            surface,
        ),
        GpuCanvas3dScene::Ico => render_canvas3d_ico_frame(frame, buffer, surface.width),
    }
}

fn render_canvas3d_ico_frame(
    frame: u32,
    buffer: GpuCanvasBuffer,
    dim: u32,
) -> Option<crate::intel::gpgpu::GpgpuShellCube20ProjectResult> {
    let texture = crate::intel::gpgpu::canvas3d_ico_project_texture_frame(frame, dim, dim)?;
    copy_texture_rgba_to_buffer(&texture, buffer)?;
    Some(texture.result)
}

fn copy_texture_rgba_to_buffer(
    texture: &GpgpuCanvas3dUi2TextureFrame,
    buffer: GpuCanvasBuffer,
) -> Option<()> {
    if texture.width == 0
        || texture.height == 0
        || texture.width > buffer.width
        || texture.height > buffer.height
    {
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
    flush_canvas_buffer_region(buffer, texture.width, texture.height);
    Some(())
}

fn pack_rgba_bytes_as_bgra_u32(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from(a) << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

fn prime_canvas_cursor_drain(drain: &mut crate::ui3::ui3_hid::Ui3CursorEventDrain) {
    let mut out =
        [crate::usb2::hid::TrueosHidCursorEvent::default(); GPU_CANVAS_CURSOR_EVENT_BATCH_CAP];
    loop {
        let read = crate::ui3::ui3_hid::drain_cursor_events(drain, out.as_mut_slice());
        if read.wrote < out.len() {
            break;
        }
    }
}

fn drain_canvas_wheel_delta(drain: &mut crate::ui3::ui3_hid::Ui3CursorEventDrain) -> i32 {
    let mut out =
        [crate::usb2::hid::TrueosHidCursorEvent::default(); GPU_CANVAS_CURSOR_EVENT_BATCH_CAP];
    let mut total = 0i32;
    loop {
        let read = crate::ui3::ui3_hid::drain_cursor_events(drain, out.as_mut_slice());
        for event in &out[..read.wrote] {
            total = total.saturating_add(crate::ui3::ui3_hid::event_wheel_delta(*event));
        }
        if read.wrote < out.len() {
            break;
        }
    }
    total
}

fn apply_canvas_wheel_delta(wheel_delta: i32, capacity_width: u32, capacity_height: u32) -> u32 {
    let max_dim = max_canvas_dim_for_viewport(capacity_width, capacity_height);
    let min_dim = GPU_CANVAS_MIN_DIM.min(max_dim).max(1);
    if wheel_delta != 0 {
        let current = GPU_CANVAS_DIM.load(Ordering::Acquire);
        let delta_px = wheel_delta.saturating_mul(GPU_CANVAS_WHEEL_STEP_PX);
        let next = (current as i32)
            .saturating_add(delta_px)
            .clamp(min_dim as i32, max_dim as i32) as u32;
        GPU_CANVAS_DIM.store(next, Ordering::Release);
    }
    GPU_CANVAS_DIM
        .load(Ordering::Acquire)
        .clamp(min_dim, max_dim)
}

fn clear_canvas_buffer_region(buffer: GpuCanvasBuffer, width: u32, height: u32) {
    let row_bytes = (width as usize).saturating_mul(core::mem::size_of::<u32>());
    for y in 0..height.min(buffer.height) as usize {
        unsafe {
            core::ptr::write_bytes(
                buffer
                    .virt
                    .add(y.saturating_mul(buffer.pitch_bytes as usize)),
                0,
                row_bytes.min(buffer.pitch_bytes as usize),
            );
        }
    }
    flush_canvas_buffer_region(buffer, width, height);
}

fn flush_canvas_buffer_region(buffer: GpuCanvasBuffer, width: u32, height: u32) {
    let width = width.min(buffer.width);
    let height = height.min(buffer.height);
    if width == 0 || height == 0 {
        return;
    }
    let last_row = (height as usize).saturating_sub(1);
    let bytes = last_row
        .saturating_mul(buffer.pitch_bytes as usize)
        .saturating_add((width as usize).saturating_mul(core::mem::size_of::<u32>()));
    crate::intel::dma_flush(buffer.virt, bytes.min(buffer.bytes));
}
