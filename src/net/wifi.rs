#![allow(dead_code)]

use super::{DriverStatus, NetworkDriver};
use crate::pci::PciDevice;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    WEP,
    WPA,
    WPA2,
    WPA3,
    Unknown,
}

impl WifiSecurity {
    pub fn as_str(&self) -> &'static str {
        match self {
            WifiSecurity::Open => "Open",
            WifiSecurity::WEP => "WEP",
            WifiSecurity::WPA => "WPA",
            WifiSecurity::WPA2 => "WPA2",
            WifiSecurity::WPA3 => "WPA3",
            WifiSecurity::Unknown => "???",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub signal_dbm: i8,
    pub security: WifiSecurity,
    pub frequency_mhz: u16,
}

impl WifiNetwork {
    pub fn signal_quality(&self) -> u8 {
        // Convert dBm to percentage: -30 dBm = 100%, -90 dBm = 0%
        if self.signal_dbm >= -30 {
            return 100;
        }
        if self.signal_dbm <= -90 {
            return 0;
        }
        ((self.signal_dbm as i16 + 90) * 100 / 60) as u8
    }

    pub fn signal_bars(&self) -> u8 {
        match self.signal_quality() {
            80..=100 => 4,
            60..=79 => 3,
            40..=59 => 2,
            20..=39 => 1,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiState {
    NoHardware,
    Disabled,
    Disconnected,
    Scanning,
    Connecting,
    Authenticating,
    Connected,
    Failed,
}

pub trait WifiDriver: NetworkDriver + Send {
    fn wifi_state(&self) -> WifiState;
    fn scan(&mut self) -> Result<(), &'static str>;
    fn scan_results(&self) -> Vec<WifiNetwork>;
    fn connect(&mut self, ssid: &str, password: &str) -> Result<(), &'static str>;
    fn disconnect(&mut self) -> Result<(), &'static str>;
    fn connected_ssid(&self) -> Option<String>;
    fn current_channel(&self) -> Option<u8>;
    fn signal_strength(&self) -> Option<i8>;
}

pub(crate) static WIFI_DRIVER: Mutex<Option<Box<dyn WifiDriver>>> = Mutex::new(None);
static WIFI_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Deferred WiFi PCI location (bus, device, function) — set during boot, probed on first use
static DEFERRED_PCI: Mutex<Option<(u8, u8, u8)>> = Mutex::new(None);

/// Last scan results (cached for UI)
static SCAN_RESULTS: Mutex<Vec<WifiNetwork>> = Mutex::new(Vec::new());

/// Current connection state
static CONNECTION_STATE: Mutex<WifiState> = Mutex::new(WifiState::NoHardware);

/// Currently connected SSID
static CONNECTED_SSID: Mutex<Option<String>> = Mutex::new(None);

/// Pending connection request
static CONNECT_REQUEST: Mutex<Option<(String, String)>> = Mutex::new(None);

pub fn set_deferred_pci(bus: u8, device: u8, function: u8) {
    *DEFERRED_PCI.lock() = Some((bus, device, function));
    crate::log!("[WIFI] Deferred PCI probe stored: {}.{}.{}", bus, device, function);
}

pub fn has_wifi() -> bool {
    WIFI_ACTIVE.load(Ordering::Relaxed) || DEFERRED_PCI.lock().is_some()
}

pub fn has_active_driver() -> bool {
    WIFI_ACTIVE.load(Ordering::Relaxed)
}

pub fn lazy_probe() -> Result<(), &'static str> {
    // Already probed?
    if WIFI_ACTIVE.load(Ordering::Relaxed) {
        return Ok(());
    }

    let pci_loc = DEFERRED_PCI.lock().take();
    let (bus, dev, func) = pci_loc.ok_or("No WiFi hardware detected during boot")?;

    crate::log!("wifi: lazy probe pci={}.{}.{}\n", bus, dev, func);
    crate::log!("[WIFI] Lazy probe: {}.{}.{}", bus, dev, func);

    // Find the PCI device by bus/device/function
    let pci_dev = crate::pci::with_devices(|devices| {
        devices
            .iter()
            .copied()
            .find(|d| d.bus == bus && d.slot == dev && d.function == func)
    })
    .ok_or("WiFi PCI device not found")?;

    // Now actually probe (map_bar0 + bus master)
    if probe_pci(&pci_dev) {
        crate::log!("wifi: probe ok driver=iwl4965\n");
        Ok(())
    } else {
        // Put the deferred info back so user can retry
        *DEFERRED_PCI.lock() = Some((bus, dev, func));
        Err("WiFi hardware probe failed")
    }
}

pub fn state() -> WifiState {
    *CONNECTION_STATE.lock()
}

pub fn is_connected() -> bool {
    *CONNECTION_STATE.lock() == WifiState::Connected
}

pub fn connected_ssid() -> Option<String> {
    CONNECTED_SSID.lock().clone()
}

pub fn signal_strength() -> Option<i8> {
    WIFI_DRIVER
        .lock()
        .as_ref()
        .and_then(|d| d.signal_strength())
}

pub fn ensure_started() -> Result<(), &'static str> {
    // Lazy probe if not yet done (deferred from boot)
    if !WIFI_ACTIVE.load(Ordering::Relaxed) {
        lazy_probe()?;
    }

    let mut guard = WIFI_DRIVER.lock();
    let driver = guard.as_mut().ok_or("No WiFi driver")?;
    if driver.status() == DriverStatus::Running {
        return Ok(());
    }
    crate::log!("[WIFI] Auto-starting driver (hw_init + firmware)...");
    crate::log!("wifi: starting hardware\n");
    match driver.start() {
        Ok(()) => {
            crate::log!("[WIFI] Driver started successfully");
            crate::log!("wifi: hardware initialized\n");
            Ok(())
        }
        Err(e) => {
            crate::log!("[WIFI] Driver start failed: {}", e);
            crate::log!("wifi: start failed: {}", e);
            Err(e)
        }
    }
}

pub fn start_scan() -> Result<(), &'static str> {
    // Lazy probe + auto-start
    ensure_started()?;

    {
        let mut guard = WIFI_DRIVER.lock();
        let driver = guard.as_mut().ok_or("No WiFi driver")?;
        *CONNECTION_STATE.lock() = WifiState::Scanning;
        driver.scan()
    }
}

pub fn get_scan_results() -> Vec<WifiNetwork> {
    SCAN_RESULTS.lock().clone()
}

pub fn poll() {
    let mut guard = WIFI_DRIVER.lock();
    if let Some(driver) = guard.as_mut() {
        driver.poll();

        // Update cached state
        let new_state = driver.wifi_state();
        let old_state = *CONNECTION_STATE.lock();

        if new_state != old_state {
            *CONNECTION_STATE.lock() = new_state;
            crate::log!("[WIFI] State: {:?} -> {:?}", old_state, new_state);
        }

        // Update scan results when scan completes
        if old_state == WifiState::Scanning && new_state != WifiState::Scanning {
            let results = driver.scan_results();
            crate::log!("[WIFI] Scan complete: {} networks found", results.len());
            *SCAN_RESULTS.lock() = results;
        }

        // Update connected SSID
        *CONNECTED_SSID.lock() = driver.connected_ssid();

        // Process pending connect request
        let request = CONNECT_REQUEST.lock().take();
        if let Some((ssid, password)) = request {
            crate::log!("[WIFI] Connecting to '{}'...", ssid);
            match driver.connect(&ssid, &password) {
                Ok(()) => {
                    *CONNECTION_STATE.lock() = WifiState::Connecting;
                }
                Err(e) => {
                    crate::log!("[WIFI] Connect failed: {}", e);
                    *CONNECTION_STATE.lock() = WifiState::Failed;
                }
            }
        }
    }
}

pub fn request_connect(ssid: &str, password: &str) {
    if let Err(e) = ensure_started() {
        crate::log!("[WIFI] Cannot connect — start failed: {}", e);
        return;
    }
    *CONNECT_REQUEST.lock() = Some((String::from(ssid), String::from(password)));
}

pub fn disconnect() -> Result<(), &'static str> {
    let mut guard = WIFI_DRIVER.lock();
    let driver = guard.as_mut().ok_or("No WiFi driver")?;
    driver.disconnect()?;
    *CONNECTION_STATE.lock() = WifiState::Disconnected;
    *CONNECTED_SSID.lock() = None;
    Ok(())
}

pub fn set_driver(driver: Box<dyn WifiDriver>) {
    crate::log!("[WIFI] WiFi driver active: {}", driver.info().name);
    *WIFI_DRIVER.lock() = Some(driver);
    WIFI_ACTIVE.store(true, Ordering::SeqCst);
    *CONNECTION_STATE.lock() = WifiState::Disconnected;
}

pub fn probe_pci(pci_dev: &PciDevice) -> bool {
    // Debug: log every device we check
    crate::log!(
        "[WIFI-PROBE] Checking {:04X}:{:04X} class={:02X} sub={:02X} at {}.{}.{}",
        pci_dev.vendor_id,
        pci_dev.device_id,
        pci_dev.class,
        pci_dev.subclass,
        pci_dev.bus,
        pci_dev.slot,
        pci_dev.function
    );

    // Intel WiFi devices: class 0x02 (Network) subclass 0x80 (Other)
    // or class 0x0D (Wireless)
    // or Intel vendor with known WiFi device IDs
    let is_wireless = pci_dev.class == 0x0D
        || (pci_dev.class == crate::pci::class::NETWORK && pci_dev.subclass == 0x80)
        || (pci_dev.vendor_id == 0x8086
            && super::iwl4965::IWL4965_DEVICE_IDS.contains(&pci_dev.device_id));

    if !is_wireless {
        crate::log!(
            "[WIFI-PROBE] -> Not wireless (class={:02X} sub={:02X} devid={:04X})",
            pci_dev.class,
            pci_dev.subclass,
            pci_dev.device_id
        );
        return false;
    }

    crate::log!(
        "[WIFI] Found wireless device: {:04X}:{:04X} at {}.{}.{}",
        pci_dev.vendor_id,
        pci_dev.device_id,
        pci_dev.bus,
        pci_dev.slot,
        pci_dev.function
    );

    // Try Intel WiFi Link 4965AGN
    if pci_dev.vendor_id == 0x8086 {
        if let Some(driver) = super::iwl4965::probe(pci_dev) {
            set_driver(driver);
            return true;
        }
    }

    false
}
