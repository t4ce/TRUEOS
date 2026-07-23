//! Allocation-free monitor of the exact stereo samples accepted by HDA.
//!
//! The producer is the serialized HDA streaming path. Readers never consume
//! samples and never wait for the speaker path: each sample is copied into an
//! atomic ring, then one release store publishes the completed frame range.

use core::sync::atomic::{AtomicBool, AtomicI16, AtomicU32, AtomicU64, Ordering};

pub(crate) const AUDIO_VISUALIZER_TAP_FRAMES: usize = 4_096;
const CHANNELS: usize = 2;
const AUDIO_VISUALIZER_TAP_SAMPLES: usize = AUDIO_VISUALIZER_TAP_FRAMES * CHANNELS;

static ENABLED: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU32 = AtomicU32::new(0);
static WRITE_FRAME_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static SAMPLES: [AtomicI16; AUDIO_VISUALIZER_TAP_SAMPLES] =
    [const { AtomicI16::new(0) }; AUDIO_VISUALIZER_TAP_SAMPLES];

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AudioVisualizerTapSnapshot {
    pub(crate) enabled: bool,
    pub(crate) consistent: bool,
    pub(crate) generation: u32,
    pub(crate) frames: usize,
    pub(crate) end_frame_sequence: u64,
}

pub(crate) fn set_enabled(enabled: bool) {
    if ENABLED.swap(enabled, Ordering::AcqRel) != enabled {
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Publish one accepted HDA buffer without allocating, locking, or modifying
/// the caller's speaker samples.
pub(crate) fn publish_i16_stereo_48k(samples: &[i16]) {
    if !enabled() || samples.is_empty() || !samples.len().is_multiple_of(CHANNELS) {
        return;
    }

    let frame_count = samples.len() / CHANNELS;
    let start_frame = WRITE_FRAME_SEQUENCE.load(Ordering::Relaxed);
    for frame in 0..frame_count {
        let ring_frame =
            start_frame.wrapping_add(frame as u64) as usize % AUDIO_VISUALIZER_TAP_FRAMES;
        let ring_sample = ring_frame * CHANNELS;
        let source_sample = frame * CHANNELS;
        SAMPLES[ring_sample].store(samples[source_sample], Ordering::Relaxed);
        SAMPLES[ring_sample + 1].store(samples[source_sample + 1], Ordering::Relaxed);
    }
    WRITE_FRAME_SEQUENCE.store(start_frame.wrapping_add(frame_count as u64), Ordering::Release);
}

/// Copy the newest complete stereo frames into the tail of `out`.
///
/// `out` is cleared first, allowing startup snapshots shorter than the
/// requested FFT window to remain correctly right-aligned. Two bounded reads
/// are enough: the 4096-frame ring leaves more than one callback of overwrite
/// runway around a 2048-frame analysis window.
pub(crate) fn snapshot_latest_i16_stereo_48k(out: &mut [i16]) -> AudioVisualizerTapSnapshot {
    out.fill(0);
    if out.is_empty() || !out.len().is_multiple_of(CHANNELS) {
        return AudioVisualizerTapSnapshot::default();
    }

    let requested_frames = (out.len() / CHANNELS).min(AUDIO_VISUALIZER_TAP_FRAMES);
    let generation = GENERATION.load(Ordering::Acquire);
    let mut last = AudioVisualizerTapSnapshot {
        enabled: enabled(),
        consistent: false,
        generation,
        frames: 0,
        end_frame_sequence: WRITE_FRAME_SEQUENCE.load(Ordering::Acquire),
    };

    for _ in 0..2 {
        let end = WRITE_FRAME_SEQUENCE.load(Ordering::Acquire);
        let available = end.min(AUDIO_VISUALIZER_TAP_FRAMES as u64) as usize;
        let frames = requested_frames.min(available);
        let start = end.saturating_sub(frames as u64);
        let destination_frame = requested_frames - frames;

        for frame in 0..frames {
            let ring_frame =
                start.wrapping_add(frame as u64) as usize % AUDIO_VISUALIZER_TAP_FRAMES;
            let ring_sample = ring_frame * CHANNELS;
            let destination_sample = (destination_frame + frame) * CHANNELS;
            out[destination_sample] = SAMPLES[ring_sample].load(Ordering::Relaxed);
            out[destination_sample + 1] = SAMPLES[ring_sample + 1].load(Ordering::Relaxed);
        }

        let confirmed_end = WRITE_FRAME_SEQUENCE.load(Ordering::Acquire);
        last = AudioVisualizerTapSnapshot {
            enabled: enabled(),
            consistent: end == confirmed_end,
            generation: GENERATION.load(Ordering::Acquire),
            frames,
            end_frame_sequence: confirmed_end,
        };
        if last.consistent {
            return last;
        }
    }

    last
}

pub(crate) fn status() -> AudioVisualizerTapSnapshot {
    let end = WRITE_FRAME_SEQUENCE.load(Ordering::Acquire);
    AudioVisualizerTapSnapshot {
        enabled: enabled(),
        consistent: true,
        generation: GENERATION.load(Ordering::Acquire),
        frames: end.min(AUDIO_VISUALIZER_TAP_FRAMES as u64) as usize,
        end_frame_sequence: end,
    }
}
