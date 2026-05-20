//! Minimal Game Boy APU register capture + pulse renderer.
//!
//! This is an incremental audio core, not a cycle-perfect DMG/CGB APU yet. It
//! preserves the sound register surface and emits host-rate samples from the two
//! pulse channels so ROM audio writes start to matter.

use alloc::vec::Vec;

const CPU_HZ: u64 = 4_194_304;
const SAMPLE_RATE_HZ: u64 = 48_000;
const CHANNELS: usize = 2;

#[derive(Clone, Copy)]
struct PulseChannel {
    enabled: bool,
    duty: u8,
    volume: u8,
    frequency_raw: u16,
    phase: u32,
}

impl PulseChannel {
    const fn new() -> Self {
        Self {
            enabled: false,
            duty: 2,
            volume: 0,
            frequency_raw: 0,
            phase: 0,
        }
    }

    fn trigger(&mut self, duty: u8, volume: u8, frequency_raw: u16) {
        self.enabled = volume != 0 && frequency_raw < 2048;
        self.duty = duty & 0x03;
        self.volume = volume & 0x0F;
        self.frequency_raw = frequency_raw & 0x07FF;
        self.phase = 0;
    }

    fn render_sample(&mut self) -> i32 {
        if !self.enabled || self.volume == 0 || self.frequency_raw >= 2048 {
            return 0;
        }

        let period = (2048u32.saturating_sub(self.frequency_raw as u32)).max(1);
        let step = ((131_072u64 << 16) / (SAMPLE_RATE_HZ * period as u64)).max(1) as u32;
        self.phase = self.phase.wrapping_add(step);
        let pos = (self.phase >> 13) & 7;
        let high = match self.duty {
            0 => pos == 7,                  // 12.5%
            1 => pos >= 6,                  // 25%
            2 => pos >= 4,                  // 50%
            _ => pos == 0 || pos >= 5,      // 75%
        };
        let amp = self.volume as i32 * 900;
        if high { amp } else { -amp }
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
        regs[0x26] = 0xF1; // NR52-ish post-boot: audio enabled, channel flags readable.
        Self {
            regs,
            ch1: PulseChannel::new(),
            ch2: PulseChannel::new(),
            sample_accum: 0,
            pending: Vec::with_capacity((SAMPLE_RATE_HZ as usize / 30) * CHANNELS),
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        let idx = (addr - 0xFF10) as usize;
        match addr {
            0xFF26 => {
                let mut v = self.regs[idx] | 0x70;
                if self.ch1.enabled { v |= 0x01; }
                if self.ch2.enabled { v |= 0x02; }
                v
            }
            0xFF10..=0xFF3F => self.regs[idx],
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        let idx = (addr - 0xFF10) as usize;
        if addr == 0xFF26 {
            self.regs[idx] = (self.regs[idx] & 0x0F) | (val & 0x80);
            if val & 0x80 == 0 {
                self.ch1.enabled = false;
                self.ch2.enabled = false;
            }
            return;
        }

        self.regs[idx] = val;
        if self.regs[0x26] & 0x80 == 0 {
            return;
        }

        match addr {
            0xFF14 if val & 0x80 != 0 => self.trigger_ch1(),
            0xFF19 if val & 0x80 != 0 => self.trigger_ch2(),
            _ => {}
        }
    }

    pub fn step(&mut self, t_cycles: u32) {
        if self.regs[0x26] & 0x80 == 0 {
            return;
        }

        self.sample_accum = self
            .sample_accum
            .saturating_add(t_cycles as u64 * SAMPLE_RATE_HZ);
        while self.sample_accum >= CPU_HZ {
            self.sample_accum -= CPU_HZ;
            let mixed = self.ch1.render_sample() + self.ch2.render_sample();
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
        let volume = self.regs[0x12] >> 4;
        let frequency_raw = self.regs[0x13] as u16 | (((self.regs[0x14] & 0x07) as u16) << 8);
        self.ch1.trigger(duty, volume, frequency_raw);
    }

    fn trigger_ch2(&mut self) {
        let duty = self.regs[0x16] >> 6;
        let volume = self.regs[0x17] >> 4;
        let frequency_raw = self.regs[0x18] as u16 | (((self.regs[0x19] & 0x07) as u16) << 8);
        self.ch2.trigger(duty, volume, frequency_raw);
    }
}

fn quantize_4bit(sample: i32) -> i16 {
    let clamped = sample.clamp(-24_000, 24_000);
    let level = ((clamped + 24_000) * 15 + 24_000) / 48_000;
    (level * 3_200 - 24_000) as i16
}
