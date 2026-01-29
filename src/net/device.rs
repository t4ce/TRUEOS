use alloc::vec::Vec;

pub trait NetDevice {
    /// Return the hardware MAC address or zeros if unavailable.
    fn mac(&self) -> [u8; 6];
    /// Poll receive path (e.g., acknowledge interrupts, service rings).
    fn poll_rx(&mut self);
    /// Pop a single received frame, if available.
    fn pop_rx(&mut self) -> Option<Vec<u8>>;
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
