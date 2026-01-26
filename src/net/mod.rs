pub mod adapter;
pub mod core;
pub mod device;
pub mod e1000;
pub mod html;
pub mod https;
pub mod ring;
pub mod r8169;
pub mod vio;

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

static DEVICE: Mutex<Option<ActiveDevice>> = Mutex::new(None);

pub fn adapter_backend_name() -> Option<&'static str> {
    let guard = DEVICE.lock();
    guard.as_ref().map(|dev| match dev {
        ActiveDevice::Virtio(_) => "virtio-net",
        ActiveDevice::E1000(_) => "e1000",
        ActiveDevice::R8169(_) => "r8169",
    })
}

pub fn init() {
    if let Ok(adapter) = R8169Adapter::init() {
        let mut guard = DEVICE.lock();
        let ring = NetRing::new(
            RX_DESC_COUNT,
            RX_BUF_SIZE,
            TX_DESC_COUNT,
            TX_BUF_SIZE,
            POLL_BUDGET,
        );
        *guard = Some(ActiveDevice::R8169(NetCore::new(adapter, ring)));
        crate::log!("net: using r8169 adapter.\n");
        return;
    }

    crate::log!("net: r8169 init failed; trying e1000.\n");

    if let Ok(adapter) = E1000Adapter::init() {
        let mut guard = DEVICE.lock();
        let ring = NetRing::new(
            RX_DESC_COUNT,
            RX_BUF_SIZE,
            TX_DESC_COUNT,
            TX_BUF_SIZE,
            POLL_BUDGET,
        );
        *guard = Some(ActiveDevice::E1000(NetCore::new(adapter, ring)));
        crate::log!("net: using e1000 adapter.\n");
        return;
    }

    crate::log!("net: e1000 init failed; trying virtio-net.\n");

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

    crate::log!("net: virtio-net init failed; no supported NIC detected.\n");

    crate::log!(
        "net: hint: in QEMU add virtio-net (e.g. -netdev user,id=net0,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net0,disable-modern=on)\n"
    );
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
