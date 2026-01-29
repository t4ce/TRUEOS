pub const DEFAULT_RATE_HZ: u32 = 48_000;
pub const DEFAULT_CHANNELS: u16 = 2;

/// Simple PCM format descriptor for sinks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub rate_hz: u32,
    pub channels: u8,
    pub bits_per_sample: u8,
}
