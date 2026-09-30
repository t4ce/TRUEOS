//! Bounded Ethernet queues between the synchronous network stack and async USB.
use super::device::{LinkState, NetDevice};
use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use spin::Mutex;

pub(crate) const QUEUE_CAP: usize = 64;
pub(crate) struct State {
    pub rx: VecDeque<Vec<u8>>,
    pub tx: VecDeque<Vec<u8>>,
    pub link: LinkState,
    pub failed: bool,
}
pub(crate) type Shared = Arc<Mutex<State>>;
pub(crate) struct UsbNic {
    mac: [u8; 6],
    pub shared: Shared,
}
impl UsbNic {
    pub fn new(mac: [u8; 6]) -> Self {
        Self {
            mac,
            shared: Arc::new(Mutex::new(State {
                rx: VecDeque::new(),
                tx: VecDeque::new(),
                link: LinkState::down(),
                failed: false,
            })),
        }
    }
}
impl NetDevice for UsbNic {
    fn mac(&self) -> [u8; 6] {
        self.mac
    }
    fn poll_rx(&mut self) -> bool {
        !self.shared.lock().rx.is_empty()
    }
    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        self.shared.lock().rx.pop_front()
    }
    fn rx_queue_len(&self) -> usize {
        self.shared.lock().rx.len()
    }
    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        let mut state = self.shared.lock();
        if state.failed
            || !state.link.up
            || state.tx.len() >= QUEUE_CAP
            || !(14..=1514).contains(&frame.len())
        {
            return Err(());
        }
        state.tx.push_back(frame.to_vec());
        Ok(())
    }
    fn transmit_ready(&mut self) -> bool {
        let state = self.shared.lock();
        !state.failed && state.link.up && state.tx.len() < QUEUE_CAP
    }
    fn link_state(&self) -> LinkState {
        self.shared.lock().link
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tx_is_bounded_and_requires_a_live_link() {
        let mut nic = UsbNic::new([2, 1, 2, 3, 4, 5]);
        let frame = [0u8; 60];
        assert!(nic.transmit(&frame).is_err());
        nic.shared.lock().link.up = true;
        assert!(nic.transmit(&[0; 13]).is_err());
        assert!(nic.transmit(&[0; 1515]).is_err());
        for _ in 0..QUEUE_CAP {
            assert!(nic.transmit(&frame).is_ok());
        }
        assert!(!nic.transmit_ready());
        assert!(nic.transmit(&frame).is_err());
        nic.shared.lock().tx.pop_front();
        assert!(nic.transmit_ready());
        nic.shared.lock().failed = true;
        assert!(!nic.transmit_ready());
        assert!(nic.transmit(&frame).is_err());
    }
    #[test]
    fn ethernet_frames_cross_the_queue_unchanged() {
        let mut nic = UsbNic::new([2, 1, 2, 3, 4, 5]);
        nic.shared.lock().link.up = true;
        let frame: Vec<u8> = (0..60).collect();
        nic.transmit(&frame).unwrap();
        assert_eq!(nic.shared.lock().tx.pop_front(), Some(frame.clone()));
        nic.shared.lock().rx.push_back(frame.clone());
        assert!(nic.poll_rx());
        assert_eq!(nic.rx_queue_len(), 1);
        assert_eq!(nic.pop_rx(), Some(frame));
        assert_eq!(nic.pop_rx(), None);
    }
}
