use crate::vcabi;
use core::mem::{align_of, size_of};

pub const DEFAULT_RATE_HZ: u32 = 48_000;
pub const DEFAULT_CHANNELS: u32 = 2;

pub const ERR_IO: i32 = -5;
pub const ERR_BAD_HANDLE: i32 = -9;
pub const ERR_BUSY: i32 = -16;
pub const ERR_FAULT: i32 = -14;
pub const ERR_INVALID: i32 = -22;
pub const ERR_NO_DEVICE: i32 = -19;
const INVALID_CURSOR: u64 = u64::MAX;

/// Versioned read-only host HDA playback endpoint snapshot.
///
/// `ready == 0` is a successful unavailable result. The active stream stays
/// fixed; advertised masks describe the selected DAC rather than requesting a
/// format switch.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AudioEndpointCapabilitiesV1 {
    pub version: u16,
    pub size: u16,
    pub sample_rate_hz: u32,
    pub dma_buffer_frames: u32,
    pub queued_pcm_lane_frames: u32,
    pub selected_dac_pcm_rates: u32,
    pub selected_dac_stream_formats: u32,
    pub controller_output_streams: u8,
    pub active_channels: u8,
    pub active_sample_bits: u8,
    pub active_frame_bytes: u8,
    pub ready: u8,
    pub controller_addr64: u8,
    pub output_path_count: u8,
    pub selected_output_path_index: u8,
    pub reserved1: u64,
    pub reserved2: u64,
}

impl AudioEndpointCapabilitiesV1 {
    pub const VERSION: u16 = 1;
    pub const SIZE: u16 = core::mem::size_of::<Self>() as u16;

    pub const fn unavailable() -> Self {
        Self {
            version: Self::VERSION,
            size: Self::SIZE,
            sample_rate_hz: 0,
            dma_buffer_frames: 0,
            queued_pcm_lane_frames: 0,
            selected_dac_pcm_rates: 0,
            selected_dac_stream_formats: 0,
            controller_output_streams: 0,
            active_channels: 0,
            active_sample_bits: 0,
            active_frame_bytes: 0,
            ready: 0,
            controller_addr64: 0,
            output_path_count: 0,
            selected_output_path_index: 0,
            reserved1: 0,
            reserved2: 0,
        }
    }

    pub fn validate(&self) -> Result<(), AudioEndpointCapabilitiesValidationError> {
        if self.version != Self::VERSION || self.size != Self::SIZE {
            return Err(AudioEndpointCapabilitiesValidationError::BadVersionOrSize);
        }
        if self.reserved1 != 0 || self.reserved2 != 0 {
            return Err(AudioEndpointCapabilitiesValidationError::ReservedNonZero);
        }
        if self.ready > 1 {
            return Err(AudioEndpointCapabilitiesValidationError::BadReady);
        }
        if self.ready == 0 && *self != Self::unavailable() {
            return Err(AudioEndpointCapabilitiesValidationError::UnavailableNotZeroed);
        }
        Ok(())
    }
}

impl Default for AudioEndpointCapabilitiesV1 {
    fn default() -> Self {
        Self::unavailable()
    }
}

const _: [(); 48] = [(); core::mem::size_of::<AudioEndpointCapabilitiesV1>()];
const _: [(); 8] = [(); core::mem::align_of::<AudioEndpointCapabilitiesV1>()];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AudioEndpointCapabilitiesValidationError {
    BadVersionOrSize,
    ReservedNonZero,
    BadReady,
    UnavailableNotZeroed,
}

pub const NATIVE_AUDIO_MAGIC_V1: u32 = 0x314E_5254;
pub const NATIVE_AUDIO_VERSION_V1: u16 = 1;
pub const NATIVE_COMMAND_SIZE_V1: u16 = 80;
/// Additive command envelope extension. V1 payloads remain accepted unchanged.
pub const NATIVE_AUDIO_VERSION_V2: u16 = 2;
pub const NATIVE_COMMAND_SIZE_V2: u16 = 104;
pub const NATIVE_AUDIO_VERSION_V3: u16 = 3;
pub const NATIVE_COMMAND_SIZE_V3: u16 = 112;

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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

impl NativeBlockHeaderV1 {
    pub const fn new(block_frames: u32, absolute_frame: u64, revision: u64) -> Self {
        Self {
            magic: NATIVE_AUDIO_MAGIC_V1,
            version: NATIVE_AUDIO_VERSION_V1,
            command_size: NATIVE_COMMAND_SIZE_V1,
            block_frames,
            sample_rate_hz: DEFAULT_RATE_HZ,
            absolute_frame,
            revision,
            flags: 0,
            reserved: 0,
        }
    }

    pub fn validate(&self) -> Result<(), NativeValidationError> {
        if self.magic != NATIVE_AUDIO_MAGIC_V1
            || self.version != NATIVE_AUDIO_VERSION_V1
            || self.command_size != NATIVE_COMMAND_SIZE_V1
        {
            return Err(NativeValidationError::BadHeader);
        }
        if self.block_frames == 0 || self.sample_rate_hz == 0 || self.reserved != 0 {
            return Err(NativeValidationError::BadHeader);
        }
        Ok(())
    }
}

/// V2 keeps the fixed 40-byte block header layout. The distinct type prevents
/// accidentally submitting 104-byte commands through the V1 entry point.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NativeBlockHeaderV2 {
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

impl NativeBlockHeaderV2 {
    pub const fn new(block_frames: u32, absolute_frame: u64, revision: u64) -> Self {
        Self {
            magic: NATIVE_AUDIO_MAGIC_V1,
            version: NATIVE_AUDIO_VERSION_V2,
            command_size: NATIVE_COMMAND_SIZE_V2,
            block_frames,
            sample_rate_hz: DEFAULT_RATE_HZ,
            absolute_frame,
            revision,
            flags: 0,
            reserved: 0,
        }
    }

    pub fn validate(&self) -> Result<(), NativeValidationError> {
        if self.magic != NATIVE_AUDIO_MAGIC_V1
            || self.version != NATIVE_AUDIO_VERSION_V2
            || self.command_size != NATIVE_COMMAND_SIZE_V2
            || self.block_frames == 0
            || self.sample_rate_hz == 0
            || self.flags != 0
            || self.reserved != 0
        {
            return Err(NativeValidationError::BadHeader);
        }
        Ok(())
    }
}

/// V3 retains the frozen 40-byte block-header layout.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NativeBlockHeaderV3 {
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
impl NativeBlockHeaderV3 {
    pub const fn new(block_frames: u32, absolute_frame: u64, revision: u64) -> Self {
        Self {
            magic: NATIVE_AUDIO_MAGIC_V1,
            version: NATIVE_AUDIO_VERSION_V3,
            command_size: NATIVE_COMMAND_SIZE_V3,
            block_frames,
            sample_rate_hz: DEFAULT_RATE_HZ,
            absolute_frame,
            revision,
            flags: 0,
            reserved: 0,
        }
    }
    pub fn validate(&self) -> Result<(), NativeValidationError> {
        if self.magic != NATIVE_AUDIO_MAGIC_V1
            || self.version != NATIVE_AUDIO_VERSION_V3
            || self.command_size != NATIVE_COMMAND_SIZE_V3
            || self.block_frames == 0
            || self.sample_rate_hz == 0
            || self.flags != 0
            || self.reserved != 0
        {
            return Err(NativeValidationError::BadHeader);
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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

/// V2 command: the first 80 bytes are byte-for-byte V1, followed by integer
/// frame counts and Q15 envelope values. No floating point crosses the ABI.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NativeRenderCommandV2 {
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
    pub attack_frames: u32,
    pub decay_frames: u32,
    pub release_frames: u32,
    pub filter_attack_frames: u32,
    pub filter_decay_frames: u32,
    pub sustain_q15: u16,
    /// Signed octave depth in Q8, matching Strudel's `lpenv` units.
    pub filter_env_octaves_q8: i16,
}

impl NativeRenderCommandV2 {
    pub const KIND_OSCILLATOR: u16 = NativeRenderCommandV1::KIND_OSCILLATOR;
    pub const KIND_SAMPLE: u16 = NativeRenderCommandV1::KIND_SAMPLE;

    pub fn validate(&self, block_frames: u32) -> Result<(), NativeValidationError> {
        if self.start_frame >= self.end_frame || self.end_frame > block_frames {
            return Err(NativeValidationError::BadSpan);
        }
        // Unlike V1, V2/V3 have an explicit release stage. Commands may
        // resume in a later block after their gate has ended, until the
        // release tail itself is exhausted.
        let envelope_frames = self
            .duration_frames
            .saturating_add(self.release_frames);
        if self.duration_frames == 0 || self.age_frames >= envelope_frames {
            return Err(NativeValidationError::BadDuration);
        }
        if self.kind != Self::KIND_OSCILLATOR && self.kind != Self::KIND_SAMPLE {
            return Err(NativeValidationError::BadKind);
        }
        if self.reserved0 != 0 || self.reserved1 != 0 || self.reserved2 != 0 {
            return Err(NativeValidationError::ReservedNonZero);
        }
        if self.sustain_q15 > 32_767 || !(-2048..=2048).contains(&self.filter_env_octaves_q8) {
            return Err(NativeValidationError::InvalidEnvelope);
        }
        Ok(())
    }
}

/// V3 preserves the complete 104-byte V2 prefix and adds filter selection.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NativeRenderCommandV3 {
    pub base: NativeRenderCommandV2,
    /// 0 = 12db, 1 = ladder, 2 = 24db.
    pub filter_type: u8,
    pub reserved3: [u8; 3],
    pub reserved4: u32,
}
impl NativeRenderCommandV3 {
    pub const FILTER_12DB: u8 = 0;
    pub const FILTER_LADDER: u8 = 1;
    pub const FILTER_24DB: u8 = 2;
    pub const KIND_OSCILLATOR: u16 = NativeRenderCommandV2::KIND_OSCILLATOR;
    pub const KIND_SAMPLE: u16 = NativeRenderCommandV2::KIND_SAMPLE;
    pub fn validate(&self, block_frames: u32) -> Result<(), NativeValidationError> {
        self.base.validate(block_frames)?;
        if self.filter_type > Self::FILTER_24DB || self.reserved3 != [0; 3] || self.reserved4 != 0 {
            return Err(NativeValidationError::ReservedNonZero);
        }
        Ok(())
    }
}

impl NativeRenderCommandV1 {
    pub const KIND_OSCILLATOR: u16 = 1;
    pub const KIND_SAMPLE: u16 = 2;

    pub fn validate(&self, block_frames: u32) -> Result<(), NativeValidationError> {
        if self.start_frame >= self.end_frame || self.end_frame > block_frames {
            return Err(NativeValidationError::BadSpan);
        }
        if self.duration_frames == 0 || self.age_frames >= self.duration_frames {
            return Err(NativeValidationError::BadDuration);
        }
        if self.kind != Self::KIND_OSCILLATOR && self.kind != Self::KIND_SAMPLE {
            return Err(NativeValidationError::BadKind);
        }
        if self.reserved0 != 0 || self.reserved1 != 0 || self.reserved2 != 0 {
            return Err(NativeValidationError::ReservedNonZero);
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NativeValidationError {
    BadHeader,
    BadSpan,
    BadDuration,
    BadKind,
    ReservedNonZero,
    InvalidSample,
    InvalidEnvelope,
}

const _: [(); 40] = [(); size_of::<NativeBlockHeaderV1>()];
const _: [(); 8] = [(); align_of::<NativeBlockHeaderV1>()];
const _: [(); 80] = [(); size_of::<NativeRenderCommandV1>()];
const _: [(); 8] = [(); align_of::<NativeRenderCommandV1>()];
const _: [(); 40] = [(); size_of::<NativeBlockHeaderV2>()];
const _: [(); 8] = [(); align_of::<NativeBlockHeaderV2>()];
const _: [(); 104] = [(); size_of::<NativeRenderCommandV2>()];
const _: [(); 8] = [(); align_of::<NativeRenderCommandV2>()];
const _: [(); 40] = [(); size_of::<NativeBlockHeaderV3>()];
const _: [(); 8] = [(); align_of::<NativeBlockHeaderV3>()];
const _: [(); 112] = [(); size_of::<NativeRenderCommandV3>()];
const _: [(); 8] = [(); align_of::<NativeRenderCommandV3>()];

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Format {
    S16LE = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum State {
    Closed,
    Prepared,
    Running,
    Disconnected,
    Unknown(i32),
}

impl State {
    fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Closed,
            1 => Self::Prepared,
            2 => Self::Running,
            3 => Self::Disconnected,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PlaybackParams {
    pub format: Format,
    pub channels: u32,
    pub rate_hz: u32,
}

impl PlaybackParams {
    pub const fn s16le_stereo_48k() -> Self {
        Self {
            format: Format::S16LE,
            channels: DEFAULT_CHANNELS,
            rate_hz: DEFAULT_RATE_HZ,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Stream {
    handle: u32,
}

impl Stream {
    pub fn open_playback(params: PlaybackParams) -> Result<Self, i32> {
        let mut handle = 0u32;
        let rc = unsafe {
            vcabi::trueos_cabi_audio_open_playback(
                params.format as u32,
                params.channels,
                params.rate_hz,
                &mut handle,
            )
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok(Self { handle })
    }

    pub const fn handle(self) -> u32 {
        self.handle
    }

    pub fn start(self) -> Result<(), i32> {
        rc_unit(unsafe { vcabi::trueos_cabi_audio_start(self.handle) })
    }

    pub fn write_interleaved_i16(self, samples: &[i16]) -> Result<usize, i32> {
        let frames = unsafe {
            vcabi::trueos_cabi_audio_write_i16_interleaved(
                self.handle,
                samples.as_ptr(),
                samples.len(),
            )
        };
        frames_result(frames)
    }

    pub fn queued_frames(self) -> Result<usize, i32> {
        frames_result(unsafe { vcabi::trueos_cabi_audio_queued_frames(self.handle) })
    }

    pub fn buffer_frames(self) -> Result<usize, i32> {
        frames_result(unsafe { vcabi::trueos_cabi_audio_buffer_frames(self.handle) })
    }

    pub fn drain(self, timeout_ms: u64) -> Result<(), i32> {
        rc_unit(unsafe { vcabi::trueos_cabi_audio_drain(self.handle, timeout_ms) })
    }

    pub fn pause(self) -> Result<(), i32> {
        self.set_paused(true)
    }

    pub fn resume(self) -> Result<(), i32> {
        self.set_paused(false)
    }

    pub fn set_paused(self, paused: bool) -> Result<(), i32> {
        rc_unit(unsafe { vcabi::trueos_cabi_audio_set_paused(self.handle, u32::from(paused)) })
    }

    pub fn paused(self) -> Result<bool, i32> {
        bool_result(unsafe { vcabi::trueos_cabi_audio_paused(self.handle) })
    }

    pub fn set_volume_percent(self, percent: u32) -> Result<u32, i32> {
        u32_result(unsafe { vcabi::trueos_cabi_audio_set_volume_percent(self.handle, percent) })
    }

    pub fn volume_percent(self) -> Result<u32, i32> {
        u32_result(unsafe { vcabi::trueos_cabi_audio_volume_percent(self.handle) })
    }

    pub fn drop_stream(self) -> Result<(), i32> {
        rc_unit(unsafe { vcabi::trueos_cabi_audio_drop(self.handle) })
    }

    pub fn close(self) -> Result<(), i32> {
        rc_unit(unsafe { vcabi::trueos_cabi_audio_close(self.handle) })
    }

    pub fn state(self) -> State {
        State::from_raw(unsafe { vcabi::trueos_cabi_audio_state(self.handle) })
    }
}

/// Native scheduled-command audio engine backed by a regular playback stream.
/// The PCM methods on `Stream` remain available as a compatibility fallback.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NativeEngine {
    stream: Stream,
}

impl NativeEngine {
    pub fn open_playback(params: PlaybackParams) -> Result<Self, i32> {
        let stream = Stream::open_playback(params)?;
        stream.start()?;
        Ok(Self { stream })
    }

    pub const fn from_stream(stream: Stream) -> Self {
        Self { stream }
    }

    pub const fn stream(self) -> Stream {
        self.stream
    }

    pub fn render(
        self,
        header: &NativeBlockHeaderV1,
        commands: &[NativeRenderCommandV1],
    ) -> Result<usize, NativeValidationErrorOrCode> {
        header
            .validate()
            .map_err(NativeValidationErrorOrCode::Validation)?;
        for command in commands {
            command
                .validate(header.block_frames)
                .map_err(NativeValidationErrorOrCode::Validation)?;
        }
        let result = unsafe {
            vcabi::trueos_cabi_audio_native_render_v1(
                self.stream.handle,
                header,
                commands.as_ptr(),
                commands.len(),
            )
        };
        if result < 0 {
            Err(NativeValidationErrorOrCode::Code(result as i32))
        } else {
            Ok(result as usize)
        }
    }

    pub fn render_v2(
        self,
        header: &NativeBlockHeaderV2,
        commands: &[NativeRenderCommandV2],
    ) -> Result<usize, NativeValidationErrorOrCode> {
        header
            .validate()
            .map_err(NativeValidationErrorOrCode::Validation)?;
        for command in commands {
            command
                .validate(header.block_frames)
                .map_err(NativeValidationErrorOrCode::Validation)?;
        }
        let result = unsafe {
            vcabi::trueos_cabi_audio_native_render_v2(
                self.stream.handle,
                header,
                commands.as_ptr(),
                commands.len(),
            )
        };
        if result < 0 {
            Err(NativeValidationErrorOrCode::Code(result as i32))
        } else {
            Ok(result as usize)
        }
    }

    pub fn render_v3(
        self,
        header: &NativeBlockHeaderV3,
        commands: &[NativeRenderCommandV3],
    ) -> Result<usize, NativeValidationErrorOrCode> {
        header
            .validate()
            .map_err(NativeValidationErrorOrCode::Validation)?;
        for command in commands {
            command
                .validate(header.block_frames)
                .map_err(NativeValidationErrorOrCode::Validation)?;
        }
        let result = unsafe {
            vcabi::trueos_cabi_audio_native_render_v3(
                self.stream.handle,
                header,
                commands.as_ptr(),
                commands.len(),
            )
        };
        if result < 0 {
            Err(NativeValidationErrorOrCode::Code(result as i32))
        } else {
            Ok(result as usize)
        }
    }

    pub fn register_sample(
        self,
        sample_id: u64,
        channels: u32,
        rate_hz: u32,
        samples: &[i16],
    ) -> Result<(), NativeValidationErrorOrCode> {
        if sample_id == 0
            || channels == 0
            || rate_hz == 0
            || samples.is_empty()
            || samples.len() % channels as usize != 0
        {
            return Err(NativeValidationErrorOrCode::Validation(
                NativeValidationError::InvalidSample,
            ));
        }
        rc_unit(unsafe {
            vcabi::trueos_cabi_audio_native_sample_register_v1(
                self.stream.handle,
                sample_id,
                channels,
                rate_hz,
                samples.as_ptr(),
                samples.len(),
            )
        })
        .map_err(NativeValidationErrorOrCode::Code)
    }

    pub fn remove_sample(self, sample_id: u64) -> Result<(), i32> {
        if sample_id == 0 {
            return Err(ERR_INVALID);
        }
        rc_unit(unsafe {
            vcabi::trueos_cabi_audio_native_sample_remove_v1(self.stream.handle, sample_id)
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NativeValidationErrorOrCode {
    Validation(NativeValidationError),
    Code(i32),
}

pub fn play_i16_stereo_48k(samples: &[i16]) -> Result<usize, i32> {
    let frames =
        unsafe { vcabi::trueos_cabi_audio_write_i16_stereo_48k(samples.as_ptr(), samples.len()) };
    frames_result(frames)
}

/// Read the host-owned HDA endpoint snapshot. This does not open, start, or
/// reconfigure playback; `Ok(caps)` with `caps.ready == 0` is the truthful
/// no-hardware/not-ready result.
pub fn endpoint_capabilities_v1() -> Result<AudioEndpointCapabilitiesV1, i32> {
    let mut caps = AudioEndpointCapabilitiesV1::unavailable();
    let rc = unsafe {
        vcabi::trueos_cabi_audio_endpoint_caps_v1(
            &mut caps,
            core::mem::size_of::<AudioEndpointCapabilitiesV1>(),
        )
    };
    if rc != 0 {
        return Err(rc);
    }
    caps.validate().map_err(|_| ERR_INVALID)?;
    Ok(caps)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Monitor {
    cursor: u64,
}

impl Monitor {
    pub fn open(preroll_samples: usize) -> Result<Self, i32> {
        let cursor = unsafe { vcabi::trueos_cabi_audio_monitor_start_cursor(preroll_samples) };
        if cursor == INVALID_CURSOR {
            Err(ERR_NO_DEVICE)
        } else {
            Ok(Self { cursor })
        }
    }

    pub const fn cursor(self) -> u64 {
        self.cursor
    }

    pub fn read_i16(&mut self, out: &mut [i16]) -> Result<usize, i32> {
        let mut next = self.cursor;
        let samples = unsafe {
            vcabi::trueos_cabi_audio_monitor_read_i16_since(
                self.cursor,
                out.as_mut_ptr(),
                out.len(),
                &mut next,
            )
        };
        let count = count_result(samples)?;
        self.cursor = next;
        Ok(count)
    }
}

fn rc_unit(rc: i32) -> Result<(), i32> {
    if rc == 0 { Ok(()) } else { Err(rc) }
}

fn frames_result(frames: isize) -> Result<usize, i32> {
    count_result(frames)
}

fn count_result(frames: isize) -> Result<usize, i32> {
    if frames < 0 {
        Err(frames as i32)
    } else {
        Ok(frames as usize)
    }
}

fn u32_result(value: i32) -> Result<u32, i32> {
    if value < 0 {
        Err(value)
    } else {
        Ok(value as u32)
    }
}

fn bool_result(value: i32) -> Result<bool, i32> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        err if err < 0 => Err(err),
        other => Err(other),
    }
}

#[cfg(test)]
mod endpoint_capability_tests {
    use super::*;

    #[test]
    fn endpoint_capability_layout_is_stable() {
        assert_eq!(size_of::<AudioEndpointCapabilitiesV1>(), 48);
        assert_eq!(align_of::<AudioEndpointCapabilitiesV1>(), 8);
    }

    #[test]
    fn unavailable_endpoint_is_a_valid_successful_snapshot() {
        let caps = AudioEndpointCapabilitiesV1::unavailable();
        assert_eq!(caps.ready, 0);
        assert_eq!(caps.validate(), Ok(()));
    }

    #[test]
    fn nonzero_reserved_endpoint_fields_are_rejected() {
        let mut caps = AudioEndpointCapabilitiesV1::unavailable();
        caps.reserved1 = 1;
        assert_eq!(caps.validate(), Err(AudioEndpointCapabilitiesValidationError::ReservedNonZero));
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;

    #[test]
    fn v1_abi_layout_and_defaults_are_stable() {
        assert_eq!(size_of::<NativeBlockHeaderV1>(), 40);
        assert_eq!(align_of::<NativeBlockHeaderV1>(), 8);
        assert_eq!(size_of::<NativeRenderCommandV1>(), 80);
        assert_eq!(align_of::<NativeRenderCommandV1>(), 8);
        assert_eq!(NativeBlockHeaderV1::new(64, 123, 4).sample_rate_hz, 48_000);
        assert_eq!(size_of::<NativeBlockHeaderV2>(), 40);
        assert_eq!(align_of::<NativeBlockHeaderV2>(), 8);
        assert_eq!(size_of::<NativeRenderCommandV2>(), 104);
        assert_eq!(align_of::<NativeRenderCommandV2>(), 8);
        let v2 = NativeBlockHeaderV2::new(64, 123, 4);
        assert!(v2.validate().is_ok());
        assert_eq!(v2.version, NATIVE_AUDIO_VERSION_V2);
        assert_eq!(v2.command_size, NATIVE_COMMAND_SIZE_V2);
    }

    #[test]
    fn native_command_validation_rejects_out_of_block_spans() {
        let command = NativeRenderCommandV1 {
            start_frame: 4,
            end_frame: 65,
            age_frames: 0,
            duration_frames: 10,
            source_id: 1,
            voice_id: 1,
            kind: NativeRenderCommandV1::KIND_OSCILLATOR,
            waveform: 0,
            midi_note: 60,
            gain_q15: 1,
            pan_q15: 0,
            playback_rate_q16: 65_536,
            sample_begin_q16: 0,
            sample_end_q16: 0,
            lpf_hz: 0,
            lpq_q8: 0,
            room_q15: 0,
            delay_q15: 0,
            phaser_q15: 0,
            shape_q15: 0,
            fm_depth_q8: 0,
            fm_rate_q8: 0,
            flags: 0,
            reserved0: 0,
            reserved1: 0,
            reserved2: 0,
        };
        assert_eq!(command.validate(64), Err(NativeValidationError::BadSpan));
    }
}
