use alloc::format;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::shell2::{
    CommandSessionInputResult, MatrixTarget, ShellBackend2, matrix_target_for_backend,
    matrix_target_interrupted, print_matrix_target_line, print_shell_line,
    set_matrix_target_active, switch_matrix_target_slot,
};

const GPU_CANVAS_SLOT: &str = "Gid";
const GPU_CANVAS_FRAME_MS: u64 = 33;
const GPU_CANVAS_OVERLAY_WIDTH: u32 = 192;
const GPU_CANVAS_OVERLAY_HEIGHT: u32 = 192;

#[derive(Clone)]
struct GpuCanvasSessionState {
    id: u64,
    cancel_requested: bool,
}

static GPU_CANVAS_SESSIONS: spin::Mutex<Vec<GpuCanvasSessionState>> = spin::Mutex::new(Vec::new());
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

            match crate::intel::gpgpu::shell_cube6_plane_project_overlay_frame(
                frame,
                half_q16,
                GPU_CANVAS_OVERLAY_WIDTH,
                GPU_CANVAS_OVERLAY_HEIGHT,
            ) {
                Some(result) => {
                    let avg_submit_ms = if result.submitted == 0 {
                        0
                    } else {
                        result.total_submit_ms / u64::from(result.submitted)
                    };
                    if frame < 3 || frame.is_multiple_of(10) {
                        print_matrix_target_line(
                            &task_target,
                            format!(
                                "ui3 canvas: frame={} ok={} submitted={} presented={} elapsed_ms={} avg_submit_ms={} visible_faces={} angle={} overlay={}x{}",
                                frame,
                                result.ok as u8,
                                result.submitted,
                                result.presented,
                                result.elapsed_ms,
                                avg_submit_ms,
                                result.visible_points,
                                result.last_angle_deg,
                                result.primary_width,
                                result.primary_height,
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
