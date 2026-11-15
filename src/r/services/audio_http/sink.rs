//! Bounded, single-source PCM admission. This module is kernel-internal.
use alloc::collections::VecDeque;

// Half a second of stereo 48 kHz PCM. Keep the most recent samples when a
// producer outruns playback; memory use never depends on the supplied length.
const CAPACITY: usize = 48_000;

pub(super) struct Sink {
    owner: Option<u64>,
    pub(super) samples: VecDeque<i16>,
}

impl Sink {
    pub(super) const fn new() -> Self {
        Self {
            owner: None,
            samples: VecDeque::new(),
        }
    }

    pub(super) fn submit(&mut self, source: u64, samples: &[i16]) -> usize {
        if samples.is_empty() || samples.len() % 2 != 0 {
            return 0;
        }
        if let Some(owner) = self.owner {
            if owner != source {
                return samples.len() / 2;
            }
        } else {
            self.owner = Some(source);
        }
        let keep = samples.len().min(CAPACITY);
        let drop = self
            .samples
            .len()
            .saturating_add(keep)
            .saturating_sub(CAPACITY);
        self.samples.drain(..drop);
        self.samples
            .extend(samples[samples.len() - keep..].iter().copied());
        samples.len() / 2
    }

    pub(super) fn finish(&mut self, source: u64) {
        if self.owner == Some(source) {
            self.owner = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_source_wins_and_competitors_succeed_without_being_queued() {
        let mut sink = Sink::new();
        assert_eq!(sink.submit(1, &[10, 11]), 1);
        assert_eq!(sink.submit(2, &[20, 21, 22, 23]), 2);
        assert_eq!(sink.submit(1, &[12, 13]), 1);
        assert_eq!(
            sink.samples.iter().copied().collect::<alloc::vec::Vec<_>>(),
            [10, 11, 12, 13]
        );
    }

    #[test]
    fn only_owner_can_release_and_the_next_source_can_play() {
        let mut sink = Sink::new();
        sink.submit(10, &[1, 2]);
        sink.finish(11);
        sink.submit(11, &[3, 4]);
        assert_eq!(sink.owner, Some(10));
        sink.finish(10);
        sink.submit(11, &[5, 6]);
        assert_eq!(sink.owner, Some(11));
        // Preserve the completed source's tail, then play the new source in
        // sequence. The rejected source's earlier samples never appear.
        assert_eq!(
            sink.samples.iter().copied().collect::<alloc::vec::Vec<_>>(),
            [1, 2, 5, 6]
        );
    }

    #[test]
    fn queue_is_bounded_and_drops_whole_stereo_frames() {
        let mut sink = Sink::new();
        sink.submit(1, &alloc::vec![1; CAPACITY]);
        sink.submit(1, &[2, 3]);
        assert_eq!(sink.samples.len(), CAPACITY);
        assert_eq!(sink.samples.pop_back(), Some(3));
        assert_eq!(sink.samples.pop_back(), Some(2));
        sink.submit(1, &alloc::vec![4; CAPACITY * 4]);
        assert_eq!(sink.samples.len(), CAPACITY);
        assert!(sink.samples.iter().all(|sample| *sample == 4));
    }

    #[test]
    fn empty_or_partial_frames_cannot_claim_ownership() {
        let mut sink = Sink::new();
        assert_eq!(sink.submit(1, &[]), 0);
        assert_eq!(sink.submit(1, &[42]), 0);
        assert_eq!(sink.owner, None);
        assert_eq!(sink.submit(2, &[1, 2]), 1);
        assert_eq!(sink.owner, Some(2));
    }
}
