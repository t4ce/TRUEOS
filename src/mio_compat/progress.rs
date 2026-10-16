//! Numeric, VM-run-scoped TCP progress. Callers already hold MIO_COMPAT.
//! Bytes describe the raw TCP stream, including TLS/HTTP framing, not HTTP data.

#[derive(Clone, Copy, Default)]
pub(super) struct Counters {
    pub generation: u64,
    pub observed: bool,
    pub rx_bytes: u64,
    pub read_bytes: u64,
    pub peek_bytes: u64,
    pub read_calls: u64,
    pub peek_calls: u64,
    pub would_block: u64,
    pub ready_probes: u64,
    pub readable_hits: u64,
    /// Largest single TCP socket receive queue observed on event admission.
    pub socket_queue_peak: u64,
}

impl Counters {
    pub const fn new() -> Self {
        Self { generation: 0, observed: false, rx_bytes: 0, read_bytes: 0,
            peek_bytes: 0, read_calls: 0, peek_calls: 0, would_block: 0,
            ready_probes: 0, readable_hits: 0, socket_queue_peak: 0 }
    }

    /// Socket generations are captured at creation. Late events from a retired
    /// run cannot reset or contaminate a newer run's counters.
    pub fn for_generation(&mut self, generation: u64) -> Option<&mut Self> {
        if self.observed && generation < self.generation { return None; }
        if !self.observed || generation > self.generation {
            *self = Self { generation, observed: true, ..Self::new() };
        }
        Some(self)
    }

    pub fn receive(&mut self, bytes: usize, queued: usize) {
        self.rx_bytes = self.rx_bytes.saturating_add(bytes as u64);
        self.socket_queue_peak = self.socket_queue_peak.max(queued as u64);
    }

    pub fn read(&mut self, bytes: usize, peek: bool, would_block: bool) {
        if peek {
            self.peek_calls = self.peek_calls.saturating_add(1);
            self.peek_bytes = self.peek_bytes.saturating_add(bytes as u64);
        } else {
            self.read_calls = self.read_calls.saturating_add(1);
            self.read_bytes = self.read_bytes.saturating_add(bytes as u64);
        }
        self.would_block = self.would_block.saturating_add(u64::from(would_block));
    }

    pub fn readiness(&mut self, readable: bool) {
        self.ready_probes = self.ready_probes.saturating_add(1);
        self.readable_hits = self.readable_hits.saturating_add(u64::from(readable));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_reset_ignores_late_old_socket_activity() {
        let mut c = Counters::new();
        c.for_generation(2).unwrap().receive(100, 100);
        c.for_generation(3).unwrap().read(4, false, false);
        assert!(c.for_generation(2).is_none());
        assert_eq!((c.generation, c.rx_bytes, c.read_bytes), (3, 0, 4));
        c.for_generation(3).unwrap().receive(20, 20);
        assert_eq!((c.rx_bytes, c.socket_queue_peak), (20, 20));
    }

    #[test]
    fn peeks_do_not_count_as_consumption_and_empty_reads_are_observable() {
        let mut c = Counters::new();
        let c = c.for_generation(1).unwrap();
        c.receive(12, 12);
        c.read(8, true, false);
        c.read(8, true, false);
        c.read(12, false, false);
        c.read(0, false, true);
        c.read(0, false, false); // EOF and zero-capacity do not imply WouldBlock.
        c.readiness(false);
        c.readiness(true);
        assert_eq!((c.rx_bytes, c.read_bytes, c.peek_bytes), (12, 12, 16));
        assert_eq!((c.read_calls, c.peek_calls, c.would_block), (3, 2, 1));
        assert_eq!((c.ready_probes, c.readable_hits), (2, 1));
    }

    #[test]
    fn counters_saturate_and_vm_slots_remain_independent() {
        let mut slots = [Counters::new(); 2];
        let c = slots[0].for_generation(1).unwrap();
        c.rx_bytes = u64::MAX - 1;
        c.read_calls = u64::MAX;
        c.receive(20, 20);
        c.read(0, false, false);
        assert_eq!((c.rx_bytes, c.read_calls), (u64::MAX, u64::MAX));
        assert!(!slots[1].observed);
    }
}
