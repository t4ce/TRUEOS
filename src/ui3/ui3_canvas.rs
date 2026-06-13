use alloc::format;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::intel::LiveOverlayRect;
use crate::intel::gpgpu::GpgpuRgba8Surface;
use crate::intel::types::Rgba8;
use crate::shell2::{
    CommandSessionInputResult, MatrixTarget, ShellBackend2, matrix_target_for_backend,
    matrix_target_interrupted, print_matrix_target_line, print_shell_line,
    set_matrix_target_active, switch_matrix_target_slot,
};

const GPU_CANVAS_SLOT: &str = "Gid";
const GPU_CANVAS_FRAME_MS: u64 = 33;
const GPU_CANVAS_OVERLAY_X: u32 = 48;
const GPU_CANVAS_OVERLAY_Y: u32 = 48;
const GPU_CANVAS_OVERLAY_WIDTH: u32 = 192;
const GPU_CANVAS_OVERLAY_HEIGHT: u32 = 192;

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
    let canvas = LiveOverlayRect::new(
        GPU_CANVAS_OVERLAY_X,
        GPU_CANVAS_OVERLAY_Y,
        GPU_CANVAS_OVERLAY_WIDTH,
        GPU_CANVAS_OVERLAY_HEIGHT,
        Rgba8::new(0, 0, 0, 0),
    );
    rects
        .iter()
        .all(|rect| !overlay_rects_intersect(canvas, *rect))
        .then_some(canvas)
}

fn canvas_rect() -> LiveOverlayRect {
    LiveOverlayRect::new(
        GPU_CANVAS_OVERLAY_X,
        GPU_CANVAS_OVERLAY_Y,
        GPU_CANVAS_OVERLAY_WIDTH,
        GPU_CANVAS_OVERLAY_HEIGHT,
        Rgba8::new(0, 0, 0, 0),
    )
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

pub(crate) fn submit_plane_draw(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    half_q16: i32,
) -> Option<u64> {
    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, GPU_CANVAS_SLOT);
    let session_id = session_start();

    print_matrix_target_line(
        &target,
        format!(
            "ui3 canvas: starting cube6 plane worker half_q16={} cadence_ms={}",
            half_q16, GPU_CANVAS_FRAME_MS
        )
        .as_str(),
    );
    set_matrix_target_active(&target, true);

    let spawn_token = match ui3_canvas_worker_task(target.clone(), session_id, half_q16) {
        Ok(token) => token,
        Err(_) => {
            session_finish(session_id);
            set_matrix_target_active(&target, false);
            print_shell_line(io, "gpgpu plane draw: worker spawn failed");
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
async fn ui3_canvas_worker_task(target: MatrixTarget, session_id: u64, half_q16: i32) {
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

            match crate::intel::gpgpu::shell_cube6_plane_project_surface_frame(
                frame, half_q16, surface,
            ) {
                Some(result) => {
                    let canvas = canvas_rect();
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
                                "ui3 canvas: frame={} ok={} submitted={} presented={} elapsed_ms={} avg_submit_ms={} visible_faces={} angle={} surface={}x{} canvas={}x{}",
                                frame,
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
                            )
                            .as_str(),
                        )
                    }
                    frame = frame.wrapping_add(1);
                }
                None => {
                    print_matrix_target_line(
                        &task_target,
                        "ui3 canvas: frame failed (check primary surface, iGPU claim, and plane worklist artifact)",
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
