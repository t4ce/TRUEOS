use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration, Timer};
use tinyaudio::prelude::*;

const TONE_HZ: u32 = 440;
const TONE_MS: u64 = 10_200;
const VOLUME: f32 = 9_000.0 / 32_767.0;
const SAMPLE_RATE: usize = 48_000;
const CHANNELS: usize = 2;
const CHANNEL_SAMPLE_COUNT: usize = 480;
const SINE_TABLE_BITS: u32 = 8;

static CALLBACKS: AtomicU32 = AtomicU32::new(0);
static SAMPLES_WRITTEN: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
pub async fn tinyaudio_service_task() {
    CALLBACKS.store(0, Ordering::Release);
    SAMPLES_WRITTEN.store(0, Ordering::Release);

    crate::log!("tinyaudio-service: smoke task start\n");

    let params = OutputDeviceParameters {
        sample_rate: SAMPLE_RATE,
        channels_count: CHANNELS,
        channel_sample_count: CHANNEL_SAMPLE_COUNT,
    };
    crate::log!(
        "tinyaudio-service: config channels={} rate={} frames={}\n",
        params.channels_count,
        params.sample_rate,
        params.channel_sample_count
    );

    let mut phase = 0u32;
    let phase_step = (((TONE_HZ as u64) << 32) / params.sample_rate as u64) as u32;
    let device = match run_output_device(params, move |data| {
        CALLBACKS.fetch_add(1, Ordering::AcqRel);
        SAMPLES_WRITTEN.fetch_add(data.len().min(u32::MAX as usize) as u32, Ordering::AcqRel);

        for frame in data.chunks_mut(CHANNELS) {
            let idx = (phase >> (32 - SINE_TABLE_BITS)) as usize;
            let sample = crate::aud::tables::SINE_TABLE[idx] as f32 / 32_767.0 * VOLUME;
            phase = phase.wrapping_add(phase_step);

            for out in frame.iter_mut() {
                *out = sample;
            }
        }
    }) {
        Ok(device) => device,
        Err(err) => {
            crate::log_warn!(target: "service"; "tinyaudio-service: open err={}\n", err);
            return;
        }
    };

    Timer::after(Duration::from_millis(TONE_MS)).await;
    drop(device);

    crate::log!(
        "tinyaudio-service: smoke done callbacks={} samples={}\n",
        CALLBACKS.load(Ordering::Acquire),
        SAMPLES_WRITTEN.load(Ordering::Acquire)
    );
}
