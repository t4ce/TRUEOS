pub mod adapter;
pub mod core;
pub mod device;
pub mod ring;
pub mod vio;

use spin::Mutex;

use crate::net::core::NetCore;
use crate::net::device::NetDevice;
use crate::net::ring::NetRing;
use crate::net::vio::VirtioNetAdapter;

const RX_DESC_COUNT: usize = 64;
const TX_DESC_COUNT: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;
const POLL_BUDGET: usize = 32;

enum ActiveDevice {
    Virtio(NetCore<VirtioNetAdapter>),
}

impl NetDevice for ActiveDevice {
    fn mac(&self) -> [u8; 6] {
        match self {
            ActiveDevice::Virtio(dev) => dev.mac(),
        }
    }

    fn poll_rx(&mut self) {
        match self {
            ActiveDevice::Virtio(dev) => dev.poll_rx(),
        }
    }

    fn pop_rx(&mut self) -> Option<alloc::vec::Vec<u8>> {
        match self {
            ActiveDevice::Virtio(dev) => dev.pop_rx(),
        }
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        match self {
            ActiveDevice::Virtio(dev) => dev.transmit(frame),
        }
    }
}

static DEVICE: Mutex<Option<ActiveDevice>> = Mutex::new(None);

pub fn init() {
    if let Ok(adapter) = VirtioNetAdapter::init() {
        let mut guard = DEVICE.lock();
        let ring = NetRing::new(
            RX_DESC_COUNT,
            RX_BUF_SIZE,
            TX_DESC_COUNT,
            TX_BUF_SIZE,
            POLL_BUDGET,
        );
        *guard = Some(ActiveDevice::Virtio(NetCore::new(adapter, ring)));
        crate::log!("net: using virtio-net adapter.\n");
        return;
    }

    crate::log!("net: no supported NIC detected.\n");
}

pub fn poll() {
    with_device(|dev| dev.poll_rx());
}

pub fn pop_rx_packet() -> Option<alloc::vec::Vec<u8>> {
    with_device(|dev| dev.pop_rx()).flatten()
}

pub fn transmit_packet(data: &[u8]) -> Result<(), ()> {
    with_device(|dev| dev.transmit(data)).unwrap_or(Err(()))
}

pub fn mac_address() -> Option<[u8; 6]> {
    with_device(|dev| Some(dev.mac())).flatten()
}

fn with_device<R>(f: impl FnOnce(&mut dyn NetDevice) -> R) -> Option<R> {
    let mut guard = DEVICE.lock();
    if let Some(ref mut dev) = *guard {
        Some(f(dev))
    } else {
        None
    }
}
