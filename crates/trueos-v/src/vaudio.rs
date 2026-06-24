use crate::vcabi;

pub const DEFAULT_RATE_HZ: u32 = 48_000;
pub const DEFAULT_CHANNELS: u32 = 2;

pub const ERR_IO: i32 = -5;
pub const ERR_BAD_HANDLE: i32 = -9;
pub const ERR_BUSY: i32 = -16;
pub const ERR_FAULT: i32 = -14;
pub const ERR_INVALID: i32 = -22;
pub const ERR_NO_DEVICE: i32 = -19;

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
    let frames = unsafe { vcabi::trueos_cabi_audio_write_i16_stereo_48k(samples.as_ptr(), samples.len()) };
    frames_result(frames)
}

fn rc_unit(rc: i32) -> Result<(), i32> {
    if rc == 0 { Ok(()) } else { Err(rc) }
}

fn frames_result(frames: isize) -> Result<usize, i32> {
    if frames < 0 {
        Err(frames as i32)
    } else {
        Ok(frames as usize)
    }
}
