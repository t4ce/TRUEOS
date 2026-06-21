extern crate alloc;

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmAdapterError {
    UnsupportedChannels,
    BadFrameAlignment,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PcmAdapterStats {
    pub source_frames: usize,
    pub sink_frames: usize,
    pub clipped_samples: usize,
}

#[derive(Clone, Debug)]
pub struct PcmSinkAdapter {
    source_channels: usize,
    source_sample_rate: u32,
    sink_sample_rate: u32,
    source_frame_phase: u64,
}

impl PcmSinkAdapter {
    pub fn new(
        source_channels: usize,
        source_sample_rate: u32,
        sink_sample_rate: u32,
    ) -> Result<Self, PcmAdapterError> {
        if source_channels == 0 || source_channels > 2 {
            return Err(PcmAdapterError::UnsupportedChannels);
        }
        Ok(Self {
            source_channels,
            source_sample_rate: source_sample_rate.max(1),
            sink_sample_rate: sink_sample_rate.max(1),
            source_frame_phase: 0,
        })
    }

    pub fn convert_packet(
        &mut self,
        interleaved_f32: &[f32],
        out: &mut Vec<i16>,
    ) -> Result<PcmAdapterStats, PcmAdapterError> {
        if interleaved_f32.len() % self.source_channels != 0 {
            return Err(PcmAdapterError::BadFrameAlignment);
        }
        let source_frames = interleaved_f32.len() / self.source_channels;
        if source_frames == 0 {
            return Ok(PcmAdapterStats::default());
        }

        let start_len = out.len();
        let mut clipped_samples = 0usize;
        while (self.source_frame_phase / self.sink_sample_rate as u64) < source_frames as u64 {
            let source_frame = (self.source_frame_phase / self.sink_sample_rate as u64) as usize;
            let base = source_frame * self.source_channels;
            let (left, right) = if self.source_channels == 1 {
                let sample = interleaved_f32[base];
                (sample, sample)
            } else {
                (interleaved_f32[base], interleaved_f32[base + 1])
            };
            let (left, left_clipped) = f32_to_i16(left);
            let (right, right_clipped) = f32_to_i16(right);
            clipped_samples = clipped_samples
                .saturating_add(left_clipped as usize)
                .saturating_add(right_clipped as usize);
            out.push(left);
            out.push(right);
            self.source_frame_phase = self
                .source_frame_phase
                .saturating_add(self.source_sample_rate as u64);
        }
        let consumed_phase = (source_frames as u64).saturating_mul(self.sink_sample_rate as u64);
        self.source_frame_phase = self.source_frame_phase.saturating_sub(consumed_phase);

        Ok(PcmAdapterStats {
            source_frames,
            sink_frames: (out.len().saturating_sub(start_len)) / 2,
            clipped_samples,
        })
    }
}

fn f32_to_i16(sample: f32) -> (i16, bool) {
    let clipped = !(-1.0..=1.0).contains(&sample);
    let sample = sample.clamp(-1.0, 1.0);
    let scaled = sample * 32767.0;
    let rounded = if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    };
    (rounded as i16, clipped)
}
