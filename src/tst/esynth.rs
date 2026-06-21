use alloc::{sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::aud::live_piano::{LivePianoRenderSource, backing_config};
use embassy_time::{Duration, Timer};
use spin::Mutex;
use tinyaudio::prelude::*;

const TONE_HZ: u32 = 440;
const VOLUME: f32 = 9_000.0 / 32_767.0;
const SAMPLE_RATE: usize = 48_000;
const CHANNELS: usize = 2;
const CHANNEL_SAMPLE_COUNT: usize = 480;
const SERVICE_HEARTBEAT_MS: u64 = 30_000;
const SINE_TABLE_BITS: u32 = 8;
const SINE_TABLE_SHIFT: u32 = 32 - SINE_TABLE_BITS;
const PIANO_SOURCE_ENABLED: bool = true;
const TONE_SOURCE_ENABLED: bool = false;
const PCM_DUMP_ENABLED: bool = false;
const PCM_DUMP_SECONDS: usize = 10;
const PCM_DUMP_PATH: &str = "audio/tinyaudio-prehda.wav";
const PCM_DUMP_POLL_MS: u64 = 1_000;
const LIVE_PCM_RING_SECONDS: usize = 4;

static CALLBACKS: AtomicU32 = AtomicU32::new(0);
static SAMPLES_WRITTEN: AtomicU32 = AtomicU32::new(0);
static LIVE_PCM_RING: Mutex<Option<LivePcmRing>> = Mutex::new(None);

struct TinyaudioDemoMixer {
    piano: PianoSource,
    tone: ToneSource,
    overlay: PcmOverlaySource,
}

struct PianoSource {
    live: LivePianoRenderSource,
    buffer: Vec<i16>,
    enabled: bool,
}

struct ToneSource {
    phase: u32,
    phase_step: u32,
    gain: f32,
    enabled: bool,
}

struct PcmDumpCapture {
    samples: Vec<i16>,
    target_samples: usize,
    complete: bool,
    taken: bool,
}

struct LivePcmRing {
    samples: Vec<i16>,
    capacity: usize,
    write_seq: u64,
}

struct PcmOverlaySource {
    current: Option<crate::aud::pcm_lane::PcmLaneRequest>,
    cursor: usize,
    stop_generation: u32,
}

impl TinyaudioDemoMixer {
    fn new(params: OutputDeviceParameters) -> Self {
        Self {
            piano: PianoSource::new(params, PIANO_SOURCE_ENABLED),
            tone: ToneSource::new(TONE_HZ, params.sample_rate, VOLUME, TONE_SOURCE_ENABLED),
            overlay: PcmOverlaySource::new(),
        }
    }

    fn render(&mut self, data: &mut [f32]) {
        CALLBACKS.fetch_add(1, Ordering::AcqRel);
        SAMPLES_WRITTEN.fetch_add(data.len().min(u32::MAX as usize) as u32, Ordering::AcqRel);

        data.fill(0.0);
        self.piano.mix_into(data);
        self.tone.mix_into(data);
        self.overlay.mix_into(data);
    }
}

impl PcmDumpCapture {
    fn new(seconds: usize) -> Self {
        let target_samples = SAMPLE_RATE.saturating_mul(CHANNELS).saturating_mul(seconds);
        Self {
            samples: Vec::with_capacity(target_samples),
            target_samples,
            complete: target_samples == 0,
            taken: false,
        }
    }

    fn capture_f32(&mut self, data: &[f32]) {
        if self.complete || self.taken {
            return;
        }

        let remaining = self.target_samples.saturating_sub(self.samples.len());
        for sample in data.iter().take(remaining) {
            self.samples.push(sample_f32_to_i16(*sample));
        }

        if self.samples.len() >= self.target_samples {
            self.complete = true;
        }
    }

    fn take_samples_if_complete(&mut self) -> Option<Vec<i16>> {
        if !self.complete || self.taken {
            return None;
        }
        self.taken = true;
        Some(core::mem::take(&mut self.samples))
    }
}

impl LivePcmRing {
    fn new(seconds: usize) -> Self {
        let capacity = SAMPLE_RATE.saturating_mul(CHANNELS).saturating_mul(seconds);
        Self {
            samples: vec![0; capacity],
            capacity,
            write_seq: 0,
        }
    }

    fn earliest_seq(&self) -> u64 {
        self.write_seq.saturating_sub(self.capacity as u64)
    }

    fn start_cursor(&self, preroll_samples: usize) -> u64 {
        let cursor = self.write_seq.saturating_sub(preroll_samples as u64);
        cursor.max(self.earliest_seq())
    }

    fn push_f32(&mut self, data: &[f32]) {
        if self.capacity == 0 {
            return;
        }

        for sample in data {
            let idx = (self.write_seq as usize) % self.capacity;
            self.samples[idx] = sample_f32_to_i16(*sample);
            self.write_seq = self.write_seq.wrapping_add(1);
        }
    }

    fn read_since(&self, cursor: u64, out: &mut Vec<i16>, max_samples: usize) -> u64 {
        if self.capacity == 0 || max_samples == 0 {
            return cursor;
        }

        let mut next = cursor.max(self.earliest_seq()).min(self.write_seq);
        let mut take = core::cmp::min(
            self.write_seq.saturating_sub(next) as usize,
            max_samples.min(self.capacity),
        );
        take &= !(CHANNELS - 1);

        for _ in 0..take {
            let idx = (next as usize) % self.capacity;
            out.push(self.samples[idx]);
            next = next.wrapping_add(1);
        }

        next
    }
}

fn live_pcm_reset(seconds: usize) {
    *LIVE_PCM_RING.lock() = Some(LivePcmRing::new(seconds));
}

fn live_pcm_push_f32(data: &[f32]) {
    if let Some(ring) = LIVE_PCM_RING.lock().as_mut() {
        ring.push_f32(data);
    }
}

pub fn live_pcm_stream_start_cursor(preroll_samples: usize) -> Option<u64> {
    LIVE_PCM_RING
        .lock()
        .as_ref()
        .map(|ring| ring.start_cursor(preroll_samples))
}

pub fn live_pcm_read_since(cursor: u64, out: &mut Vec<i16>, max_samples: usize) -> Option<u64> {
    LIVE_PCM_RING
        .lock()
        .as_ref()
        .map(|ring| ring.read_since(cursor, out, max_samples))
}

pub fn submit_pcm_overlay(label: &'static str, samples: Vec<i16>) -> Result<usize, &'static str> {
    crate::aud::pcm_lane::submit_i16_stereo_48k(label, samples)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_tinyaudio_audio_urgent_pending() -> i32 {
    i32::from(crate::aud::pcm_lane::urgent_pending())
}

impl PianoSource {
    fn new(params: OutputDeviceParameters, enabled: bool) -> Self {
        Self {
            live: LivePianoRenderSource::new(),
            buffer: vec![0; params.channel_sample_count * params.channels_count],
            enabled,
        }
    }

    fn mix_into(&mut self, data: &mut [f32]) {
        if !self.enabled || data.len() != self.buffer.len() {
            return;
        }

        self.buffer.fill(0);
        let frames = data.len() / CHANNELS;
        if !self.live.render_into(self.buffer.as_mut_slice(), frames) {
            return;
        }

        for (dst, src) in data.iter_mut().zip(self.buffer.iter().copied()) {
            *dst += src as f32 / 32_767.0;
        }
    }
}

impl PcmOverlaySource {
    const fn new() -> Self {
        Self {
            current: None,
            cursor: 0,
            stop_generation: 0,
        }
    }

    fn apply_control(&mut self) -> bool {
        let generation = crate::aud::pcm_lane::stop_generation();
        if generation != self.stop_generation {
            self.stop_generation = generation;
            if let Some(current) = self.current.take() {
                crate::log!(
                    "tinyaudio-service: overlay stop label={} samples={}\n",
                    current.label,
                    current.samples.len()
                );
            }
            self.cursor = 0;
        }

        crate::aud::pcm_lane::paused()
    }

    fn take_pending(&mut self) {
        let next = crate::aud::pcm_lane::take_pending();
        if let Some(next) = next {
            crate::log!(
                "tinyaudio-service: overlay start label={} samples={} frames={}\n",
                next.label,
                next.samples.len(),
                next.samples.len() / CHANNELS
            );
            self.current = Some(next);
            self.cursor = 0;
        }
    }

    fn mix_into(&mut self, data: &mut [f32]) {
        if self.apply_control() {
            return;
        }

        self.take_pending();

        let Some(current) = self.current.as_ref() else {
            return;
        };

        let remaining = current.samples.len().saturating_sub(self.cursor);
        let take = data.len().min(remaining);
        let volume = crate::aud::pcm_lane::volume_percent() as f32 / 100.0;
        for (dst, src) in data.iter_mut().zip(
            current.samples[self.cursor..self.cursor + take]
                .iter()
                .copied(),
        ) {
            *dst += src as f32 / 32_767.0 * volume;
        }

        self.cursor += take;
        if self.cursor >= current.samples.len() {
            crate::log!(
                "tinyaudio-service: overlay done label={} samples={}\n",
                current.label,
                current.samples.len()
            );
            self.current = None;
            self.cursor = 0;
        }
    }
}

impl ToneSource {
    fn new(freq_hz: u32, sample_rate: usize, gain: f32, enabled: bool) -> Self {
        Self {
            phase: 0,
            phase_step: (((freq_hz as u64) << 32) / sample_rate as u64) as u32,
            gain,
            enabled,
        }
    }

    fn mix_into(&mut self, data: &mut [f32]) {
        if !self.enabled {
            return;
        }

        for frame in data.chunks_mut(CHANNELS) {
            let idx = (self.phase >> SINE_TABLE_SHIFT) as usize;
            let sample = crate::aud::tables::SINE_TABLE[idx] as f32 / 32_767.0 * self.gain;
            self.phase = self.phase.wrapping_add(self.phase_step);

            for out in frame.iter_mut() {
                *out += sample;
            }
        }
    }
}

fn push_le_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_le_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn sample_f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

pub fn live_wav_stream_header() -> Vec<u8> {
    let data_bytes = u32::MAX - 36;
    let mut wav = Vec::with_capacity(44);

    wav.extend_from_slice(b"RIFF");
    push_le_u32(&mut wav, 36u32.saturating_add(data_bytes));
    wav.extend_from_slice(b"WAVE");

    wav.extend_from_slice(b"fmt ");
    push_le_u32(&mut wav, 16);
    push_le_u16(&mut wav, 1);
    push_le_u16(&mut wav, CHANNELS as u16);
    push_le_u32(&mut wav, SAMPLE_RATE as u32);
    push_le_u32(&mut wav, (SAMPLE_RATE * CHANNELS * core::mem::size_of::<i16>()) as u32);
    push_le_u16(&mut wav, (CHANNELS * core::mem::size_of::<i16>()) as u16);
    push_le_u16(&mut wav, 16);

    wav.extend_from_slice(b"data");
    push_le_u32(&mut wav, data_bytes);

    wav
}

fn wav_pcm_s16_stereo_48k(samples: &[i16]) -> Vec<u8> {
    let data_bytes = samples.len().saturating_mul(core::mem::size_of::<i16>());
    let riff_payload_len = 36usize.saturating_add(data_bytes);
    let mut wav = Vec::with_capacity(44usize.saturating_add(data_bytes));

    wav.extend_from_slice(b"RIFF");
    push_le_u32(&mut wav, riff_payload_len.min(u32::MAX as usize) as u32);
    wav.extend_from_slice(b"WAVE");

    wav.extend_from_slice(b"fmt ");
    push_le_u32(&mut wav, 16);
    push_le_u16(&mut wav, 1);
    push_le_u16(&mut wav, CHANNELS as u16);
    push_le_u32(&mut wav, SAMPLE_RATE as u32);
    push_le_u32(&mut wav, (SAMPLE_RATE * CHANNELS * core::mem::size_of::<i16>()) as u32);
    push_le_u16(&mut wav, (CHANNELS * core::mem::size_of::<i16>()) as u16);
    push_le_u16(&mut wav, 16);

    wav.extend_from_slice(b"data");
    push_le_u32(&mut wav, data_bytes.min(u32::MAX as usize) as u32);
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }

    wav
}

async fn write_pcm_dump(samples: Vec<i16>) {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!(
            "tinyaudio-service: pcm-dump skipped path={} samples={} err=no-root\n",
            PCM_DUMP_PATH,
            samples.len()
        );
        return;
    };

    let wav = wav_pcm_s16_stereo_48k(samples.as_slice());
    let bytes = wav.len();
    match crate::r::fs::trueosfs::file_in_async(disk, PCM_DUMP_PATH, wav.as_slice()).await {
        Ok(true) => {
            crate::log!(
                "tinyaudio-service: pcm-dump wrote path={} bytes={} samples={} seconds={}\n",
                PCM_DUMP_PATH,
                bytes,
                samples.len(),
                PCM_DUMP_SECONDS
            );
        }
        Ok(false) => {
            crate::log!(
                "tinyaudio-service: pcm-dump failed path={} bytes={} err=no-space\n",
                PCM_DUMP_PATH,
                bytes
            );
        }
        Err(err) => {
            crate::log!(
                "tinyaudio-service: pcm-dump failed path={} bytes={} err={:?}\n",
                PCM_DUMP_PATH,
                bytes,
                err
            );
        }
    }
}

#[embassy_executor::task]
pub async fn tinyaudio_service_task() {
    CALLBACKS.store(0, Ordering::Release);
    SAMPLES_WRITTEN.store(0, Ordering::Release);

    crate::log!(
        "tinyaudio-service: audio task start slot={} policy=ap1-ui-service\n",
        crate::percpu::current_slot()
    );
    live_pcm_reset(LIVE_PCM_RING_SECONDS);

    let params = OutputDeviceParameters {
        sample_rate: SAMPLE_RATE,
        channels_count: CHANNELS,
        channel_sample_count: CHANNEL_SAMPLE_COUNT,
    };
    let (backing_enabled, backing_bpm, backing_volume_pct) = backing_config();
    crate::log!(
        "tinyaudio-service: config channels={} rate={} frames={} piano={} tone={} backing={} backing_bpm={} backing_vol={}%\n",
        params.channels_count,
        params.sample_rate,
        params.channel_sample_count,
        PIANO_SOURCE_ENABLED,
        TONE_SOURCE_ENABLED,
        backing_enabled,
        backing_bpm,
        backing_volume_pct
    );

    let dump = if PCM_DUMP_ENABLED {
        Some(Arc::new(Mutex::new(PcmDumpCapture::new(PCM_DUMP_SECONDS))))
    } else {
        None
    };
    let dump_for_callback = dump.as_ref().map(Arc::clone);
    let mut mixer = TinyaudioDemoMixer::new(params);
    let device = match run_output_device(params, move |data| {
        mixer.render(data);
        live_pcm_push_f32(data);
        if let Some(dump) = dump_for_callback.as_ref() {
            dump.lock().capture_f32(data);
        }
    }) {
        Ok(device) => device,
        Err(err) => {
            crate::log_warn!(target: "service"; "tinyaudio-service: open err={}\n", err);
            return;
        }
    };

    let _device = device;
    let mut heartbeat_elapsed_ms = 0u64;
    loop {
        Timer::after(Duration::from_millis(PCM_DUMP_POLL_MS)).await;
        heartbeat_elapsed_ms = heartbeat_elapsed_ms.saturating_add(PCM_DUMP_POLL_MS);

        if let Some(dump) = dump.as_ref() {
            let samples = { dump.lock().take_samples_if_complete() };
            if let Some(samples) = samples {
                write_pcm_dump(samples).await;
            }
        }

        if heartbeat_elapsed_ms >= SERVICE_HEARTBEAT_MS {
            heartbeat_elapsed_ms = 0;
            crate::log!(
                "tinyaudio-service: live callbacks={} samples={}\n",
                CALLBACKS.load(Ordering::Acquire),
                SAMPLES_WRITTEN.load(Ordering::Acquire)
            );
        }
    }
}
