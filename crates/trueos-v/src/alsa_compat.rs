use crate::vaudio;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Direction {
    Playback,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Format {
    S16LE,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Access {
    RWInterleaved,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum State {
    Closed,
    Prepared,
    Running,
    Disconnected,
    Unknown(i32),
}

impl From<vaudio::State> for State {
    fn from(state: vaudio::State) -> Self {
        match state {
            vaudio::State::Closed => Self::Closed,
            vaudio::State::Prepared => Self::Prepared,
            vaudio::State::Running => Self::Running,
            vaudio::State::Disconnected => Self::Disconnected,
            vaudio::State::Unknown(raw) => Self::Unknown(raw),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HwParams {
    format: Format,
    access: Access,
    channels: u32,
    rate_hz: u32,
}

impl HwParams {
    pub const fn any(_pcm: &Pcm) -> Self {
        Self {
            format: Format::S16LE,
            access: Access::RWInterleaved,
            channels: vaudio::DEFAULT_CHANNELS,
            rate_hz: vaudio::DEFAULT_RATE_HZ,
        }
    }

    pub fn set_format(&mut self, format: Format) -> Result<(), i32> {
        self.format = format;
        Ok(())
    }

    pub fn set_access(&mut self, access: Access) -> Result<(), i32> {
        self.access = access;
        Ok(())
    }

    pub fn set_channels(&mut self, channels: u32) -> Result<(), i32> {
        if channels != vaudio::DEFAULT_CHANNELS {
            return Err(vaudio::ERR_INVALID);
        }
        self.channels = channels;
        Ok(())
    }

    pub fn set_rate(&mut self, rate_hz: u32) -> Result<(), i32> {
        if rate_hz != vaudio::DEFAULT_RATE_HZ {
            return Err(vaudio::ERR_INVALID);
        }
        self.rate_hz = rate_hz;
        Ok(())
    }

    pub const fn channels(&self) -> u32 {
        self.channels
    }

    pub const fn rate(&self) -> u32 {
        self.rate_hz
    }
}

#[derive(Debug)]
pub struct Pcm {
    stream: vaudio::Stream,
}

impl Pcm {
    pub fn open_playback(name: &str) -> Result<Self, i32> {
        if !matches!(name, "default" | "hw:0,0" | "plughw:0,0") {
            return Err(vaudio::ERR_INVALID);
        }
        let stream = vaudio::Stream::open_playback(vaudio::PlaybackParams::s16le_stereo_48k())?;
        Ok(Self { stream })
    }

    pub fn open(name: &str, direction: Direction) -> Result<Self, i32> {
        match direction {
            Direction::Playback => Self::open_playback(name),
        }
    }

    pub const fn handle(&self) -> u32 {
        self.stream.handle()
    }

    pub fn hw_params(&self, params: &HwParams) -> Result<(), i32> {
        match (params.format, params.access, params.channels, params.rate_hz) {
            (
                Format::S16LE,
                Access::RWInterleaved,
                vaudio::DEFAULT_CHANNELS,
                vaudio::DEFAULT_RATE_HZ,
            ) => Ok(()),
            _ => Err(vaudio::ERR_INVALID),
        }
    }

    pub fn prepare(&self) -> Result<(), i32> {
        Ok(())
    }

    pub fn start(&self) -> Result<(), i32> {
        self.stream.start()
    }

    pub fn writei(&self, samples: &[i16]) -> Result<usize, i32> {
        self.stream.write_interleaved_i16(samples)
    }

    pub fn drain(&self) -> Result<(), i32> {
        self.stream.drain(0)
    }

    pub fn drop_stream(&self) -> Result<(), i32> {
        self.stream.drop_stream()
    }

    pub fn close(self) -> Result<(), i32> {
        self.stream.close()
    }

    pub fn state(&self) -> State {
        self.stream.state().into()
    }

    pub fn avail_update(&self) -> Result<usize, i32> {
        self.stream.buffer_frames()
    }

    pub fn delay(&self) -> Result<usize, i32> {
        self.stream.queued_frames()
    }
}
