//! Accepted-word feedback reuses the mode legend's token dissolve.
use super::{MetaFmtStr, transition};
use alloc::vec::Vec;

const HOLD_NS: u64 = 75_000_000;

pub(super) struct Feedback {
    dissolve: transition::TokenSteps,
    width: usize,
    started: u64,
}

impl Feedback {
    pub fn new(word: &str, now: u64, from_right: bool) -> Self {
        let mut dissolve = transition::TokenSteps::default();
        dissolve.start(&[MetaFmtStr::new(word)], &[], now.saturating_add(HOLD_NS), from_right);
        Self { dissolve, width: word.chars().count(), started: now }
    }

    // Run accepted-word feedback at half speed without slowing mode legends.
    fn animation_time(&self, now: u64) -> u64 {
        self.started.saturating_add(now.saturating_sub(self.started) / 2)
    }

    pub fn finished(&self, now: u64) -> bool { self.dissolve.frame(self.animation_time(now)).is_none() }

    pub fn runs(&self, columns: usize, now: u64) -> Vec<MetaFmtStr> {
        let Some(frame) = self.dissolve.frame(self.animation_time(now)) else { return Vec::new(); };
        let mut runs = alloc::vec![MetaFmtStr::new(" ".repeat(columns.saturating_sub(self.width) / 2))];
        runs.extend(frame.display());
        runs
    }
}
