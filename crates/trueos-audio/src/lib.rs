#![no_std]

pub struct Pcm<'a> {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub samples_interleaved_i16: &'a [i16],
}

impl<'a> Pcm<'a> {
    #[inline]
    pub const fn frames(&self) -> usize {
        let ch = self.channels as usize;
        if ch == 0 {
            return 0;
        }
        self.samples_interleaved_i16.len() / ch
    }
}
