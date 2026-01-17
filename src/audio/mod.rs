#![allow(dead_code)]

pub mod demo_player;
pub mod demo {
    include!(concat!(env!("OUT_DIR"), "/demo_mp3.rs"));
}

pub struct DemoPcm {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub samples_interleaved_i16: &'static [i16],
}

pub const DEMO_RATE_HZ: u32 = demo::DEMO.sample_rate_hz;
pub const DEMO_CHANNELS: u16 = demo::DEMO.channels;

/// Simple PCM format descriptor for sinks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub rate_hz: u32,
    pub channels: u8,
    pub bits_per_sample: u8,
}

/// An abstract sink for interleaved PCM bytes.
pub trait PcmSink {
    /// Attempt to write PCM bytes; return bytes accepted.
    fn write(&mut self, pcm: &[u8]) -> usize;
}

/// A no-op sink useful as a placeholder.
pub struct NullSink;

impl PcmSink for NullSink {
    fn write(&mut self, pcm: &[u8]) -> usize {
        pcm.len()
    }
}
