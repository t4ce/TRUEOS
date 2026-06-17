use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo, OutputStreamTimestamp,
    PauseStreamError, PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError,
    StreamInstant, SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};

const SAMPLE_RATE: SampleRate = SampleRate(48_000);
const CHANNELS: u16 = 2;
const SAMPLE_FORMAT: SampleFormat = SampleFormat::I16;
const DEFAULT_CALLBACK_FRAMES: u32 = 480;
const MIN_CALLBACK_FRAMES: u32 = 64;
const MAX_CALLBACK_FRAMES: u32 = 8192;
const STREAM_HANDLE_NONE: usize = 0;
const PUMP_PERIOD_MS: u64 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

#[derive(Debug)]
pub struct Devices {
    yielded: bool,
}

pub struct Host;

pub struct Stream {
    shared: Arc<SharedStream>,
}

struct SharedStream {
    alive: AtomicBool,
    pump_spawned: AtomicBool,
    locked: AtomicBool,
    inner: UnsafeCell<StreamInner>,
}

pub type SupportedInputConfigs = alloc::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = alloc::vec::IntoIter<SupportedStreamConfigRange>;

type OutputCallback = Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>;
type ErrorCallback = Box<dyn FnMut(StreamError) + Send + 'static>;

struct StreamInner {
    hda_handle: usize,
    data_callback: OutputCallback,
    error_callback: ErrorCallback,
    buffer: Vec<i16>,
    playing: bool,
    callback_count: u64,
}

unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

unsafe extern "C" {
    fn trueos_cpal_hda_is_available() -> i32;
    fn trueos_cpal_hda_open_pcm_stream() -> usize;
    fn trueos_cpal_hda_close_pcm_stream(handle: usize);
    fn trueos_cpal_hda_writable_samples(handle: usize, guard_samples: usize) -> isize;
    fn trueos_cpal_hda_push_samples(handle: usize, samples: *const i16, len: usize) -> i32;
    fn trueos_cpal_spawn_output_pump(
        ctx: usize,
        pump: unsafe extern "C" fn(usize) -> i32,
        period_ms: u64,
    ) -> i32;
}

impl Host {
    #[allow(dead_code)]
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Self)
    }
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Self { yielded: false })
    }
}

impl Device {
    fn output_config_range() -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(
            CHANNELS,
            SAMPLE_RATE,
            SAMPLE_RATE,
            SupportedBufferSize::Range {
                min: MIN_CALLBACK_FRAMES,
                max: MAX_CALLBACK_FRAMES,
            },
            SAMPLE_FORMAT,
        )
    }

    fn output_config() -> SupportedStreamConfig {
        Self::output_config_range().with_max_sample_rate()
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("TRUEOS Intel HDA".to_string())
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Ok(Vec::new().into_iter())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(vec![Self::output_config_range()].into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Err(DefaultStreamConfigError::StreamTypeNotSupported)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        if Host::is_available() {
            Ok(Self::output_config())
        } else {
            Err(DefaultStreamConfigError::DeviceNotAvailable)
        }
    }

    fn build_input_stream_raw<D, E>(
        &self,
        _config: &StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Err(BuildStreamError::StreamConfigNotSupported)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        if sample_format != SAMPLE_FORMAT
            || config.channels != CHANNELS
            || config.sample_rate != SAMPLE_RATE
        {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        let callback_frames = match config.buffer_size {
            BufferSize::Default => DEFAULT_CALLBACK_FRAMES,
            BufferSize::Fixed(frames)
                if (MIN_CALLBACK_FRAMES..=MAX_CALLBACK_FRAMES).contains(&frames) =>
            {
                frames
            }
            BufferSize::Fixed(_) => return Err(BuildStreamError::StreamConfigNotSupported),
        };

        let hda_handle = unsafe { trueos_cpal_hda_open_pcm_stream() };
        if hda_handle == STREAM_HANDLE_NONE {
            return Err(BuildStreamError::DeviceNotAvailable);
        }

        let sample_count = callback_frames as usize * CHANNELS as usize;
        Ok(Stream {
            shared: Arc::new(SharedStream {
                alive: AtomicBool::new(true),
                pump_spawned: AtomicBool::new(false),
                locked: AtomicBool::new(false),
                inner: UnsafeCell::new(StreamInner {
                    hda_handle,
                    data_callback: Box::new(data_callback),
                    error_callback: Box::new(error_callback),
                    buffer: vec![0; sample_count],
                    playing: false,
                    callback_count: 0,
                }),
            }),
        })
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        unsafe { trueos_cpal_hda_is_available() != 0 }
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        None
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        Self::is_available().then_some(Device)
    }
}

impl SharedStream {
    fn with_inner<T>(
        &self,
        f: impl FnOnce(&mut StreamInner) -> T,
    ) -> Result<T, BackendSpecificError> {
        if self.locked.swap(true, Ordering::AcqRel) {
            return Err(backend_error("TRUEOS HDA stream is already in a callback"));
        }

        let result = f(unsafe { &mut *self.inner.get() });
        self.locked.store(false, Ordering::Release);
        Ok(result)
    }

    fn with_inner_blocking<T>(&self, f: impl FnOnce(&mut StreamInner) -> T) -> T {
        while self.locked.swap(true, Ordering::AcqRel) {
            core::hint::spin_loop();
        }

        let result = f(unsafe { &mut *self.inner.get() });
        self.locked.store(false, Ordering::Release);
        result
    }
}

impl StreamInner {
    fn pump_once(&mut self) -> Result<(), BackendSpecificError> {
        if !self.playing {
            return Ok(());
        }

        let writable =
            unsafe { trueos_cpal_hda_writable_samples(self.hda_handle, self.buffer.len()) };
        if writable < 0 {
            (self.error_callback)(StreamError::DeviceNotAvailable);
            return Err(backend_error("TRUEOS HDA stream is not writable"));
        }
        if (writable as usize) < self.buffer.len() {
            return Ok(());
        }

        self.buffer.fill(0);

        let callback_instant = StreamInstant::new(0, 0);
        let playback_instant = StreamInstant::new(0, 0);
        let info = OutputCallbackInfo::new(OutputStreamTimestamp {
            callback: callback_instant,
            playback: playback_instant,
        });
        let mut data = unsafe {
            Data::from_parts(self.buffer.as_mut_ptr().cast(), self.buffer.len(), SAMPLE_FORMAT)
        };

        (self.data_callback)(&mut data, &info);

        let rc = unsafe {
            trueos_cpal_hda_push_samples(self.hda_handle, self.buffer.as_ptr(), self.buffer.len())
        };
        if rc != 0 {
            (self.error_callback)(StreamError::DeviceNotAvailable);
            return Err(backend_error("TRUEOS HDA rejected PCM samples"));
        }

        self.callback_count = self.callback_count.saturating_add(1);
        Ok(())
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.shared
            .with_inner(|inner| {
                inner.playing = true;
                inner.pump_once()
            })
            .map_err(PlayStreamError::from)?
            .map_err(PlayStreamError::from)?;

        if !self.shared.pump_spawned.swap(true, Ordering::AcqRel) {
            let ctx = Arc::into_raw(Arc::clone(&self.shared)) as usize;
            let spawned = unsafe {
                trueos_cpal_spawn_output_pump(ctx, output_pump_trampoline, PUMP_PERIOD_MS)
            };
            if spawned != 0 {
                unsafe {
                    drop(Arc::from_raw(ctx as *const SharedStream));
                }
                self.shared.pump_spawned.store(false, Ordering::Release);
            }
        }

        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        self.shared
            .with_inner(|inner| {
                inner.playing = false;
            })
            .map_err(PauseStreamError::from)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.shared.alive.store(false, Ordering::Release);
        self.shared.with_inner_blocking(|inner| {
            inner.playing = false;
            if inner.hda_handle != STREAM_HANDLE_NONE {
                unsafe {
                    trueos_cpal_hda_close_pcm_stream(inner.hda_handle);
                }
                inner.hda_handle = STREAM_HANDLE_NONE;
            }
        });
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        if self.yielded || !Host::is_available() {
            return None;
        }
        self.yielded = true;
        Some(Device)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = usize::from(!self.yielded && Host::is_available());
        (len, Some(len))
    }
}

fn backend_error(description: &str) -> BackendSpecificError {
    BackendSpecificError {
        description: description.to_string(),
    }
}

unsafe extern "C" fn output_pump_trampoline(ctx: usize) -> i32 {
    let shared = unsafe { Arc::from_raw(ctx as *const SharedStream) };

    let keep_running = shared.alive.load(Ordering::Acquire);
    let rc = if keep_running {
        match shared.with_inner(|inner| inner.pump_once()) {
            Ok(Ok(())) => 0,
            Ok(Err(_)) | Err(_) => -1,
        }
    } else {
        -1
    };

    if rc == 0 {
        let _ = Arc::into_raw(shared);
    }

    rc
}
