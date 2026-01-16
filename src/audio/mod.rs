#![allow(dead_code)]

pub mod demo_player;

pub const DEMO_RATE_HZ: u32 = 48_000;
pub const DEMO_CHANNELS: u16 = 2;

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
