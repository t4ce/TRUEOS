use crate::time::Instant;

use super::Controller;

// Constants for the Cubic congestion control algorithm.
// See RFC 8312.
const BETA_CUBIC: f64 = 0.7;
const C: f64 = 0.4;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Cubic {
    cwnd: usize,     // Congestion window
    min_cwnd: usize, // The minimum size of congestion window
    w_max: usize,    // Window size just before congestion
    recovery_start: Option<Instant>,
    rwnd: usize, // Remote window
    last_update: Instant,
    ssthresh: usize,
}

impl Cubic {
    pub fn new() -> Cubic {
        Cubic {
            cwnd: 1024 * 2,
            min_cwnd: 1024 * 2,
            w_max: 1024 * 2,
            recovery_start: None,
            rwnd: 64 * 1024,
            last_update: Instant::from_millis(0),
            ssthresh: usize::MAX,
        }
    }
}

impl Controller for Cubic {
    fn window(&self) -> usize {
        self.cwnd
    }

    fn on_retransmit(&mut self, now: Instant) {
        self.w_max = self.cwnd;
        self.ssthresh = self.cwnd >> 1;
        self.recovery_start = Some(now);
    }

    fn on_duplicate_ack(&mut self, now: Instant) {
        self.w_max = self.cwnd;
        self.ssthresh = self.cwnd >> 1;
        self.recovery_start = Some(now);
    }

    fn set_remote_window(&mut self, remote_window: usize) {
        if self.rwnd < remote_window {
            self.rwnd = remote_window;
        }
    }

    fn on_ack(&mut self, _now: Instant, len: usize, _rtt: &crate::socket::tcp::RttEstimator) {
        // Slow start.
        if self.cwnd < self.ssthresh {
            self.cwnd = self
                .cwnd
                .saturating_add(len)
                .min(self.rwnd)
                .max(self.min_cwnd);
        }
    }

    fn pre_transmit(&mut self, now: Instant) {
        let Some(recovery_start) = self.recovery_start else {
            self.recovery_start = Some(now);
            return;
        };

        let now_millis = now.total_millis();

        // If the last update was less than 100ms ago, don't update the congestion window.
        if self.last_update > recovery_start && now_millis - self.last_update.total_millis() < 100 {
            return;
        }

        // Elapsed time since the start of the recovery phase.
        let t = now_millis - recovery_start.total_millis();
        if t < 0 {
            return;
        }

        // K = (w_max * (1 - beta) / C)^(1/3)
        let k3 = ((self.w_max as f64) * (1.0 - BETA_CUBIC)) / C;
        let k = if let Some(k) = cube_root(k3) {
            k
        } else {
            return;
        };

        // cwnd = C(T - K)^3 + w_max
        let s = t as f64 / 1000.0 - k;
        let s = s * s * s;
        let cwnd = C * s + self.w_max as f64;

        self.last_update = now;

        self.cwnd = (cwnd as usize).max(self.min_cwnd).min(self.rwnd);
    }

    fn set_mss(&mut self, mss: usize) {
        self.min_cwnd = mss;
    }
}

#[inline]
fn abs(a: f64) -> f64 {
    if a < 0.0 { -a } else { a }
}

/// Calculate cube root by using the Newton-Raphson method.
fn cube_root(a: f64) -> Option<f64> {
    if a <= 0.0 {
        return None;
    }

    let (tolerance, init) = if a < 1_000.0 {
        (1.0, 8.879040017426005) // cube_root(700.0)
    } else if a < 1_000_000.0 {
        (5.0, 88.79040017426004) // cube_root(700_000.0)
    } else if a < 1_000_000_000.0 {
        (50.0, 887.9040017426004) // cube_root(700_000_000.0)
    } else if a < 1_000_000_000_000.0 {
        (500.0, 8879.040017426003) // cube_root(700_000_000_000.0)
    } else if a < 1_000_000_000_000_000.0 {
        (5000.0, 88790.40017426001) // cube_root(700_000_000_000.0)
    } else {
        (50000.0, 887904.0017426) // cube_root(700_000_000_000_000.0)
    };

    let mut x = init; // initial value
    let mut n = 20; // The maximum iteration
    loop {
        let next_x = (2.0 * x + a / (x * x)) / 3.0;
        if abs(next_x - x) < tolerance {
            return Some(next_x);
        }
        x = next_x;

        if n == 0 {
            return Some(next_x);
        }

        n -= 1;
    }
}
