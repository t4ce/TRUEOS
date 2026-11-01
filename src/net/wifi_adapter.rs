//! Ethernet boundary for a firmware-backed Wi-Fi station.
//!
//! The radio driver owns association, keys and 802.11 framing. The ordinary
//! TCP/IP service sees exactly the same Ethernet frames as a wired adapter.

use super::DriverStatus;
use super::device::{LinkState, NetDevice};
use super::wifi::{SharedDriver, WifiState};
use alloc::{collections::VecDeque, vec::Vec};

const RX_BUDGET: usize = 64;
const RX_CAPACITY: usize = 256;

pub(crate) struct WifiNic {
    driver: SharedDriver,
    pci: crate::pci::PciDevice,
    mac: [u8; 6],
    rx: VecDeque<Vec<u8>>,
}

pub(crate) fn valid_station_mac(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac[0] & 1 == 0
}

fn ethernet_frame(frame: &[u8]) -> bool {
    (14..=1514).contains(&frame.len())
}

impl WifiNic {
    pub(crate) fn new(driver: SharedDriver, pci: crate::pci::PciDevice) -> Option<Self> {
        let mac = {
            let radio = driver.lock();
            if radio.status() != DriverStatus::Running || !radio.ethernet_data_path() {
                return None;
            }
            let mac = radio.mac_address()?;
            if !valid_station_mac(mac) {
                return None;
            }
            mac
        };
        Some(Self {
            driver,
            pci,
            mac,
            rx: VecDeque::new(),
        })
    }

    pub(crate) fn pci_device(&self) -> crate::pci::PciDevice {
        self.pci
    }

    pub(crate) fn name(&self) -> &'static str {
        self.driver.lock().info().name
    }

    fn station_link(&self) -> LinkState {
        let driver = self.driver.lock();
        let up = driver.status() == DriverStatus::Running
            && driver.ethernet_data_path()
            && driver.wifi_state() == WifiState::Connected
            && driver.link_up();
        LinkState {
            up,
            speed_mbps: if up { driver.link_speed() } else { 0 },
            full_duplex: false,
        }
    }
}

impl NetDevice for WifiNic {
    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn poll_rx(&mut self) -> bool {
        // Runtime firmware servicing and association are driven separately by
        // wifi::poll(), including before the Ethernet interface can be admitted.
        let mut driver = self.driver.lock();
        let connected = driver.status() == DriverStatus::Running
            && driver.ethernet_data_path()
            && driver.wifi_state() == WifiState::Connected
            && driver.link_up();
        if !connected {
            self.rx.clear();
            return false;
        }
        for _ in 0..RX_BUDGET {
            let Some(packet) = driver.receive() else {
                break;
            };
            if ethernet_frame(&packet) && self.rx.len() < RX_CAPACITY {
                self.rx.push_back(packet);
            }
        }
        !self.rx.is_empty()
    }

    fn pop_rx(&mut self) -> Option<Vec<u8>> {
        self.rx.pop_front()
    }

    fn rx_queue_len(&self) -> usize {
        self.rx.len()
    }

    fn transmit(&mut self, frame: &[u8]) -> Result<(), ()> {
        if !ethernet_frame(frame) {
            return Err(());
        }
        let mut driver = self.driver.lock();
        if driver.status() != DriverStatus::Running
            || !driver.ethernet_data_path()
            || driver.wifi_state() != WifiState::Connected
            || !driver.link_up()
        {
            return Err(());
        }
        driver.send(frame).map_err(|_| ())
    }

    fn link_state(&self) -> LinkState {
        self.station_link()
    }
}
