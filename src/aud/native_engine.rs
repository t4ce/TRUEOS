//! Native fixed-width render backend for Strudel-compatible Blueprint commands.

use alloc::{vec, vec::Vec};
use spin::Mutex;

use super::tables::{MIDI_FREQ, SINE_TABLE};

pub const BLOCK_MAGIC_V1: u32 = 0x314E_5254;
pub const BLOCK_VERSION_V1: u16 = 1;
pub const COMMAND_SIZE_V1: u16 = 80;
pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const KIND_OSCILLATOR: u16 = 1;
pub const KIND_SAMPLE: u16 = 2;
pub const FLAG_SAMPLE_LOOP: u32 = 1;
pub const MAX_COMMANDS: usize = 512;
pub const MAX_BLOCK_FRAMES: usize = 9_600;
const MAX_SAMPLES: usize = 128;
const MAX_SAMPLE_VALUES: usize = 48_000 * 2 * 300;
const MAX_TOTAL_SAMPLE_VALUES: usize = 48_000 * 2 * 600;
const DELAY_FRAMES: usize = 48_000;
const DELAY_TAP: usize = 12_000;
const Q15: i64 = 32_767;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeBlockHeaderV1 {
    pub magic: u32,
    pub version: u16,
    pub command_size: u16,
    pub block_frames: u32,
    pub sample_rate_hz: u32,
    pub absolute_frame: u64,
    pub revision: u64,
    pub flags: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeRenderCommandV1 {
    pub start_frame: u32,
    pub end_frame: u32,
    pub age_frames: u32,
    pub duration_frames: u32,
    pub source_id: u64,
    pub voice_id: u32,
    pub kind: u16,
    pub waveform: u8,
    pub midi_note: u8,
    pub gain_q15: u16,
    pub pan_q15: i16,
    pub playback_rate_q16: i32,
    pub sample_begin_q16: u32,
    pub sample_end_q16: u32,
    pub lpf_hz: u16,
    pub lpq_q8: u16,
    pub room_q15: u16,
    pub delay_q15: u16,
    pub phaser_q15: u16,
    pub shape_q15: u16,
    pub fm_depth_q8: u16,
    pub fm_rate_q8: u16,
    pub flags: u32,
    pub reserved0: u32,
    pub reserved1: u32,
    pub reserved2: u32,
}

const _: [(); 40] = [(); core::mem::size_of::<NativeBlockHeaderV1>()];
const _: [(); 80] = [(); core::mem::size_of::<NativeRenderCommandV1>()];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    MissingSample,
    NoSpace,
}

struct Sample {
    id: u64,
    channels: u16,
    rate_hz: u32,
    values: Vec<i16>,
}

struct Engine {
    samples: Vec<Sample>,
    delay_l: Vec<i32>,
    delay_r: Vec<i32>,
    delay_cursor: usize,
}

impl Engine {
    const fn new() -> Self {
        Self {
            samples: Vec::new(),
            delay_l: Vec::new(),
            delay_r: Vec::new(),
            delay_cursor: 0,
        }
    }

    fn register(
        &mut self,
        id: u64,
        channels: u16,
        rate_hz: u32,
        values: &[i16],
    ) -> Result<(), Error> {
        if id == 0
            || !matches!(channels, 1 | 2)
            || !(8_000..=192_000).contains(&rate_hz)
            || values.is_empty()
            || !values.len().is_multiple_of(channels as usize)
            || values.len() > MAX_SAMPLE_VALUES
        {
            return Err(Error::Invalid);
        }
        let replaced = self
            .samples
            .iter()
            .find(|s| s.id == id)
            .map_or(0, |s| s.values.len());
        let total = self.samples.iter().map(|s| s.values.len()).sum::<usize>();
        if total.saturating_sub(replaced).saturating_add(values.len()) > MAX_TOTAL_SAMPLE_VALUES {
            return Err(Error::NoSpace);
        }
        if let Some(sample) = self.samples.iter_mut().find(|s| s.id == id) {
            sample.channels = channels;
            sample.rate_hz = rate_hz;
            sample.values.clear();
            sample.values.extend_from_slice(values);
            return Ok(());
        }
        if self.samples.len() >= MAX_SAMPLES {
            return Err(Error::NoSpace);
        }
        self.samples.push(Sample {
            id,
            channels,
            rate_hz,
            values: Vec::from(values),
        });
        Ok(())
    }

    fn remove(&mut self, id: u64) -> bool {
        let Some(index) = self.samples.iter().position(|s| s.id == id) else {
            return false;
        };
        self.samples.swap_remove(index);
        true
    }

    fn render(
        &mut self,
        header: &NativeBlockHeaderV1,
        commands: &[NativeRenderCommandV1],
    ) -> Result<Vec<i16>, Error> {
        validate(header, commands)?;
        let frames = header.block_frames as usize;
        let mut mix = vec![0i64; frames * 2];
        let mut delay_send = vec![0i64; frames * 2];
        for command in commands {
            let sample = if command.kind == KIND_SAMPLE {
                Some(
                    self.samples
                        .iter()
                        .find(|s| s.id == command.source_id)
                        .ok_or(Error::MissingSample)?,
                )
            } else {
                None
            };
            let pan = i64::from(command.pan_q15);
            let gain = i64::from(command.gain_q15.min(32_767));
            let left_gain = if pan > 0 { Q15 - pan } else { Q15 };
            let right_gain = if pan < 0 { Q15 + pan } else { Q15 };
            let mut lowpass = 0i64;
            let cutoff = u64::from(command.lpf_hz).min(24_000);
            let alpha = if cutoff == 0 || cutoff >= 23_900 {
                Q15
            } else {
                (cutoff * Q15 as u64 / (cutoff + SAMPLE_RATE_HZ as u64)) as i64
            };
            for frame in command.start_frame as usize..command.end_frame as usize {
                let age =
                    u64::from(command.age_frames) + (frame - command.start_frame as usize) as u64;
                let raw =
                    sample.map_or_else(|| oscillator(command, age), |s| sample_at(s, command, age));
                let env = envelope(age, u64::from(command.duration_frames));
                let mut value = i64::from(raw) * gain / Q15 * env / Q15;
                value = shape(value, command.shape_q15);
                if command.phaser_q15 != 0 {
                    let phase = age * 4 * 256 / SAMPLE_RATE_HZ as u64;
                    let modulation = i64::from(SINE_TABLE[phase as usize & 255]);
                    value = value
                        * (Q15 - i64::from(command.phaser_q15) / 2
                            + modulation * i64::from(command.phaser_q15) / (2 * Q15))
                        / Q15;
                }
                lowpass += (value - lowpass) * alpha / Q15;
                let index = frame * 2;
                let left = lowpass * left_gain / Q15;
                let right = lowpass * right_gain / Q15;
                mix[index] += left;
                mix[index + 1] += right;
                let send = i64::from(
                    command
                        .delay_q15
                        .saturating_add(command.room_q15)
                        .min(32_767),
                );
                delay_send[index] += left * send / Q15;
                delay_send[index + 1] += right * send / Q15;
            }
        }
        self.apply_delay(&mut mix, &delay_send, frames);
        Ok(mix
            .into_iter()
            .map(|v| v.clamp(i16::MIN as i64, i16::MAX as i64) as i16)
            .collect())
    }

    fn apply_delay(&mut self, mix: &mut [i64], send: &[i64], frames: usize) {
        if self.delay_l.len() != DELAY_FRAMES {
            self.delay_l = vec![0; DELAY_FRAMES];
            self.delay_r = vec![0; DELAY_FRAMES];
            self.delay_cursor = 0;
        }
        for frame in 0..frames {
            let read = (self.delay_cursor + DELAY_FRAMES - DELAY_TAP) % DELAY_FRAMES;
            let wet_l = i64::from(self.delay_l[read]);
            let wet_r = i64::from(self.delay_r[read]);
            let index = frame * 2;
            mix[index] += wet_l / 2;
            mix[index + 1] += wet_r / 2;
            self.delay_l[self.delay_cursor] =
                (send[index] + wet_l * 3 / 8).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            self.delay_r[self.delay_cursor] =
                (send[index + 1] + wet_r * 3 / 8).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            self.delay_cursor = (self.delay_cursor + 1) % DELAY_FRAMES;
        }
    }
}

static ENGINE: Mutex<Engine> = Mutex::new(Engine::new());

pub fn register_sample_v1(
    id: u64,
    channels: u16,
    rate_hz: u32,
    values: &[i16],
) -> Result<(), Error> {
    ENGINE.lock().register(id, channels, rate_hz, values)
}
pub fn remove_sample_v1(id: u64) -> bool {
    ENGINE.lock().remove(id)
}
pub fn render_block_v1(
    header: &NativeBlockHeaderV1,
    commands: &[NativeRenderCommandV1],
) -> Result<Vec<i16>, Error> {
    ENGINE.lock().render(header, commands)
}

fn validate(header: &NativeBlockHeaderV1, commands: &[NativeRenderCommandV1]) -> Result<(), Error> {
    if header.magic != BLOCK_MAGIC_V1
        || header.version != BLOCK_VERSION_V1
        || header.command_size != COMMAND_SIZE_V1
        || header.sample_rate_hz != SAMPLE_RATE_HZ
        || header.block_frames == 0
        || header.block_frames as usize > MAX_BLOCK_FRAMES
        || header.reserved != 0
        || commands.len() > MAX_COMMANDS
    {
        return Err(Error::Invalid);
    }
    if commands.iter().any(|c| {
        c.start_frame >= c.end_frame
            || c.end_frame > header.block_frames
            || c.duration_frames == 0
            || !matches!(c.kind, KIND_OSCILLATOR | KIND_SAMPLE)
            || c.waveform > 4
            || c.sample_begin_q16 > c.sample_end_q16
            || c.sample_end_q16 > 65_536
            || (c.kind == KIND_SAMPLE && c.playback_rate_q16 <= 0)
            || c.reserved0 != 0
            || c.reserved1 != 0
            || c.reserved2 != 0
    }) {
        return Err(Error::Invalid);
    }
    Ok(())
}

fn oscillator(command: &NativeRenderCommandV1, age: u64) -> i16 {
    let base = u64::from(MIDI_FREQ[command.midi_note as usize]);
    let fm_phase = age * u64::from(command.fm_rate_q8) * 256 / (SAMPLE_RATE_HZ as u64 * 256);
    let fm = i64::from(SINE_TABLE[fm_phase as usize & 255]) * i64::from(command.fm_depth_q8)
        / (32_767 * 256);
    let phase = age * (base as i64 + fm).max(1) as u64 * 256 / SAMPLE_RATE_HZ as u64;
    let i = phase as usize & 255;
    match command.waveform {
        1 => {
            if i < 128 {
                i16::MAX
            } else {
                i16::MIN + 1
            }
        }
        2 => ((i as i32 * 257) - 32_767) as i16,
        3 => ((if i < 128 { i } else { 255 - i }) as i32 * 516 - 32_767) as i16,
        4 => {
            let mut n = age ^ u64::from(command.voice_id) ^ command.source_id;
            n ^= n << 13;
            n ^= n >> 7;
            n ^= n << 17;
            n as i16
        }
        _ => SINE_TABLE[i],
    }
}

fn sample_at(sample: &Sample, command: &NativeRenderCommandV1, age: u64) -> i16 {
    let frames = sample.values.len() / sample.channels as usize;
    let begin = (u64::from(command.sample_begin_q16) * frames as u64 / 65_536) as usize;
    let raw_end = (u64::from(command.sample_end_q16) * frames as u64 / 65_536) as usize;
    let end = if raw_end == 0 { frames } else { raw_end }.clamp(begin.saturating_add(1), frames);
    let region = end - begin;
    let source_q16 =
        age * sample.rate_hz as u64 * command.playback_rate_q16 as u64 / SAMPLE_RATE_HZ as u64;
    let mut frame = (source_q16 >> 16) as usize;
    if command.flags & FLAG_SAMPLE_LOOP != 0 {
        frame %= region;
    } else if frame >= region {
        return 0;
    }
    let index = (begin + frame) * sample.channels as usize;
    if sample.channels == 1 {
        sample.values[index]
    } else {
        ((i32::from(sample.values[index]) + i32::from(sample.values[index + 1])) / 2) as i16
    }
}

fn envelope(age: u64, duration: u64) -> i64 {
    (age * Q15 as u64 / 240)
        .min(duration.saturating_sub(age) * Q15 as u64 / 960)
        .min(Q15 as u64) as i64
}
fn shape(sample: i64, amount: u16) -> i64 {
    if amount == 0 {
        return sample;
    }
    let driven = sample.saturating_mul(Q15 + i64::from(amount) * 3) / Q15;
    driven * Q15 / (driven.abs() + Q15)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header(frames: u32) -> NativeBlockHeaderV1 {
        NativeBlockHeaderV1 {
            magic: BLOCK_MAGIC_V1,
            version: 1,
            command_size: 80,
            block_frames: frames,
            sample_rate_hz: 48_000,
            revision: 1,
            ..Default::default()
        }
    }
    fn osc(frames: u32) -> NativeRenderCommandV1 {
        NativeRenderCommandV1 {
            end_frame: frames,
            duration_frames: frames,
            voice_id: 1,
            kind: 1,
            midi_note: 69,
            gain_q15: 24_000,
            playback_rate_q16: 65_536,
            sample_end_q16: 65_536,
            lpf_hz: 24_000,
            ..Default::default()
        }
    }
    #[test]
    fn abi_sizes_are_frozen() {
        assert_eq!(core::mem::size_of::<NativeBlockHeaderV1>(), 40);
        assert_eq!(core::mem::size_of::<NativeRenderCommandV1>(), 80);
    }
    #[test]
    fn oscillator_is_stereo_and_audible() {
        let pcm = Engine::new().render(&header(480), &[osc(480)]).unwrap();
        assert_eq!(pcm.len(), 960);
        assert!(pcm.iter().any(|v| *v != 0));
    }
    #[test]
    fn invalid_span_is_rejected() {
        let mut c = osc(480);
        c.end_frame = 481;
        assert_eq!(validate(&header(480), &[c]), Err(Error::Invalid));
    }
}
