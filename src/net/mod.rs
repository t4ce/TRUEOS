pub mod adapter;
pub mod core;
pub mod device;
pub mod e1000;
pub mod ring;
pub mod r8125;
pub mod r8168;
pub mod tls_socket;
pub mod vio;
pub mod tls;

use ::core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use crate::net::core::NetCore;
use crate::net::device::NetDevice;
use crate::net::ring::NetRing;
use crate::net::r8125::R8125Adapter;
use crate::net::r8168::R8168Adapter;
use crate::net::vio::VirtioNetAdapter;
use crate::net::e1000::E1000Adapter;

const RX_DESC_COUNT: usize = 64;
const RX_BUF_SIZE: usize = 2048;
const POLL_BUDGET: usize = 256;
const ENABLE_R8125: bool = false;

enum ActiveDevice {
    Virtio(NetCore<VirtioNetAdapter>),
    E1000(NetCore<E1000Adapter>),
    R8125(NetCore<R8125Adapter>),
    R8168(NetCore<R8168Adapter>),
}

impl NetDevice for ActiveDevice {
    fn mac(&self) -> [u8; 6] {
        match self {
            ActiveDevice::Virtio(dev) => dev.mac(),
            ActiveDevice::E1000(dev) => dev.mac(),
            ActiveDevice::R8125(dev) => dev.mac(),
            ActiveDevice::R8168(dev) => dev.mac(),
        }
    }

    fn poll_rx(&mut self) {
        match self {
            ActiveDevice::Virtio(dev) => dev.poll_rx(),
            ActiveDevice::E1000(dev) => dev.poll_rx(),
            ActiveDevice::R8125(dev) => dev.poll_rx(),
            ActiveDevice::R8168(dev) => dev.poll_rx(),
        }
    }

    fn pop_rx(&mut self) -> Option<alloc::vec::Vec<u8>> {
        match self {
            ActiveDevice::Virtio(dev) => dev.pop_rx(),
            ActiveDevice::E1000(dev) => dev.pop_rx(),
            ActiveDevice::R8125(dev) => dev.pop_rx(),
            ActiveDevice::R8168(dev) => dev.pop_rx(),
        }
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        match self {
            ActiveDevice::Virtio(dev) => dev.transmit(frame),
            ActiveDevice::E1000(dev) => dev.transmit(frame),
            ActiveDevice::R8125(dev) => dev.transmit(frame),
            ActiveDevice::R8168(dev) => dev.transmit(frame),
        }
    }
}

static DEVICES: Mutex<alloc::vec::Vec<ActiveDevice>> = Mutex::new(alloc::vec::Vec::new());
static PRIMARY_DEVICE_INDEX: AtomicUsize = AtomicUsize::new(0);

pub fn init() {
    {
        let mut guard = DEVICES.lock();
        guard.clear();
    }
    PRIMARY_DEVICE_INDEX.store(0, Ordering::Relaxed);

    let mut added: usize = 0;

    // Ordering matters: most of the stack defaults to device 0 as the primary
    // interface (e.g. `mac_address()` and early boot probes). Prefer virtio in
    // virtualized environments so we get the best-performing/most-reliable NIC
    // without requiring any external run flags.

    for adapter in VirtioNetAdapter::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::Virtio(NetCore::new(adapter, ring)));
        added += 1;
    }

    for adapter in E1000Adapter::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::E1000(NetCore::new(adapter, ring)));
        added += 1;
    }

    if ENABLE_R8125 {
        for adapter in R8125Adapter::init_all() {
            let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
            let mut guard = DEVICES.lock();
            guard.push(ActiveDevice::R8125(NetCore::new(adapter, ring)));
            added += 1;
        }
    } else {
        crate::log!("net: r8125 disabled (temporary)\n");
    }

    for adapter in R8168Adapter::init_all() {
        let ring = NetRing::new(RX_DESC_COUNT, RX_BUF_SIZE, POLL_BUDGET);
        let mut guard = DEVICES.lock();
        guard.push(ActiveDevice::R8168(NetCore::new(adapter, ring)));
        added += 1;
    }

    if added == 0 {
        crate::log!("net: no supported NIC detected.\n");
    } else {
        crate::log!("net: detected {} NIC(s); primary=0 (initial)\n", added);
    }

    crate::log!("net: hint: prefer virtio-net in QEMU (e.g. -netdev user,id=net0,hostfwd=tcp::4245-:4245 -device virtio-net-pci,netdev=net0)\n");
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
    mac_address_at(primary_device_index())
}

pub fn mac_address_at(index: usize) -> Option<[u8; 6]> {
    with_device_at(index, |dev| Some(dev.mac())).flatten()
}

pub fn device_count() -> usize {
    DEVICES.lock().len()
}

pub fn primary_device_index() -> usize {
    let count = device_count();
    if count == 0 {
        return 0;
    }
    let idx = PRIMARY_DEVICE_INDEX.load(Ordering::Relaxed);
    idx.min(count - 1)
}

pub fn set_primary_device_index(index: usize) {
    let count = device_count();
    if count == 0 {
        return;
    }
    let new_idx = index.min(count - 1);
    let old_idx = PRIMARY_DEVICE_INDEX.swap(new_idx, Ordering::Relaxed);
    if old_idx != new_idx {
        crate::log!("net: primary device switched {} -> {}\n", old_idx, new_idx);
    }
}

fn with_device_at<R>(index: usize, f: impl FnOnce(&mut dyn NetDevice) -> R) -> Option<R> {
    let mut guard = DEVICES.lock();
    let dev = guard.get_mut(index)?;
    Some(f(dev))
}
