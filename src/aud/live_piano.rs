//! Live ESP piano synth service.
//!
//! UDP note edges update shared key state; this task turns that state into a
//! steady HDA PCM stream with a local polyphonic synth engine.

use alloc::vec::Vec;
use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::dmg::DmgAudioStream;
use super::synth::{CHANNELS, Envelope, SAMPLE_RATE, SynthEngine, Waveform};

const MIDI_NOTE_COUNT: usize = 128;
const RENDER_MS: u32 = 5;
const RENDER_FRAMES: usize = (SAMPLE_RATE as usize * RENDER_MS as usize) / 1_000;
const RENDER_SAMPLES: usize = RENDER_FRAMES * CHANNELS as usize;
const STREAM_START_AHEAD_FRAMES: usize = SAMPLE_RATE as usize / 200;
const STREAM_GUARD_SAMPLES: usize = (SAMPLE_RATE as usize / 200) * CHANNELS as usize;
const STREAM_TARGET_QUEUED_SAMPLES: usize =
    ((SAMPLE_RATE as usize * 10) / 1_000) * CHANNELS as usize;
const INIT_RETRY_MS: u64 = 500;

#[derive(Clone, Copy)]
struct LiveNote {
    down: bool,
    velocity: u8,
}

impl LiveNote {
    const fn up() -> Self {
        Self {
            down: false,
            velocity: 0,
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
    state.notes[idx] = LiveNote {
        down: true,
        velocity: velocity.max(1),
    };
    state.seq = state.seq.wrapping_add(1);
}

pub fn note_off(note: u8) {
    let idx = note as usize;
    if idx >= MIDI_NOTE_COUNT {
        return;
    }

    let mut state = LIVE_STATE.lock();
    state.notes[idx].down = false;
    state.seq = state.seq.wrapping_add(1);
}

pub fn all_notes_off() {
    let mut state = LIVE_STATE.lock();
    state.notes = [LiveNote::up(); MIDI_NOTE_COUNT];
    state.seq = state.seq.wrapping_add(1);
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

fn apply_note_state(
    engine: &mut SynthEngine,
    active: &mut [bool; MIDI_NOTE_COUNT],
    last_velocity: &mut [u8; MIDI_NOTE_COUNT],
    notes: &[LiveNote; MIDI_NOTE_COUNT],
) {
    for note in 0..MIDI_NOTE_COUNT {
        let wanted = notes[note];
        if wanted.down && !active[note] {
            engine.note_on(note as u8, wanted.velocity);
            active[note] = true;
            last_velocity[note] = wanted.velocity;
        } else if !wanted.down && active[note] {
            engine.note_off(note as u8);
            active[note] = false;
            last_velocity[note] = 0;
        } else if wanted.down && wanted.velocity != last_velocity[note] {
            last_velocity[note] = wanted.velocity;
        }
    }
}

#[embassy_executor::task]
pub async fn task() {
    let mut init_warn_count = 0u8;

    while !crate::hda::is_initialized() {
        match crate::hda::init() {
            Ok(()) => break,
            Err(err) => {
                if init_warn_count < 4 {
                    init_warn_count = init_warn_count.saturating_add(1);
                    crate::log!("esp-piano-audio: hda init pending err={}\n", err);
                }
                Timer::after(Duration::from_millis(INIT_RETRY_MS)).await;
            }
        }
    }

    crate::log!(
        "esp-piano-audio: live synth ready voices={} chunk_ms={} rate={}\n",
        super::synth::MAX_VOICES,
        RENDER_MS,
        SAMPLE_RATE
    );

    let mut engine = configure_engine();
    let mut stream = DmgAudioStream::new_with_start_ahead_frames(STREAM_START_AHEAD_FRAMES);
    let mut buffer: Vec<i16> = alloc::vec![0; RENDER_SAMPLES];
    let mut active = [false; MIDI_NOTE_COUNT];
    let mut last_velocity = [0u8; MIDI_NOTE_COUNT];
    let mut last_seq = 0u32;
    let mut logged_push_err = false;

    loop {
        let state = snapshot();
        if state.seq != last_seq {
            apply_note_state(&mut engine, &mut active, &mut last_velocity, &state.notes);
            last_seq = state.seq;
        }

        let active_audio = engine.active_voice_count() != 0 || has_held_note(&state.notes);
        if !active_audio {
            if stream.is_started() {
                stream.stop_reset();
                crate::log!("esp-piano-audio: stream idle stop/reset\n");
            }
            Timer::after(Duration::from_millis(1)).await;
            continue;
        }

        if stream.is_started() {
            if let Some(writable) = stream.writable_samples(STREAM_GUARD_SAMPLES) {
                if writable < RENDER_SAMPLES {
                    Timer::after(Duration::from_millis(1)).await;
                    continue;
                }
            }
            if let Some(queued) = stream.queued_samples() {
                if queued >= STREAM_TARGET_QUEUED_SAMPLES {
                    Timer::after(Duration::from_millis(1)).await;
                    continue;
                }
            }
        }

        buffer.fill(0);
        engine.render(buffer.as_mut_slice(), RENDER_FRAMES);

        if let Err(err) = stream.push_samples(buffer.as_slice()) {
            if !logged_push_err {
                logged_push_err = true;
                crate::log!("esp-piano-audio: hda push err={}\n", err);
            }
            Timer::after(Duration::from_millis(INIT_RETRY_MS)).await;
            continue;
        }
        logged_push_err = false;

        Timer::after(Duration::from_millis(RENDER_MS as u64)).await;
    }
}
