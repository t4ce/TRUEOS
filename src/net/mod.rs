pub mod adapter;
pub mod core;
pub mod device;
pub mod e1000;
pub mod ring;
pub mod r8169;
pub mod tls_socket;
pub mod vio;
pub mod tls;
pub mod tls_demo;

use spin::Mutex;

use crate::net::core::NetCore;
use crate::net::device::NetDevice;
use crate::net::ring::NetRing;
use crate::net::r8169::R8169Adapter;
use crate::net::vio::VirtioNetAdapter;
use crate::net::e1000::E1000Adapter;

const RX_DESC_COUNT: usize = 64;
const TX_DESC_COUNT: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;
const POLL_BUDGET: usize = 32;

enum ActiveDevice {
    Virtio(NetCore<VirtioNetAdapter>),
    E1000(NetCore<E1000Adapter>),
    R8169(NetCore<R8169Adapter>),
}

impl NetDevice for ActiveDevice {
    fn mac(&self) -> [u8; 6] {
        match self {
            ActiveDevice::Virtio(dev) => dev.mac(),
            ActiveDevice::E1000(dev) => dev.mac(),
            ActiveDevice::R8169(dev) => dev.mac(),
        }
    }

    fn poll_rx(&mut self) {
        match self {
            ActiveDevice::Virtio(dev) => dev.poll_rx(),
            ActiveDevice::E1000(dev) => dev.poll_rx(),
            ActiveDevice::R8169(dev) => dev.poll_rx(),
        }
    }

    fn pop_rx(&mut self) -> Option<alloc::vec::Vec<u8>> {
        match self {
            ActiveDevice::Virtio(dev) => dev.pop_rx(),
            ActiveDevice::E1000(dev) => dev.pop_rx(),
            ActiveDevice::R8169(dev) => dev.pop_rx(),
        }
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        match self {
            ActiveDevice::Virtio(dev) => dev.transmit(frame),
            ActiveDevice::E1000(dev) => dev.transmit(frame),
            ActiveDevice::R8169(dev) => dev.transmit(frame),
        }
    }
}

static DEVICES: Mutex<alloc::vec::Vec<ActiveDevice>> = Mutex::new(alloc::vec::Vec::new());

pub fn init() {
    {
        let mut guard = DEVICES.lock();
        guard.clear();
    }

    let mut added: usize = 0;

    for adapter in R8169Adapter::init_all() {
        let ring = NetRing::new(
            RX_DESC_COUNT,
            RX_BUF_SIZE,
            TX_DESC_COUNT,
            TX_BUF_SIZE,
            POLL_BUDGET,
        );
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::R8169(NetCore::new(adapter, ring)));
        added += 1;
    }

    for adapter in E1000Adapter::init_all() {
        let ring = NetRing::new(
            RX_DESC_COUNT,
            RX_BUF_SIZE,
            TX_DESC_COUNT,
            TX_BUF_SIZE,
            POLL_BUDGET,
        );
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::E1000(NetCore::new(adapter, ring)));
        added += 1;
    }

    for adapter in VirtioNetAdapter::init_all() {
        let ring = NetRing::new(
            RX_DESC_COUNT,
            RX_BUF_SIZE,
            TX_DESC_COUNT,
            TX_BUF_SIZE,
            POLL_BUDGET,
        );
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::Virtio(NetCore::new(adapter, ring)));
        added += 1;
    }

    if added == 0 {
        crate::log!("net: no supported NIC detected.\n");
    } else {
        crate::log!("net: detected {} NIC(s); primary=0\n", added);
    }

    crate::log!(
        "net: hint: in QEMU add virtio-net (e.g. -netdev user,id=net0,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net0,disable-modern=on)\n"
    );
}

pub fn poll_at(index: usize) {
    let _ = with_device_at(index, |dev| dev.poll_rx());
}

pub fn pop_rx_packet_at(index: usize) -> Option<alloc::vec::Vec<u8>> {
    with_device_at(index, |dev| dev.pop_rx()).flatten()
}

pub fn transmit_packet_at(index: usize, data: &[u8]) -> Result<(), ()> {
    with_device_at(index, |dev| dev.transmit(data)).unwrap_or(Err(()))
}

pub fn mac_address() -> Option<[u8; 6]> {
    mac_address_at(0)
}

pub fn mac_address_at(index: usize) -> Option<[u8; 6]> {
    with_device_at(index, |dev| Some(dev.mac())).flatten()
}

pub fn device_count() -> usize {
    DEVICES.lock().len()
}

fn with_device_at<R>(index: usize, f: impl FnOnce(&mut dyn NetDevice) -> R) -> Option<R> {
    let mut guard = DEVICES.lock();
    let dev = guard.get_mut(index)?;
    Some(f(dev))
}
