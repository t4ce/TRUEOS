use crate::vcabi;
#[cfg(test)]
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
