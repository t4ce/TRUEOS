use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use spin::Mutex;

use crate::hda;

static PCM_LANE_REQUEST: Mutex<Option<PcmLaneRequest>> = Mutex::new(None);
static PCM_LANE_PAUSED: AtomicBool = AtomicBool::new(false);
static PCM_LANE_VOLUME_PERCENT: AtomicU16 = AtomicU16::new(100);
static PCM_LANE_STOP_GENERATION: AtomicU32 = AtomicU32::new(0);

pub struct PcmLaneRequest {
    pub label: &'static str,
    pub samples: Vec<i16>,
}

pub fn submit_i16_stereo_48k(
    label: &'static str,
    samples: Vec<i16>,
) -> Result<usize, &'static str> {
    if samples.is_empty() {
        return Err("empty PCM buffer");
    }
    if samples.len() % hda::PCM_CHANNELS != 0 {
        return Err("PCM buffer must be stereo interleaved");
    }

    let sample_count = samples.len();
    *PCM_LANE_REQUEST.lock() = Some(PcmLaneRequest { label, samples });
    crate::log!(
        "pcm-lane: queued label={} samples={} frames={} format=s16le/stereo/48k\n",
        label,
        sample_count,
        sample_count / hda::PCM_CHANNELS
    );
    Ok(sample_count / hda::PCM_CHANNELS)
}

pub fn urgent_pending() -> bool {
    PCM_LANE_REQUEST.lock().is_some()
}

pub fn take_pending() -> Option<PcmLaneRequest> {
    PCM_LANE_REQUEST.lock().take()
}

pub fn request_stop() -> u32 {
    PCM_LANE_REQUEST.lock().take();
    PCM_LANE_PAUSED.store(false, Ordering::Release);
    PCM_LANE_STOP_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1)
}

pub fn stop_generation() -> u32 {
    PCM_LANE_STOP_GENERATION.load(Ordering::Acquire)
}

pub fn set_paused(paused: bool) {
    PCM_LANE_PAUSED.store(paused, Ordering::Release);
}

pub fn paused() -> bool {
    PCM_LANE_PAUSED.load(Ordering::Acquire)
}

pub fn set_volume_percent(percent: u16) -> u16 {
    let clamped = percent.min(100);
    PCM_LANE_VOLUME_PERCENT.store(clamped, Ordering::Release);
    clamped
}

pub fn volume_percent() -> u16 {
    PCM_LANE_VOLUME_PERCENT.load(Ordering::Acquire)
}
