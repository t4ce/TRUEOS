use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::net::device::{LinkState, NetDevice};
use crate::net::ring::NetRing;

const RX_QUEUE_SOFT_CAP: usize = 64;

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
            rx_queue: VecDeque::with_capacity(RX_QUEUE_SOFT_CAP),
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
        let mut received = false;
        self.ring.poll_rx_each(|packet| {
            received = true;
            if self.rx_queue.len() < RX_QUEUE_SOFT_CAP {
                self.rx_queue.push_back(packet);
            } else {
                crate::net::ring::recycle_packet_buf(packet);
            }
        });
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

    fn drain_rx_each(&mut self, limit: usize, f: &mut dyn FnMut(Vec<u8>)) -> usize {
        let mut drained = 0usize;
        while drained < limit {
            let Some(pkt) = self.rx_queue.pop_front() else {
                break;
            };
            drained += 1;
            f(pkt);
        }
        drained
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
