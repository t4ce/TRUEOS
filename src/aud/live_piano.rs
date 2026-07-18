//! Live network-MIDI piano synth service.
//!
//! UDP note edges update shared key state. The optional Tinyaudio piano source
//! can render that state, but this module does not own or write an audio device.

use spin::Mutex;

use super::synth::{Envelope, SAMPLE_RATE, SynthEngine, Waveform};

const MIDI_NOTE_COUNT: usize = 128;
const FAST_TAP_HOLD_MS: u32 = 24;
const FAST_TAP_HOLD_FRAMES: usize = (SAMPLE_RATE as usize * FAST_TAP_HOLD_MS as usize) / 1_000;

#[derive(Clone, Copy)]
struct LiveNote {
    down: bool,
    velocity: u8,
    strike_seq: u32,
    release_seq: u32,
}

impl LiveNote {
    const fn up() -> Self {
        Self {
            down: false,
            velocity: 0,
            strike_seq: 0,
            release_seq: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct LivePianoState {
    seq: u32,
    notes: [LiveNote; MIDI_NOTE_COUNT],
}

impl LivePianoState {
    const fn empty() -> Self {
        Self {
            seq: 0,
            notes: [LiveNote::up(); MIDI_NOTE_COUNT],
        }
    }
}

static LIVE_STATE: Mutex<LivePianoState> = Mutex::new(LivePianoState::empty());

pub fn note_on(note: u8, velocity: u8) {
    let idx = note as usize;
    if idx >= MIDI_NOTE_COUNT {
        return;
    }

    let mut state = LIVE_STATE.lock();
    let seq = state.seq.wrapping_add(1);
    state.notes[idx] = LiveNote {
        down: true,
        velocity: velocity.max(1),
        strike_seq: seq,
        release_seq: state.notes[idx].release_seq,
    };
    state.seq = seq;
}

pub fn note_off(note: u8) {
    let idx = note as usize;
    if idx >= MIDI_NOTE_COUNT {
        return;
    }

    let mut state = LIVE_STATE.lock();
    let seq = state.seq.wrapping_add(1);
    state.notes[idx].down = false;
    state.notes[idx].release_seq = seq;
    state.seq = seq;
}

pub fn all_notes_off() {
    let mut state = LIVE_STATE.lock();
    let seq = state.seq.wrapping_add(1);
    for note in &mut state.notes {
        note.down = false;
        note.velocity = 0;
        note.release_seq = seq;
    }
    state.seq = seq;
}

fn snapshot() -> LivePianoState {
    *LIVE_STATE.lock()
}

fn has_held_note(notes: &[LiveNote; MIDI_NOTE_COUNT]) -> bool {
    notes.iter().any(|note| note.down)
}

fn configure_engine() -> SynthEngine {
    let mut engine = SynthEngine::new();
    engine.waveform = Waveform::TriSine;
    engine.envelope = Envelope::new(3, 45, 72, 80);
    engine.master_volume = 96;
    engine
}

pub(crate) struct LivePianoRenderSource {
    engine: SynthEngine,
    active: [bool; MIDI_NOTE_COUNT],
    last_velocity: [u8; MIDI_NOTE_COUNT],
    last_strike_seq: [u32; MIDI_NOTE_COUNT],
    last_release_seq: [u32; MIDI_NOTE_COUNT],
    release_after_frames: [usize; MIDI_NOTE_COUNT],
    last_seq: u32,
}

impl LivePianoRenderSource {
    pub(crate) fn new() -> Self {
        Self {
            engine: configure_engine(),
            active: [false; MIDI_NOTE_COUNT],
            last_velocity: [0; MIDI_NOTE_COUNT],
            last_strike_seq: [0; MIDI_NOTE_COUNT],
            last_release_seq: [0; MIDI_NOTE_COUNT],
            release_after_frames: [0; MIDI_NOTE_COUNT],
            last_seq: 0,
        }
    }

    pub(crate) fn render_into(&mut self, buffer: &mut [i16], frames: usize) -> bool {
        let state = snapshot();
        if state.seq != self.last_seq {
            apply_note_state(
                &mut self.engine,
                &mut self.active,
                &mut self.last_velocity,
                &mut self.last_strike_seq,
                &mut self.last_release_seq,
                &mut self.release_after_frames,
                &state.notes,
            );
            self.last_seq = state.seq;
        }

        let active_piano = self.engine.active_voice_count() != 0
            || has_held_note(&state.notes)
            || has_pending_release(&self.release_after_frames);
        if active_piano {
            self.engine.render(buffer, frames);
            finish_pending_releases(
                &mut self.engine,
                &mut self.active,
                &mut self.last_velocity,
                &mut self.release_after_frames,
                frames,
            );
        }
        active_piano
    }
}

fn apply_note_state(
    engine: &mut SynthEngine,
    active: &mut [bool; MIDI_NOTE_COUNT],
    last_velocity: &mut [u8; MIDI_NOTE_COUNT],
    last_strike_seq: &mut [u32; MIDI_NOTE_COUNT],
    last_release_seq: &mut [u32; MIDI_NOTE_COUNT],
    release_after_frames: &mut [usize; MIDI_NOTE_COUNT],
    notes: &[LiveNote; MIDI_NOTE_COUNT],
) {
    for note in 0..MIDI_NOTE_COUNT {
        let wanted = notes[note];
        let struck = wanted.strike_seq != last_strike_seq[note];
        let released = wanted.release_seq != last_release_seq[note];

        if struck {
            if active[note] {
                engine.note_off(note as u8);
            }
            engine.note_on(note as u8, wanted.velocity);
            active[note] = true;
            last_velocity[note] = wanted.velocity;
            last_strike_seq[note] = wanted.strike_seq;
            release_after_frames[note] = if wanted.down { 0 } else { FAST_TAP_HOLD_FRAMES };
        }

        if released {
            last_release_seq[note] = wanted.release_seq;
            if !struck && active[note] {
                engine.note_off(note as u8);
                active[note] = false;
                last_velocity[note] = 0;
                release_after_frames[note] = 0;
            } else if struck && !wanted.down {
                release_after_frames[note] = FAST_TAP_HOLD_FRAMES;
            }
        }

        if wanted.down && !active[note] {
            engine.note_on(note as u8, wanted.velocity);
            active[note] = true;
            last_velocity[note] = wanted.velocity;
            release_after_frames[note] = 0;
        } else if !wanted.down && active[note] && !struck && !released {
            engine.note_off(note as u8);
            active[note] = false;
            last_velocity[note] = 0;
        } else if wanted.down && wanted.velocity != last_velocity[note] {
            last_velocity[note] = wanted.velocity;
        }
    }
}

fn has_pending_release(release_after_frames: &[usize; MIDI_NOTE_COUNT]) -> bool {
    release_after_frames.iter().any(|frames| *frames != 0)
}

fn finish_pending_releases(
    engine: &mut SynthEngine,
    active: &mut [bool; MIDI_NOTE_COUNT],
    last_velocity: &mut [u8; MIDI_NOTE_COUNT],
    release_after_frames: &mut [usize; MIDI_NOTE_COUNT],
    rendered_frames: usize,
) {
    for note in 0..MIDI_NOTE_COUNT {
        let frames = release_after_frames[note];
        if frames == 0 {
            continue;
        }

        release_after_frames[note] = frames.saturating_sub(rendered_frames);
        if release_after_frames[note] == 0 && active[note] {
            engine.note_off(note as u8);
            active[note] = false;
            last_velocity[note] = 0;
        }
    }
}
