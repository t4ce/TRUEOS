//! TrustOS Audio Subsystem
//!
//! Provides:
//!   - `synth` — Multi-waveform synthesizer with ADSR envelopes  
//!   - `tables` — Sine LUT, MIDI frequency table, note name parser
//!   - High-level API for playing synthesized audio through Intel HDA
//!
//! Architecture:
//!   SynthEngine → render samples → write to HDA buffer → DMA playback

#![allow(dead_code)]

pub mod pattern;
pub mod player;
pub mod synth;
pub mod tables;

use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use pattern::{Pattern, PatternBank, Step};
use player::PatternPlayer;
use synth::{Envelope, SynthEngine, Waveform};

/// Global synth engine instance
static SYNTH: Mutex<Option<SynthEngine>> = Mutex::new(None);
/// Global pattern bank
static PATTERNS: Mutex<Option<PatternBank>> = Mutex::new(None);
/// Global player
static PLAYER: Mutex<Option<PatternPlayer>> = Mutex::new(None);
static BASSLINE_TOGGLE_SEQ: AtomicU32 = AtomicU32::new(0);

const PIANO_PROBE_MAX_NOTES: usize = 8;

#[derive(Copy, Clone)]
struct PianoProbeHeldState {
    len: usize,
    notes: [u8; PIANO_PROBE_MAX_NOTES],
}

impl PianoProbeHeldState {
    const fn empty() -> Self {
        Self {
            len: 0,
            notes: [0; PIANO_PROBE_MAX_NOTES],
        }
    }
}

static PIANO_PROBE_HELD: Mutex<PianoProbeHeldState> = Mutex::new(PianoProbeHeldState::empty());

/// Initialize the audio subsystem (HDA driver + synth engine + pattern bank)
pub fn init() -> Result<(), &'static str> {
    // Ensure HDA driver is initialized
    if !crate::hda::is_initialized() {
        crate::hda::init()?;
    }

    // Create synth engine
    let engine = SynthEngine::new();
    *SYNTH.lock() = Some(engine);

    // Create pattern bank with presets
    let mut bank = PatternBank::new();
    bank.load_presets();
    *PATTERNS.lock() = Some(bank);

    // Create player
    *PLAYER.lock() = Some(PatternPlayer::new());

    log::info!("[AUDIO] TrustSynth engine + pattern bank initialized");
    Ok(())
}

/// Ensure synth is initialized, init if needed
fn ensure_init() -> Result<(), &'static str> {
    if SYNTH.lock().is_none() {
        init()?;
    }
    Ok(())
}

/// Play a single note by name (e.g., "C4", "A#3") for a duration
pub fn play_note(name: &str, duration_ms: u32) -> Result<(), &'static str> {
    ensure_init()?;

    let samples = {
        let mut synth = SYNTH.lock();
        let engine = synth.as_mut().ok_or("Synth not initialized")?;
        engine.play_note_by_name(name, duration_ms)?
    };

    // Write samples to HDA and play
    play_samples(&samples, duration_ms)?;
    Ok(())
}

/// Play a note by MIDI number
pub fn play_midi_note(note: u8, velocity: u8, duration_ms: u32) -> Result<(), &'static str> {
    ensure_init()?;

    let samples = {
        let mut synth = SYNTH.lock();
        let engine = synth.as_mut().ok_or("Synth not initialized")?;
        engine.render_note(note, velocity, duration_ms)
    };

    play_samples(&samples, duration_ms)?;
    Ok(())
}

/// Play a tone at a specific frequency
pub fn play_freq(freq_hz: u32, duration_ms: u32) -> Result<(), &'static str> {
    ensure_init()?;

    let samples = {
        let mut synth = SYNTH.lock();
        let engine = synth.as_mut().ok_or("Synth not initialized")?;
        engine.render_freq(freq_hz, duration_ms)
    };

    play_samples(&samples, duration_ms)?;
    Ok(())
}

/// Set the default waveform
pub fn set_waveform(wf: Waveform) -> Result<(), &'static str> {
    ensure_init()?;
    let mut synth = SYNTH.lock();
    let engine = synth.as_mut().ok_or("Synth not initialized")?;
    engine.set_waveform(wf);
    Ok(())
}

/// Set ADSR envelope parameters
pub fn set_adsr(
    attack_ms: u32,
    decay_ms: u32,
    sustain_pct: u32,
    release_ms: u32,
) -> Result<(), &'static str> {
    ensure_init()?;
    let mut synth = SYNTH.lock();
    let engine = synth.as_mut().ok_or("Synth not initialized")?;
    engine.set_adsr(attack_ms, decay_ms, sustain_pct, release_ms);
    Ok(())
}

/// Set envelope preset
pub fn set_envelope_preset(name: &str) -> Result<(), &'static str> {
    ensure_init()?;
    let env = match name {
        "default" => Envelope::default_env(),
        "organ" => Envelope::organ(),
        "pluck" => Envelope::pluck(),
        "pad" => Envelope::pad(),
        _ => return Err("Unknown preset (use: default, organ, pluck, pad)"),
    };
    let mut synth = SYNTH.lock();
    let engine = synth.as_mut().ok_or("Synth not initialized")?;
    engine.envelope = env;
    Ok(())
}

/// Set master volume (0-255)
pub fn set_volume(vol: u8) -> Result<(), &'static str> {
    ensure_init()?;
    let mut synth = SYNTH.lock();
    let engine = synth.as_mut().ok_or("Synth not initialized")?;
    engine.master_volume = vol;
    Ok(())
}

/// Get synth status
pub fn status() -> String {
    let synth = SYNTH.lock();
    match synth.as_ref() {
        Some(engine) => engine.status(),
        None => String::from("TrustSynth: not initialized\n"),
    }
}

/// Stop all audio
pub fn stop() -> Result<(), &'static str> {
    {
        let mut synth = SYNTH.lock();
        if let Some(engine) = synth.as_mut() {
            engine.all_notes_off();
        }
    }
    crate::hda::stop()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Internal: write rendered samples to HDA buffer and play
// ═══════════════════════════════════════════════════════════════════════════════

/// Write rendered audio samples to the HDA DMA buffer and trigger playback
fn play_samples(samples: &[i16], duration_ms: u32) -> Result<(), &'static str> {
    // Access HDA driver internals to copy samples into the DMA buffer
    crate::hda::write_samples_and_play(samples, duration_ms)
}

pub fn request_bassline_toggle() -> u32 {
    BASSLINE_TOGGLE_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1)
}

pub fn bassline_toggle_seq() -> u32 {
    BASSLINE_TOGGLE_SEQ.load(Ordering::Acquire)
}

pub fn render_retro_bassline() -> Result<(alloc::vec::Vec<i16>, u16, u32), &'static str> {
    ensure_patterns()?;

    let bpm = 116;
    let mut pattern = Pattern::new("retro-bass", 16, bpm);
    pattern.waveform = Waveform::Sawtooth;
    pattern.envelope = Envelope::new(2, 70, 42, 35);

    let steps: [(u8, u8); 16] = [
        (36, 96),
        (36, 76),
        (255, 0),
        (39, 82),
        (36, 92),
        (255, 0),
        (31, 76),
        (255, 0),
        (34, 90),
        (34, 70),
        (255, 0),
        (32, 78),
        (31, 86),
        (255, 0),
        (34, 72),
        (255, 0),
    ];

    for (idx, &(note, velocity)) in steps.iter().enumerate() {
        pattern.steps[idx] = if note == 255 {
            Step::rest()
        } else {
            Step::note_vel(note, velocity)
        };
    }

    let mut synth_lock = SYNTH.lock();
    let engine = synth_lock.as_mut().ok_or("Synth not initialized")?;
    Ok((pattern.render(engine), bpm, pattern.step_duration_ms()))
}

fn note_in_slice(notes: &[u8], len: usize, note: u8) -> bool {
    notes[..len.min(notes.len())].iter().any(|&n| n == note)
}

/// Render and play the currently held piano notes as one mixed HDA buffer.
pub fn play_piano_held_probe(
    notes: &[u8],
    velocities: &[u8],
    len: usize,
    duration_ms: u32,
) -> Result<(), &'static str> {
    ensure_init()?;

    let held_len = len
        .min(notes.len())
        .min(velocities.len())
        .min(PIANO_PROBE_MAX_NOTES);

    let samples = {
        let mut held = PIANO_PROBE_HELD.lock();
        let mut synth_lock = SYNTH.lock();
        let engine = synth_lock.as_mut().ok_or("Synth not initialized")?;
        let saved_wf = engine.waveform;
        let saved_env = engine.envelope;

        engine.waveform = Waveform::Triangle;
        engine.envelope = Envelope::new(2, 70, 70, 80);

        for idx in 0..held.len {
            let old_note = held.notes[idx];
            if !note_in_slice(notes, held_len, old_note) {
                engine.note_off(old_note);
            }
        }

        for idx in 0..held_len {
            let note = notes[idx].min(127);
            if !note_in_slice(&held.notes, held.len, note) {
                engine.note_on(note, velocities[idx].clamp(32, 127));
            }
        }

        held.len = held_len;
        for idx in 0..PIANO_PROBE_MAX_NOTES {
            held.notes[idx] = if idx < held_len {
                notes[idx].min(127)
            } else {
                0
            };
        }

        let frames = synth::ms_to_samples(duration_ms).max(1) as usize;
        let mut buffer = alloc::vec![0i16; frames * synth::CHANNELS as usize];
        engine.render(buffer.as_mut_slice(), frames);

        engine.waveform = saved_wf;
        engine.envelope = saved_env;
        buffer
    };

    play_samples(&samples, duration_ms)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pattern API
// ═══════════════════════════════════════════════════════════════════════════════

/// Ensure pattern bank is initialized
fn ensure_patterns() -> Result<(), &'static str> {
    ensure_init()?;
    if PATTERNS.lock().is_none() {
        let mut bank = PatternBank::new();
        bank.load_presets();
        *PATTERNS.lock() = Some(bank);
    }
    if PLAYER.lock().is_none() {
        *PLAYER.lock() = Some(PatternPlayer::new());
    }
    Ok(())
}

/// Create a new pattern
pub fn pattern_new(name: &str, steps: usize, bpm: u16) -> Result<(), &'static str> {
    ensure_patterns()?;
    let pattern = Pattern::new(name, steps, bpm);
    let mut bank = PATTERNS.lock();
    let bank = bank.as_mut().ok_or("Pattern bank not initialized")?;
    bank.add(pattern)?;
    Ok(())
}

/// Set a note in a pattern
pub fn pattern_set_note(name: &str, step: usize, note_name: &str) -> Result<(), &'static str> {
    ensure_patterns()?;
    let mut bank = PATTERNS.lock();
    let bank = bank.as_mut().ok_or("Pattern bank not initialized")?;
    let pat = bank.get_by_name_mut(name).ok_or("Pattern not found")?;
    pat.set_note(step, note_name)
}

/// Set BPM on a pattern
pub fn pattern_set_bpm(name: &str, bpm: u16) -> Result<(), &'static str> {
    ensure_patterns()?;
    let mut bank = PATTERNS.lock();
    let bank = bank.as_mut().ok_or("Pattern bank not initialized")?;
    let pat = bank.get_by_name_mut(name).ok_or("Pattern not found")?;
    pat.bpm = bpm;
    Ok(())
}

/// Set waveform on a pattern
pub fn pattern_set_wave(name: &str, wf: Waveform) -> Result<(), &'static str> {
    ensure_patterns()?;
    let mut bank = PATTERNS.lock();
    let bank = bank.as_mut().ok_or("Pattern bank not initialized")?;
    let pat = bank.get_by_name_mut(name).ok_or("Pattern not found")?;
    pat.waveform = wf;
    Ok(())
}

/// Display a pattern
pub fn pattern_show(name: &str) -> Result<String, &'static str> {
    ensure_patterns()?;
    let bank = PATTERNS.lock();
    let bank = bank.as_ref().ok_or("Pattern bank not initialized")?;
    let pat = bank.get_by_name(name).ok_or("Pattern not found")?;
    Ok(pat.display())
}

/// List all patterns
pub fn pattern_list() -> String {
    let bank = PATTERNS.lock();
    match bank.as_ref() {
        Some(b) => b.list(),
        None => String::from("Pattern bank not initialized\n"),
    }
}

/// Remove a pattern
pub fn pattern_remove(name: &str) -> Result<(), &'static str> {
    ensure_patterns()?;
    let mut bank = PATTERNS.lock();
    let bank = bank.as_mut().ok_or("Pattern bank not initialized")?;
    bank.remove(name)
}

/// Play a pattern by name
pub fn pattern_play(name: &str, loops: u32) -> Result<(), &'static str> {
    ensure_patterns()?;

    // Clone the pattern so we don't hold the lock during playback
    let pattern = {
        let bank = PATTERNS.lock();
        let bank = bank.as_ref().ok_or("Pattern bank not initialized")?;
        bank.get_by_name(name).ok_or("Pattern not found")?.clone()
    };

    // Get synth engine and player — need to drop locks carefully
    let mut synth_lock = SYNTH.lock();
    let engine = synth_lock.as_mut().ok_or("Synth not initialized")?;
    let mut player_lock = PLAYER.lock();
    let player = player_lock.as_mut().ok_or("Player not initialized")?;

    player.play_pattern_visual(&pattern, engine, loops)
}

/// Play a short generated phrase seeded from the latest piano MIDI note.
pub fn pattern_play_piano_probe(note: u8, velocity: u8, loops: u32) -> Result<(), &'static str> {
    ensure_patterns()?;

    let root = note.min(127);
    let vel = velocity.clamp(48, 127);
    let mut pattern = Pattern::new("piano-probe", 8, 132);
    pattern.waveform = Waveform::Triangle;
    pattern.envelope = Envelope::new(2, 90, 45, 80);

    let offsets: [i16; 8] = [0, 7, 12, 7, 3, 10, 15, 12];
    for (i, offset) in offsets.iter().enumerate() {
        let stepped = (i16::from(root) + *offset).clamp(0, 127) as u8;
        let step_vel = vel.saturating_sub((i as u8) * 4).max(40);
        pattern.steps[i] = Step::note_vel(stepped, step_vel);
    }

    let mut synth_lock = SYNTH.lock();
    let engine = synth_lock.as_mut().ok_or("Synth not initialized")?;
    let mut player_lock = PLAYER.lock();
    let player = player_lock.as_mut().ok_or("Player not initialized")?;

    player.play_pattern_visual(&pattern, engine, loops)
}

/// Stop pattern playback
pub fn pattern_stop() -> Result<(), &'static str> {
    let mut player_lock = PLAYER.lock();
    if let Some(player) = player_lock.as_mut() {
        player.stop();
    }
    Ok(())
}
