//! Standalone backing-pattern renderer.
//!
//! This source owns only pattern timing and sample generation. It has no MIDI
//! state and no direct HDA access; the Tinyaudio mixer is its sole consumer.

use super::synth::{CHANNELS, SAMPLE_RATE};
use super::tables::SINE_TABLE;

const BACKING_ENABLED: bool = true;
const BACKING_BPM: u32 = 174;
const BACKING_VOLUME_PCT: i32 = 33;
const BACKING_STEPS_PER_BEAT: u32 = 4;
const BACKING_CLOCK_INCREMENT: u32 = BACKING_BPM * BACKING_STEPS_PER_BEAT;
const BACKING_CLOCK_DENOMINATOR: u32 = SAMPLE_RATE * 60;
const BACKING_STEP_SAMPLES_ROUNDED: u32 =
    (BACKING_CLOCK_DENOMINATOR + BACKING_CLOCK_INCREMENT / 2) / BACKING_CLOCK_INCREMENT;
const BACKING_FRAC_BITS: u32 = 16;
const BACKING_TABLE_SIZE: u32 = 256;
const BACKING_NO_BASS_STEP: u8 = u8::MAX;

pub(crate) const fn backing_config() -> (bool, u32, i32) {
    (BACKING_ENABLED, BACKING_BPM, BACKING_VOLUME_PCT)
}

pub(crate) struct BackingPatternRenderSource {
    groove: BackingGroove,
}

impl BackingPatternRenderSource {
    pub(crate) const fn new() -> Self {
        Self {
            groove: BackingGroove::new(),
        }
    }

    pub(crate) fn render_into(&mut self, buffer: &mut [i16], frames: usize) -> bool {
        if !BACKING_ENABLED {
            return false;
        }

        self.groove.render_into(buffer, frames);
        true
    }
}

#[derive(Clone, Copy)]
struct BackingClock {
    phase: u32,
    step: u8,
    step_age: u32,
}

impl BackingClock {
    const fn new() -> Self {
        Self {
            phase: 0,
            step: 0,
            step_age: 0,
        }
    }

    const fn position(&self) -> (u8, u32) {
        (self.step, self.step_age)
    }

    fn advance(&mut self) {
        self.phase += BACKING_CLOCK_INCREMENT;
        if self.phase >= BACKING_CLOCK_DENOMINATOR {
            self.phase -= BACKING_CLOCK_DENOMINATOR;
            self.step = (self.step + 1) & 0x0f;
            self.step_age = 0;
        } else {
            self.step_age = self.step_age.saturating_add(1);
        }
    }
}

struct BackingGroove {
    clock: BackingClock,
    bass_phase: u32,
    bass_step: u8,
    noise: u16,
}

impl BackingGroove {
    const fn new() -> Self {
        Self {
            clock: BackingClock::new(),
            bass_phase: 0,
            bass_step: BACKING_NO_BASS_STEP,
            noise: 0xace1,
        }
    }

    fn render_into(&mut self, buffer: &mut [i16], frames: usize) {
        for frame in 0..frames {
            let (step, step_age) = self.clock.position();

            let mut sample = 0i32;
            sample += self.render_kick(step, step_age);
            sample += self.render_clap(step, step_age);
            sample += self.render_hat(step, step_age);
            sample += self.render_bass(step, step_age);
            sample = sample * BACKING_VOLUME_PCT / 100;

            let idx = frame * CHANNELS as usize;
            mix_mono(buffer, idx, sample);
            self.clock.advance();
        }
    }

    fn render_kick(&self, step: u8, age: u32) -> i32 {
        let amp = match step {
            0 => 5_200,
            10 => 3_700,
            14 => 1_500,
            _ => return 0,
        };
        let len = BACKING_STEP_SAMPLES_ROUNDED / 2;
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
        let body = noise * decay(age, BACKING_STEP_SAMPLES_ROUNDED / 2, 3_400) / 32_767;
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

        let gate_len = (BACKING_STEP_SAMPLES_ROUNDED * 3) / 4;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backing_clock_has_one_exact_step_zero_per_bar() {
        let mut clock = BackingClock::new();
        let mut step_lengths = [0u32; 16];
        let mut transitions = 0u32;

        while transitions < 16 {
            let (step, _) = clock.position();
            step_lengths[step as usize] += 1;
            clock.advance();
            if clock.step != step {
                transitions += 1;
            }
        }

        assert_eq!(clock.position(), (0, 0));
        assert_eq!(step_lengths.iter().sum::<u32>(), 66_207);
        assert!(step_lengths.iter().all(|len| matches!(*len, 4_137 | 4_138)));
    }

    #[test]
    fn backing_clock_advances_exactly_174_beats_per_minute() {
        let mut clock = BackingClock::new();
        let mut step_transitions = 0u32;

        for _ in 0..BACKING_CLOCK_DENOMINATOR {
            let step = clock.step;
            clock.advance();
            if clock.step != step {
                step_transitions += 1;
            }
        }

        assert_eq!(step_transitions, BACKING_BPM * BACKING_STEPS_PER_BEAT);
        assert_eq!(clock.phase, 0);
    }
}
