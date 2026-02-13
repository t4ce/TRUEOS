use alloc::vec::Vec;

pub trait NetDevice {
    /// Return the hardware MAC address or zeros if unavailable.
    fn mac(&self) -> [u8; 6];
    /// Poll receive path (e.g., acknowledge interrupts, service rings).
    /// Returns true if any packet was received/processed.
    fn poll_rx(&mut self) -> bool;
    /// Pop a single received frame, if available.
    fn pop_rx(&mut self) -> Option<Vec<u8>>;

    /// Drain multiple received frames, up to `limit`.
    fn drain_rx(&mut self, limit: usize) -> Vec<Vec<u8>> {
        let mut out = Vec::with_capacity(limit.min(64));
        for _ in 0..limit {
            if let Some(pkt) = self.pop_rx() {
                out.push(pkt);
            } else {
                break;
            }
        }
        out
    }

    /// Return the number of pending RX frames.
    fn rx_queue_len(&self) -> usize { 0 }
    /// Transmit a raw Ethernet frame.
    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkState {
    pub up: bool,
    pub speed_mbps: u32,
    pub full_duplex: bool,
}

impl LinkState {
    pub fn down() -> Self {
        Self::default()
    }
}
