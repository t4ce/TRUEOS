use alloc::vec::Vec;
use spin::Mutex;

use crate::hda;

static PCM_LANE_REQUEST: Mutex<Option<PcmLaneRequest>> = Mutex::new(None);

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
