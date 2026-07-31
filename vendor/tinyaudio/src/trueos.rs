//! TRUEOS output device via Intel HDA.

#![cfg(target_os = "trueos")]

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::{
    cell::UnsafeCell,
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{AudioOutputDevice, BaseAudioOutputDevice, OutputDeviceParameters};

const SAMPLE_RATE: usize = 48_000;
const CHANNELS: usize = 2;
const STREAM_HANDLE_NONE: usize = 0;
const PUMP_PERIOD_MS: u64 = 5;
const TARGET_QUEUED_MS: usize = 20;
const TARGET_QUEUED_SAMPLES: usize = (SAMPLE_RATE * TARGET_QUEUED_MS / 1_000) * CHANNELS;
const URGENT_TARGET_QUEUED_MS: usize = 80;
const URGENT_TARGET_QUEUED_SAMPLES: usize =
    (SAMPLE_RATE * URGENT_TARGET_QUEUED_MS / 1_000) * CHANNELS;
const MAX_CALLBACK_BLOCKS_PER_PUMP: usize = 4;

/// TRUEOS audio output backed by the kernel HDA PCM stream.
pub struct TrueOsAudioDevice {
    shared: Arc<SharedStream>,
}

struct SharedStream {
    alive: AtomicBool,
    locked: AtomicBool,
    inner: UnsafeCell<StreamInner>,
}

struct StreamInner {
    hda_handle: usize,
    callback: Box<dyn FnMut(&mut [f32]) + Send + 'static>,
    data_buffer: Vec<f32>,
    output_buffer: Vec<i16>,
}

#[derive(Debug)]
struct TrueOsAudioError(&'static str);

unsafe impl Send for TrueOsAudioDevice {}
unsafe impl Send for SharedStream {}
unsafe impl Sync for SharedStream {}

unsafe extern "C" {
    fn trueos_tinyaudio_hda_is_available() -> i32;
    fn trueos_tinyaudio_hda_open_pcm_stream() -> usize;
    fn trueos_tinyaudio_hda_close_pcm_stream(handle: usize);
    fn trueos_tinyaudio_hda_writable_samples(handle: usize, guard_samples: usize) -> isize;
    fn trueos_tinyaudio_hda_queued_samples(handle: usize) -> isize;
    fn trueos_tinyaudio_hda_push_samples(handle: usize, samples: *const i16, len: usize) -> i32;
    fn trueos_tinyaudio_audio_urgent_pending() -> i32;
    fn trueos_tinyaudio_spawn_output_pump(
        ctx: usize,
        pump: unsafe extern "C" fn(usize) -> i32,
        period_ms: u64,
    ) -> i32;
}

impl BaseAudioOutputDevice for TrueOsAudioDevice {}

impl AudioOutputDevice for TrueOsAudioDevice {
    fn new<C>(params: OutputDeviceParameters, data_callback: C) -> Result<Self, Box<dyn Error>>
    where
        C: FnMut(&mut [f32]) + Send + 'static,
        Self: Sized,
    {
        if unsafe { trueos_tinyaudio_hda_is_available() } == 0 {
            return Err(audio_error("TRUEOS HDA output is not available"));
        }
        if params.sample_rate != SAMPLE_RATE {
            return Err(audio_error("TRUEOS HDA output requires 48 kHz samples"));
        }
        if params.channels_count != CHANNELS {
            return Err(audio_error("TRUEOS HDA output requires stereo samples"));
        }

        let sample_count = params
            .channel_sample_count
            .checked_mul(params.channels_count)
            .ok_or_else(|| audio_error("TRUEOS HDA output buffer is too large"))?;
        if sample_count == 0 {
            return Err(audio_error("TRUEOS HDA output buffer is empty"));
        }

        let hda_handle = unsafe { trueos_tinyaudio_hda_open_pcm_stream() };
        if hda_handle == STREAM_HANDLE_NONE {
            return Err(audio_error("TRUEOS HDA output stream could not be opened"));
        }

        let shared = Arc::new(SharedStream {
            alive: AtomicBool::new(true),
            locked: AtomicBool::new(false),
            inner: UnsafeCell::new(StreamInner {
                hda_handle,
                callback: Box::new(data_callback),
                data_buffer: vec![0.0; sample_count],
                output_buffer: vec![0; sample_count],
            }),
        });

        let ctx = Arc::into_raw(Arc::clone(&shared)) as usize;
        let spawned = unsafe {
            trueos_tinyaudio_spawn_output_pump(ctx, output_pump_trampoline, PUMP_PERIOD_MS)
        };
        if spawned != 0 {
            unsafe {
                drop(Arc::from_raw(ctx as *const SharedStream));
                trueos_tinyaudio_hda_close_pcm_stream(hda_handle);
            }
            return Err(audio_error("TRUEOS HDA output pump could not be spawned"));
        }

        Ok(Self { shared })
    }
}

impl Drop for TrueOsAudioDevice {
    fn drop(&mut self) {
        self.shared.alive.store(false, Ordering::Release);
        self.shared.with_inner_blocking(|inner| {
            if inner.hda_handle != STREAM_HANDLE_NONE {
                unsafe {
                    trueos_tinyaudio_hda_close_pcm_stream(inner.hda_handle);
                }
                inner.hda_handle = STREAM_HANDLE_NONE;
            }
        });
    }
}

impl SharedStream {
    fn with_inner<T>(&self, f: impl FnOnce(&mut StreamInner) -> T) -> Option<T> {
        if self.locked.swap(true, Ordering::AcqRel) {
            return None;
        }

        let result = f(unsafe { &mut *self.inner.get() });
        self.locked.store(false, Ordering::Release);
        Some(result)
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
    fn pump_once(&mut self) -> Result<(), ()> {
        for _ in 0..MAX_CALLBACK_BLOCKS_PER_PUMP {
            let target_queued_samples = if unsafe { trueos_tinyaudio_audio_urgent_pending() } != 0 {
                URGENT_TARGET_QUEUED_SAMPLES
            } else {
                TARGET_QUEUED_SAMPLES
            };
            let queued = unsafe { trueos_tinyaudio_hda_queued_samples(self.hda_handle) };
            if queued < 0 {
                return Err(());
            }
            if (queued as usize) >= target_queued_samples {
                return Ok(());
            }

            let writable = unsafe { trueos_tinyaudio_hda_writable_samples(self.hda_handle, 0) };
            if writable < 0 {
                return Err(());
            }
            if (writable as usize) < self.output_buffer.len() {
                return Ok(());
            }

            self.data_buffer.fill(0.0);
            (self.callback)(&mut self.data_buffer);

            for (input, output) in self.data_buffer.iter().zip(self.output_buffer.iter_mut()) {
                *output = (input.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            }

            let rc = unsafe {
                trueos_tinyaudio_hda_push_samples(
                    self.hda_handle,
                    self.output_buffer.as_ptr(),
                    self.output_buffer.len(),
                )
            };
            if rc != 0 {
                return Err(());
            }
        }

        Ok(())
    }
}

impl fmt::Display for TrueOsAudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Error for TrueOsAudioError {}

fn audio_error(message: &'static str) -> Box<dyn Error> {
    Box::new(TrueOsAudioError(message))
}

unsafe extern "C" fn output_pump_trampoline(ctx: usize) -> i32 {
    let shared = unsafe { Arc::from_raw(ctx as *const SharedStream) };

    let keep_running = shared.alive.load(Ordering::Acquire);
    let rc = if keep_running {
        match shared.with_inner(|inner| inner.pump_once()) {
            Some(Ok(())) => 0,
            Some(Err(())) | None => -1,
        }
    } else {
        -1
    };

    if rc == 0 {
        let _ = Arc::into_raw(shared);
    }

    rc
}
