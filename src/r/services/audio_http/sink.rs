//! One complete, bounded PCM clip at a time. Competing clips succeed/discard.
use alloc::{sync::Arc, vec::Vec};

/// Two minutes of stereo signed 16-bit little-endian PCM at 48 kHz.
pub(super) const MAX_CLIP_BYTES: usize = 48_000 * 4 * 120;

pub(super) fn validate_size(bytes: usize) -> Result<(), i32> {
    if bytes == 0 || bytes % 4 != 0 {
        Err(-22)
    } else if bytes > MAX_CLIP_BYTES {
        Err(-27)
    } else {
        Ok(())
    }
}

pub(super) struct Sink {
    clip: Option<Clip>,
    offset: usize,
    next_id: u64,
}

#[derive(Clone)]
pub(super) struct Clip {
    pub id: u64,
    pub pcm: Arc<[u8]>,
}

impl Sink {
    pub(super) const fn new() -> Self {
        Self {
            clip: None,
            offset: 0,
            next_id: 1,
        }
    }

    /// Copy a completed clip once. Nothing is retained from competing sources,
    /// and the caller can release its allocation as soon as this returns.
    pub(super) fn submit(&mut self, bytes: &[u8]) -> Result<(), i32> {
        validate_size(bytes.len())?;
        if self.busy() {
            return Ok(());
        }
        let mut clip = Vec::new();
        clip.try_reserve_exact(bytes.len()).map_err(|_| -12)?;
        clip.extend_from_slice(bytes);
        let next_id = self.next_id.checked_add(1).ok_or(-75)?;
        self.clip = Some(Clip { id: self.next_id, pcm: clip.into() });
        self.next_id = next_id;
        self.offset = 0;
        Ok(())
    }

    pub(super) fn busy(&self) -> bool {
        self.clip.as_ref().is_some_and(|clip| self.offset < clip.pcm.len())
    }

    /// Retain only the latest accepted clip, including its complete prefix.
    /// HTTP downloads own an Arc so a later accepted clip cannot truncate them.
    pub(super) fn snapshot(&self) -> Option<Clip> { self.clip.clone() }

    /// Drain complete frames at the HTTP clock's pace. The final sample releases
    /// ownership automatically; silence fills the rest of this playback block.
    pub(super) fn render(&mut self, out: &mut [u8]) {
        assert_eq!(out.len() % 4, 0);
        out.fill(0);
        let Some(clip) = &self.clip else { return; };
        let count = out.len().min(clip.pcm.len().saturating_sub(self.offset));
        out[..count].copy_from_slice(&clip.pcm[self.offset..self.offset + count]);
        self.offset += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_clip_wins_and_competitors_succeed_without_being_queued() {
        let mut sink = Sink::new();
        sink.submit(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(sink.submit(&[9, 10, 11, 12]), Ok(()));
        let mut block = [0; 4];
        sink.render(&mut block);
        assert_eq!(block, [1, 2, 3, 4]);
        sink.render(&mut block);
        assert_eq!(block, [5, 6, 7, 8]);
        sink.render(&mut block);
        assert_eq!(block, [0; 4]);
    }

    #[test]
    fn last_sample_releases_ownership_and_caller_memory_is_not_borrowed() {
        let mut sink = Sink::new();
        let mut input = alloc::vec![1, 2, 3, 4];
        sink.submit(&input).unwrap();
        input.fill(99);
        drop(input);
        let mut block = [0; 8];
        sink.render(&mut block);
        assert_eq!(block, [1, 2, 3, 4, 0, 0, 0, 0]);
        assert!(!sink.busy());
        sink.submit(&[5, 6, 7, 8]).unwrap();
        sink.render(&mut block);
        assert_eq!(&block[..4], &[5, 6, 7, 8]);
    }

    #[test]
    fn completed_utterance_longer_than_the_old_half_second_buffer_is_lossless() {
        let mut sink = Sink::new();
        let bytes: Vec<u8> = (0..48_000 * 4 * 3).map(|n| (n % 251) as u8).collect();
        sink.submit(&bytes).unwrap();
        let mut actual = Vec::new();
        for _ in 0..300 {
            let mut block = [0; 1920];
            sink.render(&mut block);
            actual.extend_from_slice(&block);
        }
        assert_eq!(actual, bytes);
        assert!(!sink.busy());
    }

    #[test]
    fn invalid_or_oversized_clips_never_claim_the_sink() {
        let mut sink = Sink::new();
        assert_eq!(sink.submit(&[]), Err(-22));
        assert_eq!(sink.submit(&[1, 2]), Err(-22));
        assert_eq!(validate_size(MAX_CLIP_BYTES), Ok(()));
        assert_eq!(validate_size(MAX_CLIP_BYTES + 4), Err(-27));
        assert!(sink.snapshot().is_none());
    }
}
