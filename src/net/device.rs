use alloc::vec::Vec;

pub trait NetDevice {
    /// Return the hardware MAC address or zeros if unavailable.
    fn mac(&self) -> [u8; 6];
    /// Poll receive path (e.g., acknowledge interrupts, service rings).
    /// Returns true if any packet was received/processed.
    fn poll_rx(&mut self) -> bool;
    /// Pop a single received frame, if available.
    fn pop_rx(&mut self) -> Option<Vec<u8>>;

    /// Drain multiple received frames, up to `limit`, without building an
    /// intermediate burst container.
    fn drain_rx_each(&mut self, limit: usize, f: &mut dyn FnMut(Vec<u8>)) -> usize {
        let mut drained = 0usize;
        while drained < limit {
            let Some(pkt) = self.pop_rx() else {
                break;
            };
            drained += 1;
            f(pkt);
        }
        drained
    }

    /// Return the number of pending RX frames.
    fn rx_queue_len(&self) -> usize {
        0
    }
    /// Transmit a raw Ethernet frame.
    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()>;

    /// Transmit a raw Ethernet frame by filling storage owned by the device.
    ///
    /// Drivers with CPU-mapped TX DMA slots can override this to avoid an
    /// intermediate frame allocation and copy. The default preserves existing
    /// driver behavior through the shared packet pool.
    fn transmit_with(&mut self, len: usize, fill: &mut dyn FnMut(&mut [u8])) -> Result<(), ()> {
        let mut frame = crate::net::ring::alloc_packet_buf(len);
        fill(&mut frame);
        let result = self.transmit(&frame);
        crate::net::ring::recycle_packet_buf(frame);
        result
    }

    /// Whether a TX token can be offered without knowingly hitting a full
    /// hardware ring. A later transmit can still fail and must be checked.
    fn transmit_ready(&mut self) -> bool {
        self.link_state().up
    }

    /// Return current link state (if known).
    ///
    /// Default is "down/unknown" so backends that don't support link probing
    /// don't need to implement it.
    fn link_state(&self) -> LinkState {
        LinkState::down()
    }
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
