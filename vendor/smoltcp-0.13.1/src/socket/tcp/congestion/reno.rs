use crate::{socket::tcp::RttEstimator, time::Instant};

use super::Controller;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Reno {
    cwnd: usize,
    min_cwnd: usize,
    ssthresh: usize,
    rwnd: usize,
}

impl Reno {
    pub fn new() -> Self {
        Reno {
            cwnd: 1024 * 2,
            min_cwnd: 1024 * 2,
            ssthresh: usize::MAX,
            rwnd: 64 * 1024,
        }
    }
}

impl Controller for Reno {
    fn window(&self) -> usize {
        self.cwnd
    }

    fn on_ack(&mut self, _now: Instant, len: usize, _rtt: &RttEstimator) {
        let len = if self.cwnd < self.ssthresh {
            // Slow start.
            len
        } else {
            self.ssthresh = self.cwnd;
            self.min_cwnd
        };

        self.cwnd = self
            .cwnd
            .saturating_add(len)
            .min(self.rwnd)
            .max(self.min_cwnd);
    }

    fn on_duplicate_ack(&mut self, _now: Instant) {
        self.ssthresh = (self.cwnd >> 1).max(self.min_cwnd);
    }

    fn on_retransmit(&mut self, _now: Instant) {
        self.cwnd = (self.cwnd >> 1).max(self.min_cwnd);
    }

    fn set_mss(&mut self, mss: usize) {
        self.min_cwnd = mss;
    }

    fn set_remote_window(&mut self, remote_window: usize) {
        if self.rwnd < remote_window {
            self.rwnd = remote_window;
        }
    }
}
