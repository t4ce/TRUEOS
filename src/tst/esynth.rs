use core::sync::atomic::{AtomicU32, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use embassy_time::{Duration, Timer};

const TONE_HZ: u32 = 440;
const TONE_MS: u64 = 10_200;
const VOLUME: i32 = 9_000;
const SINE_TABLE_BITS: u32 = 8;

static CALLBACKS: AtomicU32 = AtomicU32::new(0);
static SAMPLES_WRITTEN: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
pub async fn cpal_service_task() {
    CALLBACKS.store(0, Ordering::Release);
    SAMPLES_WRITTEN.store(0, Ordering::Release);

    crate::log!("cpal-service: smoke task start\n");

    let host = cpal::default_host();
    crate::log!("cpal-service: host={}\n", host.id().name());

    let Some(device) = host.default_output_device() else {
        crate::log_warn!(target: "service"; "cpal-service: no default output device\n");
        return;
    };

    match device.name() {
        Ok(name) => crate::log!("cpal-service: output device={}\n", name),
        Err(err) => {
            crate::log_warn!(target: "service"; "cpal-service: output name err={:?}\n", err)
        }
    }

    let supported = match device.default_output_config() {
        Ok(config) => config,
        Err(err) => {
            crate::log_warn!(target: "service"; "cpal-service: default config err={:?}\n", err);
            return;
        }
    };

    crate::log!(
        "cpal-service: config channels={} rate={} format={}\n",
        supported.channels(),
        supported.sample_rate().0,
        supported.sample_format()
    );

    if supported.sample_format() != cpal::SampleFormat::I16 {
        crate::log_warn!(
            target: "service";
            "cpal-service: unsupported smoke format {}; expected i16\n",
            supported.sample_format()
        );
        return;
    }

    let config: cpal::StreamConfig = supported.into();
    if config.channels != 2 || config.sample_rate.0 == 0 {
        crate::log_warn!(
            target: "service";
            "cpal-service: unsupported smoke layout channels={} rate={}\n",
            config.channels,
            config.sample_rate.0
        );
        return;
    }

    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0;
    let mut phase = 0u32;
    let phase_step = (((TONE_HZ as u64) << 32) / sample_rate as u64) as u32;
    let stream = match device.build_output_stream::<i16, _, _>(
        &config,
        move |data, _info| {
            CALLBACKS.fetch_add(1, Ordering::AcqRel);
            SAMPLES_WRITTEN.fetch_add(data.len().min(u32::MAX as usize) as u32, Ordering::AcqRel);

            for frame in data.chunks_mut(channels) {
                let idx = (phase >> (32 - SINE_TABLE_BITS)) as usize;
                let sample = (crate::aud::tables::SINE_TABLE[idx] as i32 * VOLUME / 32_767) as i16;
                phase = phase.wrapping_add(phase_step);

                for out in frame.iter_mut() {
                    *out = sample;
                }
            }
        },
        move |err| {
            crate::log_warn!(target: "service"; "cpal-service: stream err={:?}\n", err);
        },
        None,
    ) {
        Ok(stream) => stream,
        Err(err) => {
            crate::log_warn!(target: "service"; "cpal-service: build stream err={:?}\n", err);
            return;
        }
    };

    if let Err(err) = stream.play() {
        crate::log_warn!(target: "service"; "cpal-service: play err={:?}\n", err);
        return;
    }

    Timer::after(Duration::from_millis(TONE_MS)).await;
    drop(stream);

    crate::log!(
        "cpal-service: smoke done callbacks={} samples={}\n",
        CALLBACKS.load(Ordering::Acquire),
        SAMPLES_WRITTEN.load(Ordering::Acquire)
    );
}