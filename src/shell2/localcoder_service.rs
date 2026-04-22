use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use localcoder::localcoder_service as lc_service;

const AI_CURSOR_SLOT_ID: u32 = 1;
const LOCALCODER_SERVICE_IDLE_MS: u64 = 8;
const LOCALCODER_SERVICE_STEP_MS: u64 = 12;
const LOCALCODER_SERVICE_QUEUE_CAP: usize = 32;

static LOCALCODER_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static LOCALCODER_SERVICE_INSTALLED: AtomicBool = AtomicBool::new(false);
static LOCALCODER_SERVICE_QUEUE: spin::Mutex<VecDeque<lc_service::LocalcoderServiceCommand>> =
    spin::Mutex::new(VecDeque::new());

pub(crate) fn ensure_registered(spawner: &Spawner) {
    if LOCALCODER_SERVICE_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        lc_service::register_handler(queue_localcoder_service_command);
        lc_service::register_context_provider(current_viewport_context);
    }

    if LOCALCODER_SERVICE_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        match localcoder_cursor_service_task() {
            Ok(token) => {
                spawner.spawn(token);
            }
            Err(_) => {
                LOCALCODER_SERVICE_STARTED.store(false, Ordering::Release);
            }
        }
    }
}

fn queue_localcoder_service_command(
    command: lc_service::LocalcoderServiceCommand,
) -> lc_service::LocalcoderServiceResult {
    let summary = summarize_command(&command);
    let mut guard = LOCALCODER_SERVICE_QUEUE.lock();
    if guard.len() >= LOCALCODER_SERVICE_QUEUE_CAP {
        return Err(lc_service::service_error("localcoder_service queue is full"));
    }
    guard.push_back(command);
    Ok(summary)
}

fn summarize_command(command: &lc_service::LocalcoderServiceCommand) -> String {
    let (vp_w, vp_h) = viewport_dimensions_px();
    match command {
        lc_service::LocalcoderServiceCommand::MoveAbs {
            x_px,
            y_px,
            x_norm,
            y_norm,
            duration_ms,
        } => match (x_px, y_px, x_norm, y_norm) {
            (Some(x), Some(y), None, None) => format!(
                "queued cursor move to {}x{} px over {} ms on {}x{} viewport",
                x, y, duration_ms, vp_w, vp_h
            ),
            (None, None, Some(x), Some(y)) => format!(
                "queued cursor move to normalized {:.3},{:.3} over {} ms on {}x{} viewport",
                x, y, duration_ms, vp_w, vp_h
            ),
            _ => String::from("queued cursor move"),
        },
        lc_service::LocalcoderServiceCommand::Orbit {
            loop_duration_ms,
            loops,
            ..
        } => format!(
            "queued cursor orbit for {} loop(s), {} ms each, on {}x{} viewport",
            loops, loop_duration_ms, vp_w, vp_h
        ),
        lc_service::LocalcoderServiceCommand::Click {
            buttons_down,
            repeat,
            delay_ms,
        } => format!(
            "queued button mask {} click x{} with {} ms delay",
            buttons_down, repeat, delay_ms
        ),
        lc_service::LocalcoderServiceCommand::ButtonDown { buttons_down } => {
            format!("queued button down mask {}", buttons_down)
        }
        lc_service::LocalcoderServiceCommand::ButtonUp { buttons_up } => {
            format!("queued button up mask {}", buttons_up)
        }
        lc_service::LocalcoderServiceCommand::SetButtons { buttons_down } => {
            format!("queued button state mask {}", buttons_down)
        }
    }
}

fn current_viewport_context() -> String {
    let (vp_w, vp_h) = viewport_dimensions_px();
    format!(
        "Current cursor viewport is {}x{} px. Normalized coordinates use 0.0 at the left/top edge and 1.0 at the right/bottom edge. radius_norm is measured against the smaller viewport dimension.",
        vp_w, vp_h
    )
}

#[embassy_executor::task(pool_size = 1)]
async fn localcoder_cursor_service_task() {
    loop {
        let command = LOCALCODER_SERVICE_QUEUE.lock().pop_front();
        if let Some(command) = command {
            run_command(command).await;
            continue;
        }

        Timer::after(EmbassyDuration::from_millis(LOCALCODER_SERVICE_IDLE_MS)).await;
    }
}

async fn run_command(command: lc_service::LocalcoderServiceCommand) {
    match command {
        lc_service::LocalcoderServiceCommand::MoveAbs {
            x_px,
            y_px,
            x_norm,
            y_norm,
            duration_ms,
        } => {
            let Some((target_x, target_y)) = resolve_point_px(x_px, y_px, x_norm, y_norm) else {
                return;
            };
            let (start_x, start_y, buttons_down) = current_cursor_position_px();
            smooth_move(start_x, start_y, target_x, target_y, duration_ms, buttons_down).await;
        }
        lc_service::LocalcoderServiceCommand::Orbit {
            center_x_px,
            center_y_px,
            center_x_norm,
            center_y_norm,
            radius_px,
            radius_norm,
            loop_duration_ms,
            loops,
        } => {
            let Some((center_x, center_y)) = resolve_point_px(
                center_x_px,
                center_y_px,
                center_x_norm,
                center_y_norm,
            ) else {
                return;
            };
            let radius_px = resolve_radius_px(radius_px, radius_norm);
            if radius_px <= 0 {
                return;
            }

            let start_x = center_x.saturating_add(radius_px);
            let start_y = center_y;
            let (cur_x, cur_y, buttons_down) = current_cursor_position_px();
            let lead_in_ms = loop_duration_ms.min(240).max(60) / 3;
            smooth_move(cur_x, cur_y, start_x, start_y, lead_in_ms, buttons_down).await;

            let steps_per_loop = (loop_duration_ms / LOCALCODER_SERVICE_STEP_MS as u32).max(16);
            let step_delay_ms = (loop_duration_ms / steps_per_loop).max(1) as u64;
            let total_steps = steps_per_loop.saturating_mul(loops.max(1));

            for step in 0..total_steps {
                let turns = step as f64 / steps_per_loop as f64;
                let angle = turns * core::f64::consts::TAU;
                let x = center_x as f64 + libm::cos(angle) * radius_px as f64;
                let y = center_y as f64 + libm::sin(angle) * radius_px as f64;
                inject_cursor(x.round() as i32, y.round() as i32, buttons_down);
                Timer::after(EmbassyDuration::from_millis(step_delay_ms)).await;
            }
        }
        lc_service::LocalcoderServiceCommand::Click {
            buttons_down,
            repeat,
            delay_ms,
        } => {
            let (x_px, y_px, base_buttons) = current_cursor_position_px();
            for idx in 0..repeat {
                inject_cursor(x_px, y_px, base_buttons | buttons_down);
                Timer::after(EmbassyDuration::from_millis(24)).await;
                inject_cursor(x_px, y_px, base_buttons);
                if idx + 1 < repeat {
                    Timer::after(EmbassyDuration::from_millis(delay_ms as u64)).await;
                }
            }
        }
        lc_service::LocalcoderServiceCommand::ButtonDown { buttons_down } => {
            let (x_px, y_px, base_buttons) = current_cursor_position_px();
            inject_cursor(x_px, y_px, base_buttons | buttons_down);
        }
        lc_service::LocalcoderServiceCommand::ButtonUp { buttons_up } => {
            let (x_px, y_px, base_buttons) = current_cursor_position_px();
            inject_cursor(x_px, y_px, base_buttons & !buttons_up);
        }
        lc_service::LocalcoderServiceCommand::SetButtons { buttons_down } => {
            let (x_px, y_px, _) = current_cursor_position_px();
            inject_cursor(x_px, y_px, buttons_down);
        }
    }
}

async fn smooth_move(
    start_x: i32,
    start_y: i32,
    target_x: i32,
    target_y: i32,
    duration_ms: u32,
    buttons_down: u32,
) {
    if duration_ms == 0 {
        inject_cursor(target_x, target_y, buttons_down);
        return;
    }

    let steps = (duration_ms / LOCALCODER_SERVICE_STEP_MS as u32).max(1);
    let step_delay_ms = (duration_ms / steps).max(1) as u64;
    for step in 1..=steps {
        let t = step as f64 / steps as f64;
        let eased = blend_linear_smoothstep(t);
        let x = lerp_i32(start_x, target_x, eased);
        let y = lerp_i32(start_y, target_y, eased);
        inject_cursor(x, y, buttons_down);
        Timer::after(EmbassyDuration::from_millis(step_delay_ms)).await;
    }
}

fn resolve_point_px(
    x_px: Option<i32>,
    y_px: Option<i32>,
    x_norm: Option<f64>,
    y_norm: Option<f64>,
) -> Option<(i32, i32)> {
    match (x_px, y_px, x_norm, y_norm) {
        (Some(x_px), Some(y_px), None, None) => Some((x_px, y_px)),
        (None, None, Some(x_norm), Some(y_norm)) => {
            let (vp_w, vp_h) = viewport_dimensions_px();
            Some((
                norm_to_px(x_norm, vp_w),
                norm_to_px(y_norm, vp_h),
            ))
        }
        _ => None,
    }
}

fn resolve_radius_px(radius_px: Option<u32>, radius_norm: Option<f64>) -> i32 {
    match (radius_px, radius_norm) {
        (Some(radius_px), None) => radius_px.min(i32::MAX as u32) as i32,
        (None, Some(radius_norm)) => {
            let (vp_w, vp_h) = viewport_dimensions_px();
            let base = vp_w.min(vp_h).max(1) as f64;
            (radius_norm * base).round().clamp(1.0, i32::MAX as f64) as i32
        }
        _ => 0,
    }
}

fn current_cursor_position_px() -> (i32, i32, u32) {
    let (vp_w, vp_h) = viewport_dimensions_px();
    let mut fallback: Option<(i32, i32, u32)> = None;
    for (slot_id, x_norm, y_norm, buttons_down) in crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons() {
        let x_px = norm_to_px(x_norm, vp_w);
        let y_px = norm_to_px(y_norm, vp_h);
        if slot_id == AI_CURSOR_SLOT_ID {
            return (x_px, y_px, buttons_down);
        }
        if fallback.is_none() {
            fallback = Some((x_px, y_px, buttons_down));
        }
    }

    fallback.unwrap_or((vp_w / 2, vp_h / 2, 0))
}

fn viewport_dimensions_px() -> (i32, i32) {
    crate::r::io::cabi::localcoder_cursor_viewport_dimensions_px()
}

fn inject_cursor(x_px: i32, y_px: i32, buttons_down: u32) {
    let _ = unsafe {
        crate::r::io::cabi::localcoder_input_write_cursor(
            AI_CURSOR_SLOT_ID,
            x_px,
            y_px,
            buttons_down,
            0,
            0,
        )
    };
}

fn norm_to_px(value: f64, extent_px: i32) -> i32 {
    let clamped = value.clamp(0.0, 1.0);
    let max = extent_px.saturating_sub(1).max(1) as f64;
    (clamped * max).round() as i32
}

fn lerp_i32(start: i32, end: i32, t: f64) -> i32 {
    let start = start as f64;
    let end = end as f64;
    (start + (end - start) * t).round() as i32
}

fn blend_linear_smoothstep(t: f64) -> f64 {
    let clamped = t.clamp(0.0, 1.0);
    let smooth = clamped * clamped * (3.0 - (2.0 * clamped));
    (0.8 * clamped) + (0.2 * smooth)
}