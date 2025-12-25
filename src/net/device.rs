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

pub struct E1000Device;

impl NetDevice for E1000Device {
    fn mac(&self) -> [u8; 6] {
        crate::net::e1000::mac_address().unwrap_or([0; 6])
    }

    fn poll_rx(&mut self) {
        crate::net::e1000::poll();
    }

    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        crate::net::e1000::pop_rx_packet()
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        crate::net::e1000::transmit_packet(frame)
    }
}

pub struct Intel8254xHalDevice;

impl NetDevice for Intel8254xHalDevice {
    fn mac(&self) -> [u8; 6] {
        [0; 6]
    }

    fn poll_rx(&mut self) {
    }

    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn transmit(&mut self, _frame: &[u8]) -> Result<(), ()> {
        Err(())
    }
}
