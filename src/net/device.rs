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
pub struct OffloadFlags {
    pub checksum: bool,
    pub tso: bool,
    pub lro: bool,
    pub vlan: bool,
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

    pub fn up(speed_mbps: u32, full_duplex: bool) -> Self {
        Self {
            up: true,
            speed_mbps,
            full_duplex,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DescFormat {
    pub desc_len: usize,
    pub align: usize,
    pub writable: bool,
}

pub trait VendorNetAdapter {
    fn init_hw(&mut self) -> Result<(), ()>;
    fn reset(&mut self);
    fn read_link(&mut self) -> LinkState;
    fn write_regs(&mut self);
    fn kick_tx(&mut self);
    fn ack_irq(&mut self);
    fn enable_irq(&mut self);
    fn disable_irq(&mut self);
    fn rx_desc_format(&self) -> DescFormat;
    fn tx_desc_format(&self) -> DescFormat;
}
