use alloc::{collections::VecDeque, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use spin::Mutex;

use crate::hda;

const PCM_LANE_MAX_QUEUED_FRAMES: usize = hda::PCM_SAMPLE_RATE_HZ as usize;
const PCM_LANE_LOG_SAMPLE_EVERY: u32 = 1_000;

static PCM_LANE_REQUESTS: Mutex<VecDeque<PcmLaneRequest>> = Mutex::new(VecDeque::new());
static PCM_LANE_PAUSED: AtomicBool = AtomicBool::new(false);
static PCM_LANE_VOLUME_PERCENT: AtomicU16 = AtomicU16::new(100);
static PCM_LANE_STOP_GENERATION: AtomicU32 = AtomicU32::new(0);
static PCM_LANE_QUEUE_LOG_SEQ: AtomicU32 = AtomicU32::new(0);

fn sampled_queue_log() -> bool {
    PCM_LANE_QUEUE_LOG_SEQ.fetch_add(1, Ordering::Relaxed) % PCM_LANE_LOG_SAMPLE_EVERY == 0
}

pub struct PcmLaneRequest {
    pub label: &'static str,
    pub samples: Vec<i16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PcmLaneError {
    EmptyBuffer,
    BadShape,
    QueueFull,
}

pub fn submit_i16_stereo_48k(
    label: &'static str,
    samples: Vec<i16>,
) -> Result<usize, PcmLaneError> {
    if samples.is_empty() {
        return Err(PcmLaneError::EmptyBuffer);
    }
    if samples.len() % hda::PCM_CHANNELS != 0 {
        return Err(PcmLaneError::BadShape);
    }

    let sample_count = samples.len();
    let frames = sample_count / hda::PCM_CHANNELS;
    let mut requests = PCM_LANE_REQUESTS.lock();
    let queued_frames = requests
        .iter()
        .map(|request| request.samples.len() / hda::PCM_CHANNELS)
        .sum::<usize>();
    if queued_frames.saturating_add(frames) > PCM_LANE_MAX_QUEUED_FRAMES {
        if sampled_queue_log() {
            crate::log_warn!(
                target: "audio";
                "pcm-lane: queue-full label={} samples={} frames={} pending_frames={} cap_frames={}\n",
                label,
                sample_count,
                frames,
                queued_frames,
                PCM_LANE_MAX_QUEUED_FRAMES
            );
            crate::audio_probe!(
                "pcm-lane: queue-full label={} samples={} frames={} pending_frames={} cap_frames={}\n",
                label,
                sample_count,
                frames,
                queued_frames,
                PCM_LANE_MAX_QUEUED_FRAMES
            );
        }
        return Err(PcmLaneError::QueueFull);
    }

    requests.push_back(PcmLaneRequest { label, samples });
    if sampled_queue_log() {
        crate::log_info!(
            target: "audio";
            "pcm-lane: queued label={} samples={} frames={} pending_frames={} format=s16le/stereo/48k\n",
            label,
            sample_count,
            frames,
            queued_frames.saturating_add(frames)
        );
        crate::audio_probe!(
            "pcm-lane: queued label={} samples={} frames={} pending_frames={} format=s16le/stereo/48k\n",
            label,
            sample_count,
            frames,
            queued_frames.saturating_add(frames)
        );
    }
    Ok(frames)
}

pub fn urgent_pending() -> bool {
    !PCM_LANE_REQUESTS.lock().is_empty()
}

pub fn pending_frames() -> usize {
    PCM_LANE_REQUESTS
        .lock()
        .iter()
        .map(|request| request.samples.len() / hda::PCM_CHANNELS)
        .sum()
}

pub fn take_pending() -> Option<PcmLaneRequest> {
    PCM_LANE_REQUESTS.lock().pop_front()
}

pub fn request_stop() -> u32 {
    let cleared_frames = {
        let mut requests = PCM_LANE_REQUESTS.lock();
        let frames = requests
            .iter()
            .map(|request| request.samples.len() / hda::PCM_CHANNELS)
            .sum::<usize>();
        requests.clear();
        frames
    };
    PCM_LANE_PAUSED.store(false, Ordering::Release);
    let generation = PCM_LANE_STOP_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    crate::log_info!(
        target: "audio";
        "pcm-lane: stop generation={} cleared_frames={}\n",
        generation,
        cleared_frames
    );
    crate::audio_probe!(
        "pcm-lane: stop generation={} cleared_frames={}\n",
        generation,
        cleared_frames
    );
    generation
}

pub fn stop_generation() -> u32 {
    PCM_LANE_STOP_GENERATION.load(Ordering::Acquire)
}

pub fn set_paused(paused: bool) {
    PCM_LANE_PAUSED.store(paused, Ordering::Release);
    crate::log_info!(target: "audio"; "pcm-lane: paused={}\n", paused);
}

pub fn paused() -> bool {
    PCM_LANE_PAUSED.load(Ordering::Acquire)
}

pub fn set_volume_percent(percent: u16) -> u16 {
    let clamped = percent.min(100);
    PCM_LANE_VOLUME_PERCENT.store(clamped, Ordering::Release);
    crate::log_info!(
        target: "audio";
        "pcm-lane: volume percent={} applied={}\n",
        percent,
        clamped
    );
    clamped
}

pub fn volume_percent() -> u16 {
    PCM_LANE_VOLUME_PERCENT.load(Ordering::Acquire)
}
