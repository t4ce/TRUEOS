use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::net::device::{LinkState, NetDevice};
use crate::net::ring::NetRing;

pub trait VendorAdapter {
    fn mac(&self) -> [u8; 6];
    fn poll_rx(&mut self);
    fn pop_rx(&mut self) -> Option<Vec<u8>>;
    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()>;

    #[inline]
    fn link_state(&self) -> LinkState {
        LinkState::down()
    }

    #[inline]
    fn pci_device(&self) -> Option<crate::pci::PciDevice> {
        None
    }

    fn bind_ring(&mut self, _ring: *mut NetRing) {}
}

pub struct NetCore<A: VendorAdapter> {
    adapter: A,
    ring: Box<NetRing>,
    rx_queue: VecDeque<Vec<u8>>,
}

impl<A: VendorAdapter> NetCore<A> {
    pub fn new(mut adapter: A, ring: NetRing) -> Self {
        let mut ring = Box::new(ring);
        adapter.bind_ring(&mut *ring as *mut NetRing);
        Self {
            adapter,
            ring,
            rx_queue: VecDeque::new(),
        }
    }

    #[inline]
    pub fn pci_device(&self) -> Option<crate::pci::PciDevice> {
        self.adapter.pci_device()
    }
}

impl<A: VendorAdapter> NetDevice for NetCore<A> {
    fn mac(&self) -> [u8; 6] {
        self.adapter.mac()
    }

    fn poll_rx(&mut self) -> bool {
        self.adapter.poll_rx();
        let packets = self.ring.poll_rx();
        let received = !packets.is_empty();
        if received {
            self.rx_queue.extend(packets);
        }
        received
    }

    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        if let Some(pkt) = self.rx_queue.pop_front() {
            return Some(pkt);
        }
        self.adapter.pop_rx()
    }

    fn drain_rx(&mut self, limit: usize) -> Vec<Vec<u8>> {
        if self.rx_queue.is_empty() {
            return Vec::new();
        }
        let len = self.rx_queue.len().min(limit);
        self.rx_queue.drain(..len).collect()
    }

    fn rx_queue_len(&self) -> usize {
        self.rx_queue.len()
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        self.adapter.transmit(frame)
    }

    fn link_state(&self) -> LinkState {
        self.adapter.link_state()
    }
}
