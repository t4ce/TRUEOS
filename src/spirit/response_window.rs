//! Spirit chat output through one retained kernel Gridpaper document.
//!
//! Reasoning text enters a small bounded queue either as one completed reply
//! or as a coalesced live prefix. This service reveals Spirit's dedicated
//! Gridpaper lease, flies Lilly's software cursor to cell zero, clicks it, and
//! types through her paired virtual keyboard while inference continues.
//! At boot the same real cursor/keyboard path visibly types and erases one
//! short greeting before hiding the session. Hiding after that exercise or a
//! response retains the Gridpaper GPU scene and document allocation.

use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer, with_timeout};
use spin::Mutex;

use crate::r::{
    gridpaper_service::{KernelGridLease, KernelGridPresentation},
    keyboard_control_service::{
        KeyboardControlDevice, KeyboardControlPrincipal, cancel_program, keyboard_is_idle,
        request_keyboard, submit_text,
    },
};

const RESPONSE_QUEUE_CAPACITY: usize = 4;
const SPIRIT_GRID_COLUMNS: u32 = 39 / 2;
const SPIRIT_GRID_ROWS: u32 = 55 / 4;
const SPIRIT_GRID_CELL_CAPACITY: usize = SPIRIT_GRID_COLUMNS as usize * SPIRIT_GRID_ROWS as usize;
const SPIRIT_GRID_SCALE_PERCENT: u16 = 150;
// Gridpaper's kernel-pool frame follows the requested logical grid extent,
// while its scale enlarges cells inside that frame. Give Spirit enough
// retained extent for its 19x13 response viewport at 150% without increasing
// the amount of response text that can be placed in the visible viewport.
const SPIRIT_FRAME_COLUMNS: u32 =
    (SPIRIT_GRID_COLUMNS * SPIRIT_GRID_SCALE_PERCENT as u32).div_ceil(100);
const SPIRIT_FRAME_ROWS: u32 = (SPIRIT_GRID_ROWS * SPIRIT_GRID_SCALE_PERCENT as u32).div_ceil(100);
const RESPONSE_SERVICE_POLL_MS: u64 = 16;
const RESPONSE_READ_MS: u64 = 30_000;
const STARTUP_WARMUP_VISIBLE_MS: u64 = 5_000;
const STARTUP_WARMUP_READY_TIMEOUT_MS: u64 = 30_000;
const STARTUP_WARMUP_TEXT: &str = "Hello from TrueOS §";
const INPUT_BROKER_SETTLE_MS: u64 = 32;
const CURSOR_SETTLE_GRACE_MS: u64 = 250;
const GRID_PRESENTATION_READY_TIMEOUT_MS: u64 = 5_000;
const GRID_TEXT_ACCEPT_TIMEOUT_MS: u64 = 5_000;
const KEYBOARD_STROKE_MS: u32 = 48;
const KEYBOARD_CHUNK_SCALARS: usize = 64;
const SPIRIT_KEYBOARD_LABEL: &str = "Spirit/Lilly chat";

static RESPONSE_QUEUE: Mutex<VecDeque<ResponseRequest>> = Mutex::new(VecDeque::new());
static RESPONSE_WAKE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static SPIRIT_TEXT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RESPONSE_STREAM_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ResponseRequest {
    id: u64,
    turn: u64,
    text: String,
    enqueued_ms: u64,
    revision: u64,
    finished: bool,
    aborted: bool,
    claimed: bool,
    streaming: bool,
}

#[derive(Copy, Clone)]
struct ClaimedResponse {
    id: u64,
    turn: u64,
    enqueued_ms: u64,
    streaming: bool,
}

struct ResponseSnapshot {
    text: String,
    revision: u64,
    finished: bool,
    aborted: bool,
}

#[derive(Copy, Clone)]
enum SpiritGridInputRun {
    StartupWarmup,
    Response(u64),
}

impl SpiritGridInputRun {
    fn is_live(self) -> bool {
        match self {
            Self::StartupWarmup => true,
            Self::Response(id) => response_stream_is_live(id),
        }
    }

    const fn ready_timeout_ms(self) -> u64 {
        match self {
            Self::StartupWarmup => STARTUP_WARMUP_READY_TIMEOUT_MS,
            Self::Response(_) => GRID_PRESENTATION_READY_TIMEOUT_MS,
        }
    }
}

/// Owned producer handle for one live reasoning response.
///
/// Prefix updates only copy into bounded Spirit-owned state and signal the
/// presenter; they never await the cursor, Gridpaper, or virtual keyboard.
/// Dropping an unfinished handle aborts the partial presentation.
#[must_use = "finish the response stream or let Drop abort it"]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct ReasoningResponseStream {
    id: u64,
    turn: u64,
    closed: bool,
}

impl ReasoningResponseStream {
    /// Replace the latest complete text prefix for this response.
    ///
    /// Coalescing snapshots instead of queueing token events bounds ingress
    /// even when inference outruns the emulated keyboard.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn update(&self, text_prefix: &str) -> bool {
        update_response_prefix(self.id, text_prefix)
    }

    /// Seal the final display-safe text into Spirit's ingress and close the
    /// producer. A true result means ingress accepted the final snapshot;
    /// asynchronous Gridpaper/keyboard delivery is reported by the presenter.
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn finish(mut self, text: &str) -> bool {
        let accepted = finish_response(self.id, text);
        self.closed = true;
        accepted
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn turn(&self) -> u64 {
        self.turn
    }
}

impl Drop for ReasoningResponseStream {
    fn drop(&mut self) {
        if !self.closed {
            abort_response(self.id);
        }
    }
}

fn next_response_id() -> u64 {
    let mut current = RESPONSE_STREAM_SEQUENCE.load(Ordering::Acquire);
    loop {
        let next = current.wrapping_add(1).max(1);
        match RESPONSE_STREAM_SEQUENCE.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return next,
            Err(observed) => current = observed,
        }
    }
}

fn push_response_request(request: ResponseRequest) -> Result<(Option<(u64, u64)>, usize), ()> {
    let mut queue = RESPONSE_QUEUE.lock();
    let dropped = if queue.len() >= RESPONSE_QUEUE_CAPACITY {
        let Some(index) = queue.iter().position(|request| !request.claimed) else {
            return Err(());
        };
        queue
            .remove(index)
            .map(|request| (request.id, request.turn))
    } else {
        None
    };
    queue.push_back(request);
    Ok((dropped, queue.len()))
}

fn log_replaced_response(
    dropped: Option<(u64, u64)>,
    newest_id: u64,
    newest_turn: u64,
    queued: usize,
) {
    if let Some((dropped_id, dropped_turn)) = dropped {
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: response ingress replaced unseen_id={} unseen_turn={} newest_id={} newest_turn={} queue={} action=prefer-latest\n",
            dropped_id,
            dropped_turn,
            newest_id,
            newest_turn,
            queued,
        );
    }
}

/// Start one live local-model response in Spirit's bounded presentation
/// ingress. The presenter may begin revealing and focusing its retained window
/// before the first stable word is available.
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn begin_reasoning_response(turn: u64) -> Option<ReasoningResponseStream> {
    let id = next_response_id();
    let request = ResponseRequest {
        id,
        turn,
        text: String::new(),
        enqueued_ms: Instant::now().as_millis(),
        revision: 1,
        finished: false,
        aborted: false,
        claimed: false,
        streaming: true,
    };
    let (dropped, queued) = match push_response_request(request) {
        Ok(result) => result,
        Err(()) => {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: response stream rejected id={} turn={} queue={} reason=all-records-claimed action=completed-fallback\n",
                id,
                turn,
                RESPONSE_QUEUE_CAPACITY,
            );
            return None;
        }
    };
    log_replaced_response(dropped, id, turn, queued);
    RESPONSE_WAKE.signal(());
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: response stream begin id={} turn={} queue={} ingress=coalesced-prefix\n",
        id,
        turn,
        queued,
    );
    Some(ReasoningResponseStream {
        id,
        turn,
        closed: false,
    })
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn update_response_prefix(id: u64, text_prefix: &str) -> bool {
    let accepted = {
        let mut queue = RESPONSE_QUEUE.lock();
        let Some(request) = queue.iter_mut().find(|request| request.id == id) else {
            return false;
        };
        if request.finished || request.aborted {
            return false;
        }
        if request.text == text_prefix {
            return true;
        }
        request.text.clear();
        request.text.push_str(text_prefix);
        request.revision = request.revision.wrapping_add(1).max(1);
        true
    };
    if accepted {
        RESPONSE_WAKE.signal(());
    }
    accepted
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn finish_response(id: u64, text: &str) -> bool {
    let accepted = {
        let mut queue = RESPONSE_QUEUE.lock();
        let Some(request) = queue.iter_mut().find(|request| request.id == id) else {
            return false;
        };
        if request.finished || request.aborted {
            return false;
        }
        request.text.clear();
        request.text.push_str(text);
        request.finished = true;
        request.revision = request.revision.wrapping_add(1).max(1);
        true
    };
    if accepted {
        RESPONSE_WAKE.signal(());
    }
    accepted
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn abort_response(id: u64) {
    let aborted = {
        let mut queue = RESPONSE_QUEUE.lock();
        let Some(index) = queue.iter().position(|request| request.id == id) else {
            return;
        };
        if queue[index].claimed {
            queue[index].aborted = true;
            queue[index].revision = queue[index].revision.wrapping_add(1).max(1);
            Some((queue[index].turn, true))
        } else {
            queue.remove(index).map(|request| (request.turn, false))
        }
    };
    if let Some((turn, claimed)) = aborted {
        RESPONSE_WAKE.signal(());
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: response stream abort id={} turn={} claimed={} action={}\n",
            id,
            turn,
            claimed as u8,
            if claimed { "cancel-active" } else { "drop-unseen" },
        );
    }
}

/// Copy one completed local-model reply into Spirit's bounded presentation
/// ingress. Oldest unseen replies yield to the newest if inference outruns the
/// one user-facing Gridpaper document.
pub(crate) fn enqueue_reasoning_response(turn: u64, text: &str) -> bool {
    let id = next_response_id();
    let request = ResponseRequest {
        id,
        turn,
        text: sanitize_response(text),
        enqueued_ms: Instant::now().as_millis(),
        revision: 1,
        finished: true,
        aborted: false,
        claimed: false,
        streaming: false,
    };
    let (dropped, queued) = match push_response_request(request) {
        Ok(result) => result,
        Err(()) => return false,
    };
    log_replaced_response(dropped, id, turn, queued);
    RESPONSE_WAKE.signal(());
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
    sanitize_response_inner(text, true)
}

fn sanitize_response_inner(text: &str, empty_fallback: bool) -> String {
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
    if words.is_empty() && empty_fallback {
        words.push("(no".chars().collect());
        words.push("text".chars().collect());
        words.push("response)".chars().collect());
    }
    if words.is_empty() {
        return String::new();
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

/// Only commit words whose following word has started while generation is
/// live. The tokenizer may still remove a trailing ASCII space when the next
/// token is punctuation, so the word immediately before trailing whitespace
/// is not append-safe yet. Reserving the final row also keeps later overflow
/// ellipsis from rewriting text which Lilly has already typed.
fn sanitize_stream_snapshot(text: &str, finished: bool) -> String {
    if finished {
        return sanitize_response(text);
    }

    let search_end = text.trim_end_matches(char::is_whitespace).len();
    let stable_end = text[..search_end]
        .char_indices()
        .filter(|(_, ch)| ch.is_whitespace())
        .map(|(offset, ch)| offset + ch.len_utf8())
        .next_back()
        .unwrap_or(0);
    let mut formatted = sanitize_response_inner(&text[..stable_end], false);
    let live_rows = (SPIRIT_GRID_ROWS as usize).saturating_sub(1);
    if live_rows == 0 {
        formatted.clear();
    } else if let Some((offset, _)) = formatted.match_indices('\n').nth(live_rows - 1) {
        formatted.truncate(offset);
    }
    formatted
}

fn claim_latest_response() -> Option<ClaimedResponse> {
    let mut queue = RESPONSE_QUEUE.lock();
    queue.retain(|request| request.claimed || !request.aborted);
    let latest_id = queue
        .iter()
        .rfind(|request| !request.claimed)
        .map(|request| request.id)?;
    queue.retain(|request| request.claimed || request.id == latest_id);
    let request = queue.iter_mut().find(|request| request.id == latest_id)?;
    request.claimed = true;
    Some(ClaimedResponse {
        id: request.id,
        turn: request.turn,
        enqueued_ms: request.enqueued_ms,
        streaming: request.streaming,
    })
}

fn response_snapshot(id: u64, last_revision: u64) -> Option<ResponseSnapshot> {
    let queue = RESPONSE_QUEUE.lock();
    let request = queue.iter().find(|request| request.id == id)?;
    if request.revision == last_revision {
        return None;
    }
    Some(ResponseSnapshot {
        text: request.text.clone(),
        revision: request.revision,
        finished: request.finished,
        aborted: request.aborted,
    })
}

fn response_stream_is_live(id: u64) -> bool {
    RESPONSE_QUEUE
        .lock()
        .iter()
        .find(|request| request.id == id)
        .is_some_and(|request| !request.aborted)
}

fn retire_response(id: u64) {
    let mut queue = RESPONSE_QUEUE.lock();
    if let Some(index) = queue.iter().position(|request| request.id == id) {
        let _ = queue.remove(index);
    }
}

fn response_queue_has_pending() -> bool {
    RESPONSE_QUEUE
        .lock()
        .iter()
        .any(|request| !request.claimed && !request.aborted)
}

async fn wait_for_pending_response(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if response_queue_has_pending() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let remaining = deadline.saturating_duration_since(now);
        let _ = with_timeout(remaining, RESPONSE_WAKE.wait()).await;
    }
}

fn presentation_is_ready(presentation: KernelGridPresentation) -> bool {
    grid_window_publish_serial(presentation.window).is_some()
}

fn grid_window_publish_serial(window: crate::ui4::WindowId) -> Option<u64> {
    let Some(output) = crate::ui4::OutputId::from_slot(0) else {
        return None;
    };
    crate::ui4::visible_windows_for_output(output)
        .into_iter()
        .find_map(|snapshot| {
            (snapshot.owner == crate::ui4::WindowOwner::GRIDPAPER_SERVICE
                && snapshot.id == window
                && snapshot.state == crate::ui4::WindowState::Ready
                && snapshot.publish_serial != 0
                && snapshot.damage.is_none()
                && snapshot.placement.visible)
                .then_some(snapshot.publish_serial)
        })
}

async fn wait_for_grid_window_publish_after(
    window: crate::ui4::WindowId,
    previous_serial: u64,
) -> Result<u64, &'static str> {
    let deadline = Instant::now() + Duration::from_millis(STARTUP_WARMUP_READY_TIMEOUT_MS);
    loop {
        if let Some(serial) = grid_window_publish_serial(window)
            && serial != previous_serial
        {
            return Ok(serial);
        }
        if Instant::now() >= deadline {
            return Err("gridpaper-startup-publish-timeout");
        }
        Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
    }
}

async fn wait_for_grid_window_acknowledged(
    window: crate::ui4::WindowId,
) -> Result<u64, &'static str> {
    let deadline = Instant::now() + Duration::from_millis(STARTUP_WARMUP_READY_TIMEOUT_MS);
    loop {
        if let Some(serial) = grid_window_publish_serial(window) {
            return Ok(serial);
        }
        if Instant::now() >= deadline {
            return Err("gridpaper-startup-ack-timeout");
        }
        Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
    }
}

async fn wait_for_ready_presentation(
    lease: KernelGridLease,
    generation: u64,
    input_run: SpiritGridInputRun,
) -> Result<KernelGridPresentation, &'static str> {
    let deadline = Instant::now() + Duration::from_millis(input_run.ready_timeout_ms());
    loop {
        if !input_run.is_live() {
            return Err("response-aborted");
        }
        if let Some(presentation) = crate::r::gridpaper_service::kernel_grid_presentation(lease)
            && presentation.published_generation == generation
            && presentation_is_ready(presentation)
        {
            return Ok(presentation);
        }
        if Instant::now() >= deadline {
            return Err("gridpaper-ready-timeout");
        }
        Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
    }
}

async fn focus_and_click_cell_zero(
    presentation: KernelGridPresentation,
    input_run: SpiritGridInputRun,
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
        if !input_run.is_live() {
            return Err("response-aborted");
        }
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
    if !input_run.is_live() {
        return Err("response-aborted");
    }
    let source = super::lilly_cursor::selection_source().map_err(|_| "lilly-cursor-source")?;
    crate::ui4::select_window_for_cursor(
        source,
        crate::ui4::WindowOwner::GRIDPAPER_SERVICE,
        presentation.window,
    )
    .map_err(|_| "gridpaper-focus")?;
    super::lilly_cursor::queue_primary_click().map_err(|_| "gridpaper-cell-zero-click")?;
    let click_deadline = Instant::now() + Duration::from_millis(CURSOR_SETTLE_GRACE_MS);
    loop {
        if !input_run.is_live() {
            return Err("response-aborted");
        }
        match super::lilly_cursor::window_approach_complete() {
            Ok(true) => break,
            Ok(false) if Instant::now() < click_deadline => {
                Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
            }
            Ok(false) => return Err("gridpaper-click-timeout"),
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

fn cancel_response_keyboard(keyboard: KeyboardControlDevice) {
    let _ = cancel_program(KeyboardControlPrincipal::Kernel, keyboard.handle);
}

async fn wait_for_keyboard_idle(
    keyboard: KeyboardControlDevice,
    input_run: SpiritGridInputRun,
) -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_millis(input_run.ready_timeout_ms());
    loop {
        if !input_run.is_live() {
            cancel_response_keyboard(keyboard);
            return Err("response-aborted");
        }
        match keyboard_is_idle(KeyboardControlPrincipal::Kernel, keyboard.handle) {
            Ok(true) => return Ok(()),
            Ok(false) if Instant::now() < deadline => {
                Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
            }
            Ok(false) => {
                cancel_response_keyboard(keyboard);
                return Err("lilly-keyboard-timeout");
            }
            Err(_) => return Err("lilly-keyboard-status"),
        }
    }
}

async fn type_response(
    keyboard: KeyboardControlDevice,
    input_run: SpiritGridInputRun,
    text: &str,
    clear_queue: bool,
) -> Result<usize, &'static str> {
    let scalars = text.chars().collect::<Vec<_>>();
    let mut typed = 0usize;
    for (chunk_index, chunk) in scalars.chunks(KEYBOARD_CHUNK_SCALARS).enumerate() {
        wait_for_keyboard_idle(keyboard, input_run).await?;
        let text = chunk.iter().collect::<String>();
        submit_text(
            KeyboardControlPrincipal::Kernel,
            keyboard.handle,
            text.as_str(),
            KEYBOARD_STROKE_MS,
            clear_queue && chunk_index == 0,
        )
        .map_err(|_| "lilly-keyboard-submit")?;
        typed = typed.saturating_add(chunk.iter().filter(|ch| !matches!(ch, '\n' | '\r')).count());
    }
    wait_for_keyboard_idle(keyboard, input_run).await?;
    Ok(typed)
}

async fn wait_for_grid_text_acceptance(
    lease: KernelGridLease,
    accepted_base: u64,
    typed_cells: usize,
) -> Result<(), &'static str> {
    let typed_cells = u64::try_from(typed_cells).map_err(|_| "gridpaper-text-count-range")?;
    let expected_cells = accepted_base.saturating_add(typed_cells);
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

async fn wait_for_grid_keyboard_edits(
    lease: KernelGridLease,
    accepted_base: u64,
    edits: usize,
) -> Result<(), &'static str> {
    let edits = u64::try_from(edits).map_err(|_| "gridpaper-edit-count-range")?;
    let expected_edits = accepted_base.saturating_add(edits);
    let deadline = Instant::now() + Duration::from_millis(STARTUP_WARMUP_READY_TIMEOUT_MS);
    loop {
        match crate::r::gridpaper_service::kernel_grid_accepted_keyboard_edits(lease) {
            Some(accepted) if accepted >= expected_edits => return Ok(()),
            Some(_) if Instant::now() < deadline => {
                Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
            }
            Some(_) => return Err("gridpaper-edit-accept-timeout"),
            None => return Err("gridpaper-presentation-lost"),
        }
    }
}

async fn wait_for_grid_published_keyboard_edits(
    lease: KernelGridLease,
    published_base: u64,
    edits: usize,
) -> Result<(), &'static str> {
    let edits = u64::try_from(edits).map_err(|_| "gridpaper-edit-count-range")?;
    let expected_edits = published_base.saturating_add(edits);
    let deadline = Instant::now() + Duration::from_millis(STARTUP_WARMUP_READY_TIMEOUT_MS);
    loop {
        match crate::r::gridpaper_service::kernel_grid_published_keyboard_edits(lease) {
            Some(published) if published >= expected_edits => return Ok(()),
            Some(_) if Instant::now() < deadline => {
                Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
            }
            Some(_) => return Err("gridpaper-edit-publish-timeout"),
            None => return Err("gridpaper-presentation-lost"),
        }
    }
}

async fn request_spirit_grid() -> KernelGridLease {
    let mut last_error = None;
    loop {
        match crate::r::gridpaper_service::request_spirit_response_grid(
            SPIRIT_FRAME_COLUMNS,
            SPIRIT_FRAME_ROWS,
            SPIRIT_GRID_SCALE_PERCENT,
        ) {
            Ok(lease) => return lease,
            Err(error) => {
                if last_error != Some(error) {
                    crate::log_warn!(
                        target: "gfx";
                        "trueos-spirit: response Gridpaper lease waiting error={:?} frame_grid={}x{} response_grid={}x{} scale={} retry_ms=250\n",
                        error,
                        SPIRIT_FRAME_COLUMNS,
                        SPIRIT_FRAME_ROWS,
                        SPIRIT_GRID_COLUMNS,
                        SPIRIT_GRID_ROWS,
                        SPIRIT_GRID_SCALE_PERCENT,
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

async fn wait_for_hidden_grid(
    lease: KernelGridLease,
    window: crate::ui4::WindowId,
) -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_millis(GRID_PRESENTATION_READY_TIMEOUT_MS);
    loop {
        let broker_window_live = crate::ui4::OutputId::from_slot(0).is_some_and(|output| {
            crate::ui4::visible_windows_for_output(output)
                .iter()
                .any(|snapshot| snapshot.id == window)
        });
        if crate::r::gridpaper_service::kernel_grid_presentation(lease).is_none()
            && !broker_window_live
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("gridpaper-hide-timeout");
        }
        Timer::after(Duration::from_millis(RESPONSE_SERVICE_POLL_MS)).await;
    }
}

async fn run_visible_startup_warmup(
    lease: KernelGridLease,
    keyboard: KeyboardControlDevice,
) -> Result<(), &'static str> {
    use crate::intel::gpu_font::GpuFontFace;

    // The greeting uses the embedded baseline face. Filesystem-backed future
    // faces warm independently and are not an admission dependency here.
    crate::intel::gpu_font::ensure_font_face_available(GpuFontFace::Default)?;
    let generation = crate::r::gridpaper_service::reset_and_show_kernel_grid(lease)
        .map_err(|_| "gridpaper-startup-reset")?;
    let input_run = SpiritGridInputRun::StartupWarmup;
    let presentation = wait_for_ready_presentation(lease, generation, input_run).await?;
    let greeting_publish_base = grid_window_publish_serial(presentation.window)
        .ok_or("gridpaper-startup-window-not-ready")?;
    focus_and_click_cell_zero(presentation, input_run).await?;
    let text_accepted_base = crate::r::gridpaper_service::kernel_grid_accepted_text_cells(lease)
        .ok_or("gridpaper-startup-accept-counter-missing")?;
    let edit_accepted_base =
        crate::r::gridpaper_service::kernel_grid_accepted_keyboard_edits(lease)
            .ok_or("gridpaper-startup-edit-counter-missing")?;
    let edit_published_base =
        crate::r::gridpaper_service::kernel_grid_published_keyboard_edits(lease)
            .ok_or("gridpaper-startup-publish-counter-missing")?;
    let typed = type_response(keyboard, input_run, STARTUP_WARMUP_TEXT, true).await?;
    wait_for_grid_text_acceptance(lease, text_accepted_base, typed).await?;
    wait_for_grid_keyboard_edits(lease, edit_accepted_base, typed).await?;
    wait_for_grid_published_keyboard_edits(lease, edit_published_base, typed).await?;
    let greeting_publish_serial =
        wait_for_grid_window_publish_after(presentation.window, greeting_publish_base).await?;
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: startup Gridpaper greeting typed window={} generation={} publish_serial={} text={:?} scalars={} dwell_ms={} path=lilly-cursor+paired-vkeyboard->ui4->gridpaper action=visible-warmup\n",
        presentation.window.raw(),
        generation,
        greeting_publish_serial,
        STARTUP_WARMUP_TEXT,
        typed,
        STARTUP_WARMUP_VISIBLE_MS,
    );

    Timer::after(Duration::from_millis(STARTUP_WARMUP_VISIBLE_MS)).await;
    let mut backspaces = String::new();
    for _ in STARTUP_WARMUP_TEXT.chars() {
        backspaces.push('\u{0008}');
    }
    let erase_edit_base = crate::r::gridpaper_service::kernel_grid_accepted_keyboard_edits(lease)
        .ok_or("gridpaper-startup-edit-counter-missing")?;
    let erase_published_base =
        crate::r::gridpaper_service::kernel_grid_published_keyboard_edits(lease)
            .ok_or("gridpaper-startup-publish-counter-missing")?;
    let erase_publish_base = wait_for_grid_window_acknowledged(presentation.window).await?;
    let backspace_count = type_response(keyboard, input_run, backspaces.as_str(), false).await?;
    wait_for_grid_keyboard_edits(lease, erase_edit_base, backspace_count).await?;
    wait_for_grid_published_keyboard_edits(lease, erase_published_base, backspace_count).await?;
    let erase_publish_serial =
        wait_for_grid_window_publish_after(presentation.window, erase_publish_base).await?;
    crate::r::gridpaper_service::hide_kernel_grid(lease).map_err(|_| "gridpaper-startup-hide")?;
    wait_for_hidden_grid(lease, presentation.window).await?;
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: startup Gridpaper warm complete window={} typed_scalars={} backspaces={} greeting_publish_serial={} erase_publish_serial={} visible_ms={} post_warm=hidden-retained display_session=none display_window=none display_visible=0 plane_release=pending-or-parked gpu_frame=retained gpu_scene=retained\n",
        presentation.window.raw(),
        typed,
        backspace_count,
        greeting_publish_serial,
        erase_publish_serial,
        STARTUP_WARMUP_VISIBLE_MS,
    );
    Ok(())
}

async fn present_claimed_response(
    lease: KernelGridLease,
    keyboard: KeyboardControlDevice,
    request: ClaimedResponse,
) -> bool {
    let generation = match crate::r::gridpaper_service::reset_and_show_kernel_grid(lease) {
        Ok(generation) => generation,
        Err(error) => {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: response dropped id={} turn={} reason=gridpaper-reset error={:?}\n",
                request.id,
                request.turn,
                error,
            );
            return false;
        }
    };

    let input_run = SpiritGridInputRun::Response(request.id);
    let presentation = match wait_for_ready_presentation(lease, generation, input_run).await {
        Ok(presentation) => presentation,
        Err(reason) => {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: response dropped id={} turn={} reason={}\n",
                request.id,
                request.turn,
                reason,
            );
            cancel_response_keyboard(keyboard);
            let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
            return false;
        }
    };
    if !response_stream_is_live(request.id) {
        cancel_response_keyboard(keyboard);
        let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
        return false;
    }
    if let Err(reason) = focus_and_click_cell_zero(presentation, input_run).await {
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: response dropped id={} turn={} window={} reason={}\n",
            request.id,
            request.turn,
            presentation.window.raw(),
            reason,
        );
        cancel_response_keyboard(keyboard);
        let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
        return false;
    }

    let Some(accepted_base) = crate::r::gridpaper_service::kernel_grid_accepted_text_cells(lease)
    else {
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: response dropped id={} turn={} window={} reason=gridpaper-accept-counter-missing\n",
            request.id,
            request.turn,
            presentation.window.raw(),
        );
        let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
        return false;
    };

    let mut emitted = String::new();
    let mut last_revision = 0u64;
    let mut observed_revisions = 0u64;
    let mut typed = 0usize;
    let mut first_commit_ms = None;
    loop {
        let Some(snapshot) = response_snapshot(request.id, last_revision) else {
            if !response_stream_is_live(request.id) {
                cancel_response_keyboard(keyboard);
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: response typing stopped id={} turn={} window={} reason=response-lost\n",
                    request.id,
                    request.turn,
                    presentation.window.raw(),
                );
                let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
                return false;
            }
            RESPONSE_WAKE.wait().await;
            continue;
        };
        last_revision = snapshot.revision;
        observed_revisions = observed_revisions.saturating_add(1);
        if snapshot.aborted {
            cancel_response_keyboard(keyboard);
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: response typing stopped id={} turn={} window={} cells={} reason=response-aborted\n",
                request.id,
                request.turn,
                presentation.window.raw(),
                typed,
            );
            let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
            return false;
        }

        let formatted = sanitize_stream_snapshot(snapshot.text.as_str(), snapshot.finished);
        let Some(delta) = formatted.strip_prefix(emitted.as_str()) else {
            cancel_response_keyboard(keyboard);
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: response typing stopped id={} turn={} window={} revision={} reason=non-monotonic-formatted-prefix emitted_bytes={} next_bytes={} action=completed-fallback-if-producer-open\n",
                request.id,
                request.turn,
                presentation.window.raw(),
                snapshot.revision,
                emitted.len(),
                formatted.len(),
            );
            let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
            return false;
        };
        if !delta.is_empty() {
            if first_commit_ms.is_none() {
                let latency_ms = Instant::now()
                    .as_millis()
                    .saturating_sub(request.enqueued_ms);
                first_commit_ms = Some(latency_ms);
                crate::log_info!(
                    target: "gfx";
                    "trueos-spirit: response stream first-commit id={} turn={} window={} latency_ms={} scalars={} revision={} path=coalesced-prefix->paired-vkeyboard\n",
                    request.id,
                    request.turn,
                    presentation.window.raw(),
                    latency_ms,
                    delta.chars().count(),
                    snapshot.revision,
                );
            }
            match type_response(keyboard, input_run, delta, typed == 0).await {
                Ok(chunk_typed) => {
                    typed = typed.saturating_add(chunk_typed);
                    emitted = formatted;
                }
                Err(reason) => {
                    crate::log_warn!(
                        target: "gfx";
                        "trueos-spirit: response typing stopped id={} turn={} window={} cells={} reason={}\n",
                        request.id,
                        request.turn,
                        presentation.window.raw(),
                        typed,
                        reason,
                    );
                    let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
                    return false;
                }
            }
        } else {
            emitted = formatted;
        }

        if !snapshot.finished {
            continue;
        }
        let accepted = wait_for_grid_text_acceptance(lease, accepted_base, typed).await;
        match accepted.and_then(|()| {
            crate::r::gridpaper_service::enable_spirit_response_rainbow_motion(lease)
                .map_err(|_| "gridpaper-motion-enable")
        }) {
            Ok(animation_serial) => crate::log_info!(
                target: "gfx";
                "trueos-spirit: response typed id={} turn={} window={} cells={} streaming={} revisions={} first_commit_ms={} latency_ms={} path=paired-vkeyboard->ui4->gridpaper-cell-patches wrap=whitespace-before-word palette=rainbow animation_serial={} animation=cpp-scale-sine-0.85..1.15 gpu-scene=resident read_ms={}\n",
                request.id,
                request.turn,
                presentation.window.raw(),
                typed,
                request.streaming as u8,
                observed_revisions,
                first_commit_ms.unwrap_or(0),
                Instant::now().as_millis().saturating_sub(request.enqueued_ms),
                animation_serial,
                RESPONSE_READ_MS,
            ),
            Err(reason) => crate::log_warn!(
                target: "gfx";
                "trueos-spirit: response typed without motion id={} turn={} window={} cells={} streaming={} revisions={} reason={} retained_static_rainbow=1\n",
                request.id,
                request.turn,
                presentation.window.raw(),
                typed,
                request.streaming as u8,
                observed_revisions,
                reason,
            ),
        }
        return true;
    }
}

#[embassy_executor::task]
pub(crate) async fn spirit_response_window_service_task(expected_slot: u32) {
    let keyboard = request_spirit_keyboard().await;
    super::dobby_ui::register_lilly_keyboard(keyboard);
    // Publish Lilly's paired keyboard before waiting on the retained response
    // Gridpaper lease. Dobby's UI4 capability is independent of whether that
    // presentation service is temporarily at capacity.
    let lease = request_spirit_grid().await;
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: response Gridpaper service online assigned_slot={} current_slot={} frame_grid={}x{} response_grid={}x{} cells={} scale={} ownership=kernel-dedicated cursor=Spirit/Lilly keyboard_slot={} input=cell-zero-click+paired-vkeyboard ingress=completed+coalesced-live-prefix wrap=whitespace-before-word style=rainbow-palette+cpp-scale-0.85..1.15 response_hide_after_ms={} startup=visible-type+backspace startup_text={:?} startup_visible_ms={} post_startup=hidden-retained no-blueprint-vm=1\n",
        expected_slot,
        crate::percpu::current_slot(),
        SPIRIT_FRAME_COLUMNS,
        SPIRIT_FRAME_ROWS,
        SPIRIT_GRID_COLUMNS,
        SPIRIT_GRID_ROWS,
        SPIRIT_GRID_CELL_CAPACITY,
        SPIRIT_GRID_SCALE_PERCENT,
        keyboard.slot_id,
        RESPONSE_READ_MS,
        STARTUP_WARMUP_TEXT,
        STARTUP_WARMUP_VISIBLE_MS,
    );
    super::dobby_ui::acquire_response_io(keyboard).await;
    let startup_result = run_visible_startup_warmup(lease, keyboard).await;
    super::dobby_ui::release_response_io(keyboard);
    if let Err(reason) = startup_result {
        cancel_response_keyboard(keyboard);
        let _ = crate::r::gridpaper_service::hide_kernel_grid(lease);
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: startup Gridpaper warm incomplete reason={} action=hide+continue-response-service\n",
            reason,
        );
    }

    loop {
        let request = match claim_latest_response() {
            Some(request) => request,
            None => {
                RESPONSE_WAKE.wait().await;
                continue;
            }
        };
        super::dobby_ui::acquire_response_io(keyboard).await;
        let completed = present_claimed_response(lease, keyboard, request).await;
        super::dobby_ui::release_response_io(keyboard);
        retire_response(request.id);
        if !completed {
            continue;
        }

        if !wait_for_pending_response(Duration::from_millis(RESPONSE_READ_MS)).await {
            // TODO(chat): replace this fixed reading timeout with an explicit
            // conversation/window lifecycle once chat history and dismissal
            // semantics are decided.
            if let Err(error) = crate::r::gridpaper_service::hide_kernel_grid(lease) {
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: response hide deferred id={} turn={} error={:?}\n",
                    request.id,
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
    fn startup_warmup_uses_one_exact_grid_row_and_one_backspace_per_scalar() {
        assert_eq!(STARTUP_WARMUP_TEXT, "Hello from TrueOS §");
        assert_eq!(STARTUP_WARMUP_TEXT.chars().count(), 19);
        assert_eq!(sanitize_response(STARTUP_WARMUP_TEXT), STARTUP_WARMUP_TEXT);
        assert!(STARTUP_WARMUP_TEXT.chars().count() <= SPIRIT_GRID_COLUMNS as usize);
        let backspaces = STARTUP_WARMUP_TEXT
            .chars()
            .map(|_| '\u{0008}')
            .collect::<String>();
        assert_eq!(backspaces.chars().count(), STARTUP_WARMUP_TEXT.chars().count());
    }

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

    #[test]
    fn live_response_commits_append_safe_words_and_flushes_the_final_tail() {
        assert_eq!(sanitize_stream_snapshot("Hello", false), "");
        assert_eq!(sanitize_stream_snapshot("Hello ", false), "");
        assert_eq!(sanitize_stream_snapshot("Hello wor", false), "Hello");
        assert_eq!(sanitize_stream_snapshot("Hello world ", false), "Hello");
        assert_eq!(sanitize_stream_snapshot("Hello world a", false), "Hello world");
        assert_eq!(sanitize_stream_snapshot("Hello world", true), "Hello world");
    }

    #[test]
    fn live_response_prefix_remains_monotonic_across_cleanup_and_wrap() {
        let before_cleanup = sanitize_stream_snapshot("aaaaaaaaaa bbbbbbbb ", false);
        let after_cleanup = sanitize_stream_snapshot("aaaaaaaaaa bbbbbbbb, c", false);
        assert_eq!(before_cleanup, "aaaaaaaaaa");
        assert_eq!(after_cleanup, "aaaaaaaaaa\nbbbbbbbb,");
        assert!(after_cleanup.starts_with(before_cleanup.as_str()));
    }

    #[test]
    fn live_response_reserves_the_final_row_for_append_safe_truncation() {
        let input = alloc::format!("{}tail", "abcdefghij ".repeat(30));
        let live = sanitize_stream_snapshot(input.as_str(), false);
        let finished = sanitize_stream_snapshot(input.as_str(), true);
        assert!(live.lines().count() <= SPIRIT_GRID_ROWS.saturating_sub(1) as usize);
        assert!(finished.starts_with(live.as_str()));
    }
}
