//! Bounded live-audio analysis for the single C++ audiovisual shader.
//!
//! Analysis is deliberately outside the HDA callback. The speaker-side tee is
//! atomic and allocation-free; this module snapshots 2048 stereo frames,
//! performs ordinary mid/side FFTs, and publishes compact visual features.

use alloc::{vec, vec::Vec};
use spin::Mutex;
use symphonia_core::dsp::{complex::Complex, fft::Fft};

use super::audio_visualizer_tap;

pub(crate) const AUDIO_VISUALIZER_SAMPLE_RATE: u32 = 48_000;
pub(crate) const AUDIO_VISUALIZER_FFT_FRAMES: usize = 2_048;
pub(crate) const AUDIO_VISUALIZER_WAVEFORM_COUNT: usize = 128;
pub(crate) const AUDIO_VISUALIZER_SPECTRUM_COUNT: usize = 64;

const CHANNELS: usize = 2;
const PCM_SAMPLE_COUNT: usize = AUDIO_VISUALIZER_FFT_FRAMES * CHANNELS;
const FFT_BIN_COUNT: usize = AUDIO_VISUALIZER_FFT_FRAMES / 2;
const MIN_FREQUENCY_HZ: f32 = 35.0;
const MAX_FREQUENCY_HZ: f32 = 18_000.0;
const INPUT_SCALE: f32 = 1.0 / 32_768.0;
const FFT_MAGNITUDE_SCALE: f32 = 2.0 / AUDIO_VISUALIZER_FFT_FRAMES as f32;

#[derive(Clone, Debug)]
pub(crate) struct AudioVisualizerFrame {
    pub(crate) sequence: u64,
    pub(crate) active: bool,
    pub(crate) rms_left: f32,
    pub(crate) rms_right: f32,
    pub(crate) peak: f32,
    pub(crate) stereo_width: f32,
    pub(crate) low: f32,
    pub(crate) mid: f32,
    pub(crate) high: f32,
    pub(crate) beat: f32,
    pub(crate) centroid: f32,
    pub(crate) flux: f32,
    pub(crate) tempo_phase: f32,
    pub(crate) signal: f32,
    pub(crate) waveform_left: [f32; AUDIO_VISUALIZER_WAVEFORM_COUNT],
    pub(crate) waveform_right: [f32; AUDIO_VISUALIZER_WAVEFORM_COUNT],
    pub(crate) spectrum: [f32; AUDIO_VISUALIZER_SPECTRUM_COUNT],
}

impl AudioVisualizerFrame {
    const fn silent() -> Self {
        Self {
            sequence: 0,
            active: false,
            rms_left: 0.0,
            rms_right: 0.0,
            peak: 0.0,
            stereo_width: 0.0,
            low: 0.0,
            mid: 0.0,
            high: 0.0,
            beat: 0.0,
            centroid: 0.0,
            flux: 0.0,
            tempo_phase: 0.0,
            signal: 0.0,
            waveform_left: [0.0; AUDIO_VISUALIZER_WAVEFORM_COUNT],
            waveform_right: [0.0; AUDIO_VISUALIZER_WAVEFORM_COUNT],
            spectrum: [0.0; AUDIO_VISUALIZER_SPECTRUM_COUNT],
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct AudioVisualizerStatus {
    pub(crate) enabled: bool,
    pub(crate) sequence: u64,
    pub(crate) captured_frames: usize,
    pub(crate) active: bool,
    pub(crate) rms: f32,
    pub(crate) peak: f32,
    pub(crate) low: f32,
    pub(crate) mid: f32,
    pub(crate) high: f32,
    pub(crate) beat: f32,
}

struct AudioAnalyzer {
    fft: Fft,
    pcm: Vec<i16>,
    mid_fft: Vec<Complex>,
    side_fft: Vec<Complex>,
    window: Vec<f32>,
    band_start: [usize; AUDIO_VISUALIZER_SPECTRUM_COUNT],
    band_end: [usize; AUDIO_VISUALIZER_SPECTRUM_COUNT],
    previous_raw: [f32; AUDIO_VISUALIZER_SPECTRUM_COUNT],
    smoothed: [f32; AUDIO_VISUALIZER_SPECTRUM_COUNT],
    average_low: f32,
    beat: f32,
    tempo_phase: f32,
    last_generation: u32,
    last_end_frame: u64,
    last_frame: AudioVisualizerFrame,
}

impl AudioAnalyzer {
    fn new() -> Self {
        let mut window = vec![0.0; AUDIO_VISUALIZER_FFT_FRAMES];
        for (index, value) in window.iter_mut().enumerate() {
            let phase =
                core::f32::consts::TAU * index as f32 / (AUDIO_VISUALIZER_FFT_FRAMES - 1) as f32;
            *value = 0.5 - 0.5 * libm::cosf(phase);
        }

        let mut band_start = [1usize; AUDIO_VISUALIZER_SPECTRUM_COUNT];
        let mut band_end = [2usize; AUDIO_VISUALIZER_SPECTRUM_COUNT];
        let frequency_ratio = MAX_FREQUENCY_HZ / MIN_FREQUENCY_HZ;
        for band in 0..AUDIO_VISUALIZER_SPECTRUM_COUNT {
            let start_frequency = MIN_FREQUENCY_HZ
                * libm::powf(frequency_ratio, band as f32 / AUDIO_VISUALIZER_SPECTRUM_COUNT as f32);
            let end_frequency = MIN_FREQUENCY_HZ
                * libm::powf(
                    frequency_ratio,
                    (band + 1) as f32 / AUDIO_VISUALIZER_SPECTRUM_COUNT as f32,
                );
            let start = (start_frequency * AUDIO_VISUALIZER_FFT_FRAMES as f32
                / AUDIO_VISUALIZER_SAMPLE_RATE as f32) as usize;
            let end = libm::ceilf(
                end_frequency * AUDIO_VISUALIZER_FFT_FRAMES as f32
                    / AUDIO_VISUALIZER_SAMPLE_RATE as f32,
            ) as usize;
            band_start[band] = start.clamp(1, FFT_BIN_COUNT - 1);
            band_end[band] = end.clamp(band_start[band] + 1, FFT_BIN_COUNT);
        }

        Self {
            fft: Fft::new(AUDIO_VISUALIZER_FFT_FRAMES),
            pcm: vec![0; PCM_SAMPLE_COUNT],
            mid_fft: vec![Complex::default(); AUDIO_VISUALIZER_FFT_FRAMES],
            side_fft: vec![Complex::default(); AUDIO_VISUALIZER_FFT_FRAMES],
            window,
            band_start,
            band_end,
            previous_raw: [0.0; AUDIO_VISUALIZER_SPECTRUM_COUNT],
            smoothed: [0.0; AUDIO_VISUALIZER_SPECTRUM_COUNT],
            average_low: 0.0,
            beat: 0.0,
            tempo_phase: 0.0,
            last_generation: 0,
            last_end_frame: 0,
            last_frame: AudioVisualizerFrame::silent(),
        }
    }

    fn analyze(&mut self) -> AudioVisualizerFrame {
        let tap = audio_visualizer_tap::snapshot_latest_i16_stereo_48k(self.pcm.as_mut_slice());
        if !tap.enabled || !tap.consistent || tap.frames == 0 {
            self.decay_silence();
            return self.last_frame.clone();
        }
        if tap.generation == self.last_generation && tap.end_frame_sequence == self.last_end_frame {
            return self.last_frame.clone();
        }

        let mut left_square = 0.0f32;
        let mut right_square = 0.0f32;
        let mut mid_square = 0.0f32;
        let mut side_square = 0.0f32;
        let mut peak = 0.0f32;
        for frame in 0..AUDIO_VISUALIZER_FFT_FRAMES {
            let left = self.pcm[frame * CHANNELS] as f32 * INPUT_SCALE;
            let right = self.pcm[frame * CHANNELS + 1] as f32 * INPUT_SCALE;
            let mid = (left + right) * 0.5;
            let side = (left - right) * 0.5;
            let window = self.window[frame];
            self.mid_fft[frame] = Complex::new(mid * window, 0.0);
            self.side_fft[frame] = Complex::new(side * window, 0.0);
            left_square += left * left;
            right_square += right * right;
            mid_square += mid * mid;
            side_square += side * side;
            peak = peak.max(left.abs()).max(right.abs());
        }
        self.fft.fft_inplace(self.mid_fft.as_mut_slice());
        self.fft.fft_inplace(self.side_fft.as_mut_slice());

        let mut raw = [0.0f32; AUDIO_VISUALIZER_SPECTRUM_COUNT];
        let mut flux = 0.0f32;
        let mut weighted = 0.0f32;
        let mut total = 0.0f32;
        for band in 0..AUDIO_VISUALIZER_SPECTRUM_COUNT {
            let mut peak_magnitude = 0.0f32;
            let mut sum_magnitude = 0.0f32;
            let mut bins = 0usize;
            for bin in self.band_start[band]..self.band_end[band] {
                let mid = self.mid_fft[bin];
                let side = self.side_fft[bin];
                let mid_magnitude =
                    libm::sqrtf(mid.re * mid.re + mid.im * mid.im) * FFT_MAGNITUDE_SCALE;
                let side_magnitude =
                    libm::sqrtf(side.re * side.re + side.im * side.im) * FFT_MAGNITUDE_SCALE;
                let magnitude = mid_magnitude + side_magnitude * 0.32;
                peak_magnitude = peak_magnitude.max(magnitude);
                sum_magnitude += magnitude;
                bins += 1;
            }
            let average = sum_magnitude / bins.max(1) as f32;
            let magnitude = peak_magnitude * 0.72 + average * 0.28;
            let compressed = libm::log1pf(magnitude * 18.0) * (1.0 / libm::log1pf(18.0));
            raw[band] = compressed.clamp(0.0, 1.0);
            flux += (raw[band] - self.previous_raw[band]).max(0.0);
            self.previous_raw[band] = raw[band];
            let smoothing = if raw[band] > self.smoothed[band] {
                0.62
            } else {
                0.13
            };
            self.smoothed[band] += (raw[band] - self.smoothed[band]) * smoothing;
            weighted += self.smoothed[band] * band as f32;
            total += self.smoothed[band];
        }
        flux = (flux / AUDIO_VISUALIZER_SPECTRUM_COUNT as f32 * 3.2).clamp(0.0, 1.0);

        let low = average(&self.smoothed[0..14]);
        let mid = average(&self.smoothed[14..42]);
        let high = average(&self.smoothed[42..AUDIO_VISUALIZER_SPECTRUM_COUNT]);
        if self.average_low == 0.0 {
            self.average_low = low;
        } else {
            self.average_low += (low - self.average_low) * 0.045;
        }
        let onset = ((low - self.average_low * 1.16) * 5.0 + flux * 0.32).clamp(0.0, 1.0);
        self.beat = (self.beat * 0.78).max(onset);

        let advanced_frames = if tap.generation == self.last_generation {
            tap.end_frame_sequence.saturating_sub(self.last_end_frame)
        } else {
            0
        };
        if onset > 0.18 {
            self.tempo_phase = 0.0;
        } else {
            self.tempo_phase +=
                advanced_frames as f32 / (AUDIO_VISUALIZER_SAMPLE_RATE as f32 * 0.58);
            self.tempo_phase -= libm::floorf(self.tempo_phase);
        }

        let frame_denominator = AUDIO_VISUALIZER_FFT_FRAMES as f32;
        let rms_left = libm::sqrtf(left_square / frame_denominator).clamp(0.0, 1.0);
        let rms_right = libm::sqrtf(right_square / frame_denominator).clamp(0.0, 1.0);
        let mid_rms = libm::sqrtf(mid_square / frame_denominator);
        let side_rms = libm::sqrtf(side_square / frame_denominator);
        let stereo_width = (side_rms / (mid_rms + side_rms + 0.000_01)).clamp(0.0, 1.0);
        let signal = (((rms_left + rms_right) * 0.5 - 0.0015) * 24.0).clamp(0.0, 1.0);
        let centroid = if total > 0.000_01 {
            (weighted / total / (AUDIO_VISUALIZER_SPECTRUM_COUNT - 1) as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let mut frame = AudioVisualizerFrame {
            sequence: tap.end_frame_sequence,
            active: signal > 0.01,
            rms_left: (rms_left * 3.0).clamp(0.0, 1.0),
            rms_right: (rms_right * 3.0).clamp(0.0, 1.0),
            peak,
            stereo_width,
            low,
            mid,
            high,
            beat: self.beat,
            centroid,
            flux,
            tempo_phase: self.tempo_phase,
            signal,
            waveform_left: [0.0; AUDIO_VISUALIZER_WAVEFORM_COUNT],
            waveform_right: [0.0; AUDIO_VISUALIZER_WAVEFORM_COUNT],
            spectrum: self.smoothed,
        };
        for index in 0..AUDIO_VISUALIZER_WAVEFORM_COUNT {
            let source_frame =
                index * (AUDIO_VISUALIZER_FFT_FRAMES - 1) / (AUDIO_VISUALIZER_WAVEFORM_COUNT - 1);
            frame.waveform_left[index] = self.pcm[source_frame * CHANNELS] as f32 * INPUT_SCALE;
            frame.waveform_right[index] =
                self.pcm[source_frame * CHANNELS + 1] as f32 * INPUT_SCALE;
        }

        self.last_generation = tap.generation;
        self.last_end_frame = tap.end_frame_sequence;
        self.last_frame = frame.clone();
        frame
    }

    fn decay_silence(&mut self) {
        self.beat *= 0.78;
        self.last_frame.beat = self.beat;
        self.last_frame.signal *= 0.86;
        self.last_frame.active = false;
        for band in &mut self.smoothed {
            *band *= 0.92;
        }
        self.last_frame.spectrum = self.smoothed;
    }
}

fn average(values: &[f32]) -> f32 {
    values.iter().copied().sum::<f32>() / values.len().max(1) as f32
}

static ANALYZER: Mutex<Option<AudioAnalyzer>> = Mutex::new(None);

pub(crate) fn set_enabled(enabled: bool) {
    audio_visualizer_tap::set_enabled(enabled);
    let mut analyzer = ANALYZER.lock();
    if enabled && analyzer.is_none() {
        *analyzer = Some(AudioAnalyzer::new());
    }
}

pub(crate) fn snapshot() -> AudioVisualizerFrame {
    let mut analyzer = ANALYZER.lock();
    analyzer.get_or_insert_with(AudioAnalyzer::new).analyze()
}

pub(crate) fn status() -> AudioVisualizerStatus {
    let tap = audio_visualizer_tap::status();
    let analyzer = ANALYZER.lock();
    let frame = analyzer.as_ref().map(|analyzer| &analyzer.last_frame);
    AudioVisualizerStatus {
        enabled: tap.enabled,
        sequence: tap.end_frame_sequence,
        captured_frames: tap.frames,
        active: frame.is_some_and(|frame| frame.active),
        rms: frame.map_or(0.0, |frame| (frame.rms_left + frame.rms_right) * 0.5),
        peak: frame.map_or(0.0, |frame| frame.peak),
        low: frame.map_or(0.0, |frame| frame.low),
        mid: frame.map_or(0.0, |frame| frame.mid),
        high: frame.map_or(0.0, |frame| frame.high),
        beat: frame.map_or(0.0, |frame| frame.beat),
    }
}
