//! DMG-style audio helpers.
//!
//! This is not a register-accurate Game Boy APU. It gives kernel demos a small
//! 4-bit pulse/noise voice that uses the same HDA sample path as the synth demos.

use alloc::vec::Vec;

const SAMPLE_RATE_HZ: u32 = 48_000;

fn quantize_4bit(sample: i32) -> i16 {
    let clamped = sample.clamp(-24_000, 24_000);
    let level = ((clamped + 24_000) * 15 + 24_000) / 48_000;
    (level * 3_200 - 24_000) as i16
}

fn push_square_note(samples: &mut Vec<i16>, freq_hz: u32, duration_ms: u32, volume: i32) {
    let frames = (SAMPLE_RATE_HZ as u64 * duration_ms as u64 / 1_000) as usize;
    let period = (SAMPLE_RATE_HZ / freq_hz.max(1)).max(1) as usize;
    let high = (period / 2).max(1);

    for i in 0..frames {
        let env = (frames.saturating_sub(i) as i32 * volume) / frames.max(1) as i32;
        let raw = if i % period < high { env } else { -env };
        let q = quantize_4bit(raw);
        samples.push(q);
        samples.push(q);
    }
}

fn push_noise_burst(samples: &mut Vec<i16>, duration_ms: u32, volume: i32) {
    let frames = (SAMPLE_RATE_HZ as u64 * duration_ms as u64 / 1_000) as usize;
    let mut lfsr = 0x7FFFu16;

    for i in 0..frames {
        let bit = (lfsr ^ (lfsr >> 1)) & 1;
        lfsr = (lfsr >> 1) | (bit << 14);
        let env = (frames.saturating_sub(i) as i32 * volume) / frames.max(1) as i32;
        let raw = if lfsr & 1 == 0 { env } else { -env };
        let q = quantize_4bit(raw);
        samples.push(q);
        samples.push(q);
    }
}

pub fn render_boot_chime() -> (Vec<i16>, u32) {
    let mut samples = Vec::with_capacity((SAMPLE_RATE_HZ / 5) as usize * 2);
    push_square_note(&mut samples, 988, 55, 18_000);
    push_square_note(&mut samples, 1_319, 70, 16_000);
    push_noise_burst(&mut samples, 35, 9_000);

    let frames = samples.len() / 2;
    let duration_ms = ((frames as u64 * 1_000) / SAMPLE_RATE_HZ as u64).max(1) as u32;
    (samples, duration_ms)
}

pub fn play_boot_chime() -> Result<(), &'static str> {
    super::ensure_init()?;

    let (samples, duration_ms) = render_boot_chime();
    super::play_samples(samples.as_slice(), duration_ms)
}
