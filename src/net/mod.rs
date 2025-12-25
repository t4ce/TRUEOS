pub mod adapter;
pub mod device;
pub mod e1000;
pub mod intel8254x_hal;
use spin::Mutex;
use device::NetDevice;
use device::{E1000Device, Intel8254xHalDevice};
enum ActiveDevice {
    E1000(E1000Device),
    Intel8254x(Intel8254xHalDevice),
}

enum ActiveDevice {}

impl NetDevice for ActiveDevice {
    fn mac(&self) -> [u8; 6] {
        match self {
            ActiveDevice::E1000(dev) => dev.mac(),
            ActiveDevice::Intel8254x(dev) => dev.mac(),
        }
    }

    fn poll_rx(&mut self) {
        match self {
            ActiveDevice::E1000(dev) => dev.poll_rx(),
            ActiveDevice::Intel8254x(dev) => dev.poll_rx(),
        }
    }

    fn pop_rx(&mut self) -> Option<alloc::vec::Vec<u8>> {
        match self {
            ActiveDevice::E1000(dev) => dev.pop_rx(),
            ActiveDevice::Intel8254x(dev) => dev.pop_rx(),
        }
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        match self {
            ActiveDevice::E1000(dev) => dev.transmit(frame),
            ActiveDevice::Intel8254x(dev) => dev.transmit(frame),
        }
    }
}

static DEVICE: Mutex<Option<ActiveDevice>> = Mutex::new(None);

pub fn init() {
    if intel8254x_hal::init().is_ok() {
        let mut guard = DEVICE.lock();
        *guard = Some(ActiveDevice::Intel8254x(Intel8254xHalDevice));
        crate::log_info!("net: using intel8254x-hal driver.");
        return;
    }
    crate::log_warn!("intel8254x-hal: init failed; falling back to e1000.");

    if e1000::init().is_ok() {
        let mut guard = DEVICE.lock();
        *guard = Some(ActiveDevice::E1000(E1000Device));
        crate::log_info!("net: using e1000 driver.");
    } else {
        crate::log_warn!("e1000 NIC not detected.");
    }
}

pub fn init() {
    crate::log_info!("net: disabled (feature \"net-go\" not enabled).");
}

pub fn poll() {
    with_device(|dev| dev.poll_rx());
}

pub fn poll() {}

pub fn pop_rx_packet() -> Option<alloc::vec::Vec<u8>> {
    with_device(|dev| dev.pop_rx()).flatten()
}

pub fn pop_rx_packet() -> Option<alloc::vec::Vec<u8>> {
    None
}

pub fn transmit_packet(data: &[u8]) -> Result<(), ()> {
    with_device(|dev| dev.transmit(data)).unwrap_or(Err(()))
}

pub fn transmit_packet(_data: &[u8]) -> Result<(), ()> {
    Err(())
}

pub fn mac_address() -> Option<[u8; 6]> {
    with_device(|dev| Some(dev.mac())).flatten()
}

pub fn mac_address() -> Option<[u8; 6]> {
    None
}

fn with_device<R>(f: impl FnOnce(&mut dyn NetDevice) -> R) -> Option<R> {
    let mut guard = DEVICE.lock();
    if let Some(ref mut dev) = *guard {
        Some(f(dev))
    } else {
        None
    }
}

fn with_device<R>(_f: impl FnOnce(&mut dyn NetDevice) -> R) -> Option<R> {
    None
}
