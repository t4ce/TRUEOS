//! Minimal Game Boy APU register capture + softened 4-bit pulse renderer.
//!
//! This is not a complete DMG/CGB APU yet. It keeps the ROM-visible audio
//! register surface and renders the two pulse channels through low gain,
//! envelope decay, and a small low-pass stage so bringup audio is not raw HDA
//! square-wave pain.

use alloc::vec::Vec;

const CPU_HZ: u64 = 4_194_304;
const SAMPLE_RATE_HZ: u64 = 48_000;
const CHANNELS: usize = 2;
const NR52_INDEX: usize = (0xFF26 - 0xFF10) as usize;
const UNTIMED_PULSE_SAFETY_SAMPLES: u32 = (SAMPLE_RATE_HZ as u32 * 220) / 1_000;
const PULSE_PEAK: i32 = 3_200;
const MIX_PEAK: i32 = 6_400;

#[derive(Clone, Copy)]
struct PulseChannel {
    enabled: bool,
    duty: u8,
    volume: u8,
    envelope_period_samples: u32,
    envelope_timer_samples: u32,
    envelope_increase: bool,
    length_samples: u32,
    safety_samples: u32,
    frequency_raw: u16,
    phase: u32,
    filter_y: i32,
}

impl PulseChannel {
    const fn new() -> Self {
        Self {
            enabled: false,
            duty: 2,
            volume: 0,
            envelope_period_samples: 0,
            envelope_timer_samples: 0,
            envelope_increase: false,
            length_samples: 0,
            safety_samples: 0,
            frequency_raw: 0,
            phase: 0,
            filter_y: 0,
        }
    }

    fn trigger(
        &mut self,
        duty: u8,
        envelope: u8,
        length: u8,
        length_enabled: bool,
        frequency_raw: u16,
    ) {
        let volume = envelope >> 4;
        let envelope_period = envelope & 0x07;
        self.enabled = volume != 0 && frequency_raw < 2048;
        self.duty = duty & 0x03;
        self.volume = volume & 0x0F;
        self.envelope_increase = envelope & 0x08 != 0;
        self.envelope_period_samples = if envelope_period == 0 {
            0
        } else {
            (SAMPLE_RATE_HZ as u32 * envelope_period as u32) / 64
        };
        self.envelope_timer_samples = self.envelope_period_samples;
        self.length_samples = if length_enabled {
            let units = 64u32.saturating_sub((length & 0x3F) as u32).max(1);
            (SAMPLE_RATE_HZ as u32 * units) / 256
        } else {
            0
        };
        self.safety_samples = if length_enabled {
            self.length_samples.max(1)
        } else {
            UNTIMED_PULSE_SAFETY_SAMPLES
        };
        self.frequency_raw = frequency_raw & 0x07FF;
        self.phase = 0;
    }

    fn render_sample(&mut self) -> i32 {
        if !self.enabled || self.volume == 0 || self.frequency_raw >= 2048 {
            self.filter_y -= self.filter_y >> 4;
            return self.filter_y;
        }

        self.safety_samples = self.safety_samples.saturating_sub(1);
        if self.safety_samples == 0 {
            self.enabled = false;
            return 0;
        }

        if self.length_samples > 0 {
            self.length_samples -= 1;
            if self.length_samples == 0 {
                self.enabled = false;
                return 0;
            }
        }

        if self.envelope_period_samples > 0 {
            self.envelope_timer_samples = self.envelope_timer_samples.saturating_sub(1);
            if self.envelope_timer_samples == 0 {
                self.envelope_timer_samples = self.envelope_period_samples;
                if self.envelope_increase {
                    self.volume = (self.volume + 1).min(15);
                } else if self.volume > 0 {
                    self.volume -= 1;
                    if self.volume == 0 {
                        self.enabled = false;
                        return 0;
                    }
                }
            }
        }

        let period = (2048u32.saturating_sub(self.frequency_raw as u32)).max(1);
        let step = ((131_072u64 << 16) / (SAMPLE_RATE_HZ * period as u64)).max(1) as u32;
        self.phase = self.phase.wrapping_add(step);
        let pos = (self.phase >> 13) & 7;
        let high = match self.duty {
            0 => pos == 7,
            1 => pos >= 6,
            2 => pos >= 4,
            _ => pos == 0 || pos >= 5,
        };

        let amp = (self.volume as i32 * PULSE_PEAK) / 15;
        let target = if high { amp } else { -amp };
        // One-pole low-pass. The square is still 4-bit-retro, but its edge is
        // closer to the existing synth path than a naked discontinuity.
        self.filter_y += (target - self.filter_y) >> 3;
        self.filter_y
    }
}

pub struct Apu {
    regs: [u8; 0x30],
    ch1: PulseChannel,
    ch2: PulseChannel,
    sample_accum: u64,
    pending: Vec<i16>,
}

impl Apu {
    pub fn new() -> Self {
        let mut regs = [0u8; 0x30];
        regs[NR52_INDEX] = 0xF1;
        Self {
            regs,
            ch1: PulseChannel::new(),
            ch2: PulseChannel::new(),
            sample_accum: 0,
            pending: Vec::with_capacity((SAMPLE_RATE_HZ as usize / 30) * CHANNELS),
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF26 => {
                let mut v = self.regs[NR52_INDEX] | 0x70;
                if self.ch1.enabled {
                    v |= 0x01;
                }
                if self.ch2.enabled {
                    v |= 0x02;
                }
                v
            }
            0xFF10..=0xFF3F => self.regs[(addr - 0xFF10) as usize],
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0xFF26 => {
                self.regs[NR52_INDEX] = (self.regs[NR52_INDEX] & 0x0F) | (val & 0x80);
                if val & 0x80 == 0 {
                    self.ch1.enabled = false;
                    self.ch2.enabled = false;
                }
            }
            0xFF10..=0xFF3F => {
                let idx = (addr - 0xFF10) as usize;
                self.regs[idx] = val;
                if self.regs[NR52_INDEX] & 0x80 == 0 {
                    return;
                }
                match addr {
                    0xFF14 if val & 0x80 != 0 => self.trigger_ch1(),
                    0xFF19 if val & 0x80 != 0 => self.trigger_ch2(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub fn step(&mut self, t_cycles: u32) {
        if self.regs[NR52_INDEX] & 0x80 == 0 {
            return;
        }

        self.sample_accum = self
            .sample_accum
            .saturating_add(t_cycles as u64 * SAMPLE_RATE_HZ);
        while self.sample_accum >= CPU_HZ {
            self.sample_accum -= CPU_HZ;
            let mixed = (self.ch1.render_sample() + self.ch2.render_sample()).clamp(-MIX_PEAK, MIX_PEAK);
            let sample = quantize_4bit(mixed);
            self.pending.push(sample);
            self.pending.push(sample);
        }
    }

    pub fn drain_samples_into(&mut self, out: &mut Vec<i16>) {
        out.extend_from_slice(self.pending.as_slice());
        self.pending.clear();
    }

    fn trigger_ch1(&mut self) {
        let duty = self.regs[0x11] >> 6;
        let length = self.regs[0x11] & 0x3F;
        let envelope = self.regs[0x12];
        let frequency_raw = self.regs[0x13] as u16 | (((self.regs[0x14] & 0x07) as u16) << 8);
        self.ch1
            .trigger(duty, envelope, length, self.regs[0x14] & 0x40 != 0, frequency_raw);
    }

    fn trigger_ch2(&mut self) {
        let duty = self.regs[0x16] >> 6;
        let length = self.regs[0x16] & 0x3F;
        let envelope = self.regs[0x17];
        let frequency_raw = self.regs[0x18] as u16 | (((self.regs[0x19] & 0x07) as u16) << 8);
        self.ch2
            .trigger(duty, envelope, length, self.regs[0x19] & 0x40 != 0, frequency_raw);
    }
}

fn quantize_4bit(sample: i32) -> i16 {
    let clamped = sample.clamp(-MIX_PEAK, MIX_PEAK);
    let level = ((clamped + MIX_PEAK) * 15 + MIX_PEAK) / (MIX_PEAK * 2);
    ((level * (MIX_PEAK * 2) / 15) - MIX_PEAK) as i16
}
