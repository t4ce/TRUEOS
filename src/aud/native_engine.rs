//! Native fixed-width render backend for Strudel-compatible Blueprint commands.

use alloc::{vec, vec::Vec};
use spin::Mutex;

use super::tables::{MIDI_FREQ, SINE_TABLE};

pub const BLOCK_MAGIC_V1: u32 = 0x314E_5254;
pub const BLOCK_VERSION_V1: u16 = 1;
pub const COMMAND_SIZE_V1: u16 = 80;
pub const BLOCK_VERSION_V2: u16 = 2;
pub const COMMAND_SIZE_V2: u16 = 104;
pub const BLOCK_VERSION_V3: u16 = 3;
pub const COMMAND_SIZE_V3: u16 = 112;
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
const ROOM_FRAMES: usize = 4_096;
const ROOM_TAPS: [usize; 4] = [1_421, 1_553, 1_733, 1_997];
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

/// V2 deliberately retains the frozen 40-byte block header layout.
pub type NativeBlockHeaderV2 = NativeBlockHeaderV1;
/// V3 deliberately retains the frozen 40-byte block header layout.
pub type NativeBlockHeaderV3 = NativeBlockHeaderV1;

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

/// Additive native command surface for envelope-aware renderers.
///
/// The v1 command is an exact 80-byte prefix so a host can translate or inspect
/// common fields without maintaining two subtly different definitions.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeRenderCommandV2 {
    pub base: NativeRenderCommandV1,
    pub attack_frames: u32,
    pub decay_frames: u32,
    pub release_frames: u32,
    pub filter_attack_frames: u32,
    pub filter_decay_frames: u32,
    pub sustain_q15: u16,
    /// Signed filter-envelope depth in octaves, Q8 (for example 4.0 = 1024).
    pub filter_env_octaves_q8: i16,
}

/// V3 adds selectable Strudel filter topology while retaining V2 as an exact
/// prefix. Values match Strudel's public `ftype` control: 0=12db, 1=ladder,
/// 2=24db. Reserved bytes must remain zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeRenderCommandV3 {
    pub base: NativeRenderCommandV2,
    pub filter_type: u8,
    pub reserved3: [u8; 3],
    pub reserved4: u32,
}

const _: [(); 40] = [(); core::mem::size_of::<NativeBlockHeaderV1>()];
const _: [(); 80] = [(); core::mem::size_of::<NativeRenderCommandV1>()];
const _: [(); 104] = [(); core::mem::size_of::<NativeRenderCommandV2>()];
const _: [(); 112] = [(); core::mem::size_of::<NativeRenderCommandV3>()];

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
    room_l: Vec<i32>,
    room_r: Vec<i32>,
    room_cursor: usize,
    voice_filters: Vec<VoiceFilterState>,
}

#[derive(Clone, Copy)]
struct VoiceFilterState {
    revision: u64,
    voice_id: u32,
    source_id: u64,
    kind: u16,
    waveform: u8,
    pole_1: i64,
    pole_2: i64,
    pole_3: i64,
    pole_4: i64,
    last_end_frame: u64,
    seen_block: u64,
}

#[derive(Clone, Copy)]
struct EnvelopeParams {
    attack_frames: u32,
    decay_frames: u32,
    sustain_q15: u16,
    release_frames: u32,
    filter_attack_frames: u32,
    filter_decay_frames: u32,
    filter_env_octaves_q8: i16,
    release_after_gate: bool,
}

impl EnvelopeParams {
    const V1: Self = Self {
        attack_frames: 240,
        decay_frames: 0,
        sustain_q15: 32_767,
        release_frames: 960,
        filter_attack_frames: 0,
        filter_decay_frames: 0,
        filter_env_octaves_q8: 0,
        release_after_gate: false,
    };
}

impl Engine {
    const fn new() -> Self {
        Self {
            samples: Vec::new(),
            delay_l: Vec::new(),
            delay_r: Vec::new(),
            delay_cursor: 0,
            room_l: Vec::new(),
            room_r: Vec::new(),
            room_cursor: 0,
            voice_filters: Vec::new(),
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

    fn render_v1(
        &mut self,
        header: &NativeBlockHeaderV1,
        commands: &[NativeRenderCommandV1],
    ) -> Result<Vec<i16>, Error> {
        validate_v1(header, commands)?;
        let envelopes = vec![EnvelopeParams::V1; commands.len()];
        let filter_types = vec![0; commands.len()];
        self.render_inner(header, commands, &envelopes, &filter_types, false)
    }

    fn render_v2(
        &mut self,
        header: &NativeBlockHeaderV2,
        commands: &[NativeRenderCommandV2],
    ) -> Result<Vec<i16>, Error> {
        validate_v2(header, commands)?;
        let bases = commands
            .iter()
            .map(|command| command.base)
            .collect::<Vec<_>>();
        let envelopes = commands
            .iter()
            .map(|command| EnvelopeParams {
                attack_frames: command.attack_frames,
                decay_frames: command.decay_frames,
                sustain_q15: command.sustain_q15,
                release_frames: command.release_frames,
                filter_attack_frames: command.filter_attack_frames,
                filter_decay_frames: command.filter_decay_frames,
                filter_env_octaves_q8: command.filter_env_octaves_q8,
                release_after_gate: true,
            })
            .collect::<Vec<_>>();
        let filter_types = vec![0; commands.len()];
        self.render_inner(header, &bases, &envelopes, &filter_types, true)
    }

    fn render_v3(
        &mut self,
        header: &NativeBlockHeaderV2,
        commands: &[NativeRenderCommandV3],
    ) -> Result<Vec<i16>, Error> {
        validate_v3(header, commands)?;
        let bases = commands
            .iter()
            .map(|command| command.base.base)
            .collect::<Vec<_>>();
        let envelopes = commands
            .iter()
            .map(|command| EnvelopeParams {
                attack_frames: command.base.attack_frames,
                decay_frames: command.base.decay_frames,
                sustain_q15: command.base.sustain_q15,
                release_frames: command.base.release_frames,
                filter_attack_frames: command.base.filter_attack_frames,
                filter_decay_frames: command.base.filter_decay_frames,
                filter_env_octaves_q8: command.base.filter_env_octaves_q8,
                release_after_gate: true,
            })
            .collect::<Vec<_>>();
        let filter_types = commands
            .iter()
            .map(|command| command.filter_type)
            .collect::<Vec<_>>();
        self.render_inner(header, &bases, &envelopes, &filter_types, true)
    }

    fn render_inner(
        &mut self,
        header: &NativeBlockHeaderV1,
        commands: &[NativeRenderCommandV1],
        envelopes: &[EnvelopeParams],
        filter_types: &[u8],
        use_soft_limiter: bool,
    ) -> Result<Vec<i16>, Error> {
        let frames = header.block_frames as usize;
        let mut mix = vec![0i64; frames * 2];
        let mut delay_send = vec![0i64; frames * 2];
        let mut room_send = vec![0i64; frames * 2];
        for ((command, envelope_params), filter_type) in
            commands.iter().zip(envelopes).zip(filter_types)
        {
            let sample_index = if command.kind == KIND_SAMPLE {
                Some(
                    self.samples
                        .iter()
                        .position(|sample| sample.id == command.source_id)
                        .ok_or(Error::MissingSample)?,
                )
            } else {
                None
            };
            let filter_index = self.filter_state_index(header, command);
            let pan = i64::from(command.pan_q15);
            let gain = i64::from(command.gain_q15.min(32_767));
            let left_gain = if pan > 0 { Q15 - pan } else { Q15 };
            let right_gain = if pan < 0 { Q15 + pan } else { Q15 };
            for frame in command.start_frame as usize..command.end_frame as usize {
                let age =
                    u64::from(command.age_frames) + (frame - command.start_frame as usize) as u64;
                let raw = sample_index.map_or_else(
                    || oscillator(command, age),
                    |index| sample_at(&self.samples[index], command, age),
                );
                let env = envelope_at(age, u64::from(command.duration_frames), *envelope_params);
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
                let filter_env = filter_envelope_at(age, *envelope_params);
                let cutoff = modulated_cutoff(
                    command.lpf_hz,
                    envelope_params.filter_env_octaves_q8,
                    filter_env,
                );
                let filtered = lowpass(
                    &mut self.voice_filters[filter_index],
                    value,
                    cutoff,
                    command.lpq_q8,
                    *filter_type,
                );
                let index = frame * 2;
                let left = filtered * left_gain / Q15;
                let right = filtered * right_gain / Q15;
                mix[index] += left;
                mix[index + 1] += right;
                let delay = i64::from(command.delay_q15.min(32_767));
                delay_send[index] += left * delay / Q15;
                delay_send[index + 1] += right * delay / Q15;
                let room = i64::from(command.room_q15.min(32_767));
                room_send[index] += left * room / Q15;
                room_send[index + 1] += right * room / Q15;
            }
            self.voice_filters[filter_index].last_end_frame = header
                .absolute_frame
                .saturating_add(u64::from(command.end_frame));
        }
        self.voice_filters
            .retain(|state| state.seen_block == header.absolute_frame);
        self.apply_delay(&mut mix, &delay_send, frames);
        self.apply_room(&mut mix, &room_send, frames);
        Ok(mix
            .into_iter()
            .map(|sample| {
                if use_soft_limiter {
                    soft_limit_i16(sample)
                } else {
                    sample.clamp(i16::MIN as i64, i16::MAX as i64) as i16
                }
            })
            .collect())
    }

    fn filter_state_index(
        &mut self,
        header: &NativeBlockHeaderV1,
        command: &NativeRenderCommandV1,
    ) -> usize {
        let start = header
            .absolute_frame
            .saturating_add(u64::from(command.start_frame));
        if let Some(index) = self.voice_filters.iter().position(|state| {
            state.revision == header.revision
                && state.voice_id == command.voice_id
                && state.source_id == command.source_id
                && state.kind == command.kind
                && state.waveform == command.waveform
        }) {
            let state = &mut self.voice_filters[index];
            if command.age_frames == 0 || state.last_end_frame != start {
                state.pole_1 = 0;
                state.pole_2 = 0;
                state.pole_3 = 0;
                state.pole_4 = 0;
            }
            state.seen_block = header.absolute_frame;
            return index;
        }
        self.voice_filters.push(VoiceFilterState {
            revision: header.revision,
            voice_id: command.voice_id,
            source_id: command.source_id,
            kind: command.kind,
            waveform: command.waveform,
            pole_1: 0,
            pole_2: 0,
            pole_3: 0,
            pole_4: 0,
            last_end_frame: start,
            seen_block: header.absolute_frame,
        });
        self.voice_filters.len() - 1
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

    fn apply_room(&mut self, mix: &mut [i64], send: &[i64], frames: usize) {
        if self.room_l.len() != ROOM_FRAMES {
            self.room_l = vec![0; ROOM_FRAMES];
            self.room_r = vec![0; ROOM_FRAMES];
            self.room_cursor = 0;
        }
        for frame in 0..frames {
            let mut wet_l = 0i64;
            let mut wet_r = 0i64;
            for tap in ROOM_TAPS {
                let read = (self.room_cursor + ROOM_FRAMES - tap) % ROOM_FRAMES;
                wet_l += i64::from(self.room_l[read]);
                wet_r += i64::from(self.room_r[read]);
            }
            wet_l /= ROOM_TAPS.len() as i64;
            wet_r /= ROOM_TAPS.len() as i64;
            let index = frame * 2;
            // Cross-coupled feedback avoids two identical mono combs while
            // remaining bounded under an arbitrary full-scale room send.
            mix[index] += wet_l / 3;
            mix[index + 1] += wet_r / 3;
            let next_l = send[index] + wet_l * 11 / 16 + wet_r / 16;
            let next_r = send[index + 1] + wet_r * 11 / 16 + wet_l / 16;
            self.room_l[self.room_cursor] = next_l.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            self.room_r[self.room_cursor] = next_r.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            self.room_cursor = (self.room_cursor + 1) % ROOM_FRAMES;
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
    ENGINE.lock().render_v1(header, commands)
}

pub fn render_block_v2(
    header: &NativeBlockHeaderV2,
    commands: &[NativeRenderCommandV2],
) -> Result<Vec<i16>, Error> {
    ENGINE.lock().render_v2(header, commands)
}

pub fn render_block_v3(
    header: &NativeBlockHeaderV2,
    commands: &[NativeRenderCommandV3],
) -> Result<Vec<i16>, Error> {
    ENGINE.lock().render_v3(header, commands)
}

fn validate_v1(
    header: &NativeBlockHeaderV1,
    commands: &[NativeRenderCommandV1],
) -> Result<(), Error> {
    if header.magic != BLOCK_MAGIC_V1
        || header.version != BLOCK_VERSION_V1
        || header.command_size != COMMAND_SIZE_V1
        || header.sample_rate_hz != SAMPLE_RATE_HZ
        || header.block_frames == 0
        || header.block_frames as usize > MAX_BLOCK_FRAMES
        || header.flags != 0
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

fn validate_v2(
    header: &NativeBlockHeaderV2,
    commands: &[NativeRenderCommandV2],
) -> Result<(), Error> {
    if header.magic != BLOCK_MAGIC_V1
        || header.version != BLOCK_VERSION_V2
        || header.command_size != COMMAND_SIZE_V2
        || header.sample_rate_hz != SAMPLE_RATE_HZ
        || header.block_frames == 0
        || header.block_frames as usize > MAX_BLOCK_FRAMES
        || header.flags != 0
        || header.reserved != 0
        || commands.len() > MAX_COMMANDS
    {
        return Err(Error::Invalid);
    }
    if commands.iter().any(|command| {
        let c = &command.base;
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
            || command.sustain_q15 > 32_767
            || !(-8 * 256..=8 * 256).contains(&i32::from(command.filter_env_octaves_q8))
    }) {
        return Err(Error::Invalid);
    }
    Ok(())
}

fn validate_v3(
    header: &NativeBlockHeaderV2,
    commands: &[NativeRenderCommandV3],
) -> Result<(), Error> {
    if header.magic != BLOCK_MAGIC_V1
        || header.version != BLOCK_VERSION_V3
        || header.command_size != COMMAND_SIZE_V3
        || header.sample_rate_hz != SAMPLE_RATE_HZ
        || header.block_frames == 0
        || header.block_frames as usize > MAX_BLOCK_FRAMES
        || header.flags != 0
        || header.reserved != 0
        || commands.len() > MAX_COMMANDS
    {
        return Err(Error::Invalid);
    }
    if commands.iter().any(|command| {
        let c = &command.base.base;
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
            || command.base.sustain_q15 > 32_767
            || !(-8 * 256..=8 * 256).contains(&i32::from(command.base.filter_env_octaves_q8))
            || command.filter_type > 2
            || command.reserved3 != [0; 3]
            || command.reserved4 != 0
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

fn envelope_at(age: u64, gate_duration: u64, params: EnvelopeParams) -> i64 {
    if !params.release_after_gate {
        return (age * Q15 as u64 / u64::from(params.attack_frames.max(1)))
            .min(
                gate_duration.saturating_sub(age) * Q15 as u64
                    / u64::from(params.release_frames.max(1)),
            )
            .min(Q15 as u64) as i64;
    }

    if age < gate_duration {
        return held_envelope_at(age, params);
    }
    let release = u64::from(params.release_frames);
    if release == 0 || age >= gate_duration.saturating_add(release) {
        return 0;
    }
    let release_start = held_envelope_at(gate_duration, params);
    release_start * (release - (age - gate_duration)) as i64 / release as i64
}

fn held_envelope_at(age: u64, params: EnvelopeParams) -> i64 {
    let attack = u64::from(params.attack_frames);
    let decay = u64::from(params.decay_frames);
    let sustain = i64::from(params.sustain_q15.min(32_767));
    if attack != 0 && age < attack {
        return (age * Q15 as u64 / attack) as i64;
    }
    if decay != 0 && age < attack.saturating_add(decay) {
        let elapsed = age.saturating_sub(attack);
        return Q15 - (Q15 - sustain) * elapsed as i64 / decay as i64;
    }
    sustain
}

fn filter_envelope_at(age: u64, params: EnvelopeParams) -> i64 {
    if params.filter_env_octaves_q8 == 0 {
        return 0;
    }
    let attack = u64::from(params.filter_attack_frames);
    let decay = u64::from(params.filter_decay_frames);
    if attack != 0 && age < attack {
        return (age * Q15 as u64 / attack) as i64;
    }
    let decay_age = age.saturating_sub(attack);
    if decay == 0 {
        Q15
    } else if decay_age >= decay {
        0
    } else {
        ((decay - decay_age) * Q15 as u64 / decay) as i64
    }
}

fn modulated_cutoff(base_hz: u16, depth_octaves_q8: i16, envelope_q15: i64) -> u16 {
    const POW2_Q16: [u32; 17] = [
        65_536, 68_436, 71_469, 74_636, 77_936, 81_384, 84_982, 88_744, 92_682, 96_815, 101_151,
        105_699, 110_474, 115_485, 120_747, 126_273, 131_072,
    ];
    let base = u64::from(base_hz).min(24_000);
    if base == 0 || depth_octaves_q8 == 0 || envelope_q15 == 0 {
        return base as u16;
    }
    let octaves_q8 = i64::from(depth_octaves_q8) * envelope_q15 / Q15;
    let magnitude = octaves_q8.unsigned_abs().min(8 * 256);
    let whole = (magnitude / 256) as u32;
    let fraction = (magnitude % 256) as u32;
    let table_index = (fraction / 16) as usize;
    let table_fraction = u64::from(fraction % 16);
    let low = u64::from(POW2_Q16[table_index]);
    let high = u64::from(POW2_Q16[table_index + 1]);
    let factor_q16 = low + (high - low) * table_fraction / 16;
    let shifted = if octaves_q8 >= 0 {
        base.saturating_mul(factor_q16)
            .saturating_mul(1u64 << whole)
            / 65_536
    } else {
        base.saturating_mul(65_536) / factor_q16 / (1u64 << whole)
    };
    shifted.clamp(1, 24_000) as u16
}

fn lowpass(
    state: &mut VoiceFilterState,
    input: i64,
    cutoff_hz: u16,
    resonance_q8: u16,
    filter_type: u8,
) -> i64 {
    let cutoff = u64::from(cutoff_hz).min(24_000);
    if cutoff == 0 {
        state.pole_1 = 0;
        state.pole_2 = 0;
        state.pole_3 = 0;
        state.pole_4 = 0;
        // The native ABI uses zero for an omitted LPF control. Treat that as
        // a disabled filter, not a zero-Hz filter which mutes the voice.
        return input;
    }
    if cutoff >= 23_900 {
        state.pole_1 = input;
        state.pole_2 = input;
        state.pole_3 = input;
        state.pole_4 = input;
        return input;
    }
    // Bilinear one-pole coefficient (2*pi*fc)/(fs + 2*pi*fc), Q15.
    let angular = cutoff.saturating_mul(6_283);
    let alpha =
        (angular.saturating_mul(Q15 as u64) / (SAMPLE_RATE_HZ as u64 * 1_000 + angular)) as i64;
    // Strudel lpq values above 16 are clamped to keep this two-pole feedback
    // topology stable under arbitrary Blueprint input.
    let resonance = i64::from(resonance_q8.min(16 * 256)) * 24_576 / (16 * 256);
    let feedback = if filter_type == 0 {
        state.pole_2
    } else {
        state.pole_4
    };
    let mut driven = input - feedback * resonance / Q15;
    if filter_type == 1 {
        // Bounded nonlinear drive gives the ladder mode its characteristic
        // stronger saturation without floating point in the audio path.
        driven = driven.saturating_mul(Q15 * 2) / (driven.abs() + Q15);
    }
    state.pole_1 += (driven - state.pole_1) * alpha / Q15;
    state.pole_2 += (state.pole_1 - state.pole_2) * alpha / Q15;
    if filter_type != 0 {
        state.pole_3 += (state.pole_2 - state.pole_3) * alpha / Q15;
        state.pole_4 += (state.pole_3 - state.pole_4) * alpha / Q15;
    } else {
        state.pole_3 = state.pole_2;
        state.pole_4 = state.pole_2;
    }
    const STATE_LIMIT: i64 = i16::MAX as i64 * 8;
    state.pole_1 = state.pole_1.clamp(-STATE_LIMIT, STATE_LIMIT);
    state.pole_2 = state.pole_2.clamp(-STATE_LIMIT, STATE_LIMIT);
    state.pole_3 = state.pole_3.clamp(-STATE_LIMIT, STATE_LIMIT);
    state.pole_4 = state.pole_4.clamp(-STATE_LIMIT, STATE_LIMIT);
    if filter_type == 0 {
        state.pole_2
    } else {
        state.pole_4
    }
}

fn soft_limit_i16(sample: i64) -> i16 {
    const KNEE: i64 = 26_000;
    let magnitude = sample.abs();
    let limited = if magnitude <= KNEE {
        magnitude
    } else {
        let over = magnitude - KNEE;
        KNEE + over * 6_000 / (6_000 + over)
    };
    if sample < 0 {
        -limited.min(32_767) as i16
    } else {
        limited.min(32_767) as i16
    }
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
    fn header_v2(frames: u32) -> NativeBlockHeaderV2 {
        NativeBlockHeaderV2 {
            version: BLOCK_VERSION_V2,
            command_size: COMMAND_SIZE_V2,
            ..header(frames)
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
        assert_eq!(core::mem::size_of::<NativeRenderCommandV2>(), 104);
        assert_eq!(core::mem::size_of::<NativeRenderCommandV3>(), 112);
    }
    #[test]
    fn oscillator_is_stereo_and_audible() {
        let pcm = Engine::new().render_v1(&header(480), &[osc(480)]).unwrap();
        assert_eq!(pcm.len(), 960);
        assert!(pcm.iter().any(|v| *v != 0));
    }
    #[test]
    fn invalid_span_is_rejected() {
        let mut c = osc(480);
        c.end_frame = 481;
        assert_eq!(validate_v1(&header(480), &[c]), Err(Error::Invalid));
    }

    #[test]
    fn v2_release_tail_is_rendered_after_gate() {
        let command = NativeRenderCommandV2 {
            base: NativeRenderCommandV1 {
                end_frame: 480,
                duration_frames: 240,
                ..osc(480)
            },
            attack_frames: 0,
            decay_frames: 0,
            sustain_q15: 32_767,
            release_frames: 240,
            ..Default::default()
        };
        let pcm = Engine::new()
            .render_v2(&header_v2(480), &[command])
            .unwrap();
        assert!(pcm[240 * 2..].iter().any(|sample| *sample != 0));
        assert!(pcm[478 * 2].unsigned_abs() < pcm[300 * 2].unsigned_abs().max(2_000));
    }

    #[test]
    fn resonance_field_changes_the_lowpass_response() {
        let template = VoiceFilterState {
            revision: 1,
            voice_id: 1,
            source_id: 1,
            kind: KIND_OSCILLATOR,
            waveform: 2,
            pole_1: 0,
            pole_2: 0,
            pole_3: 0,
            pole_4: 0,
            last_end_frame: 0,
            seen_block: 0,
        };
        let mut dry = template;
        let mut resonant = template;
        let mut dry_out = 0;
        let mut resonant_out = 0;
        for index in 0..128 {
            let input = if index < 8 { 20_000 } else { 0 };
            dry_out = lowpass(&mut dry, input, 1_200, 0, 0);
            resonant_out = lowpass(&mut resonant, input, 1_200, 8 * 256, 0);
        }
        assert_ne!(dry_out, resonant_out);
        assert_ne!(resonant.pole_2, 0);
    }

    #[test]
    fn zero_cutoff_bypasses_instead_of_muting() {
        let mut state = VoiceFilterState {
            revision: 1,
            voice_id: 1,
            source_id: 1,
            kind: KIND_OSCILLATOR,
            waveform: 2,
            pole_1: 1,
            pole_2: 2,
            pole_3: 3,
            pole_4: 4,
            last_end_frame: 0,
            seen_block: 0,
        };
        assert_eq!(lowpass(&mut state, 12_345, 0, 0, 0), 12_345);
        assert_eq!((state.pole_1, state.pole_2, state.pole_3, state.pole_4), (0, 0, 0, 0));
    }

    #[test]
    fn v3_filter_types_have_distinct_responses() {
        let mut command = NativeRenderCommandV3 {
            base: NativeRenderCommandV2 {
                base: NativeRenderCommandV1 {
                    end_frame: 480,
                    duration_frames: 480,
                    waveform: 2,
                    lpf_hz: 1_200,
                    lpq_q8: 5 * 256,
                    ..osc(480)
                },
                sustain_q15: 32_767,
                ..Default::default()
            },
            ..Default::default()
        };
        let header = NativeBlockHeaderV2 {
            version: BLOCK_VERSION_V3,
            command_size: COMMAND_SIZE_V3,
            ..header(480)
        };
        let twelve = Engine::new().render_v3(&header, &[command]).unwrap();
        command.filter_type = 2;
        let twenty_four = Engine::new().render_v3(&header, &[command]).unwrap();
        command.filter_type = 1;
        let ladder = Engine::new().render_v3(&header, &[command]).unwrap();
        assert_ne!(twelve, twenty_four);
        assert_ne!(ladder, twenty_four);
    }

    #[test]
    fn room_has_an_early_diffuse_tail_distinct_from_delay() {
        let mut room = NativeRenderCommandV2 {
            base: NativeRenderCommandV1 {
                end_frame: 4_096,
                duration_frames: 256,
                room_q15: 24_000,
                ..osc(4_096)
            },
            sustain_q15: 32_767,
            ..Default::default()
        };
        let room_pcm = Engine::new().render_v2(&header_v2(4_096), &[room]).unwrap();
        room.base.room_q15 = 0;
        room.base.delay_q15 = 24_000;
        let delay_pcm = Engine::new().render_v2(&header_v2(4_096), &[room]).unwrap();
        assert!(room_pcm[1_400 * 2..].iter().any(|sample| *sample != 0));
        assert!(delay_pcm[1_400 * 2..].iter().all(|sample| *sample == 0));
    }
}
