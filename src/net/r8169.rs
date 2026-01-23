use alloc::vec::Vec;

use crate::net::core::VendorAdapter;

pub struct R8169Adapter;

impl R8169Adapter {
    pub fn init() -> Result<Self, ()> {
        Err(())
    }
}

impl VendorAdapter for R8169Adapter {
    fn mac(&self) -> [u8; 6] {
        [0; 6]
    }

    fn poll_rx(&mut self) {}

    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn transmit(&mut self, _frame: &[u8]) -> Result<(), ()> {
        Err(())
    }
}
