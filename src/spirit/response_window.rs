//! Spirit chat output through one retained kernel Gridpaper document.
//!
//! Completed reasoning text enters a small bounded queue. This service reveals
//! Spirit's dedicated Gridpaper lease, flies Lilly's software cursor to cell
//! zero, clicks it, and types through her paired virtual keyboard. Hiding the
//! UI4 session after the reading interval retains the Gridpaper GPU scene and
//! document allocation for the next response.

use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer, with_timeout};
use spin::Mutex;

use crate::r::{
    gridpaper_service::{KernelGridLease, KernelGridPresentation},
    keyboard_control_service::{
        KeyboardControlDevice, KeyboardControlPrincipal, keyboard_is_idle, request_keyboard,
        submit_text,
    },
};

const RESPONSE_QUEUE_CAPACITY: usize = 4;
const SPIRIT_GRID_COLUMNS: u32 = 39 / 2;
const SPIRIT_GRID_ROWS: u32 = 55 / 4;
const SPIRIT_GRID_CELL_CAPACITY: usize = SPIRIT_GRID_COLUMNS as usize * SPIRIT_GRID_ROWS as usize;
const RESPONSE_SERVICE_POLL_MS: u64 = 16;
const RESPONSE_READ_MS: u64 = 30_000;
const INPUT_BROKER_SETTLE_MS: u64 = 32;
const CURSOR_SETTLE_GRACE_MS: u64 = 250;
const GRID_TEXT_ACCEPT_TIMEOUT_MS: u64 = 5_000;
const KEYBOARD_STROKE_MS: u32 = 48;
const KEYBOARD_CHUNK_SCALARS: usize = 64;
const SPIRIT_KEYBOARD_LABEL: &str = "Spirit/Lilly chat";

static RESPONSE_QUEUE: Mutex<VecDeque<ResponseRequest>> = Mutex::new(VecDeque::new());
static RESPONSE_WAKE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static SPIRIT_TEXT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ResponseRequest {
    turn: u64,
    text: String,
    enqueued_ms: u64,
}

/// Copy one completed local-model reply into Spirit's bounded presentation
/// ingress. Oldest unseen replies yield to the newest if inference outruns the
/// one user-facing Gridpaper document.
pub(crate) fn enqueue_reasoning_response(turn: u64, text: &str) -> bool {
    let text = sanitize_response(text);
    let mut queue = RESPONSE_QUEUE.lock();
    let dropped = if queue.len() == RESPONSE_QUEUE_CAPACITY {
        queue.pop_front().map(|request| request.turn)
    } else {
        None
    };
    queue.push_back(ResponseRequest {
        turn,
        text,
        enqueued_ms: Instant::now().as_millis(),
    });
    let queued = queue.len();
    drop(queue);
    RESPONSE_WAKE.signal(());
    if let Some(dropped) = dropped {
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: response ingress replaced oldest_turn={} newest_turn={} queue={} action=prefer-latest\n",
            dropped,
            turn,
            queued,
        );
    }
    true
}

pub(super) fn enqueue_package_text(text: &str) -> bool {
    let sequence = SPIRIT_TEXT_SEQUENCE
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1)
        .max(1);
    enqueue_reasoning_response(sequence, text)
}

fn sanitize_response(text: &str) -> String {
    let mut words = Vec::new();
    for source in text.split_whitespace() {
        let word = source
            .chars()
            .filter(|ch| !ch.is_control())
            .collect::<Vec<_>>();
        if !word.is_empty() {
            words.push(word);
        }
    }
    if words.is_empty() {
        words.push("(no".chars().collect());
        words.push("text".chars().collect());
        words.push("response)".chars().collect());
    }

    let mut lines = alloc::vec![Vec::new()];
    let mut truncated = false;
    'words: for word in words {
        let mut word_offset = 0usize;
        while word_offset < word.len() {
            let line_len = lines.last().map_or(0, Vec::len);
            let remaining = word.len() - word_offset;
            if remaining <= SPIRIT_GRID_COLUMNS as usize {
                if line_len == 0 {
                    lines
                        .last_mut()
                        .expect("Spirit response always has one line")
                        .extend_from_slice(&word[word_offset..]);
                    word_offset = word.len();
                } else if line_len + 1 + remaining <= SPIRIT_GRID_COLUMNS as usize {
                    let line = lines
                        .last_mut()
                        .expect("Spirit response always has one line");
                    line.push(' ');
                    line.extend_from_slice(&word[word_offset..]);
                    word_offset = word.len();
                } else if lines.len() < SPIRIT_GRID_ROWS as usize {
                    // The separating whitespace becomes a row transition. The
                    // word itself stays intact on the next line.
                    lines.push(Vec::new());
                } else {
                    truncated = true;
                    break 'words;
                }
                continue;
            }

            // A single token wider than the grid has no whitespace boundary
            // at which it can fit. Hard-wrap only that exceptional token.
            if line_len != 0 {
                if lines.len() < SPIRIT_GRID_ROWS as usize {
                    lines.push(Vec::new());
                    continue;
                }
                truncated = true;
                break 'words;
            }
            let take = remaining.min(SPIRIT_GRID_COLUMNS as usize);
            lines
                .last_mut()
                .expect("Spirit response always has one line")
                .extend_from_slice(&word[word_offset..word_offset + take]);
            word_offset += take;
            if word_offset < word.len() {
                if lines.len() < SPIRIT_GRID_ROWS as usize {
                    lines.push(Vec::new());
                } else {
                    truncated = true;
                    break 'words;
                }
            }
        }
    }

    if truncated {
        let line = lines
            .last_mut()
            .expect("Spirit response always has one line");
        line.truncate((SPIRIT_GRID_COLUMNS as usize).saturating_sub(3));
        line.extend(['.', '.', '.']);
    }

    let mut wrapped = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index != 0 {
            wrapped.push('\n');
        }
        wrapped.extend(line.iter().copied());
    }
    wrapped
}

fn take_latest_response() -> Option<ResponseRequest> {
    let mut queue = RESPONSE_QUEUE.lock();
    let latest = queue.pop_back();
    queue.clear();
    let _ = RESPONSE_WAKE.try_take();
    latest
}

fn presentation_is_ready(presentation: KernelGridPresentation) -> bool {
    let Some(output) = crate::ui4::OutputId::from_slot(0) else {
        return false;
    };
    crate::ui4::visible_windows_for_output(output)
        .into_iter()
        .any(|window| {
            window.owner == crate::ui4::WindowOwner::GRIDPAPER_SERVICE
                && window.id == presentation.window
                && window.state == crate::ui4::WindowState::Ready
                && window.publish_serial != 0
                && window.placement.visible
        })
}

async fn wait_for_ready_presentation(
    lease: KernelGridLease,
    generation: u64,
) -> KernelGridPresentation {
    loop {
        if let Some(presentation) = crate::r::gridpaper_service::kernel_grid_presentation(lease)
            && presentation.published_generation == generation
            && presentation_is_ready(presentation)
        {
            return presentation;
        }
        Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
    }
}

async fn focus_and_click_cell_zero(
    presentation: KernelGridPresentation,
) -> Result<(), &'static str> {
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("no-active-scanout")?;
    let approach_ms = super::lilly_cursor::queue_window_approach(
        presentation.cell_zero_x,
        presentation.cell_zero_y,
        screen_width,
        screen_height,
    )
    .map_err(|_| "lilly-cursor-approach")?;
    let deadline = Instant::now()
        + Duration::from_millis(u64::from(approach_ms).saturating_add(CURSOR_SETTLE_GRACE_MS));
    loop {
        match super::lilly_cursor::window_approach_complete() {
            Ok(true) => break,
            Ok(false) if Instant::now() < deadline => {
                Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
            }
            Ok(false) => return Err("lilly-cursor-timeout"),
            Err(_) => return Err("lilly-cursor-status"),
        }
    }

    Timer::after(Duration::from_millis(INPUT_BROKER_SETTLE_MS)).await;
    let source = super::lilly_cursor::selection_source().map_err(|_| "lilly-cursor-source")?;
    crate::ui4::select_window_for_cursor(
        source,
        crate::ui4::WindowOwner::GRIDPAPER_SERVICE,
        presentation.window,
    )
    .map_err(|_| "gridpaper-focus")?;
    super::lilly_cursor::queue_primary_click().map_err(|_| "gridpaper-cell-zero-click")?;
    loop {
        match super::lilly_cursor::window_approach_complete() {
            Ok(true) => break,
            Ok(false) => Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await,
            Err(_) => return Err("gridpaper-click-status"),
        }
    }
    Timer::after(Duration::from_millis(INPUT_BROKER_SETTLE_MS)).await;
    let expected = crate::ui4::CursorFrameKey::new(
        crate::ui4::WindowOwner::GRIDPAPER_SERVICE,
        presentation.window,
    );
    if crate::ui4::selected_frame_for_source(source) != Some(expected) {
        return Err("gridpaper-focus-not-latched");
    }
    let placement = crate::ui4::window_placement(
        crate::ui4::WindowOwner::GRIDPAPER_SERVICE,
        presentation.window,
    )
    .map_err(|_| "gridpaper-outline-placement")?;
    super::lilly_cursor::queue_window_outline(
        placement.x,
        placement.y,
        placement.width,
        placement.height,
        screen_width,
        screen_height,
    )
    .map_err(|_| "gridpaper-outline-submit")?;
    Ok(())
}

async fn wait_for_keyboard_idle(keyboard: KeyboardControlDevice) -> Result<(), &'static str> {
    loop {
        match keyboard_is_idle(KeyboardControlPrincipal::Kernel, keyboard.handle) {
            Ok(true) => return Ok(()),
            Ok(false) => Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await,
            Err(_) => return Err("lilly-keyboard-status"),
        }
    }
}

async fn type_response(keyboard: KeyboardControlDevice, text: &str) -> Result<usize, &'static str> {
    let scalars = text.chars().collect::<Vec<_>>();
    let mut typed = 0usize;
    for (chunk_index, chunk) in scalars.chunks(KEYBOARD_CHUNK_SCALARS).enumerate() {
        wait_for_keyboard_idle(keyboard).await?;
        let text = chunk.iter().collect::<String>();
        submit_text(
            KeyboardControlPrincipal::Kernel,
            keyboard.handle,
            text.as_str(),
            KEYBOARD_STROKE_MS,
            chunk_index == 0,
        )
        .map_err(|_| "lilly-keyboard-submit")?;
        typed = typed.saturating_add(chunk.iter().filter(|ch| !matches!(ch, '\n' | '\r')).count());
    }
    wait_for_keyboard_idle(keyboard).await?;
    Ok(typed)
}

async fn wait_for_grid_text_acceptance(
    lease: KernelGridLease,
    expected_cells: usize,
) -> Result<(), &'static str> {
    let expected_cells = u64::try_from(expected_cells).map_err(|_| "gridpaper-text-count-range")?;
    let deadline = Instant::now() + Duration::from_millis(GRID_TEXT_ACCEPT_TIMEOUT_MS);
    loop {
        match crate::r::gridpaper_service::kernel_grid_accepted_text_cells(lease) {
            Some(accepted) if accepted >= expected_cells => return Ok(()),
            Some(_) if Instant::now() < deadline => {
                Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
            }
            Some(_) => return Err("gridpaper-text-accept-timeout"),
            None => return Err("gridpaper-presentation-lost"),
        }
    }
}

async fn request_spirit_grid() -> KernelGridLease {
    let mut last_error = None;
    loop {
        match crate::r::gridpaper_service::request_spirit_response_grid(
            SPIRIT_GRID_COLUMNS,
            SPIRIT_GRID_ROWS,
        ) {
            Ok(lease) => return lease,
            Err(error) => {
                if last_error != Some(error) {
                    crate::log_warn!(
                        target: "gfx";
                        "trueos-spirit: response Gridpaper lease waiting error={:?} grid={}x{} retry_ms=250\n",
                        error,
                        SPIRIT_GRID_COLUMNS,
                        SPIRIT_GRID_ROWS,
                    );
                    last_error = Some(error);
                }
                Timer::after(Duration::from_millis(250)).await;
            }
        }
    }
}

async fn request_spirit_keyboard() -> KeyboardControlDevice {
    let mut last_error = None;
    loop {
        match request_keyboard(KeyboardControlPrincipal::Kernel, SPIRIT_KEYBOARD_LABEL) {
            Ok(keyboard) => match super::lilly_cursor::bind_keyboard(keyboard.slot_id) {
                Ok(()) => return keyboard,
                Err(reason) => {
                    let _ = crate::r::keyboard_control_service::release_keyboard(
                        KeyboardControlPrincipal::Kernel,
                        keyboard.handle,
                    );
                    if last_error != Some(reason) {
                        crate::log_warn!(
                            target: "gfx";
                            "trueos-spirit: response keyboard combo waiting reason={} retry_ms=250\n",
                            reason,
                        );
                        last_error = Some(reason);
                    }
                }
            },
            Err(_) if last_error != Some("lilly-keyboard-capacity") => {
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: response keyboard waiting reason=capacity retry_ms=250\n",
                );
                last_error = Some("lilly-keyboard-capacity");
            }
            Err(_) => {}
        }
        Timer::after(Duration::from_millis(250)).await;
    }
}

#[embassy_executor::task]
pub(crate) async fn spirit_response_window_service_task(expected_slot: u32) {
    let lease = request_spirit_grid().await;
    let keyboard = request_spirit_keyboard().await;
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: response Gridpaper service online assigned_slot={} current_slot={} grid={}x{} cells={} ownership=kernel-dedicated cursor=Spirit/Lilly keyboard_slot={} input=cell-zero-click+paired-vkeyboard wrap=whitespace-before-word style=rainbow-palette+cpp-scale-0.85..1.15 hide_after_ms={} residency=warm-hidden no-blueprint-vm=1\n",
        expected_slot,
        crate::percpu::current_slot(),
        SPIRIT_GRID_COLUMNS,
        SPIRIT_GRID_ROWS,
        SPIRIT_GRID_CELL_CAPACITY,
        keyboard.slot_id,
        RESPONSE_READ_MS,
    );

    loop {
        let request = match take_latest_response() {
            Some(request) => request,
            None => {
                RESPONSE_WAKE.wait().await;
                continue;
            }
        };
        let generation = match crate::r::gridpaper_service::reset_and_show_kernel_grid(lease) {
            Ok(generation) => generation,
            Err(error) => {
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: response dropped turn={} reason=gridpaper-reset error={:?}\n",
                    request.turn,
                    error,
                );
                continue;
            }
        };

        let presentation = wait_for_ready_presentation(lease, generation).await;
        if let Err(reason) = focus_and_click_cell_zero(presentation).await {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: response dropped turn={} window={} reason={}\n",
                request.turn,
                presentation.window.raw(),
                reason,
            );
            let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
            continue;
        }
        match type_response(keyboard, request.text.as_str()).await {
            Ok(typed) => {
                let accepted = wait_for_grid_text_acceptance(lease, typed).await;
                match accepted.and_then(|()| {
                    crate::r::gridpaper_service::enable_spirit_response_rainbow_motion(lease)
                        .map_err(|_| "gridpaper-motion-enable")
                }) {
                    Ok(animation_serial) => crate::log_info!(
                        target: "gfx";
                        "trueos-spirit: response typed turn={} window={} cells={} latency_ms={} path=paired-vkeyboard->ui4->gridpaper-cell-patches wrap=whitespace-before-word palette=rainbow animation_serial={} animation=cpp-scale-sine-0.85..1.15 gpu-scene=resident read_ms={}\n",
                        request.turn,
                        presentation.window.raw(),
                        typed,
                        Instant::now().as_millis().saturating_sub(request.enqueued_ms),
                        animation_serial,
                        RESPONSE_READ_MS,
                    ),
                    Err(reason) => crate::log_warn!(
                        target: "gfx";
                        "trueos-spirit: response typed without motion turn={} window={} cells={} reason={} retained_static_rainbow=1\n",
                        request.turn,
                        presentation.window.raw(),
                        typed,
                        reason,
                    ),
                }
            }
            Err(reason) => crate::log_warn!(
                target: "gfx";
                "trueos-spirit: response typing stopped turn={} window={} reason={}\n",
                request.turn,
                presentation.window.raw(),
                reason,
            ),
        }

        if RESPONSE_QUEUE.lock().is_empty() {
            let _ =
                with_timeout(Duration::from_millis(RESPONSE_READ_MS), RESPONSE_WAKE.wait()).await;
        }
        if RESPONSE_QUEUE.lock().is_empty() {
            // TODO(chat): replace this fixed reading timeout with an explicit
            // conversation/window lifecycle once chat history and dismissal
            // semantics are decided.
            if let Err(error) = crate::r::gridpaper_service::hide_kernel_grid(lease) {
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: response hide deferred turn={} error={:?}\n",
                    request.turn,
                    error,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_is_bounded_by_grid_cells_and_wraps_whitespace() {
        let input = alloc::format!("hello\n\tworld {}", "x".repeat(SPIRIT_GRID_CELL_CAPACITY + 20));
        let output = sanitize_response(input.as_str());
        let lines = output.lines().collect::<Vec<_>>();
        assert!(lines.len() <= SPIRIT_GRID_ROWS as usize);
        assert!(
            lines
                .iter()
                .all(|line| line.chars().count() <= SPIRIT_GRID_COLUMNS as usize)
        );
        assert!(output.starts_with("hello world\n"));
        assert!(output.ends_with("..."));
    }

    #[test]
    fn response_moves_a_word_to_the_next_row_before_splitting_it() {
        let output = sanitize_response("1234567890123456 abc next");
        assert_eq!(output, "1234567890123456\nabc next");
    }
}
