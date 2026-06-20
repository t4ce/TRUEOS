//! TRUEOS-native PCM facade.

use core::cell;
use core::ffi::CStr;
use core::fmt;
use core::marker::PhantomData;
use core::mem::size_of;

use crate::error::{Error, Result};
use crate::libc;
use crate::{Direction, ValueOr};

pub type Frames = isize;

const TRUEOS_RATE_HZ: u32 = 48_000;
const TRUEOS_CHANNELS: u32 = 2;
const TRUEOS_PERIOD_FRAMES: Frames = 1024;

fn backend() -> Result<&'static crate::trueos::PcmBackend> {
    crate::trueos::pcm_backend().ok_or_else(|| Error::new("trueos_pcm_backend", libc::ENODEV))
}

fn backend_err(func: &'static str, errno: i32) -> Error {
    Error::new(func, errno.unsigned_abs() as i32)
}

#[derive(Debug)]
pub struct PCM {
    handle: usize,
    has_io: cell::Cell<bool>,
    state: cell::Cell<State>,
    channels: cell::Cell<u32>,
    rate: cell::Cell<u32>,
    format: cell::Cell<Format>,
    access: cell::Cell<Access>,
    buffer_frames: cell::Cell<Frames>,
    period_frames: cell::Cell<Frames>,
}

unsafe impl Send for PCM {}

impl PCM {
    fn check_has_io(&self) {
        if self.has_io.get() {
            panic!("No hw_params call or additional IO objects allowed")
        }
    }

    pub fn new(_name: &str, dir: Direction, _nonblock: bool) -> Result<PCM> {
        if dir != Direction::Playback {
            return Err(Error::new("snd_pcm_open", libc::ENOTSUP));
        }

        let backend = backend()?;
        let handle = (backend.open_playback)().map_err(|err| backend_err("snd_pcm_open", err))?;
        let buffer_frames = (backend.buffer_frames)(handle)
            .map(|frames| frames.min(isize::MAX as usize) as Frames)
            .unwrap_or(TRUEOS_RATE_HZ as Frames);

        Ok(PCM {
            handle,
            has_io: cell::Cell::new(false),
            state: cell::Cell::new(State::Open),
            channels: cell::Cell::new(TRUEOS_CHANNELS),
            rate: cell::Cell::new(TRUEOS_RATE_HZ),
            format: cell::Cell::new(Format::s16()),
            access: cell::Cell::new(Access::RWInterleaved),
            buffer_frames: cell::Cell::new(buffer_frames),
            period_frames: cell::Cell::new(TRUEOS_PERIOD_FRAMES),
        })
    }

    pub fn open(name: &CStr, dir: Direction, nonblock: bool) -> Result<PCM> {
        let name = name.to_str().unwrap_or("default");
        Self::new(name, dir, nonblock)
    }

    pub fn start(&self) -> Result<()> {
        let backend = backend()?;
        (backend.start)(self.handle).map_err(|err| backend_err("snd_pcm_start", err))?;
        self.state.set(State::Running);
        Ok(())
    }

    pub fn drop(&self) -> Result<()> {
        let backend = backend()?;
        (backend.drop_stream)(self.handle).map_err(|err| backend_err("snd_pcm_drop", err))?;
        self.state.set(State::Setup);
        Ok(())
    }

    pub fn pause(&self, pause: bool) -> Result<()> {
        if pause {
            self.state.set(State::Paused);
        } else if self.state.get() == State::Paused {
            self.state.set((backend()?.state)(self.handle));
        }
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        self.state.set((backend()?.state)(self.handle));
        Ok(())
    }

    pub fn drain(&self) -> Result<()> {
        let backend = backend()?;
        (backend.drain)(self.handle).map_err(|err| backend_err("snd_pcm_drain", err))
    }

    pub fn prepare(&self) -> Result<()> {
        self.state.set(State::Prepared);
        Ok(())
    }

    pub fn reset(&self) -> Result<()> {
        self.drop()
    }

    pub fn recover(&self, _err: libc::c_int, _silent: bool) -> Result<()> {
        self.prepare()
    }

    pub fn try_recover(&self, err: Error, silent: bool) -> Result<()> {
        self.recover(err.errno() as libc::c_int, silent)
    }

    pub fn wait(&self, _timeout_ms: Option<u32>) -> Result<bool> {
        Ok(true)
    }

    pub fn state(&self) -> State {
        if matches!(self.state.get(), State::Paused | State::Open | State::Setup | State::Prepared) {
            self.state.get()
        } else {
            (backend().map(|b| (b.state)(self.handle))).unwrap_or(self.state.get())
        }
    }

    pub fn state_raw(&self) -> libc::c_int {
        self.state() as libc::c_int
    }

    pub fn bytes_to_frames(&self, bytes: isize) -> Frames {
        let frame_bytes = self.channels.get().max(1) as isize * size_of_format(self.format.get());
        bytes / frame_bytes.max(1)
    }

    pub fn frames_to_bytes(&self, frames: Frames) -> isize {
        let frame_bytes = self.channels.get().max(1) as isize * size_of_format(self.format.get());
        frames.saturating_mul(frame_bytes.max(1))
    }

    pub fn avail_update(&self) -> Result<Frames> {
        self.avail()
    }

    pub fn avail(&self) -> Result<Frames> {
        let frames = (backend()?.writable_frames)(self.handle)
            .map_err(|err| backend_err("snd_pcm_avail", err))?;
        Ok(frames.min(isize::MAX as usize) as Frames)
    }

    pub fn avail_delay(&self) -> Result<(Frames, Frames)> {
        Ok((self.avail()?, self.delay()?))
    }

    pub fn delay(&self) -> Result<Frames> {
        let frames = (backend()?.queued_frames)(self.handle)
            .map_err(|err| backend_err("snd_pcm_delay", err))?;
        Ok(frames.min(isize::MAX as usize) as Frames)
    }

    pub fn io_i16(&self) -> Result<IO<'_, i16>> {
        self.io_checked()
    }

    pub fn io_i8(&self) -> Result<IO<'_, i8>> {
        self.io_checked()
    }

    pub fn io_u8(&self) -> Result<IO<'_, u8>> {
        self.io_checked()
    }

    pub fn io_u16(&self) -> Result<IO<'_, u16>> {
        self.io_checked()
    }

    pub fn io_i32(&self) -> Result<IO<'_, i32>> {
        self.io_checked()
    }

    pub fn io_u32(&self) -> Result<IO<'_, u32>> {
        self.io_checked()
    }

    pub fn io_f32(&self) -> Result<IO<'_, f32>> {
        self.io_checked()
    }

    pub fn io_f64(&self) -> Result<IO<'_, f64>> {
        self.io_checked()
    }

    pub fn io_i32_s24(&self) -> Result<IO<'_, i32>> {
        self.verify_format(Format::s24()).map(|_| IO::new(self))
    }

    pub fn io_u32_u24(&self) -> Result<IO<'_, u32>> {
        self.verify_format(Format::u24()).map(|_| IO::new(self))
    }

    pub fn io_checked<S: IoFormat>(&self) -> Result<IO<'_, S>> {
        self.verify_format(S::FORMAT).map(|_| IO::new(self))
    }

    pub unsafe fn io_unchecked<S: IoFormat>(&self) -> IO<'_, S> {
        IO::new_unchecked(self)
    }

    pub fn io_bytes(&self) -> IO<'_, u8> {
        IO::new(self)
    }

    #[deprecated(note = "renamed to io_bytes")]
    pub fn io(&self) -> IO<'_, u8> {
        IO::new(self)
    }

    pub fn hw_params(&self, h: &HwParams<'_>) -> Result<()> {
        self.check_has_io();
        self.channels.set(h.channels.get());
        self.rate.set(h.rate.get());
        self.format.set(h.format.get());
        self.access.set(h.access.get());
        self.buffer_frames.set(h.buffer_frames.get());
        self.period_frames.set(h.period_frames.get());
        self.state.set(State::Prepared);
        Ok(())
    }

    pub fn hw_params_current(&self) -> Result<HwParams<'_>> {
        Ok(HwParams::from_pcm(self))
    }

    pub fn sw_params(&self, _h: &SwParams<'_>) -> Result<()> {
        Ok(())
    }

    pub fn sw_params_current(&self) -> Result<SwParams<'_>> {
        SwParams::new(self)
    }

    pub fn get_params(&self) -> Result<(u64, u64)> {
        Ok((
            self.buffer_frames.get().max(0) as u64,
            self.period_frames.get().max(0) as u64,
        ))
    }

    fn verify_format(&self, format: Format) -> Result<()> {
        if self.format.get() == format {
            Ok(())
        } else {
            Err(Error::unsupported("io_xx"))
        }
    }
}

impl Drop for PCM {
    fn drop(&mut self) {
        if let Some(backend) = crate::trueos::pcm_backend() {
            (backend.close)(self.handle);
        }
    }
}

#[derive(Debug)]
pub struct IO<'a, S: Copy>(&'a PCM, PhantomData<S>);

impl<'a, S: Copy> Drop for IO<'a, S> {
    fn drop(&mut self) {
        (self.0).has_io.set(false)
    }
}

impl<'a, S: Copy> IO<'a, S> {
    fn new(a: &'a PCM) -> IO<'a, S> {
        a.check_has_io();
        a.has_io.set(true);
        IO(a, PhantomData)
    }

    unsafe fn new_unchecked(a: &'a PCM) -> IO<'a, S> {
        a.has_io.set(true);
        IO(a, PhantomData)
    }

    fn to_frames(&self, items: usize) -> usize {
        let bytes = items.saturating_mul(size_of::<S>());
        self.0.bytes_to_frames(bytes.min(isize::MAX as usize) as isize).max(0) as usize
    }

    pub fn writei(&self, buf: &[S]) -> Result<usize> {
        if self.0.channels.get() != TRUEOS_CHANNELS
            || self.0.rate.get() != TRUEOS_RATE_HZ
            || self.0.format.get() != Format::s16()
            || self.0.access.get() != Access::RWInterleaved
            || size_of::<S>() != size_of::<i16>()
        {
            return Err(Error::new("snd_pcm_writei", libc::EINVAL));
        }

        let frames = self.to_frames(buf.len());
        let samples = buf.len();
        let written = (backend()?.write_i16_interleaved)(
            self.0.handle,
            buf.as_ptr() as *const i16,
            samples,
        )
        .map_err(|err| backend_err("snd_pcm_writei", err))?;
        self.0.state.set(State::Running);
        Ok(written.min(frames))
    }

    pub fn readi(&self, _buf: &mut [S]) -> Result<usize> {
        Err(Error::new("snd_pcm_readi", libc::ENOTSUP))
    }

    pub unsafe fn writen(&self, _bufs: &[*const S], _frames: usize) -> Result<usize> {
        Err(Error::new("snd_pcm_writen", libc::ENOTSUP))
    }

    pub unsafe fn readn(&self, _bufs: &mut [*mut S], _frames: usize) -> Result<usize> {
        Err(Error::new("snd_pcm_readn", libc::ENOTSUP))
    }
}

#[derive(Debug)]
pub struct HwParams<'a> {
    pcm: &'a PCM,
    channels: cell::Cell<u32>,
    rate: cell::Cell<u32>,
    format: cell::Cell<Format>,
    access: cell::Cell<Access>,
    period_frames: cell::Cell<Frames>,
    buffer_frames: cell::Cell<Frames>,
}

impl<'a> HwParams<'a> {
    fn from_pcm(pcm: &'a PCM) -> Self {
        Self {
            pcm,
            channels: cell::Cell::new(pcm.channels.get()),
            rate: cell::Cell::new(pcm.rate.get()),
            format: cell::Cell::new(pcm.format.get()),
            access: cell::Cell::new(pcm.access.get()),
            period_frames: cell::Cell::new(pcm.period_frames.get()),
            buffer_frames: cell::Cell::new(pcm.buffer_frames.get()),
        }
    }

    fn new(pcm: &'a PCM) -> Result<Self> {
        Ok(Self::from_pcm(pcm))
    }

    pub fn any(pcm: &'a PCM) -> Result<Self> {
        Self::new(pcm)
    }

    pub fn get_rate_resample(&self) -> Result<bool> {
        Ok(false)
    }

    pub fn set_rate_resample(&self, _resample: bool) -> Result<()> {
        Ok(())
    }

    pub fn set_channels_near(&self, v: u32) -> Result<u32> {
        let v = if v == TRUEOS_CHANNELS { v } else { TRUEOS_CHANNELS };
        self.channels.set(v);
        Ok(v)
    }

    pub fn set_channels(&self, v: u32) -> Result<()> {
        if v == TRUEOS_CHANNELS {
            self.channels.set(v);
            Ok(())
        } else {
            Err(Error::new("snd_pcm_hw_params_set_channels", libc::EINVAL))
        }
    }

    pub fn get_channels(&self) -> Result<u32> {
        Ok(self.channels.get())
    }

    pub fn get_channels_max(&self) -> Result<u32> {
        Ok(TRUEOS_CHANNELS)
    }

    pub fn get_channels_min(&self) -> Result<u32> {
        Ok(TRUEOS_CHANNELS)
    }

    pub fn test_channels(&self, v: u32) -> Result<()> {
        if v == TRUEOS_CHANNELS { Ok(()) } else { Err(Error::new("snd_pcm_hw_params_test_channels", libc::EINVAL)) }
    }

    pub fn set_rate_near(&self, _v: u32, _dir: ValueOr) -> Result<u32> {
        self.rate.set(TRUEOS_RATE_HZ);
        Ok(TRUEOS_RATE_HZ)
    }

    pub fn set_rate(&self, v: u32, _dir: ValueOr) -> Result<()> {
        if v == TRUEOS_RATE_HZ {
            self.rate.set(v);
            Ok(())
        } else {
            Err(Error::new("snd_pcm_hw_params_set_rate", libc::EINVAL))
        }
    }

    pub fn get_rate(&self) -> Result<u32> {
        Ok(self.rate.get())
    }

    pub fn get_rate_max(&self) -> Result<u32> {
        Ok(TRUEOS_RATE_HZ)
    }

    pub fn get_rate_min(&self) -> Result<u32> {
        Ok(TRUEOS_RATE_HZ)
    }

    pub fn test_rate(&self, rate: u32) -> Result<()> {
        if rate == TRUEOS_RATE_HZ { Ok(()) } else { Err(Error::new("snd_pcm_hw_params_test_rate", libc::EINVAL)) }
    }

    pub fn set_format(&self, v: Format) -> Result<()> {
        if v == Format::s16() {
            self.format.set(v);
            Ok(())
        } else {
            Err(Error::new("snd_pcm_hw_params_set_format", libc::EINVAL))
        }
    }

    pub fn get_format(&self) -> Result<Format> {
        Ok(self.format.get())
    }

    pub fn test_format(&self, v: Format) -> Result<()> {
        if v == Format::s16() { Ok(()) } else { Err(Error::new("snd_pcm_hw_params_test_format", libc::EINVAL)) }
    }

    pub fn test_access(&self, v: Access) -> Result<()> {
        if v == Access::RWInterleaved { Ok(()) } else { Err(Error::new("snd_pcm_hw_params_test_access", libc::EINVAL)) }
    }

    pub fn set_access(&self, v: Access) -> Result<()> {
        if v == Access::RWInterleaved {
            self.access.set(v);
            Ok(())
        } else {
            Err(Error::new("snd_pcm_hw_params_set_access", libc::EINVAL))
        }
    }

    pub fn get_access(&self) -> Result<Access> {
        Ok(self.access.get())
    }

    pub fn set_period_size_near(&self, v: Frames, _dir: ValueOr) -> Result<Frames> {
        let v = v.max(1);
        self.period_frames.set(v);
        Ok(v)
    }

    pub fn set_period_size(&self, v: Frames, _dir: ValueOr) -> Result<()> {
        self.period_frames.set(v.max(1));
        Ok(())
    }

    pub fn get_period_size(&self) -> Result<Frames> {
        Ok(self.period_frames.get())
    }

    pub fn set_buffer_size_near(&self, v: Frames) -> Result<Frames> {
        let v = v.max(1).min(self.pcm.buffer_frames.get());
        self.buffer_frames.set(v);
        Ok(v)
    }

    pub fn set_buffer_size(&self, v: Frames) -> Result<()> {
        self.buffer_frames.set(v.max(1).min(self.pcm.buffer_frames.get()));
        Ok(())
    }

    pub fn get_buffer_size(&self) -> Result<Frames> {
        Ok(self.buffer_frames.get())
    }

    pub fn get_buffer_size_min(&self) -> Result<Frames> {
        Ok(1)
    }

    pub fn get_buffer_size_max(&self) -> Result<Frames> {
        Ok(self.pcm.buffer_frames.get())
    }

    pub fn can_pause(&self) -> bool {
        true
    }

    pub fn can_resume(&self) -> bool {
        true
    }
}

#[derive(Debug)]
pub struct SwParams<'a> {
    _pcm: &'a PCM,
    avail_min: cell::Cell<Frames>,
    start_threshold: cell::Cell<Frames>,
    stop_threshold: cell::Cell<Frames>,
}

impl<'a> SwParams<'a> {
    fn new(pcm: &'a PCM) -> Result<Self> {
        Ok(Self {
            _pcm: pcm,
            avail_min: cell::Cell::new(1),
            start_threshold: cell::Cell::new(pcm.buffer_frames.get()),
            stop_threshold: cell::Cell::new(pcm.buffer_frames.get()),
        })
    }

    pub fn set_avail_min(&self, v: Frames) -> Result<()> {
        self.avail_min.set(v);
        Ok(())
    }

    pub fn get_avail_min(&self) -> Result<Frames> {
        Ok(self.avail_min.get())
    }

    pub fn get_boundary(&self) -> Result<Frames> {
        Ok(isize::MAX / 2)
    }

    pub fn set_start_threshold(&self, v: Frames) -> Result<()> {
        self.start_threshold.set(v);
        Ok(())
    }

    pub fn get_start_threshold(&self) -> Result<Frames> {
        Ok(self.start_threshold.get())
    }

    pub fn set_stop_threshold(&self, v: Frames) -> Result<()> {
        self.stop_threshold.set(v);
        Ok(v).map(|_| ())
    }

    pub fn get_stop_threshold(&self) -> Result<Frames> {
        Ok(self.stop_threshold.get())
    }

    pub fn set_tstamp_mode(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    pub fn get_tstamp_mode(&self) -> Result<bool> {
        Ok(false)
    }
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    Open = 0,
    Setup = 1,
    Prepared = 2,
    Running = 3,
    XRun = 4,
    Draining = 5,
    Paused = 6,
    Suspended = 7,
    Disconnected = 8,
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Access {
    MMapInterleaved = 0,
    MMapNonInterleaved = 1,
    MMapComplex = 2,
    RWInterleaved = 3,
    RWNonInterleaved = 4,
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Format {
    Unknown = -1,
    S8 = 0,
    U8 = 1,
    S16LE = 2,
    S16BE = 3,
    U16LE = 4,
    U16BE = 5,
    S24LE = 6,
    S24BE = 7,
    U24LE = 8,
    U24BE = 9,
    S32LE = 10,
    S32BE = 11,
    U32LE = 12,
    U32BE = 13,
    FloatLE = 14,
    FloatBE = 15,
    Float64LE = 16,
    Float64BE = 17,
}

impl Format {
    pub const fn s16() -> Format {
        #[cfg(target_endian = "little")]
        {
            Format::S16LE
        }
        #[cfg(target_endian = "big")]
        {
            Format::S16BE
        }
    }

    pub const fn u16() -> Format {
        #[cfg(target_endian = "little")]
        {
            Format::U16LE
        }
        #[cfg(target_endian = "big")]
        {
            Format::U16BE
        }
    }

    pub const fn s32() -> Format {
        #[cfg(target_endian = "little")]
        {
            Format::S32LE
        }
        #[cfg(target_endian = "big")]
        {
            Format::S32BE
        }
    }

    pub const fn u32() -> Format {
        #[cfg(target_endian = "little")]
        {
            Format::U32LE
        }
        #[cfg(target_endian = "big")]
        {
            Format::U32BE
        }
    }

    pub const fn float() -> Format {
        #[cfg(target_endian = "little")]
        {
            Format::FloatLE
        }
        #[cfg(target_endian = "big")]
        {
            Format::FloatBE
        }
    }

    pub const fn float64() -> Format {
        #[cfg(target_endian = "little")]
        {
            Format::Float64LE
        }
        #[cfg(target_endian = "big")]
        {
            Format::Float64BE
        }
    }

    pub const fn s24() -> Format {
        #[cfg(target_endian = "little")]
        {
            Format::S24LE
        }
        #[cfg(target_endian = "big")]
        {
            Format::S24BE
        }
    }

    pub const fn u24() -> Format {
        #[cfg(target_endian = "little")]
        {
            Format::U24LE
        }
        #[cfg(target_endian = "big")]
        {
            Format::U24BE
        }
    }

    pub fn width(&self) -> Result<i32> {
        Ok(size_of_format(*self) as i32 * 8)
    }

    pub fn physical_width(&self) -> Result<i32> {
        self.width()
    }

    pub fn silence_16(&self) -> u16 {
        0
    }

    pub fn little_endian(&self) -> Result<bool> {
        Ok(matches!(
            self,
            Format::S16LE | Format::U16LE | Format::S24LE | Format::U24LE | Format::S32LE | Format::U32LE | Format::FloatLE | Format::Float64LE
        ))
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub trait IoFormat: Copy {
    const FORMAT: Format;
}

impl IoFormat for i8 { const FORMAT: Format = Format::S8; }
impl IoFormat for u8 { const FORMAT: Format = Format::U8; }
impl IoFormat for i16 { const FORMAT: Format = Format::s16(); }
impl IoFormat for u16 { const FORMAT: Format = Format::u16(); }
impl IoFormat for i32 { const FORMAT: Format = Format::s32(); }
impl IoFormat for u32 { const FORMAT: Format = Format::u32(); }
impl IoFormat for f32 { const FORMAT: Format = Format::float(); }
impl IoFormat for f64 { const FORMAT: Format = Format::float64(); }

fn size_of_format(format: Format) -> isize {
    match format {
        Format::S8 | Format::U8 => 1,
        Format::S16LE | Format::S16BE | Format::U16LE | Format::U16BE => 2,
        Format::S24LE | Format::S24BE | Format::U24LE | Format::U24BE => 4,
        Format::S32LE | Format::S32BE | Format::U32LE | Format::U32BE | Format::FloatLE | Format::FloatBE => 4,
        Format::Float64LE | Format::Float64BE => 8,
        Format::Unknown => 1,
    }
}
