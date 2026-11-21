//! Native Lilly ingress and resident-frame adapter.
use spin::Mutex;
use trueos_time::Instant;

use super::lilly::{self, LillyResidentFrame};
use super::lilly_sequencer::{Contextual, IdleMode, Playback, Sequencer};

static SEQUENCER: Mutex<Option<Sequencer>> = Mutex::new(None);

#[derive(Debug)]
pub(crate) enum LillyProtocolError {
    UnknownContext,
    RingFull,
    MissingFrame(&'static str),
}

pub(super) struct LillyScheduledAnimation {
    pub(super) boundary_ms: u64,
    playback: Playback,
    frames: [LillyResidentFrame; 9],
}

impl LillyScheduledAnimation {
    pub(super) fn frame_at_elapsed(&self, elapsed_ms: u64) -> LillyResidentFrame {
        self.frames[self.playback.frame_index(elapsed_ms)]
    }
}

pub(crate) fn enqueue_contextual_word(word: &str) -> Result<(), LillyProtocolError> {
    let action = Contextual::from_word(word).ok_or(LillyProtocolError::UnknownContext)?;
    let mut state = SEQUENCER.lock();
    let sequencer = state.get_or_insert_with(|| Sequencer::new(crate::tyche::soft_rng()));
    if !sequencer.enqueue(action) {
        return Err(LillyProtocolError::RingFull);
    }
    Ok(())
}

/// Host-side control for the future Typing, Sound, Reading and Talk checkboxes.
pub(crate) fn set_idle_mode(mode: IdleMode) {
    SEQUENCER
        .lock()
        .get_or_insert_with(|| Sequencer::new(crate::tyche::soft_rng()))
        .set_mode(mode);
}

pub(crate) fn idle_mode() -> IdleMode {
    SEQUENCER
        .lock()
        .as_ref()
        .map_or(IdleMode::Normal, Sequencer::mode)
}

pub(super) fn set_window_sound(selected: bool) {
    let mut state = SEQUENCER.lock();
    if selected {
        state
            .get_or_insert_with(|| Sequencer::new(crate::tyche::soft_rng()))
            .set_window_sound(true);
    } else if let Some(sequencer) = state.as_mut() {
        sequencer.set_window_sound(false);
    }
}

/// Keep the legacy ABI callable so older guests can run during rollout.
pub(crate) fn enqueue_presentation_word(word: &str) -> Result<usize, LillyProtocolError> {
    crate::log_warn!(target: "spirit";
        "trueos-spirit: deprecated emotion signal word={:?} action=ignored\n", word);
    Ok(0)
}

pub(super) fn shift_deadlines(duration_ms: u64) {
    if let Some(sequencer) = SEQUENCER.lock().as_mut() {
        sequencer.shift_deadlines(duration_ms);
    }
}

pub(super) fn next_animation() -> Result<LillyScheduledAnimation, LillyProtocolError> {
    let now_ms = Instant::now().as_millis();
    let collapse = super::codex_handoff::needs_crossed();
    let playback = {
        let mut state = SEQUENCER.lock();
        state
            .get_or_insert_with(|| Sequencer::new(crate::tyche::soft_rng()))
            .next(now_ms, collapse)
    };
    let first_path = playback.frames[0];
    let first =
        lilly::resident_frame(first_path).ok_or(LillyProtocolError::MissingFrame(first_path))?;
    let mut frames = [first; 9];
    for (index, path) in playback.frames.iter().copied().enumerate().skip(1) {
        frames[index] =
            lilly::resident_frame(path).ok_or(LillyProtocolError::MissingFrame(path))?;
    }
    if playback.crossed_for_collapse {
        super::codex_handoff::crossed_ready();
    }
    Ok(LillyScheduledAnimation {
        boundary_ms: playback.duration_ms(),
        playback,
        frames,
    })
}
