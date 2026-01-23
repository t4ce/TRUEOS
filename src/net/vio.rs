use alloc::vec::Vec;

use crate::net::core::VendorAdapter;
use crate::net::device::{DescFormat, LinkState, VendorNetAdapter};
use crate::net::ring::{NetRing, TxError};

pub struct VirtioNetAdapter {
    ring: Option<*mut NetRing>,
}

impl VirtioNetAdapter {
    pub fn init() -> Result<Self, ()> {
        Err(())
    }
}

impl VendorAdapter for VirtioNetAdapter {
    fn mac(&self) -> [u8; 6] {
        [0; 6]
    }

    fn poll_rx(&mut self) {}

    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn transmit(&mut self, _frame: &[u8]) -> Result<(), ()> {
        if let Some(ring) = self.ring {
            // Safety: ring pointer is owned and kept alive by NetCore.
            let ring = unsafe { &mut *ring };
            return match ring.tx_submit(_frame) {
                Ok(()) => Ok(()),
                Err(TxError::RingFull) | Err(TxError::FrameTooLarge) => Err(()),
            };
        }
        Err(())
    }

    fn bind_ring(&mut self, ring: *mut NetRing) {
        self.ring = Some(ring);
    }
}

impl VendorNetAdapter for VirtioNetAdapter {
    fn init_hw(&mut self) -> Result<(), ()> {
        Err(())
    }

    fn reset(&mut self) {}

    fn read_link(&mut self) -> LinkState {
        LinkState::down()
    }

    fn write_regs(&mut self) {}

    fn kick_tx(&mut self) {}

    fn ack_irq(&mut self) {}

    fn enable_irq(&mut self) {}

    fn disable_irq(&mut self) {}

    fn rx_desc_format(&self) -> DescFormat {
        DescFormat::default()
    }

    fn tx_desc_format(&self) -> DescFormat {
        DescFormat::default()
    }
}
