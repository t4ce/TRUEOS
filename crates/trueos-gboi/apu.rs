//! Small Game Boy APU renderer.
//!
//! This is intentionally lightweight rather than register-perfect. It keeps the
//! Game Boy MMIO surface and renders enough pulse/noise PCM for kernel demos to
//! feed the existing HDA path.

use alloc::vec::Vec;

const NR52_INDEX: usize = (0xFF26 - 0xFF10) as usize;
const SAMPLE_RATE_HZ: u32 = 48_000;
const CPU_T_CYCLES_HZ: u32 = 4_194_304;
const PHASE_ONE: u32 = 1 << 24;
const DUTY_PATTERNS: [[bool; 8]; 4] = [
    [false, false, false, false, false, false, false, true],
    [true, false, false, false, false, false, false, true],
    [true, false, false, false, false, true, true, true],
    [false, true, true, true, true, true, true, false],
];

pub struct Apu {
    regs: [u8; 0x30],
    pulse1: PulseVoice,
    pulse2: PulseVoice,
    noise: NoiseVoice,
    sample_tick_accum: u64,
    sample_buf: Vec<i16>,
}

#[derive(Clone, Copy)]
struct PulseVoice {
    enabled: bool,
    phase: u32,
}

#[derive(Clone, Copy)]
struct NoiseVoice {
    enabled: bool,
    lfsr: u16,
    divider: u32,
}

impl Apu {
    pub fn new() -> Self {
        let mut regs = [0u8; 0x30];
        regs[NR52_INDEX] = 0xF1;
        Self {
            regs,
            pulse1: PulseVoice::new(),
            pulse2: PulseVoice::new(),
            noise: NoiseVoice::new(),
            sample_tick_accum: 0,
            sample_buf: Vec::new(),
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF26 => self.regs[NR52_INDEX] | 0x70,
            0xFF10..=0xFF3F => self.regs[(addr - 0xFF10) as usize],
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0xFF26 => {
                self.regs[NR52_INDEX] = (self.regs[NR52_INDEX] & 0x0F) | (val & 0x80);
                if val & 0x80 == 0 {
                    self.pulse1.enabled = false;
                    self.pulse2.enabled = false;
                    self.noise.enabled = false;
                    self.sample_buf.clear();
                }
            }
            0xFF10..=0xFF3F => {
                self.regs[(addr - 0xFF10) as usize] = val;
                match addr {
                    0xFF14 if val & 0x80 != 0 => self.trigger_pulse(1),
                    0xFF19 if val & 0x80 != 0 => self.trigger_pulse(2),
                    0xFF23 if val & 0x80 != 0 => self.trigger_noise(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub fn step(&mut self, t_cycles: u32) {
        if !self.enabled() {
            return;
        }

        self.sample_tick_accum = self
            .sample_tick_accum
            .saturating_add(u64::from(t_cycles) * u64::from(SAMPLE_RATE_HZ));
        while self.sample_tick_accum >= u64::from(CPU_T_CYCLES_HZ) {
            self.sample_tick_accum -= u64::from(CPU_T_CYCLES_HZ);
            let sample = self.render_sample();
            self.sample_buf.push(sample);
            self.sample_buf.push(sample);
        }
    }

    pub fn drain_samples_into(&mut self, out: &mut Vec<i16>) {
        out.extend_from_slice(self.sample_buf.as_slice());
        self.sample_buf.clear();
    }

    fn enabled(&self) -> bool {
        self.regs[NR52_INDEX] & 0x80 != 0
    }

    fn reg(&self, addr: u16) -> u8 {
        self.regs[(addr - 0xFF10) as usize]
    }

    fn trigger_pulse(&mut self, channel: u8) {
        if !self.enabled() {
            return;
        }
        let envelope = self.pulse_envelope(channel);
        let enabled = envelope != 0 && self.pulse_frequency(channel) != 0;
        let voice = if channel == 1 {
            &mut self.pulse1
        } else {
            &mut self.pulse2
        };
        voice.enabled = enabled;
        voice.phase = 0;
        self.update_nr52_channel_bits();
    }

    fn trigger_noise(&mut self) {
        if !self.enabled() {
            return;
        }
        self.noise.enabled = self.noise_envelope() != 0;
        self.noise.lfsr = 0x7FFF;
        self.update_nr52_channel_bits();
    }

    fn update_nr52_channel_bits(&mut self) {
        let mut bits = 0u8;
        if self.pulse1.enabled {
            bits |= 0x01;
        }
        if self.pulse2.enabled {
            bits |= 0x02;
        }
        if self.noise.enabled {
            bits |= 0x08;
        }
        self.regs[NR52_INDEX] = (self.regs[NR52_INDEX] & 0xF0) | bits;
    }

    fn pulse_envelope(&self, channel: u8) -> i32 {
        let reg = if channel == 1 {
            self.reg(0xFF12)
        } else {
            self.reg(0xFF17)
        };
        i32::from(reg >> 4)
    }

    fn noise_envelope(&self) -> i32 {
        i32::from(self.reg(0xFF21) >> 4)
    }

    fn pulse_frequency(&self, channel: u8) -> u32 {
        let (lo_addr, hi_addr) = if channel == 1 {
            (0xFF13, 0xFF14)
        } else {
            (0xFF18, 0xFF19)
        };
        let raw = u16::from(self.reg(lo_addr)) | (u16::from(self.reg(hi_addr) & 0x07) << 8);
        if raw >= 2048 {
            0
        } else {
            131_072 / u32::from(2048 - raw)
        }
    }

    fn pulse_duty(&self, channel: u8) -> usize {
        let reg = if channel == 1 {
            self.reg(0xFF11)
        } else {
            self.reg(0xFF16)
        };
        usize::from((reg >> 6) & 0x03)
    }

    fn noise_period_frames(&self) -> u32 {
        let nr43 = self.reg(0xFF22);
        let divisor_code = u32::from(nr43 & 0x07);
        let divisor = if divisor_code == 0 {
            8
        } else {
            divisor_code * 16
        };
        let shift = u32::from(nr43 >> 4).min(13);
        let freq = CPU_T_CYCLES_HZ / (divisor << shift).max(1);
        (SAMPLE_RATE_HZ / freq.max(1)).max(1)
    }

    fn render_sample(&mut self) -> i16 {
        let mut mix = 0i32;

        if self.pulse1.enabled {
            mix += self.render_pulse(1);
        }
        if self.pulse2.enabled {
            mix += self.render_pulse(2);
        }
        if self.noise.enabled {
            mix += self.render_noise();
        }

        quantize_4bit(mix)
    }

    fn render_pulse(&mut self, channel: u8) -> i32 {
        let freq = self.pulse_frequency(channel);
        let env = self.pulse_envelope(channel);
        if freq == 0 || env == 0 {
            return 0;
        }

        let duty = self.pulse_duty(channel);
        let phase_step = ((freq as u64 * PHASE_ONE as u64) / SAMPLE_RATE_HZ as u64) as u32;
        let voice = if channel == 1 {
            &mut self.pulse1
        } else {
            &mut self.pulse2
        };
        voice.phase = voice.phase.wrapping_add(phase_step);
        let idx = usize::from(((voice.phase >> 21) & 0x07) as u8);
        let amp = env * 1_300;
        if DUTY_PATTERNS[duty][idx] { amp } else { -amp }
    }

    fn render_noise(&mut self) -> i32 {
        let env = self.noise_envelope();
        if env == 0 {
            return 0;
        }

        let period = self.noise_period_frames();
        self.noise.phase_tick(period);
        let amp = env * 850;
        if self.noise.lfsr & 1 == 0 { amp } else { -amp }
    }
}

impl PulseVoice {
    const fn new() -> Self {
        Self {
            enabled: false,
            phase: 0,
        }
    }
}

impl NoiseVoice {
    const fn new() -> Self {
        Self {
            enabled: false,
            lfsr: 0x7FFF,
            divider: 0,
        }
    }

    fn phase_tick(&mut self, period: u32) {
        self.divider = self.divider.saturating_add(1);
        if self.divider >= period {
            self.divider = 0;
            let bit = (self.lfsr ^ (self.lfsr >> 1)) & 1;
            self.lfsr = (self.lfsr >> 1) | (bit << 14);
        }
    }
}

fn quantize_4bit(sample: i32) -> i16 {
    let clamped = sample.clamp(-24_000, 24_000);
    let level = ((clamped + 24_000) * 15 + 24_000) / 48_000;
    (level * 3_200 - 24_000) as i16
}
