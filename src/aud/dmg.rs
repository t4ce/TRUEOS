//! DMG-style audio helpers.
//!
//! This is not a register-accurate Game Boy APU. It gives kernel demos a small
//! 4-bit pulse/noise voice that uses the same HDA sample path as the synth demos.

use alloc::vec::Vec;

const SAMPLE_RATE_HZ: u32 = 48_000;
const STREAM_START_AHEAD_FRAMES: usize = SAMPLE_RATE_HZ as usize / 10;

pub struct DmgAudioStream {
    started: bool,
    disabled: bool,
    write_cursor: usize,
    dma_len_samples: usize,
}

impl DmgAudioStream {
    pub const fn new() -> Self {
        Self {
            started: false,
            disabled: false,
            write_cursor: 0,
            dma_len_samples: 0,
        }
    }

    pub fn push_samples(&mut self, samples: &[i16]) -> Result<(), &'static str> {
        if samples.is_empty() || self.disabled {
            return Ok(());
        }

        if !crate::hda::is_initialized() {
            return Ok(());
        }

        if !self.started {
            if let Err(err) = self.start() {
                self.disabled = true;
                return Err(err);
            }
        }

        let Some((buf, cap)) = crate::hda::get_dma_buffer_info() else {
            return Err("HDA: DMA buffer not initialized");
        };
        if cap == 0 {
            return Err("HDA: empty DMA buffer");
        }

        let mut copied = 0usize;
        while copied < samples.len() {
            let chunk = (samples.len() - copied).min(cap - self.write_cursor);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    samples.as_ptr().add(copied),
                    buf.add(self.write_cursor),
                    chunk,
                );
            }
            copied += chunk;
            self.write_cursor = (self.write_cursor + chunk) % cap;
        }

        crate::hda::clear_stream_status();
        let _ = crate::hda::ensure_running();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        if !crate::hda::is_initialized() {
            return Ok(());
        }

        let Some((buf, cap)) = crate::hda::get_dma_buffer_info() else {
            return Err("HDA: DMA buffer not initialized");
        };
        if cap == 0 {
            return Err("HDA: empty DMA buffer");
        }

        unsafe {
            core::ptr::write_bytes(buf, 0, cap);
        }

        self.dma_len_samples = cap;
        self.write_cursor = (STREAM_START_AHEAD_FRAMES * 2).min(cap.saturating_sub(2)) & !1;
        crate::hda::start_dma()?;
        self.started = true;
        Ok(())
    }
}

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

fn apply_loop_edge_fade(samples: &mut [i16], fade_ms: u32) {
    let frames = samples.len() / 2;
    let fade_frames = ((SAMPLE_RATE_HZ as u64 * fade_ms as u64) / 1_000)
        .min((frames / 2) as u64) as usize;
    if fade_frames == 0 {
        return;
    }

    for frame in 0..fade_frames {
        let in_gain = frame as i32;
        let out_gain = (fade_frames - 1 - frame) as i32;
        let denom = fade_frames.max(1) as i32;

        let head = frame * 2;
        samples[head] = ((samples[head] as i32 * in_gain) / denom) as i16;
        samples[head + 1] = ((samples[head + 1] as i32 * in_gain) / denom) as i16;

        let tail = (frames - 1 - frame) * 2;
        samples[tail] = ((samples[tail] as i32 * out_gain) / denom) as i16;
        samples[tail + 1] = ((samples[tail + 1] as i32 * out_gain) / denom) as i16;
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

pub fn render_loop_bed() -> Vec<i16> {
    let mut samples = Vec::with_capacity(SAMPLE_RATE_HZ as usize * 2);

    for _ in 0..2 {
        push_square_note(&mut samples, 262, 90, 7_000);
        push_square_note(&mut samples, 330, 90, 6_000);
        push_square_note(&mut samples, 392, 90, 6_500);
        push_square_note(&mut samples, 523, 90, 5_500);
        push_noise_burst(&mut samples, 40, 2_800);
        push_square_note(&mut samples, 392, 100, 5_500);
    }

    apply_loop_edge_fade(samples.as_mut_slice(), 8);
    samples
}

pub fn start_loop_bed() -> Result<(), &'static str> {
    super::ensure_init()?;

    let samples = render_loop_bed();
    crate::hda::start_looped_playback(samples.as_slice())
}
