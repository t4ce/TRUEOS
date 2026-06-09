//! Live ESP piano synth service.
//!
//! UDP note edges update shared key state; this task turns that state into a
//! steady HDA PCM stream with a local polyphonic synth engine.

use alloc::vec::Vec;
use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::dmg::DmgAudioStream;
use super::synth::{CHANNELS, Envelope, SAMPLE_RATE, SynthEngine, Waveform};
use super::tables::SINE_TABLE;

const MIDI_NOTE_COUNT: usize = 128;
const RENDER_MS: u32 = 5;
const RENDER_FRAMES: usize = (SAMPLE_RATE as usize * RENDER_MS as usize) / 1_000;
const RENDER_SAMPLES: usize = RENDER_FRAMES * CHANNELS as usize;
const STREAM_START_AHEAD_FRAMES: usize = SAMPLE_RATE as usize / 200;
const STREAM_GUARD_SAMPLES: usize = (SAMPLE_RATE as usize / 200) * CHANNELS as usize;
const STREAM_TARGET_QUEUED_SAMPLES: usize =
    ((SAMPLE_RATE as usize * 10) / 1_000) * CHANNELS as usize;
const INIT_RETRY_MS: u64 = 500;
const BACKING_ENABLED: bool = true;
const BACKING_BPM: u32 = 174;
const BACKING_VOLUME_PCT: i32 = 33;
const BACKING_BEAT_SAMPLES: u32 = (SAMPLE_RATE * 60) / BACKING_BPM;
const BACKING_SIXTEENTH_SAMPLES: u32 = BACKING_BEAT_SAMPLES / 4;
const BACKING_BAR_SAMPLES: u32 = BACKING_BEAT_SAMPLES * 4;
const BACKING_FRAC_BITS: u32 = 16;
const BACKING_TABLE_SIZE: u32 = 256;
const BACKING_NO_BASS_STEP: u8 = u8::MAX;

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

struct BackingGroove {
    sample_pos: u64,
    bass_phase: u32,
    bass_step: u8,
    noise: u16,
}

impl BackingGroove {
    const fn new() -> Self {
        Self {
            sample_pos: 0,
            bass_phase: 0,
            bass_step: BACKING_NO_BASS_STEP,
            noise: 0xace1,
        }
    }

    fn render_into(&mut self, buffer: &mut [i16], frames: usize) {
        for frame in 0..frames {
            let bar_pos = (self.sample_pos % BACKING_BAR_SAMPLES as u64) as u32;
            let step = ((bar_pos / BACKING_SIXTEENTH_SAMPLES) & 0x0f) as u8;
            let step_age = bar_pos % BACKING_SIXTEENTH_SAMPLES;

            let mut sample = 0i32;
            sample += self.render_kick(step, step_age);
            sample += self.render_clap(step, step_age);
            sample += self.render_hat(step, step_age);
            sample += self.render_bass(step, step_age);
            sample = sample * BACKING_VOLUME_PCT / 100;

            let idx = frame * CHANNELS as usize;
            mix_mono(buffer, idx, sample);
            self.sample_pos = self.sample_pos.wrapping_add(1);
        }
    }

    fn render_kick(&self, step: u8, age: u32) -> i32 {
        let amp = match step {
            0 => 5_200,
            10 => 3_700,
            14 => 1_500,
            _ => return 0,
        };
        let len = BACKING_SIXTEENTH_SAMPLES / 2;
        let env = decay(age, len, amp);
        if env == 0 {
            return 0;
        }

        let freq = 82u32.saturating_sub((age * 42) / len.max(1)).max(42);
        let tone = sine_at_table_phase(
            ((age as u64 * freq as u64 * BACKING_TABLE_SIZE as u64) / SAMPLE_RATE as u64) as u32,
        );
        tone * env / 32_767
    }

    fn render_clap(&mut self, step: u8, age: u32) -> i32 {
        if step != 4 && step != 12 {
            return 0;
        }

        let noise = self.noise_sample();
        let body = noise * decay(age, BACKING_SIXTEENTH_SAMPLES / 2, 3_400) / 32_767;
        let flam1 = noise * delayed_decay(age, 520, 1_300, 1_100) / 32_767;
        let flam2 = noise * delayed_decay(age, 1_040, 1_400, 800) / 32_767;
        body + flam1 + flam2
    }

    fn render_hat(&mut self, step: u8, age: u32) -> i32 {
        let len = if step & 1 == 0 { 1_400 } else { 850 };
        let amp = if step & 3 == 2 { 1_300 } else { 820 };
        let noise = self.noise_sample();
        noise * decay(age, len, amp) / 32_767
    }

    fn render_bass(&mut self, step: u8, age: u32) -> i32 {
        let Some(freq) = bass_freq(step) else {
            self.bass_step = BACKING_NO_BASS_STEP;
            return 0;
        };

        if self.bass_step != step {
            self.bass_step = step;
            self.bass_phase = 0;
        }

        let gate_len = (BACKING_SIXTEENTH_SAMPLES * 3) / 4;
        let env = decay(age, gate_len, 4_800);
        if env == 0 {
            return 0;
        }

        let phase_inc =
            ((freq as u64 * BACKING_TABLE_SIZE as u64) << BACKING_FRAC_BITS) / SAMPLE_RATE as u64;
        self.bass_phase = self.bass_phase.wrapping_add(phase_inc as u32);

        let fundamental = sine_at_phase(self.bass_phase) * env / 32_767;
        let second = sine_at_phase(self.bass_phase.wrapping_mul(2)) * env / 32_767;
        fundamental + second / 7
    }

    fn noise_sample(&mut self) -> i32 {
        let bit = (self.noise ^ (self.noise >> 2) ^ (self.noise >> 3) ^ (self.noise >> 5)) & 1;
        self.noise = (self.noise >> 1) | (bit << 15);
        if self.noise & 1 == 0 { 32_767 } else { -32_767 }
    }
}

fn bass_freq(step: u8) -> Option<u32> {
    match step {
        0 | 1 => Some(55),
        3 => Some(65),
        6 => Some(49),
        10 => Some(73),
        12 => Some(65),
        _ => None,
    }
}

fn decay(age: u32, len: u32, amp: i32) -> i32 {
    if len == 0 || age >= len {
        0
    } else {
        amp * (len - age) as i32 / len as i32
    }
}

fn delayed_decay(age: u32, delay: u32, len: u32, amp: i32) -> i32 {
    if age < delay {
        0
    } else {
        decay(age - delay, len, amp)
    }
}

fn sine_at_phase(phase: u32) -> i32 {
    let idx = ((phase >> BACKING_FRAC_BITS) & (BACKING_TABLE_SIZE - 1)) as usize;
    SINE_TABLE[idx] as i32
}

fn sine_at_table_phase(phase: u32) -> i32 {
    SINE_TABLE[(phase & (BACKING_TABLE_SIZE - 1)) as usize] as i32
}

fn mix_mono(buffer: &mut [i16], idx: usize, sample: i32) {
    for channel in 0..CHANNELS as usize {
        let out = buffer[idx + channel] as i32 + sample;
        buffer[idx + channel] = out.clamp(-32_767, 32_767) as i16;
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
        "esp-piano-audio: live synth ready voices={} chunk_ms={} rate={} backing_bpm={} backing_vol={}%\n",
        super::synth::MAX_VOICES,
        RENDER_MS,
        SAMPLE_RATE,
        BACKING_BPM,
        BACKING_VOLUME_PCT
    );

    let mut engine = configure_engine();
    let mut stream = DmgAudioStream::new_with_start_ahead_frames(STREAM_START_AHEAD_FRAMES);
    let mut backing = BackingGroove::new();
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

        let active_audio =
            BACKING_ENABLED || engine.active_voice_count() != 0 || has_held_note(&state.notes);
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
        if BACKING_ENABLED {
            backing.render_into(buffer.as_mut_slice(), RENDER_FRAMES);
        }

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
